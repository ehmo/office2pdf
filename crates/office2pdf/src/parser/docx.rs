use std::collections::HashMap;
use std::io::Read;

use crate::config::ConvertOptions;
use crate::error::{ConvertError, ConvertWarning};

/// Maximum nesting depth for tables-within-tables.  Deeper nesting is silently
/// truncated to prevent stack overflow on pathological documents.
const MAX_TABLE_DEPTH: usize = 64;
use crate::ir::{
    Alignment, Block, BorderLineStyle, BorderSide, Caption, CellBorder, CellVerticalAlign, Color,
    ColumnLayout, Document, FloatingImage, FloatingTextBox, ImageData, ImageFormat,
    ImageParagraphSpacing, Insets, LineSpacing, Page, PageNumbering, PairKerning, Paragraph,
    ParagraphStyle, Run, StyleSheet, TabAlignment, TabLeader, TabStop, Table, TableCell,
    TableOfContents, TableRow, TextDirection, TextStyle, VerticalTextAlign,
};
use crate::parser::Parser;

#[cfg(test)]
use self::contexts::scan_table_headers;
use self::contexts::{
    BidiContext, ChartContext, ContextualSpacingContext, DocxConversionContext,
    DrawingShapeContext, DrawingTextBoxContext, DrawingTextBoxInfo, FieldContext, MathContext,
    NoteContent, NoteContext, ParagraphContextualSpacing, ParagraphShadingContext,
    SmallCapsContext, TableHeaderContext, TableStyleContext, VmlTextBoxContext, VmlTextBoxInfo,
    WordWrapContext, WpgDrawingInfo, WrapContext, build_chart_context_from_xml,
    build_math_context_from_xml, build_note_context_from_xml, build_wrap_context_from_xml,
    extract_column_layout_from_section_property, is_note_reference_run, read_zip_text,
    scan_column_layouts, scan_page_numbering, scan_style_paragraph_shading, scan_style_word_wrap,
    seq_identifier, toc_caption_identifier, toc_heading_depth,
};
use self::lists::{
    NumberingMap, TaggedElement, build_numbering_map, extract_num_info, group_into_lists,
};
use self::media::{
    extract_drawing_image, extract_drawing_text_box_blocks, extract_shape_image,
    extract_vml_shape_text_box,
};
#[cfg(test)]
use self::sections::extract_page_size;
use self::sections::{
    HeaderFooterAssets, SectionOverrides, build_flow_page_from_section, build_header_footer_assets,
};
use self::styles::{
    DOC_DEFAULT_STYLE_ID, PairKerningRules, ResolvedStyle, StyleMap, TabStopOverride,
    apply_tab_stop_overrides, build_style_map, get_paragraph_style_id, merge_paragraph_style,
    merge_text_style, resolve_doc_default_text_style,
};
use self::tables::convert_table;
use self::text::{
    ThemeFonts, extract_doc_default_paragraph_style, extract_doc_default_text_style_with_theme,
    extract_paragraph_style, extract_run_style, extract_run_style_id, extract_run_text,
    extract_run_text_skip_layout_breaks, extract_tab_stop_overrides, insert_east_asian_auto_space,
    is_column_break, is_page_break, pair_kerning_from_half_points, parse_hex_color,
    parse_theme_fonts, resolve_hyperlink_url, resolve_theme_font_family,
};
#[cfg(test)]
use self::text::{extract_pair_kerning, extract_tab_stops, resolve_highlight_color};

#[path = "docx_contexts.rs"]
mod contexts;
#[path = "docx_lists.rs"]
mod lists;
#[path = "docx_media.rs"]
mod media;
#[path = "docx_sections.rs"]
mod sections;
#[path = "docx_styles.rs"]
mod styles;
#[path = "docx_tables.rs"]
mod tables;
#[path = "docx_text.rs"]
mod text;

/// Parser for DOCX (Office Open XML Word) documents.
pub struct DocxParser;

/// The paragraph spacing Word applies when neither a paragraph nor its
/// style hierarchy specifies `w:spacing w:after`: ECMA-376 leaves the gap
/// at zero, and a Word PDF export of a document whose `styles.xml` defines
/// no `Normal` spacing confirms it. Recording it explicitly (rather than
/// leaving `space_after` unset) also pins the paragraph block's `below`, so
/// Typst's own 1.2em default block spacing cannot leak into the gap.
///
/// Line height is left to the renderer, which derives Word's single-spacing
/// pitch from the actual font metrics (issues #354, #452).
pub(super) const WORD_COMPATIBLE_PARAGRAPH_SPACE_AFTER_PT: f64 = 0.0;

fn apply_word_compatible_paragraph_defaults(style: &mut ParagraphStyle) {
    style
        .space_after
        .get_or_insert(WORD_COMPATIBLE_PARAGRAPH_SPACE_AFTER_PT);
}

#[derive(Clone)]
struct DocxImageAsset {
    data: Vec<u8>,
    format: ImageFormat,
}

/// Map from relationship ID to normalized image assets.
type ImageMap = HashMap<String, DocxImageAsset>;

/// Map from relationship ID → hyperlink URL.
type HyperlinkMap = HashMap<String, String>;

/// Build a lookup map from the DOCX's hyperlinks (reader-populated field).
/// The reader stores hyperlinks as `(rid, url, type)` in `docx.hyperlinks`.
fn build_hyperlink_map(docx: &docx_rs::Docx) -> HyperlinkMap {
    docx.hyperlinks
        .iter()
        .map(|(rid, url, _type)| (rid.clone(), url.clone()))
        .collect()
}

/// Build a lookup map from the DOCX's embedded images.
/// docx-rs converts all images to PNG; we use the PNG bytes.
fn build_image_map(docx: &docx_rs::Docx) -> ImageMap {
    docx.images
        .iter()
        .map(|(id, _path, _image, png)| {
            (
                id.clone(),
                DocxImageAsset {
                    data: png.0.clone(),
                    format: ImageFormat::Png,
                },
            )
        })
        .collect()
}

