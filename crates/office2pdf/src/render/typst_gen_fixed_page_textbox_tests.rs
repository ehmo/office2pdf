use super::*;

fn assert_powerpoint_grid_words_in_order(source: &str, words: &[&str]) {
    let mut remainder = source;
    for word in words {
        let needle = format!("#o2p-pptx-word([{}]", escape_typst(word));
        let offset = remainder
            .find(&needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in:\n{source}"));
        remainder = &remainder[offset + needle.len()..];
    }
}

#[test]
fn test_fixed_page_text_box() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_text_box(100.0, 200.0, 300.0, 50.0, "Slide Title")],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert_powerpoint_grid_words_in_order(&output.source, &["Slide", "Title"]);
    assert!(output.source.contains("100pt"));
    assert!(output.source.contains("200pt"));
}

#[test]
fn test_fixed_page_text_box_uses_padding_and_center_vertical_align() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            100.0,
            200.0,
            300.0,
            50.0,
            Insets {
                top: 3.6,
                right: 7.2,
                bottom: 3.6,
                left: 7.2,
            },
            crate::ir::TextBoxVerticalAlign::Center,
            vec![Block::Paragraph(Paragraph {
                style: ParagraphStyle::default(),
                runs: vec![Run {
                    text: "Centered".to_string(),
                    style: TextStyle::default(),
                    href: None,
                    footnote: None,
                }],
            })],
        )],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output
            .source
            .contains("inset: (top: 3.6pt, right: 7.2pt, bottom: 3.6pt, left: 7.2pt)")
    );
    assert!(output.source.contains("width: 285.6pt"));
    assert!(output.source.contains(
        "#context {\n    let text_box_slack_0 = calc.max(42.8pt - measure(text_box_content_0).height, 0pt)"
    ));
    assert!(output.source.contains("#v(text_box_slack_0 / 2)"));
    assert!(output.source.contains("let text_box_aligned_0 = ["));
}

#[test]
fn test_fixed_page_text_box_multiple_paragraphs_preserve_breaks() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 100.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![
                    Block::Paragraph(Paragraph {
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: "First item".to_string(),
                            style: TextStyle::default(),
                            href: None,
                            footnote: None,
                        }],
                    }),
                    Block::Paragraph(Paragraph {
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: "Second item".to_string(),
                            style: TextStyle::default(),
                            href: None,
                            footnote: None,
                        }],
                    }),
                ],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert_powerpoint_grid_words_in_order(&output.source, &["First", "item", "Second", "item"]);
    assert_eq!(
        output
            .source
            .matches("#block(above: 0pt, below: 0pt)")
            .count(),
        2,
        "each paragraph should retain a separate block: {}",
        output.source
    );
}

#[test]
fn test_fixed_page_text_box_ordered_list_preserves_textbox_styling() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 100.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    line_spacing: Some(LineSpacing::Proportional(1.5)),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: " First item".to_string(),
                                    style: TextStyle {
                                        font_size: Some(24.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: Some(1),
                        },
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    line_spacing: Some(LineSpacing::Proportional(1.5)),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: " Second item".to_string(),
                                    style: TextStyle {
                                        font_size: Some(24.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: None,
                        },
                    ],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1.".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(!output.source.contains("#enum("));
    assert_powerpoint_grid_words_in_order(
        &output.source,
        &["1.", "First", "item", "2.", "Second", "item"],
    );
    assert_eq!(
        output.source.matches("#text(size: 24pt)[").count(),
        4,
        "both markers and both item bodies retain their 24pt style: {}",
        output.source
    );
    assert!(!output.source.contains("\\\n2. Second item"));
    // A slide list paces on PowerPoint's line box, scaled by the declared
    // `a:lnSpc`: 1.2em at 24pt is 28.8pt, and 1.5 line spacing makes the
    // advance between two items 43.2pt (issue #934).
    // A slide list paces on PowerPoint's line box, scaled by the declared
    // `a:lnSpc`: 1.2em at 24pt is 28.8pt, and 1.5 line spacing makes each
    // item's box 43.2pt — which is the advance, since nothing is emitted
    // between items that declare no spacing (issue #934).
    assert!(
        output
            .source
            .contains("#set text(top-edge: 1.386em, bottom-edge: -0.41400000000000003em)"),
        "the item takes the 1.5-scaled PowerPoint line box: {}",
        output.source
    );
    assert!(
        output.source.contains("#set par(leading: 0pt)"),
        "the line box carries the advance, so the leading is zero: {}",
        output.source
    );
}

