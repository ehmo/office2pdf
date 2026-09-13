//! Bounded font requests from the same DOCX IR used by conversion.
//! This inventory does not claim shaping or rendered-face verification.

use std::collections::{BTreeMap, BTreeSet};
use crate::ir::{Block, Document, HFInline, HeaderFooter, Page, Run, Table, TextStyle};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Request {
    pub family: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Inventory {
    pub requests: BTreeMap<Request, BTreeSet<u32>>,
    pub gaps: BTreeSet<&'static str>,
    pub runs: usize,
    pub scalars: usize,
}

impl Inventory {
    fn text(&mut self, text: &str, style: &TextStyle) -> Result<(), &'static str> {
        if text.is_empty() { return Ok(()); }
        self.runs += 1;
        if self.runs > 100_000 { return Err("font_requests_limit"); }
        let family = style.font_family.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if family.is_some_and(|s| s.len() > 512) { return Err("font_requests_limit"); }
        if family.is_none() { self.gaps.insert("unspecified-family"); }
        if style.east_asian_font_family.is_some() { self.gaps.insert("script-specific-family"); }
        if style.small_caps == Some(true) { self.gaps.insert("small-caps-shaping"); }
        let key = Request { family: family.map(str::to_owned), bold: style.bold.unwrap_or(false),
            italic: style.italic.unwrap_or(false) };
        if !self.requests.contains_key(&key) && self.requests.len() >= 128 {
            return Err("font_requests_limit");
        }
        let points = self.requests.entry(key).or_default();
        for scalar in text.chars() {
            self.scalars += 1;
            if self.scalars > 1_048_576 { return Err("font_requests_limit"); }
            if matches!(scalar, '\n' | '\r' | '\t') { continue; }
            if scalar.is_control() { self.gaps.insert("control-character"); }
            if style.all_caps == Some(true) || style.small_caps == Some(true) {
                for upper in scalar.to_uppercase() { points.insert(upper as u32); }
            } else { points.insert(scalar as u32); }
            if points.len() > 4096 { return Err("font_requests_limit"); }
        }
        Ok(())
    }

    fn run(&mut self, run: &Run, depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_requests_limit"); }
        self.text(&run.text, &run.style)?;
        if let Some(notes) = &run.footnote {
            self.gaps.insert("generated-footnote-marker");
            for note in notes { self.run(note, depth + 1)?; }
        }
        Ok(())
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_requests_limit"); }
        for row in &table.rows {
            for cell in &row.cells { self.blocks(&cell.content, depth + 1)?; }
        }
        Ok(())
    }

    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<(), &'static str> {
        if depth > 64 { return Err("font_requests_limit"); }
        for block in blocks {
            match block {
                Block::Paragraph(p) => for run in &p.runs { self.run(run, depth + 1)?; },
                Block::Caption(c) => for run in &c.paragraph.runs { self.run(run, depth + 1)?; },
                Block::Table(t) => self.table(t, depth + 1)?,
                Block::FloatingTextBox(t) => self.blocks(&t.content, depth + 1)?,
                Block::List(list) => {
                    self.gaps.insert("generated-list-marker");
                    for item in &list.items {
                        for p in &item.content { for run in &p.runs { self.run(run, depth + 1)?; } }
                    }
                }
                Block::Chart(_) => { self.gaps.insert("chart-generated-text"); }
                Block::MathEquation(_) => { self.gaps.insert("math-generated-text"); }
                Block::TableOfContents(_) => { self.gaps.insert("toc-generated-text"); }
                Block::Image(_) | Block::InlineImages(_) | Block::FloatingImage(_)
                | Block::FloatingShape(_) | Block::PageBreak | Block::ColumnBreak => {}
            }
        }
        Ok(())
    }

    fn header(&mut self, header: &HeaderFooter) -> Result<(), &'static str> {
        for paragraph in &header.paragraphs {
            for element in &paragraph.elements {
                match element {
                    HFInline::Run(run) => self.run(run, 0)?,
                    HFInline::PageNumber(style) | HFInline::TotalPages(style) => {
                        self.text("0123456789", style)?;
                    }
                    HFInline::PositionedTab(tab) => {
                        if tab.leader != crate::ir::TabLeader::None { self.gaps.insert("tab-leader"); }
                    }
                    HFInline::Image(_) => {}
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn inspect(doc: &Document) -> Result<Inventory, &'static str> {
    if doc.pages.len() > 1600 { return Err("font_requests_limit"); }
    let mut out = Inventory::default();
    for page in &doc.pages {
        let Page::Flow(flow) = page else { out.gaps.insert("non-flow-page"); continue; };
        out.blocks(&flow.content, 0)?;
        for story in [&flow.header, &flow.footer, &flow.first_header, &flow.first_footer]
            .into_iter().flatten() { out.header(story)?; }
    }
    Ok(out)
}