fn build_document_metafile_image_map<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> ImageMap {
    let Some(relationships_xml) = read_zip_text(archive, "word/_rels/document.xml.rels") else {
        return ImageMap::new();
    };
    let mut reader = quick_xml::Reader::from_str(&relationships_xml);
    let mut relationships: Vec<(String, String)> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref element))
            | Ok(quick_xml::events::Event::Empty(ref element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                let mut id: Option<String> = None;
                let mut target: Option<String> = None;
                let mut is_image: bool = false;
                for attribute in element.attributes().flatten() {
                    let Ok(value) = attribute.unescape_value() else {
                        continue;
                    };
                    match attribute.key.local_name().as_ref() {
                        b"Id" => id = Some(value.to_string()),
                        b"Target" => target = Some(value.to_string()),
                        b"Type" => is_image = value.ends_with("/image"),
                        _ => {}
                    }
                }
                if is_image && let (Some(id), Some(target)) = (id, target) {
                    let lowercase_target: String = target.to_ascii_lowercase();
                    if lowercase_target.ends_with(".emf") || lowercase_target.ends_with(".wmf") {
                        relationships.push((id, target));
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    relationships
        .into_iter()
        .filter_map(|(id, target)| {
            let path = format!("word/{}", target.trim_start_matches('/'));
            let mut data: Vec<u8> = Vec::new();
            archive.by_name(&path).ok()?.read_to_end(&mut data).ok()?;
            let svg: Vec<u8> = if target.to_ascii_lowercase().ends_with(".wmf") {
                crate::parser::wmf::convert_wmf_to_svg(&data)?
            } else {
                crate::parser::emf::convert_emf_to_svg(&data)?
            };
            Some((
                id,
                DocxImageAsset {
                    data: svg,
                    format: ImageFormat::Svg,
                },
            ))
        })
        .collect()
}

/// Pre-parsed assets extracted from the DOCX ZIP archive before docx-rs parsing.
struct ZipPreParseAssets {
    metadata: crate::ir::Metadata,
    ctx: DocxConversionContext,
    math: MathContext,
    chart_ctx: ChartContext,
    column_layouts: Vec<Option<ColumnLayout>>,
    page_numbering: Vec<Option<PageNumbering>>,
    header_footer_assets: HeaderFooterAssets,
    metafile_images: ImageMap,
    theme_fonts: ThemeFonts,
    default_paragraph_style_id: Option<String>,
    style_paragraph_backgrounds: HashMap<String, Color>,
    style_word_wraps: HashMap<String, bool>,
    /// Read from the raw `word/styles.xml` because docx-rs has no field for
    /// `w:kern` (issue #628).
    pair_kerning: PairKerningRules,
}

/// Build all pre-parse contexts from the DOCX ZIP in a single pass.
/// Falls back to empty contexts if the ZIP cannot be opened, letting
/// docx-rs produce a proper parse error downstream.
fn build_zip_preparse_assets(data: &[u8]) -> ZipPreParseAssets {
    match crate::parser::open_zip(data) {
        Ok(mut archive) => {
            let metadata = crate::parser::metadata::extract_metadata_from_zip(&mut archive);
            let doc_xml = read_zip_text(&mut archive, "word/document.xml");
            let styles_xml = read_zip_text(&mut archive, "word/styles.xml");
            let default_paragraph_style_id = styles_xml
                .as_deref()
                .and_then(styles::scan_default_paragraph_style_id);
            let style_paragraph_backgrounds = scan_style_paragraph_shading(styles_xml.as_deref());
            let style_word_wraps = scan_style_word_wrap(styles_xml.as_deref());
            let theme_xml = read_zip_text(&mut archive, "word/theme/theme1.xml");
            let notes = build_note_context_from_xml(doc_xml.as_deref(), &mut archive);
            let wraps = build_wrap_context_from_xml(doc_xml.as_deref());
            let drawing_text_boxes = DrawingTextBoxContext::from_xml(doc_xml.as_deref());
            let drawing_shapes =
                DrawingShapeContext::from_xml_with_theme(doc_xml.as_deref(), theme_xml.as_deref());
            let table_headers = TableHeaderContext::from_xml(doc_xml.as_deref());
            let table_styles =
                TableStyleContext::from_xml(doc_xml.as_deref(), styles_xml.as_deref());
            let vml_text_boxes = VmlTextBoxContext::from_xml(doc_xml.as_deref());
            let math = build_math_context_from_xml(doc_xml.as_deref());
            let chart_ctx = build_chart_context_from_xml(doc_xml.as_deref(), &mut archive);
            let column_layouts = doc_xml
                .as_deref()
                .map(scan_column_layouts)
                .unwrap_or_default();
            let page_numbering = doc_xml
                .as_deref()
                .map(scan_page_numbering)
                .unwrap_or_default();
            let bidi = BidiContext::from_xml(doc_xml.as_deref());
            let small_caps = SmallCapsContext::from_xml(doc_xml.as_deref());
            let header_footer_assets = build_header_footer_assets(&mut archive);
            let metafile_images = build_document_metafile_image_map(&mut archive);
            let ctx = DocxConversionContext {
                notes,
                wraps,
                drawing_text_boxes,
                drawing_shapes,
                table_headers,
                table_styles,
                vml_text_boxes,
                bidi,
                small_caps,
                paragraph_shading: ParagraphShadingContext::from_xml(doc_xml.as_deref()),
                word_wraps: WordWrapContext::from_xml(doc_xml.as_deref()),
                contextual_spacing: ContextualSpacingContext::from_xml(
                    doc_xml.as_deref(),
                    styles_xml.as_deref(),
                    default_paragraph_style_id.as_deref(),
                ),
                fields: FieldContext::default(),
            };
            ZipPreParseAssets {
                metadata,
                ctx,
                math,
                chart_ctx,
                column_layouts,
                page_numbering,
                header_footer_assets,
                metafile_images,
                theme_fonts: theme_xml
                    .as_deref()
                    .map(parse_theme_fonts)
                    .unwrap_or_default(),
                default_paragraph_style_id,
                style_paragraph_backgrounds,
                style_word_wraps,
                pair_kerning: PairKerningRules::from_styles_xml(styles_xml.as_deref()),
            }
        }
        Err(_) => ZipPreParseAssets {
            metadata: crate::ir::Metadata::default(),
            ctx: DocxConversionContext {
                notes: NoteContext::empty(),
                wraps: WrapContext::empty(),
                drawing_text_boxes: DrawingTextBoxContext::from_xml(None),
                drawing_shapes: DrawingShapeContext::from_xml(None),
                table_headers: TableHeaderContext::from_xml(None),
                table_styles: TableStyleContext::from_xml(None, None),
                vml_text_boxes: VmlTextBoxContext::from_xml(None),
                bidi: BidiContext::from_xml(None),
                small_caps: SmallCapsContext::from_xml(None),
                paragraph_shading: ParagraphShadingContext::from_xml(None),
                word_wraps: WordWrapContext::from_xml(None),
                contextual_spacing: ContextualSpacingContext::from_xml(None, None, None),
                fields: FieldContext::default(),
            },
            math: MathContext::empty(),
            chart_ctx: ChartContext::empty(),
            column_layouts: Vec::new(),
            page_numbering: Vec::new(),
            header_footer_assets: HeaderFooterAssets::default(),
            metafile_images: ImageMap::new(),
            theme_fonts: ThemeFonts::default(),
            default_paragraph_style_id: None,
            style_paragraph_backgrounds: HashMap::new(),
            style_word_wraps: HashMap::new(),
            pair_kerning: PairKerningRules::default(),
        },
    }
}

impl Parser for DocxParser {
    fn parse(
        &self,
        data: &[u8],
        _options: &ConvertOptions,
    ) -> Result<(Document, Vec<ConvertWarning>), ConvertError> {
        let default_tab_stop_pt: Option<f64> = extract_default_tab_stop_pt(data);
        let ZipPreParseAssets {
            metadata,
            mut ctx,
            mut math,
            mut chart_ctx,
            column_layouts,
            page_numbering,
            header_footer_assets,
            metafile_images,
            theme_fonts,
            default_paragraph_style_id,
            style_paragraph_backgrounds,
            style_word_wraps,
            pair_kerning,
        } = build_zip_preparse_assets(data);

        let docx = docx_rs::read_docx(data).map_err(|e| {
            crate::parser::parse_err(format!("Failed to parse DOCX (docx-rs): {e}"))
        })?;

        // Populate locale-specific footnote/endnote style IDs from docx styles
        ctx.notes.populate_style_ids(&docx.styles);

        let mut images = build_image_map(&docx);
        images.extend(metafile_images);
        let hyperlinks = build_hyperlink_map(&docx);
        let numberings = build_numbering_map(&docx.numberings);
        let style_map = build_style_map(
            &docx.styles,
            &theme_fonts,
            default_paragraph_style_id.as_deref(),
            &style_paragraph_backgrounds,
            &style_word_wraps,
            &pair_kerning,
        );
        let mut warnings: Vec<ConvertWarning> = Vec::new();

        let mut elements: Vec<TaggedElement> = Vec::new();
        let mut pages: Vec<Page> = Vec::new();
        let mut section_layout_index: usize = 0;
        for (idx, child) in docx.document.children.iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match child {
                docx_rs::DocumentChild::Paragraph(para) => {
                    let mut tagged = vec![convert_paragraph_element(
                        para,
                        &images,
                        &hyperlinks,
                        &style_map,
                        &ctx,
                        &docx.styles,
                    )];
                    // Inject math equations for this body child
                    let eqs = math.take(idx);
                    for eq in eqs {
                        tagged.push(TaggedElement::Plain(vec![Block::MathEquation(eq)]));
                    }
                    // Inject charts for this body child
                    let chs = chart_ctx.take(idx);
                    for ch in chs {
                        tagged.push(TaggedElement::Plain(vec![Block::Chart(ch)]));
                    }
                    tagged
                }
                docx_rs::DocumentChild::Table(table) => {
                    vec![TaggedElement::Plain(vec![Block::Table(convert_table(
                        table,
                        &images,
                        &hyperlinks,
                        &style_map,
                        &ctx,
                        0,
                    ))])]
                }
                docx_rs::DocumentChild::StructuredDataTag(sdt) => {
                    convert_sdt_children(sdt, &images, &hyperlinks, &style_map, &ctx, &docx.styles)
                }
                _ => vec![TaggedElement::Plain(vec![])],
            }));

            match result {
                Ok(elems) => elements.extend(elems),
                Err(panic_info) => {
                    let detail = if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                        (*s).to_string()
                    } else {
                        "unknown panic".to_string()
                    };
                    warnings.push(ConvertWarning::ParseSkipped {
                        format: "DOCX".to_string(),
                        reason: format!(
                            "upstream panic caught (docx-rs): element at index {idx}: {detail}"
                        ),
                    });
                }
            }

            if let docx_rs::DocumentChild::Paragraph(para) = child
                && let Some(section_prop) = para.property.section_property.as_ref()
            {
                let column_layout = match column_layouts.get(section_layout_index) {
                    Some(layout) => layout.clone(),
                    None => extract_column_layout_from_section_property(section_prop),
                };
                pages.push(Page::Flow(build_flow_page_from_section(
                    section_prop,
                    std::mem::take(&mut elements),
                    &numberings,
                    &header_footer_assets,
                    SectionOverrides {
                        column_layout,
                        page_numbering: page_numbering.get(section_layout_index).copied().flatten(),
                    },
                    style_map.get(DOC_DEFAULT_STYLE_ID),
                    &mut warnings,
                )));
                section_layout_index += 1;
            }
        }

        let final_column_layout = match column_layouts.get(section_layout_index) {
            Some(layout) => layout.clone(),
            None => extract_column_layout_from_section_property(&docx.document.section_property),
        };
        pages.push(Page::Flow(build_flow_page_from_section(
            &docx.document.section_property,
            elements,
            &numberings,
            &header_footer_assets,
            SectionOverrides {
                column_layout: final_column_layout,
                page_numbering: page_numbering.get(section_layout_index).copied().flatten(),
            },
            style_map.get(DOC_DEFAULT_STYLE_ID),
            &mut warnings,
        )));

        Ok((
            Document {
                metadata,
                pages,
                styles: StyleSheet {
                    default_tab_stop_pt,
                    default_text: Some(resolve_doc_default_text_style(
                        &docx.styles,
                        &theme_fonts,
                        &pair_kerning,
                    )),
                    ..StyleSheet::default()
                },
            },
            warnings,
        ))
    }
}

