use super::*;

pub(super) fn generate_table(
    out: &mut String,
    table: &Table,
    ctx: &mut GenCtx,
) -> Result<(), ConvertError> {
    ctx.table_depth += 1;
    // A nested table decides its cells' effective vertical alignment from its
    // own defaults; restore the enclosing table's answers afterwards, like
    // `row_has_east_asian_text` below.
    let enclosing_default_vertical_align: Option<CellVerticalAlign> =
        ctx.table_default_vertical_align;
    let enclosing_seats_on_descender: bool = ctx.table_seats_bottom_aligned_text_on_descender;
    let enclosing_box_is_aligned: bool = ctx.table_box_is_aligned;
    ctx.table_default_vertical_align = table.default_vertical_align;
    ctx.table_seats_bottom_aligned_text_on_descender = table.seats_bottom_aligned_text_on_descender;
    // `w:tblPr/w:jc` places the table box on the page and says nothing about
    // the text inside it, but Typst inherits `align` into the cells. The cells
    // undo it; a nested table's own answer must not outlive it (issue #843).
    ctx.table_box_is_aligned = matches!(
        table.alignment,
        Some(Alignment::Center) | Some(Alignment::Right)
    );
    let result = match table.alignment {
        Some(Alignment::Center) => {
            out.push_str("#align(center)[\n");
            let result = generate_table_inner(out, table, ctx);
            out.push_str("]\n");
            result
        }
        Some(Alignment::Right) => {
            out.push_str("#align(right)[\n");
            let result = generate_table_inner(out, table, ctx);
            out.push_str("]\n");
            result
        }
        _ => generate_table_inner(out, table, ctx),
    };
    ctx.table_default_vertical_align = enclosing_default_vertical_align;
    ctx.table_seats_bottom_aligned_text_on_descender = enclosing_seats_on_descender;
    ctx.table_box_is_aligned = enclosing_box_is_aligned;
    ctx.table_depth -= 1;
    result
}

fn generate_table_inner(
    out: &mut String,
    table: &Table,
    ctx: &mut GenCtx,
) -> Result<(), ConvertError> {
    out.push_str("#table(\n");

    // Only explicitly set borders render: Excel does not print gridlines,
    // and Word/PowerPoint borderless tables have none either. Typst's
    // default 1pt grid painted spurious borders on every unbordered table.
    out.push_str("  stroke: none,\n");

    if let Some(ref default_vertical_align) = table.default_vertical_align {
        let align_str: &str = match default_vertical_align {
            CellVerticalAlign::Top => "top",
            CellVerticalAlign::Center => "horizon",
            CellVerticalAlign::Bottom => "bottom",
        };
        let _ = writeln!(out, "  align: {align_str},");
    }

    if let Some(padding) = table.default_cell_padding {
        let _ = writeln!(out, "  inset: {},", format_insets(&padding));
    }

    let num_cols = if !table.column_widths.is_empty() {
        table.column_widths.len()
    } else {
        table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0)
    };

    if !table.column_widths.is_empty() {
        out.push_str("  columns: (");
        for (i, w) in table.column_widths.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "{}pt", format_f64(*w));
        }
        out.push_str("),\n");
    } else if num_cols > 1 {
        let _ = writeln!(out, "  columns: {num_cols},");
    }

    if !table.use_content_driven_row_heights && table.rows.iter().any(|row| row.height.is_some()) {
        out.push_str("  rows: (");
        for (i, row) in table.rows.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            match row.height {
                Some(height) => {
                    let _ = write!(out, "{}pt", format_f64(height));
                }
                None => out.push_str("auto"),
            }
        }
        out.push_str("),\n");
    }

    let mut rowspan_remaining = vec![0usize; num_cols];
    // The printed-headings letter strip is `rows[0]` when the XLSX parser
    // materialized it (issue #623); the print-title header counts below start
    // after it.
    let heading_strip_row_count: usize =
        usize::from(table.prints_headings && !table.rows.is_empty());
    let countable_rows: usize = table.rows.len() - heading_strip_row_count;
    let header_row_count = table.header_row_count.min(countable_rows);
    let default_cell_padding = table.default_cell_padding.unwrap_or(Insets {
        top: 5.0,
        right: 5.0,
        bottom: 5.0,
        left: 5.0,
    });

    let fixed_row_heights = !table.use_content_driven_row_heights;

    // Rows above a print-title range belong to the header block but print only
    // once, so they go in a `repeat: false` header. The repeating title rows
    // then need a higher level to keep repeating alongside it.
    let lead_row_count = table
        .non_repeating_header_row_count
        .min(countable_rows.saturating_sub(header_row_count));

    // Grid boundaries whose upper side repeats on every page while the lower
    // side prints once: the printed-headings letter strip's bottom (issue
    // #623) and the boundary between the last repeating print-title row and
    // the first body row. Border-band ties there must resolve toward the
    // repeating side (issue #619 review, remediation 2).
    let mut repeating_header_boundaries: Vec<usize> = Vec::new();
    if heading_strip_row_count > 0 && heading_strip_row_count < table.rows.len() {
        repeating_header_boundaries.push(heading_strip_row_count);
    }
    if header_row_count > 0
        && heading_strip_row_count + lead_row_count + header_row_count < table.rows.len()
    {
        repeating_header_boundaries
            .push(heading_strip_row_count + lead_row_count + header_row_count);
    }

    // Excel and Word boundary-band tables resolve which cell paints each
    // shared boundary before emission: the bands are boundary-anchored and
    // declaration-independent, so a boundary declared by both neighbours must
    // paint exactly once (issues #619 and #724). Resolved separately from
    // `TableCell::border` so the layout inset of #500/#503 keeps following
    // each cell's own declaration. Word's content-seat translation (#649)
    // follows the resolved owner later without changing that layout inset.
    let boundary_band_model: Option<TableBorderPaintModel> = match table.border_paint_model {
        TableBorderPaintModel::CenteredStroke => None,
        model => Some(model),
    };
    let painted_borders: Option<Vec<Vec<Option<CellBorder>>>> = boundary_band_model
        .map(|_| resolve_boundary_painted_borders(table, num_cols, &repeating_header_boundaries));
    if heading_strip_row_count > 0 {
        // GT prints the column-letter strip on every page (issue #623); the
        // outermost header level repeats above the print-title headers below.
        out.push_str("  table.header(repeat: true,\n");
        generate_table_rows(
            out,
            &table.rows[..heading_strip_row_count],
            painted_borders
                .as_deref()
                .map(|p| &p[..heading_strip_row_count]),
            boundary_band_model,
            &table.column_widths,
            num_cols,
            &mut rowspan_remaining,
            "    ",
            default_cell_padding,
            fixed_row_heights,
            ctx,
        )?;
        out.push_str("  ),\n");
    }

    let lead_start: usize = heading_strip_row_count;
    if lead_row_count > 0 {
        if heading_strip_row_count > 0 {
            out.push_str("  table.header(repeat: false, level: 2,\n");
        } else {
            out.push_str("  table.header(repeat: false,\n");
        }
        generate_table_rows(
            out,
            &table.rows[lead_start..lead_start + lead_row_count],
            painted_borders
                .as_deref()
                .map(|p| &p[lead_start..lead_start + lead_row_count]),
            boundary_band_model,
            &table.column_widths,
            num_cols,
            &mut rowspan_remaining,
            "    ",
            default_cell_padding,
            fixed_row_heights,
            ctx,
        )?;
        out.push_str("  ),\n");
    }

    let title_start: usize = lead_start + lead_row_count;
    if header_row_count > 0 {
        // Consecutive Typst headers need strictly increasing levels: the
        // strip (when present) takes level 1 and the lead block the next one,
        // so the print-title header lands below both.
        let title_header_level: usize =
            1 + heading_strip_row_count + usize::from(lead_row_count > 0);
        if title_header_level > 1 {
            let _ = writeln!(out, "  table.header(level: {title_header_level},");
        } else {
            out.push_str("  table.header(\n");
        }
        generate_table_rows(
            out,
            &table.rows[title_start..title_start + header_row_count],
            painted_borders
                .as_deref()
                .map(|p| &p[title_start..title_start + header_row_count]),
            boundary_band_model,
            &table.column_widths,
            num_cols,
            &mut rowspan_remaining,
            "    ",
            default_cell_padding,
            fixed_row_heights,
            ctx,
        )?;
        out.push_str("  ),\n");
    }

    generate_table_rows(
        out,
        &table.rows[title_start + header_row_count..],
        painted_borders
            .as_deref()
            .map(|p| &p[title_start + header_row_count..]),
        boundary_band_model,
        &table.column_widths,
        num_cols,
        &mut rowspan_remaining,
        "  ",
        default_cell_padding,
        fixed_row_heights,
        ctx,
    )?;

    out.push_str(")\n");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn generate_table_rows(
    out: &mut String,
    rows: &[TableRow],
    painted_borders: Option<&[Vec<Option<CellBorder>>]>,
    boundary_band_model: Option<TableBorderPaintModel>,
    // The table's declared column widths, in points, so each cell can bound
    // how wide a framed eojeol may be (issue #626). Empty when the table
    // declares none.
    column_widths: &[f64],
    num_cols: usize,
    rowspan_remaining: &mut [usize],
    indent: &str,
    default_cell_padding: Insets,
    fixed_row_heights: bool,
    ctx: &mut GenCtx,
) -> Result<(), ConvertError> {
    // A nested table decides its own rows; restore the enclosing row's answer
    // so the outer cells that follow keep sharing their baseline.
    let enclosing_row_has_east_asian_text: bool = ctx.row_has_east_asian_text;
    for (row_index, row) in rows.iter().enumerate() {
        for rs in rowspan_remaining.iter_mut() {
            if *rs > 0 {
                *rs -= 1;
            }
        }

        // Word sizes a row's lines from the whole row: if any cell holds East
        // Asian text, every cell in it takes the East Asian line height, and a
        // snapping grid applies to all of them. Asking each cell separately
        // split mixed-script rows across two baselines (issue #498).
        ctx.row_has_east_asian_text = row_has_east_asian_text(row);

        // The auto-row frame estimate walks every cell's font metrics, so
        // computing it per cell is O(cells²) on wrap-text rows; it depends
        // only on the row, so it is computed lazily at most once per row and
        // shared by every cell that needs a vertical band length (issue #619
        // review, remediation 3).
        let mut row_frame_estimate_cache: Option<Option<f64>> = None;
        let mut col_pos: usize = 0;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if cell.col_span == 0 || cell.row_span == 0 {
                continue;
            }

            while col_pos < num_cols && rowspan_remaining[col_pos] > 0 {
                col_pos += 1;
            }
            if col_pos >= num_cols {
                break;
            }

            let remaining = num_cols - col_pos;
            let clamped_colspan = (cell.col_span as usize).min(remaining).max(1) as u32;
            // `Some` selects the boundary-band regime even when this cell
            // paints nothing; `None` keeps the stroke regime.
            let boundary_band: Option<BoundaryBandCell> =
                painted_borders.map(|p| BoundaryBandCell {
                    painted_border: &p[row_index][cell_index],
                    paint_model: boundary_band_model
                        .expect("painted borders require a boundary-band model"),
                    vertical_extent: vertical_band_extent(
                        rows,
                        row_index,
                        cell,
                        fixed_row_heights,
                        default_cell_padding,
                        ctx,
                        &mut row_frame_estimate_cache,
                    ),
                });
            // A cell's own text column: the columns it spans, less the inset
            // that keeps its text off the border (issue #626).
            let enclosing_measure_pt: Option<f64> = ctx.available_measure_pt;
            if !column_widths.is_empty() {
                let inset: Insets = cell_inset_with_border(cell, default_cell_padding);
                let span_width_pt: f64 = column_widths
                    .iter()
                    .skip(col_pos)
                    .take(clamped_colspan as usize)
                    .sum();
                ctx.available_measure_pt =
                    Some(span_width_pt - inset.left - inset.right).filter(|measure| *measure > 0.0);
            }
            generate_table_cell(
                out,
                cell,
                boundary_band,
                clamped_colspan,
                indent,
                default_cell_padding,
                row.height.filter(|_| fixed_row_heights),
                // The floor belongs to the row, and a Typst table row is as
                // tall as its tallest cell, so one strut carries it. Putting
                // it in every cell would only repeat the same constraint.
                row.minimum_height.filter(|_| cell_index == 0),
                ctx,
            )?;
            ctx.available_measure_pt = enclosing_measure_pt;

            if cell.row_span > 1 {
                for rs in rowspan_remaining
                    .iter_mut()
                    .skip(col_pos)
                    .take(clamped_colspan as usize)
                {
                    *rs = cell.row_span as usize;
                }
            }
            col_pos += clamped_colspan as usize;
        }

        while col_pos < num_cols {
            if rowspan_remaining[col_pos] == 0 {
                let _ = writeln!(out, "{indent}[],");
            }
            col_pos += 1;
        }
    }
    ctx.row_has_east_asian_text = enclosing_row_has_east_asian_text;

    Ok(())
}

