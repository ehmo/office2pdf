use super::*;

/// Build a context whose sheet is a closure over `(column, row)`.
fn context<'a>(
    cell: (u32, u32),
    names: &'a HashMap<String, String>,
    value_at: &'a dyn Fn(u32, u32) -> Value,
) -> EvalContext<'a> {
    EvalContext {
        cell,
        base: (1, 1),
        names,
        value_at,
    }
}

fn empty_names() -> HashMap<String, String> {
    HashMap::new()
}

fn blank(_column: u32, _row: u32) -> Value {
    Value::Blank
}

#[test]
fn arithmetic_and_precedence() {
    let names = empty_names();
    let ctx = context((1, 1), &names, &blank);
    assert_eq!(evaluate("1+2*3", &ctx), Some(Value::Number(7.0)));
    assert_eq!(evaluate("(1+2)*3", &ctx), Some(Value::Number(9.0)));
    assert_eq!(evaluate("-2+5", &ctx), Some(Value::Number(3.0)));
    assert_eq!(evaluate("10/4", &ctx), Some(Value::Number(2.5)));
    // A division by zero stays an error value so ISERROR can inspect it.
    assert_eq!(evaluate("1/0", &ctx), Some(Value::Error));
}

/// Excel scores a comparison as TRUE/FALSE and multiplies it as 1/0, which is
/// how every one of the Gantt template's bar rules is written (issue #852).
#[test]
fn comparisons_coerce_to_one_and_zero() {
    let names = empty_names();
    let ctx = context((1, 1), &names, &blank);
    assert_eq!(evaluate("3>2", &ctx), Some(Value::Bool(true)));
    assert_eq!(evaluate("(3>2)*(1>2)", &ctx), Some(Value::Number(0.0)));
    assert_eq!(evaluate("(3>2)*(2>1)", &ctx), Some(Value::Number(1.0)));
    assert_eq!(evaluate("2<>2", &ctx), Some(Value::Bool(false)));
    assert!(evaluate("TRUE", &ctx).is_some_and(|value| value.is_truthy()));
}

/// `MOD(COLUMN(),2)` is the zebra banding of the sheet's period columns —
/// tier one of issue #852, with no cell references at all.
#[test]
fn column_parity_alternates_across_columns() {
    let names = empty_names();
    for (column, expected) in [(8_u32, true), (9, false), (10, true)] {
        let ctx = context((column, 5), &names, &blank);
        assert_eq!(
            evaluate("MOD(COLUMN(),2)=0", &ctx)
                .expect("the parity rule evaluates")
                .is_truthy(),
            expected,
            "column {column}"
        );
    }
}

#[test]
fn functions_cover_what_the_rules_use() {
    let names = empty_names();
    let ctx = context((3, 7), &names, &blank);
    assert_eq!(evaluate("COLUMN()", &ctx), Some(Value::Number(3.0)));
    assert_eq!(evaluate("ROW()", &ctx), Some(Value::Number(7.0)));
    assert_eq!(evaluate("INT(3.9)", &ctx), Some(Value::Number(3.0)));
    assert_eq!(evaluate("INT(-3.1)", &ctx), Some(Value::Number(-4.0)));
    assert_eq!(evaluate("MOD(7,3)", &ctx), Some(Value::Number(1.0)));
    assert_eq!(evaluate("MEDIAN(5,1,9)", &ctx), Some(Value::Number(5.0)));
    assert_eq!(evaluate("MEDIAN(1,2,3,4)", &ctx), Some(Value::Number(2.5)));
    assert_eq!(evaluate("ABS(0-4)", &ctx), Some(Value::Number(4.0)));
}

#[test]
fn search_and_iserror_match_excels_case_insensitive_text_rule_pattern() {
    let names = empty_names();
    let found = |_column: u32, _row: u32| Value::Text("Needs 갱신 후보 Review".to_string());
    let missing = |_column: u32, _row: u32| Value::Text("Current".to_string());

    let found_ctx = context((5, 35), &names, &found);
    assert_eq!(
        evaluate("SEARCH(\"needs\",$E35)", &found_ctx),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        evaluate("NOT(ISERROR(SEARCH(\"갱신 후보\",$E35)))", &found_ctx),
        Some(Value::Bool(true))
    );

    let missing_ctx = context((5, 35), &names, &missing);
    assert_eq!(
        evaluate("NOT(ISERROR(SEARCH(\"갱신 후보\",$E35)))", &missing_ctx),
        Some(Value::Bool(false))
    );
}

#[test]
fn if_does_not_propagate_an_error_from_the_unselected_branch() {
    let names = empty_names();
    let ctx = context((1, 1), &names, &blank);
    assert_eq!(evaluate("IF(TRUE,1,1/0)", &ctx), Some(Value::Number(1.0)));
    assert_eq!(evaluate("IF(FALSE,1/0,2)", &ctx), Some(Value::Number(2.0)));
}

