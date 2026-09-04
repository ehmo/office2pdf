//! Pagination for overflowing sheet grids and drawing-driven page windows.
//!
//! Excel prints overflow in down-then-over order. Splittable columns and
//! drawings that cross the printable width continue in page-column windows; a
//! single oversized column remains unsplit. Drawings that cross the printable
//! height continue in page-row windows when the cell grid has fixed row heights
//! and fits one printable page. Parser preflight refuses drawing overflow over
//! auto-height, multi-page, or manual-break row flow until those interactions
//! are measured.
//!
//! A drawing-only sheet has no rows or columns to split on, so both axes come
//! from the drawings' extents instead ([`split_drawing_only_page`], issue
//! #713).

use crate::error::ConvertError;
use crate::ir::{
    Alignment, Block, HFInline, HeaderFooter, Insets, SheetChart, SheetImage, SheetPage,
    SheetTextBox, Table, TableCell, TableRow,
};

/// What one sheet's `<pageSetUpPr fitToPage="1"/>` asks pagination to scale it
/// onto. Both directions are bounded separately and Excel obeys the tighter of
/// the two.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct SheetFit {
    /// `fitToWidth` when it binds; `None` leaves the column direction free.
    pub(super) pages_wide: Option<u32>,
    /// `fitToHeight` when it binds; `None` leaves the row direction free.
    pub(super) pages_tall: Option<u32>,
    /// The printed cell-grid height of the *whole* sheet in points. Pagination
    /// combines it with the bottom-most anchored drawing edge before measuring
    /// the row bound. The page handed to pagination may carry one streaming
    /// chunk or one explicit-break segment of that sheet, so its own rows
    /// cannot supply the grid total. Unread when `pages_tall` is `None`.
    pub(super) sheet_height_pt: f64,
}

/// Refuse vertical drawing overflow when its interaction with row pagination
/// is not proved. Fit-to-page scaling is applied before this check so a drawing
/// that the declared bounds bring onto one page remains supported unless it
/// still crosses a manual row break.
pub(super) fn ensure_supported_vertical_drawing_flow(
    page: &SheetPage,
    fit: SheetFit,
    header_footer_scales_with_doc: bool,
    has_manual_row_breaks: bool,
    has_rows_outside_page: bool,
) -> Result<(), ConvertError> {
    if page.charts.is_empty() && page.images.is_empty() && page.text_boxes.is_empty() {
        return Ok(());
    }
    let fitted = fit_page_to_pages(page.clone(), fit, header_footer_scales_with_doc);
    let printable_height: f64 = fitted.size.height - fitted.margins.top - fitted.margins.bottom;
    let table_height: Option<f64> = fitted
        .table
        .rows
        .iter()
        .map(|row| row.height)
        .try_fold(0.0, |sum, height| height.map(|height| sum + height));
    let drawing_bottom: f64 = drawing_bottom_extent(&fitted);
    let crosses_manual_break: bool = has_manual_row_breaks
        && table_height.is_none_or(|table_height| drawing_bottom > table_height);
    if printable_height <= 0.0 || (drawing_bottom <= printable_height && !crosses_manual_break) {
        return Ok(());
    }
    let element = if crosses_manual_break {
        "vertical drawing overflow with manual row breaks"
    } else if table_height.is_none() {
        "vertical drawing overflow with auto-height rows"
    } else if has_rows_outside_page || table_height.is_some_and(|height| height > printable_height)
    {
        "vertical drawing overflow over a multi-page cell grid"
    } else {
        return Ok(());
    };
    Err(ConvertError::UnsupportedElement {
        format: "XLSX",
        element: element.to_string(),
    })
}

/// Refuse visible merged-cell flows the horizontal slicer cannot preserve.
/// One-line left-aligned text is partitioned across page windows. Centered text
/// is safe only when all its ink remains in the first window. Empty merges keep
/// their border and fill. Wrapped text, right alignment, title-column overlap,
/// icons, and data bars refuse instead of returning a misleading PDF.
pub(super) fn ensure_supported_horizontal_merged_cell_flow(
    page: &SheetPage,
    title_columns: Option<(usize, usize)>,
    fit: SheetFit,
    header_footer_scales_with_doc: bool,
) -> Result<(), ConvertError> {
    let fitted = fit_page_to_pages(page.clone(), fit, header_footer_scales_with_doc);
    let printable_width: f64 = fitted.size.width - fitted.margins.left - fitted.margins.right;
    let column_count: usize = fitted.table.column_widths.len();
    let total_width: f64 = fitted.table.column_widths.iter().sum();
    if printable_width <= 0.0 || column_count <= 1 || total_width <= printable_width {
        return Ok(());
    }

    let title_columns: Option<(usize, usize)> = bounded_title_columns(title_columns, column_count);
    let (groups, _): (Vec<(usize, usize)>, f64) =
        page_column_groups(&fitted.table.column_widths, printable_width, title_columns);
    if groups.len() <= 1 {
        return Ok(());
    }
    let boundaries: Vec<usize> = groups
        .iter()
        .take(groups.len() - 1)
        .map(|(_, end)| *end)
        .collect();

    // Rows omit the cells covered by an earlier row-spanning cell. Track
    // those seats so every remaining cell keeps its worksheet column index.
    let mut rowspan_remaining: Vec<usize> = vec![0; column_count];
    for row in &fitted.table.rows {
        let mut column_cursor: usize = 0;
        for cell in &row.cells {
            while column_cursor < column_count && rowspan_remaining[column_cursor] > 0 {
                rowspan_remaining[column_cursor] -= 1;
                column_cursor += 1;
            }
            if column_cursor >= column_count {
                break;
            }
            let cell_start: usize = column_cursor;
            let cell_end: usize = (column_cursor + cell.col_span.max(1) as usize).min(column_count);
            let crosses_boundary: bool = boundaries
                .iter()
                .any(|boundary| cell_start < *boundary && *boundary < cell_end);
            let has_visible_content: bool =
                !cell.content.is_empty() || cell.icon_text.is_some() || cell.data_bar.is_some();
            let overlaps_title_columns: bool =
                title_columns.is_some_and(|(start, end)| cell_start < end && start < cell_end);
            if crosses_boundary
                && has_visible_content
                && (overlaps_title_columns
                    || (!can_continue_left_merged_cell(cell)
                        && !centered_merged_cell_ink_fits_first_window(
                            cell,
                            &fitted.table.column_widths,
                            cell_start,
                            cell_end,
                            &boundaries,
                        )))
            {
                return Err(ConvertError::UnsupportedElement {
                    format: "XLSX",
                    element: "merged cell across a horizontal page break".to_string(),
                });
            }
            if cell.row_span > 1 {
                for occupied in rowspan_remaining.iter_mut().take(cell_end).skip(cell_start) {
                    *occupied = (cell.row_span - 1) as usize;
                }
            }
            column_cursor = cell_end;
        }
        while column_cursor < column_count {
            if rowspan_remaining[column_cursor] > 0 {
                rowspan_remaining[column_cursor] -= 1;
            }
            column_cursor += 1;
        }
    }
    Ok(())
}