/// Whether any cell in the row carries East Asian text.
///
/// Nested tables are excluded: they run their own row loop and decide each of
/// their rows on their own content.
fn row_has_east_asian_text(row: &TableRow) -> bool {
    row.cells
        .iter()
        .flat_map(|cell| cell.content.iter())
        .any(block_has_east_asian_text)
}

fn block_has_east_asian_text(block: &Block) -> bool {
    match block {
        Block::Paragraph(paragraph) => paragraph
            .runs
            .iter()
            .any(|run| run.text.chars().any(is_cjk_like)),
        Block::List(list) => {
            list.items
                .iter()
                .flat_map(|item| item.content.iter())
                .any(|paragraph| {
                    paragraph
                        .runs
                        .iter()
                        .any(|run| run.text.chars().any(is_cjk_like))
                })
        }
        _ => false,
    }
}

/// Excel does not fill the cell with a data bar: it insets the bar from the
/// row's top and bottom edges. Native Excel PDF exports of the business corpus
/// print a 10 pt bar in every 14 pt row, which is 2 pt of clearance per side.
const DATA_BAR_VERTICAL_INSET_PT: f64 = 2.0;

/// Floor for rows shorter than the inset, so a bar never vanishes or inverts.
const DATA_BAR_MIN_HEIGHT_PT: f64 = 1.0;

/// Horizontal clearance between an Excel data bar and its cell boundaries.
///
/// This is independent of the text inset. Native Excel exports start the bars
/// about 2pt inside the left boundary and leave about 1pt on the right. With a
/// 3pt/side text inset, resolving the percentage against the text box made the
/// track 3pt too narrow and started it 1pt too far right (issue #655).
const DATA_BAR_LEFT_INSET_PT: f64 = 2.0;
const DATA_BAR_RIGHT_INSET_PT: f64 = 1.0;
/// Excel's arrow icon sets are drawn shapes, not characters. Native Excel PDFs
/// print them as sprites, filled with a vertical gradient and outlined a shade
/// darker; these constants size the flat vector stand-in.
///
/// Measured from the Excel export of `10_kpi_tracker_en`: the sheet places six
/// 11 x 11pt `fill_image` sprites, but that is the placement box. Extracting
/// them gives 12 x 12px bitmaps whose non-white ink spans 11 x 12px for the up
/// arrow and 12 x 11px for the right one — 10.08 x 11.00pt of actual arrow,
/// with about a pixel of padding on the narrow axis. Sizing to the 11 x 11 box
/// instead would give the ink the size of the whole sprite (issue #651).
const ARROW_ICON_LENGTH_PT: f64 = 11.0;
/// Across the shaft the arrow is narrower than it is long.
const ARROW_ICON_BREADTH_PT: f64 = 10.08;

/// Diameter of a circular icon-set icon, in points.
///
/// Measured from Excel's export of the audited workbook: 6.72pt printed at
/// that sheet's 75% scale, so 8.96pt at 100%. The `●` character it used to
/// print is a little over half that (#536).
const CIRCLE_ICON_DIAMETER_PT: f64 = 8.96;

/// The drawn shape for an icon-set glyph, or `None` for the sets that stay
/// characters — symbols, flags, stars.
fn icon_shape(glyph: &str, color: Option<Color>) -> Option<String> {
    if glyph == crate::ir::ICON_CIRCLE {
        let radius: f64 = CIRCLE_ICON_DIAMETER_PT / 2.0;
        let paint: String = color
            .map(|c| rgb(&c))
            .unwrap_or_else(|| "black".to_string());
        return Some(format!(
            "circle(radius: {}pt, fill: {paint}, stroke: none)",
            format_f64(radius)
        ));
    }
    arrow_icon_polygon(glyph, color)
}

