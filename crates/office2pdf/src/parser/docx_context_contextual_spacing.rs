//! Word paragraph-style identity and contextual-spacing semantics recovered
//! from raw OOXML.
//!
//! The published docx-rs release does not expose `w:contextualSpacing`.
//! Keeping the paragraph's effective style beside that flag lets the renderer
//! suppress only the before/after contribution whose owner requests it.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in super::super) struct ParagraphContextualSpacing {
    pub(in super::super) style_id: Option<String>,
    pub(in super::super) enabled: Option<bool>,
}

#[derive(Clone, Debug, Default)]
struct StyleRule {
    based_on: Option<String>,
    contextual_spacing: Option<bool>,
}

fn attribute_value(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    name: &[u8],
) -> Option<String> {
    element.attributes().flatten().find_map(|attribute| {
        (attribute.key.local_name().as_ref() == name)
            .then(|| {
                attribute
                    .decode_and_unescape_value(reader.decoder())
                    .ok()
                    .map(|value| value.into_owned())
            })
            .flatten()
    })
}

fn on_off_value(reader: &Reader<&[u8]>, element: &BytesStart<'_>) -> bool {
    !matches!(
        attribute_value(reader, element, b"val").as_deref(),
        Some("0" | "false" | "off" | "no")
    )
}

