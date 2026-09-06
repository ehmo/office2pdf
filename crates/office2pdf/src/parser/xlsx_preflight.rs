//! Fail-closed checks for package features that the XLSX renderer does not draw.
//!
//! The checks follow the workbook graph. Worksheet-only features are checked
//! only on sheets that will print, and drawing parts count only when a printed
//! sheet names them. Executable package parts are rejected everywhere because
//! they are an input-safety property, not a print-layout property.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};

use quick_xml::Reader;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesStart, BytesText, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

use super::parse_cell_ref;
use super::print_headings::column_letters;
use super::xlsx_drawing::{
    parse_workbook_sheet_rids, resolve_relative_xl_path, sheet_part_dir, sheet_part_path,
    sheet_rels_path,
};
use crate::error::ConvertError;

const BROWSER_PACKAGE_BOUNDS: PackageBounds = PackageBounds {
    compressed_bytes: 256 * 1024 * 1024,
    entries: 10_000,
    total_uncompressed_bytes: 256 * 1024 * 1024,
    xml_uncompressed_bytes: 16 * 1024 * 1024,
    other_entry_uncompressed_bytes: 128 * 1024 * 1024,
};
const MAX_XLSX_ROWS: u32 = 1_048_576;
const MAX_XLSX_COLUMNS: u32 = 16_384;

#[derive(Clone, Copy)]
struct PackageBounds {
    compressed_bytes: usize,
    entries: usize,
    total_uncompressed_bytes: u64,
    xml_uncompressed_bytes: u64,
    other_entry_uncompressed_bytes: u64,
}

#[derive(Clone)]
struct Relationship {
    target: String,
    external: bool,
    kind: String,
}

#[derive(Debug, Default)]
struct WorksheetScan {
    drawing_rids: Vec<String>,
    dxf_ids: Vec<usize>,
    legacy_drawing_rids: Vec<String>,
}

#[derive(Default)]
struct DrawingAnchor {
    prints: bool,
    unsupported: Option<&'static str>,
    saw_shape: bool,
    in_shape_text: bool,
    shape_has_paragraph: bool,
    graphic_frame: bool,
    graphic_data_uri: Option<String>,
    chart_rids: Vec<String>,
}

impl DrawingAnchor {
    fn new() -> Self {
        Self {
            prints: true,
            ..Self::default()
        }
    }
}

fn unsupported(element: impl Into<String>) -> ConvertError {
    ConvertError::UnsupportedElement {
        format: "XLSX",
        element: element.into(),
    }
}

fn validate_package_bounds(data: &[u8], bounds: PackageBounds) -> Result<(), ConvertError> {
    if data.len() > bounds.compressed_bytes {
        return Err(unsupported("XLSX package exceeds browser safety limit"));
    }
    let mut archive = crate::parser::open_zip(data)?;
    if archive.len() > bounds.entries {
        return Err(unsupported("XLSX package exceeds browser safety limit"));
    }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            crate::parser::parse_err(format!("Failed to inspect XLSX package entry: {error}"))
        })?;
        let path = entry.name().trim_start_matches('/').to_ascii_lowercase();
        if path.is_empty() || !paths.insert(path.clone()) {
            return Err(unsupported("ambiguous XLSX package path"));
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| unsupported("XLSX package exceeds browser safety limit"))?;
        let entry_limit = if path.ends_with(".xml") || path.ends_with(".rels") {
            bounds.xml_uncompressed_bytes
        } else {
            bounds.other_entry_uncompressed_bytes
        };
        if size > entry_limit || total > bounds.total_uncompressed_bytes {
            return Err(unsupported("XLSX package exceeds browser safety limit"));
        }
    }
    Ok(())
}

pub(super) fn ensure_safe_package_bounds(data: &[u8]) -> Result<(), ConvertError> {
    validate_package_bounds(data, BROWSER_PACKAGE_BOUNDS)?;
    validate_xml_parts(data)?;
    validate_upstream_parser_inputs(data)
}

const SPREADSHEETML_MAIN_NAMESPACE: &[u8] =
    b"http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const PACKAGE_RELATIONSHIPS_NAMESPACE: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/relationships";

fn namespace_for_upstream_entry(name: &str) -> Option<&'static [u8]> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".rels") {
        Some(PACKAGE_RELATIONSHIPS_NAMESPACE)
    } else if lower.ends_with(".xml") {
        Some(SPREADSHEETML_MAIN_NAMESPACE)
    } else {
        None
    }
}

fn decoded_xml_bytes(bytes: &[u8]) -> Result<Cow<'_, [u8]>, ConvertError> {
    let encoding = if bytes.starts_with(&[0xfe, 0xff])
        || (bytes.len() >= 2 && bytes[0] == 0 && bytes[1] == b'<')
    {
        Some(true)
    } else if bytes.starts_with(&[0xff, 0xfe])
        || (bytes.len() >= 2 && bytes[0] == b'<' && bytes[1] == 0)
    {
        Some(false)
    } else {
        None
    };
    let Some(big_endian) = encoding else {
        return Ok(Cow::Borrowed(bytes));
    };

    let payload = if bytes.starts_with(&[0xfe, 0xff]) || bytes.starts_with(&[0xff, 0xfe]) {
        &bytes[2..]
    } else {
        bytes
    };
    if payload.len() % 2 != 0 {
        return Err(crate::parser::parse_err(
            "Failed to decode UTF-16 XLSX XML: odd byte count",
        ));
    }
    let units = payload
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect::<Vec<_>>();
    let mut xml = String::from_utf16(&units).map_err(|error| {
        crate::parser::parse_err(format!("Failed to decode UTF-16 XLSX XML: {error}"))
    })?;
    if xml.starts_with('\u{feff}') {
        xml.remove(0);
    }

    if let Some(declaration_end) = xml.find("?>") {
        let declaration = &xml[..declaration_end];
        let lower = declaration.to_ascii_lowercase();
        if let Some(encoding_start) = lower.find("encoding") {
            let after_name = &xml[encoding_start + "encoding".len()..declaration_end];
            if let Some(equals_offset) = after_name.find('=') {
                let after_equals = encoding_start + "encoding".len() + equals_offset + 1;
                let rest = &xml[after_equals..declaration_end];
                if let Some(quote_offset) = rest.find(['\'', '"']) {
                    let quote_index = after_equals + quote_offset;
                    let quote = xml.as_bytes()[quote_index] as char;
                    if let Some(end_offset) = xml[quote_index + 1..declaration_end].find(quote) {
                        let value_start = quote_index + 1;
                        let value_end = value_start + end_offset;
                        xml.replace_range(value_start..value_end, "UTF-8");
                    }
                }
            }
        }
    }

    Ok(Cow::Owned(xml.into_bytes()))
}

fn validate_xml_parts(data: &[u8]) -> Result<(), ConvertError> {
    let mut archive = crate::parser::open_zip(data)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::parser::parse_err(format!("Failed to inspect XLSX XML part: {error}"))
        })?;
        let name = entry.name().trim_start_matches('/').to_string();
        let lower = name.to_ascii_lowercase();
        if entry.is_dir()
            || !(lower.ends_with(".xml") || lower.ends_with(".rels") || lower.ends_with(".vml"))
        {
            continue;
        }

        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|error| {
            crate::parser::parse_err(format!("Failed to read XLSX XML part {name}: {error}"))
        })?;
        validate_xml_part(&name, decoded_xml_bytes(&bytes)?.as_ref())?;
    }
    Ok(())
}

fn validate_xml_part(name: &str, xml: &[u8]) -> Result<(), ConvertError> {
    let parse_error = |detail: &dyn std::fmt::Display| {
        crate::parser::parse_err(format!("Failed to parse XLSX XML part {name}: {detail}"))
    };
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event().map_err(|error| parse_error(&error))? {
            Event::Start(element) | Event::Empty(element) => {
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| parse_error(&error))?;
                    attribute
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|error| parse_error(&error))?;
                }
            }
            Event::Text(text) => {
                text.xml_content().map_err(|error| parse_error(&error))?;
            }
            Event::CData(text) => {
                text.xml_content().map_err(|error| parse_error(&error))?;
            }
            Event::GeneralRef(reference) => {
                let value: &[u8] = &reference;
                match value {
                    b"amp" | b"apos" | b"gt" | b"lt" | b"quot" => {}
                    _ if reference.is_char_ref() => {
                        reference
                            .resolve_char_ref()
                            .map_err(|error| parse_error(&error))?;
                    }
                    _ => return Err(parse_error(&"undefined entity reference")),
                }
            }
            Event::DocType(_) => {
                return Err(unsupported("XLSX XML document type declaration"));
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

fn is_ascii_whitespace(text: &BytesText<'_>) -> bool {
    text.iter().all(u8::is_ascii_whitespace)
}

fn has_paired_empty_sheet(xml: &[u8]) -> Result<bool, ConvertError> {
    let mut reader = Reader::from_reader(xml);
    let mut pending_sheet = false;
    loop {
        let event = reader.read_event().map_err(|error| {
            crate::parser::parse_err(format!("Failed to inspect XLSX workbook sheets: {error}"))
        })?;
        if pending_sheet {
            match &event {
                Event::End(element) if element.local_name().as_ref() == b"sheet" => {
                    return Ok(true);
                }
                Event::Text(text) if is_ascii_whitespace(text) => continue,
                Event::Comment(_) | Event::PI(_) => continue,
                _ => pending_sheet = false,
            }
        }
        match event {
            Event::Start(element) if element.local_name().as_ref() == b"sheet" => {
                pending_sheet = true;
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn normalize_paired_empty_sheets(xml: &[u8]) -> Result<Vec<u8>, ConvertError> {
    let mut reader = Reader::from_reader(xml);
    let mut writer = quick_xml::Writer::new(Vec::with_capacity(xml.len()));
    let mut pending_sheet: Option<(BytesStart<'static>, Vec<Event<'static>>)> = None;
    loop {
        let event = reader.read_event().map_err(|error| {
            crate::parser::parse_err(format!("Failed to normalize XLSX workbook sheets: {error}"))
        })?;
        if let Some((sheet, mut ignorable)) = pending_sheet.take() {
            if matches!(&event, Event::End(element) if element.local_name().as_ref() == b"sheet") {
                writer.write_event(Event::Empty(sheet)).map_err(|error| {
                    crate::parser::parse_err(format!(
                        "Failed to write normalized XLSX workbook sheet: {error}"
                    ))
                })?;
                for event in ignorable {
                    writer.write_event(event).map_err(|error| {
                        crate::parser::parse_err(format!(
                            "Failed to preserve XLSX workbook sheet trivia: {error}"
                        ))
                    })?;
                }
                continue;
            }
            if matches!(&event, Event::Text(text) if is_ascii_whitespace(text))
                || matches!(&event, Event::Comment(_) | Event::PI(_))
            {
                ignorable.push(event.into_owned());
                pending_sheet = Some((sheet, ignorable));
                continue;
            }
            writer.write_event(Event::Start(sheet)).map_err(|error| {
                crate::parser::parse_err(format!(
                    "Failed to write normalized XLSX workbook sheet: {error}"
                ))
            })?;
            for event in ignorable {
                writer.write_event(event).map_err(|error| {
                    crate::parser::parse_err(format!(
                        "Failed to preserve XLSX workbook sheet content: {error}"
                    ))
                })?;
            }
        }
        match event {
            Event::Start(element) if element.local_name().as_ref() == b"sheet" => {
                pending_sheet = Some((element.into_owned(), Vec::new()));
            }
            Event::Eof => break,
            event => writer.write_event(event.into_owned()).map_err(|error| {
                crate::parser::parse_err(format!(
                    "Failed to write normalized XLSX workbook XML: {error}"
                ))
            })?,
        }
    }
    if let Some((sheet, ignorable)) = pending_sheet {
        writer.write_event(Event::Start(sheet)).map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to write normalized XLSX workbook sheet: {error}"
            ))
        })?;
        for event in ignorable {
            writer.write_event(event).map_err(|error| {
                crate::parser::parse_err(format!(
                    "Failed to preserve XLSX workbook sheet content: {error}"
                ))
            })?;
        }
    }
    Ok(writer.into_inner())
}

fn is_worksheet_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("xl/worksheets/") && lower.ends_with(".xml")
}

fn worksheet_has_missing_references(xml: &[u8]) -> Result<bool, ConvertError> {
    let mut reader = Reader::from_reader(xml);
    let mut in_sheet_data = false;
    let mut in_row = false;
    let mut current_row = 0u32;
    loop {
        let event = reader.read_event().map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to inspect XLSX worksheet references: {error}"
            ))
        })?;
        match event {
            Event::Start(ref element) if element.local_name().as_ref() == b"sheetData" => {
                in_sheet_data = true;
            }
            Event::End(ref element) if element.local_name().as_ref() == b"sheetData" => {
                in_sheet_data = false;
                in_row = false;
            }
            Event::Start(ref element)
                if in_sheet_data && element.local_name().as_ref() == b"row" =>
            {
                let Some(row) = attr_value(&reader, element, b"r")
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|row| *row > 0 && *row <= MAX_XLSX_ROWS)
                else {
                    return Ok(true);
                };
                current_row = row;
                in_row = true;
            }
            Event::Empty(ref element)
                if in_sheet_data && element.local_name().as_ref() == b"row" =>
            {
                if attr_value(&reader, element, b"r").is_none_or(|value| {
                    value
                        .parse::<u32>()
                        .ok()
                        .is_none_or(|row| row == 0 || row > MAX_XLSX_ROWS)
                }) {
                    return Ok(true);
                }
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if in_row && element.local_name().as_ref() == b"c" =>
            {
                if attr_value(&reader, element, b"r").is_none_or(|reference| {
                    parse_cell_ref(&reference).is_none_or(|(column, row)| {
                        column == 0
                            || column > MAX_XLSX_COLUMNS
                            || row == 0
                            || row > MAX_XLSX_ROWS
                            || row != current_row
                    })
                }) {
                    return Ok(true);
                }
            }
            Event::End(ref element) if element.local_name().as_ref() == b"row" => {
                in_row = false;
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn normalize_cell_reference(
    reader: &Reader<&[u8]>,
    element: &mut BytesStart<'_>,
    current_row: u32,
    current_column: &mut u32,
) -> Result<(), ConvertError> {
    if let Some(reference) = attr_value(reader, element, b"r") {
        let (column, row) = parse_cell_ref(&reference)
            .filter(|(column, row)| {
                *column > 0 && *column <= MAX_XLSX_COLUMNS && *row > 0 && *row <= MAX_XLSX_ROWS
            })
            .ok_or_else(|| unsupported("worksheet cell reference outside XLSX bounds"))?;
        if row != current_row {
            return Err(unsupported(
                "worksheet cell reference does not match its row",
            ));
        }
        *current_column = column;
    } else {
        *current_column = current_column
            .checked_add(1)
            .filter(|column| *column <= MAX_XLSX_COLUMNS)
            .ok_or_else(|| unsupported("cannot infer worksheet cell beyond XLSX bounds"))?;
        let reference = format!("{}{}", column_letters(*current_column), current_row);
        element.push_attribute(("r", reference.as_str()));
    }
    Ok(())
}

fn normalize_worksheet_references(xml: &[u8]) -> Result<Vec<u8>, ConvertError> {
    let mut reader = Reader::from_reader(xml);
    let mut writer = quick_xml::Writer::new(Vec::with_capacity(xml.len()));
    let mut in_sheet_data = false;
    let mut in_row = false;
    let mut last_row = 0u32;
    let mut current_row = 0u32;
    let mut current_column = 0u32;
    loop {
        let event = reader.read_event().map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to normalize XLSX worksheet references: {error}"
            ))
        })?;
        let event = match event {
            Event::Start(element) if element.local_name().as_ref() == b"sheetData" => {
                in_sheet_data = true;
                Event::Start(element.into_owned())
            }
            Event::End(element) if element.local_name().as_ref() == b"sheetData" => {
                in_sheet_data = false;
                in_row = false;
                Event::End(element.into_owned())
            }
            Event::Start(mut element)
                if in_sheet_data && element.local_name().as_ref() == b"row" =>
            {
                let declared = attr_value(&reader, &element, b"r");
                current_row = match declared.as_deref() {
                    Some(value) => value
                        .parse::<u32>()
                        .ok()
                        .filter(|row| *row > 0 && *row <= MAX_XLSX_ROWS)
                        .ok_or_else(|| {
                            unsupported("worksheet row reference outside XLSX bounds")
                        })?,
                    None => last_row
                        .checked_add(1)
                        .filter(|row| *row <= MAX_XLSX_ROWS)
                        .ok_or_else(|| {
                            unsupported("cannot infer worksheet row beyond XLSX bounds")
                        })?,
                };
                if declared.is_none() {
                    let row = current_row.to_string();
                    element.push_attribute(("r", row.as_str()));
                }
                last_row = current_row;
                current_column = 0;
                in_row = true;
                Event::Start(element.into_owned())
            }
            Event::Empty(mut element)
                if in_sheet_data && element.local_name().as_ref() == b"row" =>
            {
                let declared = attr_value(&reader, &element, b"r");
                current_row = match declared.as_deref() {
                    Some(value) => value
                        .parse::<u32>()
                        .ok()
                        .filter(|row| *row > 0 && *row <= MAX_XLSX_ROWS)
                        .ok_or_else(|| {
                            unsupported("worksheet row reference outside XLSX bounds")
                        })?,
                    None => last_row
                        .checked_add(1)
                        .filter(|row| *row <= MAX_XLSX_ROWS)
                        .ok_or_else(|| {
                            unsupported("cannot infer worksheet row beyond XLSX bounds")
                        })?,
                };
                if declared.is_none() {
                    let row = current_row.to_string();
                    element.push_attribute(("r", row.as_str()));
                }
                last_row = current_row;
                current_column = 0;
                in_row = false;
                Event::Empty(element.into_owned())
            }
            Event::Start(mut element) if in_row && element.local_name().as_ref() == b"c" => {
                normalize_cell_reference(&reader, &mut element, current_row, &mut current_column)?;
                Event::Start(element.into_owned())
            }
            Event::Empty(mut element) if in_row && element.local_name().as_ref() == b"c" => {
                normalize_cell_reference(&reader, &mut element, current_row, &mut current_column)?;
                Event::Empty(element.into_owned())
            }
            Event::End(element) if element.local_name().as_ref() == b"row" => {
                in_row = false;
                Event::End(element.into_owned())
            }
            Event::Eof => break,
            event => event.into_owned(),
        };
        writer.write_event(event).map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to write normalized XLSX worksheet references: {error}"
            ))
        })?;
    }
    Ok(writer.into_inner())
}

fn has_prefixed_elements(xml: &[u8], namespace: &[u8]) -> Result<bool, ConvertError> {
    let mut reader = NsReader::from_reader(xml);
    loop {
        let (resolved, event) = reader.read_resolved_event().map_err(|error| {
            crate::parser::parse_err(format!("Failed to inspect XLSX namespace prefix: {error}"))
        })?;
        match event {
            Event::Start(element) | Event::Empty(element) => {
                if is_namespace(&resolved, namespace)
                    && element.name().as_ref() != element.local_name().as_ref()
                {
                    return Ok(true);
                }
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn normalize_prefixed_elements(xml: &[u8], namespace: &[u8]) -> Result<Vec<u8>, ConvertError> {
    let mut reader = NsReader::from_reader(xml);
    let mut writer = quick_xml::Writer::new(Vec::with_capacity(xml.len()));
    loop {
        let (resolved, event) = reader.read_resolved_event().map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to normalize XLSX namespace prefix: {error}"
            ))
        })?;
        let event = match event {
            Event::Start(mut element) if is_namespace(&resolved, namespace) => {
                let local_name = element.local_name().as_ref().to_vec();
                element.set_name(&local_name);
                Event::Start(element.into_owned())
            }
            Event::Empty(mut element) if is_namespace(&resolved, namespace) => {
                let local_name = element.local_name().as_ref().to_vec();
                element.set_name(&local_name);
                Event::Empty(element.into_owned())
            }
            Event::End(element) if is_namespace(&resolved, namespace) => {
                let local_name = std::str::from_utf8(element.local_name().as_ref())
                    .map_err(|error| {
                        crate::parser::parse_err(format!(
                            "Failed to decode XLSX element name: {error}"
                        ))
                    })?
                    .to_string();
                Event::End(quick_xml::events::BytesEnd::new(local_name))
            }
            Event::Eof => break,
            event => event.into_owned(),
        };
        writer.write_event(event).map_err(|error| {
            crate::parser::parse_err(format!("Failed to write normalized XLSX XML: {error}"))
        })?;
    }
    Ok(writer.into_inner())
}

/// Normalize namespace-qualified element names that umya-spreadsheet compares
/// as raw names. The original package remains the source for every independent
/// preflight.
pub(super) fn normalize_upstream_reader_inputs(data: &[u8]) -> Result<Cow<'_, [u8]>, ConvertError> {
    let mut archive = crate::parser::open_zip(data)?;
    let mut requires_normalization = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::parser::parse_err(format!("Failed to inspect XLSX namespace prefix: {error}"))
        })?;
        let Some(namespace) = namespace_for_upstream_entry(entry.name()) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|error| {
            crate::parser::parse_err(format!("Failed to read XLSX namespace prefix: {error}"))
        })?;
        let decoded = decoded_xml_bytes(&bytes)?;
        if decoded.as_ref() != bytes
            || has_prefixed_elements(decoded.as_ref(), namespace)?
            || (entry.name().eq_ignore_ascii_case("xl/workbook.xml")
                && has_paired_empty_sheet(decoded.as_ref())?)
            || (is_worksheet_entry(entry.name())
                && worksheet_has_missing_references(decoded.as_ref())?)
        {
            requires_normalization = true;
            break;
        }
    }
    if !requires_normalization {
        return Ok(Cow::Borrowed(data));
    }

    let mut archive = crate::parser::open_zip(data)?;
    let cursor = std::io::Cursor::new(Vec::with_capacity(data.len()));
    let mut writer = zip::ZipWriter::new(cursor);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::parser::parse_err(format!("Failed to normalize XLSX package entry: {error}"))
        })?;
        let name = entry.name().to_string();
        let options = zip::write::FileOptions::default().compression_method(entry.compression());
        if entry.is_dir() {
            writer.add_directory(name, options).map_err(|error| {
                crate::parser::parse_err(format!(
                    "Failed to normalize XLSX package directory: {error}"
                ))
            })?;
            continue;
        }
        let namespace = namespace_for_upstream_entry(&name);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|error| {
            crate::parser::parse_err(format!("Failed to normalize XLSX package data: {error}"))
        })?;
        if let Some(namespace) = namespace {
            let decoded = decoded_xml_bytes(&bytes)?;
            bytes = decoded.into_owned();
            if has_prefixed_elements(&bytes, namespace)? {
                bytes = normalize_prefixed_elements(&bytes, namespace)?;
            }
            if name.eq_ignore_ascii_case("xl/workbook.xml") && has_paired_empty_sheet(&bytes)? {
                bytes = normalize_paired_empty_sheets(&bytes)?;
            }
            if is_worksheet_entry(&name) && worksheet_has_missing_references(&bytes)? {
                bytes = normalize_worksheet_references(&bytes)?;
            }
        }
        writer.start_file(name, options).map_err(|error| {
            crate::parser::parse_err(format!("Failed to normalize XLSX package file: {error}"))
        })?;
        writer.write_all(&bytes).map_err(|error| {
            crate::parser::parse_err(format!("Failed to write normalized XLSX package: {error}"))
        })?;
    }
    let normalized = writer.finish().map_err(|error| {
        crate::parser::parse_err(format!("Failed to finish normalized XLSX package: {error}"))
    })?;
    Ok(Cow::Owned(normalized.into_inner()))
}

fn attr_value(reader: &Reader<&[u8]>, element: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.local_name().as_ref() == name)
        .and_then(|attribute| {
            attribute
                .decode_and_unescape_value(reader.decoder())
                .ok()
                .map(|value| value.into_owned())
        })
}

fn false_value(value: Option<String>) -> bool {
    value.is_some_and(|value| matches!(value.as_str(), "0" | "false" | "off"))
}

fn cell_is_operand_supported(formula: &str) -> bool {
    let formula = formula.trim();
    if formula.parse::<f64>().is_ok() {
        return true;
    }
    let Some(inner) = formula
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return false;
    };
    !inner.replace("\"\"", "").contains('"')
}

fn conditional_format_detail(sheet_name: &str) -> ConvertError {
    unsupported(format!(
        "unsupported conditional-format rule on printed sheet: {sheet_name}"
    ))
}

#[derive(Clone, Copy)]
struct PreflightRange {
    start_col: u32,
    start_row: u32,
    end_col: u32,
    end_row: u32,
}

impl PreflightRange {
    fn overlaps(self, other: Self) -> bool {
        self.start_col <= other.end_col
            && other.start_col <= self.end_col
            && self.start_row <= other.end_row
            && other.start_row <= self.end_row
    }
}