#[test]
fn test_fixed_page_text_box_compact_list_items_use_full_width_blocks() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle::default(),
                                runs: vec![Run {
                                    text: "Long first item that should wrap inside the fixed text box width".to_string(),
                                    style: TextStyle {
                                        font_size: Some(20.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: Some(1),
                        },
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle::default(),
                                runs: vec![Run {
                                    text: "Long second item that should also wrap inside the fixed text box width".to_string(),
                                    style: TextStyle {
                                        font_size: Some(20.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: None,
                        },
                    ],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1)".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                    no_wrap: false,
                auto_fit: false,
            text_rotation_deg: None,
            shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    // The items' blocks contribute no spacing of their own; the gap between
    // them is emitted between them instead (issue #934).
    assert_eq!(
        output
            .source
            .matches("#block(width: 320pt, above: 0pt, below: 0pt)[")
            .count(),
        2
    );
}

#[test]
fn test_fixed_page_text_box_compact_list_preserves_hanging_indent() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![ListItem {
                        content: vec![Paragraph {
                            style: ParagraphStyle {
                                indent_left: Some(36.0),
                                indent_first_line: Some(-36.0),
                                ..ParagraphStyle::default()
                            },
                            runs: vec![Run {
                                text: "Long first item that should wrap under the body text instead of the number".to_string(),
                                style: TextStyle {
                                    font_size: Some(20.0),
                                    ..TextStyle::default()
                                },
                                href: None,
                                footnote: None,
                            }],
                        }],
                        level: 0,
                        start_at: Some(1),
                    }],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1)".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                    no_wrap: false,
                auto_fit: false,
            text_rotation_deg: None,
            shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    assert!(
        output
            .source
            .contains("#grid(columns: (36pt, 1fr), gutter: 0pt,"),
        "Expected ordered hanging-indent list to use a marker/body grid, got:\n{}",
        output.source,
    );
    assert!(!output.source.contains("hanging-indent: 36pt"));
    assert!(!output.source.contains("tab_advance_1"));
}

#[test]
fn test_fixed_page_text_box_compact_list_preserves_marker_origin_offset() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![ListItem {
                        content: vec![Paragraph {
                            style: ParagraphStyle {
                                indent_left: Some(54.0),
                                indent_first_line: Some(-36.0),
                                ..ParagraphStyle::default()
                            },
                            runs: vec![Run {
                                text: "Marker origin should stay inset while wrapped lines align to the text column"
                                    .to_string(),
                                style: TextStyle {
                                    font_size: Some(20.0),
                                    ..TextStyle::default()
                                },
                                href: None,
                                footnote: None,
                            }],
                        }],
                        level: 0,
                        start_at: Some(1),
                    }],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1)".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                    no_wrap: false,
                auto_fit: false,
            text_rotation_deg: None,
            shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    assert!(
        output
            .source
            .contains("inset: (top: 0pt, right: 0pt, bottom: 0pt, left: 18pt)")
    );
    assert!(
        output
            .source
            .contains("#grid(columns: (36pt, 1fr), gutter: 0pt,")
    );
}

#[test]
fn test_fixed_page_text_box_compact_bulleted_list_uses_custom_marker_style() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Unordered,
                    items: vec![ListItem {
                        content: vec![Paragraph {
                            style: ParagraphStyle {
                                indent_left: Some(22.5),
                                indent_first_line: Some(-22.5),
                                ..ParagraphStyle::default()
                            },
                            runs: vec![Run {
                                text: "Symbol bullet".to_string(),
                                style: TextStyle {
                                    font_family: Some("Pretendard".to_string()),
                                    font_size: Some(14.0),
                                    ..TextStyle::default()
                                },
                                href: None,
                                footnote: None,
                            }],
                        }],
                        level: 0,
                        start_at: None,
                    }],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Unordered,
                            numbering_pattern: None,
                            full_numbering: false,
                            marker_text: Some("è".to_string()),
                            marker_style: Some(TextStyle {
                                font_family: Some("Wingdings".to_string()),
                                font_size: Some(14.0),
                                ..TextStyle::default()
                            }),
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    assert!(!output.source.contains("Wingdings"));
    assert!(output.source.contains("➔"));
    assert!(output.source.contains("tab_advance_1"));
    assert_powerpoint_grid_words_in_order(&output.source, &["Symbol", "bullet"]);
}

#[test]
fn test_escape_typst_escapes_leading_dash_list_prefix() {
    assert_eq!(escape_typst("- bullet"), "\\- bullet");
}

#[test]
fn test_fixed_page_text_box_dash_bullets_use_generic_list_path() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Unordered,
                    items: vec![
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    indent_left: Some(22.5),
                                    indent_first_line: Some(-22.5),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: "First dash bullet".to_string(),
                                    style: TextStyle {
                                        font_family: Some("Pretendard".to_string()),
                                        font_size: Some(14.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: None,
                        },
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    indent_left: Some(22.5),
                                    indent_first_line: Some(-22.5),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: "Second dash bullet".to_string(),
                                    style: TextStyle {
                                        font_family: Some("Pretendard".to_string()),
                                        font_size: Some(14.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: None,
                        },
                    ],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Unordered,
                            numbering_pattern: None,
                            full_numbering: false,
                            marker_text: Some("-".to_string()),
                            marker_style: Some(TextStyle {
                                font_family: Some("Pretendard".to_string()),
                                font_size: Some(14.0),
                                ..TextStyle::default()
                            }),
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    assert!(output.source.contains("#list("));
    assert!(output.source.contains("marker: ["));
    assert!(!output.source.contains("tab_advance_1"));
}

#[test]
fn test_fixed_page_text_box_compact_list_preserves_soft_line_breaks() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![ListItem {
                        content: vec![Paragraph {
                            style: ParagraphStyle::default(),
                            runs: vec![Run {
                                text: "Line 1\u{000B}Line 2".to_string(),
                                style: TextStyle {
                                    font_size: Some(20.0),
                                    ..TextStyle::default()
                                },
                                href: None,
                                footnote: None,
                            }],
                        }],
                        level: 0,
                        start_at: Some(1),
                    }],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1)".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();

    assert!(output.source.contains("#linebreak()"));
    assert!(output.source.contains("#set text(size: 20pt"));
    assert!(output.source.contains("leading: 13pt"));
}

#[test]
fn test_fixed_page_text_box_soft_line_break_before_parenthesis_compiles() {
    // A Korean slide run reads "첫째 줄" over "(2) 둘째 줄". CJK keeps the run
    // off the advance grid and it carries no formatting wrapper, so the soft
    // break's `#linebreak()` is followed by the bare text, which Typst would
    // otherwise read as the break's argument list.
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "첫째 줄\u{000B}(2) 둘째 줄".to_string(),
                        style: TextStyle::default(),
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("#linebreak()#[(2)"),
        "the bare text after the soft break must be wrapped: {}",
        output.source
    );
    let pdf =
        crate::render::pdf::compile_to_pdf(&output.source, &output.images, None, &[], false, false)
            .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source));
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn test_fixed_page_text_box_grid_word_before_parenthesised_run_compiles() {
    // A styleless ASCII run is emitted as `#o2p-pptx-word(...)`, which ends
    // in `)`; a following Korean run starting with `(` leaves the grid and
    // would otherwise be read as that call's argument list.
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![
                        Run {
                            text: "Line 1".to_string(),
                            style: TextStyle::default(),
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: "(2) 둘째 줄".to_string(),
                            style: TextStyle::default(),
                            href: None,
                            footnote: None,
                        },
                    ],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("))#[(2)"),
        "the bare text after the grid word must be wrapped: {}",
        output.source
    );
    let pdf =
        crate::render::pdf::compile_to_pdf(&output.source, &output.images, None, &[], false, false)
            .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source));
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn test_fixed_page_text_box_with_width_height() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_text_box(50.0, 60.0, 400.0, 100.0, "Sized box")],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(output.source.contains("400pt"));
    assert!(output.source.contains("100pt"));
}

