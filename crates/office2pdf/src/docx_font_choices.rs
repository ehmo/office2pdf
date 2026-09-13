//! Apply a complete DOCX font selection before layout.
//! Original document bytes are not rewritten. Uncollected generated text refuses.

use std::collections::{BTreeMap, BTreeSet};
use crate::ir::{Block, Document, HFInline, HeaderFooter, Page, Run, Table, TextStyle};
use crate::docx_font_requests::{inspect, Request};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub source_family: String,
    pub bold: bool,
    pub italic: bool,
    pub family: String,
}

fn valid_family(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn request(style: &TextStyle) -> Request {
    Request { family: style.font_family.as_deref().map(str::trim)
        .filter(|s| !s.is_empty()).map(str::to_owned),
        bold: style.bold.unwrap_or(false), italic: style.italic.unwrap_or(false) }
}

struct Selection<'a> {
    families: BTreeMap<Request, &'a str>,
    runs: usize,
}

impl Selection<'_> {
    fn text(&mut self, text: &str, style: &mut TextStyle) -> Result<(), &'static str> {
        if text.is_empty() { return Ok(()); }
        let target = self.families.get(&request(style)).ok_or("font_choices_missing_request")?;
        style.font_family = Some((*target).to_owned()); self.runs += 1; Ok(())
    }

    fn run(&mut self, run: &mut Run, depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_choices_limit"); }
        self.text(&run.text, &mut run.style)?;
        if let Some(notes) = &mut run.footnote {
            for note in notes { self.run(note, depth + 1)?; }
        }
        Ok(())
    }

    fn table(&mut self, table: &mut Table, depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_choices_limit"); }
        for row in &mut table.rows {
            for cell in &mut row.cells { self.blocks(&mut cell.content, depth + 1)?; }
        }
        Ok(())
    }

    fn blocks(&mut self, blocks: &mut [Block], depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_choices_limit"); }
        for block in blocks {
            match block {
                Block::Paragraph(p) => for run in &mut p.runs { self.run(run, depth + 1)?; },
                Block::Caption(c) => for run in &mut c.paragraph.runs { self.run(run, depth + 1)?; },
                Block::Table(t) => self.table(t, depth + 1)?,
                Block::FloatingTextBox(t) => self.blocks(&mut t.content, depth + 1)?,
                Block::List(list) => for item in &mut list.items {
                    for p in &mut item.content { for run in &mut p.runs { self.run(run, depth + 1)?; } }
                },
                Block::Chart(_) | Block::MathEquation(_) | Block::TableOfContents(_) => {
                    return Err("font_choices_uncollected");
                }
                Block::Image(_) | Block::InlineImages(_) | Block::FloatingImage(_)
                | Block::FloatingShape(_) | Block::PageBreak | Block::ColumnBreak => {}
            }
        }
        Ok(())
    }

    fn header(&mut self, header: &mut HeaderFooter) -> Result<(), &'static str> {
        for paragraph in &mut header.paragraphs {
            for element in &mut paragraph.elements {
                match element {
                    HFInline::Run(run) => self.run(run, 0)?,
                    HFInline::PageNumber(style) | HFInline::TotalPages(style) => {
                        self.text("0123456789", style)?;
                    }
                    HFInline::PositionedTab(_) | HFInline::Image(_) => {}
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn apply(
    doc: &mut Document,
    choices: &[Choice],
    face_covers: impl Fn(&Choice, &BTreeSet<u32>) -> bool,
) -> Result<(), &'static str> {
    if choices.len() > 128 { return Err("font_choices_limit"); }
    let inventory = inspect(doc)?;
    if !inventory.gaps.is_empty() { return Err("font_choices_uncollected"); }
    if choices.len() != inventory.requests.len() { return Err("font_choices_request_set"); }
    let mut selected = Selection { families: BTreeMap::new(), runs: 0 };
    for choice in choices {
        if !valid_family(&choice.source_family) || !valid_family(&choice.family) {
            return Err("font_choices_family");
        }
        let key = Request { family: Some(choice.source_family.clone()),
            bold: choice.bold, italic: choice.italic };
        let points = inventory.requests.get(&key).ok_or("font_choices_request_set")?;
        if selected.families.insert(key, &choice.family).is_some() {
            return Err("font_choices_duplicate");
        }
        if !face_covers(choice, points) { return Err("font_choices_face"); }
    }
    for page in &mut doc.pages {
        let Page::Flow(flow) = page else { return Err("font_choices_uncollected"); };
        selected.blocks(&mut flow.content, 0)?;
        for story in [&mut flow.header, &mut flow.footer, &mut flow.first_header, &mut flow.first_footer]
            .into_iter().flatten() { selected.header(story)?; }
    }
    if selected.runs != inventory.runs { return Err("font_choices_run_count"); }
    Ok(())
}