fn centered_merged_cell_ink_fits_first_window(
    cell: &TableCell,
    column_widths: &[f64],
    cell_start: usize,
    cell_end: usize,
    boundaries: &[usize],
) -> bool {
    let [Block::Paragraph(paragraph)] = cell.content.as_slice() else {
        return false;
    };
    if paragraph.style.alignment != Some(Alignment::Center)
        || paragraph.runs.iter().any(|run| run.text.contains('\n'))
        || cell.icon_text.is_some()
        || cell.data_bar.is_some()
    {
        return false;
    }
    let Some(boundary) = boundaries
        .iter()
        .copied()
        .find(|boundary| cell_start < *boundary && *boundary < cell_end)
    else {
        return false;
    };
    let merge_width: f64 = column_widths[cell_start..cell_end].iter().sum();
    let first_window_width: f64 = column_widths[cell_start..boundary].iter().sum();
    let padding: Insets = cell.padding.unwrap_or(Insets {
        top: 5.0,
        right: 5.0,
        bottom: 5.0,
        left: 5.0,
    });
    let text_width: f64 = super::xlsx_cells::estimate_text_width_pt(&paragraph.runs);
    let text_center: f64 = (merge_width + padding.left - padding.right) / 2.0;
    text_center + text_width / 2.0 <= first_window_width
}

fn can_continue_left_merged_cell(cell: &TableCell) -> bool {
    let [Block::Paragraph(paragraph)] = cell.content.as_slice() else {
        return false;
    };
    cell.spill_width.is_some()
        && matches!(paragraph.style.alignment, None | Some(Alignment::Left))
        && paragraph.runs.iter().all(|run| !run.text.contains('\n'))
        && cell.icon_text.is_none()
        && cell.data_bar.is_none()
}

fn left_merged_cell_content_window(content: &[Block], start_pt: f64, end_pt: f64) -> Vec<Block> {
    let [Block::Paragraph(paragraph)] = content else {
        return Vec::new();
    };
    let mut paragraph = paragraph.clone();
    let mut cursor_pt: f64 = 0.0;
    paragraph.runs = paragraph
        .runs
        .iter()
        .filter_map(|source_run| {
            let mut run = source_run.clone();
            run.text = source_run
                .text
                .chars()
                .filter(|character| {
                    let glyph_start_pt: f64 = cursor_pt;
                    cursor_pt += super::xlsx_cells::estimate_character_width_pt(
                        *character,
                        source_run.style.font_family.as_deref(),
                        source_run.style.font_size.unwrap_or(11.0),
                    );
                    glyph_start_pt >= start_pt && glyph_start_pt < end_pt
                })
                .collect();
            (!run.text.is_empty()).then_some(run)
        })
        .collect();
    (!paragraph.runs.is_empty())
        .then_some(Block::Paragraph(paragraph))
        .into_iter()
        .collect()
}

/// Fit a sheet, split splittable columns into printable-width groups, then
/// expand each group into printable-height drawing windows when every row has
/// a fixed height and the grid fits one page. A single oversized column remains
/// unsplit. The low-level splitter leaves auto-height or multi-page grids on
/// Typst's vertical flow, but parser preflight refuses drawing overflow before
/// this function receives those cases. `title_columns` is the 0-based
/// inclusive-exclusive range of print-title columns (from
/// `_xlnm.Print_Titles`) repeated at the left of every horizontal overflow
/// page.
pub(super) fn split_sheet_page_by_width(
    page: SheetPage,
    title_columns: Option<(usize, usize)>,
    fit: SheetFit,
    header_footer_scales_with_doc: bool,
) -> Vec<SheetPage> {
    let page: SheetPage = fit_page_to_pages(page, fit, header_footer_scales_with_doc);
    split_sheet_page_by_width_only(page, title_columns)
        .into_iter()
        .flat_map(split_page_by_drawing_height)
        .collect()
}