fn paragraph_style(element: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Option<String> {
    let style_type = attribute_value(reader, element, b"type");
    (style_type.as_deref() == Some("paragraph"))
        .then(|| attribute_value(reader, element, b"styleId"))
        .flatten()
}

fn scan_style_rules(xml: Option<&str>) -> HashMap<String, StyleRule> {
    let Some(xml) = xml else {
        return HashMap::new();
    };
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut rules = HashMap::new();
    let mut current_id: Option<String> = None;
    let mut current_rule = StyleRule::default();
    let mut in_paragraph_properties = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => match element.local_name().as_ref() {
                b"style" => {
                    current_id = paragraph_style(&element, &reader);
                    current_rule = StyleRule::default();
                }
                b"pPr" if current_id.is_some() => in_paragraph_properties = true,
                b"basedOn" if current_id.is_some() => {
                    current_rule.based_on = attribute_value(&reader, &element, b"val");
                }
                b"contextualSpacing" if in_paragraph_properties => {
                    current_rule.contextual_spacing = Some(on_off_value(&reader, &element));
                }
                _ => {}
            },
            Ok(Event::Empty(element)) => match element.local_name().as_ref() {
                b"basedOn" if current_id.is_some() => {
                    current_rule.based_on = attribute_value(&reader, &element, b"val");
                }
                b"contextualSpacing" if in_paragraph_properties => {
                    current_rule.contextual_spacing = Some(on_off_value(&reader, &element));
                }
                _ => {}
            },
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"pPr" => in_paragraph_properties = false,
                b"style" => {
                    if let Some(style_id) = current_id.take() {
                        rules.insert(style_id, current_rule.clone());
                    }
                    current_rule = StyleRule::default();
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rules
}

fn inherited_contextual_spacing(
    style_id: &str,
    rules: &HashMap<String, StyleRule>,
) -> Option<bool> {
    let mut current = Some(style_id);
    let mut visited = HashSet::new();
    while let Some(style) = current {
        if !visited.insert(style.to_string()) {
            return None;
        }
        let rule = rules.get(style)?;
        if rule.contextual_spacing.is_some() {
            return rule.contextual_spacing;
        }
        current = rule.based_on.as_deref();
    }
    None
}

pub(in super::super) struct ContextualSpacingContext {
    values: Vec<ParagraphContextualSpacing>,
    cursor: Cell<usize>,
}

impl ContextualSpacingContext {
    pub(in super::super) fn from_xml(
        document_xml: Option<&str>,
        styles_xml: Option<&str>,
        default_style_id: Option<&str>,
    ) -> Self {
        let rules = scan_style_rules(styles_xml);
        let mut values = document_xml.map(Self::scan_document).unwrap_or_default();
        for value in &mut values {
            if value.style_id.is_none() {
                value.style_id = default_style_id.map(str::to_string);
            }
            if value.enabled.is_none() {
                value.enabled = value
                    .style_id
                    .as_deref()
                    .and_then(|style_id| inherited_contextual_spacing(style_id, &rules));
            }
        }
        Self {
            values,
            cursor: Cell::new(0),
        }
    }

    pub(in super::super) fn next(&self) -> ParagraphContextualSpacing {
        let index = self.cursor.get();
        self.cursor.set(index + 1);
        self.values.get(index).cloned().unwrap_or_default()
    }

    fn scan_document(xml: &str) -> Vec<ParagraphContextualSpacing> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut values = Vec::new();
        let mut paragraph_stack: Vec<usize> = Vec::new();
        let mut paragraph_properties: Option<usize> = None;
        let mut in_body = false;

        loop {
            match reader.read_event() {
                Ok(Event::Start(element)) => match element.local_name().as_ref() {
                    b"body" => in_body = true,
                    b"p" if in_body => {
                        values.push(ParagraphContextualSpacing::default());
                        paragraph_stack.push(values.len() - 1);
                    }
                    b"pPr" if !paragraph_stack.is_empty() => {
                        paragraph_properties = paragraph_stack.last().copied();
                    }
                    b"pStyle" if paragraph_properties.is_some() => {
                        let index = paragraph_properties.expect("checked above");
                        values[index].style_id = attribute_value(&reader, &element, b"val");
                    }
                    b"contextualSpacing" if paragraph_properties.is_some() => {
                        let index = paragraph_properties.expect("checked above");
                        values[index].enabled = Some(on_off_value(&reader, &element));
                    }
                    _ => {}
                },
                Ok(Event::Empty(element)) => match element.local_name().as_ref() {
                    b"p" if in_body => values.push(ParagraphContextualSpacing::default()),
                    b"pStyle" if paragraph_properties.is_some() => {
                        let index = paragraph_properties.expect("checked above");
                        values[index].style_id = attribute_value(&reader, &element, b"val");
                    }
                    b"contextualSpacing" if paragraph_properties.is_some() => {
                        let index = paragraph_properties.expect("checked above");
                        values[index].enabled = Some(on_off_value(&reader, &element));
                    }
                    _ => {}
                },
                Ok(Event::End(element)) => match element.local_name().as_ref() {
                    b"pPr" => paragraph_properties = None,
                    b"p" if in_body => {
                        paragraph_stack.pop();
                        paragraph_properties = None;
                    }
                    b"body" => in_body = false,
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_direct_inherited_default_and_disabled_contextual_spacing() {
        let styles = r#"<w:styles xmlns:w="w">
          <w:style w:type="paragraph" w:default="1" w:styleId="Normal"/>
          <w:style w:type="paragraph" w:styleId="ListBase"><w:pPr><w:contextualSpacing/></w:pPr></w:style>
          <w:style w:type="paragraph" w:styleId="ListParagraph"><w:basedOn w:val="ListBase"/></w:style>
        </w:styles>"#;
        let document = r#"<w:document xmlns:w="w"><w:body>
          <w:p><w:pPr><w:pStyle w:val="ListParagraph"/></w:pPr></w:p>
          <w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:contextualSpacing w:val="0"/></w:pPr></w:p>
          <w:p/>
        </w:body></w:document>"#;
        let context =
            ContextualSpacingContext::from_xml(Some(document), Some(styles), Some("Normal"));
        assert_eq!(
            context.next(),
            ParagraphContextualSpacing {
                style_id: Some("ListParagraph".to_string()),
                enabled: Some(true),
            }
        );
        assert_eq!(
            context.next(),
            ParagraphContextualSpacing {
                style_id: Some("ListParagraph".to_string()),
                enabled: Some(false),
            }
        );
        assert_eq!(
            context.next(),
            ParagraphContextualSpacing {
                style_id: Some("Normal".to_string()),
                enabled: None,
            }
        );
    }
}