/// `w:defaultTabStop w:val` from `word/settings.xml`, in points. Read from
/// the raw part because docx-rs substitutes its own default when the
/// element is absent, erasing the absent-vs-explicit distinction the
/// East Asian fallback depends on (issue #393).
fn extract_default_tab_stop_pt(data: &[u8]) -> Option<f64> {
    let mut archive = crate::parser::open_zip(data).ok()?;
    let settings_xml: String = read_zip_text(&mut archive, "word/settings.xml")?;
    let element_start: usize = settings_xml.find("<w:defaultTabStop")?;
    let rest: &str = &settings_xml[element_start..];
    let value_start: usize = rest.find("w:val=\"")? + 7;
    let value_end: usize = rest[value_start..].find('"')? + value_start;
    let twips: f64 = rest[value_start..value_end].parse().ok()?;
    (twips > 0.0).then_some(twips / 20.0)
}

/// Extract content from a StructuredDataTag (SDT), processing its paragraph
/// and table children through the standard conversion pipeline.
/// SDTs are used for various structured content in DOCX, including Table of Contents.
fn convert_sdt_children(
    sdt: &docx_rs::StructuredDataTag,
    images: &ImageMap,
    hyperlinks: &HyperlinkMap,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
    styles: &docx_rs::Styles,
) -> Vec<TaggedElement> {
    let mut result = Vec::new();
    for child in &sdt.children {
        match child {
            docx_rs::StructuredDataTagChild::Paragraph(para) => {
                result.push(convert_paragraph_element(
                    para, images, hyperlinks, style_map, ctx, styles,
                ));
            }
            docx_rs::StructuredDataTagChild::Table(table) => {
                result.push(TaggedElement::Plain(vec![Block::Table(convert_table(
                    table, images, hyperlinks, style_map, ctx, 0,
                ))]));
            }
            docx_rs::StructuredDataTagChild::StructuredDataTag(nested) => {
                result.extend(convert_sdt_children(
                    nested, images, hyperlinks, style_map, ctx, styles,
                ));
            }
            _ => {}
        }
    }
    result
}

