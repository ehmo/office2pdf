use super::elements::Block;
use super::style::StyleSheet;

/// Top-level document model produced by parsers and consumed by the renderer.
#[derive(Debug, Clone)]
pub struct Document {
    pub metadata: Metadata,
    pub pages: Vec<Page>,
    pub styles: StyleSheet,
}

/// Document metadata extracted from OOXML `docProps/core.xml` (Dublin Core).
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

/// A page in the document — variant depends on source format.
#[derive(Debug, Clone)]
pub enum Page {
    /// DOCX: flowing text pages.
    Flow(FlowPage),
    /// PPTX: fixed coordinate pages.
    Fixed(FixedPage),
    /// XLSX: spreadsheet sheet pages.
    Sheet(SheetPage),
}

/// Page dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// Width in points (1 pt = 1/72 inch).
    pub width: f64,
    /// Height in points.
    pub height: f64,
}

impl Default for PageSize {
    fn default() -> Self {
        Self {
            width: crate::defaults::A4_WIDTH_PT,
            height: crate::defaults::A4_HEIGHT_PT,
        }
    }
}

/// Page margins in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            top: crate::defaults::DEFAULT_MARGIN_PT,
            bottom: crate::defaults::DEFAULT_MARGIN_PT,
            left: crate::defaults::DEFAULT_MARGIN_PT,
            right: crate::defaults::DEFAULT_MARGIN_PT,
        }
    }
}

/// Column layout configuration for multi-column sections.
#[derive(Debug, Clone)]
pub struct ColumnLayout {
    /// Number of columns (must be >= 2 for multi-column layout).
    pub num_columns: u32,
    /// Spacing between columns in points (gutter width).
    pub spacing: f64,
    /// Optional per-column widths in points. When `None`, columns are equal width.
    pub column_widths: Option<Vec<f64>>,
}

/// A section a continuous break placed on a page a previous section started.
///
/// It cannot be a [`FlowPage`] of its own: every flow page emits a `#set page`,
/// and in Typst that rule starts the page it is supposed to stay on. Only the
/// column layout survives the fold, because that is the setting Word lets a
/// continuous section change without taking a new page.
#[derive(Debug, Clone)]
pub struct ContinuedSection {
    /// The continued section's own `w:cols`, when it states a different one.
    pub columns: Option<ColumnLayout>,
    pub content: Vec<Block>,
}

/// A flowing-content page (DOCX).
#[derive(Debug, Clone)]
pub struct FlowPage {
    pub size: PageSize,
    pub margins: Margins,
    pub content: Vec<Block>,
    pub header: Option<super::elements::HeaderFooter>,
    pub footer: Option<super::elements::HeaderFooter>,
    /// The header this section's **first** page takes, where `<w:titlePg/>`
    /// asks for one. `None` means every page takes [`FlowPage::header`]
    /// (issue #846).
    pub first_header: Option<super::elements::HeaderFooter>,
    /// The footer this section's first page takes, under the same rule.
    pub first_footer: Option<super::elements::HeaderFooter>,
    /// The header this section's **even-numbered** pages take, where
    /// `word/settings.xml` carries `<w:evenAndOddHeaders/>`. `None` means every
    /// page takes [`FlowPage::header`]. Without that setting Word ignores an
    /// `even` story completely, so a section that declares one and does not
    /// switch the setting on leaves this `None` too.
    pub even_header: Option<super::elements::HeaderFooter>,
    /// The footer this section's even-numbered pages take, under the same rule.
    pub even_footer: Option<super::elements::HeaderFooter>,
    /// Sections a continuous break folded onto this page, laid out in order
    /// after [`FlowPage::content`].
    pub continued: Vec<ContinuedSection>,
    /// Optional multi-column layout for the page.
    pub columns: Option<ColumnLayout>,
    /// Word document-grid line pitch in points (`w:docGrid w:linePitch`),
    /// present whenever the section carries a `w:docGrid` at all. That bare
    /// presence marks the file as authored in an East Asian Word edition,
    /// which is what decides the default tab stop (issue #393).
    pub line_grid_pitch: Option<f64>,
    /// Whether that grid snaps body lines to the pitch. Only a `w:docGrid
    /// w:type` of `lines`, `linesAndChars`, or `snapToChars` does; the
    /// `default` type an omitted attribute implies declares a pitch that Word
    /// then ignores for layout (issue #518).
    pub line_grid_snaps_lines: bool,
    /// Section page numbering (`w:sectPr/w:pgNumType`): where the counter
    /// restarts, and which numerals a `PAGE` field renders. `None` when the
    /// section declares nothing and simply continues.
    pub page_numbering: Option<PageNumbering>,
}

