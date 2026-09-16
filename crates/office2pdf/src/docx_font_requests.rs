//! Bounded font requests from the same DOCX IR used by conversion.
//! This inventory does not claim shaping or rendered-face verification.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::{
    Block, Document, HFInline, HeaderFooter, List, ListKind, Page, Run, Table, TextStyle,
};

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

fn intersect_marker_style(left: &TextStyle, right: &TextStyle) -> TextStyle {
    TextStyle {
        font_family: (left.font_family == right.font_family)
            .then(|| left.font_family.clone())
            .flatten(),
        font_size: (left.font_size == right.font_size)
            .then_some(left.font_size)
            .flatten(),
        bold: (left.bold == right.bold).then_some(left.bold).flatten(),
        italic: (left.italic == right.italic)
            .then_some(left.italic)
            .flatten(),
        color: (left.color == right.color).then_some(left.color).flatten(),
        letter_spacing: (left.letter_spacing == right.letter_spacing)
            .then_some(left.letter_spacing)
            .flatten(),
        pair_kerning: (left.pair_kerning == right.pair_kerning)
            .then_some(left.pair_kerning)
            .flatten(),
        ..TextStyle::default()
    }
}

fn common_list_level_text_style(list: &List, level: u32) -> TextStyle {
    let mut visible_styles = list
        .items
        .iter()
        .filter(|item| item.level == level)
        .flat_map(|item| item.content.iter())
        .flat_map(|paragraph| paragraph.runs.iter())
        .filter(|run| run.footnote.is_none() && !run.text.is_empty())
        .map(|run| &run.style);
    let Some(first_style) = visible_styles.next() else {
        return TextStyle::default();
    };
    visible_styles.fold(first_style.clone(), |common, style| {
        intersect_marker_style(&common, style)
    })
}

fn map_symbol_font_marker(font_family: &str, marker_text: &str) -> Option<&'static str> {
    let mut chars = marker_text.chars();
    let marker_char = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let normalized_family: String = font_family
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-')
        .flat_map(char::to_lowercase)
        .collect();
    match (normalized_family.as_str(), marker_char) {
        ("symbol", '\u{F0B7}') => Some("•"),
        ("wingdings", '\u{00D8}') => Some("➢"),
        ("wingdings", '\u{00E8}') => Some("➔"),
        ("wingdings", '\u{00FB}') => Some("✖"),
        ("wingdings", '\u{00FC}') => Some("✔"),
        ("wingdings", '\u{00FD}') => Some("☒"),
        ("wingdings", '\u{00FE}') => Some("☑"),
        _ => None,
    }
}

fn renderable_unordered_marker(
    marker_text: &str,
    marker_style: Option<&TextStyle>,
) -> (String, Option<TextStyle>) {
    let mut normalized_text = marker_text.to_string();
    let mut normalized_style = marker_style.cloned();
    if let Some(font_family) = marker_style.and_then(|style| style.font_family.as_deref())
        && let Some(mapped_text) = map_symbol_font_marker(font_family, marker_text)
    {
        normalized_text = mapped_text.to_string();
        if let Some(style) = normalized_style.as_mut() {
            style.font_family = None;
        }
    }
    if normalized_text
        .chars()
        .any(|character| ('\u{E000}'..='\u{F8FF}').contains(&character))
    {
        normalized_text = match normalized_text.chars().next() {
            Some('\u{F0B7}') => "•".to_string(),
            Some('\u{F0A7}') => "▪".to_string(),
            Some('\u{F0D8}') => "➢".to_string(),
            Some('\u{F076}') => "❖".to_string(),
            _ => "•".to_string(),
        };
    }
    (normalized_text, normalized_style)
}