fn preflight_sqref(raw: &str) -> Option<Vec<PreflightRange>> {
    let ranges: Vec<PreflightRange> = raw
        .split_whitespace()
        .map(|part| {
            let (start, end) = part.split_once(':').unwrap_or((part, part));
            let (start_col, start_row) = parse_cell_ref(start)?;
            let (end_col, end_row) = parse_cell_ref(end)?;
            Some(PreflightRange {
                start_col: start_col.min(end_col),
                start_row: start_row.min(end_row),
                end_col: start_col.max(end_col),
                end_row: start_row.max(end_row),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!ranges.is_empty()).then_some(ranges)
}

const SPREADSHEETML_NS: &[u8] = b"http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const X14_NS: &[u8] = b"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";

fn is_namespace(namespace: &ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(namespace, ResolveResult::Bound(value) if value.as_ref() == expected)
}

fn is_main_namespace(namespace: &ResolveResult<'_>) -> bool {
    matches!(namespace, ResolveResult::Unbound) || is_namespace(namespace, SPREADSHEETML_NS)
}

fn true_value(value: Option<String>) -> bool {
    value.is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"))
}

#[derive(Default)]
struct StandardDataBarExtensionBase {
    id: Option<String>,
    min_length: Option<String>,
    max_length: Option<String>,
    show_value: Option<String>,
    color: Option<String>,
}

#[derive(Default)]
struct ExtendedDataBar {
    id: Option<String>,
    kind: Option<String>,
    priority: Option<String>,
    min_length: Option<String>,
    max_length: Option<String>,
    show_value: Option<String>,
    gradient: Option<String>,
    axis_position: Option<String>,
    border: Option<String>,
    direction: Option<String>,
    negative_fill_color: Option<String>,
    unsupported_child: bool,
}

fn validate_x14_conditional_formatting(xml: &str, sheet_name: &str) -> Result<(), ConvertError> {
    let mut reader = NsReader::from_str(xml);
    let mut standard_rule: Option<StandardDataBarExtensionBase> = None;
    let mut standard_bases: HashMap<String, StandardDataBarExtensionBase> = HashMap::new();
    let mut extended_rule: Option<ExtendedDataBar> = None;
    let mut extended_rules = Vec::new();
    let mut in_standard_data_bar = false;
    let mut in_extended_data_bar = false;
    let mut right_to_left = false;

    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            crate::parser::parse_err(format!(
                "Failed to parse worksheet {sheet_name} extensions: {error}"
            ))
        })?;
        let is_main = is_main_namespace(&namespace);
        let is_x14 = is_namespace(&namespace, X14_NS);

        match event {
            Event::Start(ref element) if is_main && element.local_name().as_ref() == b"cfRule" => {
                if attr_value(&reader, element, b"type").as_deref() == Some("dataBar") {
                    standard_rule = Some(StandardDataBarExtensionBase::default());
                }
            }
            Event::Start(ref element) if is_x14 && element.local_name().as_ref() == b"cfRule" => {
                extended_rule = Some(ExtendedDataBar {
                    id: attr_value(&reader, element, b"id"),
                    kind: attr_value(&reader, element, b"type"),
                    priority: attr_value(&reader, element, b"priority"),
                    ..ExtendedDataBar::default()
                });
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_main && element.local_name().as_ref() == b"sheetView" =>
            {
                right_to_left |= true_value(attr_value(&reader, element, b"rightToLeft"));
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_main
                    && element.local_name().as_ref() == b"dataBar"
                    && standard_rule.is_some() =>
            {
                in_standard_data_bar = matches!(event, Event::Start(_));
                if let Some(rule) = standard_rule.as_mut() {
                    rule.min_length = attr_value(&reader, element, b"minLength");
                    rule.max_length = attr_value(&reader, element, b"maxLength");
                    rule.show_value = attr_value(&reader, element, b"showValue");
                }
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_x14
                    && element.local_name().as_ref() == b"dataBar"
                    && extended_rule.is_some() =>
            {
                in_extended_data_bar = matches!(event, Event::Start(_));
                if let Some(rule) = extended_rule.as_mut() {
                    rule.min_length = attr_value(&reader, element, b"minLength");
                    rule.max_length = attr_value(&reader, element, b"maxLength");
                    rule.show_value = attr_value(&reader, element, b"showValue");
                    rule.gradient = attr_value(&reader, element, b"gradient");
                    rule.axis_position = attr_value(&reader, element, b"axisPosition");
                    rule.border = attr_value(&reader, element, b"border");
                    rule.direction = attr_value(&reader, element, b"direction");
                }
            }
            Event::Start(ref element)
                if is_x14 && element.local_name().as_ref() == b"id" && standard_rule.is_some() =>
            {
                let name = element.name().to_owned();
                let id = reader
                    .read_text(quick_xml::name::QName(name.as_ref()))
                    .map_err(|error| {
                        crate::parser::parse_err(format!(
                            "Failed to read conditional-format extension id on sheet {sheet_name}: {error}"
                        ))
                    })?;
                if let Some(rule) = standard_rule.as_mut() {
                    rule.id = Some(id.into_owned());
                }
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_main && element.local_name().as_ref() == b"color" && in_standard_data_bar =>
            {
                if let Some(rule) = standard_rule.as_mut() {
                    rule.color = attr_value(&reader, element, b"rgb");
                }
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_x14
                    && element.local_name().as_ref() == b"negativeFillColor"
                    && in_extended_data_bar =>
            {
                if let Some(rule) = extended_rule.as_mut() {
                    rule.negative_fill_color = attr_value(&reader, element, b"rgb");
                }
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_x14 && in_extended_data_bar =>
            {
                if !matches!(
                    element.local_name().as_ref(),
                    b"cfvo" | b"axisColor" | b"negativeFillColor"
                ) && let Some(rule) = extended_rule.as_mut()
                {
                    rule.unsupported_child = true;
                }
            }
            Event::End(ref element) if is_main && element.local_name().as_ref() == b"dataBar" => {
                in_standard_data_bar = false;
            }
            Event::End(ref element) if is_x14 && element.local_name().as_ref() == b"dataBar" => {
                in_extended_data_bar = false;
            }
            Event::End(ref element) if is_main && element.local_name().as_ref() == b"cfRule" => {
                if let Some(rule) = standard_rule.take()
                    && let Some(id) = rule.id.clone()
                {
                    standard_bases.insert(id, rule);
                }
            }
            Event::End(ref element) if is_x14 && element.local_name().as_ref() == b"cfRule" => {
                if let Some(rule) = extended_rule.take() {
                    extended_rules.push(rule);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    for extension in extended_rules {
        let Some(id) = extension.id.as_deref() else {
            return Err(conditional_format_detail(sheet_name));
        };
        let Some(base) = standard_bases.get(id) else {
            return Err(conditional_format_detail(sheet_name));
        };
        let visible_values = extension
            .show_value
            .as_deref()
            .is_none_or(|value| matches!(value, "1" | "true" | "on"));
        let gradient = extension
            .gradient
            .as_deref()
            .is_none_or(|value| matches!(value, "1" | "true" | "on"));
        let no_border = extension
            .border
            .as_deref()
            .is_none_or(|value| matches!(value, "0" | "false" | "off"));
        let left_to_right = extension
            .direction
            .as_deref()
            .is_none_or(|value| matches!(value, "context" | "leftToRight"));
        let base_visible_values = base
            .show_value
            .as_deref()
            .is_none_or(|value| matches!(value, "1" | "true" | "on"));

        if extension.kind.as_deref() != Some("dataBar")
            || extension.priority.is_some()
            || extension.unsupported_child
            || extension.axis_position.as_deref() != Some("none")
            || !visible_values
            || !base_visible_values
            || !gradient
            || !no_border
            || !left_to_right
            || right_to_left
            || extension.min_length.as_deref().unwrap_or("10")
                != base.min_length.as_deref().unwrap_or("10")
            || extension.max_length.as_deref().unwrap_or("90")
                != base.max_length.as_deref().unwrap_or("90")
            || extension.negative_fill_color.as_deref() != base.color.as_deref()
        {
            return Err(conditional_format_detail(sheet_name));
        }
    }

    Ok(())
}

fn read_xml(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    path: &str,
) -> Result<Option<String>, ConvertError> {
    let actual_path = if archive.by_name(path).is_ok() {
        path.to_string()
    } else {
        (0..archive.len())
            .find_map(|index| {
                let entry = archive.by_index(index).ok()?;
                entry
                    .name()
                    .eq_ignore_ascii_case(path)
                    .then(|| entry.name().to_string())
            })
            .unwrap_or_default()
    };
    if actual_path.is_empty() {
        return Ok(None);
    }
    let mut entry = archive.by_name(&actual_path).map_err(|error| {
        crate::parser::parse_err(format!("Failed to open XLSX part {actual_path}: {error}"))
    })?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|error| {
        crate::parser::parse_err(format!("Failed to read XLSX part {path}: {error}"))
    })?;
    let decoded = decoded_xml_bytes(&bytes)?;
    String::from_utf8(decoded.into_owned())
        .map(Some)
        .map_err(|error| {
            crate::parser::parse_err(format!("Failed to read XLSX part {path}: {error}"))
        })
}

fn relationships(xml: &str) -> Result<HashMap<String, Relationship>, ConvertError> {
    let mut result = HashMap::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"Relationship" =>
            {
                let (Some(id), Some(target)) = (
                    attr_value(&reader, &element, b"Id"),
                    attr_value(&reader, &element, b"Target"),
                ) else {
                    continue;
                };
                let external = attr_value(&reader, &element, b"TargetMode")
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("external"));
                let kind = attr_value(&reader, &element, b"Type").unwrap_or_default();
                result.insert(
                    id,
                    Relationship {
                        target,
                        external,
                        kind,
                    },
                );
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse XLSX relationships: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(result)
}

fn validate_upstream_comments(xml: &str) -> Result<(), ConvertError> {
    let mut reader = Reader::from_str(xml);
    let mut author_count = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(element)) if element.name().as_ref() == b"author" => {
                author_count += 1;
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"author" => {
                author_count += 1;
            }
            Ok(Event::Start(element)) if element.name().as_ref() == b"comment" => {
                let author_id = attr_value(&reader, &element, b"authorId")
                    .and_then(|value| value.parse::<usize>().ok());
                if attr_value(&reader, &element, b"ref").is_none()
                    || author_id.is_none_or(|author_id| author_id >= author_count)
                {
                    return Err(unsupported("invalid comment author"));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to validate XLSX comments: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_upstream_chart(xml: &str) -> Result<(), ConvertError> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element))
                if element.local_name().as_ref() == b"symbol"
                    && attr_value(&reader, element, b"val").is_none() =>
            {
                return Err(unsupported("chart marker symbol missing value"));
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to validate XLSX chart input: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_upstream_chartsheet(xml: &str) -> Result<(), ConvertError> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element))
                if element.local_name().as_ref() == b"pageSetup"
                    && attr_value(&reader, element, b"id").is_some() =>
            {
                return Err(unsupported("chartsheet printer settings"));
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to validate XLSX chartsheet input: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn worksheet_printer_settings_rids(xml: &str) -> Result<Vec<String>, ConvertError> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element))
                if element.local_name().as_ref() == b"pageSetup" =>
            {
                if let Some(rid) = element
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"r:id")
                    .and_then(|attribute| {
                        attribute
                            .decode_and_unescape_value(reader.decoder())
                            .ok()
                            .map(|value| value.into_owned())
                    })
                {
                    result.push(rid);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to validate XLSX worksheet printer settings: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(result)
}

fn validate_upstream_vml(xml: &str) -> Result<(), ConvertError> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element))
                if element.local_name().as_ref() == b"fill"
                    && attr_value(&reader, element, b"relid").is_some()
                    && attr_value(&reader, element, b"title").is_none() =>
            {
                return Err(unsupported("VML image fill missing title"));
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to validate XLSX VML input: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn vml_has_visible_note(xml: &str) -> Result<bool, ConvertError> {
    let mut reader = Reader::from_str(xml);
    let mut in_note = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element)) if element.local_name().as_ref() == b"ClientData" => {
                in_note = attr_value(&reader, element, b"ObjectType").as_deref() == Some("Note");
            }
            Ok(Event::Empty(ref element))
                if in_note && element.local_name().as_ref() == b"Visible" =>
            {
                return Ok(true);
            }
            Ok(Event::Start(ref element))
                if in_note && element.local_name().as_ref() == b"Visible" =>
            {
                let name = element.name().to_owned();
                let value = reader
                    .read_text(quick_xml::name::QName(name.as_ref()))
                    .map_err(|error| {
                        crate::parser::parse_err(format!(
                            "Failed to inspect XLSX VML note visibility: {error}"
                        ))
                    })?;
                let value = value.trim();
                if !value.eq_ignore_ascii_case("false") && !value.eq_ignore_ascii_case("f") {
                    return Ok(true);
                }
            }
            Ok(Event::End(ref element)) if element.local_name().as_ref() == b"ClientData" => {
                in_note = false;
            }
            Ok(Event::Eof) => return Ok(false),
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to inspect XLSX VML note visibility: {error}"
                )));
            }
            _ => {}
        }
    }
}

fn validate_upstream_parser_inputs(data: &[u8]) -> Result<(), ConvertError> {
    let mut archive = crate::parser::open_zip(data)?;
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .collect();

    for name in names {
        let lower = name.trim_start_matches('/').to_ascii_lowercase();
        let relevant = (lower.starts_with("xl/drawings/_rels/") && lower.ends_with(".rels"))
            || (lower.starts_with("xl/charts/") && lower.ends_with(".xml"))
            || (lower.starts_with("xl/chartsheets/") && lower.ends_with(".xml"))
            || (lower.starts_with("xl/comments") && lower.ends_with(".xml"))
            || (lower.starts_with("xl/drawings/") && lower.ends_with(".vml"))
            || (lower.starts_with("xl/worksheets/") && lower.ends_with(".xml"));
        if !relevant {
            continue;
        }
        let Some(xml) = read_xml(&mut archive, &name)? else {
            continue;
        };
        if lower.starts_with("xl/drawings/_rels/") && lower.ends_with(".rels") {
            if relationships(&xml)?
                .values()
                .any(|relationship| relationship.external && relationship.kind.ends_with("/image"))
            {
                return Err(unsupported("external drawing image"));
            }
        } else if lower.starts_with("xl/charts/") && lower.ends_with(".xml") {
            validate_upstream_chart(&xml)?;
        } else if lower.starts_with("xl/chartsheets/") && lower.ends_with(".xml") {
            validate_upstream_chartsheet(&xml)?;
        } else if lower.starts_with("xl/worksheets/") && lower.ends_with(".xml") {
            let printer_settings_rids = worksheet_printer_settings_rids(&xml)?;
            if !printer_settings_rids.is_empty() {
                let rels_path = part_rels_path(name.trim_start_matches('/'));
                let rels = read_xml(&mut archive, &rels_path)?
                    .map(|xml| relationships(&xml))
                    .transpose()?
                    .unwrap_or_default();
                if printer_settings_rids
                    .iter()
                    .any(|rid| !rels.contains_key(rid))
                {
                    return Err(unsupported("unresolved worksheet printer settings"));
                }
            }
        } else if lower.starts_with("xl/comments") && lower.ends_with(".xml") {
            validate_upstream_comments(&xml)?;
        } else if lower.starts_with("xl/drawings/") && lower.ends_with(".vml") {
            validate_upstream_vml(&xml)?;
        }
    }
    Ok(())
}

fn validate_worksheet(
    xml: &str,
    sheet_name: &str,
    defined_names: &HashMap<String, String>,
) -> Result<WorksheetScan, ConvertError> {
    validate_x14_conditional_formatting(xml, sheet_name)?;

    const SUPPORTED_CONDITIONAL_FORMATS: &[&str] = &[
        "beginsWith",
        "cellIs",
        "colorScale",
        "containsText",
        "dataBar",
        "endsWith",
        "expression",
        "iconSet",
        "notContainsText",
    ];

    let mut scan = WorksheetScan::default();
    let mut reader = NsReader::from_str(xml);
    let mut control_prints: Option<bool> = None;
    let mut current_rule_kind: Option<String> = None;
    let mut current_rule_formula_count = 0usize;
    let mut current_rule_cfvo_count = 0usize;
    let mut current_rule_color_count = 0usize;
    let mut current_rule_colors_valid = true;
    let mut current_icon_set_type: Option<String> = None;
    let mut current_rule_container_seen = false;
    let mut seen_priorities = HashSet::new();
    let mut conditional_format_groups: Vec<Vec<PreflightRange>> = Vec::new();
    let mut in_cell = false;
    let mut cell_has_formula = false;
    let mut cell_has_cached_value = false;
    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|error| {
            crate::parser::parse_err(format!("Failed to parse worksheet {sheet_name}: {error}"))
        })?;
        let is_main = is_main_namespace(&namespace);
        match event {
            Event::Start(ref element) if is_main && element.local_name().as_ref() == b"c" => {
                in_cell = true;
                cell_has_formula = false;
                cell_has_cached_value = false;
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_main && in_cell && element.local_name().as_ref() == b"f" =>
            {
                cell_has_formula = true;
            }
            Event::Start(ref element) | Event::Empty(ref element)
                if is_main && in_cell && element.local_name().as_ref() == b"v" =>
            {
                cell_has_cached_value = true;
            }
            Event::Start(ref element)
                if is_main && element.local_name().as_ref() == b"conditionalFormatting" =>
            {
                let ranges = attr_value(&reader, element, b"sqref")
                    .and_then(|value| preflight_sqref(&value))
                    .ok_or_else(|| conditional_format_detail(sheet_name))?;
                if conditional_format_groups.iter().any(|existing| {
                    existing
                        .iter()
                        .any(|left| ranges.iter().any(|right| left.overlaps(*right)))
                }) {
                    return Err(conditional_format_detail(sheet_name));
                }
                conditional_format_groups.push(ranges);
            }
            Event::Start(ref element) if is_main && element.local_name().as_ref() == b"cfRule" => {
                let kind = attr_value(&reader, element, b"type")
                    .unwrap_or_else(|| "missing type".to_string());
                if !SUPPORTED_CONDITIONAL_FORMATS.contains(&kind.as_str()) {
                    return Err(unsupported(format!(
                        "conditional formatting type {kind} on printed sheet: {sheet_name}"
                    )));
                }
                let priority = attr_value(&reader, element, b"priority")
                    .and_then(|value| value.parse::<i32>().ok());
                if priority.is_none_or(|priority| priority <= 0)
                    || !seen_priorities.insert(priority.unwrap_or_default())
                    || true_value(attr_value(&reader, element, b"stopIfTrue"))
                {
                    return Err(conditional_format_detail(sheet_name));
                }
                if kind == "cellIs"
                    && !matches!(
                        attr_value(&reader, element, b"operator").as_deref(),
                        Some(
                            "equal"
                                | "notEqual"
                                | "greaterThan"
                                | "greaterThanOrEqual"
                                | "lessThan"
                                | "lessThanOrEqual"
                        )
                    )
                {
                    return Err(conditional_format_detail(sheet_name));
                }
                if let Some(raw_dxf_id) = attr_value(&reader, element, b"dxfId") {
                    let dxf_id = raw_dxf_id
                        .parse::<usize>()
                        .map_err(|_| conditional_format_detail(sheet_name))?;
                    scan.dxf_ids.push(dxf_id);
                }
                if matches!(
                    kind.as_str(),
                    "beginsWith" | "containsText" | "endsWith" | "notContainsText"
                ) && attr_value(&reader, element, b"text").is_none_or(|text| text.is_empty())
                {
                    return Err(conditional_format_detail(sheet_name));
                }
                current_rule_formula_count = 0;
                current_rule_cfvo_count = 0;
                current_rule_color_count = 0;
                current_rule_colors_valid = true;
                current_icon_set_type = None;
                current_rule_container_seen = false;
                current_rule_kind = Some(kind);
            }
            Event::Empty(ref element) if is_main && element.local_name().as_ref() == b"cfRule" => {
                let kind = attr_value(&reader, element, b"type")
                    .unwrap_or_else(|| "missing type".to_string());
                if !SUPPORTED_CONDITIONAL_FORMATS.contains(&kind.as_str()) {
                    return Err(unsupported(format!(
                        "conditional formatting type {kind} on printed sheet: {sheet_name}"
                    )));
                }
                let priority = attr_value(&reader, element, b"priority")
                    .and_then(|value| value.parse::<i32>().ok());
                if priority.is_none_or(|priority| priority <= 0)
                    || !seen_priorities.insert(priority.unwrap_or_default())
                    || true_value(attr_value(&reader, element, b"stopIfTrue"))
                {
                    return Err(conditional_format_detail(sheet_name));
                }
                if let Some(raw_dxf_id) = attr_value(&reader, element, b"dxfId") {
                    let dxf_id = raw_dxf_id
                        .parse::<usize>()
                        .map_err(|_| conditional_format_detail(sheet_name))?;
                    scan.dxf_ids.push(dxf_id);
                }
                if !matches!(
                    kind.as_str(),
                    "beginsWith" | "containsText" | "endsWith" | "notContainsText"
                ) || attr_value(&reader, element, b"text").is_none_or(|text| text.is_empty())
                {
                    return Err(conditional_format_detail(sheet_name));
                }
            }
            Event::Start(ref element)
                if is_main
                    && element.local_name().as_ref() == b"formula"
                    && matches!(current_rule_kind.as_deref(), Some("expression" | "cellIs")) =>
            {
                let name = element.name().to_owned();
                let raw = reader
                    .read_text(quick_xml::name::QName(name.as_ref()))
                    .map_err(|error| {
                        crate::parser::parse_err(format!(
                            "Failed to read conditional-format expression on sheet {sheet_name}: {error}"
                        ))
                    })?;
                let formula = quick_xml::escape::unescape(&raw).map_err(|error| {
                    crate::parser::parse_err(format!(
                        "Failed to decode conditional-format expression on sheet {sheet_name}: {error}"
                    ))
                })?;
                match current_rule_kind.as_deref() {
                    Some("expression") => {
                        if !crate::parser::xlsx_formula::supports_expression_on_sheet(
                            &formula,
                            defined_names,
                            sheet_name,
                        ) {
                            return Err(unsupported(format!(
                                "unsupported conditional-format expression on printed sheet: {sheet_name}"
                            )));
                        }
                    }
                    Some("cellIs") if !cell_is_operand_supported(&formula) => {
                        return Err(conditional_format_detail(sheet_name));
                    }
                    _ => {}
                }
                current_rule_formula_count += 1;
            }
            Event::Empty(ref element) if is_main && element.local_name().as_ref() == b"control" => {
                return Err(unsupported(format!(
                    "printable form control on sheet: {sheet_name}"
                )));
            }
            Event::Start(ref element) | Event::Empty(ref element) => {
                match element.local_name().as_ref() {
                    b"sparklineGroup" => {
                        return Err(unsupported(format!(
                            "sparklines on printed sheet: {sheet_name}"
                        )));
                    }
                    b"oleObject" if is_main => {
                        return Err(unsupported(format!(
                            "embedded OLE object on printed sheet: {sheet_name}"
                        )));
                    }
                    b"legacyDrawingHF" if is_main => {
                        return Err(unsupported(format!(
                            "header or footer image on printed sheet: {sheet_name}"
                        )));
                    }
                    b"pageSetup" if is_main => {
                        if attr_value(&reader, element, b"cellComments")
                            .is_some_and(|value| value != "none")
                        {
                            return Err(unsupported(format!(
                                "printed cell comments on sheet: {sheet_name}"
                            )));
                        }
                    }
                    b"drawing" if is_main => {
                        if let Some(rid) = attr_value(&reader, element, b"id") {
                            scan.drawing_rids.push(rid);
                        }
                    }
                    b"legacyDrawing" if is_main => {
                        if let Some(rid) = attr_value(&reader, element, b"id") {
                            scan.legacy_drawing_rids.push(rid);
                        }
                    }
                    b"control" if is_main => control_prints = Some(true),
                    b"controlPr" if is_main && control_prints.is_some() => {
                        if false_value(attr_value(&reader, element, b"print")) {
                            control_prints = Some(false);
                        }
                    }
                    b"dataBar" if is_main && current_rule_kind.as_deref() == Some("dataBar") => {
                        current_rule_container_seen = true;
                        if false_value(attr_value(&reader, element, b"showValue")) {
                            return Err(conditional_format_detail(sheet_name));
                        }
                        let min_length = match attr_value(&reader, element, b"minLength") {
                            Some(value) => value
                                .parse::<u32>()
                                .ok()
                                .filter(|value| *value <= 100)
                                .ok_or_else(|| conditional_format_detail(sheet_name))?,
                            None => 10,
                        };
                        let max_length = match attr_value(&reader, element, b"maxLength") {
                            Some(value) => value
                                .parse::<u32>()
                                .ok()
                                .filter(|value| *value <= 100)
                                .ok_or_else(|| conditional_format_detail(sheet_name))?,
                            None => 90,
                        };
                        if min_length > max_length || max_length > 100 {
                            return Err(conditional_format_detail(sheet_name));
                        }
                    }
                    b"colorScale"
                        if is_main && current_rule_kind.as_deref() == Some("colorScale") =>
                    {
                        current_rule_container_seen = true;
                    }
                    b"iconSet" if is_main && current_rule_kind.as_deref() == Some("iconSet") => {
                        current_rule_container_seen = true;
                        const SUPPORTED_ICON_SETS: &[&str] = &[
                            "3Arrows",
                            "3ArrowsGray",
                            "3Flags",
                            "3Signs",
                            "3Symbols",
                            "3Symbols2",
                            "3TrafficLights1",
                            "3TrafficLights2",
                            "4Arrows",
                            "4ArrowsGray",
                            "4TrafficLights",
                            "5Arrows",
                            "5ArrowsGray",
                        ];
                        if attr_value(&reader, element, b"iconSet")
                            .is_some_and(|kind| !SUPPORTED_ICON_SETS.contains(&kind.as_str()))
                            || false_value(attr_value(&reader, element, b"showValue"))
                            || attr_value(&reader, element, b"reverse")
                                .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"))
                        {
                            return Err(conditional_format_detail(sheet_name));
                        }
                        current_icon_set_type = attr_value(&reader, element, b"iconSet");
                    }
                    b"cfvo" if is_main => {
                        if matches!(
                            current_rule_kind.as_deref(),
                            Some("colorScale" | "dataBar" | "iconSet")
                        ) {
                            current_rule_cfvo_count += 1;
                        }
                        let kind = attr_value(&reader, element, b"type").unwrap_or_default();
                        let supported = match current_rule_kind.as_deref() {
                            Some("colorScale" | "iconSet") => {
                                matches!(
                                    kind.as_str(),
                                    "min" | "max" | "num" | "percent" | "percentile"
                                )
                            }
                            Some("dataBar") => {
                                matches!(kind.as_str(), "min" | "max" | "num" | "percent")
                            }
                            _ => true,
                        };
                        let value_is_valid = match kind.as_str() {
                            "num" | "percent" | "percentile" => {
                                attr_value(&reader, element, b"val")
                                    .and_then(|value| value.parse::<f64>().ok())
                                    .is_some_and(|value| {
                                        value.is_finite()
                                            && (kind == "num" || (0.0..=100.0).contains(&value))
                                    })
                            }
                            _ => true,
                        };
                        if !supported || !value_is_valid {
                            return Err(conditional_format_detail(sheet_name));
                        }
                        if current_rule_kind.as_deref() == Some("iconSet")
                            && attr_value(&reader, element, b"gte").is_some_and(|value| {
                                matches!(value.as_str(), "0" | "false" | "off")
                            })
                        {
                            return Err(conditional_format_detail(sheet_name));
                        }
                    }
                    b"color"
                        if is_main
                            && matches!(
                                current_rule_kind.as_deref(),
                                Some("colorScale" | "dataBar")
                            ) =>
                    {
                        current_rule_color_count += 1;
                        current_rule_colors_valid &=
                            conditional_format_color_supported(&reader, element);
                    }
                    _ => {}
                }
            }
            Event::End(element) if is_main && element.local_name().as_ref() == b"control" => {
                if control_prints.take().unwrap_or(true) {
                    return Err(unsupported(format!(
                        "printable form control on sheet: {sheet_name}"
                    )));
                }
            }
            Event::End(element) if is_main && element.local_name().as_ref() == b"c" => {
                if in_cell && cell_has_formula && !cell_has_cached_value {
                    return Err(unsupported(format!(
                        "formula without cached value on printed sheet: {sheet_name}"
                    )));
                }
                in_cell = false;
            }
            Event::End(element) if is_main && element.local_name().as_ref() == b"cfRule" => {
                match current_rule_kind.take().as_deref() {
                    Some("expression") if current_rule_formula_count != 1 => {
                        return Err(unsupported(format!(
                            "unsupported conditional-format expression on printed sheet: {sheet_name}"
                        )));
                    }
                    Some("cellIs") if current_rule_formula_count != 1 => {
                        return Err(conditional_format_detail(sheet_name));
                    }
                    Some("colorScale")
                        if !current_rule_container_seen
                            || !matches!(current_rule_cfvo_count, 2 | 3)
                            || current_rule_color_count != current_rule_cfvo_count
                            || !current_rule_colors_valid =>
                    {
                        return Err(conditional_format_detail(sheet_name));
                    }
                    Some("dataBar")
                        if !current_rule_container_seen
                            || current_rule_cfvo_count != 2
                            || current_rule_color_count != 1
                            || !current_rule_colors_valid =>
                    {
                        return Err(conditional_format_detail(sheet_name));
                    }
                    Some("iconSet") => {
                        let expected = current_icon_set_type
                            .as_deref()
                            .and_then(|kind| kind.as_bytes().first().copied())
                            .and_then(|digit| digit.checked_sub(b'0'))
                            .map(usize::from)
                            .unwrap_or(3);
                        if !current_rule_container_seen || current_rule_cfvo_count != expected {
                            return Err(conditional_format_detail(sheet_name));
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(scan)
}

fn spreadsheet_color_supported(reader: &Reader<&[u8]>, element: &BytesStart<'_>) -> bool {
    let mut base_count = 0usize;
    let mut valid = true;
    for attribute in element.attributes().flatten() {
        let name = attribute.key.local_name();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .ok()
            .map(|value| value.into_owned());
        match name.as_ref() {
            b"rgb" => {
                base_count += 1;
                valid &= value.is_some_and(|value| {
                    matches!(value.len(), 6 | 8)
                        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
            }
            b"indexed" | b"theme" => {
                base_count += 1;
                valid &= value.and_then(|value| value.parse::<u32>().ok()).is_some();
            }
            b"auto" => {
                base_count += 1;
                valid &= value.is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"));
            }
            b"tint" => {
                valid &= value
                    .and_then(|value| value.parse::<f64>().ok())
                    .is_some_and(|value| value.is_finite() && (-1.0..=1.0).contains(&value));
            }
            _ => valid = false,
        }
    }
    valid && base_count == 1
}

fn validate_differential_style_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
) -> bool {
    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    match (parent, name.as_ref()) {
        (Some(b"dxf"), b"font" | b"fill") | (Some(b"fill"), b"patternFill") => {
            if name.as_ref() == b"patternFill" {
                attr_value(reader, element, b"patternType")
                    .is_none_or(|value| matches!(value.as_str(), "none" | "solid"))
            } else {
                true
            }
        }
        (Some(b"font"), b"b" | b"i" | b"strike") => attr_value(reader, element, b"val")
            .is_none_or(|value| matches!(value.as_str(), "1" | "true" | "on")),
        (Some(b"font"), b"name") => {
            attr_value(reader, element, b"val").is_some_and(|value| !value.trim().is_empty())
        }
        (Some(b"font"), b"family") => attr_value(reader, element, b"val")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| value <= 5),
        (Some(b"font"), b"scheme") => {
            attr_value(reader, element, b"val").as_deref() == Some("none")
        }
        (Some(b"font"), b"sz") => attr_value(reader, element, b"val")
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|value| value.is_finite() && (1.0..=409.0).contains(&value)),
        (Some(b"font"), b"color") | (Some(b"patternFill"), b"fgColor" | b"bgColor") => {
            spreadsheet_color_supported(reader, element)
        }
        (Some(b"font"), b"u") => {
            attr_value(reader, element, b"val").is_none_or(|value| value == "single")
        }
        _ => false,
    }
}

fn validate_differential_styles(xml: &str, used_ids: &HashSet<usize>) -> Result<(), ConvertError> {
    if used_ids.is_empty() {
        return Ok(());
    }
    let mut reader = Reader::from_str(xml);
    let mut ancestors: Vec<Vec<u8>> = Vec::new();
    let mut next_dxf_id = 0usize;
    let mut selected = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.local_name();
                if name.as_ref() == b"dxf" {
                    selected = used_ids.contains(&next_dxf_id);
                    next_dxf_id += 1;
                } else if selected
                    && !validate_differential_style_element(&reader, &element, &ancestors)
                {
                    return Err(unsupported(
                        "unsupported differential conditional-format style",
                    ));
                }
                ancestors.push(name.as_ref().to_vec());
            }
            Ok(Event::Empty(element)) => {
                let name = element.local_name();
                if name.as_ref() == b"dxf" {
                    next_dxf_id += 1;
                } else if selected
                    && !validate_differential_style_element(&reader, &element, &ancestors)
                {
                    return Err(unsupported(
                        "unsupported differential conditional-format style",
                    ));
                }
            }
            Ok(Event::End(element)) => {
                if element.local_name().as_ref() == b"dxf" {
                    selected = false;
                }
                ancestors.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse XLSX differential styles: {error}"
                )));
            }
            _ => {}
        }
    }
    if used_ids.iter().any(|id| *id >= next_dxf_id) {
        return Err(unsupported("missing differential conditional-format style"));
    }
    Ok(())
}

