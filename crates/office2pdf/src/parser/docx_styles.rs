use std::collections::HashMap;

use crate::ir::{Color, PairKerning, ParagraphStyle, TabStop, TextStyle};

use super::{
    ThemeFonts, extract_doc_default_paragraph_style, extract_doc_default_text_style_with_theme,
    extract_paragraph_style, extract_run_style, extract_tab_stop_overrides,
    pair_kerning_from_half_points, resolve_theme_font_family,
};

/// The `w:kern` thresholds a document states outside its runs, read from the
/// raw `word/styles.xml`.
///
/// Read from the raw part for the reason `extract_default_tab_stop_pt` reads
/// `w:defaultTabStop` from raw `word/settings.xml`: the pinned docx-rs fork
/// has no field for the element, so waiting on its parse would report every
/// document as unkerned — including the 21 tracked fixtures that do state
/// `w:kern`, 9 of them as `<w:kern w:val="2"/>` in `w:docDefaults`, which asks
/// Word to kern everything (issue #628 review).
///
/// Covers the two levels a stated threshold actually reaches text from:
/// `w:docDefaults/w:rPrDefault/w:rPr/w:kern`, and each named style's
/// `w:rPr/w:kern`.
///
/// TODO(direct run `w:kern` in `word/document.xml` is not read): docx-rs drops
/// the element from `RunProperty`, and the only way to reattach it without
/// re-implementing run parsing is a positional cursor over `<w:r>` elements,
/// the shape `SmallCapsContext` uses. That cursor cannot be trusted here: the
/// scan counts runs the conversion never consumes — `w:del` runs, which
/// `flatten_tracked_changes` drops, and text-box runs, which convert through
/// their own path — so a document mixing those with a direct `w:kern` would
/// hand the threshold to the wrong run. A document whose runs state `w:kern`
/// therefore takes its style's answer, not the run's. The only tracked fixture
/// with direct run `w:kern` states `w:val="0"`, which is what the absent
/// `w:docDefaults` element already resolves to, so nothing in the corpus
/// changes. Reading it properly needs the element parsed upstream.
#[derive(Debug, Clone)]
pub(super) struct PairKerningRules {
    /// `w:docDefaults/w:rPrDefault/w:rPr/w:kern`. Absence here is a decision,
    /// not inheritance: Word ships with font kerning off, which is why the
    /// English mocks — none of which state the element — set every glyph at
    /// its nominal advance.
    document_default: PairKerning,
    /// `w:style/w:rPr/w:kern`, keyed by `w:styleId`. A style that states
    /// nothing is absent from the map and inherits.
    by_style_id: HashMap<String, PairKerning>,
}

impl Default for PairKerningRules {
    fn default() -> Self {
        Self {
            document_default: PairKerning::Never,
            by_style_id: HashMap::new(),
        }
    }
}

impl PairKerningRules {
    pub(super) fn from_styles_xml(xml: Option<&str>) -> Self {
        let Some(xml) = xml else {
            return Self::default();
        };
        Self::scan(xml)
    }

    /// The decision every run inherits when neither its style nor its own
    /// properties state one.
    pub(super) fn document_default(&self) -> PairKerning {
        self.document_default
    }

    /// What a named style states, or `None` when it states nothing and its
    /// runs inherit the document default.
    pub(super) fn for_style(&self, style_id: &str) -> Option<PairKerning> {
        self.by_style_id.get(style_id).copied()
    }

