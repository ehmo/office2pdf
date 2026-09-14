use super::*;

#[test]
fn test_table_simple_2x2() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A1")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B1")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A2")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B2")),
            ),
        ]),
    ])
    .set_grid(vec![2000, 3000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0].cells.len(), 2);
    assert_eq!(t.rows[1].cells.len(), 2);
    assert_eq!(
        t.border_paint_model,
        crate::ir::TableBorderPaintModel::WordPositiveAxisBands,
        "DOCX tables must select Word's positive-axis border bands"
    );

    let cell_text = |row: usize, col: usize| -> String {
        t.rows[row].cells[col]
            .content
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => {
                    Some(p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect::<String>()
    };
    assert_eq!(cell_text(0, 0), "A1");
    assert_eq!(cell_text(0, 1), "B1");
    assert_eq!(cell_text(1, 0), "A2");
    assert_eq!(cell_text(1, 1), "B2");
}

#[test]
fn test_table_column_widths_from_grid() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A"))),
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B"))),
    ])])
    .set_grid(vec![2000, 3000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.column_widths.len(), 2);
    assert!(
        (t.column_widths[0] - 100.0).abs() < 0.1,
        "Expected 100pt, got {}",
        t.column_widths[0]
    );
    assert!(
        (t.column_widths[1] - 150.0).abs() < 0.1,
        "Expected 150pt, got {}",
        t.column_widths[1]
    );
}

#[test]
fn test_table_column_widths_from_cell_widths_without_grid() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A")))
            .width(2000, docx_rs::WidthType::Dxa),
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B")))
            .width(3000, docx_rs::WidthType::Dxa),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.column_widths.len(), 2);
    assert!(
        (t.column_widths[0] - 100.0).abs() < 0.1,
        "Expected 100pt, got {}",
        t.column_widths[0]
    );
    assert!(
        (t.column_widths[1] - 150.0).abs() < 0.1,
        "Expected 150pt, got {}",
        t.column_widths[1]
    );
}

#[test]
fn test_table_column_widths_from_spanned_cell_widths_without_grid() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Merged")),
            )
            .grid_span(2)
            .width(4000, docx_rs::WidthType::Dxa),
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("C")))
            .width(2000, docx_rs::WidthType::Dxa),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.column_widths.len(), 3);
    assert!(
        (t.column_widths[0] - 100.0).abs() < 0.1,
        "Expected first merged column to be 100pt, got {}",
        t.column_widths[0]
    );
    assert!(
        (t.column_widths[1] - 100.0).abs() < 0.1,
        "Expected second merged column to be 100pt, got {}",
        t.column_widths[1]
    );
    assert!(
        (t.column_widths[2] - 100.0).abs() < 0.1,
        "Expected final column to be 100pt, got {}",
        t.column_widths[2]
    );
}

/// Helper for the auto-layout redistribution tests: a cell with an optional
/// `w:tcW`, an optional `w:gridSpan`, and one run in the given font/size.
/// Not cfg-gated: the degrade-path tests that run on every target use it too.
fn auto_layout_cell_xml(tcw_dxa: Option<u32>, grid_span: Option<u32>, text: &str) -> String {
    let mut tc_pr = String::new();
    if let Some(width) = tcw_dxa {
        tc_pr.push_str(&format!(r#"<w:tcW w:type="dxa" w:w="{width}"/>"#));
    }
    if let Some(span) = grid_span {
        tc_pr.push_str(&format!(r#"<w:gridSpan w:val="{span}"/>"#));
    }
    format!(
        r#"<w:tc><w:tcPr>{tc_pr}</w:tcPr><w:p><w:r><w:rPr><w:rFonts w:ascii="Libertinus Serif" w:hAnsi="Libertinus Serif"/><w:sz w:val="40"/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p></w:tc>"#
    )
}

/// Word's auto layout shrinks each over-subscribed column in proportion to
/// its compressible slack above min-content, not by a uniform scale over the
/// preferences (issue #624). Grid 100/100/100pt, `w:tblW` 300pt, but one row
/// prefers 200pt for the last column: Σpref = 400pt > 300pt. Every cell holds
/// "aa" in embedded Libertinus Serif at 20pt ('a' advance 0.457em, measured
/// with fontTools on the typst-assets face), so each column's min-content is
/// 0.914em x 20pt + 2 x 5.4pt default margins = 29.08pt, and
/// k = (300 - 87.24) / (400 - 87.24) = 0.68027.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_conflict_distributes_surplus_by_compressible_slack() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="6000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}{c}</w:tr>
                <w:tr>{a}{b}{wide}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(2000), None, "aa"),
        b = auto_layout_cell_xml(Some(2000), None, "aa"),
        c = auto_layout_cell_xml(Some(2000), None, "aa"),
        wide = auto_layout_cell_xml(Some(4000), None, "aa"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    let expected: [f64; 3] = [77.32, 77.32, 145.35];
    for (index, (width, expected_width)) in widths.iter().zip(expected).enumerate() {
        assert!(
            (width - expected_width).abs() < 0.05,
            "column {index}: expected {expected_width}pt, got {width}pt (all: {widths:?})"
        );
    }
    let total: f64 = widths.iter().sum();
    assert!(
        (total - 300.0).abs() < 0.01,
        "total must stay 300pt, got {total}"
    );
}

/// A cell that follows a `w:gridSpan` cell occupies the grid column after the
/// span, and its `w:tcW` claims THAT column; the span cell's own `w:tcW` is
/// ignored while it stays below the sum of its spanned columns' preferences.
/// Grid 50/50/200pt; the second row is a span-2 cell (tcW 25pt, ignored) plus
/// a 400pt-preference cell that must land on grid column 3. Every cell holds
/// "a" at 20pt: min-content 0.457em x 20pt + 10.8pt = 19.94pt, and
/// k = (300 - 59.82) / (500 - 59.82) = 0.54564.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_tracks_grid_occupancy_through_grid_span() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/><w:gridCol w:w="4000"/></w:tblGrid>
                <w:tr>{a}{b}{c}</w:tr>
                <w:tr>{span}{wide}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(1000), None, "a"),
        b = auto_layout_cell_xml(Some(1000), None, "a"),
        c = auto_layout_cell_xml(Some(4000), None, "a"),
        span = auto_layout_cell_xml(Some(500), Some(2), "a"),
        wide = auto_layout_cell_xml(Some(8000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    let expected: [f64; 3] = [36.34, 36.34, 227.32];
    for (index, (width, expected_width)) in widths.iter().zip(expected).enumerate() {
        assert!(
            (width - expected_width).abs() < 0.05,
            "column {index}: expected {expected_width}pt, got {width}pt (all: {widths:?})"
        );
    }
}

/// A column whose min-content exceeds its preference floors at min-content:
/// its compressible slack is zero, so redistribution cannot shrink it below
/// the widest unbreakable token. Grid 30/270pt, one row prefers 300pt for
/// column 2 (Σpref = 330 > 300). Column 1 holds "WWWW" at 20pt
/// ('W' advance 0.951em): min = 4 x 0.951em x 20pt + 10.8pt = 86.88pt > 30pt.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_floors_a_column_at_its_min_content_width() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="6000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="600"/><w:gridCol w:w="5400"/></w:tblGrid>
                <w:tr>{narrow}{wide}</w:tr>
                <w:tr>{narrow}{wider}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        narrow = auto_layout_cell_xml(Some(600), None, "WWWW"),
        wide = auto_layout_cell_xml(Some(5400), None, "a"),
        wider = auto_layout_cell_xml(Some(6000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 86.88).abs() < 0.05,
        "column 1 floors at its min-content 86.88pt, got {widths:?}"
    );
    assert!(
        (widths[1] - 213.12).abs() < 0.05,
        "column 2 absorbs the remainder, got {widths:?}"
    );
}

/// When the cell preferences agree with the fit width (Σpref == W) the grid
/// is reproduced verbatim — even when a column's min-content exceeds its
/// preference. No golden mock but the invoice reaches the redistribution
/// path, and their output must not move (issue #624).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_without_conflict_returns_grid_verbatim() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="6000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="500"/><w:gridCol w:w="5500"/></w:tblGrid>
                <w:tr>{narrow}{wide}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        narrow = auto_layout_cell_xml(Some(500), None, "WWWW"),
        wide = auto_layout_cell_xml(Some(5500), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 25.0).abs() < 0.01 && (widths[1] - 275.0).abs() < 0.01,
        "a conflict-free table keeps its declared grid, got {widths:?}"
    );
}