pub(crate) fn list_marker_request(list: &List, level: u32) -> (String, TextStyle) {
    let configured = list.level_styles.get(&level);
    let mut style = common_list_level_text_style(list, level);
    let kind = configured.map(|value| value.kind).unwrap_or(list.kind);
    let marker = match kind {
        ListKind::Unordered => {
            let (marker, explicit) = renderable_unordered_marker(
                configured
                    .and_then(|value| value.marker_text.as_deref())
                    .filter(|value| !value.is_empty())
                    .unwrap_or("•"),
                configured.and_then(|value| value.marker_style.as_ref()),
            );
            if let Some(explicit) = explicit.as_ref() {
                style.merge_from(explicit);
            }
            marker
        }
        ListKind::Ordered => {
            if let Some(explicit) = configured.and_then(|value| value.marker_style.as_ref()) {
                style.merge_from(explicit);
            }
            let mut marker =
                "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyzIVXLCDMivxlcdm"
                    .to_string();
            if let Some(pattern) = configured.and_then(|entry| entry.numbering_pattern.as_deref()) {
                marker.push_str(pattern);
            } else {
                marker.push('.');
            }
            marker
        }
    };
    (marker, style)
}

impl Inventory {
    fn add_request(
        &mut self,
        family: Option<&str>,
        bold: bool,
        italic: bool,
        required: &BTreeSet<u32>,
    ) -> Result<(), &'static str> {
        let family = family.map(str::trim).filter(|value| !value.is_empty());
        if family.is_some_and(|value| value.len() > 512) {
            return Err("font_requests_limit");
        }
        if family.is_none() {
            self.gaps.insert("unspecified-family");
        }
        let key = Request {
            family: family.map(str::to_owned),
            bold,
            italic,
        };
        if !self.requests.contains_key(&key) && self.requests.len() >= 128 {
            return Err("font_requests_limit");
        }
        let points = self.requests.entry(key).or_default();
        points.extend(required.iter().copied());
        if points.len() > 4096 {
            return Err("font_requests_limit");
        }
        Ok(())
    }

    fn text(&mut self, text: &str, style: &TextStyle) -> Result<(), &'static str> {
        if text.is_empty() {
            return Ok(());
        }
        self.runs += 1;
        if self.runs > 100_000 {
            return Err("font_requests_limit");
        }
        if style.small_caps == Some(true) {
            self.gaps.insert("small-caps-shaping");
        }
        let mut points = BTreeSet::new();
        for scalar in text.chars() {
            self.scalars += 1;
            if self.scalars > 1_048_576 {
                return Err("font_requests_limit");
            }
            if matches!(scalar, '\n' | '\r' | '\t') {
                continue;
            }
            if scalar.is_control() {
                self.gaps.insert("control-character");
            }
            if style.all_caps == Some(true) || style.small_caps == Some(true) {
                for upper in scalar.to_uppercase() {
                    points.insert(upper as u32);
                }
            } else {
                points.insert(scalar as u32);
            }
            if points.len() > 4096 {
                return Err("font_requests_limit");
            }
        }
        let family = style.font_family.as_deref();
        let bold = style.bold.unwrap_or(false);
        let italic = style.italic.unwrap_or(false);
        self.add_request(family, bold, italic, &points)?;
        if let Some(east_asian) = style
            .east_asian_font_family
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| {
                family.map_or(true, |primary| !value.eq_ignore_ascii_case(primary.trim()))
            })
        {
            self.add_request(Some(east_asian), bold, italic, &points)?;
        }
        Ok(())
    }

    fn list_marker(&mut self, list: &List, level: u32) -> Result<(), &'static str> {
        let (marker, style) = list_marker_request(list, level);
        self.text(&marker, &style)
    }

    fn run(&mut self, run: &Run, depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_requests_limit");
        }
        self.text(&run.text, &run.style)?;
        if let Some(notes) = &run.footnote {
            self.gaps.insert("generated-footnote-marker");
            for note in notes {
                self.run(note, depth + 1)?;
            }
        }
        Ok(())
    }

    fn table(&mut self, table: &Table, depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_requests_limit");
        }
        for row in &table.rows {
            for cell in &row.cells {
                self.blocks(&cell.content, depth + 1)?;
            }
        }
        Ok(())
    }

    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_requests_limit");
        }
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    for run in &p.runs {
                        self.run(run, depth + 1)?;
                    }
                }
                Block::Caption(c) => {
                    for run in &c.paragraph.runs {
                        self.run(run, depth + 1)?;
                    }
                }
                Block::Table(t) => self.table(t, depth + 1)?,
                Block::FloatingTextBox(t) => self.blocks(&t.content, depth + 1)?,
                Block::List(list) => {
                    let levels: BTreeSet<u32> = list.items.iter().map(|item| item.level).collect();
                    for level in levels {
                        self.list_marker(list, level)?;
                    }
                    for item in &list.items {
                        for p in &item.content {
                            for run in &p.runs {
                                self.run(run, depth + 1)?;
                            }
                        }
                    }
                }
                Block::Chart(_) => {
                    self.gaps.insert("chart-generated-text");
                }
                Block::MathEquation(_) => {
                    self.gaps.insert("math-generated-text");
                }
                Block::TableOfContents(_) => {
                    self.gaps.insert("toc-generated-text");
                }
                Block::Image(_)
                | Block::InlineImages(_)
                | Block::FloatingImage(_)
                | Block::FloatingShape(_)
                | Block::PageBreak
                | Block::ColumnBreak => {}
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
                        if tab.leader != crate::ir::TabLeader::None {
                            self.gaps.insert("tab-leader");
                        }
                    }
                    HFInline::Image(_) => {}
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn inspect(doc: &Document) -> Result<Inventory, &'static str> {
    if doc.pages.len() > 1600 {
        return Err("font_requests_limit");
    }
    let mut out = Inventory::default();
    for page in &doc.pages {
        let Page::Flow(flow) = page else {
            out.gaps.insert("non-flow-page");
            continue;
        };
        out.blocks(&flow.content, 0)?;
        for section in &flow.continued {
            out.blocks(&section.content, 0)?;
        }
        for story in [
            &flow.header,
            &flow.footer,
            &flow.first_header,
            &flow.first_footer,
            &flow.even_header,
            &flow.even_footer,
        ]
        .into_iter()
        .flatten()
        {
            out.header(story)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ListItem, ListLevelStyle, Paragraph, ParagraphStyle};

    fn run(text: &str, style: TextStyle) -> Run {
        Run {
            text: text.to_string(),
            style,
            href: None,
            footnote: None,
        }
    }

    #[test]
    fn script_specific_family_becomes_a_second_bounded_request() {
        let style = TextStyle {
            font_family: Some("Arial".to_string()),
            east_asian_font_family: Some("Malgun Gothic".to_string()),
            ..TextStyle::default()
        };
        let mut inventory = Inventory::default();
        inventory.text("A表", &style).unwrap();
        assert!(inventory.gaps.is_empty());
        assert_eq!(inventory.requests.len(), 2);
        for points in inventory.requests.values() {
            assert_eq!(points, &BTreeSet::from(['A' as u32, '表' as u32]));
        }
    }

    #[test]
    fn generated_list_markers_are_requested_in_the_rendered_marker_style() {
        let body_style = TextStyle {
            font_family: Some("Arial".to_string()),
            ..TextStyle::default()
        };
        let marker_style = TextStyle {
            font_family: Some("Cambria".to_string()),
            bold: Some(true),
            ..TextStyle::default()
        };
        let list = List {
            kind: ListKind::Ordered,
            items: vec![ListItem {
                level: 0,
                start_at: Some(3),
                content: vec![Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![run("Item", body_style)],
                }],
            }],
            level_styles: BTreeMap::from([(
                0,
                ListLevelStyle {
                    kind: ListKind::Ordered,
                    numbering_pattern: Some("1)".to_string()),
                    full_numbering: false,
                    marker_text: None,
                    marker_style: Some(marker_style),
                },
            )]),
        };
        let mut inventory = Inventory::default();
        inventory.blocks(&[Block::List(list)], 0).unwrap();
        assert!(!inventory.gaps.contains("generated-list-marker"));
        let marker = inventory
            .requests
            .iter()
            .find(|(request, _)| request.family.as_deref() == Some("Cambria") && request.bold)
            .map(|(_, points)| points)
            .unwrap();
        assert!(
            ['0', '9', 'A', 'z', 'I', 'M', ')']
                .into_iter()
                .all(|value| marker.contains(&(value as u32)))
        );
    }
}
