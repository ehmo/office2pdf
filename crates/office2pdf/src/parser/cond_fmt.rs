use std::collections::HashMap;

use crate::ir::{Color, DataBarInfo, IconShading};
use crate::parser::xlsx::cond_fmt_raw::{RawCondFmtHint, RawCondFmtHints};
use crate::parser::xlsx::xlsx_style::{blend_color, pattern_ink_coverage, resolve_style_color};
use crate::parser::xlsx::{CellPos, CellRange, parse_cell_ref};
use crate::parser::xlsx_formula;
use crate::parser::xml_util;

/// A conditional formatting override for a specific cell.
#[derive(Default)]
pub(crate) struct CondFmtOverride {
    pub background: Option<Color>,
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_color: Option<Color>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub data_bar: Option<DataBarInfo>,
    pub icon_text: Option<String>,
    pub icon_color: Option<Color>,
    pub icon_shading: Option<IconShading>,
}

impl CondFmtOverride {
    fn has_cell_style(&self) -> bool {
        self.background.is_some()
            || self.font_family.is_some()
            || self.font_size.is_some()
            || self.font_color.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strikethrough.is_some()
    }

    fn apply_cell_style(&mut self, style: &Self) {
        if style.background.is_some() {
            self.background = style.background;
        }
        if style.font_family.is_some() {
            self.font_family.clone_from(&style.font_family);
        }
        if style.font_size.is_some() {
            self.font_size = style.font_size;
        }
        if style.font_color.is_some() {
            self.font_color = style.font_color;
        }
        if style.bold.is_some() {
            self.bold = style.bold;
        }
        if style.italic.is_some() {
            self.italic = style.italic;
        }
        if style.underline.is_some() {
            self.underline = style.underline;
        }
        if style.strikethrough.is_some() {
            self.strikethrough = style.strikethrough;
        }
    }
}

/// Parse an sqref string (e.g., "A1:C10" or "A1") into a list of CellRanges.
fn parse_sqref(sqref: &str) -> Vec<CellRange> {
    sqref
        .split_whitespace()
        .filter_map(|part| {
            if let Some((start_str, end_str)) = part.split_once(':') {
                let (sc, sr) = parse_cell_ref(start_str)?;
                let (ec, er) = parse_cell_ref(end_str)?;
                Some(CellRange {
                    start_col: sc,
                    start_row: sr,
                    end_col: ec,
                    end_row: er,
                })
            } else {
                let (c, r) = parse_cell_ref(part)?;
                Some(CellRange {
                    start_col: c,
                    start_row: r,
                    end_col: c,
                    end_row: r,
                })
            }
        })
        .collect()
}

use xml_util::parse_argb_color;

/// Try to get a numeric value from a cell.
fn cell_numeric_value(cell: &umya_spreadsheet::Cell) -> Option<f64> {
    let raw = cell.get_raw_value().to_string();
    if let Ok(v) = raw.parse::<f64>() {
        return Some(v);
    }
    cell.get_value().to_string().parse::<f64>().ok()
}

/// Text value of a cell, or `None` when the cell is not a string cell.
///
/// Excel keeps numbers and text as distinct comparison domains: a `cellIs` rule
/// whose operand is a quoted literal only ever matches string cells, so a
/// numeric cell holding `5` must not match the operand `"5"`.
fn cell_text_value(cell: &umya_spreadsheet::Cell) -> Option<String> {
    if cell.get_data_type() != "s" {
        return None;
    }
    let text = cell.get_value().to_string();
    if text.is_empty() {
        return None;
    }
    Some(text)
}

/// The right-hand operand of a `cellIs` rule.
///
/// Excel stores it as a formula string. A quoted literal (`"REORDER"`) selects
/// text comparison; anything else that parses as a number selects numeric
/// comparison.
enum CellIsOperand {
    Number(f64),
    Text(String),
}

/// Unquote an Excel string literal, collapsing the doubled `""` escape.
fn parse_quoted_literal(raw: &str) -> Option<String> {
    let inner: &str = raw.strip_prefix('"')?.strip_suffix('"')?;
    // A lone `"` inside would have terminated the literal early, so anything
    // that still contains an unescaped quote is not a single string literal.
    if inner.replace("\"\"", "").contains('"') {
        return None;
    }
    Some(inner.replace("\"\"", "\""))
}

/// Parse the operand of a `cellIs` rule from its formula.
fn parse_cell_is_operand(
    rule: &umya_spreadsheet::ConditionalFormattingRule,
) -> Option<CellIsOperand> {
    let raw: String = rule.get_formula()?.get_address_str();
    let trimmed: &str = raw.trim();
    if let Some(text) = parse_quoted_literal(trimmed) {
        return Some(CellIsOperand::Text(text));
    }
    trimmed.parse::<f64>().ok().map(CellIsOperand::Number)
}