/// When the cells ask for LESS than the fit width the columns come from
/// `w:tblGrid`, not from the stale `w:tcW` proportions rescaled to fill it
/// (issue #925). `003_FAKTURA.docx`'s `SELGER` table states a grid of
/// 2403/2656/2997/2550 twips (120.15/132.80/149.85/127.50pt) while its cells
/// still carry 2092/2313/2610/1620 — Sigma-tcW is 431.75pt against the grid's
/// 530.30pt. Rescaling the tcW set gave the last column 99.5pt where Word
/// prints 127.5, and its `FORFALLSDATO` header spilled past the table edge.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_surplus_keeps_the_declared_grid() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2403"/><w:gridCol w:w="2656"/>
                           <w:gridCol w:w="2997"/><w:gridCol w:w="2550"/></w:tblGrid>
                <w:tr>{a}{b}{c}{d}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(2092), None, "a"),
        b = auto_layout_cell_xml(Some(2313), None, "a"),
        c = auto_layout_cell_xml(Some(2610), None, "a"),
        d = auto_layout_cell_xml(Some(1620), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    for (index, expected) in [120.15_f64, 132.80, 149.85, 127.50].iter().enumerate() {
        assert!(
            (widths[index] - expected).abs() < 0.01,
            "column {index} must come from the grid ({expected}pt), got {widths:?}"
        );
    }
}

/// Triangulation for #925: a different grid, and a `w:tblW` that is not the
/// grid total, so neither the grid verbatim nor a constant can pass. Grid
/// 100/300pt scaled to the declared 200pt fit width is 50/150pt; the tcW set
/// (100/100pt) rescaled to it would have been 100/100pt.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_surplus_scales_the_grid_to_a_narrower_tblw() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="4000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="6000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(2000), None, "a"),
        b = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 50.0).abs() < 0.01 && (widths[1] - 150.0).abs() < 0.01,
        "the grid scales to the declared 200pt fit width, got {widths:?}"
    );
}

/// When any token cannot be measured (here U+E000, which no embedded face
/// covers) the redistribution degrades to the pre-#624 uniform scale over the
/// per-column preference maxima, so font-less environments keep today's
/// output byte-identical. Grid 100/100pt, per-column max preferences
/// 150/100pt, uniform scale 200/250 = 0.8 → 120/80pt.
#[test]
fn test_auto_layout_with_unmeasurable_text_degrades_to_uniform_scale() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
                <w:tr>{c}{d}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(2000), None, "a"),
        b = auto_layout_cell_xml(Some(2000), None, "a"),
        c = auto_layout_cell_xml(Some(3000), None, "\u{E000}"),
        d = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 120.0).abs() < 0.01 && (widths[1] - 80.0).abs() < 0.01,
        "unmeasurable content must keep the uniform-scale result, got {widths:?}"
    );
}

