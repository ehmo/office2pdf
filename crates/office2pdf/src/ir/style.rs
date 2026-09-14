/// The family class a font declares about itself, independent of its name.
///
/// OOXML states this on the very element that names the face, and it outranks
/// any guess drawn from the name: `Posterama` and `Avenir Next LT Pro` carry
/// no `sans` token yet both declare themselves sans-serif (issue #891).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredFontClass {
    Serif,
    SansSerif,
    Monospace,
}

/// Collection of named styles in the document.
#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    pub styles: Vec<NamedStyle>,
    /// The family classes the source declares, keyed by lowercased family
    /// name. A face that states its class outranks any guess drawn from its
    /// name when a substitute has to be chosen (issue #891).
    pub declared_font_classes: std::collections::HashMap<String, DeclaredFontClass>,
    /// Document default tab stop interval in points (`w:defaultTabStop`
    /// from `word/settings.xml`). `None` when the document does not
    /// declare one.
    pub default_tab_stop_pt: Option<f64>,
    /// `w:docDefaults/w:rPrDefault` resolved to a run style — the family and
    /// size body text takes when it states none of its own. A computed
    /// contents entry is laid out in this rather than in the heading's own
    /// formatting (issue #610).
    pub default_text: Option<TextStyle>,
}

/// A named style that can be referenced by paragraphs/runs.
#[derive(Debug, Clone)]
pub struct NamedStyle {
    pub id: String,
    pub name: String,
    pub paragraph: Option<ParagraphStyle>,
    pub text: Option<TextStyle>,
}

/// Paragraph-level formatting.
#[derive(Debug, Clone, Default)]
pub struct ParagraphStyle {
    pub alignment: Option<Alignment>,
    /// Word's `w:wordWrap`: whether a Hangul line breaks between eojeol
    /// (`Some(true)`) or at any syllable (`Some(false)`). `None` leaves the
    /// choice to the renderer's default, which is word-level for a
    /// non-justified paragraph. The property overrides the style chain, so it
    /// cannot be inferred from `pStyle` (issue #730).
    pub word_wrap: Option<bool>,
    pub indent_left: Option<f64>,
    pub indent_right: Option<f64>,
    pub indent_first_line: Option<f64>,
    pub line_spacing: Option<LineSpacing>,
    /// Font-relative top and bottom edges used to size each text line.
    ///
    /// This is distinct from line spacing: it describes the line's intrinsic
    /// ascent/descent, while `line_spacing` controls the distance between
    /// consecutive lines.
    pub line_box: Option<LineBox>,
    pub space_before: Option<f64>,
    pub space_after: Option<f64>,
    /// The effective Word paragraph style ID. It is retained only for
    /// properties whose meaning depends on adjacent paragraphs using the same
    /// style, such as `w:contextualSpacing`.
    pub paragraph_style_id: Option<String>,
    /// Word's `w:contextualSpacing`: this paragraph's before and after spacing
    /// is ignored next to a paragraph with the same effective style. `None`
    /// means the document and its style chain state no value.
    pub contextual_spacing: Option<bool>,
    /// Heading level (1 = H1, 2 = H2, ..., 6 = H6). When set, the paragraph
    /// is emitted as a Typst `#heading` element for proper PDF structure tagging.
    pub heading_level: Option<u8>,
    /// Text direction for bidirectional rendering (RTL for Arabic/Hebrew).
    pub direction: Option<TextDirection>,
    /// Custom tab stop positions for this paragraph.
    pub tab_stops: Option<Vec<TabStop>>,
    /// Paragraph-specific default tab interval in points. DrawingML paragraph
    /// and inherited list-level `@defTabSz` values override the renderer fallback.
    pub default_tab_stop_pt: Option<f64>,
    /// Paragraph-wide shading fill (`w:pPr/w:shd`), painted behind the full
    /// paragraph width like Word's code-block backgrounds.
    pub background: Option<Color>,
    /// Paragraph borders (`w:pPr/w:pBdr`), drawn around the full paragraph
    /// width like Word's heading rules and letterhead frames. Boxed to keep
    /// paragraph-carrying enum variants compact.
    pub border: Option<Box<super::elements::CellBorder>>,
    /// Each border side's `w:space`, in points: the gap Word leaves between
    /// the paragraph text and that rule. `None` when the paragraph has no
    /// border at all; a present border with no `w:space` yields zeros, which
    /// is the attribute's own default (issue #520). Boxed for the same reason
    /// `border` is — four more `f64` inline push the paragraph-carrying enum
    /// variants past clippy's size threshold.
    pub border_space: Option<Box<super::elements::Insets>>,
}