/// Convert a docx-rs Paragraph into a TaggedElement.
/// If the paragraph has numbering, returns a `ListParagraph`; otherwise `Plain`.
fn convert_paragraph_element(
    para: &docx_rs::Paragraph,
    images: &ImageMap,
    hyperlinks: &HyperlinkMap,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
    styles: &docx_rs::Styles,
) -> TaggedElement {
    let num_info = extract_num_info(para, styles);

    // Build the paragraph IR
    let mut blocks = Vec::new();
    convert_paragraph_blocks(
        para,
        &mut blocks,
        images,
        hyperlinks,
        style_map,
        ctx,
        ParagraphContainer::Body,
    );

    match num_info {
        Some(info) => {
            // Extract the actual Paragraph from the blocks.
            // List paragraphs may also produce page breaks and images before the paragraph.
            let mut pre_blocks = Vec::new();
            let mut paragraph = None;
            for block in blocks {
                match block {
                    Block::Paragraph(p) if paragraph.is_none() => {
                        paragraph = Some(p);
                    }
                    _ => pre_blocks.push(block),
                }
            }
            if !pre_blocks.is_empty() {
                // If there were pre-blocks (page break, images), emit them as plain first.
                // We return the plain blocks — the caller will see them before the list paragraph.
                // For simplicity, we create a combined: Plain(pre) + ListParagraph.
                // But TaggedElement is a single value, so we need to handle this differently.
                // Actually, let's just emit them as plain first. The caller handles ordering.
                // Since we can only return one TaggedElement, fold the pre-blocks into the
                // paragraph by noting that list items in a list won't have page breaks.
                // For now, treat the paragraph as a plain block if it has pre-blocks.
                pre_blocks.push(Block::Paragraph(paragraph.unwrap_or_else(|| Paragraph {
                    style: ParagraphStyle::default(),
                    runs: Vec::new(),
                })));
                TaggedElement::Plain(pre_blocks)
            } else if let Some(mut paragraph) = paragraph {
                paragraph
                    .style
                    .space_after
                    .get_or_insert(WORD_COMPATIBLE_PARAGRAPH_SPACE_AFTER_PT);
                TaggedElement::ListParagraph {
                    info,
                    paragraph: Box::new(paragraph),
                }
            } else {
                TaggedElement::Plain(vec![])
            }
        }
        None => TaggedElement::Plain(blocks),
    }
}

/// Build a text `Run` from extracted text, merging explicit run styling with the
/// resolved paragraph style. Returns `None` when the text is empty, so callers
/// can skip empty runs without duplicating the emptiness check.
fn build_text_run(
    text: String,
    run_property: &docx_rs::RunProperty,
    is_small_caps: bool,
    resolved_style: Option<&ResolvedStyle>,
    style_map: &StyleMap,
    href: Option<String>,
) -> Option<Run> {
    if text.is_empty() {
        return None;
    }
    let mut explicit_style: TextStyle = extract_run_style(run_property);
    if is_small_caps {
        explicit_style.small_caps = Some(true);
    }
    // Layer the referenced character style (`<w:rStyle>`, e.g. a syntax
    // highlighting token) beneath the run's explicit properties so its color
    // and weight apply while explicit run formatting still wins (issue #176).
    if let Some(char_style) = extract_run_style_id(run_property).and_then(|id| style_map.get(&id)) {
        let mut combined: TextStyle = char_style.text.clone();
        combined.merge_from(&explicit_style);
        explicit_style = combined;
    }
    Some(Run {
        text,
        style: merge_text_style(&explicit_style, resolved_style),
        href,
        footnote: None,
    })
}

/// Intermediate results from scanning a run's children for media, text boxes,
/// and structural page/column breaks.
struct RunChildrenMedia {
    has_column_break: bool,
    has_page_break: bool,
    text_box_blocks: Vec<Block>,
}