fn conditional_format_color_supported(reader: &NsReader<&[u8]>, element: &BytesStart<'_>) -> bool {
    let rgb = attr_value(reader, element, b"rgb");
    let indexed = attr_value(reader, element, b"indexed");
    let has_supported_base = match (rgb.as_deref(), indexed.as_deref()) {
        (Some(value), None) => crate::parser::xml_util::parse_argb_color(value).is_some(),
        (None, Some(value)) => value.parse::<u8>().is_ok_and(|index| index <= 63),
        _ => false,
    };
    let tint_is_neutral = attr_value(reader, element, b"tint").is_none_or(|value| {
        value
            .parse::<f64>()
            .is_ok_and(|tint| tint.is_finite() && tint == 0.0)
    });
    let automatic_is_off = !true_value(attr_value(reader, element, b"auto"));
    has_supported_base
        && tint_is_neutral
        && automatic_is_off
        && attr_value(reader, element, b"theme").is_none()
}

fn finish_drawing_anchor(
    anchor: DrawingAnchor,
    sheet_name: &str,
    chart_rids: &mut Vec<String>,
) -> Result<(), ConvertError> {
    if !anchor.prints {
        return Ok(());
    }
    if let Some(kind) = anchor.unsupported {
        return Err(unsupported(format!(
            "printable drawing object {kind} on sheet: {sheet_name}"
        )));
    }
    if anchor.saw_shape && !anchor.shape_has_paragraph {
        return Err(unsupported(format!(
            "printable standalone shape on sheet: {sheet_name}"
        )));
    }
    if anchor.graphic_frame
        && anchor.graphic_data_uri.as_deref()
            != Some("http://schemas.openxmlformats.org/drawingml/2006/chart")
    {
        return Err(unsupported(format!(
            "non-chart graphic frame on sheet: {sheet_name}"
        )));
    }
    chart_rids.extend(anchor.chart_rids);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;

        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::FileOptions::default())
                .expect("test ZIP entry starts");
            writer.write_all(bytes).expect("test ZIP entry writes");
        }
        writer.finish().expect("test ZIP closes").into_inner()
    }

    fn assert_unsupported(error: ConvertError, expected: &str) {
        assert!(
            matches!(
                error,
                ConvertError::UnsupportedElement { format: "XLSX", ref element }
                    if element == expected
            ),
            "expected {expected:?}, got {error:?}"
        );
    }

    #[test]
    fn package_bounds_refuse_oversized_ambiguous_and_overpopulated_archives() {
        let one = zip_entries(&[("xl/workbook.xml", b"12345")]);
        validate_package_bounds(
            &one,
            PackageBounds {
                compressed_bytes: one.len(),
                entries: 1,
                total_uncompressed_bytes: 5,
                xml_uncompressed_bytes: 5,
                other_entry_uncompressed_bytes: 5,
            },
        )
        .expect("the package is exactly on every boundary");

        for (data, bounds, detail) in [
            (
                one.clone(),
                PackageBounds {
                    compressed_bytes: one.len() - 1,
                    entries: 1,
                    total_uncompressed_bytes: 5,
                    xml_uncompressed_bytes: 5,
                    other_entry_uncompressed_bytes: 5,
                },
                "XLSX package exceeds browser safety limit",
            ),
            (
                one.clone(),
                PackageBounds {
                    compressed_bytes: one.len(),
                    entries: 1,
                    total_uncompressed_bytes: 4,
                    xml_uncompressed_bytes: 5,
                    other_entry_uncompressed_bytes: 5,
                },
                "XLSX package exceeds browser safety limit",
            ),
            (
                one.clone(),
                PackageBounds {
                    compressed_bytes: one.len(),
                    entries: 1,
                    total_uncompressed_bytes: 5,
                    xml_uncompressed_bytes: 4,
                    other_entry_uncompressed_bytes: 5,
                },
                "XLSX package exceeds browser safety limit",
            ),
            (
                zip_entries(&[("xl/A.xml", b"1"), ("xl/B.xml", b"2")]),
                PackageBounds {
                    compressed_bytes: usize::MAX,
                    entries: 1,
                    total_uncompressed_bytes: 2,
                    xml_uncompressed_bytes: 1,
                    other_entry_uncompressed_bytes: 1,
                },
                "XLSX package exceeds browser safety limit",
            ),
            (
                zip_entries(&[("xl/A.xml", b"1"), ("XL/a.XML", b"2")]),
                PackageBounds {
                    compressed_bytes: usize::MAX,
                    entries: 2,
                    total_uncompressed_bytes: 2,
                    xml_uncompressed_bytes: 1,
                    other_entry_uncompressed_bytes: 1,
                },
                "ambiguous XLSX package path",
            ),
        ] {
            assert_unsupported(
                validate_package_bounds(&data, bounds).expect_err("the unsafe archive must refuse"),
                detail,
            );
        }
    }

    #[test]
    fn upstream_parser_safety_refuses_known_panic_shapes() {
        for (part, xml, expected) in [
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.test/image.png" TargetMode="External"/></Relationships>"#,
                "external drawing image",
            ),
            (
                "xl/charts/chart1.xml",
                r#"<c:chartSpace xmlns:c="urn:c"><c:symbol/></c:chartSpace>"#,
                "chart marker symbol missing value",
            ),
            (
                "xl/chartsheets/sheet1.xml",
                r#"<chartsheet xmlns:r="urn:r"><pageSetup r:id="rId1"/></chartsheet>"#,
                "chartsheet printer settings",
            ),
            (
                "xl/comments1.xml",
                r#"<comments xmlns:d="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><d:author>Author</d:author></authors><commentList><comment ref="A1" authorId="0"><text/></comment></commentList></comments>"#,
                "invalid comment author",
            ),
            (
                "xl/drawings/vmlDrawing1.vml",
                r#"<xml xmlns:o="urn:o"><fill o:relid="rId1"/></xml>"#,
                "VML image fill missing title",
            ),
        ] {
            let package = zip_entries(&[(part, xml.as_bytes())]);
            let error = match ensure_safe_package_bounds(&package) {
                Ok(()) => panic!("{part} must refuse before the upstream parser"),
                Err(error) => error,
            };
            assert_unsupported(error, expected);
        }
    }

    #[test]
    fn upstream_parser_safety_refuses_undefined_xml_entities() {
        for (part, xml) in [
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>value &a5;</t></si></sst>"#,
            ),
            (
                "docProps/core.xml",
                r#"<cp:coreProperties xmlns:cp="urn:cp"><dc:title xmlns:dc="urn:dc">value &lol9;</dc:title></cp:coreProperties>"#,
            ),
        ] {
            let package = zip_entries(&[(part, xml.as_bytes())]);
            let error = ensure_safe_package_bounds(&package)
                .expect_err("undefined XML entities must refuse before the upstream parser");
            assert!(
                matches!(error, ConvertError::Parse(ref detail) if detail.contains("entity")),
                "expected an XML entity parse error for {part}, got {error:?}"
            );
        }
    }

    #[test]
    fn xml_validation_keeps_predefined_and_numeric_references() {
        let package = zip_entries(&[(
            "xl/sharedStrings.xml",
            br#"<sst><si><t>&amp;&apos;&gt;&lt;&quot;&#65;&#x42;</t></si></sst>"#,
        )]);

        ensure_safe_package_bounds(&package).expect("standard XML references must remain accepted");
    }

    #[test]
    fn upstream_parser_safety_keeps_supported_counterparts() {
        let package = zip_entries(&[
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.test/" TargetMode="External"/></Relationships>"#,
            ),
            (
                "xl/charts/chart1.xml",
                br#"<c:chartSpace xmlns:c="urn:c"><c:symbol val="circle"/></c:chartSpace>"#,
            ),
            (
                "xl/chartsheets/sheet1.xml",
                br#"<chartsheet><pageSetup orientation="landscape"/></chartsheet>"#,
            ),
            (
                "xl/comments1.xml",
                br#"<comments><authors><author>Author</author></authors><commentList><comment ref="A1" authorId="0"/></commentList></comments>"#,
            ),
            (
                "xl/drawings/vmlDrawing1.vml",
                br#"<xml xmlns:o="urn:o"><fill o:relid="rId1" o:title="image.png"/></xml>"#,
            ),
        ]);
        ensure_safe_package_bounds(&package).expect("supported parser inputs must remain accepted");
    }

    #[test]
    fn differential_style_preflight_admits_only_properties_the_renderer_applies() {
        let supported = r#"<styleSheet><dxfs count="1"><dxf><font><name val="Arial"/><sz val="14"/><b/><i/><u val="single"/><strike/><color rgb="FF9C0006"/></font><fill><patternFill patternType="solid"><bgColor rgb="FFFFC7CE"/></patternFill></fill></dxf></dxfs></styleSheet>"#;
        validate_differential_styles(supported, &HashSet::from([0]))
            .expect("supported font properties and solid fill are represented");

        for feature in [
            "<i val=\"0\"/>",
            "<u val=\"double\"/>",
            "<sz val=\"410\"/>",
            "<charset val=\"1\"/>",
            "<scheme val=\"major\"/>",
            "<outline val=\"0\"/>",
            "<border><bottom style=\"thin\"/></border>",
            "<numFmt numFmtId=\"165\" formatCode=\"$#,##0\"/>",
            "<alignment horizontal=\"center\"/>",
        ] {
            let xml =
                format!("<styleSheet><dxfs count=\"1\"><dxf>{feature}</dxf></dxfs></styleSheet>");
            assert_unsupported(
                validate_differential_styles(&xml, &HashSet::from([0]))
                    .expect_err("the renderer drops this differential style property"),
                "unsupported differential conditional-format style",
            );
        }

        assert_unsupported(
            validate_differential_styles(supported, &HashSet::from([1]))
                .expect_err("the rule names no differential style"),
            "missing differential conditional-format style",
        );
    }

    #[test]
    fn worksheet_accepts_nonprinting_control_and_rejects_printing_control() {
        let hidden = r#"<worksheet><controls><control r:id="rId1"><controlPr print="0"/></control></controls></worksheet>"#;
        validate_worksheet(hidden, "Budget", &HashMap::new())
            .expect("an explicitly nonprinting control is absent");

        let visible = r#"<worksheet><controls><control r:id="rId1"><controlPr print="1"/></control></controls></worksheet>"#;
        assert_unsupported(
            validate_worksheet(visible, "Budget", &HashMap::new())
                .expect_err("a printable control is not drawn"),
            "printable form control on sheet: Budget",
        );
    }

    #[test]
    fn worksheet_refuses_a_formula_without_a_cached_value() {
        let uncached = r#"<worksheet><sheetData><row r="1"><c r="A1"><f>1+1</f></c></row></sheetData></worksheet>"#;
        assert_unsupported(
            validate_worksheet(uncached, "Budget", &HashMap::new())
                .expect_err("the renderer cannot calculate a missing formula result"),
            "formula without cached value on printed sheet: Budget",
        );

        let cached_empty = r#"<worksheet><sheetData><row r="1"><c r="A1"><f>IF(TRUE,&quot;&quot;,&quot;x&quot;)</f><v></v></c></row></sheetData></worksheet>"#;
        validate_worksheet(cached_empty, "Budget", &HashMap::new())
            .expect("an explicitly cached empty formula result is known");
    }

    #[test]
    fn worksheet_rejects_each_unimplemented_print_feature() {
        for (element, expected) in [
            (
                r#"<x14:sparklineGroup xmlns:x14="urn:x14"/>"#,
                "sparklines on printed sheet: Budget",
            ),
            (
                r#"<cfRule type="top10"/>"#,
                "conditional formatting type top10 on printed sheet: Budget",
            ),
            (
                r#"<pageSetup cellComments="atEnd"/>"#,
                "printed cell comments on sheet: Budget",
            ),
            (
                r#"<legacyDrawingHF r:id="rId1"/>"#,
                "header or footer image on printed sheet: Budget",
            ),
            (
                r#"<oleObject r:id="rId1"/>"#,
                "embedded OLE object on printed sheet: Budget",
            ),
        ] {
            let xml = format!(r#"<worksheet xmlns:r="urn:r">{element}</worksheet>"#);
            assert_unsupported(
                validate_worksheet(&xml, "Budget", &HashMap::new())
                    .expect_err("feature must refuse"),
                expected,
            );
        }
    }

    #[test]
    fn worksheet_rejects_an_expression_the_evaluator_cannot_execute() {
        let xml = r#"<worksheet><conditionalFormatting sqref="A1"><cfRule type="expression" priority="1"><formula>COUNTIF(A1,1)&gt;0</formula></cfRule></conditionalFormatting></worksheet>"#;
        assert_unsupported(
            validate_worksheet(xml, "Budget", &HashMap::new())
                .expect_err("an unknown conditional-format function must not disappear"),
            "unsupported conditional-format expression on printed sheet: Budget",
        );
    }

    #[test]
    fn worksheet_rejects_conditional_format_details_the_renderer_would_guess() {
        for rule in [
            r#"<cfRule type="cellIs" operator="between" priority="1"><formula>1</formula><formula>2</formula></cfRule>"#,
            r#"<cfRule type="cellIs" operator="equal" priority="1"><formula>A1</formula></cfRule>"#,
            r#"<cfRule type="containsText" priority="1"/>"#,
            r#"<cfRule type="colorScale" priority="1"><colorScale><cfvo type="formula" val="A1"/></colorScale></cfRule>"#,
            r#"<cfRule type="colorScale" priority="1"><colorScale><cfvo type="min"/><cfvo type="max"/><color rgb="FFFFFFFF"/></colorScale></cfRule>"#,
            r#"<cfRule type="dataBar" priority="1"><dataBar showValue="0"><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar></cfRule>"#,
            r#"<cfRule type="dataBar" priority="1"><dataBar minLength="bad"><cfvo type="min"/><cfvo type="max"/><color rgb="FF638EC6"/></dataBar></cfRule>"#,
            r#"<cfRule type="dataBar" priority="1"><dataBar><cfvo type="min"/><cfvo type="max"/><color theme="4"/></dataBar></cfRule>"#,
            r#"<cfRule type="iconSet" priority="1"><iconSet iconSet="3Stars"><cfvo type="percent" val="0"/></iconSet></cfRule>"#,
            r#"<cfRule type="iconSet" priority="1"><iconSet><cfvo type="percent" val="0" gte="0"/></iconSet></cfRule>"#,
            r#"<cfRule type="expression" priority="1" stopIfTrue="1"><formula>TRUE</formula></cfRule>"#,
            r#"<cfRule type="expression"><formula>TRUE</formula></cfRule>"#,
        ] {
            let xml = format!(
                r#"<worksheet><conditionalFormatting>{rule}</conditionalFormatting></worksheet>"#
            );
            assert_unsupported(
                validate_worksheet(&xml, "Budget", &HashMap::new())
                    .expect_err("the unsupported detail must not be replaced with a guess"),
                "unsupported conditional-format rule on printed sheet: Budget",
            );
        }
    }

    #[test]
    fn worksheet_accepts_only_linked_x14_data_bar_details_the_renderer_matches() {
        let supported = r#"<m:worksheet xmlns:m="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><m:conditionalFormatting sqref="A1:A3"><m:cfRule type="dataBar" priority="1"><m:dataBar minLength="10" maxLength="90" showValue="1"><m:cfvo type="num" val="0"/><m:cfvo type="max" val="0"/><m:color rgb="FF638EC6"/></m:dataBar><m:extLst><m:ext><x14:id>{BAR-ID}</x14:id></m:ext></m:extLst></m:cfRule></m:conditionalFormatting><m:extLst><m:ext><x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="dataBar" id="{BAR-ID}"><x14:dataBar minLength="10" maxLength="90" gradient="true" axisPosition="none"><x14:cfvo type="num"><xm:f>0</xm:f></x14:cfvo><x14:cfvo type="max"/><x14:negativeFillColor rgb="FF638EC6"/><x14:axisColor rgb="FF000000"/></x14:dataBar></x14:cfRule><xm:sqref>A1:A3</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings></m:ext></m:extLst></m:worksheet>"#;
        validate_worksheet(supported, "Budget", &HashMap::new())
            .expect("the extension adds no rendering semantics beyond the linked base rule");

        for unsupported in [
            supported.replace("{BAR-ID}", "{UNLINKED-ID}").replacen(
                "<x14:id>{UNLINKED-ID}</x14:id>",
                "<x14:id>{BAR-ID}</x14:id>",
                1,
            ),
            supported.replace("axisPosition=\"none\"", "axisPosition=\"middle\""),
            supported.replace("gradient=\"true\"", "gradient=\"false\""),
            supported.replace("<x14:dataBar ", "<x14:dataBar border=\"1\" "),
            supported.replace("<m:color rgb=\"FF638EC6\"/>", "<m:color rgb=\"FF0000FF\"/>"),
        ] {
            assert_unsupported(
                validate_worksheet(&unsupported, "Budget", &HashMap::new())
                    .expect_err("an x14 semantic difference must not be ignored"),
                "unsupported conditional-format rule on printed sheet: Budget",
            );
        }
    }

    #[test]
    fn worksheet_refuses_priority_across_overlapping_conditional_format_groups() {
        let overlapping = r#"<worksheet><conditionalFormatting sqref="A1:A2"><cfRule type="expression" priority="1"><formula>TRUE</formula></cfRule></conditionalFormatting><conditionalFormatting sqref="A2:A3"><cfRule type="expression" priority="2"><formula>TRUE</formula></cfRule></conditionalFormatting></worksheet>"#;
        assert_unsupported(
            validate_worksheet(overlapping, "Budget", &HashMap::new())
                .expect_err("the renderer does not globally order separate overlapping groups"),
            "unsupported conditional-format rule on printed sheet: Budget",
        );

        let disjoint = overlapping.replace("A2:A3", "B1:B3");
        validate_worksheet(&disjoint, "Budget", &HashMap::new())
            .expect("priority cannot change the result for disjoint ranges");
    }

    #[test]
    fn drawing_connector_is_ignored_only_when_it_does_not_print() {
        let visible = r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:twoCellAnchor><xdr:cxnSp/><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
        assert_unsupported(
            validate_worksheet_drawing(visible, "Budget")
                .expect_err("a visible connector is not drawn"),
            "printable drawing object connector shape on sheet: Budget",
        );

        let hidden = r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:twoCellAnchor><xdr:cxnSp/><xdr:clientData fPrintsWithSheet="0"/></xdr:twoCellAnchor></xdr:wsDr>"#;
        assert!(
            validate_worksheet_drawing(hidden, "Budget")
                .expect("a nonprinting connector is absent")
                .is_empty()
        );
    }

    #[test]
    fn drawing_rejects_standalone_shape_and_non_chart_frame() {
        let shape = r#"<xdr:wsDr xmlns:xdr="urn:xdr"><xdr:twoCellAnchor><xdr:sp><xdr:spPr/></xdr:sp><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
        assert_unsupported(
            validate_worksheet_drawing(shape, "Budget")
                .expect_err("a visible shape without a text paragraph is not drawn"),
            "printable standalone shape on sheet: Budget",
        );

        let frame = r#"<xdr:wsDr xmlns:xdr="urn:xdr" xmlns:a="urn:a"><xdr:twoCellAnchor><xdr:graphicFrame><a:graphic><a:graphicData uri="urn:unsupported"/></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
        assert_unsupported(
            validate_worksheet_drawing(frame, "Budget")
                .expect_err("a non-chart graphic frame is not drawn"),
            "non-chart graphic frame on sheet: Budget",
        );
    }

    #[test]
    fn drawing_accepts_text_shape_and_returns_chart_relationship() {
        let xml = r#"<xdr:wsDr xmlns:xdr="urn:xdr" xmlns:a="urn:a" xmlns:c="urn:c" xmlns:r="urn:r"><xdr:twoCellAnchor><xdr:sp><xdr:txBody><a:p/></xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor><xdr:twoCellAnchor><xdr:graphicFrame><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart r:id="rId7"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
        assert_eq!(
            validate_worksheet_drawing(xml, "Budget").expect("both objects are modeled"),
            vec!["rId7"]
        );
    }

    #[test]
    fn chart_drawing_rejects_every_non_shape_object() {
        for object in ["pic", "grpSp", "graphicFrame", "cxnSp", "contentPart"] {
            let xml = format!(
                r#"<cdr:userShapes xmlns:cdr="http://schemas.openxmlformats.org/drawingml/2006/chartDrawing"><cdr:relSizeAnchor><cdr:{object}/></cdr:relSizeAnchor></cdr:userShapes>"#
            );
            assert_unsupported(
                validate_chart_drawing(&xml, "Budget")
                    .expect_err("only chart user-shape sp objects are modeled"),
                &format!("chart user-shape object {object} on sheet: Budget"),
            );
        }
    }

    #[test]
    fn chart_drawing_rejects_shape_features_its_renderer_does_not_model() {
        let base = r#"<cdr:userShapes xmlns:cdr="http://schemas.openxmlformats.org/drawingml/2006/chartDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><cdr:relSizeAnchor><cdr:from><cdr:x>0</cdr:x><cdr:y>0.1</cdr:y></cdr:from><cdr:to><cdr:x>0.5</cdr:x><cdr:y>0.3</cdr:y></cdr:to><cdr:sp macro="" textlink=""><cdr:nvSpPr><cdr:cNvPr id="2" name="Caption"/><cdr:cNvSpPr txBox="1"/></cdr:nvSpPr><cdr:spPr><a:xfrm><a:off x="0" y="1000"/><a:ext cx="5000" cy="2000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></cdr:spPr><cdr:txBody><a:bodyPr anchor="t" rtlCol="0" wrap="none"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="l"/><a:r><a:rPr b="1" lang="en-US" sz="1500"><a:solidFill><a:schemeClr val="accent1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill><a:latin typeface="+mj-lt"/></a:rPr><a:t>CASH FLOW</a:t></a:r></a:p></cdr:txBody></cdr:sp></cdr:relSizeAnchor></cdr:userShapes>"#;
        validate_chart_drawing(base, "Budget")
            .expect("the shape uses only rendered or neutral chart-drawing properties");

        for xml in [
            base.replace("<a:xfrm>", "<a:xfrm rot=\"5400000\">"),
            base.replace("prst=\"rect\"", "prst=\"ellipse\""),
            base.replacen("<a:noFill/>", "<a:gradFill/>", 1),
            base.replacen(
                "<a:noFill/>",
                "<a:effectLst><a:glow rad=\"12700\"/></a:effectLst><a:noFill/>",
                1,
            ),
            base.replace("anchor=\"t\"", "anchor=\"b\""),
            base.replace("algn=\"l\"", "algn=\"dist\""),
            base.replace("sz=\"1500\"", "sz=\"1500\" u=\"sng\""),
            base.replace("<cdr:y>0.1</cdr:y>", ""),
            base.replace("<cdr:to><cdr:x>0.5</cdr:x>", "<cdr:to><cdr:x>0</cdr:x>"),
            base.replace(
                "</cdr:sp></cdr:relSizeAnchor>",
                "</cdr:sp><cdr:sp/></cdr:relSizeAnchor>",
            ),
            base.replacen(
                "<a:noFill/>",
                "<a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>",
                1,
            ),
        ] {
            assert_unsupported(
                validate_chart_drawing(&xml, "Budget")
                    .expect_err("an unrendered chart user-shape property must refuse"),
                "chart user-shape formatting on sheet: Budget",
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_scatter_and_accepts_a_drawable_column_chart() {
        let scatter = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:scatterChart><c:ser><c:xVal><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>10</c:v></c:pt></c:numLit></c:xVal><c:yVal><c:numLit><c:pt idx="0"><c:v>2</c:v></c:pt><c:pt idx="1"><c:v>3</c:v></c:pt></c:numLit></c:yVal></c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        assert_unsupported(
            validate_chart_xml(scatter, "Budget")
                .expect_err("scatter values are not plotted on a numeric x-axis"),
            "unsupported chart plot family on sheet: Budget",
        );

        let column = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>4</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(column, "Budget").expect("the column chart has a plotted series");
        assert_unsupported(
            validate_chart_xml(
                &column.replace(
                    "<c:barDir val=\"col\"/>",
                    "<c:barDir val=\"col\" hidden=\"1\"/>",
                ),
                "Budget",
            )
            .expect_err("extra attributes on a modeled chart property must refuse"),
            "unsupported chart detail barDir on sheet: Budget",
        );
    }

    #[test]
    fn chart_preflight_rejects_unmodelled_geometry_and_accepts_no_op_settings() {
        let base = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:marker val="1"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:val>FEATURE</c:ser></c:lineChart><c:catAx><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:tickLblPos val="nextTo"/><c:minorTickMark val="none"/></c:catAx><c:valAx><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:crossBetween val="between"/><c:crosses val="autoZero"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(&base.replace("FEATURE", r#"<c:smooth val="0"/>"#), "Budget")
            .expect("explicit defaults do not change the plotted line");

        for feature in [
            r#"<c:smooth val="1"/>"#,
            "<c:dLbls><c:dLbl><c:idx val=\"0\"/></c:dLbl></c:dLbls>",
            "<c:hiLowLines/>",
        ] {
            assert_unsupported(
                validate_chart_xml(&base.replace("FEATURE", feature), "Budget")
                    .expect_err("the renderer does not model this chart geometry"),
                &format!(
                    "unsupported chart detail {} on sheet: Budget",
                    if feature.contains("dLbl") {
                        "dLbl"
                    } else if feature.contains("hiLowLines") {
                        "hiLowLines"
                    } else {
                        "smooth"
                    }
                ),
            );
        }

        let reversed = base
            .replace("FEATURE", "")
            .replace("orientation val=\"minMax\"", "orientation val=\"maxMin\"");
        assert_unsupported(
            validate_chart_xml(&reversed, "Budget")
                .expect_err("a reversed axis cannot be drawn as a forward axis"),
            "unsupported chart detail orientation on sheet: Budget",
        );
    }

    #[test]
    fn chart_preflight_rejects_silently_defaulted_plot_and_axis_settings() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser><c:gapWidth val="150"/><c:overlap val="0"/></c:barChart><c:catAx><c:scaling><c:min val="0"/><c:max val="10"/></c:scaling><c:axPos val="b"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx><c:valAx><c:axPos val="l"/><c:majorUnit val="2"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("all explicit settings are rendered defaults");

        for (xml, detail) in [
            (
                base.replace("varyColors val=\"0\"", "varyColors val=\"1\""),
                "varyColors",
            ),
            (
                base.replace("grouping val=\"clustered\"", "grouping val=\"mystery\""),
                "grouping",
            ),
            (
                base.replace("barDir val=\"col\"", "barDir val=\"diagonal\""),
                "barDir",
            ),
            (
                base.replace("gapWidth val=\"150\"", "gapWidth val=\"wide\""),
                "gapWidth",
            ),
            (
                base.replace("overlap val=\"0\"", "overlap val=\"wide\""),
                "overlap",
            ),
            (
                base.replace("<c:max val=\"10\"/>", "<c:max val=\"NaN\"/>"),
                "max",
            ),
            (
                base.replace("majorUnit val=\"2\"", "majorUnit val=\"0\""),
                "majorUnit",
            ),
            (
                base.replace("<c:auto val=\"1\"/>", "<c:auto val=\"0\"/>"),
                "auto",
            ),
            (
                base.replace("lblAlgn val=\"ctr\"", "lblAlgn val=\"l\""),
                "lblAlgn",
            ),
            (
                base.replace("lblOffset val=\"100\"", "lblOffset val=\"80\""),
                "lblOffset",
            ),
            (
                base.replace("noMultiLvlLbl val=\"0\"", "noMultiLvlLbl val=\"1\""),
                "noMultiLvlLbl",
            ),
            (
                base.replace(
                    "<c:min val=\"0\"/>",
                    "<c:logBase val=\"10\"/><c:min val=\"0\"/>",
                ),
                "logBase",
            ),
            (
                base.replace("<c:crossBetween", "<c:crossesAt val=\"5\"/><c:crossBetween"),
                "crossesAt",
            ),
            (
                base.replace("<c:crossBetween", "<c:dispUnits/><c:crossBetween"),
                "dispUnits",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("the renderer must not default this setting"),
                &format!("unsupported chart detail {detail} on sheet: Budget"),
            );
        }

        let doughnut = base
            .replace("<c:barChart>", "<c:doughnutChart>")
            .replace("</c:barChart>", "<c:holeSize val=\"5\"/></c:doughnutChart>")
            .replace("<c:barDir val=\"col\"/>", "")
            .replace("<c:grouping val=\"clustered\"/>", "")
            .replace("<c:gapWidth val=\"150\"/><c:overlap val=\"0\"/>", "")
            .replace("varyColors val=\"0\"", "varyColors val=\"1\"");
        assert_unsupported(
            validate_chart_xml(&doughnut, "Budget").expect_err("a 5% doughnut hole is invalid"),
            "unsupported chart detail holeSize on sheet: Budget",
        );
    }

    #[test]
    fn chart_preflight_rejects_data_point_visuals_it_does_not_draw() {
        let base = r#"<c:chartSpace xmlns:c="urn:c" xmlns:a="urn:a"><c:chart><c:plotArea><c:pieChart><c:varyColors val="1"/><c:ser><c:dPt><c:idx val="0"/><c:bubble3D val="0"/><c:invertIfNegative val="0"/><c:spPr><a:solidFill><a:srgbClr val="336699"/></a:solidFill><a:ln><a:noFill/></a:ln><a:effectLst/></c:spPr></c:dPt><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("the point fill and no-outline are represented");

        for (xml, detail) in [
            (base.replace("<a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>", "<a:noFill/>"), "data point formatting"),
            (base.replace("<a:srgbClr val=\"336699\"/>", "<a:srgbClr val=\"336699\"><a:alpha val=\"50000\"/></a:srgbClr>"), "data point formatting"),
            (base.replace("<a:ln><a:noFill/></a:ln>", "<a:ln w=\"12700\"><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill></a:ln>"), "data point formatting"),
            (base.replace("bubble3D val=\"0\"", "bubble3D val=\"1\""), "bubble3D"),
            (base.replace("<a:effectLst/>", "<a:effectLst><a:glow rad=\"12700\"/></a:effectLst>"), "data point formatting"),
            (base.replace("<c:idx val=\"0\"/>", "<c:idx val=\"1\"/>"), "data point formatting"),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget").expect_err("the data-point visual would change"),
                &format!("unsupported chart detail {detail} on sheet: Budget"),
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_series_shape_details_it_does_not_draw() {
        let column = r#"<c:chartSpace xmlns:c="urn:c" xmlns:a="urn:a"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:spPr><a:solidFill><a:srgbClr val="336699"/></a:solidFill><a:ln><a:noFill/></a:ln><a:effectLst/></c:spPr><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(column, "Budget")
            .expect("the bar fill and absent outline are represented");
        for xml in [
            column.replace(
                "<a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>",
                "<a:noFill/>",
            ),
            column.replace(
                "<a:srgbClr val=\"336699\"/>",
                "<a:srgbClr val=\"336699\"><a:alpha val=\"50000\"/></a:srgbClr>",
            ),
            column.replace(
                "<a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>",
                "<a:gradFill/>",
            ),
            column.replace(
                "<a:effectLst/>",
                "<a:effectLst><a:glow rad=\"12700\"/></a:effectLst>",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget").expect_err("the series visual would change"),
                "unsupported chart detail series formatting on sheet: Budget",
            );
        }

        let line = column
            .replace(
                "<c:barChart><c:barDir val=\"col\"/>",
                "<c:lineChart><c:grouping val=\"standard\"/><c:marker val=\"1\"/>",
            )
            .replace("</c:barChart>", "</c:lineChart>")
            .replace(
                "<c:pt idx=\"0\"><c:v>A</c:v></c:pt>",
                "<c:pt idx=\"0\"><c:v>A</c:v></c:pt><c:pt idx=\"1\"><c:v>B</c:v></c:pt>",
            )
            .replace(
                "<c:pt idx=\"0\"><c:v>1</c:v></c:pt>",
                "<c:pt idx=\"0\"><c:v>1</c:v></c:pt><c:pt idx=\"1\"><c:v>2</c:v></c:pt>",
            );
        assert_unsupported(
            validate_chart_xml(&line, "Budget").expect_err("a no-fill line must remain invisible"),
            "unsupported chart detail series formatting on sheet: Budget",
        );
    }

    #[test]
    fn chart_preflight_rejects_unrendered_shape_properties() {
        let base = r#"<c:chartSpace xmlns:c="urn:c" xmlns:a="urn:a">CHARTSPACE<c:chart><c:plotArea>PLOT<c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser>LABELS</c:barChart><c:catAx><c:axPos val="b"/>AXIS</c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/></c:valAx></c:plotArea>LEGEND</c:chart></c:chartSpace>"#;
        let rendered = base.replace(
            "CHARTSPACE",
            r#"<c:spPr><a:solidFill><a:srgbClr val="336699"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:effectLst/></c:spPr>"#,
        );
        validate_chart_xml(&rendered, "Budget")
            .expect("the chart-area fill and solid outline are rendered");

        for (xml, detail) in [
            (
                base.replace("CHARTSPACE", "<c:spPr><a:gradFill/></c:spPr>"),
                "chart area formatting",
            ),
            (
                rendered.replace(
                    "<a:srgbClr val=\"336699\"/>",
                    "<a:srgbClr val=\"336699\"><a:alpha val=\"50000\"/></a:srgbClr>",
                ),
                "chart area formatting",
            ),
            (
                rendered.replace("val=\"336699\"", "val=\"336699\" ignored=\"1\""),
                "chart area formatting",
            ),
            (
                rendered.replace(
                    "<a:srgbClr val=\"336699\"/>",
                    "<a:srgbClr val=\"336699\"><a:tint val=\"-1\"/></a:srgbClr>",
                ),
                "chart area formatting",
            ),
            (
                rendered.replace("prstDash val=\"solid\"", "prstDash val=\"dash\""),
                "chart area formatting",
            ),
            (
                rendered.replace("<a:ln w=\"12700\">", "<a:ln w=\"12700\" cap=\"rnd\">"),
                "chart area formatting",
            ),
            (
                rendered.replace("<a:ln w=\"12700\">", "<a:ln w=\"12700\" cmpd=\"dbl\">"),
                "chart area formatting",
            ),
            (
                rendered.replace("<a:ln w=\"12700\">", "<a:ln w=\"12700\" algn=\"in\">"),
                "chart area formatting",
            ),
            (
                base.replace("CHARTSPACE", "<c:spPr><a:solidFill/></c:spPr>"),
                "chart area formatting",
            ),
            (
                base.replace(
                    "CHARTSPACE",
                    "<c:spPr><a:solidFill><a:srgbClr val=\"000000\"/><a:srgbClr val=\"FFFFFF\"/></a:solidFill></c:spPr>",
                ),
                "chart area formatting",
            ),
            (
                rendered.replacen(
                    "<a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>",
                    "<a:noFill/><a:solidFill><a:srgbClr val=\"336699\"/></a:solidFill>",
                    1,
                ),
                "chart area formatting",
            ),
            (
                rendered.replacen(
                    "<a:ln w=\"12700\">",
                    "<a:ln><a:noFill/></a:ln><a:ln w=\"12700\">",
                    1,
                ),
                "chart area formatting",
            ),
            (
                base.replace("PLOT", "<c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr>"),
                "plot area formatting",
            ),
            (
                base.replace("LABELS", "<c:dLbls><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:dLbls>"),
                "data label formatting",
            ),
            (
                base.replace("LEGEND", "<c:legend><c:spPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></c:spPr></c:legend>"),
                "legend formatting",
            ),
            (
                base.replace("AXIS", "<c:spPr><a:ln><a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill><a:prstDash val=\"dash\"/></a:ln></c:spPr>"),
                "axis formatting",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("the renderer must not silently drop shape properties"),
                &format!("unsupported chart detail {detail} on sheet: Budget"),
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_overlay_and_lossy_manual_layout() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart>DETAIL<c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        for (detail, expected) in [
            (
                r#"<c:title><c:overlay val="1"/></c:title>"#,
                "unsupported chart detail overlay on sheet: Budget",
            ),
            (
                r#"<c:legend><c:legendPos val="corner"/></c:legend>"#,
                "unsupported chart detail legend position on sheet: Budget",
            ),
            (
                r#"<c:title><c:layout><c:manualLayout><c:xMode val="factor"/><c:yMode val="edge"/><c:x val="0.1"/><c:y val="0.1"/></c:manualLayout></c:layout></c:title>"#,
                "unsupported chart detail manual layout on sheet: Budget",
            ),
            (
                r#"<c:title><c:layout><c:manualLayout><c:xMode val="edge"/><c:yMode val="edge"/><c:x val="0.1"/><c:y val="0.1"/><c:w val="0.5"/></c:manualLayout></c:layout></c:title>"#,
                "unsupported chart detail manual layout on sheet: Budget",
            ),
        ] {
            let xml = base.replace("DETAIL", detail);
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("the renderer must not silently change chart placement"),
                expected,
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_unrendered_text_rotation_and_run_properties() {
        let base = r#"<c:chartSpace xmlns:c="urn:c" xmlns:a="urn:a"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/><c:txPr><a:bodyPr rot="-60000000" spcFirstLastPara="1" vertOverflow="ellipsis" vert="horz" wrap="square" anchor="ctr" anchorCtr="1"/><a:p><a:pPr><a:defRPr sz="900" b="0" i="0" u="none" strike="noStrike" kern="1200" baseline="0"/></a:pPr></a:p></c:txPr></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/><c:title><c:tx><c:rich><a:bodyPr rot="-5400000"/><a:p><a:pPr><a:defRPr sz="900" b="1" i="0" u="none" strike="noStrike" baseline="0"/></a:pPr><a:r><a:rPr sz="900" b="1" i="0" u="none" strike="noStrike" baseline="0"/><a:t>Value</a:t></a:r></a:p></c:rich></c:tx></c:title></c:valAx></c:plotArea></c:chart><c:txPr><a:bodyPr rot="0" wrap="square"/><a:p><a:pPr><a:defRPr sz="1000" b="0" i="0" u="none" strike="noStrike" kern="1200" baseline="0"/></a:pPr></a:p></c:txPr></c:chartSpace>"#;
        validate_chart_xml(base, "Budget")
            .expect("neutral text properties and the two modeled rotations are admitted");
        let language_metadata = base.replacen(
            "<a:rPr sz=\"900\" b=\"1\" i=\"0\" u=\"none\" strike=\"noStrike\" baseline=\"0\"/>",
            "<a:rPr sz=\"900\" b=\"1\" i=\"0\" u=\"none\" strike=\"noStrike\" baseline=\"0\" lang=\"en-US\"/>",
            1,
        );
        validate_chart_xml(&language_metadata, "Budget")
            .expect("language metadata does not change rendered chart text");
        let left_to_right_paragraph = base.replacen("<a:pPr>", "<a:pPr rtl=\"0\">", 1);
        validate_chart_xml(&left_to_right_paragraph, "Budget")
            .expect("an explicit left-to-right paragraph does not change rendered chart text");
        let script_defaults = base.replacen(
            "<a:defRPr sz=\"900\" b=\"0\" i=\"0\" u=\"none\" strike=\"noStrike\" kern=\"1200\" baseline=\"0\"/>",
            "<a:defRPr sz=\"900\" b=\"0\" i=\"0\" u=\"none\" strike=\"noStrike\" kern=\"1200\" baseline=\"0\"><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr>",
            1,
        );
        validate_chart_xml(&script_defaults, "Budget")
            .expect("standard minor-script slots use the modeled mixed-script fallback");

        for (xml, detail) in [
            (
                base.replacen("rot=\"-60000000\"", "rot=\"-2700000\"", 1),
                "text orientation",
            ),
            (
                base.replacen("rot=\"0\" wrap=\"square\"", "rot=\"5400000\" wrap=\"square\"", 1),
                "text orientation",
            ),
            (
                base.replacen("i=\"0\"", "i=\"1\"", 1),
                "text formatting",
            ),
            (
                base.replacen("u=\"none\"", "u=\"sng\"", 1),
                "text formatting",
            ),
            (
                base.replacen("strike=\"noStrike\"", "strike=\"sngStrike\"", 1),
                "text formatting",
            ),
            (
                base.replacen("baseline=\"0\"", "baseline=\"25000\"", 1),
                "text formatting",
            ),
            (
                base.replacen("<a:pPr>", "<a:pPr rtl=\"1\">", 1),
                "text formatting",
            ),
            (
                script_defaults.replacen("+mn-ea", "MS Gothic", 1),
                "text formatting",
            ),
            (
                base.replacen("vert=\"horz\"", "vert=\"vert\"", 1),
                "text layout",
            ),
            (
                base.replacen("anchor=\"ctr\"", "anchor=\"b\"", 1),
                "text layout",
            ),
            (
                base.replacen(
                    "<a:rPr sz=\"900\" b=\"1\" i=\"0\" u=\"none\" strike=\"noStrike\" baseline=\"0\"/>",
                    "<a:rPr sz=\"900\" b=\"1\" i=\"0\" u=\"none\" strike=\"noStrike\" baseline=\"0\"><a:glow rad=\"63500\"/></a:rPr>",
                    1,
                ),
                "text formatting",
            ),
            (
                base.replacen(
                    "</a:r></a:p></c:rich>",
                    "</a:r><a:r><a:rPr sz=\"900\"/><a:t> axis</a:t></a:r></a:p></c:rich>",
                    1,
                ),
                "multi-run text",
            ),
            (
                base.replacen(
                    "</a:r></a:p></c:rich>",
                    "</a:r></a:p><a:p><a:r><a:t>axis</a:t></a:r></a:p></c:rich>",
                    1,
                ),
                "multi-paragraph text",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("a visible chart text property must not disappear"),
                &format!("unsupported chart detail {detail} on sheet: Budget"),
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_collapsed_cache_gaps_and_unmodelled_axes() {
        let gapped = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:ptCount val="3"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="2"><c:v>C</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="2"><c:v>3</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"#;
        assert_unsupported(
            validate_chart_xml(gapped, "Budget")
                .expect_err("compressing missing cache indices moves later points"),
            "unsupported chart detail non-contiguous data cache on sheet: Budget",
        );

        let secondary_axis = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx/><c:valAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"#;
        assert_unsupported(
            validate_chart_xml(secondary_axis, "Budget")
                .expect_err("the renderer has only one value axis"),
            "unsupported chart detail multiple category or value axes on sheet: Budget",
        );

        let markerless_line = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:marker val="0"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:val></c:ser></c:lineChart><c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"#;
        assert_unsupported(
            validate_chart_xml(markerless_line, "Budget")
                .expect_err("automatic markers would appear despite the chart-level switch"),
            "unsupported chart detail line marker visibility on sheet: Budget",
        );
    }

    #[test]
    fn chart_preflight_rejects_series_order_marker_and_style_changes_it_does_not_draw() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:style val="2"/><c:chart><c:plotArea><c:lineChart><c:marker val="1"/><c:ser><c:idx val="0"/><c:order val="0"/><c:marker><c:symbol val="circle"/><c:size val="5"/></c:marker><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:val></c:ser></c:lineChart><c:catAx><c:axPos val="b"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("the rendered defaults are admitted");
        for size in ["2", "6", "9", "72"] {
            let xml = base.replace("size val=\"5\"", &format!("size val=\"{size}\""));
            validate_chart_xml(&xml, "Budget")
                .unwrap_or_else(|error| panic!("marker size {size} must be rendered: {error}"));
        }

        for (xml, detail) in [
            (
                base.replace("<c:order val=\"0\"/>", "<c:order val=\"1\"/>"),
                "series ordering",
            ),
            (
                base.replace("symbol val=\"circle\"", "symbol val=\"star\""),
                "marker symbol",
            ),
            (
                base.replace("size val=\"5\"", "size val=\"1\""),
                "marker size",
            ),
            (
                base.replace("size val=\"5\"", "size val=\"73\""),
                "marker size",
            ),
            (
                base.replace("<c:style val=\"2\"/>", "<c:style val=\"3\"/>"),
                "chart style",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget").expect_err("the renderer changes this detail"),
                &format!("unsupported chart detail {detail} on sheet: Budget"),
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_extra_or_invalid_attributes_on_modeled_properties() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:dLbls><c:numFmt formatCode="0.0%" sourceLinked="0"/><c:showVal val="1"/></c:dLbls><c:cat><c:strLit><c:ptCount val="1"/><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="1"/><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/><c:majorTickMark val="out"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/><c:numFmt formatCode="0%" sourceLinked="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("every stated property is modeled");

        for xml in [
            base.replace("<c:order val=\"0\"/>", "<c:order val=\"0\" hidden=\"1\"/>"),
            base.replacen("<c:pt idx=\"0\">", "<c:pt idx=\"0\" hidden=\"1\">", 1),
            base.replacen(
                "<c:ptCount val=\"1\"/>",
                "<c:ptCount val=\"1\" hidden=\"1\"/>",
                1,
            ),
            base.replace("<c:showVal val=\"1\"/>", "<c:showVal val=\"maybe\"/>"),
            base.replace(
                "<c:majorTickMark val=\"out\"/>",
                "<c:majorTickMark val=\"sideways\"/>",
            ),
            base.replace(
                "<c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/>",
                "<c:numFmt formatCode=\"0%\" sourceLinked=\"0\" hidden=\"1\"/>",
            ),
            base.replace("<c:plotArea>", "<c:plotArea hidden=\"1\">"),
            base.replace("<c:barChart>", "<c:barChart hidden=\"1\">"),
            base.replace("<c:ser>", "<c:ser hidden=\"1\">"),
            base.replace("<c:plotArea>", "<c:showFancyLabels val=\"1\"/><c:plotArea>"),
        ] {
            assert!(
                validate_chart_xml(&xml, "Budget").is_err(),
                "a property the parser only partly understands must refuse: {xml}"
            );
        }
    }

    #[test]
    fn chart_preflight_requires_the_exact_supported_alternate_style_branches() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart" Requires="c14"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("the exact supported style pair is neutral");

        for xml in [
            base.replace("Requires=\"c14\"", "Requires=\"unknown\""),
            base.replace("Requires=\"c14\"", "Requires=\"c14\" hidden=\"1\""),
            base.replace("<mc:Fallback>", "<mc:Fallback hidden=\"1\">"),
        ] {
            assert!(
                validate_chart_xml(&xml, "Budget").is_err(),
                "an alternate branch the renderer cannot select exactly must refuse: {xml}"
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_duplicate_singleton_properties() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:dLbls><c:numFmt formatCode="0.0%" sourceLinked="0"/><c:showVal val="1"/></c:dLbls><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/><c:numFmt formatCode="0%" sourceLinked="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("one of each modeled property is unambiguous");

        for xml in [
            base.replace(
                "<c:barDir val=\"col\"/>",
                "<c:barDir val=\"col\"/><c:barDir val=\"bar\"/>",
            ),
            base.replace(
                "<c:showVal val=\"1\"/>",
                "<c:showVal val=\"1\"/><c:showVal val=\"0\"/>",
            ),
            base.replace(
                "<c:axPos val=\"l\"/>",
                "<c:axPos val=\"l\"/><c:axPos val=\"b\"/>",
            ),
            base.replace(
                "<c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/>",
                "<c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/>",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("last-wins chart properties must not pass preflight"),
                "unsupported chart detail duplicate property on sheet: Budget",
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_modeled_properties_in_unmodeled_scopes() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:dLbls><c:showVal val="1"/></c:dLbls><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/><c:majorUnit val="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("each property is in the scope the parser reads");

        for xml in [
            base.replace("<c:chart>", "<c:chart><c:showVal val=\"1\"/>"),
            base.replace(
                "<c:barDir val=\"col\"/><c:ser>",
                "<c:ser><c:barDir val=\"col\"/>",
            ),
            base.replace(
                "<c:majorUnit val=\"1\"/>",
                "<c:majorUnit val=\"1\"/><c:gapWidth val=\"150\"/>",
            ),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("a valid property in an ignored scope must refuse"),
                "unsupported chart detail chart structure on sheet: Budget",
            );
        }
    }

    #[test]
    fn chart_preflight_validates_axis_topology_and_neutral_extensions() {
        let base = r#"<c:chartSpace xmlns:c="urn:c" xmlns:c16="http://schemas.microsoft.com/office/drawing/2014/chart"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/><c:order val="0"/><c:extLst><c:ext uri="{C3380CC4-5D6E-409C-BE32-E72D297353CC}"><c16:uniqueId val="{SERIES}"/></c:ext></c:extLst><c:dLbls><c:extLst><c:ext xmlns:c15="http://schemas.microsoft.com/office/drawing/2012/chart" uri="{CE6537A1-D6FC-4f65-9D91-7224C49458BB}"><c15:showLeaderLines val="0"/></c:ext></c:extLst></c:dLbls><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:axPos val="b"/><c:crossAx val="20"/></c:catAx><c:valAx><c:axId val="20"/><c:axPos val="l"/><c:crossAx val="10"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        validate_chart_xml(base, "Budget").expect("axis links and the neutral series id agree");

        for xml in [
            base.replacen("<c:crossAx val=\"20\"/>", "<c:crossAx val=\"30\"/>", 1),
            base.replacen("<c:axId val=\"20\"/>", "<c:axId val=\"30\"/>", 1),
            base.replace(
                "uri=\"{C3380CC4-5D6E-409C-BE32-E72D297353CC}\"",
                "uri=\"{UNKNOWN}\"",
            ),
            base.replace("val=\"{SERIES}\"", "val=\"{SERIES}\" hidden=\"1\""),
            base.replace("<c15:showLeaderLines val=\"0\"/>", "<c15:showLeaderLines/>"),
            base.replace(
                "<c15:showLeaderLines val=\"0\"/>",
                "<c15:showLeaderLines val=\"1\"/>",
            ),
        ] {
            assert!(
                validate_chart_xml(&xml, "Budget").is_err(),
                "inconsistent topology or an unknown extension must refuse: {xml}"
            );
        }
    }

    #[test]
    fn chart_preflight_rejects_cache_values_that_would_collapse_or_misalign() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:ptCount val="2"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:ptCount val="2"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart><c:catAx><c:axPos val="b"/></c:catAx><c:valAx><c:axPos val="l"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        for xml in [
            base.replace("<c:v>2</c:v>", "<c:v>#N/A</c:v>"),
            base.replace("<c:v>B</c:v>", "<c:v></c:v>"),
            base.replace("<c:ptCount val=\"2\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt><c:pt idx=\"1\"><c:v>2</c:v></c:pt>", "<c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>1</c:v></c:pt>"),
        ] {
            assert_unsupported(
                validate_chart_xml(&xml, "Budget")
                    .expect_err("collapsed cache entries shift the plotted point"),
                "unsupported chart detail non-contiguous data cache on sheet: Budget",
            );
        }
    }

    #[test]
    fn chart_preflight_refuses_visible_only_plotting_when_hidden_source_data_exists() {
        let base = r#"<c:chartSpace xmlns:c="urn:c"><c:chart>VISIBLE<c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt></c:numLit></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        for visible in ["", r#"<c:plotVisOnly val="1"/>"#] {
            assert_unsupported(
                validate_chart_xml_with_hidden_sources(
                    &base.replace("VISIBLE", visible),
                    "Budget",
                    true,
                )
                .expect_err("the cached plot does not identify which hidden points disappear"),
                "unsupported chart detail hidden source data on sheet: Budget",
            );
        }

        validate_chart_xml_with_hidden_sources(
            &base.replace("VISIBLE", r#"<c:plotVisOnly val="0"/>"#),
            "Budget",
            true,
        )
        .expect("the chart explicitly includes hidden cells");
    }

    #[test]
    fn actual_active_package_parts_refuse_without_content_type_guessing() {
        for (part, expected) in [
            ("xl/vbaProject.bin", "macro-enabled workbook"),
            ("xl/activeX/activeX1.bin", "ActiveX control"),
            ("xl/embeddings/oleObject1.bin", "embedded OLE package"),
        ] {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            writer
                .start_file(part, zip::write::FileOptions::default())
                .expect("part starts");
            std::io::Write::write_all(&mut writer, b"probe").expect("part writes");
            let package = writer.finish().expect("package closes").into_inner();
            assert_unsupported(
                ensure_supported_package(&package, &HashSet::new())
                    .expect_err("an actual active part must refuse"),
                expected,
            );
        }
    }

    #[test]
    fn printed_dialog_sheet_refuses_before_rendering() {
        let package = zip_entries(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Dialog" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/dialogsheet" Target="dialogsheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/dialogsheets/sheet1.xml",
                br#"<dialogsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#,
            ),
        ]);
        assert_unsupported(
            ensure_supported_package(&package, &HashSet::from(["Dialog".to_string()]))
                .expect_err("the renderer does not draw dialog sheets"),
            "dialog sheet selected for printing: Dialog",
        );
    }

    #[test]
    fn visible_note_linked_from_a_printed_sheet_refuses() {
        let package = zip_entries(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><legacyDrawing r:id="rIdNote"/></worksheet>"#,
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdNote" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#,
            ),
            (
                "xl/drawings/vmlDrawing1.vml",
                br#"<xml xmlns:x="urn:schemas-microsoft-com:office:excel"><x:ClientData ObjectType="Note"><x:Row>0</x:Row><x:Column>0</x:Column><x:Visible/></x:ClientData></xml>"#,
            ),
        ]);
        assert_unsupported(
            ensure_supported_package(&package, &HashSet::from(["Budget".to_string()]))
                .expect_err("a visible note is part of the printed result"),
            "visible cell comment on sheet: Budget",
        );

        ensure_supported_package(&package, &HashSet::new())
            .expect("a note on a sheet that is not selected cannot affect the PDF");

        let hidden_package = zip_entries(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><legacyDrawing r:id="rIdNote"/></worksheet>"#,
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdNote" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#,
            ),
            (
                "xl/drawings/vmlDrawing1.vml",
                br#"<xml xmlns:x="urn:schemas-microsoft-com:office:excel"><x:ClientData ObjectType="Note"><x:Visible>False</x:Visible></x:ClientData></xml>"#,
            ),
        ]);
        ensure_supported_package(&hidden_package, &HashSet::from(["Budget".to_string()]))
            .expect("an explicit false visibility value leaves the note hidden");
    }
}

fn validate_worksheet_drawing(xml: &str, sheet_name: &str) -> Result<Vec<String>, ConvertError> {
    let mut reader = Reader::from_str(xml);
    let mut anchor: Option<DrawingAnchor> = None;
    let mut chart_rids = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element)) => {
                let local = element.local_name();
                match local.as_ref() {
                    b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                        anchor = Some(DrawingAnchor::new());
                    }
                    b"cxnSp" | b"grpSp" | b"contentPart" => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.unsupported = Some(match local.as_ref() {
                                b"cxnSp" => "connector shape",
                                b"grpSp" => "group shape",
                                _ => "content part",
                            });
                        }
                    }
                    b"sp" => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.saw_shape = true;
                            if attr_value(&reader, element, b"macro")
                                .is_some_and(|value| !value.is_empty())
                            {
                                anchor.unsupported = Some("macro callback");
                            }
                        }
                    }
                    b"txBody" => {
                        if let Some(anchor) = anchor.as_mut()
                            && anchor.saw_shape
                        {
                            anchor.in_shape_text = true;
                        }
                    }
                    b"p" => {
                        if let Some(anchor) = anchor.as_mut()
                            && anchor.in_shape_text
                        {
                            anchor.shape_has_paragraph = true;
                        }
                    }
                    b"graphicFrame" => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.graphic_frame = true;
                            if attr_value(&reader, element, b"macro")
                                .is_some_and(|value| !value.is_empty())
                            {
                                anchor.unsupported = Some("macro callback");
                            }
                        }
                    }
                    b"graphicData" => {
                        if let Some(anchor) = anchor.as_mut()
                            && anchor.graphic_frame
                        {
                            anchor.graphic_data_uri = attr_value(&reader, element, b"uri");
                        }
                    }
                    b"chart" => {
                        if let Some(anchor) = anchor.as_mut()
                            && anchor.graphic_frame
                            && let Some(rid) = attr_value(&reader, element, b"id")
                        {
                            anchor.chart_rids.push(rid);
                        }
                    }
                    b"clientData" => {
                        if let Some(anchor) = anchor.as_mut()
                            && false_value(attr_value(&reader, element, b"fPrintsWithSheet"))
                        {
                            anchor.prints = false;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"txBody" => {
                    if let Some(anchor) = anchor.as_mut() {
                        anchor.in_shape_text = false;
                    }
                }
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => {
                    if let Some(finished) = anchor.take() {
                        finish_drawing_anchor(finished, sheet_name, &mut chart_rids)?;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse drawing on sheet {sheet_name}: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(chart_rids)
}

#[derive(Default)]
struct ChartDrawingAnchorScan {
    relative: bool,
    from_x: Option<f64>,
    from_y: Option<f64>,
    to_x: Option<f64>,
    to_y: Option<f64>,
    extent: bool,
    shapes: usize,
    auto_fit: bool,
    explicit_paint: bool,
}

fn finish_chart_drawing_anchor(
    anchor: ChartDrawingAnchorScan,
    sheet_name: &str,
) -> Result<(), ConvertError> {
    let complete_origin = anchor.from_x.is_some() && anchor.from_y.is_some();
    let valid_extent = if anchor.relative {
        matches!(
            (anchor.from_x, anchor.from_y, anchor.to_x, anchor.to_y),
            (Some(from_x), Some(from_y), Some(to_x), Some(to_y))
                if to_x > from_x && to_y > from_y
        ) && !anchor.extent
    } else {
        complete_origin && anchor.extent && anchor.to_x.is_none() && anchor.to_y.is_none()
    };
    if !complete_origin
        || !valid_extent
        || anchor.shapes != 1
        || (anchor.auto_fit && anchor.explicit_paint)
    {
        return Err(chart_user_shape_formatting(sheet_name));
    }
    Ok(())
}

fn validate_chart_drawing(xml: &str, sheet_name: &str) -> Result<(), ConvertError> {
    let mut reader = Reader::from_str(xml);
    let mut ancestors: Vec<Vec<u8>> = Vec::new();
    let mut anchor: Option<ChartDrawingAnchorScan> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element)) => {
                validate_chart_drawing_element(&reader, element, &ancestors, sheet_name)?;
                let local = element.local_name();
                match local.as_ref() {
                    b"relSizeAnchor" | b"absSizeAnchor" => {
                        if anchor.is_some() {
                            return Err(chart_user_shape_formatting(sheet_name));
                        }
                        anchor = Some(ChartDrawingAnchorScan {
                            relative: local.as_ref() == b"relSizeAnchor",
                            ..ChartDrawingAnchorScan::default()
                        });
                    }
                    b"pic" | b"grpSp" | b"graphicFrame" | b"cxnSp" | b"contentPart"
                        if anchor.is_some() =>
                    {
                        let kind = String::from_utf8_lossy(local.as_ref());
                        return Err(unsupported(format!(
                            "chart user-shape object {kind} on sheet: {sheet_name}"
                        )));
                    }
                    b"sp"
                        if matches!(
                            ancestors.last().map(Vec::as_slice),
                            Some(b"relSizeAnchor" | b"absSizeAnchor")
                        ) =>
                    {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.shapes += 1;
                        }
                    }
                    b"ext"
                        if matches!(
                            ancestors.last().map(Vec::as_slice),
                            Some(b"relSizeAnchor" | b"absSizeAnchor")
                        ) =>
                    {
                        if let Some(anchor) = anchor.as_mut() {
                            if anchor.extent {
                                return Err(chart_user_shape_formatting(sheet_name));
                            }
                            anchor.extent = true;
                        }
                    }
                    b"solidFill" if ancestors.iter().any(|name| name == b"spPr") => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.explicit_paint = true;
                        }
                    }
                    b"spAutoFit" => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.auto_fit = true;
                        }
                    }
                    _ => {}
                }
                ancestors.push(local.as_ref().to_vec());
            }
            Ok(Event::Empty(ref element)) => {
                validate_chart_drawing_element(&reader, element, &ancestors, sheet_name)?;
                let local = element.local_name();
                if matches!(
                    local.as_ref(),
                    b"pic" | b"grpSp" | b"graphicFrame" | b"cxnSp" | b"contentPart"
                ) && anchor.is_some()
                {
                    let kind = String::from_utf8_lossy(local.as_ref());
                    return Err(unsupported(format!(
                        "chart user-shape object {kind} on sheet: {sheet_name}"
                    )));
                }
                match local.as_ref() {
                    b"relSizeAnchor" | b"absSizeAnchor" => {
                        return Err(chart_user_shape_formatting(sheet_name));
                    }
                    b"sp"
                        if matches!(
                            ancestors.last().map(Vec::as_slice),
                            Some(b"relSizeAnchor" | b"absSizeAnchor")
                        ) =>
                    {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.shapes += 1;
                        }
                    }
                    b"ext"
                        if matches!(
                            ancestors.last().map(Vec::as_slice),
                            Some(b"relSizeAnchor" | b"absSizeAnchor")
                        ) =>
                    {
                        if let Some(anchor) = anchor.as_mut() {
                            if anchor.extent {
                                return Err(chart_user_shape_formatting(sheet_name));
                            }
                            anchor.extent = true;
                        }
                    }
                    b"solidFill" if ancestors.iter().any(|name| name == b"spPr") => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.explicit_paint = true;
                        }
                    }
                    b"spAutoFit" => {
                        if let Some(anchor) = anchor.as_mut() {
                            anchor.auto_fit = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref text)) => {
                if matches!(ancestors.last().map(Vec::as_slice), Some(b"x" | b"y")) {
                    let value = text
                        .xml_content()
                        .ok()
                        .and_then(|value| value.trim().parse::<f64>().ok());
                    let relative = ancestors.iter().any(|name| name == b"relSizeAnchor");
                    if value.is_none_or(|value| {
                        !value.is_finite() || (relative && !(0.0..=1.0).contains(&value))
                    }) {
                        return Err(chart_user_shape_formatting(sheet_name));
                    }
                    let value = value.expect("validated chart drawing coordinate");
                    let target = match (
                        ancestors.iter().rev().nth(1).map(Vec::as_slice),
                        ancestors.last().map(Vec::as_slice),
                    ) {
                        (Some(b"from"), Some(b"x")) => {
                            anchor.as_mut().map(|anchor| &mut anchor.from_x)
                        }
                        (Some(b"from"), Some(b"y")) => {
                            anchor.as_mut().map(|anchor| &mut anchor.from_y)
                        }
                        (Some(b"to"), Some(b"x")) => anchor.as_mut().map(|anchor| &mut anchor.to_x),
                        (Some(b"to"), Some(b"y")) => anchor.as_mut().map(|anchor| &mut anchor.to_y),
                        _ => None,
                    };
                    let Some(target) = target else {
                        return Err(chart_user_shape_formatting(sheet_name));
                    };
                    if target.replace(value).is_some() {
                        return Err(chart_user_shape_formatting(sheet_name));
                    }
                }
            }
            Ok(Event::End(element)) => {
                if matches!(
                    element.local_name().as_ref(),
                    b"relSizeAnchor" | b"absSizeAnchor"
                ) {
                    let Some(finished) = anchor.take() else {
                        return Err(chart_user_shape_formatting(sheet_name));
                    };
                    finish_chart_drawing_anchor(finished, sheet_name)?;
                }
                ancestors.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse chart drawing on sheet {sheet_name}: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

fn chart_user_shape_formatting(sheet_name: &str) -> ConvertError {
    unsupported(format!(
        "chart user-shape formatting on sheet: {sheet_name}"
    ))
}

fn chart_drawing_attributes(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
) -> Option<HashMap<Vec<u8>, String>> {
    let mut values = HashMap::new();
    for attribute in element.attributes() {
        let attribute = attribute.ok()?;
        if chart_namespace_attribute_supported(reader, &attribute) {
            continue;
        }
        let key = attribute.key.local_name().as_ref().to_vec();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .ok()?
            .into_owned();
        if values.insert(key, value).is_some() {
            return None;
        }
    }
    Some(values)
}

fn chart_exact_attribute(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    name: &[u8],
) -> Option<String> {
    let attributes = chart_drawing_attributes(reader, element)?;
    (attributes.len() == 1)
        .then(|| attributes.get(name).cloned())
        .flatten()
}

fn validate_chart_drawing_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    let name = element.local_name();
    if matches!(
        name.as_ref(),
        b"pic" | b"grpSp" | b"graphicFrame" | b"cxnSp" | b"contentPart"
    ) && ancestors
        .iter()
        .any(|ancestor| matches!(ancestor.as_slice(), b"relSizeAnchor" | b"absSizeAnchor"))
    {
        return Err(unsupported(format!(
            "chart user-shape object {} on sheet: {sheet_name}",
            String::from_utf8_lossy(name.as_ref())
        )));
    }
    let parent = ancestors.last().map(Vec::as_slice);
    if ancestors.iter().any(|ancestor| ancestor == b"effectLst") {
        return Err(chart_user_shape_formatting(sheet_name));
    }
    if chart_color_element_supported(reader, element, parent) {
        return Ok(());
    }
    let Some(attributes) = chart_drawing_attributes(reader, element) else {
        return Err(chart_user_shape_formatting(sheet_name));
    };
    let empty = || attributes.is_empty();
    let exact = |key: &[u8]| {
        if attributes.len() == 1 {
            attributes.get(key).map(String::as_str)
        } else {
            None
        }
    };
    let finite = |key: &[u8], positive: bool| {
        attributes
            .get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|value| value.is_finite() && (!positive || value > 0.0))
    };
    let valid = match (parent, name.as_ref()) {
        (None, b"userShapes")
        | (Some(b"userShapes"), b"relSizeAnchor" | b"absSizeAnchor")
        | (Some(b"relSizeAnchor" | b"absSizeAnchor"), b"from" | b"to")
        | (Some(b"from" | b"to"), b"x" | b"y")
        | (Some(b"sp"), b"nvSpPr" | b"spPr" | b"style" | b"txBody")
        | (Some(b"nvSpPr"), b"nvPr")
        | (Some(b"spPr"), b"xfrm" | b"noFill" | b"solidFill" | b"effectLst")
        | (Some(b"prstGeom"), b"avLst")
        | (Some(b"txBody"), b"lstStyle" | b"p")
        | (Some(b"p"), b"r")
        | (Some(b"r"), b"t")
        | (Some(b"rPr" | b"defRPr"), b"solidFill") => empty(),
        (Some(b"relSizeAnchor" | b"absSizeAnchor"), b"sp") => {
            attributes.iter().all(|(key, value)| {
                matches!(key.as_slice(), b"macro" | b"textlink") && value.is_empty()
            })
        }
        (Some(b"relSizeAnchor" | b"absSizeAnchor" | b"xfrm"), b"ext") => {
            attributes.len() == 2 && finite(b"cx", true) && finite(b"cy", true)
        }
        (Some(b"nvSpPr"), b"cNvPr") => {
            attributes.len() == 2
                && attributes
                    .get(b"id".as_slice())
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|value| value > 0)
                && attributes
                    .get(b"name".as_slice())
                    .is_some_and(|value| !value.trim().is_empty())
        }
        (Some(b"nvSpPr"), b"cNvSpPr") => {
            empty() || exact(b"txBox").is_some_and(|value| true_value(Some(value.to_string())))
        }
        (Some(b"xfrm"), b"off") => {
            attributes.len() == 2 && finite(b"x", false) && finite(b"y", false)
        }
        (Some(b"spPr"), b"prstGeom") => exact(b"prst") == Some("rect"),
        (Some(b"spPr"), b"ln") => chart_line_attributes_supported(reader, element, false),
        (Some(b"ln"), b"noFill" | b"solidFill" | b"round") => empty(),
        (Some(b"ln"), b"prstDash") => exact(b"val") == Some("solid"),
        (Some(b"style"), b"lnRef" | b"fillRef" | b"effectRef") => exact(b"idx") == Some("0"),
        (Some(b"style"), b"fontRef") => exact(b"idx") == Some("minor"),
        (Some(b"lnRef" | b"fillRef" | b"effectRef"), b"scrgbClr") => {
            attributes.len() == 3
                && [b"r".as_slice(), b"g".as_slice(), b"b".as_slice()]
                    .iter()
                    .all(|key| attributes.get(*key).is_some_and(|value| value == "0"))
        }
        (Some(b"fontRef"), b"schemeClr") => exact(b"val") == Some("tx1"),
        (Some(b"txBody"), b"bodyPr") => {
            attributes.iter().all(|(key, value)| match key.as_slice() {
                b"anchor" => value == "t",
                b"rtlCol" => false_value(Some(value.clone())),
                b"wrap" => matches!(value.as_str(), "none" | "square"),
                b"lIns" | b"rIns" | b"tIns" | b"bIns" => value
                    .parse::<f64>()
                    .is_ok_and(|value| value.is_finite() && value >= 0.0),
                _ => false,
            })
        }
        (Some(b"bodyPr"), b"spAutoFit") => empty(),
        (Some(b"lstStyle"), level)
            if matches!(
                level,
                b"lvl1pPr"
                    | b"lvl2pPr"
                    | b"lvl3pPr"
                    | b"lvl4pPr"
                    | b"lvl5pPr"
                    | b"lvl6pPr"
                    | b"lvl7pPr"
                    | b"lvl8pPr"
                    | b"lvl9pPr"
            ) =>
        {
            let number = usize::from(level[3] - b'1');
            attributes.len() == 2
                && attributes
                    .get(b"indent".as_slice())
                    .is_some_and(|value| value == "0")
                && attributes
                    .get(b"marL".as_slice())
                    .is_some_and(|value| value == &(number * 457_200).to_string())
        }
        (Some(level), b"defRPr") if level.starts_with(b"lvl") => exact(b"sz") == Some("1100"),
        (Some(b"p"), b"pPr") => {
            exact(b"algn").is_some_and(|value| matches!(value, "l" | "ctr" | "r" | "just"))
        }
        (Some(b"r"), b"rPr") => attributes.iter().all(|(key, value)| match key.as_slice() {
            b"sz" => value
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value > 0.0),
            b"b" | b"i" => matches!(value.as_str(), "0" | "1" | "false" | "true" | "off" | "on"),
            b"lang" | b"altLang" => !value.trim().is_empty(),
            b"dirty" | b"smtClean" => {
                matches!(value.as_str(), "0" | "1" | "false" | "true" | "off" | "on")
            }
            _ => false,
        }),
        (Some(b"rPr"), b"latin") => {
            exact(b"typeface").is_some_and(|value| !value.trim().is_empty())
        }
        (Some(b"defRPr"), b"latin") => exact(b"typeface") == Some("+mn-lt"),
        (Some(b"defRPr"), b"ea") => exact(b"typeface") == Some("+mn-ea"),
        (Some(b"defRPr"), b"cs") => exact(b"typeface") == Some("+mn-cs"),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(chart_user_shape_formatting(sheet_name))
    }
}