/// `w:rFonts w:eastAsia` routes only East Asian codepoints; a Latin-only run
/// beside an unresolvable East Asian family must still measure with its Latin
/// face and reach the slack-proportional path. Grid 100/100pt, per-column
/// preferences 150/100pt against W = 200pt, "a" at 20pt in each cell:
/// min = 19.94pt, k = (200 - 39.88) / (250 - 39.88) = 0.76204.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_latin_text_ignores_unresolvable_east_asian_family() {
    let cell = |tcw: u32| -> String {
        format!(
            r#"<w:tc><w:tcPr><w:tcW w:type="dxa" w:w="{tcw}"/></w:tcPr><w:p><w:r><w:rPr><w:rFonts w:ascii="Libertinus Serif" w:hAnsi="Libertinus Serif" w:eastAsia="NoSuchFamily624"/><w:sz w:val="40"/></w:rPr><w:t xml:space="preserve">a</w:t></w:r></w:p></w:tc>"#
        )
    };
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
                <w:tr>{c}{d}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = cell(2000),
        b = cell(2000),
        c = cell(3000),
        d = cell(2000),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 119.05).abs() < 0.05 && (widths[1] - 80.95).abs() < 0.05,
        "Latin tokens must measure with the Latin face, got {widths:?}"
    );
}

/// Word breaks a line between ANY two CJK characters, so a Korean cell's
/// min-content is its widest single glyph, not the whole phrase (issue #624
/// review). Grid 100/100pt, the Korean cell states a conflicting 150pt tcW.
/// Treating the 13-syllable phrase as one unbreakable token would floor the
/// column near 250pt and overflow the 200pt fit width; per-character breaking
/// keeps the slack model close to the uniform 120/80pt split. The exact
/// widths depend on which Korean face resolves (or on the uniform-scale
/// degrade when none does), so the pin is a band, not a point.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_breaks_cjk_text_between_every_character() {
    let korean_cell: &str = r#"<w:tc><w:tcPr><w:tcW w:type="dxa" w:w="3000"/></w:tcPr><w:p><w:r><w:rPr><w:rFonts w:ascii="Libertinus Serif" w:hAnsi="Libertinus Serif" w:eastAsia="Malgun Gothic"/><w:sz w:val="40"/></w:rPr><w:t xml:space="preserve">총계약금액은일금오천만원정임</w:t></w:r></w:p></w:tc>"#;
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{korean_cell}{latin}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        latin = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    let total: f64 = widths.iter().sum();
    assert!(
        (total - 200.0).abs() < 0.01,
        "the table must not overflow its 200pt fit width, got {widths:?}"
    );
    assert!(
        (115.0..=125.0).contains(&widths[0]) && (75.0..=85.0).contains(&widths[1]),
        "a CJK cell floors at one glyph, near the 120/80 split, got {widths:?}"
    );
}

/// A `w:tblW` beyond the grid total is outside the direction verified against
/// GT (Word clamps such tables to the content width, which is not modeled),
/// so the fit target stays the grid total and the conflict still compresses:
/// grid 100/100pt, prefs 150/100pt, "a" cells at 20pt → min 19.94pt each,
/// k = (200 - 39.88) / (250 - 39.88) = 0.76204 → 119.05/80.95pt. Extrapolating
/// toward the 600pt tblW would have ballooned column 1 to 366pt.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_ignores_tblw_beyond_the_grid_total() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="12000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(3000), None, "a"),
        b = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 119.05).abs() < 0.05 && (widths[1] - 80.95).abs() < 0.05,
        "an oversized tblW must not stretch the slack model, got {widths:?}"
    );
}

/// When the preferences undershoot the fit width (Σpref < W) nothing is
/// over-subscribed, so `w:tblGrid` stands: an equal 100/100pt grid stays equal
/// however lopsided the stale `w:tcW` pair (50/100pt) on it is. This direction
/// used to keep the pre-#624 uniform scale as a placeholder because it had no
/// GT; #925 measured it on `003_FAKTURA.docx` and the grid is what Word draws.
#[test]
fn test_auto_layout_preference_undershoot_keeps_the_declared_grid() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:tblW w:type="dxa" w:w="4000"/></w:tblPr>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(1000), None, "a"),
        b = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 100.0).abs() < 0.01 && (widths[1] - 100.0).abs() < 0.01,
        "an undershoot must return the declared grid, got {widths:?}"
    );
}

/// A conflicted table whose cells are all empty makes no font measurement at
/// all, so the slack model has nothing verified to work from: it must degrade
/// to the uniform-scale result (maxima 150/100 scaled to 200pt → 120/80pt).
/// This also keeps wasm and native identical on empty form-skeleton tables —
/// margins-only minima would have produced 119.53/80.47pt on native only.
#[test]
fn test_auto_layout_all_empty_cells_keep_uniform_scale() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{a}{b}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        a = auto_layout_cell_xml(Some(3000), None, ""),
        b = auto_layout_cell_xml(Some(2000), None, ""),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 120.0).abs() < 0.01 && (widths[1] - 80.0).abs() < 0.01,
        "an all-empty table must keep the uniform-scale widths, got {widths:?}"
    );
}

/// Word does not break at no-break spaces, so "1 240,00" with a U+00A0
/// thousands separator is ONE token. Libertinus Serif at 20pt: the full
/// string advances 3.26em → min 76.0pt with margins; splitting at the NBSP
/// would have measured only "240,00" (2.545em → 61.7pt). Grid 100/100pt,
/// prefs 150/100pt: k = (200 - 95.94) / (250 - 95.94) = 0.67545 →
/// 125.98/74.02pt.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_auto_layout_no_break_space_stays_inside_a_token() {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr/>
                <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
                <w:tr>{price}{b}</w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#,
        price = auto_layout_cell_xml(Some(3000), None, "1\u{00A0}240,00"),
        b = auto_layout_cell_xml(Some(2000), None, "a"),
    );
    let data = build_docx_with_columns(&document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let widths = &first_table(&doc).column_widths;

    assert!(
        (widths[0] - 125.98).abs() < 0.05 && (widths[1] - 74.02).abs() < 0.05,
        "a no-break space must not split the token, got {widths:?}"
    );
}