fn split_sheet_page_by_width_only(
    page: SheetPage,
    title_columns: Option<(usize, usize)>,
) -> Vec<SheetPage> {
    let printable_width: f64 = page.size.width - page.margins.left - page.margins.right;
    let total_width: f64 = page.table.column_widths.iter().sum();
    if total_width <= printable_width {
        return split_fitting_grid_by_drawing_extent(page, printable_width);
    }
    if page.table.column_widths.len() <= 1 {
        return vec![page];
    }

    let title_columns: Option<(usize, usize)> =
        bounded_title_columns(title_columns, page.table.column_widths.len());
    // Reserve the repeated title width so overflow groups still fit the
    // page. The first group holds the title columns physically (they never
    // get prepended to it), so it packs against the full printable width —
    // reserving there too underpacked page 1 by the title width (issue #623
    // adversarial review, finding 3).
    let (groups, title_width): (Vec<(usize, usize)>, f64) =
        page_column_groups(&page.table.column_widths, printable_width, title_columns);
    if groups.len() <= 1 {
        return vec![page];
    }
    let title_table: Option<Table> =
        title_columns.map(|(start, end)| slice_table_columns(&page.table, start, end));

    let mut result: Vec<SheetPage> = Vec::with_capacity(groups.len());
    let mut last_window_left: f64 = 0.0;
    let mut last_window_width: f64 = 0.0;
    let mut last_output_left: f64 = 0.0;
    for (index, &(start, end)) in groups.iter().enumerate() {
        let mut table: Table = slice_table_columns(&page.table, start, end);
        let repeats_title_columns: bool = title_columns
            .map(|(title_start, _)| start > title_start)
            .unwrap_or(false);
        // Excel repeats title columns on pages that no longer show them.
        if let (Some(title_table), Some((title_start, _))) = (title_table.as_ref(), title_columns)
            && start > title_start
        {
            table = prepend_title_columns(title_table, table);
        }
        let window_left: f64 = page.table.column_widths[..start].iter().sum();
        let group_width: f64 = page.table.column_widths[start..end].iter().sum();
        let output_left: f64 = if repeats_title_columns {
            title_width
        } else {
            0.0
        };
        // A page break before another populated column is a hard worksheet
        // boundary. The final populated group has no such boundary, so a
        // drawing can use the rest of the physical printable width before it
        // continues onto drawing-only pages.
        let printable_drawing_width: f64 = (printable_width - output_left).max(0.0);
        let window_width: f64 = if index + 1 == groups.len() {
            printable_drawing_width
        } else {
            group_width.min(printable_drawing_width)
        };
        last_window_left = window_left;
        last_window_width = window_width;
        last_output_left = output_left;
        result.push(SheetPage {
            name: page.name.clone(),
            size: page.size,
            margins: page.margins,
            table,
            header: page.header.clone(),
            footer: page.footer.clone(),
            charts: charts_for_column_group(&page.charts, window_left, window_width, output_left),
            images: images_for_column_group(&page.images, window_left, window_width, output_left),
            text_boxes: text_boxes_for_column_group(
                &page.text_boxes,
                window_left,
                window_width,
                output_left,
            ),
        });
    }

    // The drawing layer can continue after the final populated column group.
    // Those pages repeat print-title columns but contain no ordinary cells.
    let right_extent: f64 = drawing_right_extent(&page);
    let mut window_left: f64 = last_window_left + last_window_width;
    let window_width: f64 = (printable_width - last_output_left).max(0.0);
    let empty_table: Table = if let Some(title_table) = title_table {
        title_table
    } else {
        slice_table_columns(
            &page.table,
            page.table.column_widths.len(),
            page.table.column_widths.len(),
        )
    };
    while window_width > 0.0 && right_extent > window_left {
        result.push(SheetPage {
            name: page.name.clone(),
            size: page.size,
            margins: page.margins,
            table: empty_table.clone(),
            header: page.header.clone(),
            footer: page.footer.clone(),
            charts: charts_for_column_group(
                &page.charts,
                window_left,
                window_width,
                last_output_left,
            ),
            images: images_for_column_group(
                &page.images,
                window_left,
                window_width,
                last_output_left,
            ),
            text_boxes: text_boxes_for_column_group(
                &page.text_boxes,
                window_left,
                window_width,
                last_output_left,
            ),
        });
        window_left += window_width;
    }
    result
}