/// A relative reference is rebased onto the evaluated cell: `A$4` means "my
/// own column, row 4" and `$C1` means "column C, my own row".
#[test]
fn relative_references_rebase_onto_the_cell() {
    let names = empty_names();
    let sheet = |column: u32, row: u32| Value::Number(f64::from(column * 100 + row));
    let ctx = context((11, 9), &names, &sheet);
    assert_eq!(evaluate("A$4", &ctx), Some(Value::Number(1104.0)));
    assert_eq!(evaluate("$C1", &ctx), Some(Value::Number(309.0)));
    assert_eq!(evaluate("$H$2", &ctx), Some(Value::Number(802.0)));
}

/// A defined name expands to its own formula, and its relative references are
/// written against A1 whatever base the using formula has (issue #852).
#[test]
fn a_defined_name_expands_and_rebases_against_a1() {
    let mut names = empty_names();
    names.insert(
        "PERIODINPLAN".to_string(),
        "Sheet!A$4=MEDIAN(Sheet!A$4,Sheet!$C1,Sheet!$C1+Sheet!$D1-1)".to_string(),
    );
    // Row 5 is a task starting at period 2 for 4 periods, so periods 2..5 are
    // in plan. Column H is period 1, I is 2, and so on.
    let sheet = |column: u32, row: u32| -> Value {
        match (column, row) {
            (3, 5) => Value::Number(2.0),                     // $C5 start
            (4, 5) => Value::Number(4.0),                     // $D5 duration
            (_, 4) => Value::Number(f64::from(column) - 7.0), // row 4 period numbers
            _ => Value::Blank,
        }
    };
    let in_plan = |column: u32| -> bool {
        let ctx = context((column, 5), &names, &sheet);
        evaluate("PeriodInPlan", &ctx)
            .expect("the name resolves")
            .is_truthy()
    };
    assert!(!in_plan(8), "period 1 is before the start");
    for column in 9..=12 {
        assert!(in_plan(column), "period {} is in plan", column - 7);
    }
    assert!(!in_plan(13), "period 6 is past the end");
}

/// A name that references itself stops rather than looping.
#[test]
fn a_self_referential_name_terminates() {
    let mut names = empty_names();
    names.insert("LOOP".to_string(), "Loop".to_string());
    let ctx = context((1, 1), &names, &blank);
    assert_eq!(evaluate("Loop", &ctx), None);
}

/// Anything the evaluator does not model answers `None`, so the caller leaves
/// the cell alone instead of painting it on a guess.
#[test]
fn an_unmodelled_formula_answers_none() {
    let names = empty_names();
    let ctx = context((1, 1), &names, &blank);
    assert_eq!(evaluate("VLOOKUP(A1,B:C,2,FALSE)", &ctx), None);
    assert_eq!(evaluate("1+", &ctx), None);
    assert_eq!(
        evaluate("SUM(A1:A9)", &ctx),
        None,
        "ranges are not modelled"
    );
}

#[test]
fn preflight_distinguishes_supported_syntax_from_unknown_or_misread_forms() {
    let names = std::collections::HashMap::from([(
        "PERIOD_SELECTED".to_string(),
        "MOD(COLUMN()-1,3)+1".to_string(),
    )]);

    for formula in [
        "H$4=period_selected",
        "IF(A1>0,ABS(A1),0)",
        "AND(A1>0,B1<2)",
        "SUM()=0",
        "NOT(ISERROR(SEARCH(\"갱신 후보\",$E35)))",
    ] {
        assert!(
            supports_expression(formula, &names),
            "{formula} is inside the evaluator's exact grammar"
        );
    }
    for formula in [
        "COUNTIF(A1,1)>0",
        "SUM(A1:A3)>0",
        "COLUMN(A1)=1",
        "IF(A1)",
        "missing_name=1",
        "A1^2>4",
    ] {
        assert!(
            !supports_expression(formula, &names),
            "{formula} would otherwise fail or be evaluated with different semantics"
        );
    }

    assert!(supports_expression_on_sheet(
        "Budget!A1>0",
        &names,
        "Budget"
    ));
    assert!(!supports_expression_on_sheet(
        "Inputs!A1>0",
        &names,
        "Budget"
    ));
}

/// A blank cell is zero in arithmetic, which is what makes a task row with no
/// start date score as "not in plan" rather than as an error.
#[test]
fn a_blank_cell_reads_as_zero() {
    let names = empty_names();
    let ctx = context((8, 5), &names, &blank);
    assert_eq!(evaluate("$C1+0", &ctx), Some(Value::Number(0.0)));
    assert_eq!(evaluate("$C1>0", &ctx), Some(Value::Bool(false)));
}