/// Evaluate a CellIs conditional formatting rule against a numeric cell value.
fn evaluate_cell_is_rule(
    cell_val: f64,
    operator: &umya_spreadsheet::ConditionalFormattingOperatorValues,
    threshold: f64,
) -> bool {
    use umya_spreadsheet::ConditionalFormattingOperatorValues::*;

    match operator {
        GreaterThan => cell_val > threshold,
        GreaterThanOrEqual => cell_val >= threshold,
        LessThan => cell_val < threshold,
        LessThanOrEqual => cell_val <= threshold,
        Equal => (cell_val - threshold).abs() < f64::EPSILON,
        NotEqual => (cell_val - threshold).abs() >= f64::EPSILON,
        Between => cell_val >= threshold,
        NotBetween => cell_val < threshold,
        _ => false,
    }
}

/// Evaluate a CellIs rule against a text cell value.
///
/// Excel compares strings case-insensitively and orders them lexicographically.
fn evaluate_cell_is_text_rule(
    cell_text: &str,
    operator: &umya_spreadsheet::ConditionalFormattingOperatorValues,
    operand: &str,
) -> bool {
    use umya_spreadsheet::ConditionalFormattingOperatorValues::*;

    let lhs: String = cell_text.to_lowercase();
    let rhs: String = operand.to_lowercase();

    match operator {
        Equal => lhs == rhs,
        NotEqual => lhs != rhs,
        GreaterThan => lhs > rhs,
        GreaterThanOrEqual => lhs >= rhs,
        LessThan => lhs < rhs,
        LessThanOrEqual => lhs <= rhs,
        // Between/NotBetween need both operands; umya keeps only the last
        // formula, so the lower bound is unavailable for text ranges.
        _ => false,
    }
}

/// Extract formatting overrides from a conditional formatting rule's style.
fn extract_cond_fmt_style(
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
) -> CondFmtOverride {
    let mut result = CondFmtOverride::default();

    if let Some(style) = rule.get_style() {
        // The pattern fill is read first and directly: `Style::get_background_color`
        // hands back the `fgColor` whatever the `patternType` is, which for a
        // hatch is the ink rather than what the cell prints — the Gantt
        // legend's `lightUp` bars came out solid dark purple instead of the
        // quarter-strength lilac beside them (issues #926, #852). It stays as
        // the fallback for a style that carries a colour but no `<fill>` at
        // all, which is the shape umya builds from `set_background_color`.
        result.background = dxf_fill_color(style, theme).or_else(|| {
            style
                .get_background_color()
                .and_then(|color| resolve_style_color(color, theme))
        });
        if let Some(font) = style.get_font() {
            let family = font.get_name();
            if !family.is_empty() {
                result.font_family = Some(family.to_string());
            }
            let size = *font.get_size();
            if size.is_finite() && size > 0.0 {
                result.font_size = Some(size);
            }
            if *font.get_bold() {
                result.bold = Some(true);
            }
            if *font.get_italic() {
                result.italic = Some(true);
            }
            if !matches!(
                font.get_font_underline().get_val(),
                umya_spreadsheet::UnderlineValues::None
            ) {
                result.underline = Some(true);
            }
            if *font.get_strikethrough() {
                result.strikethrough = Some(true);
            }
            let color_argb = font.get_color().get_argb();
            if !color_argb.is_empty() && color_argb != "FF000000" {
                result.font_color = parse_argb_color(color_argb);
            } else if color_argb.is_empty() {
                result.font_color = resolve_style_color(font.get_color(), theme);
            }
        }
    }

    result
}

/// Parse an ARGB hex string from umya Color into an IR Color.
fn parse_umya_color_argb(color: &umya_spreadsheet::Color) -> Option<Color> {
    let argb = color.get_argb();
    if argb.is_empty() {
        return None;
    }
    parse_argb_color(argb)
}

/// Interpolate between two colors based on a ratio (0.0 = color_a, 1.0 = color_b).
fn interpolate_color(color_a: Color, color_b: Color, ratio: f64) -> Color {
    let ratio = ratio.clamp(0.0, 1.0);
    let r = (color_a.r as f64 + (color_b.r as f64 - color_a.r as f64) * ratio).round() as u8;
    let g = (color_a.g as f64 + (color_b.g as f64 - color_a.g as f64) * ratio).round() as u8;
    let b = (color_a.b as f64 + (color_b.b as f64 - color_a.b as f64) * ratio).round() as u8;
    Color::new(r, g, b)
}

/// Collect all numeric values in ranges from the sheet (for color scale min/max).
fn collect_numeric_values_in_ranges(
    sheet: &umya_spreadsheet::Worksheet,
    ranges: &[CellRange],
) -> Vec<f64> {
    let mut values = Vec::new();
    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                if let Some(cell) = sheet.get_cell((col, row))
                    && let Some(val) = cell_numeric_value(cell)
                {
                    values.push(val);
                }
            }
        }
    }
    values
}

/// Compute the min, max, and range span of a set of values.
/// Returns `None` if the slice is empty.
fn compute_min_max(values: &[f64]) -> Option<(f64, f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let min_val: f64 = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val: f64 = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let val_range: f64 = max_val - min_val;
    Some((min_val, max_val, val_range))
}