    fn scan(xml: &str) -> Self {
        use quick_xml::events::Event;

        let mut rules = Self::default();
        let mut reader = quick_xml::Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut in_doc_defaults: bool = false;
        let mut in_run_property_default: bool = false;
        // `w:pPr` never carries a `w:rPr` in a style definition (the schema
        // gives styles `CT_PPrGeneral`, which has no run properties), but a
        // malformed part must not be allowed to plant a threshold either.
        let mut in_paragraph_property: bool = false;
        let mut current_style_id: Option<String> = None;

        loop {
            match reader.read_event() {
                Ok(Event::Start(element) | Event::Empty(element)) => {
                    match element.local_name().as_ref() {
                        b"docDefaults" => in_doc_defaults = true,
                        b"rPrDefault" => in_run_property_default = true,
                        b"pPr" => in_paragraph_property = true,
                        b"style" => {
                            current_style_id = element
                                .attributes()
                                .flatten()
                                .find(|attribute| attribute.key.local_name().as_ref() == b"styleId")
                                .and_then(|attribute| {
                                    attribute
                                        .decode_and_unescape_value(reader.decoder())
                                        .ok()
                                        .map(|value| value.into_owned())
                                });
                        }
                        b"kern" if !in_paragraph_property => {
                            let Some(half_points) = read_val_attribute(&element, reader.decoder())
                            else {
                                continue;
                            };
                            let kerning: PairKerning = pair_kerning_from_half_points(half_points);
                            if in_doc_defaults && in_run_property_default {
                                rules.document_default = kerning;
                            } else if let Some(style_id) = current_style_id.as_ref() {
                                rules.by_style_id.insert(style_id.clone(), kerning);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(element)) => match element.local_name().as_ref() {
                    b"docDefaults" => in_doc_defaults = false,
                    b"rPrDefault" => in_run_property_default = false,
                    b"pPr" => in_paragraph_property = false,
                    b"style" => current_style_id = None,
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }

        rules
    }
}

/// `w:val` of an element, as the number `w:kern` states it in: half-points.
fn read_val_attribute(
    element: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::Decoder,
) -> Option<f64> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == b"val")
        .and_then(|attribute| attribute.decode_and_unescape_value(decoder).ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
}

/// Resolved style formatting extracted from a document style definition.
/// Contains text and paragraph formatting along with an optional heading level.
pub(super) struct ResolvedStyle {
    pub(super) text: TextStyle,
    pub(super) paragraph: ParagraphStyle,
    pub(super) paragraph_tab_overrides: Option<Vec<TabStopOverride>>,
    /// Heading level from outline_lvl (0 = Heading 1, 1 = Heading 2, ..., 5 = Heading 6).
    pub(super) heading_level: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum TabStopOverride {
    Set(TabStop),
    Clear(f64),
}

/// Map from style_id → resolved formatting.
pub(super) type StyleMap = HashMap<String, ResolvedStyle>;

/// Synthetic style ID used for document-level default text properties.
pub(super) const DOC_DEFAULT_STYLE_ID: &str = "__office2pdf_doc_defaults";

pub(super) fn scan_default_paragraph_style_id(styles_xml: &str) -> Option<String> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_str(styles_xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"style" =>
            {
                let mut style_type = None;
                let mut is_default = false;
                let mut style_id = None;
                for attribute in element.attributes().flatten() {
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .ok()
                        .map(|value| value.into_owned());
                    match attribute.key.local_name().as_ref() {
                        b"type" => style_type = value,
                        b"default" => is_default = matches!(value.as_deref(), Some("1" | "true")),
                        b"styleId" => style_id = value,
                        _ => {}
                    }
                }
                if style_type.as_deref() == Some("paragraph") && is_default {
                    return style_id;
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

use crate::defaults::HEADING_FONT_SIZES;

/// Build a map from style ID → resolved formatting by extracting formatting
/// from each style's run_property and paragraph_property.
pub(super) fn build_style_map(
    styles: &docx_rs::Styles,
    theme_fonts: &ThemeFonts,
    default_paragraph_style_id: Option<&str>,
    paragraph_backgrounds: &HashMap<String, Color>,
    style_word_wraps: &HashMap<String, bool>,
    pair_kerning: &PairKerningRules,
) -> StyleMap {
    let mut map = StyleMap::new();
    let default_text: TextStyle = resolve_doc_default_text_style(styles, theme_fonts, pair_kerning);
    let default_paragraph: ParagraphStyle = extract_doc_default_paragraph_style(styles);

    map.insert(
        DOC_DEFAULT_STYLE_ID.to_string(),
        ResolvedStyle {
            text: default_text,
            paragraph: default_paragraph.clone(),
            paragraph_tab_overrides: None,
            heading_level: None,
        },
    );

    for style in &styles.styles {
        match style.style_type {
            docx_rs::StyleType::Paragraph => {
                let mut own_text = extract_run_style(&style.run_property);
                if own_text.font_family.is_none()
                    && let Ok(run_property_json) = serde_json::to_value(&style.run_property)
                {
                    own_text.font_family =
                        resolve_theme_font_family(&run_property_json, theme_fonts);
                }
                // `None` here is "states nothing", so the merge below leaves
                // the document default's decision standing (issue #628).
                own_text.pair_kerning = pair_kerning.for_style(&style.style_id);
                let text = merge_text_style(&own_text, map.get(DOC_DEFAULT_STYLE_ID));
                // A named style states only what it changes; everything else
                // falls through to `w:pPrDefault`, exactly as its run
                // properties fall through to `w:rPrDefault` above (issue #574).
                let mut paragraph = fill_paragraph_defaults(
                    extract_paragraph_style(&style.paragraph_property),
                    &default_paragraph,
                );
                paragraph.background = paragraph_backgrounds.get(&style.style_id).copied();
                // From the raw styles.xml, since the published docx-rs does
                // not parse `w:wordWrap` (issue #1041).
                paragraph.word_wrap = style_word_wraps.get(&style.style_id).copied();
                let paragraph_tab_overrides =
                    extract_tab_stop_overrides(&style.paragraph_property.tabs);
                let heading_level = style
                    .paragraph_property
                    .outline_lvl
                    .as_ref()
                    .map(|outline_level| outline_level.v)
                    .filter(|&value| value < 6);

                map.insert(
                    style.style_id.clone(),
                    ResolvedStyle {
                        text,
                        paragraph,
                        paragraph_tab_overrides,
                        heading_level,
                    },
                );
            }
            // Character styles (e.g. pandoc's `BuiltInTok`/`StringTok` syntax
            // highlighting tokens) contribute only run-level text properties.
            // They deliberately do NOT inherit document defaults, so that
            // overlaying a run's `rStyle` onto its paragraph style changes only
            // the properties the character style actually sets (issue #176).
            docx_rs::StyleType::Character => {
                let mut text = extract_run_style(&style.run_property);
                text.pair_kerning = pair_kerning.for_style(&style.style_id);
                map.insert(
                    style.style_id.clone(),
                    ResolvedStyle {
                        text,
                        paragraph: ParagraphStyle::default(),
                        paragraph_tab_overrides: None,
                        heading_level: None,
                    },
                );
            }
            _ => {}
        }
    }

    // Paragraphs without an explicit pStyle inherit the default paragraph
    // style (w:default="1", normally "Normal"), not just the bare document
    // defaults — fold it into the synthetic doc-default entry so its spacing,
    // line spacing, and text properties survive the cascade (issue #288).
    if let Some(style_id) = default_paragraph_style_id
        && let Some(default_style) = map.get(style_id)
    {
        let merged = ResolvedStyle {
            text: default_style.text.clone(),
            paragraph: default_style.paragraph.clone(),
            paragraph_tab_overrides: default_style.paragraph_tab_overrides.clone(),
            heading_level: None,
        };
        map.insert(DOC_DEFAULT_STYLE_ID.to_string(), merged);
    }

    map
}

/// The document-wide run defaults, with the kerning threshold the raw
/// `word/styles.xml` states folded in.
///
/// Kept apart from `extract_doc_default_text_style_with_theme` because that
/// function reads docx-rs' parse, which has no `w:kern` to give.
pub(super) fn resolve_doc_default_text_style(
    styles: &docx_rs::Styles,
    theme_fonts: &ThemeFonts,
    pair_kerning: &PairKerningRules,
) -> TextStyle {
    let mut text: TextStyle = extract_doc_default_text_style_with_theme(styles, theme_fonts);
    text.pair_kerning = Some(pair_kerning.document_default());
    text
}

/// Fill a style's unstated paragraph properties from the document default.
///
/// Only the properties Word actually inherits through `w:pPrDefault` are
/// filled. `heading_level`, `tab_stops`, `background`, and the borders are
/// deliberately absent: the first three are resolved from the style's own
/// `w:outlineLvl`, `w:tabs`, and `w:shd`, and a document default never carries
/// a `w:pBdr` that should frame every paragraph in the file.
fn fill_paragraph_defaults(mut style: ParagraphStyle, defaults: &ParagraphStyle) -> ParagraphStyle {
    style.alignment = style.alignment.or(defaults.alignment);
    style.indent_left = style.indent_left.or(defaults.indent_left);
    style.indent_right = style.indent_right.or(defaults.indent_right);
    style.indent_first_line = style.indent_first_line.or(defaults.indent_first_line);
    style.line_spacing = style.line_spacing.or(defaults.line_spacing);
    style.space_before = style.space_before.or(defaults.space_before);
    style.space_after = style.space_after.or(defaults.space_after);
    style
}

/// Merge style text formatting with explicit run formatting.
/// Explicit formatting (from the run itself) takes priority over style formatting.
/// For heading styles, default sizes and bold are applied when neither the style
/// nor the run specifies them.
pub(super) fn merge_text_style(explicit: &TextStyle, style: Option<&ResolvedStyle>) -> TextStyle {
    let (style_text, heading_level) = match style {
        Some(style) => (&style.text, style.heading_level),
        None => return explicit.clone(),
    };

    let mut merged: TextStyle = style_text.clone();

    // Heading defaults: apply fallback size/bold when the style itself
    // doesn't specify them. This must happen before the explicit overwrite
    // so that explicit values still win.
    if let Some(level) = heading_level {
        if merged.font_size.is_none() {
            merged.font_size = Some(HEADING_FONT_SIZES[level]);
        }
        if merged.bold.is_none() {
            merged.bold = Some(true);
        }
    }

    merged.merge_from(explicit);

    merged
}

/// Merge style paragraph formatting with explicit paragraph formatting.
/// Explicit formatting takes priority.
pub(super) fn merge_paragraph_style(
    explicit: &ParagraphStyle,
    explicit_tab_overrides: Option<&[TabStopOverride]>,
    style: Option<&ResolvedStyle>,
) -> ParagraphStyle {
    let style_paragraph = style.map(|resolved_style| &resolved_style.paragraph);
    let inherited_tab_stops = style.and_then(resolve_style_tab_stops);

    ParagraphStyle {
        alignment: explicit
            .alignment
            .or(style_paragraph.and_then(|style| style.alignment)),
        // Measured on Word: a paragraph's own w:wordWrap beats the one its
        // style carries — a ListParagraph with w:val="0" breaks mid-eojeol
        // although the style alone would not (issue #730).
        word_wrap: explicit
            .word_wrap
            .or(style_paragraph.and_then(|style| style.word_wrap)),
        indent_left: explicit
            .indent_left
            .or(style_paragraph.and_then(|style| style.indent_left)),
        indent_right: explicit
            .indent_right
            .or(style_paragraph.and_then(|style| style.indent_right)),
        indent_first_line: explicit
            .indent_first_line
            .or(style_paragraph.and_then(|style| style.indent_first_line)),
        line_spacing: explicit
            .line_spacing
            .or(style_paragraph.and_then(|style| style.line_spacing)),
        line_box: explicit
            .line_box
            .or(style_paragraph.and_then(|style| style.line_box)),
        space_before: explicit
            .space_before
            .or(style_paragraph.and_then(|style| style.space_before)),
        space_after: explicit
            .space_after
            .or(style_paragraph.and_then(|style| style.space_after)),
        paragraph_style_id: explicit.paragraph_style_id.clone(),
        contextual_spacing: explicit.contextual_spacing,
        heading_level: style
            .and_then(|resolved_style| resolved_style.heading_level)
            .map(|level| (level + 1) as u8),
        direction: explicit.direction,
        tab_stops: merge_tab_stops(
            explicit.tab_stops.as_deref(),
            explicit_tab_overrides,
            inherited_tab_stops.as_deref(),
        ),
        default_tab_stop_pt: explicit
            .default_tab_stop_pt
            .or(style_paragraph.and_then(|style| style.default_tab_stop_pt)),
        background: explicit
            .background
            .or(style_paragraph.and_then(|style| style.background)),
        border: explicit
            .border
            .clone()
            .or_else(|| style_paragraph.and_then(|style| style.border.clone())),
        // Follows the border it measures from rather than merging separately:
        // a paragraph that inherits its rules inherits their gaps with them,
        // and one that overrides them overrides the gaps too (issue #520).
        border_space: if explicit.border.is_some() {
            explicit.border_space.clone()
        } else {
            style_paragraph.and_then(|style| style.border_space.clone())
        },
    }
}

fn resolve_style_tab_stops(style: &ResolvedStyle) -> Option<Vec<TabStop>> {
    resolve_tab_stop_source(
        style.paragraph.tab_stops.as_deref(),
        style.paragraph_tab_overrides.as_deref(),
    )
}

fn resolve_tab_stop_source(
    tab_stops: Option<&[TabStop]>,
    tab_overrides: Option<&[TabStopOverride]>,
) -> Option<Vec<TabStop>> {
    if let Some(tab_overrides) = tab_overrides {
        let mut resolved: Vec<TabStop> = Vec::new();
        apply_tab_stop_overrides(&mut resolved, tab_overrides);
        return Some(resolved);
    }

    tab_stops.map(|tab_stops| tab_stops.to_vec())
}

fn merge_tab_stops(
    explicit_tab_stops: Option<&[TabStop]>,
    explicit_tab_overrides: Option<&[TabStopOverride]>,
    inherited_tab_stops: Option<&[TabStop]>,
) -> Option<Vec<TabStop>> {
    if let Some(explicit_tab_overrides) = explicit_tab_overrides {
        let mut resolved: Vec<TabStop> = inherited_tab_stops.unwrap_or(&[]).to_vec();
        apply_tab_stop_overrides(&mut resolved, explicit_tab_overrides);
        return Some(resolved);
    }

    explicit_tab_stops
        .map(|tab_stops| tab_stops.to_vec())
        .or_else(|| inherited_tab_stops.map(|tab_stops| tab_stops.to_vec()))
}

pub(super) fn apply_tab_stop_overrides(
    tab_stops: &mut Vec<TabStop>,
    tab_overrides: &[TabStopOverride],
) {
    for tab_override in tab_overrides {
        match tab_override {
            TabStopOverride::Set(tab_stop) => {
                tab_stops.retain(|existing| {
                    !tab_stop_positions_match(existing.position, tab_stop.position)
                });
                tab_stops.push(*tab_stop);
            }
            TabStopOverride::Clear(position) => {
                tab_stops
                    .retain(|existing| !tab_stop_positions_match(existing.position, *position));
            }
        }
    }

    tab_stops.sort_by(|left, right| {
        left.position
            .partial_cmp(&right.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn tab_stop_positions_match(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.01
}

/// Look up the pStyle reference from a paragraph's property.
pub(super) fn get_paragraph_style_id(prop: &docx_rs::ParagraphProperty) -> Option<&str> {
    prop.style.as_ref().map(|style| style.val.as_str())
}