/// Scan a run's children for drawings, VML shapes, and layout breaks.
/// Extracted images are pushed to `inline_images`; text boxes and break detection
/// are returned in `RunChildrenMedia`.
fn extract_run_children_media(
    run: &docx_rs::Run,
    images: &ImageMap,
    hyperlinks: &HyperlinkMap,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
    inline_images: &mut Vec<Block>,
) -> RunChildrenMedia {
    let mut has_column_break: bool = false;
    let mut has_page_break: bool = false;
    let mut text_box_blocks: Vec<Block> = Vec::new();

    for run_child in &run.children {
        if let docx_rs::RunChild::Drawing(drawing) = run_child {
            let wpg_drawing: Option<WpgDrawingInfo> = ctx.drawing_shapes.consume_wpg_drawing();
            let canvas_image_offset: Option<(f64, f64)> =
                ctx.drawing_shapes.consume_canvas_image_offset();
            if let Some(wpg_drawing) = wpg_drawing {
                // docx-rs represents only one child from a WPG group. Use the
                // complete raw-XML group instead to avoid dropping its siblings.
                text_box_blocks.extend(convert_wpg_drawing_blocks(
                    wpg_drawing,
                    images,
                    hyperlinks,
                    style_map,
                    ctx,
                ));
            } else {
                if let Some(img_block) =
                    extract_drawing_image(drawing, images, &ctx.wraps, canvas_image_offset)
                {
                    inline_images.push(img_block);
                }
                text_box_blocks.extend(extract_drawing_text_box_blocks(
                    drawing, images, hyperlinks, style_map, ctx,
                ));
                if drawing.data.is_none()
                    && let Some(shape) = ctx.drawing_shapes.consume_next()
                {
                    // docx-rs leaves geometry-only `wps:wsp` drawings unclassified.
                    text_box_blocks.push(Block::FloatingShape(shape));
                }
            }
        }
        if let docx_rs::RunChild::Shape(shape) = run_child {
            let vml_text_box: VmlTextBoxInfo = ctx.vml_text_boxes.consume_next();
            if let Some(floating_text_box) = extract_vml_shape_text_box(shape, &vml_text_box) {
                text_box_blocks.push(Block::FloatingTextBox(floating_text_box));
            } else {
                text_box_blocks.extend(vml_text_box.into_blocks());
            }

            if let Some(img_block) = extract_shape_image(shape, images) {
                inline_images.push(img_block);
            }
        }
        if let docx_rs::RunChild::Break(br) = run_child
            && is_column_break(br)
        {
            has_column_break = true;
        }
        if let docx_rs::RunChild::Break(br) = run_child
            && is_page_break(br)
        {
            has_page_break = true;
        }
    }

    RunChildrenMedia {
        has_column_break,
        has_page_break,
        text_box_blocks,
    }
}

fn convert_wpg_drawing_blocks(
    drawing: WpgDrawingInfo,
    images: &ImageMap,
    hyperlinks: &HyperlinkMap,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
) -> Vec<Block> {
    let mut result: Vec<Block> = Vec::new();
    for child in drawing.children {
        if let Some(shape) = child.shape {
            result.push(Block::FloatingShape(shape));
        }

        let mut content: Vec<Block> = Vec::new();
        for document_child in &child.content {
            match document_child {
                // A shape's text frame is its own flow, not the cell's, even
                // when the shape is anchored inside one.
                docx_rs::DocumentChild::Paragraph(paragraph) => convert_paragraph_blocks(
                    paragraph,
                    &mut content,
                    images,
                    hyperlinks,
                    style_map,
                    ctx,
                    ParagraphContainer::Body,
                ),
                docx_rs::DocumentChild::Table(table) => content.push(Block::Table(convert_table(
                    table, images, hyperlinks, style_map, ctx, 0,
                ))),
                _ => {}
            }
        }
        if let Some(text_color) = child.text_color {
            apply_default_text_color(&mut content, text_color);
        }
        if !content.is_empty() {
            result.push(Block::FloatingTextBox(FloatingTextBox {
                content,
                wrap_mode: child.wrap_mode,
                width: child.width,
                height: child.height,
                padding: child.padding,
                vertical_align: child.vertical_align,
                offset_x: child.offset_x,
                offset_y: child.offset_y,
            }));
        }
    }
    result
}

fn apply_default_text_color(blocks: &mut [Block], color: Color) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                for run in &mut paragraph.runs {
                    run.style.color.get_or_insert(color);
                }
            }
            Block::List(list) => {
                for item in &mut list.items {
                    for paragraph in &mut item.content {
                        for run in &mut paragraph.runs {
                            run.style.color.get_or_insert(color);
                        }
                    }
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        apply_default_text_color(&mut cell.content, color);
                    }
                }
            }
            Block::FloatingTextBox(text_box) => {
                apply_default_text_color(&mut text_box.content, color);
            }
            _ => {}
        }
    }
}

/// The list a paragraph's `TOC` field produces, if it carries one.
///
/// A dirty `TOC` field is stored as its instruction and nothing else, so the
/// paragraph holding it has no text to render and the contents page came out
/// blank. The field becomes a block the renderer resolves against the
/// document itself instead — `\o` against its headings, `\a` against the
/// captions of one `SEQ` sequence (issue #576).
fn toc_field(para: &docx_rs::Paragraph) -> Option<TableOfContents> {
    para.children
        .iter()
        .filter_map(|child| match child {
            docx_rs::ParagraphChild::Run(run) => Some(run),
            _ => None,
        })
        .flat_map(|run| run.children.iter())
        .find_map(|child| {
            let instruction: &str = match child {
                docx_rs::RunChild::InstrText(instruction) => match instruction.as_ref() {
                    docx_rs::InstrText::Unsupported(text) => text,
                    _ => return None,
                },
                docx_rs::RunChild::InstrTextString(text) => text,
                _ => return None,
            };
            toc_caption_identifier(instruction)
                .map(|identifier| TableOfContents::Captions { identifier })
                .or_else(|| {
                    toc_heading_depth(instruction).map(|depth| TableOfContents::Headings { depth })
                })
        })
}

/// The number a run's `SEQ` field renders, if it carries one.
///
/// Word stores a caption number in the field, not in the text, so a run that
/// holds `SEQ Table` contributes the counter's next value. Text between the
/// field's `separate` and `end` is its cached result — what Word last
/// computed — and is replaced by the value computed here rather than added to
/// it (issue #577).
fn seq_field_text(
    run: &docx_rs::Run,
    fields: &FieldContext,
    seen: &mut Option<String>,
) -> Option<String> {
    let mut identifier: Option<String> = None;
    for child in &run.children {
        match child {
            docx_rs::RunChild::InstrText(instruction) => {
                if let docx_rs::InstrText::Unsupported(text) = instruction.as_ref()
                    && let Some(found) = seq_identifier(text)
                {
                    identifier = Some(found.to_string());
                }
            }
            docx_rs::RunChild::InstrTextString(text) => {
                if let Some(found) = seq_identifier(text) {
                    identifier = Some(found.to_string());
                }
            }
            _ => {}
        }
    }
    identifier.map(|identifier| {
        let number = fields.next_in_sequence(&identifier).to_string();
        *seen = Some(identifier);
        number
    })
}