/// Apply a CellIs conditional formatting rule to matching cells in the given ranges.
/// Apply text-match conditional rules (containsText / notContainsText /
/// beginsWith / endsWith) using the rule's `text` attribute.
fn apply_text_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
) {
    use umya_spreadsheet::ConditionalFormatValues;
    let needle: &str = rule.get_text();
    if needle.is_empty() {
        return;
    }
    let needle_folded: String = needle.to_lowercase();
    let fmt = extract_cond_fmt_style(rule, theme);

    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                let Some(cell) = sheet.get_cell((col, row)) else {
                    continue;
                };
                let value_folded: String = cell.get_formatted_value().to_lowercase();
                let matched = match rule.get_type() {
                    ConditionalFormatValues::ContainsText => value_folded.contains(&needle_folded),
                    ConditionalFormatValues::NotContainsText => {
                        !value_folded.contains(&needle_folded)
                    }
                    ConditionalFormatValues::BeginsWith => value_folded.starts_with(&needle_folded),
                    ConditionalFormatValues::EndsWith => value_folded.ends_with(&needle_folded),
                    _ => false,
                };
                if matched {
                    let entry = overrides.entry((col, row)).or_default();
                    entry.apply_cell_style(&fmt);
                }
            }
        }
    }
}

fn apply_cell_is_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
) {
    let operator = rule.get_operator();
    let Some(operand) = parse_cell_is_operand(rule) else {
        return;
    };
    let fmt = extract_cond_fmt_style(rule, theme);

    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                let Some(cell) = sheet.get_cell((col, row)) else {
                    continue;
                };
                let matched: bool = match &operand {
                    CellIsOperand::Number(threshold) => cell_numeric_value(cell)
                        .is_some_and(|val| evaluate_cell_is_rule(val, operator, *threshold)),
                    CellIsOperand::Text(text) => cell_text_value(cell)
                        .is_some_and(|val| evaluate_cell_is_text_rule(&val, operator, text)),
                };
                if matched {
                    let entry = overrides.entry((col, row)).or_default();
                    entry.apply_cell_style(&fmt);
                }
            }
        }
    }
}

/// Apply a ColorScale conditional formatting rule to cells in the given ranges.
fn apply_color_scale_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
) {
    let Some(cs) = rule.get_color_scale() else {
        return;
    };

    let colors: Vec<Option<Color>> = cs
        .get_color_collection()
        .iter()
        .map(parse_umya_color_argb)
        .collect();

    if colors.len() < 2 {
        return;
    }

    let numeric_vals: Vec<f64> = collect_numeric_values_in_ranges(sheet, ranges);
    let Some((min_val, max_val, val_range)) = compute_min_max(&numeric_vals) else {
        return;
    };

    // Each colour stop sits on the value its `<cfvo>` names. A stop with no
    // resolvable cfvo is spaced evenly between the endpoints, which is what
    // the whole ramp used to do.
    let cfvos = cs.get_cfvo_collection();
    let last_stop: f64 = (colors.len() - 1) as f64;
    let anchors: Vec<f64> = (0..colors.len())
        .map(|index| {
            cfvos
                .get(index)
                .and_then(|cfvo| {
                    resolve_color_scale_cfvo(cfvo, min_val, max_val, val_range, &numeric_vals)
                })
                .unwrap_or(min_val + val_range * (index as f64 / last_stop))
        })
        .collect();

    let resolved_colors: Vec<Color> = colors
        .iter()
        .enumerate()
        .map(|(index, color)| {
            color.unwrap_or(if index == 0 {
                Color::white()
            } else if index + 1 == colors.len() {
                Color::black()
            } else {
                Color::new(255, 255, 0)
            })
        })
        .collect();

    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                if let Some(cell) = sheet.get_cell((col, row))
                    && let Some(val) = cell_numeric_value(cell)
                {
                    let (segment, ratio) = color_scale_position(val, &anchors);
                    let color: Color = interpolate_color(
                        resolved_colors[segment],
                        resolved_colors[segment + 1],
                        ratio,
                    );

                    let entry = overrides.entry((col, row)).or_default();
                    entry.background = Some(color);
                }
            }
        }
    }
}

/// Apply a DataBar conditional formatting rule to cells in the given ranges.
fn apply_data_bar_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
    raw_hint: Option<&RawCondFmtHint>,
) {
    let Some(db) = rule.get_data_bar() else {
        return;
    };

    let bar_color: Color = db
        .get_color_collection()
        .first()
        .and_then(parse_umya_color_argb)
        .unwrap_or(Color::new(0x63, 0x8E, 0xC6)); // default blue

    let numeric_vals: Vec<f64> = collect_numeric_values_in_ranges(sheet, ranges);
    let Some((range_min, range_max, _)) = compute_min_max(&numeric_vals) else {
        return;
    };

    // Resolve the bar axis from the cfvo pair; fixed axes (num/percent) are
    // independent of the observed values, exactly like Excel.
    let cfvos = db.get_cfvo_collection();
    let axis_min: f64 = cfvos
        .first()
        .and_then(|cfvo| resolve_data_bar_cfvo(cfvo, range_min, range_max))
        .unwrap_or(range_min);
    let axis_max: f64 = cfvos
        .get(1)
        .and_then(|cfvo| resolve_data_bar_cfvo(cfvo, range_min, range_max))
        .unwrap_or(range_max);
    let axis_range: f64 = axis_max - axis_min;

    // Excel maps the axis onto [minLength, maxLength] percent of the cell
    // width (spec defaults 10/90), so the minimum still shows a short bar.
    let min_length: f64 = f64::from(raw_hint.and_then(|hint| hint.min_length).unwrap_or(10));
    let max_length: f64 = f64::from(raw_hint.and_then(|hint| hint.max_length).unwrap_or(90));

    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                if let Some(cell) = sheet.get_cell((col, row))
                    && let Some(val) = cell_numeric_value(cell)
                {
                    let fraction: f64 = if axis_range.abs() < f64::EPSILON {
                        0.5
                    } else {
                        ((val - axis_min) / axis_range).clamp(0.0, 1.0)
                    };
                    let pct: f64 = min_length + (max_length - min_length) * fraction;
                    let entry = overrides.entry((col, row)).or_default();
                    entry.data_bar = Some(DataBarInfo {
                        color: bar_color,
                        fill_pct: pct,
                    });
                }
            }
        }
    }
}