#[test]
fn test_scan_table_headers_counts_only_leading_rows() {
    let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
    <w:body>
        <w:tbl>
            <w:tr>
                <w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>H1</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
                <w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>H2</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
                <w:tc><w:p><w:r><w:t>D1</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
                <w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>Ignored</w:t></w:r></w:p></w:tc>
            </w:tr>
        </w:tbl>
        <w:tbl>
            <w:tr>
                <w:tc><w:p><w:r><w:t>Only body</w:t></w:r></w:p></w:tc>
            </w:tr>
        </w:tbl>
    </w:body>
</w:document>"#;

    let headers = scan_table_headers(document_xml);

    assert_eq!(headers.len(), 2);
    assert_eq!(headers[0].repeat_rows, 2);
    assert_eq!(headers[1].repeat_rows, 0);
}

#[test]
fn test_scan_table_headers_tracks_visual_rtl_per_table() {
    let document_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl><w:tblPr><w:bidiVisual/></w:tblPr></w:tbl>
            <w:tbl><w:tblPr><w:bidiVisual w:val="0"/></w:tblPr></w:tbl>
            <w:tbl><w:tblPr/></w:tbl>
        </w:body>
    </w:document>"#;

    let tables = scan_table_headers(document_xml);

    assert_eq!(tables.len(), 3);
    assert!(tables[0].is_visual_rtl);
    assert!(!tables[1].is_visual_rtl);
    assert!(!tables[2].is_visual_rtl);
}

#[test]
fn test_visual_rtl_reverses_unequal_widths_and_preserves_colspan() {
    let document_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblPr><w:bidiVisual/></w:tblPr>
                <w:tblGrid>
                    <w:gridCol w:w="1000"/><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/>
                </w:tblGrid>
                <w:tr>
                    <w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Wide</w:t></w:r></w:p></w:tc>
                    <w:tc><w:p><w:r><w:t>Narrow</w:t></w:r></w:p></w:tc>
                </w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#;
    let data = build_docx_with_columns(document_xml);
    let (document, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let table = first_table(&document);
    let cell_text = |index: usize| -> String {
        table.rows[0].cells[index]
            .content
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(
                    paragraph
                        .runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    };

    assert_eq!(cell_text(0), "Narrow");
    assert_eq!(table.rows[0].cells[0].col_span, 1);
    assert_eq!(cell_text(1), "Wide");
    assert_eq!(table.rows[0].cells[1].col_span, 2);
    assert_eq!(table.column_widths, vec![150.0, 100.0, 50.0]);
}

#[test]
fn test_table_header_rows_from_raw_docx_xml() {
    let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
    <w:body>
        <w:tbl>
            <w:tblPr/>
            <w:tblGrid>
                <w:gridCol w:w="2000"/>
                <w:gridCol w:w="2000"/>
            </w:tblGrid>
            <w:tr>
                <w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>Header A</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>Header B</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
                <w:tc><w:p><w:r><w:t>Body A</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>Body B</w:t></w:r></w:p></w:tc>
            </w:tr>
        </w:tbl>
        <w:sectPr/>
    </w:body>
</w:document>"#;

    let data = build_docx_with_columns(document_xml);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.header_row_count, 1);
}

#[test]
fn test_table_without_cell_margins_uses_word_defaults() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Cell"))),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        t.default_cell_padding,
        Some(Insets {
            top: 0.0,
            right: 5.4,
            bottom: 0.0,
            left: 5.4,
        })
    );
}

#[test]
fn test_table_default_cell_margins_from_table_property() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Cell"))),
    ])])
    .margins(docx_rs::TableCellMargins::new().margin(40, 60, 20, 80));

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        t.default_cell_padding,
        Some(Insets {
            top: 2.0,
            right: 3.0,
            bottom: 1.0,
            left: 4.0,
        })
    );
    assert!(t.rows[0].cells[0].padding.is_none());
}

#[test]
fn test_table_cell_margins_override_table_defaults() {
    let mut cell = docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Cell")));
    cell.property = docx_rs::TableCellProperty::new()
        .margin_top(100, docx_rs::WidthType::Dxa)
        .margin_left(120, docx_rs::WidthType::Dxa);

    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![cell])])
        .margins(docx_rs::TableCellMargins::new().margin(20, 40, 60, 80));

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        t.default_cell_padding,
        Some(Insets {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        })
    );
    assert_eq!(
        t.rows[0].cells[0].padding,
        Some(Insets {
            top: 5.0,
            right: 2.0,
            bottom: 3.0,
            left: 6.0,
        })
    );
}

#[test]
fn test_table_row_uses_largest_effective_vertical_cell_margins() {
    let mut first_cell = docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("First")));
    first_cell.property = docx_rs::TableCellProperty::new()
        .margin_top(200, docx_rs::WidthType::Dxa)
        .margin_left(300, docx_rs::WidthType::Dxa);
    let second_cell = docx_rs::TableCell::new()
        .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Second")));
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![first_cell, second_cell])])
        .margins(docx_rs::TableCellMargins::new().margin(100, 400, 300, 200));

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let table = first_table(&doc);

    assert_eq!(
        table.rows[0].cells[0].padding,
        Some(Insets {
            top: 10.0,
            right: 20.0,
            bottom: 15.0,
            left: 15.0,
        })
    );
    assert_eq!(
        table.rows[0].cells[1].padding,
        Some(Insets {
            top: 10.0,
            right: 20.0,
            bottom: 15.0,
            left: 10.0,
        }),
        "Word aligns top-oriented cell content using the row's largest effective vertical margins"
    );
}