fn chart_user_shapes_rid(xml: &str) -> Result<Option<String>, ConvertError> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref element) | Event::Empty(ref element))
                if element.local_name().as_ref() == b"userShapes" =>
            {
                return Ok(attr_value(&reader, element, b"id"));
            }
            Ok(Event::Eof) => return Ok(None),
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse XLSX chart: {error}"
                )));
            }
            _ => {}
        }
    }
}

fn chart_detail(sheet_name: &str, detail: &str) -> ConvertError {
    unsupported(format!(
        "unsupported chart detail {detail} on sheet: {sheet_name}"
    ))
}

fn chart_plot(sheet_name: &str, detail: &str) -> ConvertError {
    unsupported(format!(
        "unsupported chart plot {detail} on sheet: {sheet_name}"
    ))
}

fn chart_percent(value: Option<String>, low: f64, high: f64) -> bool {
    value
        .as_deref()
        .map(str::trim)
        .map(|value| value.strip_suffix('%').unwrap_or(value).trim())
        .and_then(|value| value.parse::<f64>().ok())
        .is_some_and(|value| value.is_finite() && (low..=high).contains(&value))
}

fn chart_element_is_known(name: &[u8]) -> bool {
    matches!(
        name,
        b"AlternateContent"
            | b"Choice"
            | b"Fallback"
            | b"alpha"
            | b"area3DChart"
            | b"areaChart"
            | b"auto"
            | b"autoTitleDeleted"
            | b"axId"
            | b"axPos"
            | b"bar3DChart"
            | b"barChart"
            | b"barDir"
            | b"bodyPr"
            | b"bubble3D"
            | b"bubbleChart"
            | b"cat"
            | b"catAx"
            | b"chart"
            | b"chartSpace"
            | b"crossAx"
            | b"crossBetween"
            | b"crosses"
            | b"crossesAt"
            | b"cs"
            | b"dLbl"
            | b"dLblPos"
            | b"dLbls"
            | b"dPt"
            | b"date1904"
            | b"defRPr"
            | b"delete"
            | b"dispBlanksAs"
            | b"dispUnits"
            | b"doughnutChart"
            | b"dropLines"
            | b"ea"
            | b"effectLst"
            | b"endParaRPr"
            | b"errBars"
            | b"explosion"
            | b"ext"
            | b"extLst"
            | b"f"
            | b"firstSliceAng"
            | b"formatCode"
            | b"gapWidth"
            | b"glow"
            | b"gradFill"
            | b"grouping"
            | b"gs"
            | b"gsLst"
            | b"h"
            | b"headerFooter"
            | b"hiLowLines"
            | b"holeSize"
            | b"hueOff"
            | b"idx"
            | b"invertIfNegative"
            | b"lang"
            | b"latin"
            | b"layout"
            | b"layoutTarget"
            | b"lblAlgn"
            | b"lblOffset"
            | b"leaderLines"
            | b"legend"
            | b"legendEntry"
            | b"legendPos"
            | b"lin"
            | b"line3DChart"
            | b"lineChart"
            | b"ln"
            | b"logBase"
            | b"lstStyle"
            | b"lumMod"
            | b"lumOff"
            | b"majorGridlines"
            | b"majorTickMark"
            | b"majorUnit"
            | b"manualLayout"
            | b"marker"
            | b"max"
            | b"min"
            | b"minorGridlines"
            | b"minorTickMark"
            | b"minorUnit"
            | b"noFill"
            | b"noMultiLvlLbl"
            | b"numCache"
            | b"numFmt"
            | b"numLit"
            | b"numRef"
            | b"ofPieChart"
            | b"ofPieType"
            | b"order"
            | b"orientation"
            | b"overlap"
            | b"overlay"
            | b"p"
            | b"pPr"
            | b"pageMargins"
            | b"pageSetup"
            | b"pie3DChart"
            | b"pieChart"
            | b"plotArea"
            | b"plotVisOnly"
            | b"printSettings"
            | b"prstDash"
            | b"pt"
            | b"ptCount"
            | b"r"
            | b"rPr"
            | b"radarChart"
            | b"radarStyle"
            | b"rich"
            | b"round"
            | b"roundedCorners"
            | b"satMod"
            | b"satOff"
            | b"scaling"
            | b"scatterChart"
            | b"scatterStyle"
            | b"schemeClr"
            | b"separator"
            | b"ser"
            | b"shade"
            | b"showBubbleSize"
            | b"showCatName"
            | b"showDLblsOverMax"
            | b"showLeaderLines"
            | b"showLegendKey"
            | b"showPercent"
            | b"showSerName"
            | b"showVal"
            | b"size"
            | b"smooth"
            | b"solidFill"
            | b"spPr"
            | b"srgbClr"
            | b"stockChart"
            | b"strCache"
            | b"strLit"
            | b"strRef"
            | b"style"
            | b"surface3DChart"
            | b"surfaceChart"
            | b"symbol"
            | b"sysClr"
            | b"t"
            | b"tickLblPos"
            | b"tint"
            | b"title"
            | b"trendline"
            | b"tx"
            | b"txPr"
            | b"uFillTx"
            | b"uniqueId"
            | b"upDownBars"
            | b"userShapes"
            | b"v"
            | b"val"
            | b"valAx"
            | b"varyColors"
            | b"w"
            | b"x"
            | b"xMode"
            | b"xVal"
            | b"y"
            | b"yMode"
            | b"yVal"
    )
}