/// Resolve one colorScale `<cfvo>` to the value its colour stop sits on.
///
/// A `percentile` stop anchors on the p-th percentile *of the data*, which is
/// only the same number as a linear position for symmetrically distributed
/// values. Reading it as a position put the middle stop of a skewed range in
/// the wrong place and shifted every colour between the endpoints (#653).
///
/// `None` for types this cannot resolve (`formula`), leaving the caller to
/// space that stop evenly.
fn resolve_color_scale_cfvo(
    cfvo: &umya_spreadsheet::ConditionalFormatValueObject,
    min_val: f64,
    max_val: f64,
    val_range: f64,
    values: &[f64],
) -> Option<f64> {
    use umya_spreadsheet::ConditionalFormatValueObjectValues as CfvoType;
    match cfvo.get_type() {
        CfvoType::Min => Some(min_val),
        CfvoType::Max => Some(max_val),
        CfvoType::Number => cfvo.get_val().parse().ok(),
        CfvoType::Percent => {
            let pct: f64 = cfvo.get_val().parse().ok()?;
            Some(min_val + val_range * (pct / 100.0))
        }
        CfvoType::Percentile => {
            let pct: f64 = cfvo.get_val().parse().ok()?;
            Some(percentile(values, pct))
        }
        _ => None,
    }
}

/// Where a value sits on a ramp whose stops are pinned to `anchors`.
///
/// Returns the index of the segment the value falls in and how far along it
/// is, so the caller interpolates between that segment's two colours. Anchors
/// are assumed non-decreasing; a zero-width segment yields its start.
fn color_scale_position(value: f64, anchors: &[f64]) -> (usize, f64) {
    let last: usize = anchors.len().saturating_sub(2);
    for index in 0..anchors.len().saturating_sub(1) {
        let (low, high) = (anchors[index], anchors[index + 1]);
        if value <= high || index == last {
            let width: f64 = high - low;
            let ratio: f64 = if width.abs() < f64::EPSILON {
                0.0
            } else {
                (value - low) / width
            };
            return (index, ratio.clamp(0.0, 1.0));
        }
    }
    (0, 0.0)
}

/// Resolve a dataBar cfvo to an absolute axis value. Returns None for types
/// that fall back to the observed range bounds (min/max/formula).
fn resolve_data_bar_cfvo(
    cfvo: &umya_spreadsheet::ConditionalFormatValueObject,
    range_min: f64,
    range_max: f64,
) -> Option<f64> {
    use umya_spreadsheet::ConditionalFormatValueObjectValues as CfvoType;
    match cfvo.get_type() {
        CfvoType::Number => cfvo.get_val().parse().ok(),
        CfvoType::Percent => {
            let pct: f64 = cfvo.get_val().parse().ok()?;
            Some(range_min + (range_max - range_min) * (pct / 100.0))
        }
        _ => None,
    }
}