#[test]
fn test_table_cell_paragraph_uses_word_default_line_box_and_spacing() {
    let default_paragraph =
        docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Default"));
    let explicit_paragraph = docx_rs::Paragraph::new()
        .add_run(docx_rs::Run::new().add_text("Explicit"))
        .line_spacing(
            docx_rs::LineSpacing::new()
                .line_rule(docx_rs::LineSpacingType::Exact)
                .line(360)
                .after(120),
        );
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new().add_paragraph(default_paragraph),
        docx_rs::TableCell::new().add_paragraph(explicit_paragraph),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let table = first_table(&doc);
    let paragraph = |cell_index: usize| match &table.rows[0].cells[cell_index].content[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected table cell paragraph, got {other:?}"),
    };

    // Line height stays unset in the IR (issue #354); an unspecified
    // `w:spacing w:after` is zero (issue #452).
    assert_eq!(paragraph(0).style.line_box, None);
    assert_eq!(paragraph(0).style.space_after, Some(0.0));

    assert_eq!(paragraph(1).style.line_box, None);
    assert_eq!(paragraph(1).style.space_after, Some(6.0));
    assert!(matches!(
        paragraph(1).style.line_spacing,
        Some(LineSpacing::Exact(points)) if (points - 18.0).abs() < f64::EPSILON
    ));
}

#[test]
fn test_table_alignment_from_table_property() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Centered")),
        ),
    ])])
    .align(docx_rs::TableAlignmentType::Center);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.alignment, Some(Alignment::Center));
}

#[test]
fn test_table_cell_with_formatted_text() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new()
                .add_run(docx_rs::Run::new().add_text("Bold").bold())
                .add_run(docx_rs::Run::new().add_text(" and italic").italic()),
        ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell = &t.rows[0].cells[0];
    let para = match &cell.content[0] {
        Block::Paragraph(p) => p,
        _ => panic!("Expected Paragraph in cell"),
    };
    assert_eq!(para.runs.len(), 2);
    assert_eq!(para.runs[0].text, "Bold");
    assert_eq!(para.runs[0].style.bold, Some(true));
    assert_eq!(para.runs[1].text, " and italic");
    assert_eq!(para.runs[1].style.italic, Some(true));
}

#[test]
fn test_table_colspan_via_grid_span() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Merged")),
                )
                .grid_span(2),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A2")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B2")),
            ),
        ]),
    ])
    .set_grid(vec![2000, 2000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows[0].cells.len(), 1);
    assert_eq!(t.rows[0].cells[0].col_span, 2);
    assert_eq!(t.rows[1].cells.len(), 2);
    assert_eq!(t.rows[1].cells[0].col_span, 1);
}

#[test]
fn test_table_rowspan_via_vmerge() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Tall")),
                )
                .vertical_merge(docx_rs::VMergeType::Restart),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B1")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(docx_rs::Paragraph::new())
                .vertical_merge(docx_rs::VMergeType::Continue),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B2")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(docx_rs::Paragraph::new())
                .vertical_merge(docx_rs::VMergeType::Continue),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B3")),
            ),
        ]),
    ])
    .set_grid(vec![2000, 2000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows.len(), 3);
    let tall_cell = &t.rows[0].cells[0];
    assert_eq!(tall_cell.row_span, 3);
    assert_eq!(t.rows[1].cells.len(), 1);
    assert_eq!(t.rows[2].cells.len(), 1);
}

#[test]
fn test_table_exact_row_height_and_cell_vertical_align() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Centered")),
                )
                .vertical_align(docx_rs::VAlignType::Center),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Peer")),
            ),
        ])
        .row_height(360.0)
        .height_rule(docx_rs::HeightRule::Exact),
    ])
    .set_grid(vec![2000, 2000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    // `w:trHeight/@w:val` is ST_TwipsMeasure: 360 twips is 18pt.
    assert_eq!(t.rows[0].height, Some(18.0));
    assert_eq!(
        t.rows[0].cells[0].vertical_align,
        Some(CellVerticalAlign::Center)
    );
}

/// Two heights that differ by more than the 20x factor, so a parser that
/// forwards raw twips cannot satisfy both by coincidence.
#[test]
fn test_exact_row_heights_convert_twips_to_points_per_row() {
    let row = |text: &str, twips: f32| {
        docx_rs::TableRow::new(vec![docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)),
        )])
        .row_height(twips)
        .height_rule(docx_rs::HeightRule::Exact)
    };

    // 461 and 403 twips are the invoice-template header rows from issue #842.
    let table =
        docx_rs::Table::new(vec![row("Header", 461.0), row("Terms", 403.0)]).set_grid(vec![4000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows[0].height, Some(23.05));
    assert_eq!(t.rows[1].height, Some(20.15));
}

#[test]
fn test_table_cell_background_color() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Red bg")),
            )
            .shading(docx_rs::Shading::new().fill("FF0000")),
        docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("No bg")),
        ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows[0].cells[0].background, Some(Color::new(255, 0, 0)));
    assert!(t.rows[0].cells[1].background.is_none());
}