fn chart_child_is_singleton(parent: &[u8], child: &[u8]) -> bool {
    match parent {
        b"chartSpace" => matches!(
            child,
            b"date1904"
                | b"lang"
                | b"roundedCorners"
                | b"style"
                | b"chart"
                | b"spPr"
                | b"printSettings"
                | b"userShapes"
                | b"extLst"
        ),
        b"chart" => matches!(
            child,
            b"title"
                | b"autoTitleDeleted"
                | b"plotArea"
                | b"legend"
                | b"plotVisOnly"
                | b"dispBlanksAs"
                | b"showDLblsOverMax"
        ),
        b"plotArea" => matches!(child, b"layout" | b"spPr"),
        b"barChart" => matches!(
            child,
            b"barDir" | b"grouping" | b"varyColors" | b"dLbls" | b"gapWidth" | b"overlap"
        ),
        b"lineChart" | b"areaChart" => matches!(
            child,
            b"grouping" | b"varyColors" | b"dLbls" | b"dropLines" | b"marker" | b"smooth"
        ),
        b"pieChart" | b"doughnutChart" => matches!(
            child,
            b"varyColors" | b"dLbls" | b"firstSliceAng" | b"holeSize"
        ),
        b"radarChart" => matches!(child, b"radarStyle" | b"varyColors" | b"dLbls"),
        b"scatterChart" => matches!(child, b"scatterStyle" | b"varyColors" | b"dLbls"),
        b"ser" => matches!(
            child,
            b"idx"
                | b"order"
                | b"tx"
                | b"spPr"
                | b"invertIfNegative"
                | b"marker"
                | b"dLbls"
                | b"cat"
                | b"val"
                | b"xVal"
                | b"yVal"
                | b"smooth"
                | b"explosion"
                | b"extLst"
        ),
        b"dPt" => matches!(
            child,
            b"idx"
                | b"invertIfNegative"
                | b"bubble3D"
                | b"explosion"
                | b"marker"
                | b"spPr"
                | b"extLst"
        ),
        b"dLbls" => matches!(
            child,
            b"numFmt"
                | b"spPr"
                | b"txPr"
                | b"dLblPos"
                | b"showLegendKey"
                | b"showVal"
                | b"showCatName"
                | b"showSerName"
                | b"showPercent"
                | b"showBubbleSize"
                | b"separator"
                | b"showLeaderLines"
                | b"leaderLines"
                | b"extLst"
        ),
        b"catAx" | b"valAx" => matches!(
            child,
            b"axId"
                | b"scaling"
                | b"delete"
                | b"axPos"
                | b"majorGridlines"
                | b"minorGridlines"
                | b"title"
                | b"numFmt"
                | b"majorTickMark"
                | b"minorTickMark"
                | b"tickLblPos"
                | b"spPr"
                | b"txPr"
                | b"crossAx"
                | b"crosses"
                | b"crossBetween"
                | b"auto"
                | b"lblAlgn"
                | b"lblOffset"
                | b"noMultiLvlLbl"
                | b"majorUnit"
                | b"minorUnit"
                | b"dispUnits"
        ),
        b"scaling" => matches!(child, b"logBase" | b"orientation" | b"max" | b"min"),
        b"title" => matches!(child, b"tx" | b"layout" | b"overlay" | b"spPr" | b"txPr"),
        b"legend" => matches!(
            child,
            b"legendPos" | b"layout" | b"overlay" | b"spPr" | b"txPr"
        ),
        b"manualLayout" => matches!(
            child,
            b"layoutTarget" | b"xMode" | b"yMode" | b"x" | b"y" | b"w" | b"h"
        ),
        b"marker" => matches!(child, b"symbol" | b"size" | b"spPr"),
        b"numRef" | b"strRef" => matches!(child, b"f" | b"numCache" | b"strCache"),
        b"numCache" | b"strCache" | b"numLit" | b"strLit" => {
            matches!(child, b"formatCode" | b"ptCount")
        }
        b"printSettings" => matches!(child, b"headerFooter" | b"pageMargins" | b"pageSetup"),
        b"AlternateContent" => matches!(child, b"Choice" | b"Fallback"),
        _ => false,
    }
}

