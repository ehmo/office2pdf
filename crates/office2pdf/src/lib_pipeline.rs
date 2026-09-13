use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::config::{ConvertOptions, Format};
use crate::error::{ConvertError, ConvertMetrics, ConvertResult, ConvertWarning};
use crate::parser::Parser;
use crate::{ir, parser, render};

fn format_label(format: Format) -> &'static str {
    match format {
        Format::Docx => "DOCX",
        Format::Pptx => "PPTX",
        Format::Xlsx => "XLSX",
    }
}

fn dedup_warnings(warnings: &mut Vec<ConvertWarning>) {
    let mut seen: HashSet<String> = HashSet::new();
    warnings.retain(|warning| seen.insert(warning.to_string()));
}

/// Build a `ConvertResult`, deduplicating warnings automatically so callers
/// don't need to remember to call `dedup_warnings` before every return site.
fn build_convert_result(
    pdf: Vec<u8>,
    mut warnings: Vec<ConvertWarning>,
    metrics: Option<ConvertMetrics>,
) -> ConvertResult {
    dedup_warnings(&mut warnings);
    ConvertResult {
        pdf,
        warnings,
        metrics,
    }
}

fn extract_panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic".to_string()
    }
}

const OLE2_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

pub(super) fn is_ole2(data: &[u8]) -> bool {
    data.len() >= OLE2_MAGIC.len() && data[..OLE2_MAGIC.len()] == OLE2_MAGIC
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn should_resolve_font_context(
    doc: &ir::Document,
    options: &ConvertOptions,
    has_embedded_fonts: bool,
) -> bool {
    has_embedded_fonts
        || !options.font_paths.is_empty()
        || !options.font_bytes.is_empty()
        || options
            .last_resort_font_family
            .as_deref()
            .is_some_and(|family| !family.trim().is_empty())
        || render::font_subst::document_requests_font_families(doc)
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_font_context_with_embedded(
    doc: &ir::Document,
    options: &ConvertOptions,
    embedded_font_dir: Option<&parser::embedded_fonts::EmbeddedFontDir>,
    in_memory_fonts: &[typst::text::Font],
) -> Option<render::font_context::FontSearchContext> {
    let has_embedded = embedded_font_dir.is_some_and(|d| !d.is_empty());
    if !should_resolve_font_context(doc, options, has_embedded) {
        return None;
    }
    let mut all_paths: Vec<std::path::PathBuf> = options.font_paths.clone();
    if let Some(dir) = embedded_font_dir
        && !dir.is_empty()
    {
        all_paths.push(dir.path().to_path_buf());
    }
    Some(
        render::font_context::resolve_font_search_context(&all_paths)
            .with_in_memory_fonts(in_memory_fonts)
            .with_last_resort_family(options.last_resort_font_family.as_deref()),
    )
}

fn load_registered_fonts(options: &ConvertOptions) -> Result<Vec<typst::text::Font>, ConvertError> {
    let mut fonts = Vec::new();
    for (index, bytes) in options.font_bytes.iter().enumerate() {
        let parsed = render::pdf::load_fonts_from_bytes([bytes.as_slice()]);
        if parsed.is_empty() {
            return Err(ConvertError::Render(format!(
                "registered font at index {index} contains no usable font faces"
            )));
        }
        fonts.extend(parsed);
    }
    Ok(fonts)
}

fn load_additional_fonts(options: &ConvertOptions) -> Result<Vec<typst::text::Font>, ConvertError> {
    let fonts = load_registered_fonts(options)?;
    #[cfg(all(target_arch = "wasm32", feature = "wasm-cjk-font"))]
    {
        let mut fonts = fonts;
        fonts.extend_from_slice(crate::bundled_fonts::cjk_fonts());
        Ok(fonts)
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wasm-cjk-font")))]
    {
        Ok(fonts)
    }
}

fn effective_last_resort_family(options: &ConvertOptions) -> Option<&str> {
    let configured = options
        .last_resort_font_family
        .as_deref()
        .map(str::trim)
        .filter(|family| !family.is_empty());
    #[cfg(all(target_arch = "wasm32", feature = "wasm-cjk-font"))]
    return configured.or(Some(crate::bundled_fonts::CJK_LAST_RESORT_FAMILY));
    #[cfg(not(all(target_arch = "wasm32", feature = "wasm-cjk-font")))]
    configured
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn convert(path: impl AsRef<std::path::Path>) -> Result<ConvertResult, ConvertError> {
    convert_with_options(path, &ConvertOptions::default())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn convert_with_options(
    path: impl AsRef<std::path::Path>,
    options: &ConvertOptions,
) -> Result<ConvertResult, ConvertError> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| ConvertError::UnsupportedFormat("no file extension".to_string()))?;

    let format = Format::from_extension(ext)
        .ok_or_else(|| ConvertError::UnsupportedFormat(ext.to_string()))?;

    let data = std::fs::read(path)?;
    convert_bytes(&data, format, options)
}

pub(super) fn convert_bytes(
    data: &[u8],
    format: Format,
    options: &ConvertOptions,
) -> Result<ConvertResult, ConvertError> {
    convert_bytes_with_docx_choices(data, format, options, None)
}

pub(super) fn convert_reviewed_docx_bytes(
    data: &[u8],
    options: &ConvertOptions,
    choices: &[crate::docx_font_choices::Choice],
) -> Result<ConvertResult, ConvertError> {
    if data.is_empty() || data.len() > 1024 * 1024 || options.streaming {
        return Err(ConvertError::Parse("font_choices_input".into()));
    }
    convert_bytes_with_docx_choices(data, Format::Docx, options, Some(choices))
}

fn convert_bytes_with_docx_choices(
    data: &[u8],
    format: Format,
    options: &ConvertOptions,
    choices: Option<&[crate::docx_font_choices::Choice]>,
) -> Result<ConvertResult, ConvertError> {
    if is_ole2(data) {
        return Err(ConvertError::UnsupportedEncryption);
    }

    #[cfg(feature = "pdf-ops")]
    if options.streaming && format == Format::Xlsx {
        return convert_bytes_streaming_xlsx(data, options);
    }

    let total_start: Instant = Instant::now();
    let input_size_bytes = data.len() as u64;
    let additional_fonts = load_additional_fonts(options)?;

    // Extract embedded fonts before parsing (PPTX/DOCX only). Native keeps the
    // materialized directory alive through compilation; WASM keeps parsed
    // faces in memory for its filesystem-free Typst world.
    #[cfg(not(target_arch = "wasm32"))]
    let embedded_font_dir = parser::embedded_fonts::extract_embedded_fonts(data, format);
    #[cfg(target_arch = "wasm32")]
    let embedded_font_data = parser::embedded_fonts::extract_embedded_font_data(data, format);
    #[cfg(target_arch = "wasm32")]
    let embedded_fonts = embedded_font_data.as_ref().map_or_else(Vec::new, |fonts| {
        render::pdf::load_fonts_from_bytes(fonts.font_bytes())
    });
    #[cfg(target_arch = "wasm32")]
    let in_memory_fonts: Vec<typst::text::Font> = embedded_fonts
        .into_iter()
        .chain(additional_fonts.iter().cloned())
        .collect();

    let parser: Box<dyn Parser> = match format {
        #[cfg(feature = "format-docx")]
        Format::Docx => Box::new(parser::docx::DocxParser),
        #[cfg(feature = "format-pptx")]
        Format::Pptx => Box::new(parser::pptx::PptxParser),
        #[cfg(feature = "format-xlsx")]
        Format::Xlsx => Box::new(parser::xlsx::XlsxParser),
        #[allow(unreachable_patterns)]
        _ => {
            return Err(ConvertError::UnsupportedFormat(format_label(format).to_string()));
        }
    };

    let parse_start: Instant = Instant::now();
    let parse_result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parser.parse(data, options)));
    let (mut doc, mut warnings) = match parse_result {
        Ok(result) => result?,
        Err(panic_info) => {
            return Err(ConvertError::Parse(format!(
                "upstream parser panicked: {}",
                extract_panic_message(&panic_info)
            )));
        }
    };
    let parse_duration = parse_start.elapsed();
    let page_count = doc.pages.len() as u32;
    if let Some(choices) = choices {
        use typst::text::{FontStretch, FontStyle};
        if !warnings.is_empty() { return Err(ConvertError::Parse("font_choices_parse_warning".into())); }
        crate::docx_font_choices::apply(&mut doc, choices, |choice, points| {
            additional_fonts.iter().any(|font| {
                let info = font.info();
                info.family.eq_ignore_ascii_case(&choice.family)
                    && info.variant.weight.to_number() == if choice.bold { 700 } else { 400 }
                    && info.variant.style == if choice.italic { FontStyle::Italic } else { FontStyle::Normal }
                    && info.variant.stretch == FontStretch::NORMAL
                    && points.iter().all(|point| char::from_u32(*point).is_some_and(|scalar|
                        font.ttf().glyph_index(scalar).is_some_and(|glyph| glyph.0 != 0)))
            })
        }).map_err(|message| ConvertError::Parse(message.into()))?;
    }

    #[cfg(not(target_arch = "wasm32"))]
    let font_context = resolve_font_context_with_embedded(
        &doc,
        options,
        embedded_font_dir.as_ref(),
        &additional_fonts,
    );
    #[cfg(target_arch = "wasm32")]
    let font_context = (!in_memory_fonts.is_empty()
        || effective_last_resort_family(options).is_some()
        || render::font_subst::document_requests_font_families(&doc))
    .then(|| {
        render::font_context::resolve_font_search_context_from_fonts(&in_memory_fonts)
            .with_last_resort_family(effective_last_resort_family(options))
    });

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(font_context) = font_context.as_ref() {
        warnings.extend(
            render::font_subst::detect_missing_font_fallbacks_with_context(&doc, font_context)
                .into_iter()
                .map(|(from, to)| ConvertWarning::FallbackUsed {
                    format: format_label(format).to_string(),
                    from,
                    to,
                }),
        );
    }

    #[cfg(target_arch = "wasm32")]
    if let Some(font_context) = font_context.as_ref() {
        warnings.extend(
            render::font_subst::detect_missing_font_fallbacks_with_context(&doc, font_context)
                .into_iter()
                .map(|(from, to)| ConvertWarning::FallbackUsed {
                    format: format_label(format).to_string(),
                    from,
                    to,
                }),
        );
    } else {
        warnings.extend(
            render::font_subst::detect_missing_font_fallbacks(&doc, &options.font_paths)
                .into_iter()
                .map(|(from, to)| ConvertWarning::FallbackUsed {
                    format: format_label(format).to_string(),
                    from,
                    to,
                }),
        );
    }

    let codegen_start: Instant = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let output = render::typst_gen::generate_typst_with_options_and_font_context(
        &doc,
        options,
        font_context.as_ref(),
    )?;
    #[cfg(target_arch = "wasm32")]
    let output = match font_context.as_ref() {
        Some(font_context) => render::typst_gen::generate_typst_with_options_and_font_context(
            &doc,
            options,
            Some(font_context),
        )?,
        None => render::typst_gen::generate_typst_with_options(&doc, options)?,
    };
    let codegen_duration = codegen_start.elapsed();

    let compile_start: Instant = Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let pdf = render::pdf::compile_to_pdf_with_fonts(
        &output.source,
        &output.images,
        options.pdf_standard,
        font_context
            .as_ref()
            .map(|context| context.search_paths())
            .unwrap_or(&[]),
        &additional_fonts,
        options.tagged,
        options.pdf_ua,
    )?;
    #[cfg(target_arch = "wasm32")]
    let pdf = render::pdf::compile_to_pdf_with_fonts(
        &output.source,
        &output.images,
        options.pdf_standard,
        &[],
        &in_memory_fonts,
        options.tagged,
        options.pdf_ua,
    )?;
    let compile_duration = compile_start.elapsed();

    let total_duration = total_start.elapsed();
    let output_size_bytes = pdf.len() as u64;

    Ok(build_convert_result(
        pdf,
        warnings,
        Some(ConvertMetrics {
            parse_duration,
            codegen_duration,
            compile_duration,
            total_duration,
            input_size_bytes,
            output_size_bytes,
            page_count,
        }),
    ))
}

#[cfg(all(feature = "pdf-ops", feature = "format-xlsx"))]
fn convert_bytes_streaming_xlsx(
    data: &[u8],
    options: &ConvertOptions,
) -> Result<ConvertResult, ConvertError> {
    let total_start: Instant = Instant::now();
    let input_size_bytes = data.len() as u64;
    let additional_fonts = load_additional_fonts(options)?;
    let chunk_size = options
        .streaming_chunk_size
        .unwrap_or(crate::defaults::DEFAULT_STREAMING_CHUNK_SIZE);

    let xlsx_parser = parser::xlsx::XlsxParser;

    let parse_start: Instant = Instant::now();
    let parse_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        xlsx_parser.parse_streaming(data, options, chunk_size)
    }));
    let (chunk_docs, mut warnings) = match parse_result {
        Ok(result) => result?,
        Err(panic_info) => {
            return Err(ConvertError::Parse(format!(
                "upstream parser panicked: {}",
                extract_panic_message(&panic_info)
            )));
        }
    };
    let parse_duration = parse_start.elapsed();

    let needs_in_memory_font_context = !additional_fonts.is_empty()
        || effective_last_resort_family(options).is_some()
        || chunk_docs
            .iter()
            .any(render::font_subst::document_requests_font_families);
    #[cfg(not(target_arch = "wasm32"))]
    let font_context =
        (needs_in_memory_font_context || !options.font_paths.is_empty()).then(|| {
            render::font_context::resolve_font_search_context(&options.font_paths)
                .with_in_memory_fonts(&additional_fonts)
                .with_last_resort_family(effective_last_resort_family(options))
        });
    #[cfg(target_arch = "wasm32")]
    let font_context = needs_in_memory_font_context.then(|| {
        render::font_context::resolve_font_search_context_from_fonts(&additional_fonts)
            .with_last_resort_family(effective_last_resort_family(options))
    });

    if chunk_docs.is_empty() {
        let empty_doc = ir::Document {
            metadata: ir::Metadata::default(),
            pages: vec![],
            styles: ir::StyleSheet::default(),
        };
        let output = render::typst_gen::generate_typst_with_options_and_font_context(
            &empty_doc,
            options,
            font_context.as_ref(),
        )?;
        let pdf = render::pdf::compile_to_pdf_with_fonts(
            &output.source,
            &output.images,
            options.pdf_standard,
            font_context
                .as_ref()
                .map(|context| context.search_paths())
                .unwrap_or(&[]),
            &additional_fonts,
            options.tagged,
            options.pdf_ua,
        )?;
        let total_duration = total_start.elapsed();
        return Ok(build_convert_result(
            pdf,
            warnings,
            Some(ConvertMetrics {
                parse_duration,
                codegen_duration: std::time::Duration::ZERO,
                compile_duration: std::time::Duration::ZERO,
                total_duration,
                input_size_bytes,
                output_size_bytes: 0,
                page_count: 0,
            }),
        ));
    }

    let mut all_pdfs: Vec<Vec<u8>> = Vec::with_capacity(chunk_docs.len());
    let mut codegen_duration_total = std::time::Duration::ZERO;
    let mut compile_duration_total = std::time::Duration::ZERO;
    let mut total_page_count: u32 = 0;

    for chunk_doc in chunk_docs {
        total_page_count += chunk_doc.pages.len() as u32;

        if let Some(font_context) = font_context.as_ref() {
            warnings.extend(
                render::font_subst::detect_missing_font_fallbacks_with_context(
                    &chunk_doc,
                    font_context,
                )
                .into_iter()
                .map(|(from, to)| ConvertWarning::FallbackUsed {
                    format: format_label(Format::Xlsx).to_string(),
                    from,
                    to,
                }),
            );
        }

        let codegen_start: Instant = Instant::now();
        let output = render::typst_gen::generate_typst_with_options_and_font_context(
            &chunk_doc,
            options,
            font_context.as_ref(),
        )?;
        codegen_duration_total += codegen_start.elapsed();

        let compile_start: Instant = Instant::now();
        let pdf = render::pdf::compile_to_pdf_with_fonts(
            &output.source,
            &output.images,
            options.pdf_standard,
            font_context
                .as_ref()
                .map(|context| context.search_paths())
                .unwrap_or(&[]),
            &additional_fonts,
            options.tagged,
            options.pdf_ua,
        )?;
        compile_duration_total += compile_start.elapsed();

        all_pdfs.push(pdf);
    }

    let final_pdf = if all_pdfs.len() == 1 {
        // Safety: len() == 1 guarantees at least one element
        all_pdfs
            .into_iter()
            .next()
            .expect("all_pdfs is non-empty (len == 1)")
    } else {
        let refs: Vec<&[u8]> = all_pdfs.iter().map(|p| p.as_slice()).collect();
        crate::pdf_ops::merge(&refs)
            .map_err(|e| ConvertError::Render(format!("PDF merge failed: {e}")))?
    };

    let total_duration = total_start.elapsed();
    let output_size_bytes = final_pdf.len() as u64;

    Ok(build_convert_result(
        final_pdf,
        warnings,
        Some(ConvertMetrics {
            parse_duration,
            codegen_duration: codegen_duration_total,
            compile_duration: compile_duration_total,
            total_duration,
            input_size_bytes,
            output_size_bytes,
            page_count: total_page_count,
        }),
    ))
}