/// A custom tab stop definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStop {
    /// Position in points from the paragraph's rendered text origin. Parsers
    /// normalize format-specific coordinate origins before storing the stop.
    pub position: f64,
    /// Alignment of text at this tab stop.
    pub alignment: TabAlignment,
    /// Leader character filling the space before this tab stop.
    pub leader: TabLeader,
}

/// Tab stop alignment type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabAlignment {
    #[default]
    Left,
    Center,
    Right,
    Decimal,
}

/// Leader character for a tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabLeader {
    #[default]
    None,
    Dot,
    Hyphen,
    Underscore,
}

/// Text direction for bidirectional (BiDi) rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    /// Left-to-right (default for Latin, CJK scripts).
    Ltr,
    /// Right-to-left (Arabic, Hebrew scripts).
    Rtl,
}

/// Text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
    Justify,
}

/// Line spacing specification.
#[derive(Debug, Clone, Copy)]
pub enum LineSpacing {
    /// Multiplier (e.g. 1.0 = single, 1.5, 2.0 = double).
    Proportional(f64),
    /// Exact spacing in points.
    Exact(f64),
}

/// Font-relative line box metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    /// Distance above the baseline, in em units.
    pub ascent_em: f64,
    /// Distance below the baseline, in em units.
    pub descent_em: f64,
}

/// A run's vertical displacement as a fraction of its own font size.
/// Positive values raise the run; negative values lower it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaselineShiftEm(pub f64);

/// Vertical alignment for superscript/subscript text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalTextAlign {
    Superscript,
    Subscript,
}

/// Whether the source format asks for OpenType pair kerning, and from which
/// size up.
///
/// This is not a boolean in either Office format. Word's `w:kern`
/// (ECMA-376 §17.3.2.15) and PowerPoint's `a:rPr/@kern` both carry a *size
/// threshold*: pair kerning is applied only to text at or above that point
/// size. Word states it in half-points, PowerPoint in hundredths of a point;
/// both are normalised to points here so the IR carries one unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PairKerning {
    /// The application never kerns this run — its threshold is zero, which
    /// Word writes as `w:kern w:val="0"`, or the document states no threshold
    /// at all, which is Word's own default. Absence resolves to this only at
    /// the `w:docDefaults` level; below it, absence is inheritance and leaves
    /// the whole decision `None`.
    Never,
    /// The application kerns this run only when its size reaches this many
    /// points.
    AtOrAbovePt(f64),
}

/// The size a run is laid out at when it states none of its own. Word's
/// `w:docDefaults` ships 10pt, and a threshold comparison needs *some* size to
/// answer; a run whose size is unknown is body text, which sits below every
/// threshold a document realistically states.
const UNSTATED_FONT_SIZE_PT: f64 = 10.0;

impl PairKerning {
    /// Whether kerning applies to a run set at `font_size_pt`.
    ///
    /// The comparison is inclusive: Word kerns text *at* the threshold, so a
    /// 14pt run under `w:kern w:val="28"` (= 14pt) is kerned.
    pub fn applies_at(self, font_size_pt: Option<f64>) -> bool {
        match self {
            PairKerning::Never => false,
            PairKerning::AtOrAbovePt(threshold_pt) => {
                font_size_pt.unwrap_or(UNSTATED_FONT_SIZE_PT) >= threshold_pt
            }
        }
    }
}