/// Continue drawings through printable-height windows when the cell grid fits
/// one page and every row has a fixed height. The low-level fallback leaves a
/// multi-page grid or unknown row height on Typst's flow pagination; parser
/// preflight refuses drawing overflow over those unproved row interactions.
fn split_page_by_drawing_height(page: SheetPage) -> Vec<SheetPage> {
    let printable_height: f64 = page.size.height - page.margins.top - page.margins.bottom;
    if printable_height <= 0.0 {
        return vec![page];
    }
    let Some(table_height) = page
        .table
        .rows
        .iter()
        .map(|row| row.height)
        .try_fold(0.0, |sum, height| height.map(|height| sum + height))
    else {
        return vec![page];
    };
    if table_height > printable_height {
        return vec![page];
    }
    let bottom_extent: f64 = drawing_bottom_extent(&page);
    if bottom_extent <= printable_height {
        return vec![page];
    }
    let group_count: usize = (bottom_extent / printable_height).ceil() as usize;
    let mut empty_table: Table = page.table.clone();
    empty_table.rows.clear();
    empty_table.header_row_count = 0;
    empty_table.non_repeating_header_row_count = 0;

    (0..group_count)
        .map(|group| {
            let window_top: f64 = group as f64 * printable_height;
            let mut paged: SheetPage = page.clone();
            if group > 0 {
                paged.table = empty_table.clone();
            }
            paged.charts = charts_for_row_group(&page.charts, window_top, printable_height);
            paged.images = images_for_row_group(&page.images, window_top, printable_height);
            paged.text_boxes =
                text_boxes_for_row_group(&page.text_boxes, window_top, printable_height);
            paged
        })
        .collect()
}

fn charts_for_row_group(
    charts: &[SheetChart],
    window_top: f64,
    window_height: f64,
) -> Vec<SheetChart> {
    charts
        .iter()
        .filter_map(|chart| {
            let Some(placement) = chart.placement else {
                return (window_top == 0.0).then(|| chart.clone());
            };
            let bottom: f64 = placement.y_offset_pt + placement.height * placement.print_scale;
            (bottom > window_top && placement.y_offset_pt < window_top + window_height).then(|| {
                let mut paged = chart.clone();
                paged
                    .placement
                    .as_mut()
                    .expect("a placed chart keeps its placement")
                    .y_offset_pt -= window_top;
                paged
            })
        })
        .collect()
}

fn images_for_row_group(
    images: &[SheetImage],
    window_top: f64,
    window_height: f64,
) -> Vec<SheetImage> {
    images
        .iter()
        .filter(|image| {
            let bottom: f64 = image.y_offset_pt + image.image.height.unwrap_or(0.0);
            bottom > window_top && image.y_offset_pt < window_top + window_height
        })
        .map(|image| {
            let mut paged = image.clone();
            paged.y_offset_pt -= window_top;
            paged
        })
        .collect()
}

fn text_boxes_for_row_group(
    text_boxes: &[SheetTextBox],
    window_top: f64,
    window_height: f64,
) -> Vec<SheetTextBox> {
    text_boxes
        .iter()
        .filter(|text_box| {
            let bottom: f64 = text_box.y_offset_pt + text_box.height * text_box.print_scale;
            bottom > window_top && text_box.y_offset_pt < window_top + window_height
        })
        .map(|text_box| {
            let mut paged = text_box.clone();
            paged.y_offset_pt -= window_top;
            paged
        })
        .collect()
}

/// Add printable-width page-columns when the cell grid fits but a drawing
/// extends beyond it.
///
/// The first page keeps the complete grid. Later page-columns keep the same
/// row geometry but no cells, because their only printable content is the
/// continued drawing. The fixed-width windows matter here: the populated
/// grid may stop at 100pt while the physical page boundary remains at 400pt.
fn split_fitting_grid_by_drawing_extent(page: SheetPage, printable_width: f64) -> Vec<SheetPage> {
    if printable_width <= 0.0 {
        return vec![page];
    }
    let right_extent: f64 = drawing_right_extent(&page);
    if right_extent <= printable_width {
        return vec![page];
    }
    let group_count: usize = ((right_extent / printable_width).ceil() as usize).max(2);
    let empty_table: Table = slice_table_columns(
        &page.table,
        page.table.column_widths.len(),
        page.table.column_widths.len(),
    );

    (0..group_count)
        .map(|group| {
            let window_left: f64 = group as f64 * printable_width;
            let mut paged: SheetPage = page.clone();
            if group > 0 {
                paged.table = empty_table.clone();
            }
            paged.charts = charts_for_column_group(&page.charts, window_left, printable_width, 0.0);
            paged.images = images_for_column_group(&page.images, window_left, printable_width, 0.0);
            paged.text_boxes =
                text_boxes_for_column_group(&page.text_boxes, window_left, printable_width, 0.0);
            paged
        })
        .collect()
}

/// Concatenate the repeated title columns before a column group's table.
/// Shrink a sheet until it fits the pages `fitToWidth` and `fitToHeight`
/// allow.
///
/// A sheet with `<pageSetUpPr fitToPage="1"/>` and `fitToWidth="1"` asks Excel
/// to scale it onto one page wide rather than to spill the overflow onto a
/// second strip. Reading neither attribute printed the repository workbook on
/// 53 pages where Excel prints 23 (issue #530).
///
/// `fitToHeight` bounds the row direction the same way, and ECMA-376 defaults
/// it to one page just as it defaults `fitToWidth`, so a sheet naming neither
/// is asking to be squeezed onto a single page both ways. Excel obeys the
/// tighter of the two bounds: the reported college-budget workbook fits A3's
/// width at 0.89 and its height at 0.78, and its native export is one page at
/// 0.78 (issue #1181).
///
/// Excel scales the whole sheet, not the columns alone, so the row heights and
/// the type scale with the widths — the audited sheet's 10pt body text prints
/// at 7.50pt, the same 0.75 the columns take.
///
/// Excel never scales *up* to fill a page, so a sheet that already fits is
/// left alone.
fn fit_page_to_pages(
    page: SheetPage,
    fit: SheetFit,
    header_footer_scales_with_doc: bool,
) -> SheetPage {
    let printable_width: f64 = page.size.width - page.margins.left - page.margins.right;
    // Excel includes printable drawing objects in the fitted sheet extent.
    // Using only the populated cells can choose a scale that fits the grid but
    // still sends an anchored chart, picture, or text box onto extra pages.
    let total_width: f64 = page
        .table
        .column_widths
        .iter()
        .sum::<f64>()
        .max(drawing_right_extent(&page));
    let printable_height: f64 = page.size.height - page.margins.top - page.margins.bottom;
    let total_height: f64 = fit.sheet_height_pt.max(drawing_bottom_extent(&page));
    let Some(scale) = [
        fit_scale(fit.pages_wide, printable_width, total_width),
        fit_scale(fit.pages_tall, printable_height, total_height),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min) else {
        return page;
    };
    if scale >= 1.0 {
        return page;
    }
    scale_sheet_page(page, scale, header_footer_scales_with_doc)
}