/// Apply an IconSet conditional formatting rule to cells in the given ranges.
fn apply_icon_set_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
    raw_hint: Option<&RawCondFmtHint>,
) {
    let numeric_vals: Vec<f64> = collect_numeric_values_in_ranges(sheet, ranges);
    let Some((min_val, max_val, val_range)) = compute_min_max(&numeric_vals) else {
        return;
    };

    // Prefer the cfvo (type, val) pairs parsed from the raw worksheet XML:
    // umya-spreadsheet drops cfvos written as start/end tag pairs, and its
    // values do not carry the cfvo type, so treating every value as a
    // percentage misplaced the bands (issue #406).
    let cfvo_thresholds: Vec<f64> = raw_hint
        .map(|hint| hint.icon_cfvos.as_slice())
        .filter(|cfvos| !cfvos.is_empty())
        .map(|cfvos| {
            cfvos
                .iter()
                .filter_map(|(kind, raw_val)| {
                    icon_cfvo_threshold(kind, raw_val, min_val, max_val, val_range, &numeric_vals)
                })
                .collect::<Vec<f64>>()
        })
        .filter(|thresholds| thresholds.len() >= 2)
        .unwrap_or_else(|| {
            // Legacy fallback: umya's cfvos (no type) treated as percent.
            rule.get_icon_set()
                .map(|is| is.get_cfvo_collection())
                .unwrap_or(&[])
                .iter()
                .filter_map(|cfvo| {
                    let pct: f64 = cfvo.get_val().parse().ok()?;
                    Some(min_val + val_range * (pct / 100.0))
                })
                .collect()
        });

    // Default to 3-icon equal-thirds if no thresholds available
    let thresholds: Vec<f64> = if cfvo_thresholds.len() >= 2 {
        cfvo_thresholds
    } else {
        vec![
            min_val,
            min_val + val_range / 3.0,
            min_val + val_range * 2.0 / 3.0,
        ]
    };

    let set_type: &str = raw_hint
        .and_then(|hint| hint.icon_set_type.as_deref())
        .unwrap_or("");
    let icons: Vec<IconBand> = icon_set_glyphs(set_type, thresholds.len().max(3));

    for range in ranges {
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                if let Some(cell) = sheet.get_cell((col, row))
                    && let Some(val) = cell_numeric_value(cell)
                {
                    let icon_idx: usize = evaluate_icon_index(val, &thresholds, icons.len());
                    let (glyph, color, shading) = &icons[icon_idx];
                    let entry = overrides.entry((col, row)).or_default();
                    entry.icon_text = Some((*glyph).to_string());
                    entry.icon_color = *color;
                    entry.icon_shading = *shading;
                }
            }
        }
    }
}

/// Excel's icon band colors (sampled from Excel's own PDF output).
const ICON_RED: Color = Color {
    r: 214,
    g: 85,
    b: 50,
};
const ICON_YELLOW: Color = Color {
    r: 234,
    g: 191,
    b: 87,
};
/// Sampled from Excel's own export of the audited workbook: the traffic-light
/// green reads `#62C17A`, not the desaturated teal recorded before (#536).
const ICON_GREEN: Color = Color {
    r: 98,
    g: 193,
    b: 122,
};
/// The arrow sets' own fills, which are not the traffic lights'.
///
/// Excel draws each arrow as a sprite filled with a ramp under a dark outline,
/// so one flat colour can only stand in for that ramp. Measured from the Excel
/// export of `10_kpi_tracker_en`: extract the sprites, drop the near-white
/// background, then keep only ink pixels whose four neighbours are also ink —
/// that peels the outline ring, which a plain dominant-colour sample returns
/// instead of the fill. The mean of what remains is the colour whose flat area
/// matches the gradient's (issue #651).
///
/// | band | interior mean |
/// | --- | --- |
/// | down | `#E77979` |
/// | right | `#F9D06A` |
/// | up | `#59B06D` |
///
/// Since #1134 the `3Arrows` bands carry the ramp itself in the shading below,
/// and these stay beside it as the stand-in the renderer falls back to for a
/// glyph it cannot draw as a ramped shape. Scoped to `3Arrows`, the only set
/// measured. `4Arrows` and `5Arrows` keep the shared palette until there is an
/// export to measure them on.
const ARROW_ICON_RED: Color = Color {
    r: 231,
    g: 121,
    b: 121,
};
const ARROW_ICON_YELLOW: Color = Color {
    r: 249,
    g: 208,
    b: 106,
};
const ARROW_ICON_GREEN: Color = Color {
    r: 89,
    g: 176,
    b: 109,
};

/// The same three sprites read as what they are — a ramp under an outline —
/// rather than averaged into one colour.
///
/// Every interior pixel of a sprite is a function of `x + y`: the up arrow's
/// `#75C68B` recurs at (4,3) and (3,4), its `#70C487` at (5,3) and (4,4), and
/// so on down all three bitmaps. The ramp therefore runs along the box's
/// diagonal, and since the sprite's pixel is square to within 0.03% that
/// diagonal is 45 degrees. A least-squares fit per channel over the interior
/// pixels — those whose four neighbours are ink and none of them the outline,
/// so no antialiased rim pixel drags it dark — holds to under 0.75 of a level
/// on every band, which is what makes two stops the whole model.
///
/// The stops below are that fit evaluated at the ends of our own icon box.
/// A sprite pixel's centre sits at `t = (x + y + 1) / 23` along the box
/// diagonal, so `t = 0` is `x + y = -1` and `t = 1` is `x + y = 22`; the ink
/// itself only spans `x + y` of 5 to 16, well inside the fitted range, so the
/// ends are a parameterisation and not a claim about unpainted corners.
///
/// | band | outline | interior at the ends |
/// | --- | --- | --- |
/// | down | `#90271B` | `#FCD1D2` → `#F0393C` |
/// | right | `#D87103` | `#FEFCF3` → `#FEC500` |
/// | up | `#255E1B` | `#9FD8AE` → `#28A54A` |
///
/// The outline is flat across each sprite and is nothing like a darkening of
/// the interior: `darken(30%)` of the amber band gave `#AE924A`, an olive,
/// where Excel draws `#D87103` (issue #1134). Scoped to `3Arrows` with the
/// fills above, for the same reason they are.
const ARROW_ICON_RED_SHADING: IconShading = IconShading {
    fill_start: Color {
        r: 252,
        g: 209,
        b: 210,
    },
    fill_end: Color {
        r: 240,
        g: 57,
        b: 60,
    },
    outline: Color {
        r: 144,
        g: 39,
        b: 27,
    },
};
const ARROW_ICON_YELLOW_SHADING: IconShading = IconShading {
    fill_start: Color {
        r: 254,
        g: 252,
        b: 243,
    },
    fill_end: Color {
        r: 254,
        g: 197,
        b: 0,
    },
    outline: Color {
        r: 216,
        g: 113,
        b: 3,
    },
};
const ARROW_ICON_GREEN_SHADING: IconShading = IconShading {
    fill_start: Color {
        r: 159,
        g: 216,
        b: 174,
    },
    fill_end: Color {
        r: 40,
        g: 165,
        b: 74,
    },
    outline: Color {
        r: 37,
        g: 94,
        b: 27,
    },
};