/// Character-level formatting.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextStyle {
    pub font_family: Option<String>,
    /// `w:rFonts w:eastAsia`: the family Word shapes East Asian codepoints
    /// with, which is a different face from the one it shapes Latin with in
    /// the same run. Kept beside `font_family` rather than folded into it,
    /// because a run states both and needs both (issue #575).
    pub east_asian_font_family: Option<String>,
    pub font_size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub color: Option<Color>,
    /// Text highlight background color.
    pub highlight: Option<Color>,
    /// Superscript or subscript vertical alignment.
    pub vertical_align: Option<VerticalTextAlign>,
    /// Font-relative baseline displacement without automatic glyph scaling.
    pub baseline_shift: Option<BaselineShiftEm>,
    /// All caps: render text in uppercase.
    pub all_caps: Option<bool>,
    /// Small caps: render lowercase letters as smaller uppercase.
    pub small_caps: Option<bool>,
    /// Character spacing (letter spacing / tracking) in points.
    ///
    /// A non-zero value also switches ligatures and pair kerning off: both
    /// redistribute the inter-glyph spacing this field states, and a
    /// substituted face's pairs are not the ones the document was set in
    /// (issues #684, #864).
    pub letter_spacing: Option<f64>,
    /// Whether the source application kerns this run, and from which size up.
    ///
    /// `None` means the format states nothing about kerning, so this field
    /// alone leaves the renderer's own default standing. Only the DOCX path
    /// resolves it today (issue #628) — but it is no longer the only input to
    /// the decision: a non-zero `letter_spacing` switches kerning off
    /// regardless of this field's own answer, except under the RTL shaping
    /// exemption, and that reaches PPTX and XLSX too (issue #864).
    pub pair_kerning: Option<PairKerning>,
}

impl TextStyle {
    /// Merge fields from `other` into `self`. For each field, if `other` has
    /// `Some(value)`, it overwrites `self`'s value. Fields that are `None` in
    /// `other` are left unchanged.
    pub fn merge_from(&mut self, other: &TextStyle) {
        if other.font_family.is_some() {
            self.font_family = other.font_family.clone();
        }
        if other.east_asian_font_family.is_some() {
            self.east_asian_font_family = other.east_asian_font_family.clone();
        }
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strikethrough.is_some() {
            self.strikethrough = other.strikethrough;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.highlight.is_some() {
            self.highlight = other.highlight;
        }
        if other.vertical_align.is_some() {
            self.vertical_align = other.vertical_align;
        }
        if other.baseline_shift.is_some() {
            self.baseline_shift = other.baseline_shift;
        }
        if other.all_caps.is_some() {
            self.all_caps = other.all_caps;
        }
        if other.small_caps.is_some() {
            self.small_caps = other.small_caps;
        }
        if other.letter_spacing.is_some() {
            self.letter_spacing = other.letter_spacing;
        }
        if other.pair_kerning.is_some() {
            self.pair_kerning = other.pair_kerning;
        }
    }
}

impl ParagraphStyle {
    /// Merge fields from `other` into `self`. For each field, if `other` has
    /// `Some(value)`, it overwrites `self`'s value. Fields that are `None` in
    /// `other` are left unchanged.
    pub fn merge_from(&mut self, other: &ParagraphStyle) {
        if other.alignment.is_some() {
            self.alignment = other.alignment;
        }
        if other.word_wrap.is_some() {
            self.word_wrap = other.word_wrap;
        }
        if other.indent_left.is_some() {
            self.indent_left = other.indent_left;
        }
        if other.indent_right.is_some() {
            self.indent_right = other.indent_right;
        }
        if other.indent_first_line.is_some() {
            self.indent_first_line = other.indent_first_line;
        }
        if other.line_spacing.is_some() {
            self.line_spacing = other.line_spacing;
        }
        if other.line_box.is_some() {
            self.line_box = other.line_box;
        }
        if other.space_before.is_some() {
            self.space_before = other.space_before;
        }
        if other.space_after.is_some() {
            self.space_after = other.space_after;
        }
        if other.paragraph_style_id.is_some() {
            self.paragraph_style_id = other.paragraph_style_id.clone();
        }
        if other.contextual_spacing.is_some() {
            self.contextual_spacing = other.contextual_spacing;
        }
        if other.heading_level.is_some() {
            self.heading_level = other.heading_level;
        }
        if other.direction.is_some() {
            self.direction = other.direction;
        }
        if other.tab_stops.is_some() {
            self.tab_stops = other.tab_stops.clone();
        }
        if other.default_tab_stop_pt.is_some() {
            self.default_tab_stop_pt = other.default_tab_stop_pt;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.border.is_some() {
            self.border = other.border.clone();
        }
    }
}

/// RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Create a color from RGB components.
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Black (`#000000`).
    pub fn black() -> Self {
        Self { r: 0, g: 0, b: 0 }
    }

    /// White (`#FFFFFF`).
    pub fn white() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
        }
    }
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