/// The scale that fits `total_pt` of sheet into `pages` pages of `printable_pt`,
/// or `None` when that direction is unconstrained — either unbounded by the
/// file (a declared zero is Excel's "as many pages as it takes") or unmeasurable.
///
/// Excel's auto-fit scale is a whole percent, truncated rather than rounded so
/// the content is guaranteed to fit. Keeping the raw ratio leaves every derived
/// type size a fraction of a point off the printed sheet — the audited sheet
/// came out at 7.55pt against Excel's 7.50pt.
fn fit_scale(pages: Option<u32>, printable_pt: f64, total_pt: f64) -> Option<f64> {
    let pages: u32 = pages.filter(|pages| *pages > 0)?;
    if printable_pt <= 0.0 || total_pt <= 0.0 {
        return None;
    }
    let exact_scale: f64 = (printable_pt * f64::from(pages)) / total_pt;
    Some((exact_scale * 100.0).floor() / 100.0)
}

/// Multiply a sheet's widths, heights, type sizes, cell padding, and anchored
/// drawings by `scale`.
///
/// Padding has to scale with the rest: it is a fixed per-row overhead, so
/// leaving it at full size while the rows shrink costs a constant slice of
/// every row and accumulates into whole extra pages over a long sheet.
///
/// The header and footer scale too, unless the sheet opts out.
/// `headerFooter/@scaleWithDoc` defaults to 1 (ECMA-376 §18.3.1.46), so Excel
/// shrinks them with the sheet; leaving them at full size printed the Gantt
/// template's 8pt `&8` run beside 5.85pt body text (issue #940).
fn scale_sheet_page(
    mut page: SheetPage,
    scale: f64,
    header_footer_scales_with_doc: bool,
) -> SheetPage {
    if header_footer_scales_with_doc {
        for header_footer in [page.header.as_mut(), page.footer.as_mut()]
            .into_iter()
            .flatten()
        {
            scale_header_footer_font_sizes(header_footer, scale);
        }
    }
    // Every size below is multiplied by the scale outright. The factor itself
    // rides on the table because a rule Excel evaluates at the declared size
    // and scales afterwards cannot be recovered from the products — the
    // wrapped-line advance of issue #1163 is one.
    page.table.print_scale = Some(page.table.print_scale.unwrap_or(1.0) * scale);
    for width in &mut page.table.column_widths {
        *width *= scale;
    }
    // An anchored chart is measured against the sheet's own columns and rows,
    // so the scale that shrinks those has to shrink the chart with them —
    // otherwise the fitted grid slides out from under a full-size chart. On
    // the reported workbook the 0.82 scale left the chart 183pt wider than
    // the band it is anchored to, spilling past the printable edge (#982).
    //
    // The scale rides on the placement rather than being folded into its frame
    // because Excel shrinks the whole drawing, not just the box around it: the
    // chart's tick labels, category labels and legend scale with it. Shrinking
    // the frame alone printed them at the size the chart XML declares, about
    // 22% larger than the native export's (#1069).
    for placement in page
        .charts
        .iter_mut()
        .filter_map(|chart| chart.placement.as_mut())
    {
        placement.x_offset_pt *= scale;
        placement.y_offset_pt *= scale;
        placement.print_scale *= scale;
    }
    // A picture is anchored to the same columns and rows, so it shrinks with
    // them too. Scaling the grid alone printed the reported workbook's photo
    // at 234.95 x 171.05pt against the native export's 192.66 x 140.26 —
    // 1/0.82 in each axis, the print scale never reaching it — and 84.83pt
    // further down the page (#1111).
    //
    // The scale goes into the frame rather than riding beside it as the
    // chart's does: a picture carries no text of its own, so there is nothing
    // in it that a plain resize would leave at full size.
    for image in &mut page.images {
        image.x_offset_pt *= scale;
        image.y_offset_pt *= scale;
        if let Some(width) = image.image.width.as_mut() {
            *width *= scale;
        }
        if let Some(height) = image.image.height.as_mut() {
            *height *= scale;
        }
    }
    // Text boxes carry text, padding, fill and a border, so their scale stays
    // beside the full-size frame just as a chart's does. The renderer can then
    // shrink the complete drawing instead of leaving full-size text in a
    // smaller box.
    for text_box in &mut page.text_boxes {
        text_box.x_offset_pt *= scale;
        text_box.y_offset_pt *= scale;
        text_box.print_scale *= scale;
    }
    for row in &mut page.table.rows {
        if let Some(height) = row.height.as_mut() {
            *height *= scale;
        }
        for cell in &mut row.cells {
            if let Some(padding) = cell.padding.as_mut() {
                padding.top *= scale;
                padding.right *= scale;
                padding.bottom *= scale;
                padding.left *= scale;
            }
            for block in &mut cell.content {
                scale_block_font_sizes(block, scale);
            }
        }
    }
    page
}