const ICON_GRAY: Color = Color {
    r: 128,
    g: 128,
    b: 128,
};
const ICON_BLACK: Color = Color { r: 0, g: 0, b: 0 };

// The arrow and circle bands are recorded as these shared codepoints so the
// renderer can recognize them and draw Excel's own shapes — filled arrows
// (issue #377), filled discs (issue #536) — instead of a character. The sets
// that stay characters, flags and symbols and stars, use solid glyphs that
// keep text presentation and take the icon fill color, which the heavy "black
// arrow" codepoints do not: those resolve to color emoji.
use crate::ir::{
    ICON_ARROW_DOWN as ARROW_DOWN, ICON_ARROW_DOWN_RIGHT as ARROW_DOWN_RIGHT,
    ICON_ARROW_RIGHT as ARROW_RIGHT, ICON_ARROW_UP as ARROW_UP,
    ICON_ARROW_UP_RIGHT as ARROW_UP_RIGHT, ICON_CIRCLE as CIRCLE,
};

/// One band of an icon set: the glyph the parser records, the flat colour it
/// carries, and Excel's sprite paint for the bands read off a native export.
type IconBand = (&'static str, Option<Color>, Option<IconShading>);

/// A band the renderer fills with one flat colour.
fn flat_band(glyph: &'static str, color: Color) -> IconBand {
    (glyph, Some(color), None)
}

/// A band whose sprite was measured. The flat colour stays beside the shading
/// as the stand-in any path that cannot ramp falls back to.
fn shaded_band(glyph: &'static str, color: Color, shading: IconShading) -> IconBand {
    (glyph, Some(color), Some(shading))
}

/// A band of an unknown set type, which carries no colour of its own.
fn uncolored_band(glyph: &'static str) -> IconBand {
    (glyph, None, None)
}

/// Map an OOXML iconSet type to per-band [`IconBand`]s, low band first.
/// An absent attribute means the spec default 3TrafficLights1. Unknown set
/// types fall back to colored arrows of the requested band count.
fn icon_set_glyphs(set_type: &str, band_count: usize) -> Vec<IconBand> {
    let effective_type: &str = if set_type.is_empty() {
        "3TrafficLights1"
    } else {
        set_type
    };
    match effective_type {
        "3TrafficLights1" | "3TrafficLights2" | "3Signs" => vec![
            flat_band(CIRCLE, ICON_RED),
            flat_band(CIRCLE, ICON_YELLOW),
            flat_band(CIRCLE, ICON_GREEN),
        ],
        "4TrafficLights" => vec![
            flat_band(CIRCLE, ICON_BLACK),
            flat_band(CIRCLE, ICON_RED),
            flat_band(CIRCLE, ICON_YELLOW),
            flat_band(CIRCLE, ICON_GREEN),
        ],
        "3Symbols" | "3Symbols2" => vec![
            flat_band("✗", ICON_RED),
            flat_band("!", ICON_YELLOW),
            flat_band("✓", ICON_GREEN),
        ],
        "3Flags" => vec![
            flat_band("⚑", ICON_RED),
            flat_band("⚑", ICON_YELLOW),
            flat_band("⚑", ICON_GREEN),
        ],
        "3Arrows" => vec![
            shaded_band(ARROW_DOWN, ARROW_ICON_RED, ARROW_ICON_RED_SHADING),
            shaded_band(ARROW_RIGHT, ARROW_ICON_YELLOW, ARROW_ICON_YELLOW_SHADING),
            shaded_band(ARROW_UP, ARROW_ICON_GREEN, ARROW_ICON_GREEN_SHADING),
        ],
        "3ArrowsGray" => vec![
            flat_band(ARROW_DOWN, ICON_GRAY),
            flat_band(ARROW_RIGHT, ICON_GRAY),
            flat_band(ARROW_UP, ICON_GRAY),
        ],
        "4Arrows" => vec![
            flat_band(ARROW_DOWN, ICON_RED),
            flat_band(ARROW_DOWN_RIGHT, ICON_YELLOW),
            flat_band(ARROW_UP_RIGHT, ICON_YELLOW),
            flat_band(ARROW_UP, ICON_GREEN),
        ],
        "4ArrowsGray" => vec![
            flat_band(ARROW_DOWN, ICON_GRAY),
            flat_band(ARROW_DOWN_RIGHT, ICON_GRAY),
            flat_band(ARROW_UP_RIGHT, ICON_GRAY),
            flat_band(ARROW_UP, ICON_GRAY),
        ],
        "5Arrows" => vec![
            flat_band(ARROW_DOWN, ICON_RED),
            flat_band(ARROW_DOWN_RIGHT, ICON_YELLOW),
            flat_band(ARROW_RIGHT, ICON_YELLOW),
            flat_band(ARROW_UP_RIGHT, ICON_YELLOW),
            flat_band(ARROW_UP, ICON_GREEN),
        ],
        "5ArrowsGray" => vec![
            flat_band(ARROW_DOWN, ICON_GRAY),
            flat_band(ARROW_DOWN_RIGHT, ICON_GRAY),
            flat_band(ARROW_RIGHT, ICON_GRAY),
            flat_band(ARROW_UP_RIGHT, ICON_GRAY),
            flat_band(ARROW_UP, ICON_GRAY),
        ],
        _ => {
            if band_count >= 5 {
                vec![
                    uncolored_band(ARROW_DOWN),
                    uncolored_band(ARROW_DOWN_RIGHT),
                    uncolored_band(ARROW_RIGHT),
                    uncolored_band(ARROW_UP_RIGHT),
                    uncolored_band(ARROW_UP),
                ]
            } else {
                vec![
                    uncolored_band(ARROW_DOWN),
                    uncolored_band(ARROW_RIGHT),
                    uncolored_band(ARROW_UP),
                ]
            }
        }
    }
}

/// The colour a differential format's `<fill>` paints.
///
/// A dxf inverts the cell convention: its *solid* fill states the colour in
/// `bgColor` and leaves `fgColor` as `auto`, which is why the cond-format path
/// reads the background where `extract_cell_background` reads the foreground.
/// A hatch keeps the cell meaning — `fgColor` is the ink over a `bgColor`
/// ground — so the two composite the same way (issue #926).
///
/// Both are commonly `<… theme="N" tint="T"/>` with no ARGB of their own, so
/// they resolve against the workbook scheme; reading the bare ARGB found
/// nothing at all and every conditional fill in the Gantt template of #841 was
/// dropped (issues #853, #852).
fn dxf_fill_color(
    style: &umya_spreadsheet::Style,
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
) -> Option<Color> {
    let pattern = style.get_fill()?.get_pattern_fill()?;
    let pattern_type: &umya_spreadsheet::PatternValues = pattern.get_pattern_type();
    let background: Option<Color> = pattern
        .get_background_color()
        .and_then(|color| resolve_style_color(color, theme));
    // A dxf that states no `patternType` at all is a solid fill of its
    // `bgColor` — Excel writes a conditional format's plain fill that way, and
    // treating the absent attribute as "no fill" dropped every band and
    // highlight in the Gantt template of #841.
    let coverage: f64 = match pattern_type {
        umya_spreadsheet::PatternValues::None | umya_spreadsheet::PatternValues::Solid => 1.0,
        hatch => pattern_ink_coverage(hatch),
    };
    if coverage >= 1.0 {
        return background;
    }
    let foreground: Option<Color> = pattern
        .get_foreground_color()
        .and_then(|color| resolve_style_color(color, theme));
    match (foreground, background) {
        // `bgColor auto="1"` resolves to nothing and means the sheet's own
        // white, which is what a hatch over an unfilled cell sits on.
        (Some(ink), ground) => Some(blend_color(ground.unwrap_or(Color::white()), ink, coverage)),
        (None, ground) => ground,
    }
}

/// Apply one `cfRule type="expression"` across its ranges.
///
/// The rule's formula is evaluated once per cell, with relative references
/// rebased onto that cell from the top-left of the `sqref` — which is how
/// `H$4=period_selected` reads "the period number above me" in every column of
/// the range. A formula the evaluator does not model answers `None` and the
/// cell is left alone rather than painted on a guess (issue #852).
fn apply_expression_rule(
    sheet: &umya_spreadsheet::Worksheet,
    rule: &umya_spreadsheet::ConditionalFormattingRule,
    ranges: &[CellRange],
    defined_names: &HashMap<String, String>,
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
    raw_hint: Option<&RawCondFmtHint>,
    overrides: &mut HashMap<CellPos, CondFmtOverride>,
) {
    let Some(formula) = raw_hint.and_then(|hint| hint.formulas.first()) else {
        return;
    };
    let fmt = extract_cond_fmt_style(rule, theme);
    if !fmt.has_cell_style() {
        return;
    }
    let value_at = |column: u32, row: u32| -> xlsx_formula::Value {
        let Some(cell) = sheet.get_cell((column, row)) else {
            return xlsx_formula::Value::Blank;
        };
        if let Some(number) = cell_numeric_value(cell) {
            return xlsx_formula::Value::Number(number);
        }
        match cell_text_value(cell) {
            Some(text) => xlsx_formula::Value::Text(text),
            None => xlsx_formula::Value::Blank,
        }
    };

    for range in ranges {
        let base: (u32, u32) = (range.start_col, range.start_row);
        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                let ctx = xlsx_formula::EvalContext {
                    cell: (col, row),
                    base,
                    names: defined_names,
                    value_at: &value_at,
                };
                if !xlsx_formula::evaluate(formula, &ctx).is_some_and(|value| value.is_truthy()) {
                    continue;
                }
                let entry = overrides.entry((col, row)).or_default();
                entry.apply_cell_style(&fmt);
            }
        }
    }
}