#[test]
fn test_fixed_page_text_box_with_solid_fill() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 50.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "White BG".to_string(),
                        style: TextStyle::default(),
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: Some(Color {
                    r: 255,
                    g: 255,
                    b: 255,
                }),
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("fill: rgb(255, 255, 255)"),
        "Expected white fill in output, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_with_fill_and_stroke() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 50.0,
            y: 80.0,
            width: 200.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "Bordered".to_string(),
                        style: TextStyle::default(),
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: Some(Color {
                    r: 200,
                    g: 220,
                    b: 240,
                }),
                opacity: None,
                stroke: Some(BorderSide {
                    width: 1.0,
                    color: Color { r: 0, g: 0, b: 0 },
                    style: BorderLineStyle::Solid,
                }),
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("fill: rgb(200, 220, 240)"),
        "Expected fill color in output, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("stroke: 1pt + rgb(0, 0, 0)"),
        "Expected stroke in output, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_with_fill_and_opacity() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 10.0,
            y: 20.0,
            width: 150.0,
            height: 30.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "Semi-transparent".to_string(),
                        style: TextStyle::default(),
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: Some(Color {
                    r: 255,
                    g: 255,
                    b: 255,
                }),
                opacity: Some(0.5),
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("fill: rgb(255, 255, 255, 128)"),
        "Expected fill with alpha in output, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_with_polygon_shape_kind() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 50.0,
            y: 80.0,
            width: 200.0,
            height: 60.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "Arrow Tab".to_string(),
                        style: TextStyle::default(),
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets {
                    top: 3.6,
                    right: 7.2,
                    bottom: 3.6,
                    left: 7.2,
                },
                vertical_align: crate::ir::TextBoxVerticalAlign::Center,
                fill: Some(Color {
                    r: 0,
                    g: 37,
                    b: 154,
                }),
                opacity: None,
                stroke: None,
                shape_kind: Some(ShapeKind::Polygon {
                    vertices: vec![(0.0, 0.0), (0.85, 0.0), (1.0, 0.5), (0.85, 1.0), (0.0, 1.0)],
                }),
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    // Should contain #polygon for the shape background
    assert!(
        output.source.contains("#polygon("),
        "Expected polygon in output for non-rectangular text box, got:\n{}",
        output.source,
    );
    // The fill should be on the polygon, not the block
    assert!(
        output.source.contains("fill: rgb(0, 37, 154)"),
        "Expected fill color on polygon, got:\n{}",
        output.source,
    );
    // Should NOT have fill on the block itself
    let block_line = output
        .source
        .lines()
        .find(|l| l.contains("#block("))
        .expect("Expected #block line");
    assert!(
        !block_line.contains("fill:"),
        "Block should not have fill when shape_kind is set, got:\n{block_line}",
    );
}

#[test]
fn test_fixed_page_text_box_no_fill_no_stroke() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_text_box(10.0, 20.0, 150.0, 30.0, "Plain")],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("fill: white"),
        "Expected fill: white for no-background slide, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains("stroke:"),
        "Expected no stroke in output, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_uses_place_for_positioning() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_text_box(100.0, 200.0, 300.0, 50.0, "Positioned")],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(output.source.contains("place("));
}