/// Scale every run of a header or footer.
///
/// A run that states no size takes the renderer's default rather than being
/// left alone: it is the size the run actually prints at, and skipping it left
/// the Gantt template's leading `_x000D_` at 11pt while everything around it
/// shrank (issue #940).
fn scale_header_footer_font_sizes(header_footer: &mut HeaderFooter, scale: f64) {
    for paragraph in &mut header_footer.paragraphs {
        for element in &mut paragraph.elements {
            if let HFInline::Run(run) = element {
                let size_pt: f64 = run
                    .style
                    .font_size
                    .unwrap_or(crate::defaults::TYPST_DEFAULT_FONT_SIZE_PT);
                run.style.font_size = Some(size_pt * scale);
            }
        }
    }
}

fn scale_block_font_sizes(block: &mut Block, scale: f64) {
    match block {
        Block::Paragraph(paragraph) => {
            for run in &mut paragraph.runs {
                if let Some(size) = run.style.font_size.as_mut() {
                    *size *= scale;
                }
            }
        }
        Block::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    for nested in &mut cell.content {
                        scale_block_font_sizes(nested, scale);
                    }
                }
            }
        }
        _ => {}
    }
}

fn prepend_title_columns(title_table: &Table, group_table: Table) -> Table {
    let mut column_widths: Vec<f64> = title_table.column_widths.clone();
    column_widths.extend(group_table.column_widths.iter().copied());

    let rows: Vec<TableRow> = title_table
        .rows
        .iter()
        .zip(group_table.rows)
        .map(|(title_row, group_row)| {
            let mut cells: Vec<TableCell> = title_row.cells.clone();
            cells.extend(group_row.cells);
            TableRow {
                minimum_height: None,
                cells,
                height: group_row.height,
            }
        })
        .collect();

    Table {
        rows,
        column_widths,
        ..group_table
    }
}

/// Greedily pack columns left-to-right into groups whose summed width fits
/// their capacity; every group holds at least one column. The first group
/// packs against `first_group_width` (the full printable width — it shows
/// the title columns in place); later groups pack against
/// `overflow_group_width`, which reserves room for the prepended titles.
fn column_groups(
    column_widths: &[f64],
    first_group_width: f64,
    overflow_group_width: f64,
) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut start: usize = 0;
    let mut acc: f64 = 0.0;
    for (index, width) in column_widths.iter().enumerate() {
        let capacity: f64 = if groups.is_empty() {
            first_group_width
        } else {
            overflow_group_width
        };
        if index > start && acc + width > capacity {
            groups.push((start, index));
            start = index;
            acc = 0.0;
        }
        acc += width;
    }
    groups.push((start, column_widths.len()));
    groups
}

fn bounded_title_columns(
    title_columns: Option<(usize, usize)>,
    column_count: usize,
) -> Option<(usize, usize)> {
    title_columns
        .map(|(start, end)| (start, end.min(column_count)))
        .filter(|(start, end)| start < end)
}

/// Calculate the exact column groups used by both parser preflight and the
/// renderer-facing slicer. Keeping this in one function prevents a supported
/// preflight case from later crossing a different render boundary.
fn page_column_groups(
    column_widths: &[f64],
    printable_width: f64,
    title_columns: Option<(usize, usize)>,
) -> (Vec<(usize, usize)>, f64) {
    let title_width: f64 = title_columns
        .map(|(start, end)| column_widths[start..end].iter().sum())
        .unwrap_or(0.0);
    let widest_column: f64 = column_widths.iter().copied().fold(0.0, f64::max);
    let groups = column_groups(
        column_widths,
        printable_width.max(widest_column),
        (printable_width - title_width).max(widest_column),
    );
    (groups, title_width)
}

/// Copy each image to every page-column window it intersects. Coordinates on
/// the copy are relative to that page's table, after any repeated print-title
/// columns, and the renderer clips the image to the window instead of drawing
/// the overlapping portion twice.
fn images_for_column_group(
    images: &[SheetImage],
    window_left: f64,
    window_width: f64,
    output_left: f64,
) -> Vec<SheetImage> {
    let window_right: f64 = window_left + window_width;
    images
        .iter()
        .filter(|image| match image.image.width {
            Some(width) if width > 0.0 => {
                image.x_offset_pt + width > window_left && image.x_offset_pt < window_right
            }
            _ => image.x_offset_pt >= window_left && image.x_offset_pt < window_right,
        })
        .map(|image| {
            let mut paged_image: SheetImage = image.clone();
            paged_image.x_offset_pt = output_left + image.x_offset_pt - window_left;
            paged_image.clip_left_pt = Some(output_left);
            paged_image.clip_width_pt = Some(window_width);
            paged_image
        })
        .collect()
}