/// Build the Typst `polygon` for one of the arrow icon-set glyphs, or `None`
/// for any other glyph.
fn arrow_icon_polygon(glyph: &str, color: Option<Color>) -> Option<String> {
    // Head half-width, shaft half-width, and where the head meets the shaft,
    // as fractions of the arrow's breadth and length.
    let breadth: f64 = ARROW_ICON_BREADTH_PT;
    let length: f64 = ARROW_ICON_LENGTH_PT;
    let shaft: f64 = breadth * 0.28;
    let neck: f64 = length * 0.45;

    // Points of an up arrow, clockwise from the tip.
    let up: Vec<(f64, f64)> = vec![
        (breadth / 2.0, 0.0),
        (breadth, neck),
        (breadth / 2.0 + shaft, neck),
        (breadth / 2.0 + shaft, length),
        (breadth / 2.0 - shaft, length),
        (breadth / 2.0 - shaft, neck),
        (0.0, neck),
    ];
    let flip_y = |points: &[(f64, f64)]| -> Vec<(f64, f64)> {
        points.iter().map(|(x, y)| (*x, length - *y)).collect()
    };
    let transpose = |points: &[(f64, f64)]| -> Vec<(f64, f64)> {
        points.iter().map(|(x, y)| (length - *y, *x)).collect()
    };

    let (points, rotation): (Vec<(f64, f64)>, Option<i32>) = match glyph {
        crate::ir::ICON_ARROW_UP => (up, None),
        crate::ir::ICON_ARROW_DOWN => (flip_y(&up), None),
        crate::ir::ICON_ARROW_RIGHT => (transpose(&up), None),
        crate::ir::ICON_ARROW_UP_RIGHT => (up, Some(45)),
        crate::ir::ICON_ARROW_DOWN_RIGHT => (flip_y(&up), Some(-45)),
        _ => return None,
    };

    let coordinates: String = points
        .iter()
        .map(|(x, y)| format!("({}pt, {}pt)", format_f64(*x), format_f64(*y)))
        .collect::<Vec<String>>()
        .join(", ");
    let paint: String = color
        .map(|c| rgb(&c))
        .unwrap_or_else(|| "black".to_string());
    let shape: String =
        format!("polygon(fill: {paint}, stroke: 0.4pt + {paint}.darken(30%), {coordinates})");
    Some(match rotation {
        Some(degrees) => format!("rotate({degrees}deg, {shape})"),
        None => shape,
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_table_cell(
    out: &mut String,
    cell: &TableCell,
    boundary_band: Option<BoundaryBandCell>,
    clamped_colspan: u32,
    indent: &str,
    default_cell_padding: Insets,
    row_height: Option<f64>,
    row_minimum_height: Option<f64>,
    ctx: &mut GenCtx,
) -> Result<(), ConvertError> {
    let needs_cell_fn = clamped_colspan > 1
        || cell.row_span > 1
        || cell.border.is_some()
        || cell.background.is_some()
        || cell.vertical_align.is_some()
        || cell.padding.is_some();

    // The alignment the cell actually renders with: its own, or the table's
    // default (Excel's bottom). The paragraph codegen needs the effective
    // answer, not the cell's declaration, because Excel's untouched default
    // cells are exactly the bottom-aligned ones (issue #618).
    let effective_vertical_align: Option<CellVerticalAlign> =
        cell.vertical_align.or(ctx.table_default_vertical_align);
    let enclosing_cell_seats_on_descender: bool = ctx.cell_seats_text_on_descender;
    // Descender seating applies only to FIXED-height rows (`row_height` is
    // `Some` only then). In auto rows the renderer sizes the row from the
    // content itself, whose intrinsic height was calibrated against Excel GT
    // (#396/#411/#498) with the symmetric box; only fixed rows have slack for
    // alignment to distribute, and only they were measured in #618.
    ctx.cell_seats_text_on_descender = ctx.table_seats_bottom_aligned_text_on_descender
        && effective_vertical_align == Some(CellVerticalAlign::Bottom)
        && row_height.is_some();

    let paints_boundary_bands: bool = boundary_band.is_some();

    if needs_cell_fn {
        out.push_str(indent);
        out.push_str("table.cell(");
        write_cell_params(
            out,
            cell,
            clamped_colspan,
            default_cell_padding,
            paints_boundary_bands,
        );
        out.push_str(")[");
    } else {
        out.push_str(indent);
        out.push('[');
    }

    // The `#align(...)` that places an aligned table's box is inherited by
    // everything inside it, so a cell paragraph that declares no alignment of
    // its own would be laid out centred or right. Reset it at the cell, where
    // a paragraph's own `#set align(...)` still nests deeper and wins
    // (issue #843).
    if ctx.table_box_is_aligned {
        out.push_str("#set align(start)\n");
    }

    if let Some(band) = &boundary_band {
        if let Some(border) = band.painted_border {
            let inset: Insets = cell_inset_with_border(cell, default_cell_padding);
            match band.paint_model {
                TableBorderPaintModel::ExcelBoundaryBands => {
                    write_boundary_anchored_border_overlays(
                        out,
                        border,
                        inset,
                        &band.vertical_extent,
                    );
                }
                TableBorderPaintModel::WordPositiveAxisBands => {
                    write_word_positive_axis_border_overlays(
                        out,
                        border,
                        inset,
                        &band.vertical_extent,
                    );
                }
                TableBorderPaintModel::CenteredStroke => {
                    unreachable!("centred strokes do not select the boundary-band path")
                }
            }
        }
    } else if let Some(border) = &cell.border {
        write_double_border_overlays(out, border, cell.padding.unwrap_or(default_cell_padding));
    }

    if let Some(ref db) = cell.data_bar {
        // Excel draws the bar behind the value on the same line (no track),
        // with a horizontal fade of the bar color; #place keeps it out of
        // layout so the value renders on top at its normal position. The bar
        // height must be concrete: in auto-height rows a relative height has
        // no cell frame to resolve against and blows up to the page height,
        // smearing over neighboring rows (issue #362).
        //
        // Where Excel's fade ends was read off its own export of
        // `06_sales_dashboard_en` rather than picked (issue #654). Sampling
        // along two bars and fitting gives a straight ramp to 0.84 of the way
        // to white, worst residual under 1.2%.
        //
        // 83 rather than 84 because the fit is of the *rendered* page, and our
        // own rendering reads a little light: the 70% this replaced measured
        // 0.706 back. 84% was tried and landed about three levels past Excel
        // at the bar's tail; 83% reproduces every sampled pixel on both bars
        // to within one level. The earlier 70% stopped short of Excel
        // altogether and left brief bars reading near-solid.
        let pct = db.fill_pct.clamp(0.0, 100.0);
        let cell_inset: Insets = cell_inset_with_border(cell, default_cell_padding);
        let (bar_dx, bar_width): (f64, String) = match ctx.available_measure_pt {
            Some(content_width) => {
                // `available_measure_pt` is the column span less the text
                // inset. Restore the full span, apply Excel's independent bar
                // inset, then quantise the painted width to whole PDF points.
                // This reproduces all 5 bars in `06_sales_dashboard_en` and
                // all 45 page-1 bars in `03_inventory_en` exactly (#655).
                let track_width = (content_width + cell_inset.left + cell_inset.right
                    - DATA_BAR_LEFT_INSET_PT
                    - DATA_BAR_RIGHT_INSET_PT)
                    .max(0.0);
                let painted_width = (track_width * pct / 100.0).round();
                (
                    DATA_BAR_LEFT_INSET_PT - cell_inset.left,
                    format!("{}pt", format_f64(painted_width)),
                )
            }
            None => (0.0, format!("{}%", format_f64(pct))),
        };
        let bar_height: String = match row_height {
            Some(height) => {
                let inset_height =
                    (height - 2.0 * DATA_BAR_VERTICAL_INSET_PT).max(DATA_BAR_MIN_HEIGHT_PT);
                format!("{}pt", format_f64(inset_height))
            }
            // Excel sizes default rows to the font's line box; 1.2em tracks
            // that for single-line numeric cells, less the same inset.
            None => format!("1.2em - {}pt", format_f64(2.0 * DATA_BAR_VERTICAL_INSET_PT)),
        };
        let _ = write!(
            out,
            "#place(left + horizon, dx: {}pt, box(width: {}, height: {}, fill: gradient.linear({}, {}.lighten(83%))))",
            format_geometry(bar_dx),
            bar_width,
            bar_height,
            rgb(&db.color),
            rgb(&db.color),
        );
    }

    if let Some(ref icon) = cell.icon_text {
        // Excel draws icon set glyphs in their band color, independent of
        // the cell's font color, anchored at the cell's left edge on the
        // value's own line. Placing the icon out of layout keeps narrow
        // cells from wrapping the value onto a second line, which doubled
        // the row height (issue #367). Because it takes no width here, the
        // cell carries `ICON_SET_VALUE_RESERVE_PT` of extra left inset so the
        // value still aligns to the icon's right, as Excel does (issue #652).
        // Excel's arrow sets are drawn shapes rather than characters: a shaft
        // with a triangular head, outlined and filling most of the row. The
        // triangle characters the parser records are only a third that size,
        // so arrows are re-drawn as polygons.
        // The circle sets are drawn discs for the same reason (#536).
        match (icon_shape(icon, cell.icon_color), cell.icon_color) {
            (Some(polygon), _) => {
                let _ = write!(out, "#place(left + horizon, {polygon})");
            }
            (None, Some(color)) => {
                let _ = write!(
                    out,
                    "#place(left + horizon, text(fill: {}, weight: \"bold\")[{}])",
                    rgb(&color),
                    icon
                );
            }
            (None, None) => {
                let _ = write!(
                    out,
                    "#place(left + horizon, text(weight: \"bold\")[{icon}])"
                );
            }
        }
    }

    // Word paints a cell boundary into the positive x/y side, but seats the
    // content from that painted edge rather than from Typst's unpainted track
    // centre. Native Word PDFs put the first glyph half a left band farther
    // in and the first baseline one top band lower. Keep that correction as a
    // visual translation: row pitch and column measure already match Word and
    // must not grow with the border (issue #649).
    let word_content_shift: Option<(f64, f64)> = word_cell_content_shift(&boundary_band);
    if let Some((dx, dy)) = word_content_shift {
        let _ = write!(
            out,
            "#move(dx: {}pt, dy: {}pt)[",
            format_geometry(dx),
            format_geometry(dy),
        );
    }

    if let Some(spill_width) = cell.spill_width {
        // An unwrapped cell keeps its text on one line: lay the content out in
        // a clipped box via #place (out of layout) and hold the row height with
        // a zero-width strut, so the line does not grow the row.
        //
        // The clip box does *not* keep the line unwrapped — a Typst box breaks
        // its content at the width it states, and this one states one. What
        // holds the line together is the inner box sized from `measure()`
        // further down; see the comment there (issue #811).
        //
        // The box is anchored where the cell's own alignment puts it. A
        // general/left cell paints rightwards across empty neighbours from its
        // left edge; a centred or right-aligned one is clipped at its own edge,
        // so anchoring it left would slide its text out of the column
        // (issue #615).
        let anchor = match cell_horizontal_alignment(cell) {
            Some(Alignment::Center) => "center",
            Some(Alignment::Right) => "right",
            _ => "left",
        };
        // `#place` ignores the table's `align:`, so the wrapper must anchor
        // where the cell's effective vertical alignment puts the line. The
        // hardcoded `horizon` centred bottom-aligned titles in tall rows
        // (issue #618). A bottom anchor needs the box and strut sized from
        // the paragraph's own line box at the run's font size. The bottom
        // anchor applies only to FIXED-height rows — auto rows are
        // content-sized against the legacy shape (see the seating gate above)
        // — and top-aligned seating is unverified against Excel GT and out of
        // #618's measured scope, so Top shares Centred's `horizon` anchor.
        let vertical_anchor: &str = match effective_vertical_align {
            Some(CellVerticalAlign::Bottom) if row_height.is_some() => "bottom",
            _ => "horizon",
        };
        // Every anchor sizes its clip box from the cell's own line. `1.3em`
        // resolves against the *ambient* text size, so a cell set larger than
        // its surroundings was clipped mid-glyph: an 18.9pt title on an 11pt
        // sheet got a 14.30pt box against the 21.74pt its glyphs span, cutting
        // every descender off flat at the baseline (issue #927). The anchor
        // itself is unchanged — #618 measured the centred position correct,
        // and only the box around it was wrong.
        let line_box_height_pt: Option<f64> = spill_line_box_height_pt(cell, ctx);
        // The clip box states a width, and a Typst box **wraps** its content
        // at the width it states. The line therefore broke into several, the
        // clip hid all but one of them, and the one left visible was the tail:
        // a merged title rendered starting mid-sentence, with its opening
        // words gone (issue #811).
        //
        // Binding the content and sizing an inner box to `measure()`'s answer
        // is what keeps it on one line — measure lays out in an unbounded
        // region, so the inner box is the text's natural width and has nothing
        // to break at. The clip then cuts that single line at the spill edge,
        // which is where Excel cuts it, and each anchor keeps the fragment
        // Excel leaves visible: the head for a left cell, the tail for a right
        // one, the middle for a centred one.
        let height: String = match line_box_height_pt {
            Some(height_pt) => format!("{}pt", format_f64(height_pt)),
            // Unknown font metrics: keep the legacy ambient-sized shape.
            None => "1.3em".to_string(),
        };
        out.push_str("#context {let o2p-spill = [");
        let enclosing_in_spill_cell = ctx.in_spill_cell;
        ctx.in_spill_cell = true;
        let spill_content = generate_cell_content(out, &cell.content, ctx);
        ctx.in_spill_cell = enclosing_in_spill_cell;
        spill_content?;
        let _ = write!(
            out,
            "]; place({anchor} + {vertical_anchor}, box(width: {}pt, height: {height}, clip: true)\
             [#box(width: measure(o2p-spill).width)[#o2p-spill]])}}#box(width: 0pt, height: {height})",
            format_f64(spill_width),
        );
    } else {
        // A `w:trHeight` floor is `max(floor, content)`, which no Typst row
        // length expresses — a stated length pins the row, and `auto` drops
        // the floor. A one-row grid beside a strut of the floor's height is
        // exactly that maximum, and it costs nothing when the content already
        // wins (issue #965).
        let strut_height_pt: Option<f64> = row_minimum_height.map(|floor| {
            let inset: Insets = cell.padding.unwrap_or(default_cell_padding);
            (floor - inset.top - inset.bottom).max(0.0)
        });
        if let Some(height) = strut_height_pt {
            let _ = write!(
                out,
                "#grid(columns: (0pt, 1fr), rows: (auto,), box(width: 0pt, height: {}pt), [",
                format_f64(height)
            );
        }
        generate_cell_content(out, &cell.content, ctx)?;
        if strut_height_pt.is_some() {
            out.push_str("])");
        }
    }
    if word_content_shift.is_some() {
        out.push(']');
    }
    ctx.cell_seats_text_on_descender = enclosing_cell_seats_on_descender;
    out.push_str("],\n");
    Ok(())
}

/// The translation from Typst's cell track to Word's positive-axis content
/// seat. Use the resolved boundary owner so a shared rule affects only the
/// following cell, exactly like the painted band itself.
fn word_cell_content_shift(boundary_band: &Option<BoundaryBandCell<'_>>) -> Option<(f64, f64)> {
    let band = boundary_band.as_ref()?;
    if band.paint_model != TableBorderPaintModel::WordPositiveAxisBands {
        return None;
    }
    let border = band.painted_border.as_ref()?;
    let dx = border
        .left
        .as_ref()
        .map_or(0.0, |side| word_pdf_border_side(side).width / 2.0);
    let dy = border
        .top
        .as_ref()
        .map_or(0.0, |side| word_pdf_border_side(side).width);
    (dx != 0.0 || dy != 0.0).then_some((dx, dy))
}

/// Height, in points, of the single line box a spill cell's paragraph emits —
/// the same metric edges the block carries, times the run's own font size.
/// `None` when the font's metrics are unknown.
fn spill_line_box_height_pt(cell: &TableCell, ctx: &GenCtx) -> Option<f64> {
    let paragraph: &Paragraph = cell.content.iter().find_map(|block| match block {
        Block::Paragraph(paragraph) => Some(paragraph),
        _ => None,
    })?;
    let line_box: CellLineBox = word_cell_line_box(
        &paragraph.runs,
        &paragraph.style,
        ctx.line_grid_pitch,
        ctx.row_has_east_asian_text,
        ctx.cell_seats_text_on_descender,
    )?;
    Some((line_box.top_em + line_box.bottom_em) * line_box.font_size_pt)
}

/// The horizontal alignment a cell's own paragraph declares, if any.
fn cell_horizontal_alignment(cell: &TableCell) -> Option<Alignment> {
    cell.content.iter().find_map(|block| match block {
        Block::Paragraph(paragraph) => paragraph.style.alignment,
        _ => None,
    })
}

fn write_double_border_overlays(out: &mut String, border: &CellBorder, padding: Insets) {
    if let Some(side) = border
        .top
        .as_ref()
        .filter(|side| side.style == BorderLineStyle::Double)
    {
        write_horizontal_double_border(out, side, padding, true);
    }
    if let Some(side) = border
        .bottom
        .as_ref()
        .filter(|side| side.style == BorderLineStyle::Double)
    {
        write_horizontal_double_border(out, side, padding, false);
    }
    if let Some(side) = border
        .left
        .as_ref()
        .filter(|side| side.style == BorderLineStyle::Double)
    {
        write_vertical_double_border(out, side, padding, true);
    }
    if let Some(side) = border
        .right
        .as_ref()
        .filter(|side| side.style == BorderLineStyle::Double)
    {
        write_vertical_double_border(out, side, padding, false);
    }
}

fn write_horizontal_double_border(
    out: &mut String,
    side: &BorderSide,
    padding: Insets,
    is_top: bool,
) {
    let align = if is_top {
        "top + left"
    } else {
        "bottom + left"
    };
    let first_dy = if is_top {
        -padding.top - side.width
    } else {
        padding.bottom - side.width
    };
    let second_dy = if is_top {
        -padding.top + side.width
    } else {
        padding.bottom + side.width
    };
    let dx = -padding.left;
    let length_extra = padding.left + padding.right;
    write_double_border_line(out, align, dx, first_dy, "0deg", length_extra, side);
    write_double_border_line(out, align, dx, second_dy, "0deg", length_extra, side);
}

fn write_vertical_double_border(
    out: &mut String,
    side: &BorderSide,
    padding: Insets,
    is_left: bool,
) {
    let align = if is_left { "top + left" } else { "top + right" };
    let first_dx = if is_left {
        -padding.left - side.width
    } else {
        padding.right - side.width
    };
    let second_dx = if is_left {
        -padding.left + side.width
    } else {
        padding.right + side.width
    };
    let dy = -padding.top;
    let length_extra = padding.top + padding.bottom;
    write_double_border_line(out, align, first_dx, dy, "90deg", length_extra, side);
    write_double_border_line(out, align, second_dx, dy, "90deg", length_extra, side);
}

fn write_double_border_line(
    out: &mut String,
    align: &str,
    dx: f64,
    dy: f64,
    angle: &str,
    length_extra: f64,
    side: &BorderSide,
) {
    let _ = write!(
        out,
        "#place({align}, dx: {}pt, dy: {}pt, line(length: 100% + {}pt, angle: {angle}, stroke: {}pt + {}))",
        format_geometry(dx),
        format_geometry(dy),
        format_geometry(length_extra),
        format_geometry(side.width),
        rgb(&side.color),
    );
}

pub(super) fn format_geometry(value: f64) -> String {
    let rounded = (value * 1_000.0).round() / 1_000.0;
    format_f64(if rounded == -0.0 { 0.0 } else { rounded })
}

/// The cell's inset, with the layout space its horizontal borders occupy.
///
/// Typst draws our per-cell strokes without reserving room for them, but Word
/// counts a border's width in the row height. Each horizontal border is shared
/// between the rows above and below it, so each cell takes half (issues #500,
/// #503).
fn cell_inset_with_border(cell: &TableCell, default_cell_padding: Insets) -> Insets {
    let padding: Insets = cell.padding.unwrap_or(default_cell_padding);
    let Some(border) = &cell.border else {
        return padding;
    };
    let half = |side: &Option<BorderSide>| side.as_ref().map_or(0.0, |s| s.width / 2.0);
    Insets {
        top: padding.top + half(&border.top),
        bottom: padding.bottom + half(&border.bottom),
        ..padding
    }
}

/// Excel extends every border band 1pt past its end boundary — the
/// `[A_start, A_end + 1]` run rule, measured independent of weight (issue
/// #619). It is what lets horizontal bands own the corner blocks.
const BAND_RUN_END_EXTENSION_PT: f64 = 1.0;

/// Width of Excel's printed gridline band, in points.
///
/// Measured on native Excel exports of NumberFormatTests (issue #622,
/// /Volumes/T7/scratch/issue-622/nft2-p1.rects.txt and nft2-p2.trace): every
/// gridline is an axis-aligned fill rect exactly 1.0pt thick filling the
/// boundary band [B, B+1] — no stroke ops and no fractional hairlines exist
/// anywhere in the traces.
const PRINTED_GRIDLINE_WIDTH_PT: f64 = 1.0;

/// The side a printed gridline paints on an unowned boundary.
///
/// Pure black, not gray and not a theme colour: the GT traces fill every
/// gridline with "0 0 0" in ICCBased sRGB (issue #622 measurement — the
/// common assumption of gray printed gridlines is wrong for Excel GT).
fn printed_gridline_side() -> BorderSide {
    BorderSide {
        width: PRINTED_GRIDLINE_WIDTH_PT,
        color: Color::black(),
        style: BorderLineStyle::Solid,
    }
}

/// One edge of the black 1pt print frame that `<printOptions headings="1"/>`
/// draws around the heading bands and the data grid (issue #623).
///
/// GT (nft-sheet-0002 trace): the frame is four 1pt pure-black fill bands on
/// the table's exterior boundaries — [54,538]x[72,73] top, [54,55]x[72,710]
/// left, [537,538]x[72,710] right, [54,538]x[709,710] bottom — each on the
/// same [B, B+1] band convention as the #619/#622 rules, and everything else
/// on the page is clipped to the frame's interior.
fn print_heading_frame_side() -> BorderSide {
    BorderSide {
        width: 1.0,
        color: Color::black(),
        style: BorderLineStyle::Solid,
    }
}

/// Total order for Excel's shared-boundary conflict rule (issue #619 review,
/// remediation 1). Derived `PartialOrd` compares the fields lexicographically
/// in declaration order.
///
/// Heaviness is a style precedence, not the stored stroke width: Excel paints
/// a double rule on top of every single band — including thick — even though
/// each of a double's two strokes is stored at the thin 1pt weight (Excel's
/// double-on-top conflict behaviour). Below double, the total painted band
/// width decides (thick 3 > medium 2 > thin/hair 1), and at equal width a
/// solid rule beats a patterned one (hair/dotted/dashed). Exact rank ties
/// fall back to the caller's positional rule — the lower/right cell's
/// top/left slot keeps the boundary — which never consults colour, so
/// ownership is colour-stable: declarations differing only in colour resolve
/// the same way regardless of which side declares which colour.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct BoundaryConflictRank {
    /// Excel's double-on-top rule: any double outranks any single band.
    is_double: bool,
    /// Total painted band width in points (thick 3 > medium 2 > thin 1).
    band_width: f64,
    /// Solid beats patterned at equal band width.
    is_solid: bool,
}

/// Rank one declared side for shared-boundary conflict resolution.
fn boundary_conflict_rank(side: &BorderSide) -> BoundaryConflictRank {
    BoundaryConflictRank {
        is_double: side.style == BorderLineStyle::Double,
        band_width: side.width,
        is_solid: side.style == BorderLineStyle::Solid,
    }
}

/// Which border sides each cell paints in the boundary-band regime, parallel
/// to `table.rows[r].cells[c]`.
///
/// A printed border belongs to the grid *boundary*, not to the declaring
/// cell: the band is anchored to the boundary whichever neighbour declares
/// it, so a boundary declared by both neighbours must paint exactly once.
/// Conflicting declarations resolve to the heavier style per
/// [`BoundaryConflictRank`]. Each internal boundary is therefore left on the
/// highest-ranked declaration; equal declarations use the lower/right cell's
/// top/left slot. That slot paints toward positive x/y for both Excel's
/// measured bands and Word's filled rectangles.
///
/// Suppression is whole-side: when merged cells overlap a boundary only
/// partially, both declarations are kept and the equal bands overlap
/// invisibly. Partial overlaps of *differing* weight would need per-track
/// resolution (known limitation, deliberately skipped).
///
/// Each of the `repeating_header_boundaries` is a grid boundary whose upper
/// side lives in a repeating header block (the #623 letter strip's bottom,
/// the last print-title row's bottom) while its lower side prints once. The
/// lower row renders once but the header repeats on every page, so a band
/// left on the lower side would vanish under the repeated header on pages
/// 2+. At those boundaries ties therefore go to the *header's* declaration,
/// and a strictly heavier lower declaration is additionally adopted into the
/// header cell's bottom slot: both sides then paint the same
/// boundary-anchored band — coincident and invisible where they overlap on
/// page 1, while the header's copy repeats with it.
///
/// A sheet that prints headings is the second exception, and a broader one:
/// there "exactly one declaration" does not hold at any ordinary horizontal
/// boundary, because a band left on one side alone closes only one side of a
/// page break codegen cannot see. Both are kept. See
/// `keeps_coincident_horizontal_bands` in the body (issue #722).
pub(super) fn resolve_boundary_painted_borders(
    table: &Table,
    num_cols: usize,
    repeating_header_boundaries: &[usize],
) -> Vec<Vec<Option<CellBorder>>> {
    use std::collections::{HashMap, HashSet};

    /// Grid footprint of one emitted cell.
    struct CellPlacement {
        row_index: usize,
        cell_index: usize,
        first_col: usize,
        row_span: usize,
        col_span: usize,
    }

    // Mirror the emission walk in `generate_table_rows` exactly, so the
    // painted set stays parallel to what actually renders.
    let mut placements: Vec<CellPlacement> = Vec::new();
    let mut rowspan_remaining: Vec<usize> = vec![0usize; num_cols];
    for (row_index, row) in table.rows.iter().enumerate() {
        for rs in rowspan_remaining.iter_mut() {
            if *rs > 0 {
                *rs -= 1;
            }
        }
        let mut col_pos: usize = 0;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            if cell.col_span == 0 || cell.row_span == 0 {
                continue;
            }
            while col_pos < num_cols && rowspan_remaining[col_pos] > 0 {
                col_pos += 1;
            }
            if col_pos >= num_cols {
                break;
            }
            let remaining: usize = num_cols - col_pos;
            let col_span: usize = (cell.col_span as usize).min(remaining).max(1);
            placements.push(CellPlacement {
                row_index,
                cell_index,
                first_col: col_pos,
                row_span: (cell.row_span as usize).max(1),
                col_span,
            });
            if cell.row_span > 1 {
                for rs in rowspan_remaining.iter_mut().skip(col_pos).take(col_span) {
                    *rs = cell.row_span as usize;
                }
            }
            col_pos += col_span;
        }
    }

    let cell_of = |placement: &CellPlacement| -> &TableCell {
        &table.rows[placement.row_index].cells[placement.cell_index]
    };

    // Declared sides per (grid boundary index, crossing track). A horizontal
    // boundary `b` separates grid rows `b-1` and `b`; its declarations are
    // the bottoms of cells ending at `b` and the tops of cells starting
    // there. Vertical boundaries likewise with columns. Whole sides are kept
    // (not just widths) because ranking needs the style and the header/body
    // boundary below adopts the winning side wholesale.
    let mut bottom_sides: HashMap<(usize, usize), BorderSide> = HashMap::new();
    let mut top_sides: HashMap<(usize, usize), BorderSide> = HashMap::new();
    let mut right_sides: HashMap<(usize, usize), BorderSide> = HashMap::new();
    let mut left_sides: HashMap<(usize, usize), BorderSide> = HashMap::new();
    for placement in &placements {
        let Some(border) = &cell_of(placement).border else {
            continue;
        };
        let column_tracks = placement.first_col..placement.first_col + placement.col_span;
        let row_tracks = placement.row_index..placement.row_index + placement.row_span;
        if let Some(side) = &border.bottom {
            for col in column_tracks.clone() {
                bottom_sides.insert(
                    (placement.row_index + placement.row_span, col),
                    side.clone(),
                );
            }
        }
        if let Some(side) = &border.top {
            for col in column_tracks {
                top_sides.insert((placement.row_index, col), side.clone());
            }
        }
        if let Some(side) = &border.right {
            for row in row_tracks.clone() {
                right_sides.insert(
                    (placement.first_col + placement.col_span, row),
                    side.clone(),
                );
            }
        }
        if let Some(side) = &border.left {
            for row in row_tracks {
                left_sides.insert((placement.first_col, row), side.clone());
            }
        }
    }

    // Whether a horizontal boundary keeps *both* its declarations instead of
    // resolving them down to one.
    //
    // Codegen cannot see the page breaks Typst chooses, so a boundary painted
    // by only one of its two owners is closed on only one side of a break. At
    // a tie the rule below hands the boundary to the top owner — the row
    // *below* — which on an intermediate page is the first row of the *next*
    // page, leaving the previous page's bottom edge open. Excel frames every
    // page across the full block width, and the row-number gutter's span was
    // the part left hanging: the data span survived only because #622's
    // gridline seeds happen to put a band there independently, and with
    // gridlines off the whole bottom edge would go (issue #722).
    //
    // Inverting the tie would only move the hole to the top of the next page,
    // so both bands are kept. They are one rule drawn twice at the same
    // coordinate, which is what `augment_page_with_print_headings` already
    // assumes when it declares a bottom on every gutter cell: "adjacent
    // cells' coincident bands overlap invisibly, as in #622". The cost is a
    // doubled draw op per interior boundary, visible in a rect census but not
    // in the ink.
    //
    // Scoped to sheets that print headings, which is where the frame is a
    // stated Excel behaviour. Word tables keep one owner per boundary.
    let keeps_coincident_horizontal_bands: bool = table.prints_headings;
    // The repeating-header ownership exception was measured for Excel's
    // split header/body emission. Word paints an equal shared rule inside the
    // following body row even when the row above repeats (#724).
    let applies_excel_repeating_header_ownership: bool =
        table.border_paint_model == TableBorderPaintModel::ExcelBoundaryBands;

    let mut painted: Vec<Vec<Option<CellBorder>>> = table
        .rows
        .iter()
        .map(|row| vec![None; row.cells.len()])
        .collect();
    for placement in &placements {
        let Some(border) = &cell_of(placement).border else {
            continue;
        };
        let column_tracks = placement.first_col..placement.first_col + placement.col_span;
        let row_tracks = placement.row_index..placement.row_index + placement.row_span;
        let mut resolved: CellBorder = border.clone();
        // A bottom/right yields to a neighbour's declaration ranked at least
        // as heavy (ties keep the top/left owner); a top/left yields only to
        // a strictly heavier one. Exactly one side survives per fully shared
        // boundary in every rank combination. The repeating-header boundary
        // inverts the tie direction (see the function docs).
        let bottom_boundary: usize = placement.row_index + placement.row_span;
        let bottom_is_repeating_header_boundary: bool = applies_excel_repeating_header_ownership
            && repeating_header_boundaries.contains(&bottom_boundary);
        if let Some(side) = &resolved.bottom
            && (bottom_is_repeating_header_boundary || !keeps_coincident_horizontal_bands)
            && column_tracks.clone().all(|col| {
                top_sides
                    .get(&(bottom_boundary, col))
                    .is_some_and(|neighbour| {
                        let neighbour_rank = boundary_conflict_rank(neighbour);
                        let own_rank = boundary_conflict_rank(side);
                        if bottom_is_repeating_header_boundary {
                            neighbour_rank > own_rank
                        } else {
                            neighbour_rank >= own_rank
                        }
                    })
            })
        {
            resolved.bottom = if bottom_is_repeating_header_boundary {
                // The body's strictly heavier band must also repeat with the
                // header: adopt the highest-ranked body declaration into this
                // header cell's bottom slot instead of dropping it.
                column_tracks
                    .clone()
                    .filter_map(|col| top_sides.get(&(bottom_boundary, col)))
                    .max_by(|a, b| {
                        boundary_conflict_rank(a)
                            .partial_cmp(&boundary_conflict_rank(b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .cloned()
            } else {
                None
            };
        }
        if let Some(side) = &resolved.top
            && column_tracks.clone().all(|col| {
                bottom_sides
                    .get(&(placement.row_index, col))
                    .is_some_and(|neighbour| {
                        let neighbour_rank = boundary_conflict_rank(neighbour);
                        let own_rank = boundary_conflict_rank(side);
                        if applies_excel_repeating_header_ownership
                            && repeating_header_boundaries.contains(&placement.row_index)
                        {
                            // Ties at the repeating-header boundary stay with
                            // the header's bottom declaration.
                            neighbour_rank >= own_rank
                        } else {
                            neighbour_rank > own_rank
                        }
                    })
            })
        {
            resolved.top = None;
        }
        if let Some(side) = &resolved.right
            && row_tracks.clone().all(|row| {
                left_sides
                    .get(&(placement.first_col + placement.col_span, row))
                    .is_some_and(|neighbour| {
                        boundary_conflict_rank(neighbour) >= boundary_conflict_rank(side)
                    })
            })
        {
            resolved.right = None;
        }
        if let Some(side) = &resolved.left
            && row_tracks.clone().all(|row| {
                right_sides
                    .get(&(placement.first_col, row))
                    .is_some_and(|neighbour| {
                        boundary_conflict_rank(neighbour) > boundary_conflict_rank(side)
                    })
            })
        {
            resolved.left = None;
        }
        if resolved.top.is_some()
            || resolved.bottom.is_some()
            || resolved.left.is_some()
            || resolved.right.is_some()
        {
            painted[placement.row_index][placement.cell_index] = Some(resolved);
        }
    }

    // Printed gridlines (issue #622): `<printOptions gridLines="1"/>` rules
    // every cell boundary of the printed range with Excel's gridline band,
    // strictly below any explicit declaration — a boundary owned by any
    // declared side (either neighbour's) keeps that side alone, hair borders
    // included, which the #619 rank would otherwise wrongly outrank. Every
    // placement seeds all four of its unowned sides: the two seeds of an
    // interior boundary are boundary-anchored to the same [B, B+1] strip and
    // coincide invisibly, and the redundant bottom seed is what closes the
    // grid at a page break, where GT draws the bottom rule (which row ends a
    // page only the renderer knows).
    if table.prints_gridlines {
        // A cell fill suppresses all four adjacent gridline segments: GT
        // truncates the interior verticals at a filled row and omits the
        // horizontal at the fill's bottom boundary (Tests p1 vs the
        // fill-free p2 control), because fills paint after gridlines.
        //
        // TODO(#622 follow-up: a background-filled row that lands as the
        // first row of a page under natural pagination leaves the previous
        // page's grid open at that boundary — GT closes it; suppression is
        // kept because an unsuppressed band would paint over the fill's top
        // edge on every within-page filled row, the far more common case).
        let mut fill_suppressed_horizontal: HashSet<(usize, usize)> = HashSet::new();
        let mut fill_suppressed_vertical: HashSet<(usize, usize)> = HashSet::new();
        for placement in &placements {
            if cell_of(placement).background.is_none() {
                continue;
            }
            for col in placement.first_col..placement.first_col + placement.col_span {
                fill_suppressed_horizontal.insert((placement.row_index, col));
                fill_suppressed_horizontal.insert((placement.row_index + placement.row_span, col));
            }
            for row in placement.row_index..placement.row_index + placement.row_span {
                fill_suppressed_vertical.insert((placement.first_col, row));
                fill_suppressed_vertical.insert((placement.first_col + placement.col_span, row));
            }
        }
        // Printed headings (issue #623): boundary 0 in each direction is the
        // heading exterior — the strip row's top and the gutter column's
        // left. GT rules those edges as the black print FRAME, which the
        // forcing pass below paints; excluding GRIDLINE-styled seeds here
        // keeps the frame band the boundary's only owner (replace, not
        // stack). The data area starts at row/column 1, so its seeding is
        // untouched.
        let heading_exterior_is_excluded: bool = table.prints_headings;
        // Stated as "no disqualifier applies" rather than as a chain of ANDed
        // negations: the two are equivalent by De Morgan, but only this form
        // satisfies clippy::nonminimal_bool.
        let horizontal_boundary_is_free = |boundary: usize, col: usize| -> bool {
            !((heading_exterior_is_excluded && boundary == 0)
                || top_sides.contains_key(&(boundary, col))
                || bottom_sides.contains_key(&(boundary, col))
                || fill_suppressed_horizontal.contains(&(boundary, col)))
        };
        let vertical_boundary_is_free = |boundary: usize, row: usize| -> bool {
            !((heading_exterior_is_excluded && boundary == 0)
                || left_sides.contains_key(&(boundary, row))
                || right_sides.contains_key(&(boundary, row))
                || fill_suppressed_vertical.contains(&(boundary, row)))
        };
        for placement in &placements {
            let column_tracks = placement.first_col..placement.first_col + placement.col_span;
            let row_tracks = placement.row_index..placement.row_index + placement.row_span;
            let mut seeded: CellBorder = painted[placement.row_index][placement.cell_index]
                .take()
                .unwrap_or_default();
            // Whole-side seeding: a side whose boundary is even partially
            // declared or fill-suppressed stays unseeded — the merged-cell
            // partial-overlap simplification of #619, erring toward fewer
            // rules, which is also GT's direction for fills.
            if seeded.top.is_none()
                && column_tracks
                    .clone()
                    .all(|col| horizontal_boundary_is_free(placement.row_index, col))
            {
                seeded.top = Some(printed_gridline_side());
            }
            if seeded.bottom.is_none()
                && column_tracks.clone().all(|col| {
                    horizontal_boundary_is_free(placement.row_index + placement.row_span, col)
                })
            {
                seeded.bottom = Some(printed_gridline_side());
            }
            if seeded.left.is_none()
                && row_tracks
                    .clone()
                    .all(|row| vertical_boundary_is_free(placement.first_col, row))
            {
                seeded.left = Some(printed_gridline_side());
            }
            if seeded.right.is_none()
                && row_tracks.clone().all(|row| {
                    vertical_boundary_is_free(placement.first_col + placement.col_span, row)
                })
            {
                seeded.right = Some(printed_gridline_side());
            }
            if seeded.top.is_some()
                || seeded.bottom.is_some()
                || seeded.left.is_some()
                || seeded.right.is_some()
            {
                painted[placement.row_index][placement.cell_index] = Some(seeded);
            }
        }
    }

    // Printed headings (issue #623): GT draws a 1pt black frame enclosing
    // the heading bands and the data grid, on the table's exterior
    // boundaries — the corner box's top and left edges ARE the frame. Forced
    // here, after declaration resolution and gridline seeding, so the frame
    // band REPLACES whatever landed on a frame boundary (a heading gray
    // rule, a #622 closure seed) instead of stacking on it; painting is
    // band-only, so no layout inset moves. The strip-top edge rides the
    // repeating header block (pages 2+ carry it) and the left/right edges
    // ride each row, so they close on every rendered page; the bottom edge
    // exists only on the LAST table row — a Typst page break inside the
    // table leaves that page's bottom open unless printed gridlines already
    // close it with their own black band (tracked in #722).
    // TODO(#623 follow-up: whether a gridLines-only sheet — headings off —
    // prints this frame is unmeasured; the frame is gated on
    // prints_headings alone until a GT probe answers it).
    if table.prints_headings {
        let frame_rank: BoundaryConflictRank = boundary_conflict_rank(&print_heading_frame_side());
        // A strictly heavier declared band keeps its boundary (GT for a
        // heavy cell border meeting the frame is unmeasured; erring toward
        // the author's declaration); equal-rank sides — the heading gray
        // rules — yield to the frame, as GT clips them to its interior.
        let force_frame = |slot: &mut Option<BorderSide>| {
            if !slot
                .as_ref()
                .is_some_and(|side| boundary_conflict_rank(side) > frame_rank)
            {
                *slot = Some(print_heading_frame_side());
            }
        };
        for placement in &placements {
            let on_top_exterior: bool = placement.row_index == 0;
            let on_left_exterior: bool = placement.first_col == 0;
            let on_right_exterior: bool = placement.first_col + placement.col_span == num_cols;
            let on_bottom_exterior: bool =
                placement.row_index + placement.row_span == table.rows.len();
            if !(on_top_exterior || on_left_exterior || on_right_exterior || on_bottom_exterior) {
                continue;
            }
            let mut framed: CellBorder = painted[placement.row_index][placement.cell_index]
                .take()
                .unwrap_or_default();
            if on_top_exterior {
                force_frame(&mut framed.top);
            }
            if on_left_exterior {
                force_frame(&mut framed.left);
            }
            if on_right_exterior {
                force_frame(&mut framed.right);
            }
            if on_bottom_exterior {
                force_frame(&mut framed.bottom);
            }
            painted[placement.row_index][placement.cell_index] = Some(framed);
        }
    }

    painted
}

/// One cell's share of the boundary-band regime, threaded from the row walk
/// into the cell writer.
struct BoundaryBandCell<'a> {
    /// The sides this cell paints after shared-boundary resolution. `None`
    /// paints nothing but still selects the band regime (no cell stroke).
    painted_border: &'a Option<CellBorder>,
    /// The source application's placement convention for those bands.
    paint_model: TableBorderPaintModel,
    /// How far this cell's vertical bands may extend.
    vertical_extent: VerticalBandExtent,
}

/// How a cell's vertical border bands obtain their length (issue #619).
///
/// A Typst-relative length (`100%`) inside a `#place` resolves against the
/// measurement region — the remaining page, not the cell — whenever any row
/// the cell spans is auto-sized (measured on typst 0.14/0.15), painting
/// page-long spears. Vertical bands therefore always use concrete lengths.
enum VerticalBandExtent {
    /// Every spanned row's height is fixed: one top-anchored band of the
    /// summed frame height covers boundary to boundary exactly.
    FrameHeight(f64),
    /// The span includes auto-sized rows, whose final height only the
    /// renderer knows. Two twin bands anchored at the cell's top and bottom
    /// edges are painted instead, each sized from the row's tallest
    /// single-line frame: the twins coincide exactly on single-line rows
    /// (the row is sized by that same line box) and cover a wrapped row from
    /// both ends without overshooting, because a row is at least as tall as
    /// its tallest cell's first line. Rows wrapping past roughly twice the
    /// estimate keep a mid-row gap (known limitation).
    TwinBands(f64),
    /// No cell in the row has usable line metrics: twin bands sized by the
    /// ambient text size, following the data-bar `1.2em` precedent.
    TwinBandsEmFallback,
}

/// Decide [`VerticalBandExtent`] for one cell of a boundary-band table.
///
/// `row_frame_estimate_cache` is the calling row-walk's per-row memo for
/// [`auto_row_frame_height_estimate_pt`]: the estimate is row-wide and
/// costly (it reads every cell's font metrics), so each row computes it at
/// most once however many cells land here.
#[allow(clippy::too_many_arguments)]
fn vertical_band_extent(
    rows: &[TableRow],
    row_index: usize,
    cell: &TableCell,
    fixed_row_heights: bool,
    default_cell_padding: Insets,
    ctx: &GenCtx,
    row_frame_estimate_cache: &mut Option<Option<f64>>,
) -> VerticalBandExtent {
    let row_span: usize = (cell.row_span as usize).max(1);
    // A span reaching outside this row group (header/body split) falls back
    // to the twins below rather than summing a partial height.
    let spanned_rows: &[TableRow] = &rows[row_index..(row_index + row_span).min(rows.len())];
    if fixed_row_heights && spanned_rows.len() == row_span {
        let spanned_height: Option<f64> = spanned_rows
            .iter()
            .try_fold(0.0_f64, |sum, row| row.height.map(|height| sum + height));
        if let Some(frame_height_pt) = spanned_height {
            return VerticalBandExtent::FrameHeight(frame_height_pt);
        }
    }
    // Estimate from the anchor row only; a multi-row span over auto rows
    // keeps whatever the twins cover (known limitation).
    let frame_estimate: Option<f64> = *row_frame_estimate_cache.get_or_insert_with(|| {
        auto_row_frame_height_estimate_pt(&rows[row_index], default_cell_padding, ctx)
    });
    match frame_estimate {
        Some(frame_estimate_pt) => VerticalBandExtent::TwinBands(frame_estimate_pt),
        None => VerticalBandExtent::TwinBandsEmFallback,
    }
}

// Test-only probe counting `auto_row_frame_height_estimate_pt` calls, so
// the once-per-row caching contract (issue #619 review, remediation 3) is
// assertable: the estimate walks every cell's font metrics, and calling it
// per cell made vertical-band preparation O(cells²) per row. (A regular
// comment: rustc discards doc comments attached to macro invocations.)
#[cfg(test)]
thread_local! {
    pub(super) static AUTO_ROW_FRAME_ESTIMATE_CALLS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

/// The frame height the renderer will give an auto-sized row, estimated as
/// the tallest cell's single-line box plus that cell's insets — exact when
/// the sizing cell holds one line (the common spreadsheet case), an
/// underestimate when it wraps.
fn auto_row_frame_height_estimate_pt(
    row: &TableRow,
    default_cell_padding: Insets,
    ctx: &GenCtx,
) -> Option<f64> {
    #[cfg(test)]
    AUTO_ROW_FRAME_ESTIMATE_CALLS.with(|calls| calls.set(calls.get() + 1));
    row.cells
        .iter()
        .filter_map(|cell| {
            let paragraph: &Paragraph = cell.content.iter().find_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })?;
            // Auto rows never seat text on the descender — the seating gate
            // keys on a fixed row height — so the estimate must not either.
            let line_box: CellLineBox = word_cell_line_box(
                &paragraph.runs,
                &paragraph.style,
                ctx.line_grid_pitch,
                ctx.row_has_east_asian_text,
                false,
            )?;
            let inset: Insets = cell_inset_with_border(cell, default_cell_padding);
            Some(
                (line_box.top_em + line_box.bottom_em) * line_box.font_size_pt
                    + inset.top
                    + inset.bottom,
            )
        })
        .fold(None, |tallest: Option<f64>, height| {
            Some(tallest.map_or(height, |t| t.max(height)))
        })
}

/// Word's PDF graphics grid is 1/300 inch (0.24 pt). Its native exports snap
/// a declared half-point table border to two grid units, or 0.48 pt (#724).
const WORD_PDF_GRAPHICS_GRID_PT: f64 = 72.0 / 300.0;

fn word_pdf_border_side(side: &BorderSide) -> BorderSide {
    let grid_units: f64 = (side.width / WORD_PDF_GRAPHICS_GRID_PT).round().max(1.0);
    BorderSide {
        width: grid_units * WORD_PDF_GRAPHICS_GRID_PT,
        ..side.clone()
    }
}

/// Paint Word table borders as rectangles whose leading edge is the nominal
/// grid boundary and whose ink extends along positive x/y (#724). Solid and
/// double rules use filled rectangles, matching Word's PDF primitives;
/// patterned rules keep a line solely to preserve their dash sequence.
fn write_word_positive_axis_border_overlays(
    out: &mut String,
    border: &CellBorder,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
) {
    if let Some(side) = &border.top {
        write_word_horizontal_border(out, side, inset, true);
    }
    if let Some(side) = &border.bottom {
        write_word_horizontal_border(out, side, inset, false);
    }
    if let Some(side) = &border.left {
        write_word_vertical_border(out, side, inset, vertical_extent, true);
    }
    if let Some(side) = &border.right {
        write_word_vertical_border(out, side, inset, vertical_extent, false);
    }
}

fn write_word_horizontal_border(out: &mut String, side: &BorderSide, inset: Insets, is_top: bool) {
    let painted_side: BorderSide = word_pdf_border_side(side);
    let horizontal_length: String =
        format!("100% + {}pt", format_geometry(inset.left + inset.right));
    let align: &str = if is_top {
        "top + left"
    } else {
        "bottom + left"
    };
    let offsets: &[f64] = if painted_side.style == BorderLineStyle::Double {
        &[0.0, painted_side.width * 2.0]
    } else {
        &[0.0]
    };
    for inward_offset in offsets {
        if matches!(
            painted_side.style,
            BorderLineStyle::Solid | BorderLineStyle::Double | BorderLineStyle::None
        ) {
            let dy: f64 = if is_top {
                -inset.top + inward_offset
            } else {
                inset.bottom + painted_side.width + inward_offset
            };
            write_word_band_rect(
                out,
                align,
                &format!("{}pt", format_geometry(-inset.left)),
                &format!("{}pt", format_geometry(dy)),
                &horizontal_length,
                &format!("{}pt", format_geometry(painted_side.width)),
                &painted_side.color,
            );
        } else {
            let centre: f64 = painted_side.width / 2.0;
            let dy: f64 = if is_top {
                -inset.top + centre
            } else {
                inset.bottom + centre
            };
            write_boundary_band_line(
                out,
                align,
                -inset.left,
                dy,
                "0deg",
                &horizontal_length,
                &painted_side,
            );
        }
    }
}

fn write_word_vertical_border(
    out: &mut String,
    side: &BorderSide,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
    is_left: bool,
) {
    let painted_side: BorderSide = word_pdf_border_side(side);
    let align: &str = if is_left { "top + left" } else { "top + right" };
    let offsets: &[f64] = if painted_side.style == BorderLineStyle::Double {
        &[0.0, painted_side.width * 2.0]
    } else {
        &[0.0]
    };
    for inward_offset in offsets {
        let band_anchor_x: f64 = if is_left {
            -inset.left + inward_offset
        } else {
            inset.right + painted_side.width + inward_offset
        };
        if matches!(
            painted_side.style,
            BorderLineStyle::Solid | BorderLineStyle::Double | BorderLineStyle::None
        ) {
            write_word_vertical_band_rect(
                out,
                align,
                band_anchor_x,
                painted_side.width,
                inset,
                vertical_extent,
                &painted_side.color,
            );
        } else {
            let centre_x: f64 = if is_left {
                band_anchor_x + painted_side.width / 2.0
            } else {
                band_anchor_x - painted_side.width / 2.0
            };
            write_word_patterned_vertical_band(
                out,
                align,
                centre_x,
                inset,
                vertical_extent,
                &painted_side,
            );
        }
    }
}

fn write_word_vertical_band_rect(
    out: &mut String,
    align: &str,
    dx: f64,
    width: f64,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
    color: &Color,
) {
    let dx: String = format!("{}pt", format_geometry(dx));
    let width: String = format!("{}pt", format_geometry(width));
    match *vertical_extent {
        VerticalBandExtent::FrameHeight(frame_height_pt) => {
            write_word_band_rect(
                out,
                align,
                &dx,
                &format!("{}pt", format_geometry(-inset.top)),
                &width,
                &format!("{}pt", format_geometry(frame_height_pt)),
                color,
            );
        }
        VerticalBandExtent::TwinBands(frame_estimate_pt) => {
            let height: String = format!("{}pt", format_geometry(frame_estimate_pt));
            write_word_band_rect(
                out,
                align,
                &dx,
                &format!("{}pt", format_geometry(-inset.top)),
                &width,
                &height,
                color,
            );
            write_word_band_rect(
                out,
                &align.replacen("top", "bottom", 1),
                &dx,
                &format!("{}pt", format_geometry(inset.bottom)),
                &width,
                &height,
                color,
            );
        }
        VerticalBandExtent::TwinBandsEmFallback => {
            let height: String = format!("1.2em + {}pt", format_geometry(inset.top + inset.bottom));
            write_word_band_rect(
                out,
                align,
                &dx,
                &format!("{}pt", format_geometry(-inset.top)),
                &width,
                &height,
                color,
            );
            write_word_band_rect(
                out,
                &align.replacen("top", "bottom", 1),
                &dx,
                &format!("{}pt", format_geometry(inset.bottom)),
                &width,
                &height,
                color,
            );
        }
    }
}

fn write_word_patterned_vertical_band(
    out: &mut String,
    align: &str,
    dx: f64,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
    side: &BorderSide,
) {
    match *vertical_extent {
        VerticalBandExtent::FrameHeight(frame_height_pt) => write_boundary_band_line(
            out,
            align,
            dx,
            -inset.top,
            "90deg",
            &format!("{}pt", format_geometry(frame_height_pt)),
            side,
        ),
        VerticalBandExtent::TwinBands(frame_estimate_pt) => {
            let length: String = format!("{}pt", format_geometry(frame_estimate_pt));
            write_boundary_band_line(out, align, dx, -inset.top, "90deg", &length, side);
            write_boundary_band_line(
                out,
                &align.replacen("top", "bottom", 1),
                dx,
                inset.bottom,
                "-90deg",
                &length,
                side,
            );
        }
        VerticalBandExtent::TwinBandsEmFallback => {
            let length: String = format!("1.2em + {}pt", format_geometry(inset.top + inset.bottom));
            write_boundary_band_line(out, align, dx, -inset.top, "90deg", &length, side);
            write_boundary_band_line(
                out,
                &align.replacen("top", "bottom", 1),
                dx,
                inset.bottom,
                "-90deg",
                &length,
                side,
            );
        }
    }
}

fn write_word_band_rect(
    out: &mut String,
    align: &str,
    dx: &str,
    dy: &str,
    width: &str,
    height: &str,
    color: &Color,
) {
    let _ = write!(
        out,
        "#place({align}, dx: {dx}, dy: {dy}, rect(width: {width}, height: {height}, fill: {}, stroke: none))",
        rgb(color),
    );
}

/// Paint a cell's borders as filled bands anchored to the nominal grid
/// boundaries, as Excel prints them (issue #619; native Excel 16.111
/// one-factor probe + golden-mock GT traces):
///
/// - 1pt styles (`thin`, `hair`, dashes): band `[B, B+1]`, on the +x/+y side
///   even at the table's outer right/bottom edge;
/// - `medium` (2pt): `[B-1, B+1]`; `thick` (3pt): `[B-1, B+2]` — an odd
///   leftover point always lands on the +x/+y side;
/// - `double`: two 1pt bands `[B-1, B]` and `[B+1, B+2]`, the boundary strip
///   `[B, B+1]` being the gap.
///
/// The `dx`/`dy` offsets back out the cell's effective inset so `B` is the
/// grid boundary the table already lays out. Horizontal bands run from the
/// cell's left boundary to 1pt past its right boundary, owning the corner
/// blocks; verticals span the same extended run instead of Excel's trim to
/// strictly between the horizontals — the overlap is same-colour in the GT
/// regimes and therefore invisible. Corners whose crossing rules differ in
/// colour would need that trim (known limitation, deliberately skipped).
fn write_boundary_anchored_border_overlays(
    out: &mut String,
    border: &CellBorder,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
) {
    // Horizontal bands can stay relative: a line's `100%` length resolves
    // against the cell's width, which the spreadsheet's fixed column tracks
    // always determine (and colspans span correctly through it).
    let horizontal_length: String = format!(
        "100% + {}pt",
        format_geometry(inset.left + inset.right + BAND_RUN_END_EXTENSION_PT)
    );
    if let Some(side) = &border.top {
        for centre in band_centre_offsets(side) {
            write_boundary_band_line(
                out,
                "top + left",
                -inset.left,
                -inset.top + centre,
                "0deg",
                &horizontal_length,
                side,
            );
        }
    }
    if let Some(side) = &border.bottom {
        for centre in band_centre_offsets(side) {
            write_boundary_band_line(
                out,
                "bottom + left",
                -inset.left,
                inset.bottom + centre,
                "0deg",
                &horizontal_length,
                side,
            );
        }
    }
    if let Some(side) = &border.left {
        for centre in band_centre_offsets(side) {
            write_vertical_boundary_band(
                out,
                side,
                "left",
                -inset.left + centre,
                inset,
                vertical_extent,
            );
        }
    }
    if let Some(side) = &border.right {
        for centre in band_centre_offsets(side) {
            write_vertical_boundary_band(
                out,
                side,
                "right",
                inset.right + centre,
                inset,
                vertical_extent,
            );
        }
    }
}

/// Paint one vertical band rule at `dx` from the cell's `horizontal_anchor`
/// edge, spanning from the row's top boundary to 1pt past its bottom boundary
/// per [`VerticalBandExtent`]'s answer for this cell.
fn write_vertical_boundary_band(
    out: &mut String,
    side: &BorderSide,
    horizontal_anchor: &str,
    dx: f64,
    inset: Insets,
    vertical_extent: &VerticalBandExtent,
) {
    let top_anchor: String = format!("top + {horizontal_anchor}");
    match *vertical_extent {
        VerticalBandExtent::FrameHeight(frame_height_pt) => {
            let length: String = format!(
                "{}pt",
                format_geometry(frame_height_pt + BAND_RUN_END_EXTENSION_PT)
            );
            write_boundary_band_line(out, &top_anchor, dx, -inset.top, "90deg", &length, side);
        }
        VerticalBandExtent::TwinBands(frame_estimate_pt) => {
            let length: String = format!(
                "{}pt",
                format_geometry(frame_estimate_pt + BAND_RUN_END_EXTENSION_PT)
            );
            write_vertical_twin_bands(out, side, dx, inset, &top_anchor, &length);
        }
        VerticalBandExtent::TwinBandsEmFallback => {
            let length: String = format!(
                "1.2em + {}pt",
                format_geometry(inset.top + inset.bottom + BAND_RUN_END_EXTENSION_PT)
            );
            write_vertical_twin_bands(out, side, dx, inset, &top_anchor, &length);
        }
    }
}

/// Two same-length rules: one hanging from the row's top boundary, one rising
/// from 1pt past its bottom boundary. On a single-line auto row they coincide
/// exactly; on a wrapped row they cover it from both ends.
fn write_vertical_twin_bands(
    out: &mut String,
    side: &BorderSide,
    dx: f64,
    inset: Insets,
    top_anchor: &str,
    length: &str,
) {
    let bottom_anchor: String = top_anchor.replacen("top", "bottom", 1);
    write_boundary_band_line(out, top_anchor, dx, -inset.top, "90deg", length, side);
    write_boundary_band_line(
        out,
        &bottom_anchor,
        dx,
        inset.bottom + BAND_RUN_END_EXTENSION_PT,
        "-90deg",
        length,
        side,
    );
}

/// Offsets of each painted rule's centre line from the boundary `B`, for a
/// band of the side's width `w`: a single band `[B - floor(w/2), ...]` puts
/// the centre at `w/2 - floor(w/2)` (0.5 for thin/thick, 0 for medium); a
/// double paints one rule per band.
fn band_centre_offsets(side: &BorderSide) -> Vec<f64> {
    if side.style == BorderLineStyle::Double {
        vec![-side.width / 2.0, side.width * 1.5]
    } else {
        vec![side.width / 2.0 - (side.width / 2.0).floor()]
    }
}

#[allow(clippy::too_many_arguments)]
fn write_boundary_band_line(
    out: &mut String,
    align: &str,
    dx: f64,
    dy: f64,
    angle: &str,
    length: &str,
    side: &BorderSide,
) {
    // `stroke_value` keeps the dash dict of patterned styles (dashed, dotted,
    // hair) on the overlay line; a double side's two rules are each plain.
    let _ = write!(
        out,
        "#place({align}, dx: {}pt, dy: {}pt, line(length: {length}, angle: {angle}, stroke: {}))",
        format_geometry(dx),
        format_geometry(dy),
        stroke_value(side, true),
    );
}

fn write_cell_params(
    out: &mut String,
    cell: &TableCell,
    clamped_colspan: u32,
    default_cell_padding: Insets,
    paints_boundary_bands: bool,
) {
    let mut first = true;

    if clamped_colspan > 1 {
        write_param(out, &mut first, &format!("colspan: {clamped_colspan}"));
    }
    if cell.row_span > 1 {
        write_param(out, &mut first, &format!("rowspan: {}", cell.row_span));
    }
    if let Some(ref bg) = cell.background {
        write_param(out, &mut first, &format_color(bg));
    }
    let inset: Insets = cell_inset_with_border(cell, default_cell_padding);
    if cell.padding.is_some() || cell.border.is_some() {
        write_param(
            out,
            &mut first,
            &format!("inset: {}", format_insets(&inset)),
        );
    }
    // A boundary-band cell paints its borders as overlays instead: a Typst
    // stroke is centred on the track boundary, which cannot reproduce either
    // Excel's measured bands (#619) or Word's positive-axis rectangles
    // (#724). The
    // `inset` above still reserves the border's layout space either way.
    if !paints_boundary_bands && let Some(ref border) = cell.border {
        let stroke = format_cell_stroke(border);
        if !stroke.is_empty() {
            write_param(out, &mut first, &stroke);
        }
    }
    if let Some(ref va) = cell.vertical_align {
        let align_str: &str = match va {
            CellVerticalAlign::Top => "top",
            CellVerticalAlign::Center => "horizon",
            CellVerticalAlign::Bottom => "bottom",
        };
        write_param(out, &mut first, &format!("align: {align_str}"));
    }
}

fn format_cell_stroke(border: &CellBorder) -> String {
    let mut parts = Vec::with_capacity(4);

    if let Some(ref side) = border.top
        && side.style != BorderLineStyle::Double
    {
        parts.push(format!("top: {}", format_border_side(side)));
    }
    if let Some(ref side) = border.bottom
        && side.style != BorderLineStyle::Double
    {
        parts.push(format!("bottom: {}", format_border_side(side)));
    }
    if let Some(ref side) = border.left
        && side.style != BorderLineStyle::Double
    {
        parts.push(format!("left: {}", format_border_side(side)));
    }
    if let Some(ref side) = border.right
        && side.style != BorderLineStyle::Double
    {
        parts.push(format!("right: {}", format_border_side(side)));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("stroke: ({})", parts.join(", "))
    }
}

fn format_border_side(side: &BorderSide) -> String {
    stroke_value(side, true)
}

fn generate_cell_content(
    out: &mut String,
    blocks: &[Block],
    ctx: &mut GenCtx,
) -> Result<(), ConvertError> {
    // Word separates stacked cell paragraphs only by the resolved
    // `w:spacing w:after`/`w:before` — the explicit `#v` emissions — but
    // sibling `#block` wrappers otherwise pick up Typst's ambient default
    // block spacing (1.2em at the document size), adding ~13pt Word never
    // shows (issue #625). This counts the stacked blocks; whether a given
    // paragraph may actually drop that ambient spacing is decided in
    // `generate_cell_paragraph`, which zeroes it only for paragraphs that
    // emit a fixed line box of their own. A lone block keeps today's exact
    // emission, since its boundary spacing vanishes at the cell edge anyway.
    let rendered_block_count: usize = blocks
        .iter()
        .filter(|block| {
            !matches!(
                block,
                Block::TableOfContents(_) | Block::PageBreak | Block::ColumnBreak
            )
        })
        .count();
    let stacks_multiple_blocks: bool = rendered_block_count > 1;
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let paragraph_ctx = |para: &Paragraph| CellParagraphCtx {
            default_tab_width_pt: ctx.default_tab_width_pt,
            line_grid_pitch: ctx.line_grid_pitch,
            row_has_east_asian_text: ctx.row_has_east_asian_text,
            seats_text_on_descender: ctx.cell_seats_text_on_descender,
            in_spill_cell: ctx.in_spill_cell,
            uses_powerpoint_line_box: ctx.table_uses_powerpoint_line_box,
            stacks_multiple_blocks,
            paragraph_mark_metric_runs: para
                .runs
                .is_empty()
                .then(|| empty_cell_paragraph_metric_runs(blocks, i))
                .flatten(),
            breaks_hangul_at_eojeol: ctx.breaks_hangul_at_eojeol,
            available_measure_pt: ctx.available_measure_pt,
        };
        match block {
            // A `TOC` field inside a table cell is not a shape Word produces.
            Block::TableOfContents(_) => {}
            Block::Caption(caption) => {
                generate_cell_paragraph(out, &caption.paragraph, &paragraph_ctx(&caption.paragraph))
            }
            Block::Paragraph(para) => generate_cell_paragraph(out, para, &paragraph_ctx(para)),
            Block::Table(table) => {
                if ctx.table_depth < MAX_TABLE_DEPTH {
                    generate_table(out, table, ctx)?;
                }
            }
            Block::Image(img) => generate_image(out, img, ctx),
            Block::InlineImages(images) => {
                for image in images {
                    generate_image(out, image, ctx);
                }
            }
            Block::FloatingImage(fi) => generate_floating_image(out, fi, ctx),
            Block::FloatingTextBox(ftb) => generate_floating_text_box(out, ftb, ctx)?,
            Block::FloatingShape(fs) => generate_floating_shape(out, fs),
            Block::List(list) => {
                if can_render_fixed_text_list_inline(list) {
                    generate_fixed_text_list(out, list, true, None, false)?;
                } else {
                    // No wrapper settings reach a cell list, so it has no
                    // fixed text edges of its own to restore (issue #626).
                    let list_id = ctx.next_list_id();
                    generate_list(
                        out,
                        list,
                        None,
                        list_id,
                        ctx.default_tab_width_pt,
                        ListEojeolWrap {
                            breaks_hangul_at_eojeol: ctx.breaks_hangul_at_eojeol,
                            line_box_em: None,
                            available_measure_pt: ctx.available_measure_pt,
                        },
                    )?;
                }
            }
            Block::MathEquation(math) => generate_math_equation(out, math),
            Block::Chart(chart) => generate_chart(out, chart),
            Block::PageBreak | Block::ColumnBreak => {}
        }
    }
    Ok(())
}

/// The cell-level facts a paragraph's emission needs beyond its own IR.
struct CellParagraphCtx<'a> {
    default_tab_width_pt: f64,
    line_grid_pitch: Option<f64>,
    /// Decided once per row so every cell in it shares a baseline (issue #498).
    row_has_east_asian_text: bool,
    seats_text_on_descender: bool,
    /// Whether this paragraph is inside a spill cell's clipped wrapper, where
    /// the `#place` anchor already carries the cell's horizontal alignment.
    /// A `width: 100%` block inside that wrapper is not just redundant: the
    /// wrapper sizes itself from `measure()`, which lays out in an unbounded
    /// region where a percentage width has nothing to resolve against, and the
    /// paragraph came back so narrow that every word took a line of its own
    /// (issue #811).
    in_spill_cell: bool,
    /// Whether the cell stacks more than one rendered block, so this
    /// paragraph has a sibling to leak Typst's default block spacing against.
    stacks_multiple_blocks: bool,
    /// Runs standing in for the paragraph mark's own font when the paragraph
    /// has none of its own — see [`empty_cell_paragraph_metric_runs`].
    paragraph_mark_metric_runs: Option<&'a [Run]>,
    /// Whether the enclosing page is a Word flow page, whose Hangul lines
    /// break only at eojeol boundaries (issue #626). False for a slide or a
    /// sheet, which keep the engine's syllable breaking.
    breaks_hangul_at_eojeol: bool,
    /// Whether this cell paces its lines on PowerPoint's flat 1.2em line
    /// instead of Word's hhea one — true inside a slide's `<a:tbl>`
    /// (issue #663).
    uses_powerpoint_line_box: bool,
    /// The width one line of this cell has, in points: the column width less
    /// the cell's own inset. Bounds how wide a framed eojeol may be.
    available_measure_pt: Option<f64>,
}

/// The runs an empty `<w:p>` in a cell borrows its line box from.
///
/// Word lays a blank cell paragraph out on a full line, sized from the
/// paragraph mark's own `w:rPr`. The IR carries no runs — and so no font or
/// size — for such a paragraph, so the nearest sibling paragraph in the same
/// cell stands in: the one above by preference, since a spacer line follows
/// the text it separates (issue #625).
///
/// `None` when the cell holds no other text at all — a wholly blank cell,
/// whose height Word takes from the row and the cell insets rather than from
/// any run this codegen could measure.
/// TODO(#625 follow-up: a wholly blank cell keeps today's zero-height
/// emission, so a blank auto-height row is still one line short of Word;
/// sizing it needs the table/style default font, which the IR does not carry
/// to codegen — measure against a Word GT before inventing one).
fn empty_cell_paragraph_metric_runs(blocks: &[Block], index: usize) -> Option<&[Run]> {
    fn paragraph_runs(block: &Block) -> Option<&[Run]> {
        match block {
            Block::Paragraph(paragraph) => Some(paragraph.runs.as_slice()),
            Block::Caption(caption) => Some(caption.paragraph.runs.as_slice()),
            _ => None,
        }
    }
    let preceding = blocks[..index]
        .iter()
        .rev()
        .filter_map(paragraph_runs)
        .find(|runs| !runs.is_empty());
    preceding.or_else(|| {
        blocks[index + 1..]
            .iter()
            .filter_map(paragraph_runs)
            .find(|runs| !runs.is_empty())
    })
}

fn generate_cell_paragraph(out: &mut String, para: &Paragraph, cell: &CellParagraphCtx) {
    let style: &ParagraphStyle = &para.style;
    let alignment = style.alignment;
    let align_str: Option<&str> = match alignment {
        Some(Alignment::Left) => Some("left"),
        Some(Alignment::Center) => Some("center"),
        Some(Alignment::Right) => Some("right"),
        _ => None,
    };
    let line_height_settings: Option<String> = if cell.uses_powerpoint_line_box {
        // A slide's table cell paces on PowerPoint's flat 1.2em line, the same
        // model its own text boxes use, not on Word's hhea line (issue #663).
        powerpoint_line_height_settings(&para.runs, style)
    } else {
        // Off-slide, table-cell text occupies the font's full single-spacing
        // (hhea) line as a fixed box: a single-line cell must fill the whole
        // line height Word gives it rather than only the tighter metric box,
        // or auto-height rows come out short (issue #396). A cell whose *row*
        // holds East Asian text takes 1.3 times that line, like body text, and
        // a snapping grid's pitch above it — decided once per row so every
        // cell in it shares a baseline, the numeric ones included
        // (issues #498, #518).
        word_cell_line_box_settings(
            &para.runs,
            style,
            cell.line_grid_pitch,
            cell.row_has_east_asian_text,
            cell.seats_text_on_descender,
        )
    };
    // Whichever fixed edges the block wrapper below puts in force — the
    // computed cell line box, or the paragraph's own `LineBox` — is what a
    // framed eojeol has to restore inside itself (issue #626). The two are
    // mutually exclusive: `word_cell_line_box` bails on a paragraph that
    // declares a `LineBox`.
    let cell_line_box_em: Option<(f64, f64)> = word_cell_line_box(
        &para.runs,
        style,
        cell.line_grid_pitch,
        cell.row_has_east_asian_text,
        cell.seats_text_on_descender,
    )
    .map(|line_box| (line_box.top_em, line_box.bottom_em))
    .or_else(|| {
        style
            .line_box
            .map(|line_box| (line_box.ascent_em, line_box.descent_em))
    });
    // An empty `<w:p>` has no runs, so it resolves no line box above and would
    // otherwise emit nothing at all — zero height, where Word gives the
    // paragraph mark a full blank line (issue #625). Size that line from the
    // neighbours' metrics and hold it with a zero-width strut, the same shape
    // the spill wrapper uses. This mirrors the body path's `#v` branch for an
    // empty paragraph, at the cell's fixed line box instead of a flat 12pt.
    //
    // The blank line has to come from the same model as its neighbours, or a
    // slide's empty cell keeps Word's hhea height while the cell beside it
    // takes PowerPoint's 1.2em one (issue #663).
    let paragraph_mark_line_pt: Option<f64> = cell.paragraph_mark_metric_runs.and_then(|runs| {
        if cell.uses_powerpoint_line_box {
            powerpoint_line_box_pt(runs)
        } else {
            word_cell_line_box(
                runs,
                style,
                cell.line_grid_pitch,
                cell.row_has_east_asian_text,
                cell.seats_text_on_descender,
            )
            .map(|line_box| (line_box.top_em + line_box.bottom_em) * line_box.font_size_pt)
        }
    });
    // Typst's default block spacing may only be dropped where this paragraph
    // supplies a fixed line box of its own, which carries the whole advance;
    // adding Typst's gap on top would count the line twice. A paragraph that
    // resolves no box — an unknown face, say — keeps the default, or zeroing
    // its wrapper would leave it no vertical separation at all and collapse the
    // stack onto itself. A `w:spacing w:line` used to land in that second case
    // and advance short of Word for want of a box; it now scales one
    // (issue #727).
    let emits_fixed_line_box: bool =
        line_height_settings.is_some() || paragraph_mark_line_pt.is_some();
    let suppress_default_block_spacing: bool = cell.stacks_multiple_blocks && emits_fixed_line_box;
    // Word's `w:ind` offsets a paragraph's whole column wherever the paragraph
    // sits, cells included, so it rides the cell wrapper as an inset the same
    // way the body path puts it on its own outer block (issues #464, #938).
    // Unlike the body path this needs no separate outer block: a cell paragraph
    // paints no `w:shd` or `w:pBdr` of its own, so nothing has to span the
    // un-inset width.
    let indent: Option<(f64, f64)> = paragraph_indent_pt(style);
    let has_block_wrapper = cell_paragraph_needs_block_wrapper(style)
        || align_str.is_some()
        || line_height_settings.is_some()
        || suppress_default_block_spacing
        || indent.is_some();

    if has_block_wrapper {
        out.push_str("#block(");
        write_cell_paragraph_block_params(
            out,
            align_str.is_some() && !cell.in_spill_cell,
            suppress_default_block_spacing,
            indent,
        );
        out.push_str(")[\n");
        write_line_box_settings(out, style.line_box);
        write_par_settings(out, style);
        if let Some(align_str) = align_str {
            let _ = writeln!(out, "  #set align({align_str})");
        }
        if let Some(ref settings) = line_height_settings {
            out.push_str(settings);
        }
    }

    if let Some(space_before) = style.space_before {
        let _ = writeln!(out, "#v({}pt)", format_f64(space_before));
    }

    match paragraph_mark_line_pt {
        Some(height_pt) => {
            let _ = write!(out, "#box(width: 0pt, height: {}pt)", format_f64(height_pt));
        }
        None => {
            let eojeol_wrap = paragraph_eojeol_wrap(
                cell.breaks_hangul_at_eojeol,
                style,
                cell_line_box_em,
                cell.available_measure_pt,
            );
            if cell.uses_powerpoint_line_box {
                generate_powerpoint_runs_with_tabs(
                    out,
                    &para.runs,
                    style,
                    style.tab_stops.as_deref(),
                    paragraph_default_tab_width_pt(style, cell.default_tab_width_pt),
                    eojeol_wrap,
                    cell.available_measure_pt,
                );
            } else {
                generate_runs_with_tabs(
                    out,
                    &para.runs,
                    style.tab_stops.as_deref(),
                    paragraph_default_tab_width_pt(style, cell.default_tab_width_pt),
                    eojeol_wrap,
                );
            }
        }
    }

    // Suppressed when the grid-snapped line box already contains it, or the
    // gap would be counted twice (issues #500, #503).
    // TODO(#625 follow-up: cells compose w:after + w:before additively via
    // strong #v while body flow max-collapses them; Word's in-cell rule is
    // unmeasured — probe before changing).
    if let Some(space_after) = style.space_after
        && !cell_grid_absorbs_space_after(style, cell.line_grid_pitch, cell.row_has_east_asian_text)
    {
        let _ = write!(out, "\n#v({}pt)", format_f64(space_after));
    }

    if has_block_wrapper {
        out.push_str("\n]");
    }
}

fn cell_paragraph_needs_block_wrapper(style: &ParagraphStyle) -> bool {
    style.line_spacing.is_some()
        || style.line_box.is_some()
        || matches!(style.alignment, Some(Alignment::Justify))
        || matches!(style.direction, Some(TextDirection::Rtl))
}

fn write_cell_paragraph_block_params(
    out: &mut String,
    needs_full_width: bool,
    suppress_default_block_spacing: bool,
    indent: Option<(f64, f64)>,
) {
    let mut first = true;

    if needs_full_width {
        write_param(out, &mut first, "width: 100%");
    }
    // Stacked cell paragraphs: the inter-paragraph gap is carried entirely by
    // the explicit `#v(space_before)`/`#v(space_after)` emissions (which are
    // the resolved Word values), so the wrapper must contribute nothing —
    // Typst's default `block` spacing is 1.2em of engine whitespace Word does
    // not have (issue #625). The trailing `#v(space_after)` stays *inside* the
    // block rather than becoming a weak `below:`, because Word counts it into
    // the row height and weak spacing would vanish at the cell's edge.
    if suppress_default_block_spacing {
        write_param(out, &mut first, "above: 0pt");
        write_param(out, &mut first, "below: 0pt");
    }
    // Word's `w:ind`, as the block's own padding: the wrapper shrinks to its
    // content, so a left inset shifts the text right by exactly that much and a
    // right inset takes the same width off the measure the text wraps in
    // (issue #938).
    if let Some((left, right)) = indent {
        write_param(
            out,
            &mut first,
            &format!(
                "inset: (left: {}pt, right: {}pt)",
                format_f64(left),
                format_f64(right)
            ),
        );
    }
}