#[test]
fn test_table_cell_borders() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Bordered")),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Top)
                    .size(16)
                    .color("FF0000"),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Bottom)
                    .size(8)
                    .color("0000FF"),
            ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell = &t.rows[0].cells[0];
    let border = cell.border.as_ref().expect("Expected cell border");
    let top = border.top.as_ref().expect("Expected top border");
    assert!(
        (top.width - 2.0).abs() < 0.01,
        "Expected 2pt, got {}",
        top.width
    );
    assert_eq!(top.color, Color::new(255, 0, 0));

    let bottom = border.bottom.as_ref().expect("Expected bottom border");
    assert!(
        (bottom.width - 1.0).abs() < 0.01,
        "Expected 1pt, got {}",
        bottom.width
    );
    assert_eq!(bottom.color, Color::new(0, 0, 255));
}

#[test]
fn test_table_cell_border_styles() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Styled borders")),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Top)
                    .size(16)
                    .color("000000")
                    .border_type(docx_rs::BorderType::Dashed),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Bottom)
                    .size(8)
                    .color("0000FF")
                    .border_type(docx_rs::BorderType::Dotted),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Left)
                    .size(12)
                    .color("FF0000")
                    .border_type(docx_rs::BorderType::DotDash),
            )
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Right)
                    .size(16)
                    .color("00FF00")
                    .border_type(docx_rs::BorderType::Double),
            ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell = &t.rows[0].cells[0];
    let border = cell.border.as_ref().expect("Expected cell border");
    let top = border.top.as_ref().expect("Expected top border");
    assert_eq!(top.style, BorderLineStyle::Dashed, "Top should be dashed");

    let bottom = border.bottom.as_ref().expect("Expected bottom border");
    assert_eq!(
        bottom.style,
        BorderLineStyle::Dotted,
        "Bottom should be dotted"
    );

    let left = border.left.as_ref().expect("Expected left border");
    assert_eq!(
        left.style,
        BorderLineStyle::DashDot,
        "Left should be dashDot"
    );

    let right = border.right.as_ref().expect("Expected right border");
    assert_eq!(
        right.style,
        BorderLineStyle::Double,
        "Right should be double"
    );
}

#[test]
fn test_table_cell_solid_border_default_style() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Solid")))
            .set_border(
                docx_rs::TableCellBorder::new(docx_rs::TableCellBorderPosition::Top)
                    .size(16)
                    .color("000000"),
            ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);
    let cell = &t.rows[0].cells[0];
    let border = cell.border.as_ref().expect("Expected cell border");
    let top = border.top.as_ref().expect("Expected top border");
    assert_eq!(top.style, BorderLineStyle::Solid, "Single -> Solid");
}

#[test]
fn test_table_cell_with_multiple_paragraphs() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Para 1")),
            )
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Para 2")),
            ),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell = &t.rows[0].cells[0];
    let paras: Vec<&str> = cell
        .content
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) if !p.runs.is_empty() => Some(p.runs[0].text.as_str()),
            _ => None,
        })
        .collect();
    assert!(paras.contains(&"Para 1"), "Expected 'Para 1' in cell");
    assert!(paras.contains(&"Para 2"), "Expected 'Para 2' in cell");
}

#[test]
fn test_table_with_paragraph_before_and_after() {
    let data = {
        let docx = docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Before")),
            )
            .add_table(docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
                docx_rs::TableCell::new().add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Cell")),
                ),
            ])]))
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("After")),
            );
        let buf = Vec::new();
        let mut cursor = Cursor::new(buf);
        docx.build().pack(&mut cursor).unwrap();
        cursor.into_inner()
    };

    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let blocks = all_blocks(&doc);

    assert!(
        blocks.len() >= 3,
        "Expected at least 3 blocks, got {}",
        blocks.len()
    );
    assert!(matches!(&blocks[0], Block::Paragraph(_)));
    let has_table = blocks.iter().any(|b| matches!(b, Block::Table(_)));
    assert!(has_table, "Expected a Table block");
}

#[test]
fn test_table_colspan_and_rowspan_combined() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(
                    docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Big")),
                )
                .grid_span(2)
                .vertical_merge(docx_rs::VMergeType::Restart),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("C1")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new()
                .add_paragraph(docx_rs::Paragraph::new())
                .grid_span(2)
                .vertical_merge(docx_rs::VMergeType::Continue),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("C2")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A3")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B3")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("C3")),
            ),
        ]),
    ])
    .set_grid(vec![2000, 2000, 2000]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let big_cell = &t.rows[0].cells[0];
    assert_eq!(big_cell.col_span, 2, "Expected colspan=2");
    assert_eq!(big_cell.row_span, 2, "Expected rowspan=2");
    assert_eq!(t.rows[1].cells.len(), 1);
    assert_eq!(t.rows[2].cells.len(), 3);
}

#[test]
fn test_table_empty_cells() {
    let table = docx_rs::Table::new(vec![docx_rs::TableRow::new(vec![
        docx_rs::TableCell::new().add_paragraph(docx_rs::Paragraph::new()),
        docx_rs::TableCell::new().add_paragraph(docx_rs::Paragraph::new()),
    ])]);

    let data = build_docx_with_table(table);
    let parser = DocxParser;
    let (doc, _warnings) = parser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(t.rows.len(), 1);
    assert_eq!(t.rows[0].cells.len(), 2);
    for cell in &t.rows[0].cells {
        assert_eq!(cell.col_span, 1);
        assert_eq!(cell.row_span, 1);
    }
}