/// Copy each placed chart to every page-column window its printed frame
/// intersects. A chart with no drawing anchor remains flow content on the
/// first page only.
fn charts_for_column_group(
    charts: &[SheetChart],
    window_left: f64,
    window_width: f64,
    output_left: f64,
) -> Vec<SheetChart> {
    let window_right: f64 = window_left + window_width;
    charts
        .iter()
        .filter(|chart| match chart.placement {
            Some(placement) => {
                let width: f64 = placement.width * placement.print_scale;
                placement.x_offset_pt + width > window_left && placement.x_offset_pt < window_right
            }
            None => window_left == 0.0,
        })
        .map(|chart| {
            let mut paged_chart: SheetChart = chart.clone();
            if let Some(placement) = paged_chart.placement.as_mut() {
                placement.x_offset_pt = output_left + placement.x_offset_pt - window_left;
                placement.clip_left_pt = Some(output_left);
                placement.clip_width_pt = Some(window_width);
            }
            paged_chart
        })
        .collect()
}

/// Copy each text box to every page-column window its frame intersects.
fn text_boxes_for_column_group(
    text_boxes: &[SheetTextBox],
    window_left: f64,
    window_width: f64,
    output_left: f64,
) -> Vec<SheetTextBox> {
    let window_right: f64 = window_left + window_width;
    text_boxes
        .iter()
        .filter(|text_box| {
            text_box.x_offset_pt + text_box.width * text_box.print_scale > window_left
                && text_box.x_offset_pt < window_right
        })
        .map(|text_box| {
            let mut paged_text_box: SheetTextBox = text_box.clone();
            paged_text_box.x_offset_pt = output_left + text_box.x_offset_pt - window_left;
            paged_text_box.clip_left_pt = Some(output_left);
            paged_text_box.clip_width_pt = Some(window_width);
            paged_text_box
        })
        .collect()
}

fn image_right_extent(images: &[SheetImage]) -> f64 {
    images
        .iter()
        .map(|image| image.x_offset_pt + image.image.width.unwrap_or(0.0))
        .fold(0.0, f64::max)
}

fn drawing_right_extent(page: &SheetPage) -> f64 {
    let chart_extent: f64 = page
        .charts
        .iter()
        .filter_map(|chart| chart.placement)
        .map(|placement| placement.x_offset_pt + placement.width * placement.print_scale)
        .fold(0.0, f64::max);
    let text_box_extent: f64 = page
        .text_boxes
        .iter()
        .map(|text_box| text_box.x_offset_pt + text_box.width * text_box.print_scale)
        .fold(0.0, f64::max);
    image_right_extent(&page.images)
        .max(chart_extent)
        .max(text_box_extent)
}

fn drawing_bottom_extent(page: &SheetPage) -> f64 {
    let chart_extent: f64 = page
        .charts
        .iter()
        .filter_map(|chart| chart.placement)
        .map(|placement| placement.y_offset_pt + placement.height * placement.print_scale)
        .fold(0.0, f64::max);
    let image_extent: f64 = page
        .images
        .iter()
        .map(|image| image.y_offset_pt + image.image.height.unwrap_or(0.0))
        .fold(0.0, f64::max);
    let text_box_extent: f64 = page
        .text_boxes
        .iter()
        .map(|text_box| text_box.y_offset_pt + text_box.height * text_box.print_scale)
        .fold(0.0, f64::max);
    image_extent.max(chart_extent).max(text_box_extent)
}