#[test]
fn test_fixed_page_text_box_no_wrap_centered_text_uses_inline_box() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 220.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "Centered Title".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("clip: false"),
        "Expected clip: false for no-wrap text box, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("#set align(center)"),
        "Expected centered alignment in output, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("#box["),
        "Expected inline no-wrap box in output, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_keeps_mixed_latin_cjk_searchable() {
    // The heading issue #664 measured: `panic 안전성` came out with a joiner
    // between every character and its space swapped for U+00A0, so searching
    // the PDF for the heading found nothing.
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 180.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "panic 안전성".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("panic 안전성"),
        "Expected the heading with an ordinary space, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_still_strips_the_kinsoku_marker() {
    // Triangulation: the zero-width kinsoku marker must keep being dropped. A
    // no-wrap box never takes that break, and letting U+200B through would
    // trade one invisible text-layer character for another.
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 180.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "\u{200B}제안\u{200B}개요".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("제안개요"),
        "Expected the marker stripped, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains('\u{200B}'),
        "No U+200B may reach the text layer, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_keeps_cjk_title_extractable() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 180.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "제안개요".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    // The enclosing `#box[` already forbids every break inside the paragraph,
    // so no joiner is needed — and one here would reach the PDF text layer and
    // make the title unsearchable (issue #664).
    assert!(
        output.source.contains("제안개요"),
        "Expected the title verbatim in output, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains('\u{2060}'),
        "No U+2060 WORD JOINER may reach the text layer, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains('\u{00A0}'),
        "No U+00A0 NO-BREAK SPACE may reach the text layer, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("#box["),
        "The no-wrap box is what suppresses the breaks, got:\n{}",
        output.source,
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn test_fixed_page_text_box_no_wrap_keeps_latin_text_extractable() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 180.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "Test text".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    let extracted: String = crate::render::pdf::compiled_text_runs(&output.source, 0)
        .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source))
        .into_iter()
        .map(|run| run.text)
        .collect();
    assert!(
        extracted.contains("Test text"),
        "Expected plain Latin no-wrap text to remain extractable, got {extracted:?}:\n{}",
        output.source
    );
    assert!(
        !output.source.contains('\u{2060}') && !output.source.contains('\u{00A0}'),
        "Expected no invisible joiners or non-breaking spaces for Latin no-wrap text, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_keeps_mixed_script_titles_unbroken() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 320.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "III. 기술부문".to_string(),
                        style: TextStyle {
                            font_size: Some(28.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    // The heading must stay unbreakable *and* readable: the enclosing `#box[`
    // supplies the first, so the text itself is emitted verbatim (issue #664).
    assert!(
        output.source.contains("III. 기술부문"),
        "Expected the mixed-script heading verbatim, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("#box["),
        "The no-wrap box is what keeps the heading unbroken, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains('\u{2060}') && !output.source.contains('\u{00A0}'),
        "No invisible joiner may reach the text layer, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_preserves_mixed_script_titles_across_runs() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 320.0,
            height: 40.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![
                        Run {
                            text: "III.".to_string(),
                            style: TextStyle {
                                font_size: Some(28.0),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: " 기술부문".to_string(),
                            style: TextStyle {
                                font_size: Some(40.0),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                    ],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    // Split across runs, the heading still has to come out as one readable
    // string rather than a joiner-separated one (issue #664).
    assert!(
        output.source.contains("III.") && output.source.contains(" 기술부문"),
        "Expected both runs of the heading verbatim, the second keeping its
         ordinary leading space, got:\n{}",
        output.source,
    );
    assert!(
        !output.source.contains('\u{2060}') && !output.source.contains('\u{00A0}'),
        "No invisible joiner may reach the text layer, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_auto_fit_short_text_uses_scale_to_fit() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 120.0,
            width: 145.0,
            height: 12.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        alignment: Some(Alignment::Center),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: "Server(Cloud VM)".to_string(),
                        style: TextStyle {
                            font_size: Some(18.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: true,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("let text_box_scale_width_0 = (145pt / calc.max(measure(text_box_raw_0).width, 1pt)) * 100%"),
        "Expected width scale calculation, got:\n{}",
        output.source,
    );
    assert!(
        output
            .source
            .contains("let text_box_scale_height_0 = (12pt / 21.599999999999998pt) * 100%"),
        "Expected estimated line-height scale calculation, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("let text_box_scale_0 = calc.min(100%, calc.min(text_box_scale_width_0, text_box_scale_height_0))"),
        "Expected combined width/height scale clamp, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains(
            "#scale(x: text_box_scale_0, y: text_box_scale_0, origin: top + left, reflow: true)["
        ),
        "Expected scale-to-fit wrapper, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("#align(center)["),
        "Expected center alignment wrapper, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_no_wrap_auto_fit_uses_scale_to_fit() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 295.0,
            y: 78.0,
            width: 143.16,
            height: 58.15,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![
                        Run {
                            text: "- ".to_string(),
                            style: TextStyle {
                                font_size: Some(41.99),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: "목 차 ".to_string(),
                            style: TextStyle {
                                font_size: Some(41.99),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: "-".to_string(),
                            style: TextStyle {
                                font_size: Some(41.99),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                    ],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: true,
                auto_fit: true,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("#let text_box_raw_0 = ["),
        "Expected no-wrap auto-fit title to use raw single-line measurement, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("let text_box_scale_width_0 ="),
        "Expected no-wrap auto-fit title to compute width scale, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("let text_box_scale_height_0 ="),
        "Expected no-wrap auto-fit title to compute height scale, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains(
            "#scale(x: text_box_scale_0, y: text_box_scale_0, origin: top + left, reflow: true)["
        ),
        "Expected no-wrap auto-fit title to use scale-to-fit, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_mixed_font_header_uses_scale_to_fit() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 0.0,
            y: 2.4,
            width: 474.5,
            height: 57.9,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![
                        Run {
                            text: "3. 시스템 연동 방안".to_string(),
                            style: TextStyle {
                                font_size: Some(25.0),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: "| 클라우드 기반 업무 시스템 연동".to_string(),
                            style: TextStyle {
                                font_size: Some(16.0),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                    ],
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Center,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("#let text_box_raw_0 = ["),
        "Expected raw single-line content wrapper, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains(
            "#scale(x: text_box_scale_0, y: text_box_scale_0, origin: top + left, reflow: true)["
        ),
        "Expected mixed-font header to use scale-to-fit, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_mixed_font_header_with_tight_leading_uses_scale_to_fit() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 0.0,
            y: 2.4,
            width: 474.5,
            height: 57.9,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        line_spacing: Some(LineSpacing::Proportional(0.585)),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![
                        Run {
                            text: "3. 시스템 연동 방안".to_string(),
                            style: TextStyle {
                                font_size: Some(24.99),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                        Run {
                            text: "|  클라우드 기반 업무 시스템 연동".to_string(),
                            style: TextStyle {
                                font_size: Some(16.0),
                                ..TextStyle::default()
                            },
                            href: None,
                            footnote: None,
                        },
                    ],
                })],
                padding: Insets {
                    top: 3.6,
                    right: 7.2,
                    bottom: 3.6,
                    left: 7.2,
                },
                vertical_align: crate::ir::TextBoxVerticalAlign::Center,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output.source.contains("#let text_box_raw_0 = ["),
        "Expected mixed-font header to use raw single-line measurement, got:\n{}",
        output.source,
    );
    assert!(
        output
            .source
            .contains("let text_box_scale_0 = calc.min(100%, calc.min(text_box_scale_width_0, text_box_scale_height_0))"),
        "Expected mixed-font header to use combined scale-to-fit, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_wrapped_centered_paragraph_scales_to_fit_height() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 368.9,
            y: 376.8,
            width: 139.0,
            height: 58.5,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "업무 시스템의 URL 기준으로 문서의 암/복호화 지원".to_string(),
                        style: TextStyle {
                            font_size: Some(18.0),
                            color: Some(Color {
                                r: 255,
                                g: 255,
                                b: 255,
                            }),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets {
                    top: 3.6,
                    right: 7.2,
                    bottom: 3.6,
                    left: 7.2,
                },
                vertical_align: crate::ir::TextBoxVerticalAlign::Center,
                fill: Some(Color {
                    r: 0,
                    g: 120,
                    b: 185,
                }),
                opacity: None,
                stroke: Some(BorderSide {
                    color: Color {
                        r: 0,
                        g: 120,
                        b: 185,
                    },
                    width: 1.0,
                    style: BorderLineStyle::Solid,
                }),
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output
            .source
            .contains("#let text_box_raw_0 = block(width: 124.60000000000001pt)["),
        "Expected wrapped paragraph measurement block, got:\n{}",
        output.source,
    );
    assert!(
        output.source.contains("let text_box_scale_0 = calc.min(100%, (51.3pt / calc.max(measure(text_box_raw_0).height, 1pt)) * 100%)"),
        "Expected height-based wrapped paragraph scale clamp, got:\n{}",
        output.source,
    );
}

#[test]
fn test_fixed_page_text_box_ordered_grid_normalizes_marker_spacing() {
    use crate::ir::List;

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 140.0,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::List(List {
                    kind: ListKind::Ordered,
                    items: vec![
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    indent_left: Some(36.0),
                                    indent_first_line: Some(-36.0),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: " Alpha".to_string(),
                                    style: TextStyle {
                                        font_size: Some(20.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: Some(1),
                        },
                        ListItem {
                            content: vec![Paragraph {
                                style: ParagraphStyle {
                                    indent_left: Some(36.0),
                                    indent_first_line: Some(-36.0),
                                    ..ParagraphStyle::default()
                                },
                                runs: vec![Run {
                                    text: "Beta".to_string(),
                                    style: TextStyle {
                                        font_size: Some(20.0),
                                        ..TextStyle::default()
                                    },
                                    href: None,
                                    footnote: None,
                                }],
                            }],
                            level: 0,
                            start_at: None,
                        },
                    ],
                    level_styles: BTreeMap::from([(
                        0,
                        ListLevelStyle {
                            kind: ListKind::Ordered,
                            numbering_pattern: Some("1.".to_string()),
                            full_numbering: false,
                            marker_text: None,
                            marker_style: None,
                        },
                    )]),
                })],
                padding: Insets::default(),
                vertical_align: crate::ir::TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);
    let output = generate_typst(&doc).unwrap();
    assert!(
        output
            .source
            .contains("#text(size: 20pt)[#o2p-pptx-word([1\\.]"),
        "the first marker keeps its 20pt style: {}",
        output.source
    );
    assert!(
        output
            .source
            .contains("#text(size: 20pt)[#o2p-pptx-word([2\\.]"),
        "the second marker keeps its 20pt style: {}",
        output.source
    );
    assert_eq!(
        output.source.matches("#o2p-pptx-space()").count(),
        2,
        "each marker owns exactly one normalized trailing space: {}",
        output.source
    );
}

// ----- PowerPoint's line model (issue #513) -----

/// A slide text box holding one paragraph of `text` in `family` at `size`.
fn slide_text_box_source(family: &str, size: f64, style: ParagraphStyle) -> Option<String> {
    crate::render::pdf::powerpoint_line_box_em(family)?;
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            100.0,
            100.0,
            400.0,
            200.0,
            Insets::default(),
            crate::ir::TextBoxVerticalAlign::Top,
            vec![Block::Paragraph(Paragraph {
                style,
                runs: vec![Run {
                    text: text_for(family),
                    style: TextStyle {
                        font_family: Some(family.to_string()),
                        font_size: Some(size),
                        ..TextStyle::default()
                    },
                    href: None,
                    footnote: None,
                }],
            })],
        )],
    )]);
    Some(generate_typst(&doc).unwrap().source)
}

fn text_for(family: &str) -> String {
    format!("Slide body set in {family}")
}

#[test]
fn slide_text_takes_powerpoints_flat_1_2em_line() {
    // PowerPoint gives every line 1.2 times the font size whatever the font's
    // own metrics say. Measured on native exports from both platforms: one
    // wrapped Arial paragraph advances 20.3657pt over 14 gaps at 17pt
    // (1.1980em) and 28.8400pt over 18 at 24pt (1.2017em). Slide text used to
    // take Word's hhea pitch, which is up to 4% short per line (issue #513).
    let Some(source) = slide_text_box_source("Libertinus Serif", 18.0, ParagraphStyle::default())
    else {
        return; // no font book available (e.g. exotic CI sandbox)
    };
    let (top, bottom) =
        emitted_line_box_em(&source).unwrap_or_else(|| panic!("no line box emitted: {source}"));

    assert!(
        (top + bottom - 1.2).abs() < 0.001,
        "a slide line spans 1.2em, got {}em: {source}",
        top + bottom
    );
    assert!(
        source.contains("leading: 0pt"),
        "the advance is carried by the box, not by leading: {source}"
    );
}

#[test]
fn slide_baseline_splits_the_lines_extra_leading_evenly() {
    // The glyphs take hhea `ascent + descent`; whatever the 1.2em line has
    // left over is split evenly above and below them, seating the baseline at
    // `(1.2 + ascent - descent) / 2`. A proportional split on OS/2
    // `usWinAscent` was tried and measured 0.45-1.12pt low on every frame of
    // `08_marketing_report_en` (issues #513, #660).
    let Some(source) = slide_text_box_source("Libertinus Serif", 18.0, ParagraphStyle::default())
    else {
        return;
    };
    let (top, bottom) = emitted_line_box_em(&source).expect("line box emitted");
    let (expected_top, expected_bottom) =
        crate::render::pdf::powerpoint_line_box_em("Libertinus Serif").expect("metrics resolve");

    assert!(
        (top - expected_top).abs() < 0.001 && (bottom - expected_bottom).abs() < 0.001,
        "expected {expected_top}/{expected_bottom}em, got {top}/{bottom}em: {source}"
    );
    assert!(
        bottom > 0.01,
        "the descent gap must be real: a bottom-anchored box keeps it below its \
         last baseline, and we used to drop it entirely: {source}"
    );
}

#[test]
fn the_split_differs_between_fonts_while_the_line_does_not() {
    // Triangulation: the height is a property of PowerPoint and the split is a
    // property of the font, so two faces must agree on 1.2em and disagree on
    // where the baseline sits inside it.
    let Some(serif) = crate::render::pdf::powerpoint_line_box_em("Libertinus Serif") else {
        return;
    };
    let Some(mono) = crate::render::pdf::powerpoint_line_box_em("DejaVu Sans Mono") else {
        return;
    };

    assert!((serif.0 + serif.1 - 1.2).abs() < 0.000_001);
    assert!((mono.0 + mono.1 - 1.2).abs() < 0.000_001);
    assert!(
        (serif.0 - mono.0).abs() > 0.001,
        "two faces with different hhea ascent/descent should seat the baseline \
         differently: {serif:?} vs {mono:?}"
    );
    assert!(
        serif.0 > 0.0 && serif.1 > 0.0 && mono.0 > 0.0 && mono.1 > 0.0,
        "both edges are positive distances: {serif:?} {mono:?}"
    );
}

/// The line advance, in em, the generator emits for `percent` line spacing.
fn slide_line_advance_em(percent: f64) -> Option<f64> {
    let source = slide_text_box_source(
        "Libertinus Serif",
        18.0,
        ParagraphStyle {
            line_spacing: Some(LineSpacing::Proportional(percent)),
            ..ParagraphStyle::default()
        },
    )?;
    let (top, bottom) =
        emitted_line_box_em(&source).unwrap_or_else(|| panic!("no line box emitted: {source}"));
    Some(top + bottom)
}

#[test]
fn a_slide_paragraph_scales_the_1_2em_line_by_its_own_line_spacing() {
    // `a:lnSpc` scales PowerPoint's line rather than replacing it: the advance
    // is `percent x 1.2em`. Carrying it as `par(leading)` instead did nothing
    // between single-line paragraphs — a slide's code block is one `<a:p>` per
    // line — so eight lines of Rust stacked into the height of three (#541).
    let Some(advance) = slide_line_advance_em(1.18) else {
        return; // no font book available (e.g. exotic CI sandbox)
    };

    assert!(
        (advance - 1.18 * 1.2).abs() < 0.001,
        "118% line spacing spans {}em, expected {}em",
        advance,
        1.18 * 1.2
    );
}

#[test]
fn slide_line_spacing_scales_proportionally() {
    // Triangulation: a second percentage must land on its own multiple of
    // 1.2em, so the advance cannot be a constant that happens to fit 118%.
    let (Some(at_118), Some(at_125), Some(at_100)) = (
        slide_line_advance_em(1.18),
        slide_line_advance_em(1.25),
        slide_line_advance_em(1.0),
    ) else {
        return;
    };

    assert!((at_125 - 1.25 * 1.2).abs() < 0.001, "125% spans {at_125}em");
    assert!((at_100 - 1.2).abs() < 0.001, "100% spans {at_100}em");
    assert!(
        at_125 > at_118 && at_118 > at_100,
        "advance must rise with the percentage: {at_100} / {at_118} / {at_125}"
    );
}

#[test]
fn slide_line_spacing_keeps_the_fonts_baseline_split() {
    // Scaling the line must not move the baseline within it: the glyphs keep
    // their half of the taller box's extra leading.
    let Some(source) = slide_text_box_source(
        "Libertinus Serif",
        18.0,
        ParagraphStyle {
            line_spacing: Some(LineSpacing::Proportional(1.5)),
            ..ParagraphStyle::default()
        },
    ) else {
        return;
    };
    let (top, bottom) = emitted_line_box_em(&source).expect("line box emitted");
    let (unscaled_top, unscaled_bottom) =
        crate::render::pdf::powerpoint_line_box_em("Libertinus Serif").expect("metrics resolve");

    assert!(
        (top / (top + bottom) - unscaled_top / (unscaled_top + unscaled_bottom)).abs() < 0.001,
        "the baseline split moved: {top}/{bottom} against {unscaled_top}/{unscaled_bottom}"
    );
    assert!(
        source.contains("leading: 0pt"),
        "the advance is carried by the box, not by leading: {source}"
    );
}

#[test]
fn a_slide_paragraph_declares_its_own_block_spacing() {
    // A slide paragraph's gaps are its own `a:spcBef`/`a:spcAft` and nothing
    // else. Leaving them unset let Typst's 1.2em `block.spacing` default in,
    // which put 13pt between the lines of a code block declaring no spacing
    // (issue #513).
    let Some(source) = slide_text_box_source("Libertinus Serif", 11.0, ParagraphStyle::default())
    else {
        return;
    };

    assert!(
        source.contains("above: 0pt, below: 0pt"),
        "an unspaced slide paragraph pins both gaps to zero: {source}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn consecutive_slide_paragraphs_keep_powerpoints_full_line_advance() {
    // The unstyled case takes Typst's embedded default. Calibri verifies a
    // named Office family and the synthetic name forces the final embedded
    // fallback after every named candidate misses.
    for family in [None, Some("Calibri"), Some("Office2Pdf Missing Test Face")] {
        let paragraphs = ["Paragraph one", "Paragraph two", "Paragraph three"]
            .into_iter()
            .map(|text| {
                Block::Paragraph(Paragraph {
                    style: ParagraphStyle {
                        line_spacing: Some(LineSpacing::Proportional(1.0)),
                        ..ParagraphStyle::default()
                    },
                    runs: vec![Run {
                        text: text.to_string(),
                        style: TextStyle {
                            font_family: family.map(str::to_string),
                            font_size: Some(18.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })
            })
            .collect();
        let doc = make_doc(vec![make_fixed_page(
            960.0,
            540.0,
            vec![make_fixed_text_box(
                54.0,
                54.0,
                800.0,
                250.0,
                Insets::default(),
                crate::ir::TextBoxVerticalAlign::Top,
                paragraphs,
            )],
        )]);
        let output = generate_typst(&doc).unwrap();
        let mut baselines: Vec<f64> = crate::render::pdf::compiled_text_runs(&output.source, 0)
            .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source))
            .into_iter()
            .filter(|run| run.text.starts_with("Paragraph"))
            .map(|run| run.baseline_pt)
            .collect();
        baselines.sort_by(f64::total_cmp);

        assert_eq!(
            baselines.len(),
            3,
            "expected three paragraphs: {baselines:?}"
        );
        for gap in baselines.windows(2).map(|pair| pair[1] - pair[0]) {
            assert!(
                (gap - 21.6).abs() < 0.01,
                "18pt slide paragraphs in {family:?} should advance by 1.2em (21.6pt), \
                 got {gap}pt; baselines={baselines:?}\n{}",
                output.source
            );
        }
    }
}

/// A hard break starts a PowerPoint line whose own largest run sets the full
/// 1.2em advance. Typst normally combines the preceding line's descent with
/// the following line's ascent, which is wrong when the sizes differ (#683).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn powerpoint_hard_break_advance_uses_the_following_lines_font_size() {
    let family = "Arial";
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            72.0,
            72.0,
            400.0,
            120.0,
            Insets::default(),
            crate::ir::TextBoxVerticalAlign::Top,
            vec![Block::Paragraph(Paragraph {
                style: ParagraphStyle::default(),
                runs: vec![
                    Run {
                        text: "Large\n".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(12.5),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                    Run {
                        text: "Small\u{000B}".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(10.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                    Run {
                        text: "Large".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(12.5),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                ],
            })],
        )],
    )]);
    let output = generate_typst(&doc).unwrap();
    let runs = crate::render::pdf::compiled_text_runs(&output.source, 0)
        .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source));
    let mut baselines: Vec<f64> = runs
        .iter()
        .filter(|run| run.text.contains("Large") || run.text.contains("Small"))
        .map(|run| run.baseline_pt)
        .collect();
    baselines.sort_by(f64::total_cmp);
    baselines.dedup_by(|left, right| (*left - *right).abs() < 0.01);

    assert_eq!(baselines.len(), 3, "expected three lines: {baselines:?}");
    let (top_em, _) = crate::render::pdf::powerpoint_line_box_em(family)
        .expect("the Arial-compatible line metrics must resolve");
    let expected_first_baseline = 72.0 + top_em * 12.5;
    assert!(
        (baselines[0] - expected_first_baseline).abs() < 0.01,
        "hard-break boxing must preserve the first line's top seating: \
         expected {expected_first_baseline}, got {baselines:?}\n{}",
        output.source
    );
    assert!(
        (baselines[1] - baselines[0] - 12.0).abs() < 0.01,
        "the 10pt following line must own a 12pt advance: {baselines:?}\n{}",
        output.source
    );
    assert!(
        (baselines[2] - baselines[1] - 15.0).abs() < 0.01,
        "the 12.5pt following line must own a 15pt advance: {baselines:?}\n{}",
        output.source
    );
}

/// A proportional `<a:lnSpc>` scales the following line's complete box; the
/// hard-break correction must preserve that paragraph-level multiplier.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn powerpoint_hard_break_preserves_proportional_line_spacing() {
    let family = "Arial";
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            72.0,
            72.0,
            400.0,
            120.0,
            Insets::default(),
            crate::ir::TextBoxVerticalAlign::Top,
            vec![Block::Paragraph(Paragraph {
                style: ParagraphStyle {
                    line_spacing: Some(LineSpacing::Proportional(1.5)),
                    ..ParagraphStyle::default()
                },
                runs: vec![
                    Run {
                        text: "Large\n".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(12.5),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                    Run {
                        text: "Small".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(10.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                ],
            })],
        )],
    )]);
    let output = generate_typst(&doc).unwrap();
    let runs = crate::render::pdf::compiled_text_runs(&output.source, 0)
        .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source));
    let large_baseline = runs
        .iter()
        .find(|run| run.text.contains("Large"))
        .expect("Large run")
        .baseline_pt;
    let small_baseline = runs
        .iter()
        .find(|run| run.text.contains("Small"))
        .expect("Small run")
        .baseline_pt;

    assert!(
        (small_baseline - large_baseline - 18.0).abs() < 0.01,
        "the 1.5 x 1.2em box of the 10pt following line must advance 18pt: \
         {large_baseline}, {small_baseline}\n{}",
        output.source
    );
}