fn chart_element_parent_is_supported(parent: Option<&[u8]>, name: &[u8]) -> bool {
    match name {
        b"chartSpace" => parent.is_none(),
        b"chart" => parent == Some(b"chartSpace"),
        b"plotArea" => parent == Some(b"chart"),
        b"barChart" | b"lineChart" | b"areaChart" | b"pieChart" | b"doughnutChart"
        | b"radarChart" | b"scatterChart" | b"ofPieChart" | b"stockChart" | b"surfaceChart"
        | b"bar3DChart" | b"line3DChart" | b"pie3DChart" | b"area3DChart" | b"surface3DChart"
        | b"bubbleChart" => parent == Some(b"plotArea"),
        b"catAx" | b"valAx" => parent == Some(b"plotArea"),
        b"ser" => parent.is_some_and(|parent| parent.ends_with(b"Chart")),
        b"dPt" => parent == Some(b"ser"),
        b"dLbls" => {
            parent == Some(b"ser") || parent.is_some_and(|parent| parent.ends_with(b"Chart"))
        }
        b"legend" => parent == Some(b"chart"),
        b"title" => matches!(parent, Some(b"chart" | b"catAx" | b"valAx")),
        b"layout" => matches!(
            parent,
            Some(b"plotArea" | b"title" | b"legend" | b"dLbls" | b"dLbl")
        ),
        b"manualLayout" => parent == Some(b"layout"),
        b"scaling" | b"majorGridlines" | b"minorGridlines" => {
            matches!(parent, Some(b"catAx" | b"valAx"))
        }
        b"barDir" => matches!(parent, Some(b"barChart" | b"bar3DChart")),
        b"grouping" => matches!(
            parent,
            Some(
                b"barChart"
                    | b"bar3DChart"
                    | b"lineChart"
                    | b"line3DChart"
                    | b"areaChart"
                    | b"area3DChart"
            )
        ),
        b"gapWidth" | b"overlap" => matches!(parent, Some(b"barChart" | b"bar3DChart")),
        b"holeSize" => parent == Some(b"doughnutChart"),
        b"firstSliceAng" => matches!(parent, Some(b"pieChart" | b"pie3DChart" | b"doughnutChart")),
        b"radarStyle" => parent == Some(b"radarChart"),
        b"scatterStyle" => parent == Some(b"scatterChart"),
        b"ofPieType" => parent == Some(b"ofPieChart"),
        b"varyColors" => parent.is_some_and(|parent| parent.ends_with(b"Chart")),
        b"marker" => matches!(parent, Some(b"lineChart" | b"ser" | b"dPt")),
        b"symbol" | b"size" => parent == Some(b"marker"),
        b"smooth" => matches!(parent, Some(b"lineChart" | b"scatterChart" | b"ser")),
        b"bubble3D" | b"invertIfNegative" | b"explosion" => {
            matches!(parent, Some(b"ser" | b"dPt"))
        }
        b"autoTitleDeleted" | b"plotVisOnly" | b"dispBlanksAs" | b"showDLblsOverMax" => {
            parent == Some(b"chart")
        }
        b"legendPos" => parent == Some(b"legend"),
        b"overlay" => matches!(parent, Some(b"title" | b"legend")),
        b"showLegendKey" | b"showVal" | b"showCatName" | b"showSerName" | b"showPercent"
        | b"showBubbleSize" | b"dLblPos" | b"separator" | b"leaderLines" => {
            matches!(parent, Some(b"dLbls" | b"dLbl"))
        }
        b"showLeaderLines" => matches!(parent, Some(b"dLbls" | b"dLbl" | b"ext")),
        b"numFmt" => matches!(parent, Some(b"catAx" | b"valAx" | b"dLbls" | b"dLbl")),
        b"orientation" | b"logBase" | b"min" | b"max" => parent == Some(b"scaling"),
        b"majorUnit" | b"minorUnit" | b"crossesAt" => {
            matches!(parent, Some(b"catAx" | b"valAx"))
        }
        b"dispUnits" => parent == Some(b"valAx"),
        b"crossBetween" => parent == Some(b"valAx"),
        b"auto" | b"lblAlgn" | b"lblOffset" | b"noMultiLvlLbl" => parent == Some(b"catAx"),
        b"axPos" | b"majorTickMark" | b"minorTickMark" | b"tickLblPos" | b"crosses"
        | b"crossAx" => matches!(parent, Some(b"catAx" | b"valAx")),
        b"axId" => {
            matches!(parent, Some(b"catAx" | b"valAx"))
                || parent.is_some_and(|parent| parent.ends_with(b"Chart"))
        }
        b"date1904" | b"lang" | b"roundedCorners" => parent == Some(b"chartSpace"),
        b"style" => matches!(parent, Some(b"chartSpace" | b"Choice" | b"Fallback")),
        b"AlternateContent" => parent == Some(b"chartSpace"),
        b"Choice" | b"Fallback" => parent == Some(b"AlternateContent"),
        b"ptCount" | b"formatCode" => {
            matches!(
                parent,
                Some(b"numCache" | b"strCache" | b"numLit" | b"strLit")
            )
        }
        b"pt" => matches!(
            parent,
            Some(b"numCache" | b"strCache" | b"numLit" | b"strLit")
        ),
        b"f" => matches!(parent, Some(b"numRef" | b"strRef")),
        b"numCache" => parent == Some(b"numRef"),
        b"strCache" => parent == Some(b"strRef"),
        b"numRef" | b"numLit" => matches!(parent, Some(b"val" | b"yVal" | b"xVal" | b"cat")),
        b"strRef" | b"strLit" => matches!(parent, Some(b"tx" | b"cat" | b"xVal")),
        b"cat" | b"val" | b"xVal" | b"yVal" => parent == Some(b"ser"),
        _ => true,
    }
}

fn validate_chart_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    sheet_name: &str,
) -> Result<(), ConvertError> {
    let name = element.local_name();
    if !chart_element_is_known(name.as_ref()) {
        return Err(chart_detail(
            sheet_name,
            &format!("unknown element {}", String::from_utf8_lossy(name.as_ref())),
        ));
    }
    let Some(attributes) = chart_drawing_attributes(reader, element) else {
        return Err(chart_detail(sheet_name, "element attributes"));
    };
    let value = || {
        if attributes.len() == 1 {
            attributes.get(b"val".as_slice()).cloned()
        } else {
            None
        }
    };
    let empty_or = |predicate: &dyn Fn(Option<String>) -> bool| {
        attributes.is_empty() || (attributes.len() == 1 && predicate(value()))
    };
    let rejected = match name.as_ref() {
        b"chart" | b"plotArea" | b"barChart" | b"lineChart" | b"areaChart" | b"pieChart"
        | b"doughnutChart" | b"radarChart" | b"scatterChart" | b"ofPieChart" | b"stockChart"
        | b"surfaceChart" | b"bar3DChart" | b"line3DChart" | b"pie3DChart" | b"area3DChart"
        | b"surface3DChart" | b"bubbleChart" | b"ser" | b"dPt" | b"dLbls" | b"catAx" | b"valAx"
        | b"scaling" | b"title" | b"legend" | b"layout" | b"manualLayout" | b"cat" | b"val"
        | b"xVal" | b"yVal" | b"tx" | b"numRef" | b"strRef" | b"numCache" | b"strCache"
        | b"numLit" | b"strLit" | b"majorGridlines" | b"printSettings" | b"AlternateContent"
        | b"Fallback" => !attributes.is_empty(),
        b"Choice" => {
            attributes.len() != 1
                || attributes.get(b"Requires".as_slice()).map(String::as_str) != Some("c14")
        }
        b"dLbl" | b"dispUnits" | b"dropLines" | b"errBars" | b"hiLowLines" | b"leaderLines"
        | b"logBase" | b"minorGridlines" | b"minorUnit" | b"trendline" | b"upDownBars"
        | b"crossesAt" => true,
        b"orientation" => !empty_or(&|value| value.as_deref() == Some("minMax")),
        b"majorTickMark" => !matches!(value().as_deref(), Some("none" | "in" | "out" | "cross")),
        b"minorTickMark" => !matches!(value().as_deref(), Some("none")),
        b"autoTitleDeleted" | b"delete" | b"showVal" | b"showCatName" | b"showSerName"
        | b"showPercent" => !empty_or(&|value| {
            matches!(
                value.as_deref(),
                Some("0" | "1" | "false" | "true" | "off" | "on")
            )
        }),
        b"numFmt" => {
            let format = attributes
                .get(b"formatCode".as_slice())
                .is_some_and(|value| !value.trim().is_empty());
            let linked = attributes
                .get(b"sourceLinked".as_slice())
                .is_none_or(|value| {
                    matches!(value.as_str(), "0" | "1" | "false" | "true" | "off" | "on")
                });
            !format || !linked || attributes.len() > 2
        }
        b"smooth" | b"bubble3D" | b"invertIfNegative" | b"roundedCorners" | b"showDLblsOverMax"
        | b"showLeaderLines" | b"showLegendKey" | b"showBubbleSize" | b"noMultiLvlLbl" => {
            attributes.len() != 1 || !false_value(value())
        }
        b"auto" => !empty_or(&|value| true_value(value)),
        b"barDir" => !matches!(value().as_deref(), Some("bar" | "col")),
        b"grouping" => !matches!(
            value().as_deref(),
            Some("standard" | "clustered" | "stacked" | "percentStacked")
        ),
        b"gapWidth" => !chart_percent(value(), 0.0, 500.0),
        b"overlap" => !chart_percent(value(), -100.0, 100.0),
        b"holeSize" => !chart_percent(value(), 10.0, 90.0),
        b"min" | b"max" => value()
            .and_then(|value| value.parse::<f64>().ok())
            .is_none_or(|value| !value.is_finite()),
        b"majorUnit" => value()
            .and_then(|value| value.parse::<f64>().ok())
            .is_none_or(|value| !value.is_finite() || value <= 0.0),
        b"lblAlgn" => value().as_deref() != Some("ctr"),
        b"lblOffset" => value().as_deref() != Some("100"),
        b"explosion" | b"firstSliceAng" => !empty_or(&|value| value.as_deref() == Some("0")),
        b"dLblPos" => !matches!(
            value().as_deref(),
            Some("ctr" | "outEnd" | "inEnd" | "inBase")
        ),
        b"axPos" => !matches!(value().as_deref(), Some("b" | "l")),
        b"tickLblPos" => !matches!(value().as_deref(), Some("nextTo" | "low")),
        b"crosses" => !empty_or(&|value| value.as_deref() == Some("autoZero")),
        b"crossBetween" => !matches!(value().as_deref(), Some("between" | "midCat")),
        b"dispBlanksAs" => !empty_or(&|value| value.as_deref() == Some("gap")),
        b"date1904" => !empty_or(&|value| false_value(value)),
        b"overlay" => !empty_or(&|value| false_value(value)),
        b"legendPos" => !matches!(value().as_deref(), Some("b" | "l" | "r" | "t" | "tr")),
        _ => false,
    };
    if rejected {
        let detail = match name.as_ref() {
            b"legendPos" => std::borrow::Cow::Borrowed("legend position"),
            _ => String::from_utf8_lossy(name.as_ref()),
        };
        return Err(chart_detail(sheet_name, &detail));
    }
    Ok(())
}

fn chart_text_attributes_supported(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
) -> Result<(), &'static str> {
    let name = element.local_name();
    let in_text = ancestors
        .iter()
        .any(|ancestor| matches!(ancestor.as_slice(), b"txPr" | b"rich"));
    if !in_text {
        return Ok(());
    }

    let parent = ancestors.last().map(Vec::as_slice);
    if !matches!(name.as_ref(), b"bodyPr" | b"defRPr" | b"rPr") {
        let supported = match name.as_ref() {
            b"lstStyle" | b"p" | b"r" | b"solidFill" => element.attributes().all(|attribute| {
                attribute
                    .is_ok_and(|attribute| chart_namespace_attribute_supported(reader, &attribute))
            }),
            b"pPr" => element.attributes().all(|attribute| {
                attribute.is_ok_and(|attribute| {
                    chart_namespace_attribute_supported(reader, &attribute)
                        || (attribute.key.local_name().as_ref() == b"rtl"
                            && attribute
                                .decode_and_unescape_value(reader.decoder())
                                .is_ok_and(|value| matches!(value.as_ref(), "0" | "false" | "off")))
                })
            }),
            b"t" => element.attributes().all(|attribute| {
                attribute.is_ok_and(|attribute| {
                    chart_namespace_attribute_supported(reader, &attribute)
                        || (attribute.key.local_name().as_ref() == b"space"
                            && attribute
                                .decode_and_unescape_value(reader.decoder())
                                .is_ok_and(|value| value == "preserve"))
                })
            }),
            b"latin" | b"ea" | b"cs" => {
                let mut typeface = None;
                for attribute in element.attributes() {
                    let Ok(attribute) = attribute else {
                        return Err("text formatting");
                    };
                    if chart_namespace_attribute_supported(reader, &attribute) {
                        continue;
                    }
                    if attribute.key.local_name().as_ref() != b"typeface" || typeface.is_some() {
                        return Err("text formatting");
                    }
                    typeface = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .ok()
                        .filter(|value| !value.trim().is_empty());
                }
                match (name.as_ref(), typeface.as_deref()) {
                    (b"latin", Some(_)) => true,
                    // A chart's standard minor East Asian and complex-script
                    // slots are covered by the renderer's mixed-script
                    // fallback chain. Literal and major-slot faces need their
                    // own modeled font chain, so they remain fail-closed.
                    (b"ea", Some("+mn-ea")) | (b"cs", Some("+mn-cs")) => true,
                    _ => false,
                }
            }
            b"endParaRPr" => element.attributes().all(|attribute| {
                attribute.is_ok_and(|attribute| {
                    chart_namespace_attribute_supported(reader, &attribute)
                        || matches!(
                            attribute.key.local_name().as_ref(),
                            b"lang" | b"altLang" | b"dirty" | b"smtClean"
                        )
                })
            }),
            b"srgbClr" | b"schemeClr" | b"sysClr" | b"alpha" | b"tint" | b"shade" | b"hueOff"
            | b"satMod" | b"satOff" | b"lumMod" | b"lumOff" => {
                chart_color_element_supported(reader, element, parent)
            }
            _ => false,
        };
        return if supported {
            Ok(())
        } else {
            Err("text formatting")
        };
    }

    let axis_labels = ancestors
        .iter()
        .any(|ancestor| matches!(ancestor.as_slice(), b"catAx" | b"valAx"))
        && ancestors.iter().any(|ancestor| ancestor == b"txPr")
        && !ancestors.iter().any(|ancestor| ancestor == b"title");
    let value_axis_title = ancestors.iter().any(|ancestor| ancestor == b"valAx")
        && ancestors.iter().any(|ancestor| ancestor == b"title")
        && ancestors.iter().any(|ancestor| ancestor == b"rich");

    for attribute in element.attributes() {
        let Ok(attribute) = attribute else {
            return Err("text formatting");
        };
        if chart_namespace_attribute_supported(reader, &attribute) {
            continue;
        }
        let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) else {
            return Err("text formatting");
        };
        let value = value.as_ref();
        let detail = if name.as_ref() == b"bodyPr" && attribute.key.local_name().as_ref() == b"rot"
        {
            "text orientation"
        } else if name.as_ref() == b"bodyPr" {
            "text layout"
        } else {
            "text formatting"
        };
        let supported = if name.as_ref() == b"bodyPr" {
            match attribute.key.local_name().as_ref() {
                b"rot" => {
                    value == "0"
                        || (value == "-60000000" && axis_labels)
                        || (value == "-5400000" && value_axis_title)
                }
                b"spcFirstLastPara" | b"anchorCtr" => {
                    matches!(value, "1" | "true" | "on")
                }
                b"vertOverflow" => value == "ellipsis",
                b"vert" => value == "horz",
                b"wrap" => value == "square",
                b"anchor" => value == "ctr",
                _ => return Err("text layout"),
            }
        } else {
            match attribute.key.local_name().as_ref() {
                b"sz" => value
                    .parse::<f64>()
                    .is_ok_and(|size| size.is_finite() && size > 0.0),
                b"b" => matches!(value, "0" | "1" | "false" | "true" | "off" | "on"),
                b"spc" => value.parse::<i32>().is_ok(),
                b"i" => matches!(value, "0" | "false" | "off"),
                b"u" => value == "none",
                b"strike" => value == "noStrike",
                b"baseline" => value == "0",
                b"kern" => value.parse::<u32>().is_ok(),
                b"lang" | b"altLang" => !value.trim().is_empty(),
                b"dirty" | b"smtClean" => {
                    matches!(value, "0" | "1" | "false" | "true" | "off" | "on")
                }
                _ => false,
            }
        };
        if !supported {
            return Err(detail);
        }
    }
    Ok(())
}