/// Build a table containing only columns `[start, end)`, truncating cell spans
/// at the group boundary. One-line left-aligned merged text is partitioned by
/// glyph start so its selectable characters occur once across all windows.
/// Centered text that fits wholly before the first boundary keeps its original
/// seat through an adjusted inset. Empty merges retain their border and fill;
/// parser preflight refuses other visible cross-window constructs.
fn slice_table_columns(table: &Table, start: usize, end: usize) -> Table {
    let column_count: usize = table.column_widths.len();
    // Tracks rows still covered by a row-spanning cell, per column.
    let mut rowspan_remaining: Vec<usize> = vec![0; column_count];

    let mut rows: Vec<TableRow> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut column_cursor: usize = 0;
        let mut cells: Vec<TableCell> = Vec::new();

        for cell in &row.cells {
            while column_cursor < column_count && rowspan_remaining[column_cursor] > 0 {
                rowspan_remaining[column_cursor] -= 1;
                column_cursor += 1;
            }
            if column_cursor >= column_count {
                break;
            }

            let span: usize = cell.col_span.max(1) as usize;
            let cell_start: usize = column_cursor;
            let cell_end: usize = (column_cursor + span).min(column_count);

            if cell.row_span > 1 {
                for occupied in rowspan_remaining.iter_mut().take(cell_end).skip(cell_start) {
                    *occupied = (cell.row_span - 1) as usize;
                }
            }

            let overlap_start: usize = cell_start.max(start);
            let overlap_end: usize = cell_end.min(end);
            if overlap_start < overlap_end {
                let mut sliced: TableCell = cell.clone();
                sliced.col_span = (overlap_end - overlap_start) as u32;
                let crosses_group_edge: bool = cell_start < start || cell_end > end;
                if crosses_group_edge && can_continue_left_merged_cell(cell) {
                    let default_padding: Insets = table.default_cell_padding.unwrap_or(Insets {
                        top: 5.0,
                        right: 5.0,
                        bottom: 5.0,
                        left: 5.0,
                    });
                    let original_padding: Insets = cell.padding.unwrap_or(default_padding);
                    let window_start_pt: f64 =
                        table.column_widths[cell_start..overlap_start].iter().sum();
                    let window_end_pt: f64 =
                        table.column_widths[cell_start..overlap_end].iter().sum();
                    sliced.content = left_merged_cell_content_window(
                        &cell.content,
                        (window_start_pt - original_padding.left).max(0.0),
                        (window_end_pt - original_padding.left).max(0.0),
                    );
                    let available: f64 =
                        table.column_widths[overlap_start..overlap_end].iter().sum();
                    sliced.spill_width = (!sliced.content.is_empty()).then_some(available);
                    if cell_start < start {
                        sliced.padding = Some(Insets {
                            left: 0.0,
                            ..original_padding
                        });
                    }
                } else if cell_end > end
                    && centered_merged_cell_ink_fits_first_window(
                        cell,
                        &table.column_widths,
                        cell_start,
                        cell_end,
                        &[end],
                    )
                {
                    let default_padding: Insets = table.default_cell_padding.unwrap_or(Insets {
                        top: 5.0,
                        right: 5.0,
                        bottom: 5.0,
                        left: 5.0,
                    });
                    let original_padding: Insets = cell.padding.unwrap_or(default_padding);
                    let full_width: f64 = table.column_widths[cell_start..cell_end].iter().sum();
                    let available: f64 =
                        table.column_widths[overlap_start..overlap_end].iter().sum();
                    sliced.padding = Some(Insets {
                        left: original_padding.left + full_width - available,
                        ..original_padding
                    });
                    sliced.spill_width = Some(available);
                } else if cell_start < start {
                    // A non-text continuation is blanked. Parser preflight
                    // refuses visible unsupported cases before this slicer.
                    sliced.content = Vec::new();
                    sliced.spill_width = None;
                } else if let Some(spill) = sliced.spill_width {
                    // The spill width was measured against the whole sheet, so
                    // it can reach far past the columns this page actually
                    // carries — on a sheet wide enough to split, past the paper
                    // edge, losing the ink entirely (#631). Clamp it to what
                    // remains of the group from this cell's left edge.
                    let available: f64 = table.column_widths[overlap_start..end].iter().sum();
                    sliced.spill_width = Some(spill.min(available));
                }
                cells.push(sliced);
            }

            column_cursor = cell_end;
        }

        // Columns occupied only by rowspans still need their counters advanced.
        while column_cursor < column_count {
            if rowspan_remaining[column_cursor] > 0 {
                rowspan_remaining[column_cursor] -= 1;
            }
            column_cursor += 1;
        }

        rows.push(TableRow {
            minimum_height: None,
            cells,
            height: row.height,
        });
    }

    Table {
        rows,
        column_widths: table.column_widths[start..end].to_vec(),
        header_row_count: table.header_row_count,
        non_repeating_header_row_count: table.non_repeating_header_row_count,
        alignment: table.alignment,
        default_cell_padding: table.default_cell_padding,
        use_content_driven_row_heights: table.use_content_driven_row_heights,
        default_vertical_align: table.default_vertical_align,
        seats_bottom_aligned_text_on_descender: table.seats_bottom_aligned_text_on_descender,
        bottom_aligned_descent_floor_pt: table.bottom_aligned_descent_floor_pt,
        border_paint_model: table.border_paint_model,
        prints_gridlines: table.prints_gridlines,
        prints_headings: table.prints_headings,
        centers_between_print_margins: table.centers_between_print_margins,
        print_scale: table.print_scale,
    }
}

/// Split a drawing-only sheet into page windows across both printable axes.
///
/// Excel prints a drawing that crosses the printable edge clipped there and
/// continues it on the next page-column. The empty-sheet branch previously
/// emitted one page and let the pictures overflow the right margin — the
/// tomcat of `WithDrawing.xlsx` ran 36pt past the printable edge on a single
/// page where Excel prints two (issue #713). The table is empty, so
/// [`split_sheet_page_by_width`] has no column widths to split on; the
/// drawings' extents drive the paging instead.
///
/// On a horizontal split, every placed drawing carries its page-column clip
/// window. The renderer applies the page-row clip to every page, and a
/// continued copy can carry a negative `x_offset_pt`, `y_offset_pt`, or both.
pub(super) fn split_drawing_only_page(page: SheetPage, fit: SheetFit) -> Vec<SheetPage> {
    let page: SheetPage = fit_page_to_pages(page, fit, true);
    split_drawing_only_page_by_width(page)
        .into_iter()
        .flat_map(split_page_by_drawing_height)
        .collect()
}

fn split_drawing_only_page_by_width(page: SheetPage) -> Vec<SheetPage> {
    let printable_width: f64 = page.size.width - page.margins.left - page.margins.right;
    if printable_width <= 0.0 {
        return vec![page];
    }
    let right_extent: f64 = drawing_right_extent(&page);
    if right_extent <= printable_width {
        return vec![page];
    }
    let group_count: usize = ((right_extent / printable_width).ceil() as usize).max(2);

    (0..group_count)
        .map(|group| {
            let window_left: f64 = group as f64 * printable_width;
            let mut paged: SheetPage = page.clone();
            paged.charts = charts_for_column_group(&page.charts, window_left, printable_width, 0.0);
            paged.images = images_for_column_group(&page.images, window_left, printable_width, 0.0);
            paged.text_boxes =
                text_boxes_for_column_group(&page.text_boxes, window_left, printable_width, 0.0);
            paged
        })
        .collect()
}

#[cfg(test)]
#[path = "xlsx_pagination_tests.rs"]
mod tests;