/// Resolve a note's runs against the style it names.
///
/// A note is read from `footnotes.xml` before the stylesheet is, so its runs
/// arrive carrying only their own `w:rPr`. Word resolves them through the same
/// cascade as the body: the note's `w:pStyle` — `FootnoteText` and friends —
/// supplies the size, colour, and family the runs leave unstated, and falls
/// back to the document defaults when the note names no style (issue #580).
fn resolve_note_runs(content: &NoteContent, style_map: &StyleMap) -> Vec<Run> {
    let note_style = content
        .style_id
        .as_deref()
        .and_then(|style_id| style_map.get(style_id))
        .or_else(|| style_map.get(DOC_DEFAULT_STYLE_ID));

    content
        .runs
        .iter()
        .map(|note_run| Run {
            text: note_run.text.clone(),
            style: merge_text_style(&note_run.explicit, note_style),
            href: None,
            footnote: None,
        })
        .collect()
}

/// A paragraph child once tracked changes have been resolved away.
///
/// Callers match only the variants they render; a header paragraph ignores
/// `Hyperlink`, and the body ignores the two field variants a header uses.
pub(super) enum ParagraphItem<'a> {
    Run(&'a docx_rs::Run),
    Hyperlink(&'a docx_rs::Hyperlink),
    PageNum,
    NumPages,
}

/// Resolve a paragraph's tracked changes to the final document.
///
/// Word shows two views of a document with change tracking on. The review
/// view marks up both sides; the final view — what "No Markup" shows, what
/// accepting every revision produces, and what a converter is expected to
/// render — keeps the insertions and drops the deletions.
///
/// `w:ins` and `w:del` were both falling through the paragraph child match's
/// catch-all arm, so both sides vanished. Dropping `w:del` is right; dropping
/// `w:ins` silently lost ordinary document text whose only distinction was
/// having been typed while tracking was on (issue #583).
///
/// A `w:del` nested inside a `w:ins` is text that was inserted and then
/// deleted again, so it is absent from the final document too and is dropped
/// with the rest.
pub(super) fn flatten_tracked_changes(
    children: &[docx_rs::ParagraphChild],
) -> Vec<ParagraphItem<'_>> {
    let mut items: Vec<ParagraphItem<'_>> = Vec::with_capacity(children.len());
    for child in children {
        match child {
            docx_rs::ParagraphChild::Run(run) => items.push(ParagraphItem::Run(run)),
            docx_rs::ParagraphChild::Hyperlink(hyperlink) => {
                items.push(ParagraphItem::Hyperlink(hyperlink))
            }
            docx_rs::ParagraphChild::PageNum(_) => items.push(ParagraphItem::PageNum),
            docx_rs::ParagraphChild::NumPages(_) => items.push(ParagraphItem::NumPages),
            docx_rs::ParagraphChild::Insert(insert) => {
                for inserted in &insert.children {
                    if let docx_rs::InsertChild::Run(run) = inserted {
                        items.push(ParagraphItem::Run(run));
                    }
                }
            }
            _ => {}
        }
    }
    items
}

/// Process hyperlink children, extracting text runs with the resolved URL.
fn process_hyperlink_runs(
    hyperlink: &docx_rs::Hyperlink,
    hyperlinks: &HyperlinkMap,
    resolved_style: Option<&ResolvedStyle>,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
    runs: &mut Vec<Run>,
) {
    let href: Option<String> = resolve_hyperlink_url(hyperlink, hyperlinks);
    for hchild in &hyperlink.children {
        if let docx_rs::ParagraphChild::Run(run) = hchild {
            let hl_small_caps: bool = ctx.small_caps.next_is_small_caps();
            let text: String = extract_run_text(run);
            if let Some(ir_run) = build_text_run(
                text,
                &run.run_property,
                hl_small_caps,
                resolved_style,
                style_map,
                href.clone(),
            ) {
                runs.push(ir_run);
            }
        }
    }
}

/// Which text flow a paragraph belongs to.
///
/// Word's East Asian/Latin auto space is a property of the flow, not of the
/// paragraph's own formatting: cell text never gets it while body text with
/// identical run properties does, so the text path has to be told which one it
/// is building (issue #627).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ParagraphContainer {
    /// The document body, an SDT, a WPG shape's text, or a drawing text
    /// box's text — the paragraphs that reach `convert_paragraph_blocks`
    /// from outside a `<w:tc>`. Headers, footers and footnote text convert
    /// through their own paths and never reach
    /// `insert_east_asian_auto_space` at all, so this enum does not speak
    /// for them.
    Body,
    /// Inside a `<w:tc>`, at any nesting depth.
    TableCell,
}

/// What the surrounding flow contributes to a paragraph, as opposed to the
/// paragraph's own formatting: the direction `w:bidi` inherits onto it, the
/// shading its style hierarchy paints behind it, and which flow it is in.
///
/// Resolved once per `<w:p>` because the bidi and shading cursors advance on
/// read, then handed to every paragraph the `<w:p>` splits into.
#[derive(Clone)]
struct ParagraphFlow {
    is_rtl: bool,
    background: Option<Color>,
    /// The paragraph's own `w:wordWrap`, recovered from the raw XML — the
    /// published docx-rs does not parse it (issue #1041).
    word_wrap: Option<bool>,
    contextual_spacing: ParagraphContextualSpacing,
    container: ParagraphContainer,
}