/// The previous line's bottom edge can only be replaced when the explicit
/// segment fits one physical line. Otherwise the replacement would also alter
/// every ordinary wrapped line inside that segment (#683).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn powerpoint_hard_break_keeps_normal_edges_when_the_segment_soft_wraps() {
    let family = "Arial";
    let font_size_pt = 12.5;
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            72.0,
            72.0,
            45.0,
            140.0,
            Insets::default(),
            crate::ir::TextBoxVerticalAlign::Top,
            vec![Block::Paragraph(Paragraph {
                style: ParagraphStyle::default(),
                runs: vec![
                    Run {
                        text: "Large words wrap\n".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(font_size_pt),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                    Run {
                        text: "Small".to_string(),
                        style: TextStyle {
                            font_family: Some(family.to_string()),
                            font_size: Some(10.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    },
                ],
            })],
        )],
    )]);
    let output = generate_typst(&doc).unwrap();
    let mut large_baselines: Vec<f64> = crate::render::pdf::compiled_text_runs(&output.source, 0)
        .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source))
        .into_iter()
        .filter(|run| run.text.contains("Large") || run.text.contains("words"))
        .map(|run| run.baseline_pt)
        .collect();
    large_baselines.sort_by(f64::total_cmp);
    large_baselines.dedup_by(|left, right| (*left - *right).abs() < 0.01);

    assert!(
        large_baselines.len() >= 2,
        "the probe must soft-wrap before its explicit break: {large_baselines:?}\n{}",
        output.source
    );
    for gap in large_baselines.windows(2).map(|pair| pair[1] - pair[0]) {
        assert!(
            (gap - 1.2 * font_size_pt).abs() < 0.01,
            "ordinary wrapped lines must retain their 15pt pitch, got {gap}: \
             {large_baselines:?}\n{}",
            output.source
        );
    }
}