/// The Gantt template's own names and sheet, verbatim from
/// `004_Gantt-prosjektplanlegger1.xlsx` (issue #852). Row 5 is `Aktivitet 01`:
/// planned start 1 for 5 periods, actual start 1 for 4, 25% complete.
fn gantt_names() -> HashMap<String, String> {
    [
        ("PERIODINPLAN", "Prosjektplanlegging!A$4=MEDIAN(Prosjektplanlegging!A$4,Prosjektplanlegging!$C1,Prosjektplanlegging!$C1+Prosjektplanlegging!$D1-1)"),
        ("PERIODINACTUAL", "Prosjektplanlegging!A$4=MEDIAN(Prosjektplanlegging!A$4,Prosjektplanlegging!$E1,Prosjektplanlegging!$E1+Prosjektplanlegging!$F1-1)"),
        ("PLAN", "PeriodInPlan*(Prosjektplanlegging!$C1>0)"),
        ("FAKTISK", "(PeriodInActual*(Prosjektplanlegging!$E1>0))*PeriodInPlan"),
        ("ACTUALBEYOND", "PeriodInActual*(Prosjektplanlegging!$E1>0)"),
        ("PERCENTCOMPLETEBEYOND", "(Prosjektplanlegging!A$4=MEDIAN(Prosjektplanlegging!A$4,Prosjektplanlegging!$E1,Prosjektplanlegging!$E1+Prosjektplanlegging!$F1)*(Prosjektplanlegging!$E1>0))*((Prosjektplanlegging!A$4<(INT(Prosjektplanlegging!$E1+Prosjektplanlegging!$F1*Prosjektplanlegging!$G1)))+(Prosjektplanlegging!A$4=Prosjektplanlegging!$E1))*(Prosjektplanlegging!$G1>0)"),
        ("PERCENTCOMPLETE", "PercentCompleteBeyond*PeriodInPlan"),
        ("PERIOD_SELECTED", "Prosjektplanlegging!$H$2"),
    ]
    .into_iter()
    .map(|(name, formula)| (name.to_string(), formula.to_string()))
    .collect()
}

/// `Aktivitet 01`: C5=1 D5=5 E5=1 F5=4 G5=0.25, period numbers on row 4 from
/// column H (period 1) rightwards, and H2 the selected period.
fn gantt_sheet(column: u32, row: u32) -> Value {
    match (column, row) {
        (8, 2) => Value::Number(1.0),  // H2, period_selected
        (3, 5) => Value::Number(1.0),  // C5 planned start
        (4, 5) => Value::Number(5.0),  // D5 planned duration
        (5, 5) => Value::Number(1.0),  // E5 actual start
        (6, 5) => Value::Number(4.0),  // F5 actual duration
        (7, 5) => Value::Number(0.25), // G5 percent complete
        (_, 4) => Value::Number(f64::from(column) - 7.0),
        _ => Value::Blank,
    }
}

/// The bar rules paint the periods each task actually spans — the whole chart
/// of the Gantt template, which ships no chart part at all (issue #852).
#[test]
fn the_gantt_bar_rules_paint_the_task_span() {
    let names = gantt_names();
    let truth = |formula: &str, column: u32| -> bool {
        let ctx = EvalContext {
            cell: (column, 5),
            base: (8, 5), // the sqref H5:BO30
            names: &names,
            value_at: &gantt_sheet,
        };
        evaluate(formula, &ctx)
            .unwrap_or_else(|| panic!("{formula} evaluates at column {column}"))
            .is_truthy()
    };

    // Planned: periods 1..5, i.e. columns H..L (8..12).
    for column in 8..=12 {
        assert!(truth("Plan", column), "planned period {}", column - 7);
    }
    assert!(!truth("Plan", 13), "period 6 is past the plan");

    // Actual: periods 1..4, and inside the plan, so `Faktisk` covers H..K.
    for column in 8..=11 {
        assert!(truth("Faktisk", column), "actual period {}", column - 7);
    }
    assert!(!truth("Faktisk", 12), "period 5 is past the actual");

    // 25% of a 4-period actual is one period, so only period 1 is complete.
    assert!(truth("PercentComplete", 8), "period 1 is complete");
    assert!(!truth("PercentComplete", 10), "period 3 is not");

    // The selected period highlight, written directly on the rule rather than
    // as a name, so its relative `H` rebases from the sqref's own corner.
    assert!(truth("H$4=period_selected", 8), "column H is period 1");
    assert!(!truth("H$4=period_selected", 9), "column I is period 2");
}