/// Convert a docx-rs Paragraph to IR blocks, handling page breaks and inline images.
/// If the paragraph has `page_break_before`, a `Block::PageBreak` is emitted first.
/// Consecutive inline images within a paragraph are kept in one wrapping flow container.
/// Style formatting from the document's style definitions is merged with explicit formatting.
fn convert_paragraph_blocks(
    para: &docx_rs::Paragraph,
    out: &mut Vec<Block>,
    images: &ImageMap,
    hyperlinks: &HyperlinkMap,
    style_map: &StyleMap,
    ctx: &DocxConversionContext,
    container: ParagraphContainer,
) {
    // Check bidi direction for this paragraph (must be called once per XML <w:p>)
    let flow = ParagraphFlow {
        is_rtl: ctx.bidi.next_is_bidi(),
        background: ctx.paragraph_shading.next_background(),
        word_wrap: ctx.word_wraps.next_word_wrap(),
        contextual_spacing: ctx.contextual_spacing.next(),
        container,
    };

    // Emit page break before the paragraph if requested
    if para.property.page_break_before == Some(true) {
        out.push(Block::PageBreak);
    }

    // A dirty `TOC` field is stored as its instruction and nothing else, so
    // the paragraph carrying it has no text of its own to render (issue #576).
    // A field Word has already computed keeps its cached entries instead:
    // those are the result, and recomputing over them would drop the numbers
    // the document shipped.
    if let Some(contents) = toc_field(para)
        && para
            .children
            .iter()
            .filter_map(|child| match child {
                docx_rs::ParagraphChild::Run(run) => Some(run),
                _ => None,
            })
            .all(|run| extract_run_text(run).trim().is_empty())
    {
        out.push(Block::TableOfContents(contents));
        return;
    }

    // Look up the paragraph's referenced style
    let resolved_style = get_paragraph_style_id(&para.property)
        .and_then(|id| style_map.get(id))
        .or_else(|| style_map.get(DOC_DEFAULT_STYLE_ID));

    // Collect text runs and detect inline images
    let mut runs: Vec<Run> = Vec::new();
    let mut inline_images: Vec<Block> = Vec::new();
    let mut emitted_paragraph: bool = false;
    let mut emitted_media_blocks: bool = false;
    let mut emitted_floating_anchor: bool = false;
    let mut emitted_layout_break: bool = false;
    // Set by the run carrying a `SEQ` field, so the finished paragraph can be
    // wrapped as the caption a `TOC \a` list collects (issue #576).
    let mut caption_identifier: Option<String> = None;

    for child in flatten_tracked_changes(&para.children) {
        match child {
            ParagraphItem::Run(run) => {
                // Advance smallCaps cursor for every <w:r> in body
                let is_small_caps: bool = ctx.small_caps.next_is_small_caps();

                // Check for footnote/endnote reference runs
                if is_note_reference_run(run, &ctx.notes) {
                    if let Some(content) = ctx.notes.consume_next() {
                        runs.push(Run {
                            text: String::new(),
                            style: TextStyle::default(),
                            href: None,
                            footnote: Some(resolve_note_runs(&content, style_map)),
                        });
                    }
                    continue;
                }

                let media = extract_run_children_media(
                    run,
                    images,
                    hyperlinks,
                    style_map,
                    ctx,
                    &mut inline_images,
                );

                // A picture is the paragraph's content, so its paragraph mark
                // belongs to the picture rather than to a blank line. Counting
                // only text boxes here left a picture-only paragraph emitting an
                // empty paragraph as well, adding a full line box below every
                // figure (issue #496).
                emitted_media_blocks |= !inline_images.is_empty();

                if !media.text_box_blocks.is_empty() {
                    emitted_media_blocks = true;
                    emitted_floating_anchor |= media.text_box_blocks.iter().any(|block| {
                        matches!(block, Block::FloatingShape(_) | Block::FloatingTextBox(_))
                    });
                    if !runs.is_empty() {
                        push_inline_images(
                            out,
                            &mut inline_images,
                            paragraph_alignment(para),
                            paragraph_image_spacing(para, resolved_style),
                        );
                        push_paragraph_from_runs(
                            out,
                            para,
                            resolved_style,
                            &flow,
                            &mut runs,
                            caption_identifier.as_deref(),
                        );
                        emitted_paragraph = true;
                    } else if !inline_images.is_empty() {
                        push_inline_images(
                            out,
                            &mut inline_images,
                            paragraph_alignment(para),
                            paragraph_image_spacing(para, resolved_style),
                        );
                    }
                    out.extend(media.text_box_blocks);
                }

                if media.has_page_break || media.has_column_break {
                    // Flush current runs as a paragraph before the layout break.
                    if !runs.is_empty() {
                        push_inline_images(
                            out,
                            &mut inline_images,
                            paragraph_alignment(para),
                            paragraph_image_spacing(para, resolved_style),
                        );
                        push_paragraph_from_runs(
                            out,
                            para,
                            resolved_style,
                            &flow,
                            &mut runs,
                            caption_identifier.as_deref(),
                        );
                        emitted_paragraph = true;
                    }
                    out.push(if media.has_page_break {
                        Block::PageBreak
                    } else {
                        Block::ColumnBreak
                    });
                    emitted_layout_break = true;

                    // Still extract any text from this run (after the break)
                    let text: String = seq_field_text(run, &ctx.fields, &mut caption_identifier)
                        .unwrap_or_else(|| extract_run_text_skip_layout_breaks(run));
                    if let Some(ir_run) = build_text_run(
                        text,
                        &run.run_property,
                        is_small_caps,
                        resolved_style,
                        style_map,
                        None,
                    ) {
                        runs.push(ir_run);
                    }
                } else {
                    let text: String = seq_field_text(run, &ctx.fields, &mut caption_identifier)
                        .unwrap_or_else(|| extract_run_text(run));
                    if let Some(ir_run) = build_text_run(
                        text,
                        &run.run_property,
                        is_small_caps,
                        resolved_style,
                        style_map,
                        None,
                    ) {
                        runs.push(ir_run);
                    }
                }
            }
            ParagraphItem::Hyperlink(hyperlink) => {
                process_hyperlink_runs(
                    hyperlink,
                    hyperlinks,
                    resolved_style,
                    style_map,
                    ctx,
                    &mut runs,
                );
            }
            // `w:pgNum`/`w:numPages` are header and footer fields; the body
            // resolves its page numbers through `w:fldSimple` instead.
            ParagraphItem::PageNum | ParagraphItem::NumPages => {}
        }
    }

    push_inline_images(
        out,
        &mut inline_images,
        paragraph_alignment(para),
        paragraph_image_spacing(para, resolved_style),
    );

    // A paragraph whose remaining content is just the mark left behind by a
    // page or column break is a break carrier: Word uses it only to force the
    // break, so it must not add a line box on the new page. An empty paragraph
    // with no break is a deliberate blank line and is still kept.
    let is_layout_break_carrier: bool = emitted_layout_break && runs.is_empty();

    if !is_layout_break_carrier
        && (!runs.is_empty()
            || !emitted_media_blocks
            || (emitted_floating_anchor && !emitted_paragraph))
    {
        // Keep paragraph marks for floating drawing anchors. The drawing itself
        // is positioned by offsets, but the source paragraph still contributes
        // to flow spacing between the drawing cluster and following content.
        push_paragraph_from_runs(
            out,
            para,
            resolved_style,
            &flow,
            &mut runs,
            caption_identifier.as_deref(),
        );
    }
}