/// PowerPoint rounds every nominal glyph advance to the nearest 1/8pt before
/// it decides where a line ends. Ten 17pt Libertinus `o` glyphs are a stable,
/// environment-free edge case: their exact Typst advances fit this box, while
/// their PowerPoint-grid advances exceed it. The tenth word must therefore
/// move to a second line (issue #661).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn slide_text_wraps_on_powerpoints_one_eighth_point_advance_grid() {
    let family = "Libertinus Serif";
    let font_size_pt = 17.0;
    let word_advance_pt = crate::render::pdf::text_advance_em(family, false, "o")
        .expect("the embedded Libertinus Serif face must resolve")
        * font_size_pt;
    let space_advance_pt = crate::render::pdf::text_advance_em(family, false, " ")
        .expect("the embedded Libertinus Serif space must resolve")
        * font_size_pt;
    let quantize = |advance_pt: f64| (advance_pt * 8.0).round() / 8.0;
    let exact_line_pt = 10.0 * word_advance_pt + 9.0 * space_advance_pt;
    let grid_line_pt = 10.0 * quantize(word_advance_pt) + 9.0 * quantize(space_advance_pt);
    assert!(
        grid_line_pt > exact_line_pt + 0.25,
        "probe must straddle the 1/8pt grid: exact={exact_line_pt}, grid={grid_line_pt}"
    );

    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![make_fixed_text_box(
            72.0,
            72.0,
            (exact_line_pt + grid_line_pt) / 2.0,
            100.0,
            Insets::default(),
            crate::ir::TextBoxVerticalAlign::Center,
            vec![Block::Paragraph(Paragraph {
                style: ParagraphStyle::default(),
                runs: vec![Run {
                    text: "o o o o o o o o o o".to_string(),
                    style: TextStyle {
                        font_family: Some(family.to_string()),
                        font_size: Some(font_size_pt),
                        ..TextStyle::default()
                    },
                    href: None,
                    footnote: None,
                }],
            })],
        )],
    )]);
    let output = generate_typst(&doc).unwrap();
    let mut baselines: Vec<f64> = crate::render::pdf::compiled_text_runs(&output.source, 0)
        .unwrap_or_else(|error| panic!("compile failed: {error}\n{}", output.source))
        .into_iter()
        .filter(|run| run.text.contains('o'))
        .map(|run| run.baseline_pt)
        .collect();
    baselines.sort_by(f64::total_cmp);
    baselines.dedup_by(|left, right| (*left - *right).abs() < 0.01);

    assert_eq!(
        baselines.len(),
        2,
        "the grid-rounded tenth word should wrap: {baselines:?}\n{}",
        output.source
    );
    let (top_em, bottom_em) = crate::render::pdf::powerpoint_line_box_em(family)
        .expect("the embedded Libertinus Serif line metrics must resolve");
    let line_height_pt = (top_em + bottom_em) * font_size_pt;
    let expected_first_baseline_pt =
        72.0 + (100.0 - 2.0 * line_height_pt) / 2.0 + top_em * font_size_pt;
    assert!(
        (baselines[0] - expected_first_baseline_pt).abs() < 0.01,
        "advance-grid boxes must preserve the active line box during vertical centring: \
         got {}, expected {expected_first_baseline_pt}; {baselines:?}\n{}",
        baselines[0],
        output.source
    );
    assert!(
        (baselines[1] - baselines[0] - line_height_pt).abs() < 0.01,
        "grid-rounded lines must retain PowerPoint's 1.2em advance: {baselines:?}\n{}",
        output.source
    );
}