fn validate_chart_text_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    chart_text_attributes_supported(reader, element, ancestors)
        .map_err(|detail| chart_detail(sheet_name, detail))
}

fn validate_chart_extension_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    if name.as_ref() == b"extLst" && matches!(parent, Some(b"ser" | b"dPt" | b"dLbls")) {
        return if chart_drawing_attributes(reader, element).is_some_and(|attrs| attrs.is_empty()) {
            Ok(())
        } else {
            Err(chart_detail(sheet_name, "chart extension"))
        };
    }
    let Some(extension_index) = ancestors.iter().rposition(|ancestor| ancestor == b"extLst") else {
        return Ok(());
    };
    let owner = extension_index
        .checked_sub(1)
        .map(|index| ancestors[index].as_slice());
    let exact = |key: &[u8]| chart_exact_attribute(reader, element, key);
    let supported = match (owner, parent, name.as_ref()) {
        (Some(b"ser" | b"dPt"), Some(b"extLst"), b"ext") => {
            exact(b"uri").as_deref() == Some("{C3380CC4-5D6E-409C-BE32-E72D297353CC}")
        }
        (Some(b"ser" | b"dPt"), Some(b"ext"), b"uniqueId") => {
            exact(b"val").is_some_and(|value| !value.trim().is_empty())
        }
        (Some(b"dLbls"), Some(b"extLst"), b"ext") => {
            exact(b"uri").as_deref() == Some("{CE6537A1-D6FC-4f65-9D91-7224C49458BB}")
        }
        (Some(b"dLbls"), Some(b"ext"), b"showLeaderLines") => {
            exact(b"val").is_some_and(|value| false_value(Some(value)))
        }
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        Err(chart_detail(sheet_name, "chart extension"))
    }
}

fn chart_namespace_attribute_supported(reader: &Reader<&[u8]>, attribute: &Attribute<'_>) -> bool {
    let key = attribute.key.as_ref();
    if key != b"xmlns" && !key.starts_with(b"xmlns:") {
        return false;
    }
    attribute
        .decode_and_unescape_value(reader.decoder())
        .is_ok_and(|value| {
            matches!(
                value.as_ref(),
                "http://schemas.openxmlformats.org/drawingml/2006/main"
                    | "http://purl.oclc.org/ooxml/drawingml/main"
                    | "http://schemas.openxmlformats.org/drawingml/2006/chart"
                    | "http://purl.oclc.org/ooxml/drawingml/chart"
                    | "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing"
                    | "http://purl.oclc.org/ooxml/drawingml/chartDrawing"
                    | "http://schemas.openxmlformats.org/markup-compatibility/2006"
                    | "http://schemas.microsoft.com/office/drawing/2007/8/2/chart"
                    | "http://schemas.microsoft.com/office/drawing/2012/chart"
                    | "http://schemas.microsoft.com/office/drawing/2014/chart"
            )
        })
}

#[derive(Default)]
struct ManualLayoutPreflight {
    title: bool,
    layout_target: bool,
    x_mode: bool,
    y_mode: bool,
    x: bool,
    y: bool,
    width: bool,
    height: bool,
}

fn chart_line_attributes_supported(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    invisible: bool,
) -> bool {
    let mut width = None;
    let mut saw_cap = false;
    let mut saw_compound = false;
    let mut saw_alignment = false;
    for attribute in element.attributes() {
        let Ok(attribute) = attribute else {
            return false;
        };
        let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) else {
            return false;
        };
        match attribute.key.local_name().as_ref() {
            b"w" if width.is_none() => {
                width = value.parse::<f64>().ok();
                if width.is_none() {
                    return false;
                }
            }
            b"cap" if !saw_cap && value == "flat" => saw_cap = true,
            b"cmpd" if !saw_compound && value == "sng" => saw_compound = true,
            b"algn" if !saw_alignment && value == "ctr" => saw_alignment = true,
            _ => return false,
        }
    }
    width.is_none_or(|value| value.is_finite() && value >= 0.0 && (!invisible || value == 0.0))
}

fn chart_color_element_supported(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    parent: Option<&[u8]>,
) -> bool {
    let exact_value = |key: &[u8]| {
        let mut found = None;
        for attribute in element.attributes() {
            let attribute = attribute.ok()?;
            if chart_namespace_attribute_supported(reader, &attribute) {
                continue;
            }
            if found.is_some() || attribute.key.local_name().as_ref() != key {
                return None;
            }
            found = attribute
                .decode_and_unescape_value(reader.decoder())
                .ok()
                .map(|value| value.into_owned());
        }
        found
    };
    let exact_integer = |key: &[u8]| exact_value(key).and_then(|value| value.parse::<i64>().ok());
    let name = element.local_name();
    match (parent, name.as_ref()) {
        (Some(b"solidFill"), b"srgbClr") => exact_value(b"val").is_some_and(|value| {
            value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        }),
        (Some(b"solidFill"), b"schemeClr") => matches!(
            exact_value(b"val").as_deref(),
            Some(
                "dk1"
                    | "lt1"
                    | "dk2"
                    | "lt2"
                    | "bg1"
                    | "tx1"
                    | "bg2"
                    | "tx2"
                    | "accent1"
                    | "accent2"
                    | "accent3"
                    | "accent4"
                    | "accent5"
                    | "accent6"
                    | "hlink"
                    | "folHlink"
            )
        ),
        (Some(b"solidFill"), b"sysClr") => {
            let mut saw_val = false;
            let mut last = None;
            for attribute in element.attributes() {
                let Ok(attribute) = attribute else {
                    return false;
                };
                if chart_namespace_attribute_supported(reader, &attribute) {
                    continue;
                }
                let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) else {
                    return false;
                };
                match attribute.key.local_name().as_ref() {
                    b"val" if !saw_val && !value.trim().is_empty() => saw_val = true,
                    b"lastClr" if last.is_none() => last = Some(value.into_owned()),
                    _ => return false,
                }
            }
            saw_val
                && last.is_some_and(|value| {
                    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        }
        (Some(b"srgbClr" | b"schemeClr" | b"sysClr"), b"alpha") => {
            exact_integer(b"val") == Some(100_000)
        }
        (Some(b"srgbClr" | b"schemeClr" | b"sysClr"), b"tint" | b"shade") => {
            exact_integer(b"val").is_some_and(|value| (0..=100_000).contains(&value))
        }
        (Some(b"srgbClr" | b"schemeClr" | b"sysClr"), b"satMod" | b"lumMod") => {
            exact_integer(b"val").is_some_and(|value| value >= 0)
        }
        (Some(b"srgbClr" | b"schemeClr" | b"sysClr"), b"satOff" | b"lumOff") => {
            exact_integer(b"val").is_some_and(|value| (-100_000..=100_000).contains(&value))
        }
        (Some(b"srgbClr" | b"schemeClr" | b"sysClr"), b"hueOff") => {
            exact_integer(b"val").is_some_and(|value| (-21_600_000..=21_600_000).contains(&value))
        }
        _ => false,
    }
}

fn validate_unrendered_chart_shape_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
) -> bool {
    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    if ancestors.iter().any(|name| name == b"effectLst") {
        return false;
    }
    match (parent, name.as_ref()) {
        (_, b"spPr") => true,
        (Some(b"spPr"), b"noFill" | b"effectLst") => true,
        (Some(b"spPr"), b"ln") => chart_line_attributes_supported(reader, element, true),
        (Some(b"ln"), b"noFill" | b"round") => true,
        _ => false,
    }
}

fn validate_rendered_chart_shape_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    axis_line: bool,
) -> bool {
    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    if ancestors.iter().any(|name| name == b"effectLst") {
        return false;
    }
    if chart_color_element_supported(reader, element, parent) {
        return true;
    }
    match (parent, name.as_ref()) {
        (_, b"spPr") => true,
        (Some(b"spPr"), b"noFill" | b"effectLst") => true,
        (Some(b"spPr"), b"solidFill") if !axis_line => true,
        (Some(b"spPr"), b"ln") => chart_line_attributes_supported(reader, element, false),
        (Some(b"ln"), b"noFill" | b"solidFill") => true,
        (Some(b"ln"), b"prstDash") => {
            chart_drawing_attributes(reader, element).is_some_and(|attributes| {
                attributes.len() == 1
                    && attributes.get(b"val".as_slice()).map(String::as_str) == Some("solid")
            })
        }
        (Some(b"ln"), b"round") => true,
        _ => false,
    }
}

fn validate_chart_shape_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    let name = element.local_name();
    let owner = if name.as_ref() == b"spPr" {
        ancestors.last().map(Vec::as_slice)
    } else {
        ancestors
            .iter()
            .rposition(|ancestor| ancestor == b"spPr")
            .and_then(|index| index.checked_sub(1))
            .map(|index| ancestors[index].as_slice())
    };
    let Some(owner) = owner else {
        return Ok(());
    };
    let (supported, detail) = match owner {
        b"chartSpace" => (
            validate_rendered_chart_shape_element(reader, element, ancestors, false),
            "chart area formatting",
        ),
        b"catAx" | b"valAx" | b"majorGridlines" => (
            validate_rendered_chart_shape_element(reader, element, ancestors, true),
            "axis formatting",
        ),
        b"plotArea" => (
            validate_unrendered_chart_shape_element(reader, element, ancestors),
            "plot area formatting",
        ),
        b"dLbls" => (
            validate_unrendered_chart_shape_element(reader, element, ancestors),
            "data label formatting",
        ),
        b"legend" => (
            validate_unrendered_chart_shape_element(reader, element, ancestors),
            "legend formatting",
        ),
        b"title" => (
            validate_unrendered_chart_shape_element(reader, element, ancestors),
            "title formatting",
        ),
        _ => return Ok(()),
    };
    if supported {
        Ok(())
    } else {
        Err(chart_detail(sheet_name, detail))
    }
}

fn chart_shape_detail(ancestors: &[Vec<u8>]) -> &'static str {
    let owner = ancestors
        .iter()
        .rposition(|ancestor| ancestor == b"spPr")
        .and_then(|index| index.checked_sub(1))
        .map(|index| ancestors[index].as_slice());
    match owner {
        Some(b"chartSpace") => "chart area formatting",
        Some(b"catAx" | b"valAx" | b"majorGridlines") => "axis formatting",
        Some(b"dPt") => "data point formatting",
        Some(b"ser") => "series formatting",
        Some(b"plotArea") => "plot area formatting",
        Some(b"dLbls") => "data label formatting",
        Some(b"legend") => "legend formatting",
        Some(b"title") => "title formatting",
        _ => "shape formatting",
    }
}

fn validate_data_point_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    if !ancestors.iter().any(|name| name == b"dPt") {
        return Ok(());
    }
    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    let reject = || Err(chart_detail(sheet_name, "data point formatting"));

    if ancestors.iter().any(|ancestor| ancestor == b"effectLst") {
        return reject();
    }
    if ancestors.iter().any(|ancestor| ancestor == b"extLst") {
        return match name.as_ref() {
            b"extLst" => Ok(()),
            b"ext"
                if chart_drawing_attributes(reader, element).is_some_and(|attributes| {
                    attributes.len() == 1
                        && attributes.get(b"uri".as_slice()).map(String::as_str)
                            == Some("{C3380CC4-5D6E-409C-BE32-E72D297353CC}")
                }) =>
            {
                Ok(())
            }
            b"uniqueId"
                if chart_drawing_attributes(reader, element).is_some_and(|attributes| {
                    attributes.len() == 1
                        && attributes
                            .get(b"val".as_slice())
                            .is_some_and(|value| !value.is_empty())
                }) =>
            {
                Ok(())
            }
            _ => reject(),
        };
    }
    if ancestors.iter().any(|ancestor| ancestor == b"ln") {
        return if name.as_ref() == b"noFill" {
            Ok(())
        } else {
            reject()
        };
    }
    if chart_color_element_supported(reader, element, parent) {
        return Ok(());
    }

    match (parent, name.as_ref()) {
        (Some(b"dPt"), b"idx" | b"invertIfNegative" | b"bubble3D" | b"spPr" | b"extLst")
        | (Some(b"spPr"), b"solidFill" | b"ln" | b"effectLst") => Ok(()),
        _ => reject(),
    }
}

fn validate_series_shape_element(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    ancestors: &[Vec<u8>],
    sheet_name: &str,
) -> Result<(), ConvertError> {
    if ancestors
        .iter()
        .any(|name| matches!(name.as_slice(), b"dPt" | b"marker" | b"dLbls"))
    {
        return Ok(());
    }
    let Some(shape_index) = ancestors.iter().rposition(|name| name == b"spPr") else {
        return Ok(());
    };
    if shape_index == 0 || ancestors[shape_index - 1] != b"ser" {
        return Ok(());
    }

    let name = element.local_name();
    let parent = ancestors.last().map(Vec::as_slice);
    let line_series = ancestors.iter().any(|name| name == b"lineChart");
    let reject = || Err(chart_detail(sheet_name, "series formatting"));
    if ancestors.iter().any(|name| name == b"effectLst") {
        return reject();
    }
    if chart_color_element_supported(reader, element, parent) {
        return Ok(());
    }
    if ancestors.iter().any(|name| name == b"ln") {
        return match (parent, name.as_ref()) {
            (Some(b"ln"), b"noFill") if !line_series => Ok(()),
            (Some(b"ln"), b"solidFill") if line_series => Ok(()),
            (Some(b"ln"), b"prstDash")
                if line_series
                    && chart_drawing_attributes(reader, element).is_some_and(|attributes| {
                        attributes.len() == 1
                            && attributes.get(b"val".as_slice()).map(String::as_str)
                                == Some("solid")
                    }) =>
            {
                Ok(())
            }
            _ => reject(),
        };
    }

    match (parent, name.as_ref()) {
        (Some(b"spPr"), b"solidFill" | b"effectLst") => Ok(()),
        (Some(b"spPr"), b"ln") => {
            let width = attr_value(reader, element, b"w");
            let valid = if line_series {
                width.is_none_or(|value| {
                    value
                        .parse::<f64>()
                        .is_ok_and(|value| value.is_finite() && value > 0.0)
                })
            } else {
                width.as_deref().is_none_or(|value| value == "0")
            };
            if valid { Ok(()) } else { reject() }
        }
        _ => reject(),
    }
}

#[derive(Default)]
struct ChartPreflightScan {
    cat_axes: usize,
    val_axes: usize,
    cat_axis_positions: Vec<String>,
    val_axis_positions: Vec<String>,
    cross_between: Vec<String>,
    line_family_seen: bool,
    line_marker_enabled: Option<bool>,
    cache_expected: Option<usize>,
    cache_indices: Option<Vec<usize>>,
    cache_is_numeric: bool,
    cache_point_open: bool,
    cache_point_has_value: bool,
    in_cache_value: bool,
    cache_value: String,
    category_cache_seen: bool,
    bad_cache: bool,
    series_count: usize,
    current_series: Option<(usize, bool, bool)>,
    current_data_point_index: Option<Option<usize>>,
    series_data_point_indices: Vec<Vec<usize>>,
    bad_data_point: bool,
    plot_visible_only: Option<bool>,
    manual_layout: Option<ManualLayoutPreflight>,
    shape_fill: Option<(&'static str, usize)>,
    rich_text_open: bool,
    rich_runs: usize,
    rich_paragraphs: usize,
    current_family_axis_ids: Option<Vec<u64>>,
    family_axis_ids: Vec<Vec<u64>>,
    category_axis_ids: Vec<u64>,
    value_axis_ids: Vec<u64>,
    category_cross_axis_ids: Vec<u64>,
    value_cross_axis_ids: Vec<u64>,
    child_frames: Vec<HashSet<Vec<u8>>>,
}

impl ChartPreflightScan {
    fn observe(
        &mut self,
        reader: &Reader<&[u8]>,
        element: &BytesStart<'_>,
        ancestors: &[Vec<u8>],
        sheet_name: &str,
    ) -> Result<(), ConvertError> {
        let name = element.local_name();
        let parent = ancestors.last().map(Vec::as_slice);
        if !chart_element_parent_is_supported(parent, name.as_ref()) {
            return Err(chart_detail(sheet_name, "chart structure"));
        }
        if matches!(parent, Some(b"spPr" | b"ln"))
            && matches!(name.as_ref(), b"noFill" | b"solidFill")
        {
            let Some(seen) = self.child_frames.last_mut() else {
                return Err(chart_detail(sheet_name, "chart structure"));
            };
            if seen.contains(b"noFill".as_slice()) || seen.contains(b"solidFill".as_slice()) {
                return Err(chart_detail(sheet_name, chart_shape_detail(ancestors)));
            }
            seen.insert(name.as_ref().to_vec());
        }
        if parent == Some(b"spPr") && name.as_ref() == b"ln" {
            let Some(seen) = self.child_frames.last_mut() else {
                return Err(chart_detail(sheet_name, "chart structure"));
            };
            if !seen.insert(name.as_ref().to_vec()) {
                return Err(chart_detail(sheet_name, chart_shape_detail(ancestors)));
            }
        }
        if parent.is_some_and(|parent| chart_child_is_singleton(parent, name.as_ref())) {
            let Some(seen) = self.child_frames.last_mut() else {
                return Err(chart_detail(sheet_name, "chart structure"));
            };
            if !seen.insert(name.as_ref().to_vec()) {
                return Err(chart_detail(sheet_name, "duplicate property"));
            }
        }
        validate_chart_element(reader, element, sheet_name)?;
        validate_chart_text_element(reader, element, ancestors, sheet_name)?;
        validate_chart_shape_element(reader, element, ancestors, sheet_name)?;
        validate_data_point_element(reader, element, ancestors, sheet_name)?;
        validate_series_shape_element(reader, element, ancestors, sheet_name)?;
        validate_chart_extension_element(reader, element, ancestors, sheet_name)?;
        match name.as_ref() {
            name if name.ends_with(b"Chart") && name != b"chart" => {
                if self.current_family_axis_ids.is_some() {
                    return Err(chart_detail(sheet_name, "nested chart family"));
                }
                self.current_family_axis_ids = Some(Vec::new());
                self.line_family_seen |= name == b"lineChart";
            }
            b"rich" => {
                self.rich_text_open = true;
                self.rich_runs = 0;
                self.rich_paragraphs = 0;
            }
            b"p" if self.rich_text_open => {
                self.rich_paragraphs += 1;
                if self.rich_paragraphs > 1 {
                    return Err(chart_detail(sheet_name, "multi-paragraph text"));
                }
            }
            b"r" if self.rich_text_open => {
                self.rich_runs += 1;
                if self.rich_runs > 1 {
                    return Err(chart_detail(sheet_name, "multi-run text"));
                }
            }
            b"solidFill" if ancestors.iter().any(|name| name == b"spPr") => {
                if self.shape_fill.is_some() {
                    return Err(chart_detail(sheet_name, "shape formatting"));
                }
                self.shape_fill = Some((chart_shape_detail(ancestors), 0));
            }
            b"srgbClr" | b"schemeClr" | b"sysClr" if parent == Some(b"solidFill") => {
                if let Some((detail, colors)) = self.shape_fill.as_mut() {
                    *colors += 1;
                    if *colors > 1 {
                        return Err(chart_detail(sheet_name, detail));
                    }
                }
            }
            b"manualLayout" => {
                if self.manual_layout.is_some() {
                    return Err(chart_detail(sheet_name, "manual layout"));
                }
                let title = ancestors.iter().rev().any(|name| name == b"title");
                let plot_area = ancestors.iter().rev().any(|name| name == b"plotArea");
                if title == plot_area {
                    return Err(chart_detail(sheet_name, "manual layout"));
                }
                self.manual_layout = Some(ManualLayoutPreflight {
                    title,
                    ..ManualLayoutPreflight::default()
                });
            }
            b"layoutTarget" if parent == Some(b"manualLayout") => {
                let Some(layout) = self.manual_layout.as_mut() else {
                    return Err(chart_detail(sheet_name, "manual layout"));
                };
                if layout.title
                    || layout.layout_target
                    || chart_exact_attribute(reader, element, b"val").as_deref() != Some("inner")
                {
                    return Err(chart_detail(sheet_name, "manual layout"));
                }
                layout.layout_target = true;
            }
            b"xMode" | b"yMode" if parent == Some(b"manualLayout") => {
                let Some(layout) = self.manual_layout.as_mut() else {
                    return Err(chart_detail(sheet_name, "manual layout"));
                };
                let seen = if name.as_ref() == b"xMode" {
                    &mut layout.x_mode
                } else {
                    &mut layout.y_mode
                };
                if *seen
                    || chart_exact_attribute(reader, element, b"val").as_deref() != Some("edge")
                {
                    return Err(chart_detail(sheet_name, "manual layout"));
                }
                *seen = true;
            }
            b"x" | b"y" | b"w" | b"h" if parent == Some(b"manualLayout") => {
                let Some(layout) = self.manual_layout.as_mut() else {
                    return Err(chart_detail(sheet_name, "manual layout"));
                };
                let Some(value) = chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite())
                else {
                    return Err(chart_detail(sheet_name, "manual layout"));
                };
                let seen = match name.as_ref() {
                    b"x" => &mut layout.x,
                    b"y" => &mut layout.y,
                    b"w" if !layout.title && value > 0.0 => &mut layout.width,
                    b"h" if !layout.title && value > 0.0 => &mut layout.height,
                    _ => return Err(chart_detail(sheet_name, "manual layout")),
                };
                if *seen {
                    return Err(chart_detail(sheet_name, "manual layout"));
                }
                *seen = true;
            }
            b"ser" => {
                if self.current_series.is_some() {
                    return Err(chart_detail(sheet_name, "nested series"));
                }
                self.current_series = Some((self.series_count, false, false));
                self.series_data_point_indices.push(Vec::new());
                self.series_count += 1;
            }
            b"dPt" => {
                if self.current_data_point_index.is_some() {
                    self.bad_data_point = true;
                }
                self.current_data_point_index = Some(None);
            }
            b"idx" if parent == Some(b"ser") => {
                let Some((expected, seen, _)) = self.current_series.as_mut() else {
                    return Err(chart_detail(sheet_name, "series ordering"));
                };
                let valid = !*seen
                    && chart_exact_attribute(reader, element, b"val")
                        .and_then(|value| value.parse::<usize>().ok())
                        == Some(*expected);
                *seen = true;
                if !valid {
                    return Err(chart_detail(sheet_name, "series ordering"));
                }
            }
            b"order" if parent == Some(b"ser") => {
                let Some((expected, _, seen)) = self.current_series.as_mut() else {
                    return Err(chart_detail(sheet_name, "series ordering"));
                };
                let valid = !*seen
                    && chart_exact_attribute(reader, element, b"val")
                        .and_then(|value| value.parse::<usize>().ok())
                        == Some(*expected);
                *seen = true;
                if !valid {
                    return Err(chart_detail(sheet_name, "series ordering"));
                }
            }
            b"idx" if parent == Some(b"dPt") => {
                let parsed = chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<usize>().ok());
                let Some(index) = self.current_data_point_index.as_mut() else {
                    self.bad_data_point = true;
                    return Ok(());
                };
                if index.is_some() || parsed.is_none() {
                    self.bad_data_point = true;
                }
                *index = parsed;
            }
            b"varyColors" => {
                let valid = if matches!(parent, Some(b"pieChart" | b"doughnutChart")) {
                    true_value(chart_exact_attribute(reader, element, b"val"))
                } else {
                    false_value(chart_exact_attribute(reader, element, b"val"))
                };
                if !valid {
                    return Err(chart_detail(sheet_name, "varyColors"));
                }
            }
            b"catAx" => self.cat_axes += 1,
            b"valAx" => self.val_axes += 1,
            b"axId" => {
                let Some(value) = chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    return Err(chart_detail(sheet_name, "axis identifier"));
                };
                match parent {
                    Some(b"catAx") => self.category_axis_ids.push(value),
                    Some(b"valAx") => self.value_axis_ids.push(value),
                    _ => {
                        let Some(ids) = self.current_family_axis_ids.as_mut() else {
                            return Err(chart_detail(sheet_name, "axis identifier"));
                        };
                        ids.push(value);
                    }
                }
            }
            b"crossAx" => {
                let Some(value) = chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    return Err(chart_detail(sheet_name, "axis crossing"));
                };
                match parent {
                    Some(b"catAx") => self.category_cross_axis_ids.push(value),
                    Some(b"valAx") => self.value_cross_axis_ids.push(value),
                    _ => return Err(chart_detail(sheet_name, "axis crossing")),
                }
            }
            b"marker" if parent == Some(b"lineChart") => {
                let attributes = chart_drawing_attributes(reader, element)
                    .ok_or_else(|| chart_detail(sheet_name, "line marker visibility"))?;
                let valid = attributes.is_empty()
                    || (attributes.len() == 1
                        && attributes.get(b"val".as_slice()).is_some_and(|value| {
                            matches!(value.as_str(), "0" | "1" | "false" | "true" | "off" | "on")
                        }));
                if !valid {
                    return Err(chart_detail(sheet_name, "line marker visibility"));
                }
                let enabled = attributes
                    .get(b"val".as_slice())
                    .is_none_or(|value| matches!(value.as_str(), "1" | "true" | "on"));
                if self
                    .line_marker_enabled
                    .is_some_and(|prior| prior != enabled)
                {
                    return Err(chart_detail(sheet_name, "conflicting line marker switches"));
                }
                self.line_marker_enabled = Some(enabled);
            }
            b"symbol" if parent == Some(b"marker") && self.current_series.is_some() => {
                if !matches!(
                    chart_exact_attribute(reader, element, b"val").as_deref(),
                    Some("auto" | "none" | "circle" | "diamond" | "square" | "triangle" | "x")
                ) {
                    return Err(chart_detail(sheet_name, "marker symbol"));
                }
            }
            b"size" if parent == Some(b"marker") && self.current_series.is_some() => {
                if !chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<u8>().ok())
                    .is_some_and(|size| (2..=72).contains(&size))
                {
                    return Err(chart_detail(sheet_name, "marker size"));
                }
            }
            b"spPr" if parent == Some(b"marker") && self.current_series.is_some() => {
                return Err(chart_detail(sheet_name, "marker formatting"));
            }
            b"style"
                if parent == Some(b"chartSpace")
                    || ancestors.iter().any(|name| name == b"AlternateContent") =>
            {
                if !matches!(
                    chart_exact_attribute(reader, element, b"val").as_deref(),
                    Some("2" | "102")
                ) {
                    return Err(chart_detail(sheet_name, "chart style"));
                }
            }
            b"plotVisOnly" => {
                let value = chart_exact_attribute(reader, element, b"val");
                if value.is_none()
                    || !matches!(
                        value.as_deref(),
                        Some("0" | "1" | "false" | "true" | "off" | "on")
                    )
                {
                    return Err(chart_detail(sheet_name, "plot visible cells"));
                }
                let enabled = true_value(value);
                if self.plot_visible_only.is_some_and(|prior| prior != enabled) {
                    return Err(chart_detail(sheet_name, "plot visible cells"));
                }
                self.plot_visible_only = Some(enabled);
            }
            b"axPos" => {
                let Some(position) = chart_exact_attribute(reader, element, b"val") else {
                    return Err(chart_detail(sheet_name, "axis position"));
                };
                if ancestors.iter().rev().any(|name| name == b"catAx") {
                    self.cat_axis_positions.push(position);
                } else if ancestors.iter().rev().any(|name| name == b"valAx") {
                    self.val_axis_positions.push(position);
                }
            }
            b"crossBetween" => {
                let Some(value) = chart_exact_attribute(reader, element, b"val") else {
                    return Err(chart_detail(sheet_name, "category crossing"));
                };
                self.cross_between.push(value);
            }
            b"numCache" | b"strCache" | b"numLit" | b"strLit" => {
                if self.cache_indices.is_some() {
                    return Err(chart_detail(sheet_name, "nested data cache"));
                }
                self.cache_is_numeric = matches!(name.as_ref(), b"numCache" | b"numLit");
                self.category_cache_seen |= ancestors
                    .iter()
                    .rev()
                    .take_while(|name| name.as_slice() != b"ser")
                    .any(|name| matches!(name.as_slice(), b"cat" | b"xVal"));
                self.cache_expected = None;
                self.cache_indices = Some(Vec::new());
            }
            b"ptCount" if self.cache_indices.is_some() => {
                if self.cache_expected.is_some() {
                    self.bad_cache = true;
                }
                self.cache_expected = chart_exact_attribute(reader, element, b"val")
                    .and_then(|value| value.parse::<usize>().ok());
                if self.cache_expected.is_none() {
                    self.bad_cache = true;
                }
            }
            b"pt" if self.cache_indices.is_some() => {
                if self.cache_point_open {
                    self.bad_cache = true;
                }
                self.cache_point_open = true;
                self.cache_point_has_value = false;
                let index = chart_exact_attribute(reader, element, b"idx")
                    .and_then(|value| value.parse::<usize>().ok());
                if let (Some(indices), Some(index)) = (self.cache_indices.as_mut(), index) {
                    indices.push(index);
                } else {
                    self.bad_cache = true;
                }
            }
            b"v" if self.cache_point_open => {
                if self.in_cache_value || self.cache_point_has_value {
                    self.bad_cache = true;
                }
                self.in_cache_value = true;
                self.cache_value.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn enter_element(&mut self) {
        self.child_frames.push(HashSet::new());
    }

    fn leave_element(&mut self) {
        self.child_frames.pop();
    }

    fn finish_element(&mut self, name: &[u8], sheet_name: &str) -> Result<(), ConvertError> {
        if name.ends_with(b"Chart") && name != b"chart" {
            let Some(ids) = self.current_family_axis_ids.take() else {
                return Err(chart_detail(sheet_name, "chart family axes"));
            };
            self.family_axis_ids.push(ids);
        }
        if name == b"rich" {
            self.rich_text_open = false;
            self.rich_runs = 0;
            self.rich_paragraphs = 0;
        }
        if name == b"v" && self.in_cache_value {
            self.in_cache_value = false;
            let value = self.cache_value.trim();
            let valid = if self.cache_is_numeric {
                value.parse::<f64>().is_ok_and(|number| number.is_finite())
            } else {
                !value.is_empty()
            };
            self.bad_cache |= !valid;
            self.cache_point_has_value = valid;
            self.cache_value.clear();
        }
        if name == b"pt" && self.cache_point_open {
            self.bad_cache |= !self.cache_point_has_value;
            self.cache_point_open = false;
            self.cache_point_has_value = false;
        }
        if matches!(name, b"numCache" | b"strCache" | b"numLit" | b"strLit") {
            let mut indices = self.cache_indices.take().unwrap_or_default();
            indices.sort_unstable();
            let contiguous = indices.iter().copied().eq(0..indices.len());
            let expected_matches = self
                .cache_expected
                .is_none_or(|count| count == indices.len());
            self.bad_cache |= !contiguous || !expected_matches;
            self.cache_expected = None;
            self.cache_is_numeric = false;
            self.cache_point_open = false;
            self.cache_point_has_value = false;
            self.in_cache_value = false;
            self.cache_value.clear();
        }
        if name == b"ser" {
            self.current_series = None;
        }
        if name == b"dPt" {
            match self.current_data_point_index.take().flatten() {
                Some(index) => {
                    let Some(indices) = self.series_data_point_indices.last_mut() else {
                        self.bad_data_point = true;
                        return Ok(());
                    };
                    if indices.contains(&index) {
                        self.bad_data_point = true;
                    }
                    indices.push(index);
                }
                None => self.bad_data_point = true,
            }
        }
        if name == b"manualLayout" {
            let Some(layout) = self.manual_layout.take() else {
                return Err(chart_detail(sheet_name, "manual layout"));
            };
            let complete = layout.x_mode
                && layout.y_mode
                && layout.x
                && layout.y
                && (layout.title || (layout.layout_target && layout.width && layout.height));
            if !complete {
                return Err(chart_detail(sheet_name, "manual layout"));
            }
        }
        if name == b"solidFill"
            && let Some((detail, colors)) = self.shape_fill.take()
            && colors != 1
        {
            return Err(chart_detail(sheet_name, detail));
        }
        Ok(())
    }

    fn observe_text(&mut self, text: &str) {
        if self.in_cache_value {
            self.cache_value.push_str(text);
        }
    }
}