fn push_inline_images(
    out: &mut Vec<Block>,
    inline_images: &mut Vec<Block>,
    alignment: Option<Alignment>,
    spacing: Option<ImageParagraphSpacing>,
) {
    let mut grouped: Vec<ImageData> = Vec::new();

    for block in inline_images.drain(..) {
        match block {
            Block::Image(mut image) => {
                // Inline images inherit the containing paragraph's alignment
                // and its `w:spacing`: the picture consumes the paragraph, so
                // the gaps Word draws around it have to travel with the
                // picture instead (issue #499).
                if image.alignment.is_none() {
                    image.alignment = alignment;
                }
                if image.paragraph_spacing.is_none() {
                    image.paragraph_spacing = spacing;
                }
                grouped.push(image)
            }
            other => {
                flush_inline_image_group(out, &mut grouped);
                out.push(other);
            }
        }
    }
    flush_inline_image_group(out, &mut grouped);
}

fn flush_inline_image_group(out: &mut Vec<Block>, grouped: &mut Vec<ImageData>) {
    match grouped.len() {
        0 => {}
        1 => out.push(Block::Image(grouped.pop().expect("one inline image"))),
        _ => out.push(Block::InlineImages(std::mem::take(grouped))),
    }
}

/// The paragraph's explicit horizontal alignment, if any.
fn paragraph_alignment(para: &docx_rs::Paragraph) -> Option<Alignment> {
    extract_paragraph_style(&para.property).alignment
}

/// The `w:spacing` a picture paragraph contributes to the flow.
///
/// Resolved through the same style merge a text paragraph uses, so spacing
/// inherited from `styles.xml` counts as well as direct formatting.
fn paragraph_image_spacing(
    para: &docx_rs::Paragraph,
    resolved_style: Option<&ResolvedStyle>,
) -> Option<ImageParagraphSpacing> {
    let style: ParagraphStyle = merge_paragraph_style(
        &extract_paragraph_style(&para.property),
        None,
        resolved_style,
    );
    let spacing = ImageParagraphSpacing {
        before: style.space_before,
        after: style.space_after,
    };
    (spacing != ImageParagraphSpacing::default()).then_some(spacing)
}

fn push_paragraph_from_runs(
    out: &mut Vec<Block>,
    para: &docx_rs::Paragraph,
    resolved_style: Option<&ResolvedStyle>,
    flow: &ParagraphFlow,
    runs: &mut Vec<Run>,
    caption_identifier: Option<&str>,
) {
    let mut explicit_para_style = extract_paragraph_style(&para.property);
    explicit_para_style.background = flow.background;
    explicit_para_style.word_wrap = flow.word_wrap;
    explicit_para_style.paragraph_style_id = flow.contextual_spacing.style_id.clone();
    explicit_para_style.contextual_spacing = flow.contextual_spacing.enabled;
    let explicit_tab_overrides = extract_tab_stop_overrides(&para.property.tabs);
    let mut style = merge_paragraph_style(
        &explicit_para_style,
        explicit_tab_overrides.as_deref(),
        resolved_style,
    );
    if flow.is_rtl {
        style.direction = Some(TextDirection::Rtl);
    }
    apply_word_compatible_paragraph_defaults(&mut style);
    // Word's automatic East Asian/Latin space, applied once per paragraph so a
    // boundary falling between two runs is caught too. Justified paragraphs are
    // left alone: Word treats the space as compressible and absorbs it into the
    // justification, which is why every *justified* boundary that lacks it in
    // the corpus GT is on a line Word is actively stretching or compressing
    // (issue #521). Centred paragraphs lack it for a different reason — see
    // below.
    //
    // Table cells are left alone as well: Word applies no auto space to cell
    // text at all (issue #627).
    let entry_text: Option<String> = caption_identifier.map(|_| caption_entry_text(runs));
    // A centred paragraph gets none either. Measured on `02_contract_ko`
    // page 1: Word advances 5.78pt at each digit-to-Hangul boundary of the
    // centred date line and 8.41pt at the same boundaries in the body
    // paragraph above it, so it applies the space in one and not the other.
    // 8.41 - 5.78 is 2.63pt, and 0.25em at that line's 10.5pt is 2.625pt
    // (issue #728). Right alignment is left alone: nothing measured it.
    if flow.container == ParagraphContainer::Body
        && !matches!(
            style.alignment,
            Some(Alignment::Justify) | Some(Alignment::Center)
        )
    {
        insert_east_asian_auto_space(runs);
    }
    let paragraph = Paragraph {
        style,
        runs: std::mem::take(runs),
    };
    match (caption_identifier, entry_text) {
        (Some(identifier), Some(entry_text)) => out.push(Block::Caption(Caption {
            identifier: identifier.to_string(),
            entry_text,
            paragraph,
        })),
        _ => out.push(Block::Paragraph(paragraph)),
    }
}

/// The text a `TOC \a` list shows for a caption.
///
/// Word lists the caption without the label and the number that precede it —
/// `종전 헤드리스 변환 스택과 …`, not `그림 1  종전 헤드리스 변환 스택과 …`.
/// The number is its own run, produced by the `SEQ` field, so everything from
/// the run after it onward is the caption proper.
///
/// Read before `insert_east_asian_auto_space` rewrites the runs: those markers
/// are an instruction about the caption's own layout, and the list entry is a
/// separate piece of text that gets its own. Taking the text afterwards
/// carried them into the entry, where they rendered as stray glyphs.
fn caption_entry_text(runs: &[Run]) -> String {
    let after_number = runs
        .iter()
        .position(|run| !run.text.is_empty() && run.text.chars().all(|c| c.is_ascii_digit()))
        .map(|index| index + 1)
        .unwrap_or(0);
    runs[after_number..]
        .iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
#[path = "docx_tests.rs"]
mod tests;