/// A box barely one line tall does not scale its text unless the file asked
/// for it (issue #898).
///
/// `single_line_fit_paragraph` folded the paragraph onto one line and scaled
/// it whenever the box was short, whatever `auto_fit` said. The deck in #841
/// gives its sensitivity label a 67.4 x 9.6pt box holding 8pt text and no
/// `<a:normAutofit/>`; we rendered it at 0.61x — an effective 4.9pt — where
/// the reference keeps 8pt and lets it overflow.
#[test]
fn a_short_box_without_autofit_does_not_scale_its_text() {
    let doc = make_doc(vec![make_fixed_page(
        960.0,
        540.0,
        vec![FixedElement {
            x: 5.0,
            y: 525.0,
            width: 67.4,
            height: 9.6,
            kind: FixedElementKind::TextBox(crate::ir::TextBoxData {
                content: vec![Block::Paragraph(Paragraph {
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "Sensitivity: Internal".to_string(),
                        style: TextStyle {
                            font_size: Some(8.0),
                            ..TextStyle::default()
                        },
                        href: None,
                        footnote: None,
                    }],
                })],
                padding: Insets::default(),
                vertical_align: TextBoxVerticalAlign::Top,
                fill: None,
                opacity: None,
                stroke: None,
                shape_kind: None,
                no_wrap: false,
                auto_fit: false,
                text_rotation_deg: None,
                shape_rotation_deg: None,
            }),
        }],
    )]);

    let output = generate_typst(&doc).unwrap();

    assert!(
        !output.source.contains("text_box_scale_"),
        "nothing asked for the text to shrink:\n{}",
        output.source
    );
}