#[test]
fn test_table_level_borders_expand_to_cells() {
    // w:tblBorders (single, incl. insideH/insideV) must reach cells now that
    // the renderer no longer paints a default grid.
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A1")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B1")),
            ),
        ]),
        docx_rs::TableRow::new(vec![
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("A2")),
            ),
            docx_rs::TableCell::new().add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("B2")),
            ),
        ]),
    ])
    .set_borders(docx_rs::TableBorders::new());
    let docx = docx_rs::Docx::new().add_table(table);
    let mut cursor = Cursor::new(Vec::new());
    docx.build().pack(&mut cursor).unwrap();

    let parser = DocxParser;
    let (doc, _warnings) = parser
        .parse(&cursor.into_inner(), &ConvertOptions::default())
        .unwrap();
    let page = match &doc.pages[0] {
        Page::Flow(p) => p,
        _ => panic!("Expected FlowPage"),
    };
    let table = page
        .content
        .iter()
        .find_map(|b| match b {
            Block::Table(t) => Some(t),
            _ => None,
        })
        .expect("table");
    let first = table.rows[0].cells[0]
        .border
        .as_ref()
        .expect("cell border from tblBorders");
    assert!(first.top.is_some(), "outer top on first row");
    assert!(first.bottom.is_some(), "insideH between rows");
}

/// A content control that wraps a whole `<w:p>` inside a table cell — the
/// block-level form Word writes for a placeholder line — contributed nothing,
/// because `TableCellContent::StructuredDataTag` fell through the cell walker
/// (issue #844). The run-level form, where the `w:sdt` sits inside an existing
/// paragraph, always worked; both appear here so the fix cannot regress it.
#[test]
fn test_cell_renders_a_block_level_sdt_paragraph() {
    let document_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>
                <w:tr>
                    <w:tc>
                        <w:p><w:sdt><w:sdtPr><w:showingPlcHdr/></w:sdtPr><w:sdtContent><w:r><w:t>Gateadresse</w:t></w:r></w:sdtContent></w:sdt></w:p>
                        <w:sdt>
                            <w:sdtPr><w:showingPlcHdr/></w:sdtPr>
                            <w:sdtContent>
                                <w:p><w:r><w:t>Postnummer, poststed</w:t></w:r></w:p>
                            </w:sdtContent>
                        </w:sdt>
                        <w:p><w:r><w:t>Telefon</w:t></w:r></w:p>
                    </w:tc>
                </w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#;

    let data = build_docx_with_columns(document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell_text: Vec<String> = t.rows[0].cells[0]
        .content
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p.runs.iter().map(|r| r.text.as_str()).collect::<String>()),
            _ => None,
        })
        .collect();

    assert_eq!(
        cell_text,
        vec!["Gateadresse", "Postnummer, poststed", "Telefon"],
        "the block-level sdt's paragraph must keep its place between its siblings"
    );
}

/// A block-level `w:sdt` may hold several paragraphs and may nest, and all of
/// them belong to the cell.
#[test]
fn test_cell_renders_every_paragraph_of_a_nested_block_sdt() {
    let document_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>
            <w:tbl>
                <w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>
                <w:tr>
                    <w:tc>
                        <w:sdt>
                            <w:sdtPr/>
                            <w:sdtContent>
                                <w:p><w:r><w:t>Outer one</w:t></w:r></w:p>
                                <w:sdt>
                                    <w:sdtPr/>
                                    <w:sdtContent>
                                        <w:p><w:r><w:t>Inner</w:t></w:r></w:p>
                                    </w:sdtContent>
                                </w:sdt>
                                <w:p><w:r><w:t>Outer two</w:t></w:r></w:p>
                            </w:sdtContent>
                        </w:sdt>
                    </w:tc>
                </w:tr>
            </w:tbl>
            <w:sectPr/>
        </w:body>
    </w:document>"#;

    let data = build_docx_with_columns(document_xml);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    let cell_text: Vec<String> = t.rows[0].cells[0]
        .content
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p.runs.iter().map(|r| r.text.as_str()).collect::<String>()),
            _ => None,
        })
        .collect();

    assert_eq!(cell_text, vec!["Outer one", "Inner", "Outer two"]);
}

/// A table style's resolved run and paragraph properties beyond colour and
/// bold — `w:caps` from a conditional first row, and `w:jc` from the style's
/// own `w:pPr` — never reached the cell (issue #845).
fn build_docx_with_table_style(styles_xml_body: &str, table_xml: &str) -> Vec<u8> {
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:body>{table_xml}<w:sectPr/></w:body>
    </w:document>"#
    );
    let styles_xml = format!(
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{styles_xml_body}</w:styles>"#
    );
    build_docx_with_notes_xml(
        &document_xml,
        &styles_xml,
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
    )
}

fn first_run_of(cell: &TableCell) -> &Run {
    cell.content
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(p) => p.runs.first(),
            _ => None,
        })
        .expect("cell holds a run")
}

fn first_paragraph_of(cell: &TableCell) -> &Paragraph {
    cell.content
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .expect("cell holds a paragraph")
}

const TABLE_WITH_STYLE: &str = r#"<w:tbl>
    <w:tblPr><w:tblStyle w:val="Invoice"/><w:tblLook w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr>
    <w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>
    <w:tr><w:tc><w:p><w:r><w:t>Stilling</w:t></w:r></w:p></w:tc></w:tr>
    <w:tr><w:tc><w:p><w:r><w:t>Produkt</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>"#;

#[test]
fn test_table_style_first_row_caps_reaches_the_run() {
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:tblStylePr w:type="firstRow"><w:rPr><w:caps/></w:rPr></w:tblStylePr>
        </w:style>"#,
        TABLE_WITH_STYLE,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_run_of(&t.rows[0].cells[0]).style.all_caps,
        Some(true),
        "the conditional first row's w:caps must reach its run"
    );
    assert_ne!(
        first_run_of(&t.rows[1].cells[0]).style.all_caps,
        Some(true),
        "a row outside the conditional region must not take it"
    );
}