fn chart_axis_topology_is_valid(scan: &ChartPreflightScan) -> bool {
    let any_stated = !scan.family_axis_ids.iter().all(Vec::is_empty)
        || !scan.category_axis_ids.is_empty()
        || !scan.value_axis_ids.is_empty()
        || !scan.category_cross_axis_ids.is_empty()
        || !scan.value_cross_axis_ids.is_empty();
    if !any_stated {
        return true;
    }
    let ([category], [value], [category_cross], [value_cross]) = (
        scan.category_axis_ids.as_slice(),
        scan.value_axis_ids.as_slice(),
        scan.category_cross_axis_ids.as_slice(),
        scan.value_cross_axis_ids.as_slice(),
    ) else {
        return false;
    };
    if category == value || category_cross != value || value_cross != category {
        return false;
    }
    scan.family_axis_ids.iter().all(|ids| {
        if ids.is_empty() {
            return true;
        }
        ids.len() == 2 && ids.contains(category) && ids.contains(value) && ids[0] != ids[1]
    })
}

#[cfg(test)]
fn validate_chart_xml(xml: &str, sheet_name: &str) -> Result<(), ConvertError> {
    validate_chart_xml_with_hidden_sources(xml, sheet_name, false)
}

fn validate_chart_xml_with_hidden_sources(
    xml: &str,
    sheet_name: &str,
    hidden_source_data_exists: bool,
) -> Result<(), ConvertError> {
    use crate::ir::ChartType;

    let mut reader = Reader::from_str(xml);
    let mut ancestors: Vec<Vec<u8>> = Vec::new();
    let mut scan = ChartPreflightScan::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                scan.observe(&reader, &element, &ancestors, sheet_name)?;
                if matches!(
                    element.local_name().as_ref(),
                    b"bar3DChart"
                        | b"line3DChart"
                        | b"pie3DChart"
                        | b"area3DChart"
                        | b"surface3DChart"
                ) {
                    return Err(chart_plot(sheet_name, "3D family"));
                }
                scan.enter_element();
                ancestors.push(element.local_name().as_ref().to_vec());
            }
            Ok(Event::Empty(element)) => {
                scan.observe(&reader, &element, &ancestors, sheet_name)?;
                if matches!(
                    element.local_name().as_ref(),
                    b"bar3DChart"
                        | b"line3DChart"
                        | b"pie3DChart"
                        | b"area3DChart"
                        | b"surface3DChart"
                ) {
                    return Err(chart_plot(sheet_name, "3D family"));
                }
                scan.finish_element(element.local_name().as_ref(), sheet_name)?;
            }
            Ok(Event::End(element)) => {
                scan.finish_element(element.local_name().as_ref(), sheet_name)?;
                scan.leave_element();
                ancestors.pop();
            }
            Ok(Event::Text(value)) => {
                if let Ok(value) = value.xml_content() {
                    scan.observe_text(value.as_ref());
                } else {
                    scan.bad_cache |= scan.in_cache_value;
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(value) = crate::parser::xml_util::decode_general_ref(&reference) {
                    scan.observe_text(&value);
                } else {
                    scan.bad_cache |= scan.in_cache_value;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(crate::parser::parse_err(format!(
                    "Failed to parse XLSX chart: {error}"
                )));
            }
            _ => {}
        }
    }
    if scan.bad_cache
        || scan.cat_axes > 1
        || scan.val_axes > 1
        || !chart_axis_topology_is_valid(&scan)
    {
        let detail = if scan.bad_cache {
            "non-contiguous data cache"
        } else if !chart_axis_topology_is_valid(&scan) {
            "axis topology"
        } else {
            "multiple category or value axes"
        };
        return Err(chart_detail(sheet_name, detail));
    }
    if hidden_source_data_exists && scan.plot_visible_only.unwrap_or(true) {
        return Err(chart_detail(sheet_name, "hidden source data"));
    }

    let colors = HashMap::new();
    let aliases = HashMap::new();
    let scheme = crate::parser::drawingml::SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };
    let Some(chart) = crate::parser::chart::parse_chart_xml(xml, &scheme) else {
        return Err(chart_plot(sheet_name, "family"));
    };
    if scan.bad_data_point
        || scan
            .series_data_point_indices
            .iter()
            .enumerate()
            .any(|(series_index, indices)| {
                let value_count = chart
                    .series
                    .get(series_index)
                    .map_or(0, |series| series.values.len());
                indices.iter().any(|index| *index >= value_count)
            })
    {
        return Err(chart_detail(sheet_name, "data point formatting"));
    }
    if matches!(chart.chart_type, ChartType::Scatter)
        || matches!(&chart.chart_type, ChartType::Other(kind) if kind != crate::ir::RADAR_CHART_LABEL)
    {
        return Err(chart_plot(sheet_name, "family"));
    }
    if let Some(plot_type) = chart
        .series
        .iter()
        .find_map(|series| match &series.plot_type {
            Some(ChartType::Scatter | ChartType::Other(_)) => series.plot_type.as_ref(),
            _ => None,
        })
    {
        return Err(chart_plot(
            sheet_name,
            &format!("mixed family {plot_type:?}"),
        ));
    }
    if scan.category_cache_seen
        && chart
            .series
            .iter()
            .any(|series| series.values.len() != chart.categories.len())
    {
        return Err(chart_detail(sheet_name, "non-contiguous data cache"));
    }
    let line_series_marker_mismatch = scan.line_family_seen
        && scan.line_marker_enabled != Some(true)
        && chart.series.iter().any(|series| {
            let is_line = (matches!(chart.chart_type, ChartType::Line)
                && series.plot_type.is_none())
                || matches!(series.plot_type, Some(ChartType::Line));
            is_line && series.marker_symbol != Some(crate::ir::MarkerSymbol::Off)
        });
    let (expected_cat_axis, expected_val_axis) = match chart.chart_type {
        ChartType::Bar => ("l", "b"),
        _ => ("b", "l"),
    };
    if line_series_marker_mismatch
        || scan
            .cat_axis_positions
            .iter()
            .any(|position| position != expected_cat_axis)
        || scan
            .val_axis_positions
            .iter()
            .any(|position| position != expected_val_axis)
        || scan.cross_between.iter().any(|value| value != "between")
    {
        let detail = if line_series_marker_mismatch {
            "line marker visibility"
        } else if scan
            .cat_axis_positions
            .iter()
            .any(|position| position != expected_cat_axis)
            || scan
                .val_axis_positions
                .iter()
                .any(|position| position != expected_val_axis)
        {
            "axis position"
        } else {
            "category crossing"
        };
        return Err(chart_detail(sheet_name, detail));
    }

    let drawable = match &chart.chart_type {
        ChartType::Bar | ChartType::Column => {
            !chart.series.is_empty() && !chart.categories.is_empty()
        }
        ChartType::Line | ChartType::Area => {
            !chart.series.is_empty() && chart.categories.len() >= 2
        }
        ChartType::Pie | ChartType::Doughnut => chart
            .series
            .first()
            .is_some_and(|series| series.values.iter().any(|value| *value > 0.0)),
        ChartType::Other(kind) if kind == crate::ir::RADAR_CHART_LABEL => {
            !chart.series.is_empty()
                && chart.categories.len() >= 3
                && chart
                    .series
                    .iter()
                    .any(|series| series.values.iter().any(|value| *value > 0.0))
        }
        ChartType::Scatter | ChartType::Other(_) => false,
    };
    if !drawable {
        return Err(chart_plot(sheet_name, "without drawable cached data"));
    }
    Ok(())
}

fn part_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn part_rels_path(path: &str) -> String {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    format!("{dir}/_rels/{file}.rels")
}

fn follow_chart_user_shapes(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    chart_path: &str,
    sheet_name: &str,
    hidden_source_data_exists: bool,
) -> Result<(), ConvertError> {
    let Some(chart_xml) = read_xml(archive, chart_path)? else {
        return Err(unsupported(format!(
            "missing chart part on sheet: {sheet_name}"
        )));
    };
    validate_chart_xml_with_hidden_sources(&chart_xml, sheet_name, hidden_source_data_exists)?;
    let Some(rid) = chart_user_shapes_rid(&chart_xml)? else {
        return Ok(());
    };
    let rels_path = part_rels_path(chart_path);
    let Some(rels_xml) = read_xml(archive, &rels_path)? else {
        return Err(unsupported(format!(
            "missing chart user-shapes relationship on sheet: {sheet_name}"
        )));
    };
    let rels = relationships(&rels_xml)?;
    let Some(relationship) = rels.get(&rid) else {
        return Err(unsupported(format!(
            "unresolved chart user-shapes relationship on sheet: {sheet_name}"
        )));
    };
    if relationship.external {
        return Err(unsupported(format!(
            "external chart user-shapes relationship on sheet: {sheet_name}"
        )));
    }
    let drawing_path = resolve_relative_xl_path(part_dir(chart_path), &relationship.target);
    let Some(drawing_xml) = read_xml(archive, &drawing_path)? else {
        return Err(unsupported(format!(
            "missing chart user-shapes part on sheet: {sheet_name}"
        )));
    };
    validate_chart_drawing(&drawing_xml, sheet_name)
}

fn validate_external_link_parts(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    names: &[String],
) -> Result<(), ConvertError> {
    for name in names.iter().filter(|name| {
        name.starts_with("xl/externalLinks/") && name.ends_with(".xml") && !name.contains("/_rels/")
    }) {
        let Some(xml) = read_xml(archive, name)? else {
            continue;
        };
        let mut reader = Reader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(Event::Start(element) | Event::Empty(element)) => {
                    match element.local_name().as_ref() {
                        b"ddeLink" => return Err(unsupported("DDE link")),
                        b"oleLink" => return Err(unsupported("external OLE link")),
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(error) => {
                    return Err(crate::parser::parse_err(format!(
                        "Failed to parse XLSX external link {name}: {error}"
                    )));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn package_has_hidden_worksheet_data(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    names: &[String],
) -> Result<bool, ConvertError> {
    for name in names.iter().filter(|name| {
        name.starts_with("xl/worksheets/") && name.ends_with(".xml") && !name.contains("/_rels/")
    }) {
        let Some(xml) = read_xml(archive, name)? else {
            continue;
        };
        let mut reader = Reader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(Event::Start(element) | Event::Empty(element))
                    if matches!(element.local_name().as_ref(), b"row" | b"col")
                        && true_value(attr_value(&reader, &element, b"hidden")) =>
                {
                    return Ok(true);
                }
                Ok(Event::Eof) => break,
                Err(error) => {
                    return Err(crate::parser::parse_err(format!(
                        "Failed to inspect hidden XLSX source data in {name}: {error}"
                    )));
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

pub(super) fn ensure_supported_package(
    data: &[u8],
    printed_sheet_names: &HashSet<String>,
) -> Result<(), ConvertError> {
    let mut archive = crate::parser::open_zip(data)?;
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .collect();

    for name in &names {
        let lower = name.trim_start_matches('/').to_ascii_lowercase();
        if lower == "xl/vbaproject.bin" {
            return Err(unsupported("macro-enabled workbook"));
        }
        if lower.starts_with("xl/activex/") {
            return Err(unsupported("ActiveX control"));
        }
        if lower.starts_with("xl/embeddings/") {
            return Err(unsupported("embedded OLE package"));
        }
    }
    validate_external_link_parts(&mut archive, &names)?;
    let hidden_source_data_exists = package_has_hidden_worksheet_data(&mut archive, &names)?;

    let workbook_xml = read_xml(&mut archive, "xl/workbook.xml")?
        .ok_or_else(|| crate::parser::parse_err("XLSX package has no workbook part"))?;
    let workbook_rels_xml = read_xml(&mut archive, "xl/_rels/workbook.xml.rels")?
        .ok_or_else(|| crate::parser::parse_err("XLSX package has no workbook relationships"))?;
    let workbook_rels = relationships(&workbook_rels_xml)?;
    let defined_names = super::cond_fmt_raw::extract_defined_names(data);

    let mut printable_drawings = Vec::new();
    let mut used_dxf_ids = HashSet::new();
    for (sheet_name, sheet_rid) in parse_workbook_sheet_rids(&workbook_xml) {
        if !printed_sheet_names.contains(&sheet_name) {
            continue;
        }
        let Some(sheet_relationship) = workbook_rels.get(&sheet_rid) else {
            return Err(unsupported(format!(
                "unresolved printed sheet relationship: {sheet_name}"
            )));
        };
        if sheet_relationship.external {
            return Err(unsupported(format!(
                "external printed sheet relationship: {sheet_name}"
            )));
        }
        if sheet_relationship.kind.ends_with("/dialogsheet") {
            return Err(unsupported(format!(
                "dialog sheet selected for printing: {sheet_name}"
            )));
        }
        let sheet_path = sheet_part_path(&sheet_relationship.target);
        let Some(sheet_xml) = read_xml(&mut archive, &sheet_path)? else {
            return Err(unsupported(format!(
                "missing printed sheet part: {sheet_name}"
            )));
        };
        let sheet_scan = validate_worksheet(&sheet_xml, &sheet_name, &defined_names)?;
        used_dxf_ids.extend(sheet_scan.dxf_ids);
        printable_drawings.push((
            sheet_name,
            sheet_relationship.target.clone(),
            sheet_scan.drawing_rids,
            sheet_scan.legacy_drawing_rids,
        ));
    }

    if !used_dxf_ids.is_empty() {
        let mut style_relationships = workbook_rels
            .values()
            .filter(|relationship| relationship.kind.ends_with("/styles"));
        let Some(styles_relationship) = style_relationships.next() else {
            return Err(unsupported("missing differential conditional-format style"));
        };
        if style_relationships.next().is_some() || styles_relationship.external {
            return Err(unsupported(
                "ambiguous differential conditional-format style",
            ));
        }
        let styles_path = resolve_relative_xl_path("xl", &styles_relationship.target);
        let Some(styles_xml) = read_xml(&mut archive, &styles_path)? else {
            return Err(unsupported("missing differential conditional-format style"));
        };
        validate_differential_styles(&styles_xml, &used_dxf_ids)?;
    }

    for (sheet_name, sheet_target, drawing_rids, legacy_drawing_rids) in printable_drawings {
        if drawing_rids.is_empty() && legacy_drawing_rids.is_empty() {
            continue;
        }
        let rels_path = sheet_rels_path(&sheet_target);
        let Some(sheet_rels_xml) = read_xml(&mut archive, &rels_path)? else {
            return Err(unsupported(format!(
                "missing drawing relationships on sheet: {sheet_name}"
            )));
        };
        let sheet_rels = relationships(&sheet_rels_xml)?;
        for legacy_drawing_rid in legacy_drawing_rids {
            let Some(legacy_relationship) = sheet_rels.get(&legacy_drawing_rid) else {
                return Err(unsupported(format!(
                    "unresolved legacy drawing relationship on sheet: {sheet_name}"
                )));
            };
            if legacy_relationship.external || !legacy_relationship.kind.ends_with("/vmlDrawing") {
                return Err(unsupported(format!(
                    "unsupported legacy drawing relationship on sheet: {sheet_name}"
                )));
            }
            let vml_path = resolve_relative_xl_path(
                &sheet_part_dir(&sheet_target),
                &legacy_relationship.target,
            );
            let Some(vml_xml) = read_xml(&mut archive, &vml_path)? else {
                return Err(unsupported(format!(
                    "missing legacy drawing part on sheet: {sheet_name}"
                )));
            };
            if vml_has_visible_note(&vml_xml)? {
                return Err(unsupported(format!(
                    "visible cell comment on sheet: {sheet_name}"
                )));
            }
        }
        for drawing_rid in drawing_rids {
            let Some(drawing_relationship) = sheet_rels.get(&drawing_rid) else {
                return Err(unsupported(format!(
                    "unresolved drawing relationship on sheet: {sheet_name}"
                )));
            };
            if drawing_relationship.external {
                return Err(unsupported(format!(
                    "external drawing relationship on sheet: {sheet_name}"
                )));
            }
            let drawing_path = resolve_relative_xl_path(
                &sheet_part_dir(&sheet_target),
                &drawing_relationship.target,
            );
            let Some(drawing_xml) = read_xml(&mut archive, &drawing_path)? else {
                return Err(unsupported(format!(
                    "missing drawing part on sheet: {sheet_name}"
                )));
            };
            let chart_rids = validate_worksheet_drawing(&drawing_xml, &sheet_name)?;
            if chart_rids.is_empty() {
                continue;
            }
            let drawing_rels_path = part_rels_path(&drawing_path);
            let Some(drawing_rels_xml) = read_xml(&mut archive, &drawing_rels_path)? else {
                return Err(unsupported(format!(
                    "missing chart relationships on sheet: {sheet_name}"
                )));
            };
            let drawing_rels = relationships(&drawing_rels_xml)?;
            for chart_rid in chart_rids {
                let Some(chart_relationship) = drawing_rels.get(&chart_rid) else {
                    return Err(unsupported(format!(
                        "unresolved chart relationship on sheet: {sheet_name}"
                    )));
                };
                if chart_relationship.external {
                    return Err(unsupported(format!(
                        "external chart relationship on sheet: {sheet_name}"
                    )));
                }
                let chart_path =
                    resolve_relative_xl_path(part_dir(&drawing_path), &chart_relationship.target);
                follow_chart_user_shapes(
                    &mut archive,
                    &chart_path,
                    &sheet_name,
                    hidden_source_data_exists,
                )?;
            }
        }
    }

    Ok(())
}