pub(super) fn render_document(doc: &ir::Document) -> Result<Vec<u8>, ConvertError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let options = ConvertOptions::default();
        let font_context = resolve_font_context_with_embedded(doc, &options, None, &[]);
        let output = render::typst_gen::generate_typst_with_options_and_font_context(
            doc,
            &options,
            font_context.as_ref(),
        )?;
        render::pdf::compile_to_pdf(
            &output.source,
            &output.images,
            None,
            font_context
                .as_ref()
                .map(|context| context.search_paths())
                .unwrap_or(&[]),
            false,
            false,
        )
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "wasm-cjk-font")))]
    {
        let output = render::typst_gen::generate_typst(doc)?;
        render::pdf::compile_to_pdf(&output.source, &output.images, None, &[], false, false)
    }
    #[cfg(all(target_arch = "wasm32", feature = "wasm-cjk-font"))]
    {
        let options = ConvertOptions::default();
        let fonts = load_additional_fonts(&options)?;
        let font_context = render::font_context::resolve_font_search_context_from_fonts(&fonts)
            .with_last_resort_family(effective_last_resort_family(&options));
        let output = render::typst_gen::generate_typst_with_options_and_font_context(
            doc,
            &options,
            Some(&font_context),
        )?;
        render::pdf::compile_to_pdf_with_fonts(
            &output.source,
            &output.images,
            None,
            &[],
            &fonts,
            false,
            false,
        )
    }
}