/// Build a map of conditional formatting overrides for all cells in the sheet.
pub(crate) fn build_cond_fmt_overrides(
    sheet: &umya_spreadsheet::Worksheet,
    raw_hints: Option<&RawCondFmtHints>,
    defined_names: &HashMap<String, String>,
    theme: Option<&umya_spreadsheet::structs::drawing::Theme>,
) -> HashMap<(u32, u32), CondFmtOverride> {
    let mut overrides: HashMap<CellPos, CondFmtOverride> = HashMap::new();

    for cf in sheet.get_conditional_formatting_collection() {
        let sqref = cf.get_sequence_of_references().get_sqref();
        let ranges: Vec<CellRange> = parse_sqref(&sqref);
        if ranges.is_empty() {
            continue;
        }

        // Excel resolves overlapping rules highest priority first — the
        // smallest `priority` number — and a cell keeps the first fill that
        // matches. Applying them in document order let a low-priority band
        // paint over the bar above it (issue #852).
        let mut rules: Vec<&umya_spreadsheet::ConditionalFormattingRule> =
            cf.get_conditional_collection().iter().collect();
        rules.sort_by_key(|rule| std::cmp::Reverse(*rule.get_priority()));

        for rule in rules {
            use umya_spreadsheet::ConditionalFormatValues;
            let raw_hint = raw_hints.and_then(|hints| hints.get(rule.get_priority()));

            match rule.get_type() {
                ConditionalFormatValues::CellIs => {
                    apply_cell_is_rule(sheet, rule, &ranges, theme, &mut overrides);
                }
                ConditionalFormatValues::ContainsText
                | ConditionalFormatValues::NotContainsText
                | ConditionalFormatValues::BeginsWith
                | ConditionalFormatValues::EndsWith => {
                    apply_text_rule(sheet, rule, &ranges, theme, &mut overrides);
                }
                ConditionalFormatValues::ColorScale => {
                    apply_color_scale_rule(sheet, rule, &ranges, &mut overrides);
                }
                ConditionalFormatValues::DataBar => {
                    apply_data_bar_rule(sheet, rule, &ranges, &mut overrides, raw_hint);
                }
                ConditionalFormatValues::IconSet => {
                    apply_icon_set_rule(sheet, rule, &ranges, &mut overrides, raw_hint);
                }
                ConditionalFormatValues::Expression => {
                    apply_expression_rule(
                        sheet,
                        rule,
                        &ranges,
                        defined_names,
                        theme,
                        raw_hint,
                        &mut overrides,
                    );
                }
                _ => {}
            }
        }
    }

    overrides
}

