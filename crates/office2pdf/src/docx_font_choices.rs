//! Apply a complete DOCX font selection before layout.
//! Original document bytes are not rewritten. Uncollected generated text refuses.

use std::collections::{BTreeMap, BTreeSet};

use crate::docx_font_requests::{Request, inspect, list_marker_request};
use crate::ir::{Block, Document, HFInline, HeaderFooter, List, Page, Run, Table, TextStyle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    pub source_family: String,
    pub bold: bool,
    pub italic: bool,
    pub family: String,
}

fn valid_family(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn request_for(family: Option<&str>, style: &TextStyle) -> Request {
    Request {
        family: family
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        bold: style.bold.unwrap_or(false),
        italic: style.italic.unwrap_or(false),
    }
}

struct Selection<'a> {
    families: BTreeMap<Request, &'a str>,
    runs: usize,
}

impl Selection<'_> {
    fn target(&self, family: Option<&str>, style: &TextStyle) -> Result<String, &'static str> {
        self.families
            .get(&request_for(family, style))
            .map(|target| (*target).to_owned())
            .ok_or("font_choices_missing_request")
    }

    fn text(&mut self, text: &str, style: &mut TextStyle) -> Result<(), &'static str> {
        if text.is_empty() {
            return Ok(());
        }
        let source_family = style.font_family.clone();
        let source_east_asian = style.east_asian_font_family.clone();
        let target = self.target(source_family.as_deref(), style)?;
        let east_asian_target = match source_east_asian.as_deref().map(str::trim) {
            Some(east_asian)
                if !east_asian.is_empty()
                    && source_family
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|primary| east_asian.eq_ignore_ascii_case(primary)) =>
            {
                Some(target.clone())
            }
            Some(east_asian) if !east_asian.is_empty() => {
                Some(self.target(Some(east_asian), style)?)
            }
            _ => None,
        };
        style.font_family = Some(target);
        if east_asian_target.is_some() {
            style.east_asian_font_family = east_asian_target;
        }
        self.runs += 1;
        Ok(())
    }

    fn list_marker(&mut self, list: &mut List, level: u32) -> Result<(), &'static str> {
        let (marker, mut style) = list_marker_request(list, level);
        self.text(&marker, &mut style)?;
        if let Some(configured) = list.level_styles.get_mut(&level) {
            if configured.kind == crate::ir::ListKind::Unordered {
                configured.marker_text = Some(marker);
            }
            configured.marker_style = Some(style);
        }
        Ok(())
    }

    fn run(&mut self, run: &mut Run, depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_choices_limit");
        }
        self.text(&run.text, &mut run.style)?;
        if let Some(notes) = &mut run.footnote {
            for note in notes {
                self.run(note, depth + 1)?;
            }
        }
        Ok(())
    }

    fn table(&mut self, table: &mut Table, depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_choices_limit");
        }
        for row in &mut table.rows {
            for cell in &mut row.cells {
                self.blocks(&mut cell.content, depth + 1)?;
            }
        }
        Ok(())
    }

    fn blocks(&mut self, blocks: &mut [Block], depth: usize) -> Result<(), &'static str> {
        if depth > 64 {
            return Err("font_choices_limit");
        }
        for block in blocks {
            match block {
                Block::Paragraph(p) => {
                    for run in &mut p.runs {
                        self.run(run, depth + 1)?;
                    }
                }
                Block::Caption(c) => {
                    for run in &mut c.paragraph.runs {
                        self.run(run, depth + 1)?;
                    }
                }
                Block::Table(t) => self.table(t, depth + 1)?,
                Block::FloatingTextBox(t) => self.blocks(&mut t.content, depth + 1)?,
                Block::List(list) => {
                    let levels: BTreeSet<u32> = list.items.iter().map(|item| item.level).collect();
                    for level in levels {
                        self.list_marker(list, level)?;
                    }
                    for item in &mut list.items {
                        for p in &mut item.content {
                            for run in &mut p.runs {
                                self.run(run, depth + 1)?;
                            }
                        }
                    }
                }
                Block::Chart(_) | Block::MathEquation(_) | Block::TableOfContents(_) => {
                    return Err("font_choices_uncollected");
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
    if choices.len() > 128 {
        return Err("font_choices_limit");
    }
    let inventory = inspect(doc)?;
    if !inventory.gaps.is_empty() {
        return Err("font_choices_uncollected");
    }
    if choices.len() != inventory.requests.len() {
        return Err("font_choices_request_set");
    }
    let mut selected = Selection {
        families: BTreeMap::new(),
        runs: 0,
    };
    for choice in choices {
        if !valid_family(&choice.source_family) || !valid_family(&choice.family) {
            return Err("font_choices_family");
        }
        let key = Request {
            family: Some(choice.source_family.clone()),
            bold: choice.bold,
            italic: choice.italic,
        };
        let points = inventory
            .requests
            .get(&key)
            .ok_or("font_choices_request_set")?;
        if selected.families.insert(key, &choice.family).is_some() {
            return Err("font_choices_duplicate");
        }
        if !face_covers(choice, points) {
            return Err("font_choices_face");
        }
    }
    for page in &mut doc.pages {
        let Page::Flow(flow) = page else {
            return Err("font_choices_uncollected");
        };
        selected.blocks(&mut flow.content, 0)?;
        for section in &mut flow.continued {
            selected.blocks(&mut section.content, 0)?;
        }
        for story in [
            &mut flow.header,
            &mut flow.footer,
            &mut flow.first_header,
            &mut flow.first_footer,
            &mut flow.even_header,
            &mut flow.even_footer,
        ]
        .into_iter()
        .flatten()
        {
            selected.header(story)?;
        }
    }
    if selected.runs != inventory.runs {
        return Err("font_choices_run_count");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ListItem, ListKind, ListLevelStyle, Paragraph, ParagraphStyle};

    fn selection<'a>(entries: Vec<(Request, &'a str)>) -> Selection<'a> {
        Selection {
            families: entries.into_iter().collect(),
            runs: 0,
        }
    }

    #[test]
    fn applies_independent_primary_and_east_asian_choices() {
        let mut selected = selection(vec![
            (
                Request {
                    family: Some("Arial".to_string()),
                    bold: false,
                    italic: false,
                },
                "Liberation Sans",
            ),
            (
                Request {
                    family: Some("Malgun Gothic".to_string()),
                    bold: false,
                    italic: false,
                },
                "Noto Sans CJK KR",
            ),
        ]);
        let mut style = TextStyle {
            font_family: Some("Arial".to_string()),
            east_asian_font_family: Some("Malgun Gothic".to_string()),
            ..TextStyle::default()
        };
        selected.text("A表", &mut style).unwrap();
        assert_eq!(style.font_family.as_deref(), Some("Liberation Sans"));
        assert_eq!(
            style.east_asian_font_family.as_deref(),
            Some("Noto Sans CJK KR")
        );
        assert_eq!(selected.runs, 1);
    }

    #[test]
    fn applies_the_reviewed_choice_to_a_normalized_symbol_marker() {
        let body_style = TextStyle {
            font_family: Some("Arial".to_string()),
            ..TextStyle::default()
        };
        let marker_style = TextStyle {
            font_family: Some("Wingdings".to_string()),
            ..TextStyle::default()
        };
        let mut list = List {
            kind: ListKind::Unordered,
            items: vec![ListItem {
                level: 0,
                start_at: None,
                content: vec![Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "Item".to_string(),
                        style: body_style,
                        href: None,
                        footnote: None,
                    }],
                }],
            }],
            level_styles: BTreeMap::from([(
                0,
                ListLevelStyle {
                    kind: ListKind::Unordered,
                    numbering_pattern: None,
                    full_numbering: false,
                    marker_text: Some("Ø".to_string()),
                    marker_style: Some(marker_style),
                },
            )]),
        };
        let mut selected = selection(vec![(
            Request {
                family: Some("Arial".to_string()),
                bold: false,
                italic: false,
            },
            "Liberation Sans",
        )]);
        selected.list_marker(&mut list, 0).unwrap();
        let configured = list.level_styles.get(&0).unwrap();
        assert_eq!(configured.marker_text.as_deref(), Some("➢"));
        assert_eq!(
            configured
                .marker_style
                .as_ref()
                .and_then(|style| style.font_family.as_deref()),
            Some("Liberation Sans")
        );
        assert_eq!(selected.runs, 1);
    }
}