/// The style's own `w:pPr` applies to every row, not only the first.
#[test]
fn test_table_style_paragraph_alignment_reaches_the_cell() {
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:pPr><w:jc w:val="right"/></w:pPr>
        </w:style>"#,
        TABLE_WITH_STYLE,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_paragraph_of(&t.rows[0].cells[0]).style.alignment,
        Some(Alignment::Right)
    );
    assert_eq!(
        first_paragraph_of(&t.rows[1].cells[0]).style.alignment,
        Some(Alignment::Right)
    );
}

/// A different alignment, so the value is read rather than assumed.
#[test]
fn test_table_style_centre_alignment_is_read_as_written() {
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:pPr><w:jc w:val="center"/></w:pPr>
        </w:style>"#,
        TABLE_WITH_STYLE,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_paragraph_of(&t.rows[0].cells[0]).style.alignment,
        Some(Alignment::Center)
    );
}

/// A paragraph that declares its own `w:jc` keeps it: direct formatting wins
/// over the style.
#[test]
fn test_a_cell_paragraph_own_alignment_beats_the_table_style() {
    let table_xml = TABLE_WITH_STYLE.replace(
        r#"<w:tr><w:tc><w:p><w:r><w:t>Produkt</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:p><w:pPr><w:jc w:val="left"/></w:pPr><w:r><w:t>Produkt</w:t></w:r></w:p></w:tc></w:tr>"#,
    );
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:pPr><w:jc w:val="right"/></w:pPr>
        </w:style>"#,
        &table_xml,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_paragraph_of(&t.rows[1].cells[0]).style.alignment,
        Some(Alignment::Left)
    );
}

/// `w:tblPr/w:jc` positions the table box and must not be mistaken for the
/// style's paragraph alignment.
#[test]
fn test_a_table_level_jc_in_the_style_does_not_align_cell_text() {
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:tblPr><w:jc w:val="center"/></w:tblPr>
        </w:style>"#,
        TABLE_WITH_STYLE,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_paragraph_of(&t.rows[0].cells[0]).style.alignment,
        None
    );
}

/// A paragraph's own `w:jc="distribute"` is Word's East Asian distributed
/// justification. Dropping it to `None` let the table style overwrite it,
/// which contradicted "direct formatting wins" (issue #845 review).
#[test]
fn test_a_distributed_paragraph_keeps_its_own_alignment() {
    let table_xml = TABLE_WITH_STYLE.replace(
        r#"<w:tr><w:tc><w:p><w:r><w:t>Produkt</w:t></w:r></w:p></w:tc></w:tr>"#,
        r#"<w:tr><w:tc><w:p><w:pPr><w:jc w:val="distribute"/></w:pPr><w:r><w:t>Produkt</w:t></w:r></w:p></w:tc></w:tr>"#,
    );
    let data = build_docx_with_table_style(
        r#"<w:style w:type="table" w:styleId="Invoice"><w:name w:val="Invoice"/>
            <w:pPr><w:jc w:val="right"/></w:pPr>
        </w:style>"#,
        &table_xml,
    );
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let t = first_table(&doc);

    assert_eq!(
        first_paragraph_of(&t.rows[1].cells[0]).style.alignment,
        Some(Alignment::Justify),
        "the style's right alignment must not overwrite a declared distribute"
    );
}

/// `w:trHeight/@w:hRule` defaults to `atLeast` when the attribute is absent,
/// which makes `w:val` a floor rather than a fixed height. Reading only the
/// `exact` case dropped the floor outright, collapsing the row to its content
/// (issue #965).
#[test]
fn an_at_least_row_height_becomes_a_minimum_not_a_fixed_height() {
    for rule in [None, Some(docx_rs::HeightRule::AtLeast)] {
        let mut row = docx_rs::TableRow::new(vec![docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Short")),
        )])
        // 2215 twips is 110.75pt.
        .row_height(2215.0);
        if let Some(rule) = rule {
            row = row.height_rule(rule);
        }
        let table = docx_rs::Table::new(vec![row]).set_grid(vec![9000]);

        let data = build_docx_with_table(table);
        let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
        let parsed = first_table(&doc);

        assert_eq!(
            parsed.rows[0].height, None,
            "not a fixed height for rule {rule:?}"
        );
        assert!(
            parsed.rows[0]
                .minimum_height
                .is_some_and(|height| (height - 110.75).abs() < 0.01),
            "2215 twips is a 110.75pt floor for rule {rule:?}: {:?}",
            parsed.rows[0].minimum_height
        );
    }
}

/// `hRule="exact"` still pins the row, and states no floor on top of it.
#[test]
fn an_exact_row_height_stays_a_fixed_height() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Short")),
        )])
        .row_height(2215.0)
        .height_rule(docx_rs::HeightRule::Exact),
    ])
    .set_grid(vec![9000]);

    let data = build_docx_with_table(table);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let parsed = first_table(&doc);

    assert!(
        parsed.rows[0]
            .height
            .is_some_and(|height| (height - 110.75).abs() < 0.01),
        "{:?}",
        parsed.rows[0].height
    );
    assert_eq!(parsed.rows[0].minimum_height, None);
}

/// `hRule="auto"` states that the row is content-driven, so `w:val` carries no
/// constraint at all.
#[test]
fn an_auto_row_height_states_no_constraint() {
    let table = docx_rs::Table::new(vec![
        docx_rs::TableRow::new(vec![docx_rs::TableCell::new().add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Short")),
        )])
        .row_height(2215.0)
        .height_rule(docx_rs::HeightRule::Auto),
    ])
    .set_grid(vec![9000]);

    let data = build_docx_with_table(table);
    let (doc, _warnings) = DocxParser.parse(&data, &ConvertOptions::default()).unwrap();
    let parsed = first_table(&doc);

    assert_eq!(parsed.rows[0].height, None);
    assert_eq!(parsed.rows[0].minimum_height, None);
}