/// Determine which icon index a value falls into based on thresholds.
/// Resolve one icon-set `<cfvo>` (type, value) into the numeric threshold a
/// cell value is compared against. Excel's cfvo types map differently:
/// `num` is a literal value, `percent` is a fraction of the value range,
/// `percentile` is the p-th percentile of the values, and `min`/`max` are
/// the extremes (issue #406). Unsupported types (e.g. `formula`) yield None.
fn icon_cfvo_threshold(
    kind: &str,
    raw_val: &str,
    min_val: f64,
    max_val: f64,
    val_range: f64,
    values: &[f64],
) -> Option<f64> {
    match kind {
        "min" => Some(min_val),
        "max" => Some(max_val),
        "num" => raw_val.parse::<f64>().ok(),
        "percent" => {
            let pct: f64 = raw_val.parse().ok()?;
            Some(min_val + val_range * (pct / 100.0))
        }
        "percentile" => {
            let pct: f64 = raw_val.parse().ok()?;
            Some(percentile(values, pct))
        }
        _ => None,
    }
}

/// The `pct`-th percentile (0-100) of `values` using linear interpolation
/// between closest ranks — Excel's `PERCENTILE.INC` convention.
fn percentile(values: &[f64], pct: f64) -> f64 {
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank: f64 = (pct / 100.0) * (sorted.len() as f64 - 1.0);
    let lower: usize = rank.floor() as usize;
    let upper: usize = rank.ceil() as usize;
    let weight: f64 = rank - lower as f64;
    sorted[lower] * (1.0 - weight) + sorted[upper] * weight
}

fn evaluate_icon_index(val: f64, thresholds: &[f64], num_icons: usize) -> usize {
    if num_icons == 0 {
        return 0;
    }
    // Iterate thresholds from highest to lowest
    for i in (1..thresholds.len()).rev() {
        if val >= thresholds[i] {
            return (i).min(num_icons - 1);
        }
    }
    0
}

#[cfg(test)]
#[path = "cond_fmt_tests.rs"]
mod tests;