/// A section's `w:pgNumType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageNumbering {
    /// `w:start`: the number this section's first page takes. `None` continues
    /// from the previous section.
    pub start: Option<u32>,
    /// `w:fmt`: the numerals a `PAGE` field renders in.
    pub format: PageNumberFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageNumberFormat {
    #[default]
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerLetter,
    UpperLetter,
}

impl PageNumberFormat {
    /// The Typst numbering pattern that renders this format.
    pub fn typst_pattern(self) -> &'static str {
        match self {
            PageNumberFormat::Decimal => "1",
            PageNumberFormat::LowerRoman => "i",
            PageNumberFormat::UpperRoman => "I",
            PageNumberFormat::LowerLetter => "a",
            PageNumberFormat::UpperLetter => "A",
        }
    }
}

/// A fixed-layout page (PPTX slides).
#[derive(Debug, Clone)]
pub struct FixedPage {
    pub size: PageSize,
    pub elements: Vec<FixedElement>,
    /// Optional background color for the page.
    pub background_color: Option<super::style::Color>,
    /// Optional gradient background (takes precedence over `background_color` when present).
    pub background_gradient: Option<super::elements::GradientFill>,
}

/// An element with fixed position on a page.
#[derive(Debug, Clone)]
pub struct FixedElement {
    /// X position in points from left edge.
    pub x: f64,
    /// Y position in points from top edge.
    pub y: f64,
    /// Width in points.
    pub width: f64,
    /// Height in points.
    pub height: f64,
    /// The content of this element.
    pub kind: FixedElementKind,
}

/// Types of fixed-position elements.
#[derive(Debug, Clone)]
pub enum FixedElementKind {
    TextBox(super::elements::TextBoxData),
    Image(super::elements::ImageData),
    Shape(super::elements::Shape),
    Table(super::elements::Table),
    SmartArt(super::elements::SmartArt),
    /// Boxed: `Chart` is much the largest variant, and carrying it inline
    /// made every `FixedElement` pay for it (clippy's
    /// `large_enum_variant`).
    Chart(Box<super::elements::Chart>),
}

/// A spreadsheet sheet page (XLSX sheets).
#[derive(Debug, Clone)]
pub struct SheetPage {
    pub name: String,
    pub size: PageSize,
    pub margins: Margins,
    pub table: super::elements::Table,
    pub header: Option<super::elements::HeaderFooter>,
    pub footer: Option<super::elements::HeaderFooter>,
    /// Charts anchored within this sheet, stored as (anchor_row, chart) where
    /// `anchor_row` is the 1-indexed row number after which the chart is rendered.
    pub charts: Vec<(u32, super::elements::Chart)>,
    /// Drawing images anchored within this sheet.
    pub images: Vec<SheetImage>,
    /// Drawing text boxes anchored within this sheet.
    pub text_boxes: Vec<SheetTextBox>,
}

/// A worksheet text box anchored to a sheet row.
#[derive(Debug, Clone)]
pub struct SheetTextBox {
    /// 1-indexed anchor row. Used only to order drawings deterministically;
    /// placement comes from `x_offset_pt`/`y_offset_pt` (issue #474).
    pub anchor_row: u32,
    /// Horizontal offset of the anchor from the sheet's left edge, points.
    pub x_offset_pt: f64,
    /// Vertical offset of the anchor from the sheet's content top, points
    /// (issue #474).
    pub y_offset_pt: f64,
    pub width: f64,
    pub height: f64,
    pub paragraphs: Vec<super::elements::Paragraph>,
    /// Box fill color.
    pub fill: Option<super::style::Color>,
    /// Box outline.
    pub border: Option<super::elements::BorderSide>,
    /// bodyPr anchor="ctr": center text vertically inside the box.
    pub vertical_center: bool,
}

/// A worksheet drawing image anchored to a sheet row.
#[derive(Debug, Clone)]
pub struct SheetImage {
    /// 1-indexed anchor row. Used only to order drawings deterministically;
    /// placement comes from `x_offset_pt`/`y_offset_pt` (issue #474).
    pub anchor_row: u32,
    /// Horizontal offset of the anchor from the sheet's left edge, points.
    pub x_offset_pt: f64,
    /// Vertical offset of the anchor from the sheet's content top, points.
    /// Excel overlays drawings on the grid at absolute worksheet
    /// coordinates rather than placing them between rows (issue #474).
    pub y_offset_pt: f64,
    pub image: super::elements::ImageData,
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
