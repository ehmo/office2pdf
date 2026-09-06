use super::*;

use std::sync::LazyLock;

static NO_THEME_COLORS: LazyLock<std::collections::HashMap<String, Color>> =
    LazyLock::new(std::collections::HashMap::new);
static NO_THEME_ALIASES: LazyLock<std::collections::HashMap<String, String>> =
    LazyLock::new(std::collections::HashMap::new);

/// A chart with no host theme behind it: every `<a:schemeClr>` falls through,
/// which is what these tests want unless they say otherwise.
trait EmptyScheme {
    fn empty() -> SchemeColors<'static>;
}

impl EmptyScheme for SchemeColors<'static> {
    fn empty() -> SchemeColors<'static> {
        SchemeColors {
            colors: &NO_THEME_COLORS,
            aliases: &NO_THEME_ALIASES,
        }
    }
}

#[test]
fn test_parse_bar_chart() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:title><c:tx><c:rich><a:p><a:r><a:t>Sales Data</a:t></a:r></a:p></c:rich></c:tx></c:title>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="col"/>
                        <c:grouping val="clustered"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx>
                            <c:cat>
                                <c:strRef><c:strCache>
                                    <c:pt idx="0"><c:v>Q1</c:v></c:pt>
                                    <c:pt idx="1"><c:v>Q2</c:v></c:pt>
                                    <c:pt idx="2"><c:v>Q3</c:v></c:pt>
                                </c:strCache></c:strRef>
                            </c:cat>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>100</c:v></c:pt>
                                    <c:pt idx="1"><c:v>200</c:v></c:pt>
                                    <c:pt idx="2"><c:v>150</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    assert_eq!(chart.chart_type, ChartType::Column);
    assert_eq!(chart.title.as_deref(), Some("Sales Data"));
    assert_eq!(chart.categories, vec!["Q1", "Q2", "Q3"]);
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name.as_deref(), Some("Revenue"));
    assert_eq!(chart.series[0].values, vec![100.0, 200.0, 150.0]);
}

#[test]
fn test_parse_pie_chart() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:pieChart>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat>
                                <c:strLit>
                                    <c:pt idx="0"><c:v>Apple</c:v></c:pt>
                                    <c:pt idx="1"><c:v>Banana</c:v></c:pt>
                                    <c:pt idx="2"><c:v>Cherry</c:v></c:pt>
                                </c:strLit>
                            </c:cat>
                            <c:val>
                                <c:numLit>
                                    <c:pt idx="0"><c:v>30</c:v></c:pt>
                                    <c:pt idx="1"><c:v>45</c:v></c:pt>
                                    <c:pt idx="2"><c:v>25</c:v></c:pt>
                                </c:numLit>
                            </c:val>
                        </c:ser>
                    </c:pieChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    assert_eq!(chart.chart_type, ChartType::Pie);
    assert!(chart.title.is_none());
    assert_eq!(chart.categories, vec!["Apple", "Banana", "Cherry"]);
    assert_eq!(chart.series[0].values, vec![30.0, 45.0, 25.0]);
}

#[test]
fn test_parse_line_chart_multiple_series() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:title><c:tx><c:rich><a:p><a:r><a:t>Trends</a:t></a:r></a:p></c:rich></c:tx></c:title>
                <c:plotArea>
                    <c:lineChart>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Series A</c:v></c:pt></c:strCache></c:strRef></c:tx>
                            <c:cat>
                                <c:strRef><c:strCache>
                                    <c:pt idx="0"><c:v>Jan</c:v></c:pt>
                                    <c:pt idx="1"><c:v>Feb</c:v></c:pt>
                                </c:strCache></c:strRef>
                            </c:cat>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>10</c:v></c:pt>
                                    <c:pt idx="1"><c:v>20</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                        <c:ser>
                            <c:idx val="1"/>
                            <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Series B</c:v></c:pt></c:strCache></c:strRef></c:tx>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>15</c:v></c:pt>
                                    <c:pt idx="1"><c:v>25</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                    </c:lineChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    assert_eq!(chart.chart_type, ChartType::Line);
    assert_eq!(chart.title.as_deref(), Some("Trends"));
    assert_eq!(chart.categories, vec!["Jan", "Feb"]);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].name.as_deref(), Some("Series A"));
    assert_eq!(chart.series[0].values, vec![10.0, 20.0]);
    assert_eq!(chart.series[1].name.as_deref(), Some("Series B"));
    assert_eq!(chart.series[1].values, vec![15.0, 25.0]);
}

#[test]
fn test_parse_chart_no_title() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="col"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>42</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    assert!(chart.title.is_none());
    assert_eq!(chart.categories, vec!["A"]);
    assert_eq!(chart.series[0].values, vec![42.0]);
}

/// Wrap a `<c:barChart>` body in the chartSpace scaffolding Excel writes.
fn bar_chart_xml(bar_chart_body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:barChart>
                        {bar_chart_body}
                        <c:ser>
                            <c:idx val="0"/>
                            <c:order val="0"/>
                            <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>프로덕션 LOC</c:v></c:pt></c:strCache></c:strRef></c:tx>
                            <c:cat>
                                <c:strRef><c:strCache>
                                    <c:pt idx="0"><c:v>parser</c:v></c:pt>
                                    <c:pt idx="1"><c:v>render</c:v></c:pt>
                                    <c:pt idx="2"><c:v>core</c:v></c:pt>
                                </c:strCache></c:strRef>
                            </c:cat>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>23334</c:v></c:pt>
                                    <c:pt idx="1"><c:v>8331</c:v></c:pt>
                                    <c:pt idx="2"><c:v>4120</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#
    )
}

#[test]
fn test_bar_dir_col_is_a_column_chart() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Column);
    assert_eq!(chart.categories, vec!["parser", "render", "core"]);
    assert_eq!(chart.series[0].values, vec![23334.0, 8331.0, 4120.0]);
}

#[test]
fn test_grouping_stacked_is_read() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="stacked"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.grouping, ChartGrouping::Stacked);
}

#[test]
fn test_grouping_percent_stacked_is_read() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="percentStacked"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.grouping, ChartGrouping::PercentStacked);
}

#[test]
fn test_grouping_clustered_is_read() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.grouping, ChartGrouping::Clustered);
}

#[test]
fn test_grouping_defaults_to_clustered_when_absent() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.grouping, ChartGrouping::Clustered);
}

#[test]
fn test_line_chart_standard_grouping_is_not_stacked() {
    // Line and area charts spell their unstacked form `standard`, which must
    // not fall through to a stacked reading.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:lineChart>
                        <c:grouping val="standard"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>Jan</c:v></c:pt><c:pt idx="1"><c:v>Feb</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="1"><c:v>5</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:lineChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.grouping, ChartGrouping::Clustered);
}

#[test]
fn test_bar_dir_bar_is_a_bar_chart() {
    let xml = bar_chart_xml(r#"<c:barDir val="bar"/><c:grouping val="clustered"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Bar);
}

#[test]
fn test_bar_dir_defaults_to_column_when_absent() {
    // ECMA-376 gives ST_BarDir's `val` a default of `col`, so a chart that omits
    // the required element is read as a column chart rather than rotated.
    let xml = bar_chart_xml(r#"<c:grouping val="clustered"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Column);
}

#[test]
fn test_bar_dir_does_not_leak_into_other_chart_families() {
    // <c:barDir> is exclusive to the bar family; a line chart must stay a line
    // chart even when a bar chart precedes it in the same plot area.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:lineChart>
                        <c:grouping val="standard"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>Jan</c:v></c:pt><c:pt idx="1"><c:v>Feb</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>3</c:v></c:pt><c:pt idx="1"><c:v>5</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:lineChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Line);
}

#[test]
fn test_bar3d_chart_honours_bar_dir() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:bar3DChart>
                        <c:barDir val="bar"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>A</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>7</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:bar3DChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Bar);
}

#[test]
fn test_scan_chart_references() {
    let xml = r#"<?xml version="1.0"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                    xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
                    xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                    xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                    xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
            <w:body>
                <w:p><w:r><w:t>Hello</w:t></w:r></w:p>
                <w:p>
                    <w:r>
                        <w:drawing>
                            <wp:inline>
                                <a:graphic>
                                    <a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
                                        <c:chart r:id="rId4"/>
                                    </a:graphicData>
                                </a:graphic>
                            </wp:inline>
                        </w:drawing>
                    </w:r>
                </w:p>
            </w:body>
        </w:document>"#;

    let refs = scan_chart_references(xml);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].0, 1); // second body child
    assert_eq!(refs[0].1, "rId4");
}

#[test]
fn test_scan_chart_rels() {
    let rels_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
            <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart1.xml"/>
            <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart2.xml"/>
        </Relationships>"#;

    let rels = scan_chart_rels(rels_xml);
    assert_eq!(rels.len(), 2);
    assert_eq!(rels.get("rId4").unwrap(), "word/charts/chart1.xml");
    assert_eq!(rels.get("rId5").unwrap(), "word/charts/chart2.xml");
}

// ----- Legend position (issue #546) -----

/// Wrap a bar chart in a chartSpace carrying `legend_xml` beside the plot area.
fn bar_chart_with_legend(legend_xml: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="col"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>DOCX</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>9</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
                {legend_xml}
            </c:chart>
        </c:chartSpace>"#
    )
}

#[test]
fn test_legend_pos_bottom_is_read() {
    let xml =
        bar_chart_with_legend(r#"<c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.legend_position, LegendPosition::Bottom);
    assert!(chart.legend_position.is_horizontal());
}

#[test]
fn test_explicit_legend_leaf_elements_are_read() {
    let xml = bar_chart_with_legend(
        r#"<c:legend><c:legendPos val="b"></c:legendPos><c:delete val="1"></c:delete></c:legend>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.legend_position, LegendPosition::Bottom);
    assert!(!chart.has_legend);
}

#[test]
fn test_every_legend_pos_value_is_mapped() {
    for (val, expected) in [
        ("b", LegendPosition::Bottom),
        ("l", LegendPosition::Left),
        ("r", LegendPosition::Right),
        ("t", LegendPosition::Top),
        ("tr", LegendPosition::TopRight),
    ] {
        let xml = bar_chart_with_legend(&format!(
            r#"<c:legend><c:legendPos val="{val}"/></c:legend>"#
        ));

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        assert_eq!(chart.legend_position, expected, "legendPos val=\"{val}\"");
    }
}

#[test]
fn test_legend_pos_defaults_to_right_when_absent() {
    // ECMA-376 gives ST_LegendPos a default of `r`, which is also where every
    // legend was drawn before the element was read.
    let xml = bar_chart_with_legend(r#"<c:legend><c:overlay val="0"/></c:legend>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.legend_position, LegendPosition::Right);
    assert!(!chart.legend_position.is_horizontal());
}

#[test]
fn test_chart_without_a_legend_element_keeps_the_default() {
    let xml = bar_chart_with_legend("");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.legend_position, LegendPosition::Right);
}

// ----- Series and data-point fills (issue #535) -----

/// The audited workbook's column chart: one series with an explicit fill.
#[test]
fn test_series_sppr_fill_is_read() {
    let xml = bar_chart_xml(
        r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#,
    )
    .replace(
        "<c:tx>",
        r#"<c:spPr><a:solidFill><a:srgbClr val="4f81bd"/></a:solidFill><a:ln w="9360"><a:solidFill><a:srgbClr val="f9f9f9"/></a:solidFill></a:ln></c:spPr><c:tx>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, Some(Color::new(0x4f, 0x81, 0xbd)));
}

#[test]
fn test_data_point_fills_override_the_series() {
    // The audited pie's three <c:dPt> entries. The third is what exposed the
    // palette: its declared green landed as the palette's yellow.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:pieChart>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:spPr><a:solidFill><a:srgbClr val="111111"/></a:solidFill></c:spPr>
                            <c:dPt><c:idx val="0"/><c:bubble3D val="0"/><c:spPr><a:solidFill><a:srgbClr val="4f81bd"/></a:solidFill></c:spPr></c:dPt>
                            <c:dPt><c:idx val="1"/><c:bubble3D val="0"/><c:spPr><a:solidFill><a:srgbClr val="c0504d"/></a:solidFill></c:spPr></c:dPt>
                            <c:dPt><c:idx val="2"/><c:bubble3D val="0"/><c:spPr><a:solidFill><a:srgbClr val="9bbb59"/></a:solidFill></c:spPr></c:dPt>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>DOCX</c:v></c:pt><c:pt idx="1"><c:v>PPTX</c:v></c:pt><c:pt idx="2"><c:v>XLSX</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>115</c:v></c:pt><c:pt idx="1"><c:v>92</c:v></c:pt><c:pt idx="2"><c:v>138</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:pieChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    let series = &chart.series[0];

    assert_eq!(series.fill_for_point(0), Some(Color::new(0x4f, 0x81, 0xbd)));
    assert_eq!(series.fill_for_point(1), Some(Color::new(0xc0, 0x50, 0x4d)));
    assert_eq!(series.fill_for_point(2), Some(Color::new(0x9b, 0xbb, 0x59)));
    // A point past the declared ones falls back to the series' own fill.
    assert_eq!(series.fill_for_point(3), Some(Color::new(0x11, 0x11, 0x11)));
}

#[test]
fn test_a_series_without_a_fill_leaves_the_palette_to_decide() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, None);
    assert_eq!(chart.series[0].fill_for_point(0), None);
}

#[test]
fn test_a_theme_colour_the_scheme_cannot_resolve_falls_through_to_the_palette() {
    // A host whose theme is missing or does not carry the named entry leaves
    // the colour unresolved, and an unresolved colour must not be mistaken for
    // an explicit one. Resolution itself is covered by
    // `a_series_scheme_color_fill_resolves_against_the_host_theme`.
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#).replace(
        "<c:tx>",
        r#"<c:spPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></c:spPr><c:tx>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, None);
}

#[test]
fn test_a_data_label_fill_is_not_mistaken_for_the_series_fill() {
    // `<c:dLbls>` carries an `<c:spPr>` for the label box. The series-level
    // match is flat, so without consuming the element that fill would be read
    // as the series' own — and this series declares none of its own.
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#).replace(
        "<c:cat>",
        r#"<c:dLbls><c:spPr><a:solidFill><a:srgbClr val="ff0000"/></a:solidFill></c:spPr><c:showVal val="1"/></c:dLbls><c:cat>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, None);
    assert_eq!(chart.series[0].values, vec![23334.0, 8331.0, 4120.0]);
}

#[test]
fn test_a_data_label_fill_does_not_shadow_a_declared_series_fill() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#)
        .replace(
            "<c:tx>",
            r#"<c:spPr><a:solidFill><a:srgbClr val="4f81bd"/></a:solidFill></c:spPr><c:tx>"#,
        )
        .replace(
            "<c:cat>",
            r#"<c:dLbls><c:spPr><a:solidFill><a:srgbClr val="ff0000"/></a:solidFill></c:spPr></c:dLbls><c:cat>"#,
        );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, Some(Color::new(0x4f, 0x81, 0xbd)));
}

// ----- Series point markers (issue #1107) -----

/// The audited workbook's `<c:lineChart>` series carries its marker between
/// `<c:spPr>` and `<c:dLbls>`; `marker_xml` goes in that slot.
fn line_series_with_marker(marker_xml: &str) -> String {
    bar_chart_xml(r#"<c:barDir val="col"/>"#)
        .replace("c:barChart", "c:lineChart")
        .replace("<c:barDir val=\"col\"/>", "")
        .replace("<c:cat>", &format!("{marker_xml}<c:cat>"))
}

#[test]
fn test_a_series_marker_symbol_is_read() {
    // The `Amount Spent` series of the audited workbook, verbatim. It names its
    // symbol, so nothing about the series' position in the chart may decide it
    // (issue #1107).
    let xml = line_series_with_marker(
        r#"<c:marker><c:symbol val="circle"/><c:size val="5"/><c:spPr><a:solidFill><a:srgbClr val="008889"/></a:solidFill></c:spPr></c:marker>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].marker_symbol, Some(MarkerSymbol::Circle));
    assert_eq!(chart.series[0].marker_size_pt, Some(5.0));
}

#[test]
fn test_every_marker_symbol_this_renderer_draws_is_mapped() {
    for (value, expected) in [
        ("none", MarkerSymbol::Off),
        ("circle", MarkerSymbol::Circle),
        ("diamond", MarkerSymbol::Diamond),
        ("square", MarkerSymbol::Square),
        ("triangle", MarkerSymbol::Triangle),
        ("x", MarkerSymbol::Cross),
    ] {
        let xml = line_series_with_marker(&format!(
            r#"<c:marker><c:symbol val="{value}"/></c:marker>"#
        ));

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        assert_eq!(
            chart.series[0].marker_symbol,
            Some(expected),
            "<c:symbol val=\"{value}\"/> must parse as {expected:?}"
        );
    }
}

#[test]
fn test_a_symbol_with_no_shape_here_is_left_to_the_automatic_cycle() {
    // `auto` is the file asking for the automatic symbol outright; the rest are
    // symbols this renderer has no shape for, and substituting another named
    // one would state something the file did not.
    for value in ["auto", "dash", "dot", "plus", "star", "picture"] {
        let xml = line_series_with_marker(&format!(
            r#"<c:marker><c:symbol val="{value}"/></c:marker>"#
        ));

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        assert_eq!(
            chart.series[0].marker_symbol, None,
            "<c:symbol val=\"{value}\"/> has no shape here, so the cycle decides"
        );
    }
}

#[test]
fn test_a_series_without_a_marker_leaves_the_shape_cycle_to_decide() {
    let xml = line_series_with_marker("");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].marker_symbol, None);
}

#[test]
fn test_a_marker_fill_is_not_mistaken_for_the_series_fill() {
    // `<c:marker>` carries an `<c:spPr>` for the symbol's own fill. The
    // series-level match is flat, so without consuming the element that fill
    // would be read as the series' own — and this series declares none.
    let xml = line_series_with_marker(
        r#"<c:marker><c:symbol val="circle"/><c:spPr><a:solidFill><a:srgbClr val="008889"/></a:solidFill></c:spPr></c:marker>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].fill, None);
    assert_eq!(chart.series[0].values, vec![23334.0, 8331.0, 4120.0]);
}

#[test]
fn test_the_line_family_show_markers_flag_is_not_read_as_a_series_symbol() {
    // `<c:marker val="1"/>` beside the `<c:ser>` elements is `CT_Boolean`, not
    // the series' `CT_Marker`. Reading it as a symbol would silence the cycle
    // for every line chart that shows markers at all.
    let xml = line_series_with_marker("")
        .replace("</c:lineChart>", "<c:marker val=\"1\"/></c:lineChart>");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].marker_symbol, None);
    assert_eq!(chart.series[0].values, vec![23334.0, 8331.0, 4120.0]);
}

// ----- Series stroke weight (issue #1113) -----

/// A `<c:ser>` whose own `<c:spPr>` is `sppr_xml`, in a `<c:lineChart>`.
fn line_series_with_shape_properties(sppr_xml: &str) -> String {
    bar_chart_xml(r#"<c:barDir val="col"/>"#)
        .replace("c:barChart", "c:lineChart")
        .replace("<c:barDir val=\"col\"/>", "")
        .replace("<c:tx>", &format!("{sppr_xml}<c:tx>"))
}

#[test]
fn test_a_series_line_width_is_read() {
    // The `Amount Spent` series of the audited workbook, verbatim: 28440 EMU is
    // 2.24pt, against the renderer's flat 2.0pt constant (issue #1113).
    let xml = line_series_with_shape_properties(
        r#"<c:spPr><a:ln w="28440" cap="rnd"><a:solidFill><a:srgbClr val="008889"/></a:solidFill><a:round/></a:ln></c:spPr>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    let width = chart.series[0]
        .line_width_pt
        .expect("a declared width reaches the model");
    assert!(
        (width - 28440.0 / 12700.0).abs() < 1e-9,
        "28440 EMU is {width}pt, expected {}",
        28440.0 / 12700.0
    );
    // The line's own `<a:solidFill>` is still the series colour: a line series
    // states its colour nowhere else.
    assert_eq!(chart.series[0].fill, Some(Color::new(0x00, 0x88, 0x89)));
}

#[test]
fn test_every_declared_series_line_width_is_read_as_stated() {
    // Triangulation: the width is read off the attribute, not matched against
    // the one workbook that reported the defect.
    for (emu, expected_pt) in [(9360.0, 0.7370), (19050.0, 1.5), (28440.0, 2.2394)] {
        let xml = line_series_with_shape_properties(&format!(
            r#"<c:spPr><a:ln w="{emu}"><a:solidFill><a:srgbClr val="008889"/></a:solidFill></a:ln></c:spPr>"#
        ));

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        let width = chart.series[0]
            .line_width_pt
            .expect("a declared width reaches the model");
        assert!(
            (width - expected_pt).abs() < 5e-4,
            "{emu} EMU must reach the model as {expected_pt}pt, got {width}pt"
        );
    }
}

#[test]
fn test_a_series_without_a_line_leaves_the_default_weight_to_decide() {
    let xml = line_series_with_shape_properties(
        r#"<c:spPr><a:solidFill><a:srgbClr val="008889"/></a:solidFill></c:spPr>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].line_width_pt, None);
    assert_eq!(chart.series[0].fill, Some(Color::new(0x00, 0x88, 0x89)));
}

#[test]
fn test_a_line_stating_only_a_colour_leaves_the_default_weight() {
    // `<a:ln>` without `w` states a colour and nothing about weight, so the
    // renderer's default must still apply rather than a zero-width stroke.
    let xml = line_series_with_shape_properties(
        r#"<c:spPr><a:ln><a:solidFill><a:srgbClr val="008889"/></a:solidFill></a:ln></c:spPr>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].line_width_pt, None);
}

#[test]
fn test_a_zero_width_line_is_the_no_outline_idiom_not_a_weight() {
    // `<a:ln w="0"><a:noFill/></a:ln>` is what Office writes for "no outline"
    // — `office2pdf_repository_workbook.xlsx` and `123233_charts.xlsx` both
    // carry it. Reading the 0 as a weight would stroke nothing where the
    // default had been drawn all along.
    let xml = line_series_with_shape_properties(
        r#"<c:spPr><a:solidFill><a:srgbClr val="4f81bd"/></a:solidFill><a:ln w="0"><a:noFill/></a:ln></c:spPr>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].line_width_pt, None);
    assert_eq!(chart.series[0].fill, Some(Color::new(0x4f, 0x81, 0xbd)));
}

#[test]
fn test_a_marker_line_width_is_not_mistaken_for_the_series_line() {
    // `<c:marker><c:spPr><a:ln>` is the symbol's outline, not the plotted
    // line. The series-level match is flat, so without consuming the marker
    // whole that width would be read as the series' own — and this series
    // declares none.
    let xml = line_series_with_shape_properties("").replace(
        "<c:cat>",
        r#"<c:marker><c:symbol val="circle"/><c:spPr><a:ln w="9360"><a:solidFill><a:srgbClr val="ffffff"/></a:solidFill></a:ln></c:spPr></c:marker><c:cat>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series[0].line_width_pt, None);
    assert_eq!(chart.series[0].values, vec![23334.0, 8331.0, 4120.0]);
}

// ----- Unmapped chart families (issue #544) -----

/// Wrap `chart_element` — a whole `<c:fooChart>` — in a chartSpace.
fn chart_of_type(chart_element: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart>
                <c:plotArea>
                    <c:{chart_element}>
                        {body}
                        <c:ser>
                            <c:idx val="0"/>
                            <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Coverage</c:v></c:pt></c:strCache></c:strRef></c:tx>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>Text</c:v></c:pt><c:pt idx="1"><c:v>Table</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>9</c:v></c:pt><c:pt idx="1"><c:v>6</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:{chart_element}>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#
    )
}

#[test]
fn test_a_radar_chart_survives_with_its_data() {
    // The deck's slide 27 loses its entire left half when this returns None.
    let xml = chart_of_type("radarChart", r#"<c:radarStyle val="marker"/>"#);

    let chart =
        parse_chart_xml(&xml, &SchemeColors::empty()).expect("a radar chart must not be dropped");

    assert_eq!(
        chart.chart_type,
        ChartType::Other("Radar Chart".to_string())
    );
    assert_eq!(chart.categories, vec!["Text", "Table"]);
    assert_eq!(chart.series[0].values, vec![9.0, 6.0]);
}

#[test]
fn test_a_doughnut_chart_survives_with_its_data() {
    let xml = chart_of_type("doughnutChart", r#"<c:holeSize val="50"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty())
        .expect("a doughnut chart must not be dropped");

    // It was `Other("Doughnut Chart")` while the tabular fallback stood in for
    // it (issue #544). It is a plotted family now, so it carries its own type
    // and its hole size rather than a caption string (issue #679).
    assert_eq!(chart.chart_type, ChartType::Doughnut);
    assert_eq!(chart.hole_size_percent, Some(50));
    assert_eq!(chart.series[0].values, vec![9.0, 6.0]);
}

/// A doughnut with no `<c:holeSize>` still plots. The parser records the
/// absence as `None`; the renderer substitutes a placeholder rather than
/// treating it as zero, which would collapse the ring into a pie. That
/// placeholder is not a measured default — see `doughnut_inner_radius`.
#[test]
fn test_a_doughnut_chart_without_a_hole_size_still_plots() {
    let xml = chart_of_type("doughnutChart", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty())
        .expect("a doughnut chart must not be dropped");

    assert_eq!(chart.chart_type, ChartType::Doughnut);
    assert_eq!(chart.hole_size_percent, None);
}

/// Triangulation: a pie is not a doughnut, so it carries no hole size and the
/// new field cannot be a constant.
#[test]
fn test_a_pie_chart_carries_no_hole_size() {
    let xml = chart_of_type("pieChart", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("a pie chart parses");

    assert_eq!(chart.chart_type, ChartType::Pie);
    assert_eq!(chart.hole_size_percent, None);
}

#[test]
fn test_every_schema_chart_family_is_recognised() {
    // ECMA-376's full CT_PlotArea group. None may return None.
    for element in [
        "areaChart",
        "area3DChart",
        "lineChart",
        "line3DChart",
        "stockChart",
        "radarChart",
        "scatterChart",
        "pieChart",
        "pie3DChart",
        "doughnutChart",
        "barChart",
        "bar3DChart",
        "ofPieChart",
        "surfaceChart",
        "surface3DChart",
        "bubbleChart",
    ] {
        let xml = chart_of_type(element, "");

        let chart = parse_chart_xml(&xml, &SchemeColors::empty())
            .unwrap_or_else(|| panic!("<c:{element}> must not be dropped"));

        assert_eq!(
            chart.series[0].values,
            vec![9.0, 6.0],
            "<c:{element}> must keep its data"
        );
    }
}

#[test]
fn test_an_unmapped_family_keeps_a_readable_label() {
    // The label is what the data-table fallback prints as the chart's kind.
    for (element, expected) in [
        ("bubbleChart", "Bubble Chart"),
        ("stockChart", "Stock Chart"),
        ("surface3DChart", "Surface Chart"),
    ] {
        let xml = chart_of_type(element, "");

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        assert_eq!(
            chart.chart_type,
            ChartType::Other(expected.to_string()),
            "<c:{element}>"
        );
    }
}

#[test]
fn test_a_non_chart_element_is_still_not_a_chart() {
    // A part with no plot-area family in it yields no chart, so the suffix
    // rule cannot be opening one on `chartSpace` or `chart` themselves.
    let xml = r#"<?xml version="1.0"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
            <c:chart><c:plotArea><c:layout/></c:plotArea></c:chart>
        </c:chartSpace>"#;

    assert!(parse_chart_xml(xml, &SchemeColors::empty()).is_none());
}

#[test]
fn test_of_pie_names_the_shape_its_ofpietype_asks_for() {
    // One element, two shapes. Without reading `<c:ofPieType>` a bar-of-pie
    // chart would be labelled as its sibling.
    for (of_pie_type, expected) in [
        (r#"<c:ofPieType val="bar"/>"#, "Bar of Pie Chart"),
        (r#"<c:ofPieType val="pie"/>"#, "Pie of Pie Chart"),
        // ECMA-376 defaults ST_OfPieType to `pie`.
        ("", "Pie of Pie Chart"),
    ] {
        let xml = chart_of_type("ofPieChart", of_pie_type);

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

        assert_eq!(
            chart.chart_type,
            ChartType::Other(expected.to_string()),
            "ofPieType {of_pie_type:?}"
        );
    }
}

// ----- Axis titles (issue #552) -----

/// The audited workbook's column chart, with `cat_ax_body` and `val_ax_body`
/// spliced into the matching axis element.
fn chart_with_axes(cat_ax_body: &str, val_ax_body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:title><c:tx><c:rich><a:p><a:r><a:t>계층별 프로덕션 LOC</a:t></a:r></a:p></c:rich></c:tx></c:title>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="col"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strLit><c:pt idx="0"><c:v>parser</c:v></c:pt></c:strLit></c:cat>
                            <c:val><c:numLit><c:pt idx="0"><c:v>23334</c:v></c:pt></c:numLit></c:val>
                        </c:ser>
                    </c:barChart>
                    <c:catAx>
                        <c:axId val="59291440"/>
                        <c:axPos val="b"/>
                        {cat_ax_body}
                        <c:tickLblPos val="nextTo"/>
                    </c:catAx>
                    <c:valAx>
                        <c:axId val="21056836"/>
                        <c:axPos val="l"/>
                        {val_ax_body}
                        <c:tickLblPos val="nextTo"/>
                    </c:valAx>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#
    )
}

/// A `<c:title>` as Excel writes it inside an axis.
fn axis_title(text: &str, rotation: &str) -> String {
    format!(
        r#"<c:title><c:tx><c:rich><a:bodyPr rot="{rotation}"/><a:lstStyle/><a:p><a:r><a:rPr b="1" sz="1000"/><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>"#
    )
}

#[test]
fn test_axis_titles_are_read() {
    let xml = chart_with_axes(&axis_title("계층", "0"), &axis_title("LOC", "-5400000"));

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_title.as_deref(), Some("계층"));
    assert_eq!(chart.value_axis_title.as_deref(), Some("LOC"));
    assert_eq!(
        chart.category_axis_title_text_style.rotation_degrees,
        Some(0.0)
    );
    assert_eq!(
        chart.value_axis_title_text_style.rotation_degrees,
        Some(-90.0)
    );
}

#[test]
fn test_an_axis_title_does_not_displace_the_chart_title() {
    // The chart title is the first `<c:title>` in the part; the axis ones come
    // later and must not be mistaken for it, nor it for them.
    let xml = chart_with_axes(&axis_title("계층", "0"), &axis_title("LOC", "-5400000"));

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.title.as_deref(), Some("계층별 프로덕션 LOC"));
}

#[test]
fn test_axes_without_titles_report_none() {
    let xml = chart_with_axes("", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_title, None);
    assert_eq!(chart.value_axis_title, None);
    assert_eq!(chart.title.as_deref(), Some("계층별 프로덕션 LOC"));
}

#[test]
fn test_one_titled_axis_does_not_borrow_the_others_title() {
    let xml = chart_with_axes("", &axis_title("LOC", "-5400000"));

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_title, None);
    assert_eq!(chart.value_axis_title.as_deref(), Some("LOC"));
}

// ----- Major tick marks (issue #672) -----

#[test]
fn test_outward_major_tick_marks_are_read() {
    // Both fixtures in issue #672 spell it this way, and it is what Office
    // writes for a default chart.
    let xml = chart_with_axes(
        r#"<c:majorTickMark val="out"/>"#,
        r#"<c:majorTickMark val="out"/>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_major_tick_mark, AxisTickMark::Outside);
    assert_eq!(chart.value_axis_major_tick_mark, AxisTickMark::Outside);
}

#[test]
fn test_each_axis_keeps_its_own_major_tick_mark() {
    // Triangulation: the two axes are independent, and neither `none` nor `in`
    // may collapse onto the `out` the common case uses.
    let xml = chart_with_axes(
        r#"<c:majorTickMark val="none"/>"#,
        r#"<c:majorTickMark val="in"/>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_major_tick_mark, AxisTickMark::None);
    assert_eq!(chart.value_axis_major_tick_mark, AxisTickMark::Inside);
}

#[test]
fn test_crossing_major_tick_marks_are_read() {
    let xml = chart_with_axes(
        r#"<c:majorTickMark val="cross"/>"#,
        r#"<c:majorTickMark val="cross"/>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_major_tick_mark, AxisTickMark::Cross);
    assert_eq!(chart.value_axis_major_tick_mark, AxisTickMark::Cross);
}

#[test]
fn test_an_axis_without_the_element_still_ticks_outward() {
    // Excel 16.0 exports `tests/fixtures/xlsx/WithChart.xlsx` — written by
    // Apache POI without a single `<c:majorTickMark>` — with 3.17pt outward
    // ticks on both axes, so the rendered default is `out`, not the `cross`
    // ECMA-376 gives the attribute.
    let xml = chart_with_axes("", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.category_axis_major_tick_mark, AxisTickMark::Outside);
    assert_eq!(chart.value_axis_major_tick_mark, AxisTickMark::Outside);
}

// ----- Switched-off axes (issue #672) -----

/// A `<c:delete>` element in the state `on` describes.
fn axis_delete(on: bool) -> String {
    format!(r#"<c:delete val="{}"/>"#, u8::from(on))
}

#[test]
fn test_a_switched_off_axis_is_read_as_deleted() {
    // Office leaves the rest of a switched-off axis' settings in place, tick
    // marks included, so the flag is the only thing that says it is gone.
    let xml = chart_with_axes(
        &format!(r#"{}<c:majorTickMark val="out"/>"#, axis_delete(true)),
        &format!(r#"{}<c:majorTickMark val="out"/>"#, axis_delete(false)),
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(chart.category_axis_deleted);
    assert!(!chart.value_axis_deleted);
    assert_eq!(chart.category_axis_major_tick_mark, AxisTickMark::Outside);
}

#[test]
fn test_the_value_axis_switches_off_independently_of_the_category_one() {
    // Triangulation: a bar chart with only the value axis switched off is the
    // common authoring pattern, so neither flag may stand for both.
    let xml = chart_with_axes(&axis_delete(false), &axis_delete(true));

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(!chart.category_axis_deleted);
    assert!(chart.value_axis_deleted);
}

#[test]
fn test_an_axis_without_the_element_stays_on() {
    let xml = chart_with_axes("", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(!chart.category_axis_deleted);
    assert!(!chart.value_axis_deleted);
}

#[test]
fn test_the_element_without_its_attribute_switches_the_axis_off() {
    // `CT_Boolean/@val` defaults to true, so the attribute is optional.
    let xml = chart_with_axes("<c:delete/>", "");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(chart.category_axis_deleted);
    assert!(!chart.value_axis_deleted);
}

// ----- Data labels (issue #547) -----

/// A bar chart whose series carries `dlbls_xml`.
fn chart_with_data_labels(dlbls_xml: &str) -> String {
    bar_chart_xml(r#"<c:barDir val="col"/>"#).replace("<c:cat>", &format!("{dlbls_xml}<c:cat>"))
}

#[test]
fn test_show_val_turns_the_value_on() {
    // Slide 17's series: `<c:showVal val="1"/>` with everything else off.
    let xml = chart_with_data_labels(
        r#"<c:dLbls><c:dLblPos val="ctr"/><c:showLegendKey val="0"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/></c:dLbls>"#,
    );

    let labels = &parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .series[0]
        .data_labels;

    assert!(labels.show_value);
    assert!(!labels.show_category && !labels.show_series && !labels.show_percent);
    assert!(!labels.is_empty());
}

#[test]
fn test_a_series_without_dlbls_prints_nothing() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#);

    assert!(
        parse_chart_xml(&xml, &SchemeColors::empty())
            .unwrap()
            .series[0]
            .data_labels
            .is_empty()
    );
}

#[test]
fn test_all_dlbls_parts_and_the_separator_are_read() {
    // The workbook's charts turn several on at once and set a separator.
    let xml = chart_with_data_labels(
        r#"<c:dLbls><c:showVal val="1"/><c:showCatName val="1"/><c:showSerName val="1"/><c:showPercent val="1"/><c:separator>; </c:separator></c:dLbls>"#,
    );

    let labels = &parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .series[0]
        .data_labels;

    assert!(labels.show_value && labels.show_category && labels.show_series && labels.show_percent);
    assert_eq!(labels.separator, "; ");
}

#[test]
fn test_a_bare_show_flag_defaults_to_on() {
    // ECMA-376 defaults CT_Boolean's `val` to true, so `<c:showVal/>` counts.
    let xml = chart_with_data_labels(r#"<c:dLbls><c:showVal/></c:dLbls>"#);

    assert!(
        parse_chart_xml(&xml, &SchemeColors::empty())
            .unwrap()
            .series[0]
            .data_labels
            .show_value
    );
}

#[test]
fn test_data_labels_do_not_disturb_the_series_fill() {
    // `<c:dLbls>` is read rather than skipped now, so the guard that keeps its
    // `<c:spPr>` out of the series fill has to still hold.
    let xml = bar_chart_xml(r#"<c:barDir val="col"/>"#).replace(
        "<c:cat>",
        r#"<c:dLbls><c:spPr><a:solidFill><a:srgbClr val="ff0000"/></a:solidFill></c:spPr><c:showVal val="1"/></c:dLbls><c:cat>"#,
    );

    let series = &parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .series[0];

    assert_eq!(
        series.fill, None,
        "the label box fill is not the series fill"
    );
    assert!(series.data_labels.show_value);
}

#[test]
fn test_a_per_point_label_override_is_not_the_series_default() {
    // `CT_DLbls` is `dLbl*` then the group-level settings, and `<c:dLbl>`
    // repeats the same element names. A block carrying only a per-point
    // override must leave the series printing nothing.
    let xml = chart_with_data_labels(
        r#"<c:dLbls><c:dLbl><c:idx val="0"/><c:showVal val="1"/><c:showCatName val="1"/></c:dLbl></c:dLbls>"#,
    );

    let labels = &parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .series[0]
        .data_labels;

    assert!(
        labels.is_empty(),
        "one point's override is not the series default: {labels:?}"
    );
}

#[test]
fn test_group_settings_still_win_after_per_point_overrides() {
    let xml = chart_with_data_labels(
        r#"<c:dLbls><c:dLbl><c:idx val="0"/><c:showCatName val="1"/></c:dLbl><c:showVal val="1"/></c:dLbls>"#,
    );

    let labels = &parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .series[0]
        .data_labels;

    assert!(labels.show_value, "the group-level showVal applies");
    assert!(
        !labels.show_category,
        "the point's showCatName is not the series default: {labels:?}"
    );
}

// ----- Bar band layout (issue #671) -----

/// A column chart declaring `bar_layout_xml` where Office writes it: after the
/// last `</c:ser>`, ahead of the axis ids.
fn chart_with_bar_layout(bar_layout_xml: &str) -> String {
    bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:ser>",
        &format!("</c:ser>{bar_layout_xml}<c:axId val=\"59291440\"/>"),
    )
}

#[test]
fn test_gap_width_and_overlap_are_read_from_the_bar_chart() {
    // What Office writes for a modern clustered chart, and what six of this
    // repository's bar-chart fixtures carry verbatim.
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="219"/><c:overlap val="-27"/>"#);

    let layout = parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(layout.gap_width_percent, 219.0);
    assert_eq!(layout.overlap_percent, -27.0);
}

#[test]
fn test_a_different_declaration_gives_a_different_layout() {
    // Triangulation against the fixture in the issue: `bar-chart.pptx` declares
    // a gap of 100 and no overlap at all.
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="100"/>"#);

    let layout = parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(layout.gap_width_percent, 100.0);
    assert_eq!(layout.overlap_percent, 0.0);
}

#[test]
fn test_a_bar_chart_without_the_elements_takes_the_office_defaults() {
    // `tests/fixtures/xlsx/chart_sheet.xlsx` declares neither element, and
    // Excel 16.0 exports it at exactly 150 / 0.
    let xml = chart_with_bar_layout("");

    let layout = parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(layout.gap_width_percent, 150.0);
    assert_eq!(layout.overlap_percent, 0.0);
}

#[test]
fn test_a_percent_suffixed_amount_is_the_same_number() {
    // `ST_GapAmount` and `ST_Overlap` are unions of a bare integer and a
    // percentage string, so `"90%"` and `"90"` describe the same chart.
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="90%"/><c:overlap val="100%"/>"#);

    let layout = parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(layout.gap_width_percent, 90.0);
    assert_eq!(layout.overlap_percent, 100.0);
}

#[test]
fn test_values_past_the_schema_bounds_are_pulled_back() {
    // PowerPoint 16.0 refuses to open a file whose gapWidth reads 1000 while
    // opening 500 happily, so a value outside `ST_GapAmount` describes no
    // drawable chart and the nearest bound is the honest reading of it.
    let over = chart_with_bar_layout(r#"<c:gapWidth val="1000"/><c:overlap val="400"/>"#);
    let under = chart_with_bar_layout(r#"<c:gapWidth val="-40"/><c:overlap val="-500"/>"#);

    let over_layout = parse_chart_xml(&over, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;
    let under_layout = parse_chart_xml(&under, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(over_layout.gap_width_percent, 500.0);
    assert_eq!(over_layout.overlap_percent, 100.0);
    assert_eq!(under_layout.gap_width_percent, 0.0);
    assert_eq!(under_layout.overlap_percent, -100.0);
}

#[test]
fn test_an_unreadable_amount_leaves_the_default_standing() {
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="wide"/>"#);

    assert_eq!(
        parse_chart_xml(&xml, &SchemeColors::empty())
            .unwrap()
            .bar_band_layout,
        BarBandLayout::default()
    );
}

#[test]
fn test_a_trailing_line_chart_keeps_the_bar_layout() {
    // A combo plot area holds one element per chart family, and only the bar
    // family carries these two. The `<c:lineChart>` that follows must leave
    // what the bars asked for standing.
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="90"/><c:overlap val="100"/>"#).replace(
        "</c:barChart>",
        r#"</c:barChart><c:lineChart><c:grouping val="standard"/><c:ser><c:idx val="1"/><c:val><c:numLit><c:pt idx="0"><c:v>7</c:v></c:pt></c:numLit></c:val></c:ser></c:lineChart>"#,
    );

    let layout = parse_chart_xml(&xml, &SchemeColors::empty())
        .unwrap()
        .bar_band_layout;

    assert_eq!(layout.gap_width_percent, 90.0);
    assert_eq!(layout.overlap_percent, 100.0);
}

// ── `<c:legend>` presence (issue #762) ─────────────────────────────────

/// A declared legend is a legend, whatever edge it names.
#[test]
fn test_declared_legend_is_recorded() {
    let xml =
        bar_chart_with_legend(r#"<c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(chart.has_legend);
    // The position still arrives: reading the legend body must not swallow it.
    assert_eq!(chart.legend_position, LegendPosition::Bottom);
}

/// No `<c:legend>` at all. `legend_position` falls back to its default either
/// way, which is why the presence flag exists.
#[test]
fn test_absent_legend_is_not_recorded() {
    let chart = parse_chart_xml(&bar_chart_with_legend(""), &SchemeColors::empty()).unwrap();

    assert!(!chart.has_legend);
    assert_eq!(chart.legend_position, LegendPosition::Right);
}

/// `<c:delete val="1"/>` switches a declared legend off while keeping its
/// settings, the same shape the axes use.
#[test]
fn test_deleted_legend_is_not_recorded() {
    let xml =
        bar_chart_with_legend(r#"<c:legend><c:legendPos val="b"/><c:delete val="1"/></c:legend>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(!chart.has_legend);
    // Its settings survive, so switching it back on would not lose the edge.
    assert_eq!(chart.legend_position, LegendPosition::Bottom);
}

/// `<c:delete val="0"/>` is an explicit "keep it".
#[test]
fn test_undeleted_legend_is_recorded() {
    let xml =
        bar_chart_with_legend(r#"<c:legend><c:legendPos val="t"/><c:delete val="0"/></c:legend>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(chart.has_legend);
    assert_eq!(chart.legend_position, LegendPosition::Top);
}

/// A minimal chart part, with `body` spliced in after `</c:chart>` — which is
/// where the schema puts `c:chartSpace/c:spPr`.
fn chart_space_with(trailing: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:spPr><a:ln w="76200"><a:solidFill><a:srgbClr val="ff0000"/></a:solidFill></a:ln></c:spPr>
                    <c:lineChart>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:val><c:numRef><c:numCache>
                                <c:pt idx="0"><c:v>1</c:v></c:pt>
                                <c:pt idx="1"><c:v>2</c:v></c:pt>
                            </c:numCache></c:numRef></c:val>
                        </c:ser>
                    </c:lineChart>
                </c:plotArea>
            </c:chart>
            {trailing}
        </c:chartSpace>"#
    )
}

#[test]
fn a_chart_with_no_chart_space_line_takes_the_default_outline() {
    // The plot area in the fixture above declares a fat red `a:ln` of its own.
    // The event loop is flat, so reading the first `c:spPr` it meets would pick
    // that one up and report the chart area as red (#637).
    let chart =
        parse_chart_xml(&chart_space_with(""), &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.chart_area_fill, ChartAreaFill::Unspecified);
    assert_eq!(chart.chart_area_outline, ChartAreaOutline::Default);
}

#[test]
fn a_chart_space_no_fill_is_distinct_from_an_unspecified_fill() {
    let chart = parse_chart_xml(
        &chart_space_with("<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>"),
        &SchemeColors::empty(),
    )
    .expect("chart parses");

    assert_eq!(chart.chart_area_fill, ChartAreaFill::Transparent);
    assert_eq!(chart.chart_area_outline, ChartAreaOutline::Suppressed);
}

#[test]
fn a_chart_space_fill_does_not_take_the_lines_colour() {
    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:spPr>
                 <a:solidFill><a:srgbClr val="123456"/></a:solidFill>
                 <a:ln w="9360"><a:solidFill><a:srgbClr val="d9d9d9"/></a:solidFill></a:ln>
               </c:spPr>"#,
        ),
        &SchemeColors::empty(),
    )
    .expect("chart parses");

    assert_eq!(
        chart.chart_area_fill,
        ChartAreaFill::Solid(Color::new(0x12, 0x34, 0x56))
    );
    assert_eq!(
        chart.chart_area_outline,
        ChartAreaOutline::Explicit {
            width_pt: Some(9360.0 / 12700.0),
            color: Some(Color::new(0xd9, 0xd9, 0xd9)),
            round_join: false,
        }
    );
}

#[test]
fn a_chart_space_fill_resolves_a_transformed_theme_colour() {
    let colors: std::collections::HashMap<String, Color> =
        [("bg1".to_string(), Color::new(0xFF, 0xFF, 0xFF))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };
    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:spPr><a:solidFill><a:schemeClr val="bg1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill></c:spPr>"#,
        ),
        &scheme,
    )
    .expect("chart parses");

    assert_eq!(
        chart.chart_area_fill,
        ChartAreaFill::Solid(Color::new(128, 128, 128))
    );
    assert_eq!(chart.chart_area_outline, ChartAreaOutline::Default);
}

#[test]
fn a_chart_space_no_fill_suppresses_the_outline() {
    let chart = parse_chart_xml(
        &chart_space_with("<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>"),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(chart.chart_area_outline, ChartAreaOutline::Suppressed);
}

#[test]
fn a_chart_space_line_keeps_its_width_and_colour() {
    // 9360 EMU is the width `office2pdf_repository_workbook.xlsx` declares, and
    // #d9d9d9 its colour.
    let chart = parse_chart_xml(&chart_space_with(
        r#"<c:spPr><a:ln w="9360"><a:solidFill><a:srgbClr val="d9d9d9"/></a:solidFill></a:ln></c:spPr>"#,
    ), &SchemeColors::empty())
    .expect("chart parses");
    match chart.chart_area_outline {
        ChartAreaOutline::Explicit {
            width_pt,
            color,
            round_join,
        } => {
            let width = width_pt.expect("a declared width reaches the model");
            assert!(
                (width - 9360.0 / 12700.0).abs() < 1e-9,
                "9360 EMU is {width}pt, expected {}",
                9360.0 / 12700.0
            );
            assert_eq!(color, Some(Color::new(0xd9, 0xd9, 0xd9)));
            assert!(!round_join);
        }
        other => panic!("expected an explicit outline, got {other:?}"),
    }
}

/// The chart-area outline resolves a theme colour on the same chain a series'
/// fill does — it is the same `<a:solidFill>` markup in the same part (#876).
#[test]
fn a_chart_space_line_resolves_a_theme_colour() {
    let colors: std::collections::HashMap<String, Color> =
        [("bg1".to_string(), Color::new(0xFF, 0xFF, 0xFF))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:spPr><a:ln w="9360"><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></a:ln></c:spPr>"#,
        ),
        &scheme,
    )
    .expect("chart parses");
    match chart.chart_area_outline {
        ChartAreaOutline::Explicit { color, .. } => {
            assert_eq!(color, Some(Color::new(0xFF, 0xFF, 0xFF)));
        }
        other => panic!("expected an explicit outline, got {other:?}"),
    }
}

// ----- Chart text's declared face (issue #668) -----

#[test]
fn a_chart_space_tx_pr_latin_typeface_reaches_the_model() {
    // `office2pdf_introduction_ko.pptx`'s chart1.xml names the face outright
    // rather than deferring to the theme.
    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr>
             <a:defRPr><a:latin typeface="Calibri"/></a:defRPr>
           </a:pPr><a:endParaRPr lang="en-US"/></a:p></c:txPr>"#,
        ),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(chart.text_font_family.as_deref(), Some("Calibri"));
}

#[test]
fn a_chart_space_tx_pr_keeps_the_unresolved_theme_token() {
    // The chart part names no theme of its own, so the parser cannot turn
    // `+mn-lt` into a face. Whoever loaded the part resolves it.
    let chart = parse_chart_xml(&chart_space_with(
        r#"<c:txPr><a:p><a:pPr><a:defRPr><a:latin typeface="+mn-lt"/></a:defRPr></a:pPr></a:p></c:txPr>"#,
    ), &SchemeColors::empty())
    .expect("chart parses");
    assert_eq!(chart.text_font_family.as_deref(), Some("+mn-lt"));
}

#[test]
fn a_chart_with_no_tx_pr_names_no_face() {
    // `bar-chart.pptx` is this shape: the face has to come from the theme's
    // minor font, which only the loader knows.
    let chart =
        parse_chart_xml(&chart_space_with(""), &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.text_font_family, None);
}

#[test]
fn a_plot_area_tx_pr_is_not_read_as_the_chart_space_one() {
    // `c:txPr` is a sibling of `c:chart` written after it, exactly like
    // `c:spPr`. A flat event loop that does not wait for `</c:chart>` would
    // pick up an axis' own `c:txPr` instead (#637 made the same mistake).
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:lineChart><c:ser><c:idx val="0"/>
                        <c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val>
                    </c:ser></c:lineChart>
                    <c:catAx>
                        <c:txPr><a:p><a:pPr><a:defRPr><a:latin typeface="Impact"/></a:defRPr></a:pPr></a:p></c:txPr>
                    </c:catAx>
                </c:plotArea>
            </c:chart>
            <c:txPr><a:p><a:pPr><a:defRPr><a:latin typeface="Calibri"/></a:defRPr></a:pPr></a:p></c:txPr>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.text_font_family.as_deref(), Some("Calibri"));
}

// ----- Run properties declared in c:txPr (issue #669) -----

#[test]
fn a_chart_space_tx_pr_size_reaches_the_model() {
    // `bar-chart.pptx` sets all chart text to 18pt this way.
    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1800"/></a:pPr>
           <a:endParaRPr lang="en-US"/></a:p></c:txPr>"#,
        ),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(chart.text_style.size_pt, Some(18.0));
    assert_eq!(chart.text_style.bold, None);
}

#[test]
fn chart_text_character_spacing_is_read_in_hundredths_of_a_point() {
    let chart = parse_chart_xml(
        &chart_space_with(r#"<c:txPr><a:p><a:pPr><a:defRPr spc="-125"/></a:pPr></a:p></c:txPr>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(chart.text_style.letter_spacing_hundredths, Some(-125));
}

#[test]
fn chart_text_kerning_threshold_is_read_in_points() {
    let chart = parse_chart_xml(
        &chart_space_with(r#"<c:txPr><a:p><a:pPr><a:defRPr kern="1200"/></a:pPr></a:p></c:txPr>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(
        chart.text_style.pair_kerning,
        Some(crate::ir::PairKerning::AtOrAbovePt(12.0))
    );
}

#[test]
fn chart_text_zero_kerning_threshold_means_never() {
    let chart = parse_chart_xml(
        &chart_space_with(r#"<c:txPr><a:p><a:pPr><a:defRPr kern="0"/></a:pPr></a:p></c:txPr>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(
        chart.text_style.pair_kerning,
        Some(crate::ir::PairKerning::Never)
    );
}

#[test]
fn a_chart_space_tx_pr_bold_reaches_the_model() {
    let chart = parse_chart_xml(
        &chart_space_with(
            r#"<c:txPr><a:p><a:pPr><a:defRPr sz="1200" b="1"/></a:pPr></a:p></c:txPr>"#,
        ),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(chart.text_style.size_pt, Some(12.0));
    assert_eq!(chart.text_style.bold, Some(true));
}

#[test]
fn a_chart_with_no_tx_pr_declares_no_run_properties() {
    let chart =
        parse_chart_xml(&chart_space_with(""), &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.text_style, crate::ir::ChartTextStyle::default());
}

#[test]
fn an_axis_tx_pr_overrides_the_chart_space_one() {
    // `office2pdf_introduction_ko.pptx` page 17 bolds only the category labels.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:barChart><c:barDir val="col"/><c:ser><c:idx val="0"/>
                        <c:cat><c:strLit><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strLit></c:cat>
                        <c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val>
                    </c:ser></c:barChart>
                    <c:catAx>
                        <c:txPr><a:p><a:pPr><a:defRPr sz="1100" b="1" spc="100"/></a:pPr></a:p></c:txPr>
                    </c:catAx>
                    <c:valAx>
                        <c:txPr><a:p><a:pPr><a:defRPr sz="900"/></a:pPr></a:p></c:txPr>
                    </c:valAx>
                </c:plotArea>
            </c:chart>
            <c:txPr><a:p><a:pPr><a:defRPr sz="1800"/></a:pPr></a:p></c:txPr>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.text_style.size_pt, Some(18.0));
    assert_eq!(chart.category_axis_text_style.size_pt, Some(11.0));
    assert_eq!(chart.category_axis_text_style.bold, Some(true));
    assert_eq!(
        chart.category_axis_text_style.letter_spacing_hundredths,
        Some(100)
    );
    assert_eq!(chart.value_axis_text_style.size_pt, Some(9.0));
    assert_eq!(chart.value_axis_text_style.bold, None);
    assert_eq!(chart.value_axis_text_style.letter_spacing_hundredths, None);
}

#[test]
fn axis_titles_keep_their_own_run_properties() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea>
                <c:barChart><c:barDir val="col"/></c:barChart>
                <c:catAx><c:title><c:tx><c:rich><a:p><a:r><a:t>Quarter</a:t></a:r></a:p></c:rich></c:tx>
                    <c:txPr><a:p><a:pPr><a:defRPr sz="800" b="0">
                        <a:solidFill><a:srgbClr val="123456"/></a:solidFill>
                        <a:latin typeface="Arial"/>
                    </a:defRPr></a:pPr></a:p></c:txPr>
                </c:title></c:catAx>
                <c:valAx><c:title><c:tx><c:rich><a:p><a:r><a:t>Revenue</a:t></a:r></a:p></c:rich></c:tx>
                    <c:txPr><a:p><a:pPr><a:defRPr sz="1100" b="1"><a:latin typeface="Aptos"/></a:defRPr></a:pPr></a:p></c:txPr>
                </c:title></c:valAx>
            </c:plotArea></c:chart>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.category_axis_title.as_deref(), Some("Quarter"));
    assert_eq!(chart.category_axis_title_text_style.size_pt, Some(8.0));
    assert_eq!(
        chart.category_axis_title_text_style.font_family.as_deref(),
        Some("Arial")
    );
    assert_eq!(chart.category_axis_title_text_style.bold, Some(false));
    assert_eq!(
        chart.category_axis_title_text_style.color,
        Some(Color::new(0x12, 0x34, 0x56))
    );
    assert_eq!(chart.value_axis_title.as_deref(), Some("Revenue"));
    assert_eq!(chart.value_axis_title_text_style.size_pt, Some(11.0));
    assert_eq!(
        chart.value_axis_title_text_style.font_family.as_deref(),
        Some("Aptos")
    );
    assert_eq!(chart.value_axis_title_text_style.bold, Some(true));
}

#[test]
fn package_theme_resolves_every_chart_text_scope() {
    let mut chart =
        parse_chart_xml(&chart_space_with(""), &SchemeColors::empty()).expect("chart parses");
    chart.text_font_family = Some("+mn-lt".to_string());
    chart.text_style.font_family = Some("+mn-lt".to_string());
    chart.title_text_style.font_family = Some("+mj-lt".to_string());
    chart.legend_text_style.font_family = Some("+mn-lt".to_string());
    chart.category_axis_text_style.font_family = Some("Arial".to_string());
    chart.value_axis_text_style.font_family = Some("+mj-lt".to_string());
    chart.category_axis_title_text_style.font_family = Some("+mn-lt".to_string());
    chart.value_axis_title_text_style.font_family = Some("+mj-lt".to_string());
    chart.series[0].data_labels.text_style.font_family = Some("+mn-lt".to_string());
    let theme = crate::parser::drawingml::ThemeFontScheme {
        major_latin: Some("Aptos Display".to_string()),
        minor_latin: Some("Aptos".to_string()),
    };

    resolve_chart_text_fonts(&mut chart, &theme);

    assert_eq!(chart.text_font_family.as_deref(), Some("Aptos"));
    assert_eq!(chart.text_style.font_family.as_deref(), Some("Aptos"));
    assert_eq!(
        chart.title_text_style.font_family.as_deref(),
        Some("Aptos Display")
    );
    assert_eq!(
        chart.legend_text_style.font_family.as_deref(),
        Some("Aptos")
    );
    assert_eq!(
        chart.category_axis_text_style.font_family.as_deref(),
        Some("Arial")
    );
    assert_eq!(
        chart.value_axis_text_style.font_family.as_deref(),
        Some("Aptos Display")
    );
    assert_eq!(
        chart.category_axis_title_text_style.font_family.as_deref(),
        Some("Aptos")
    );
    assert_eq!(
        chart.value_axis_title_text_style.font_family.as_deref(),
        Some("Aptos Display")
    );
    assert_eq!(
        chart.series[0]
            .data_labels
            .text_style
            .font_family
            .as_deref(),
        Some("Aptos")
    );
}

#[test]
fn rich_axis_title_run_properties_reach_the_model() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea>
                <c:barChart><c:barDir val="col"/></c:barChart>
                <c:catAx><c:title><c:tx><c:rich><a:bodyPr/><a:p><a:pPr>
                    <a:defRPr sz="900" b="1"><a:latin typeface="Calibri"/></a:defRPr>
                </a:pPr><a:r><a:rPr sz="800" b="0">
                    <a:solidFill><a:srgbClr val="123456"/></a:solidFill>
                    <a:latin typeface="Arial"/>
                </a:rPr><a:t>Quarter</a:t></a:r></a:p></c:rich></c:tx></c:title></c:catAx>
            </c:plotArea></c:chart>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.category_axis_title_text_style.size_pt, Some(8.0));
    assert_eq!(chart.category_axis_title_text_style.bold, Some(false));
    assert_eq!(
        chart.category_axis_title_text_style.font_family.as_deref(),
        Some("Arial")
    );
    assert_eq!(
        chart.category_axis_title_text_style.color,
        Some(Color::new(0x12, 0x34, 0x56))
    );
}

#[test]
fn a_legend_tx_pr_keeps_its_own_run_properties() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea>
                <c:legend><c:legendPos val="b"/><c:txPr><a:p><a:pPr>
                    <a:defRPr sz="1700" b="1" spc="125">
                        <a:solidFill><a:srgbClr val="C02A7A"/></a:solidFill>
                    </a:defRPr>
                </a:pPr></a:p></c:txPr></c:legend>
            </c:chart>
            <c:txPr><a:p><a:pPr><a:defRPr sz="1200" b="0"/></a:pPr></a:p></c:txPr>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");

    assert!(chart.has_legend);
    assert_eq!(chart.legend_position, LegendPosition::Bottom);
    assert_eq!(chart.legend_text_style.size_pt, Some(17.0));
    assert_eq!(chart.legend_text_style.bold, Some(true));
    assert_eq!(chart.legend_text_style.letter_spacing_hundredths, Some(125));
    assert_eq!(
        chart.legend_text_style.color,
        Some(Color::new(0xC0, 0x2A, 0x7A))
    );
    assert_eq!(
        chart.text_style.resolved_size_pt(&chart.legend_text_style),
        Some(17.0),
        "the legend overrides the chart-space 12pt size"
    );
}

#[test]
fn a_legend_entry_tx_pr_is_not_promoted_to_the_whole_legend() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea><c:barChart><c:barDir val="col"/></c:barChart></c:plotArea>
                <c:legend>
                    <c:legendPos val="b"/>
                    <c:legendEntry><c:idx val="0"/><c:txPr><a:p><a:pPr>
                        <a:defRPr sz="1700" b="1">
                            <a:solidFill><a:srgbClr val="C02A7A"/></a:solidFill>
                        </a:defRPr>
                    </a:pPr></a:p></c:txPr></c:legendEntry>
                </c:legend>
            </c:chart>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.legend_text_style, ChartTextStyle::default());
}

#[test]
fn an_axis_preserves_its_ellipsis_overflow_policy() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea>
                <c:barChart><c:barDir val="col"/></c:barChart>
                <c:catAx><c:txPr><a:bodyPr vertOverflow="ellipsis"/>
                    <a:p><a:pPr><a:defRPr sz="1197"/></a:pPr></a:p>
                </c:txPr></c:catAx>
            </c:plotArea></c:chart>
        </c:chartSpace>"#;
    let chart = parse_chart_xml(xml, &SchemeColors::empty()).expect("chart parses");
    assert!(chart.category_axis_text_style.ellipsis_overflow);
    assert_eq!(chart.category_axis_text_style.size_pt, Some(11.97));
}

#[test]
fn an_axis_declaring_no_tx_pr_inherits_the_chart_space_one() {
    let chart = parse_chart_xml(
        &chart_space_with(r#"<c:txPr><a:p><a:pPr><a:defRPr sz="1800"/></a:pPr></a:p></c:txPr>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert_eq!(
        chart.category_axis_text_style,
        crate::ir::ChartTextStyle::default()
    );
    // Resolution against the chart-space default is the renderer's job; the
    // parser reports only what each element actually declared.
    assert_eq!(
        chart
            .text_style
            .resolved_size_pt(&chart.category_axis_text_style),
        Some(18.0)
    );
}

// ----- A run colour's transform children (issue #1160) -----

/// The `<a:defRPr>` Excel writes for every chart string in
/// `Gift Budget and Tracker1.xlsx`: a scheme colour whose luminance transforms
/// are children of the colour element, followed by the face.
fn def_rpr_with_text_slot_colour() -> &'static str {
    r#"<a:defRPr sz="900" b="0" i="0" u="none" strike="noStrike" kern="1200" baseline="0">
           <a:solidFill><a:schemeClr val="tx1"><a:lumMod val="65000"/><a:lumOff val="35000"/></a:schemeClr></a:solidFill>
           <a:latin typeface="Segoe UI"/>
       </a:defRPr>"#
}

/// A theme keying its slots the way `<a:clrScheme>` does — a workbook carries
/// no `<p:clrMap>`, so `tx1` reaches `dk1` through the implicit pairing.
fn black_text_slot_scheme() -> (
    std::collections::HashMap<String, Color>,
    std::collections::HashMap<String, String>,
) {
    (
        [("dk1".to_string(), Color::new(0, 0, 0))]
            .into_iter()
            .collect(),
        std::collections::HashMap::new(),
    )
}

/// `<a:schemeClr val="tx1">` carrying lumMod 65% / lumOff 35% is #595959, not
/// black: the transforms are the colour element's children, so reading only
/// its start tag drops them and every chart string prints a shade too dark
/// (issue #1160).
#[test]
fn a_chart_text_colour_applies_its_luminance_transforms() {
    let (colors, aliases) = black_text_slot_scheme();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };
    let xml = chart_space_with(&format!(
        "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr>{}</a:pPr><a:endParaRPr lang=\"en-US\"/></a:p></c:txPr>",
        def_rpr_with_text_slot_colour()
    ));

    let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");

    assert_eq!(chart.text_style.color, Some(Color::new(0x59, 0x59, 0x59)));
}

/// Consuming the colour element must not swallow what follows it inside the
/// same `<a:defRPr>`: Excel writes `<a:latin>` after `<a:solidFill>`.
#[test]
fn a_chart_text_colour_leaves_the_face_after_it_readable() {
    let (colors, aliases) = black_text_slot_scheme();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };
    let xml = chart_space_with(&format!(
        "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr>{}</a:pPr></a:p></c:txPr>",
        def_rpr_with_text_slot_colour()
    ));

    let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");

    assert_eq!(chart.text_font_family.as_deref(), Some("Segoe UI"));
    assert_eq!(chart.text_style.size_pt, Some(9.0));
}

/// An axis' own `c:txPr` reads the transforms the same way — the chart in
/// #1160 states them on every scope, not just the chart space.
#[test]
fn an_axis_text_colour_applies_its_luminance_transforms() {
    let (colors, aliases) = black_text_slot_scheme();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };
    let tx_pr = format!(
        "<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr>{}</a:pPr></a:p></c:txPr>",
        def_rpr_with_text_slot_colour()
    );
    let xml = chart_space_with("").replace(
        "</c:plotArea>",
        &format!(
            "<c:catAx><c:axId val=\"1\"/>{tx_pr}</c:catAx><c:valAx><c:axId val=\"2\"/>{tx_pr}</c:valAx></c:plotArea>"
        ),
    );

    let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");

    let lifted = Some(Color::new(0x59, 0x59, 0x59));
    assert_eq!(chart.category_axis_text_style.color, lifted);
    assert_eq!(chart.value_axis_text_style.color, lifted);
}

/// Triangulation: a colour element with no children is still read from its own
/// attributes, and one stating a transform-free scheme slot still resolves to
/// the theme colour itself.
#[test]
fn a_chart_text_colour_without_transforms_is_unchanged() {
    let (colors, aliases) = black_text_slot_scheme();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    for (fill, expected) in [
        (
            r#"<a:srgbClr val="C6FC15"/>"#,
            Some(Color::new(0xC6, 0xFC, 0x15)),
        ),
        (r#"<a:schemeClr val="tx1"/>"#, Some(Color::new(0, 0, 0))),
        (
            r#"<a:srgbClr val="C6FC15"><a:alpha val="100000"/></a:srgbClr>"#,
            Some(Color::new(0xC6, 0xFC, 0x15)),
        ),
    ] {
        let xml = chart_space_with(&format!(
            "<c:txPr><a:p><a:pPr><a:defRPr sz=\"900\"><a:solidFill>{fill}</a:solidFill></a:defRPr></a:pPr></a:p></c:txPr>"
        ));
        let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");
        assert_eq!(chart.text_style.color, expected, "fill {fill}");
        assert_eq!(chart.text_style.size_pt, Some(9.0), "fill {fill}");
    }
}

/// `<c:formatCode>` inside the numeric cache is how a chart states that its
/// values are percentages, currency or dates. Without it the data-table
/// fallback printed the stored fraction — `0.024` where the source, and every
/// other renderer, shows `2.4%` (issue #865).
fn percent_chart_xml(format_code: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea><c:bubbleChart>
                <c:ser>
                    <c:idx val="0"/>
                    <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Rate</c:v></c:pt></c:strCache></c:strRef></c:tx>
                    <c:cat><c:strRef><c:strCache>
                        <c:pt idx="0"><c:v>Q1</c:v></c:pt>
                        <c:pt idx="1"><c:v>Q2</c:v></c:pt>
                    </c:strCache></c:strRef></c:cat>
                    <c:val><c:numRef><c:numCache>
                        <c:formatCode>{format_code}</c:formatCode>
                        <c:pt idx="0"><c:v>0.024</c:v></c:pt>
                        <c:pt idx="1"><c:v>0.689</c:v></c:pt>
                    </c:numCache></c:numRef></c:val>
                </c:ser>
            </c:bubbleChart></c:plotArea></c:chart>
        </c:chartSpace>"#
    )
}

#[test]
fn test_series_carries_its_number_format() {
    let chart =
        parse_chart_xml(&percent_chart_xml("0.0%"), &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.series[0].number_format.as_deref(), Some("0.0%"));
    assert_eq!(chart.series[0].values, vec![0.024, 0.689]);
}

/// A different code is read as written, so the value is not assumed to be a
/// percentage.
#[test]
fn test_a_declared_currency_format_is_read_as_written() {
    let chart = parse_chart_xml(&percent_chart_xml("#,##0.00"), &SchemeColors::empty())
        .expect("chart parses");
    assert_eq!(chart.series[0].number_format.as_deref(), Some("#,##0.00"));
}

/// A cache that states no format leaves the field unset, so the renderer keeps
/// its plain rendering.
#[test]
fn test_a_series_without_a_format_code_states_none() {
    let xml = percent_chart_xml("0%").replace("<c:formatCode>0%</c:formatCode>", "");
    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.series[0].number_format, None);
}

/// `General` is Excel's "no format", and applying it would only reformat the
/// number the plain path already prints.
#[test]
fn test_a_general_format_code_states_none() {
    let chart = parse_chart_xml(&percent_chart_xml("General"), &SchemeColors::empty())
        .expect("chart parses");
    assert_eq!(chart.series[0].number_format, None);
}

/// `<c:valAx><c:numFmt>` and `<c:dLbls><c:numFmt>` are what the chart states
/// for its tick labels and its data labels. They outrank the numeric cache's
/// `formatCode`, which is the source cell's own — the deck on #841 declares
/// `0.00%` in the cache but `0%` on the axis and `0.0%` on the labels, and the
/// reference prints the latter two (issue #865).
const AXIS_AND_LABEL_FORMAT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
    <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
        <c:chart><c:plotArea>
            <c:barChart>
                <c:barDir val="col"/>
                <c:ser>
                    <c:idx val="0"/>
                    <c:dLbls><c:numFmt formatCode="0.0%" sourceLinked="0"/><c:showVal val="1"/></c:dLbls>
                    <c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat>
                    <c:val><c:numRef><c:numCache>
                        <c:formatCode>0.00%</c:formatCode>
                        <c:pt idx="0"><c:v>0.024</c:v></c:pt>
                    </c:numCache></c:numRef></c:val>
                </c:ser>
            </c:barChart>
            <c:catAx><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/></c:catAx>
            <c:valAx><c:axPos val="l"/><c:numFmt formatCode="0%" sourceLinked="0"/></c:valAx>
        </c:plotArea></c:chart>
    </c:chartSpace>"#;

#[test]
fn test_value_axis_number_format_is_read() {
    let chart =
        parse_chart_xml(AXIS_AND_LABEL_FORMAT_XML, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.value_axis_number_format.as_deref(), Some("0%"));
}

#[test]
fn test_data_label_number_format_is_read() {
    let chart =
        parse_chart_xml(AXIS_AND_LABEL_FORMAT_XML, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(
        chart.series[0].data_labels.number_format.as_deref(),
        Some("0.0%")
    );
}

/// The category axis states `General`, which is Excel's "no format" — it must
/// not be mistaken for the value axis' code, and it must not reach the chart.
#[test]
fn test_a_general_axis_format_states_none() {
    let xml =
        AXIS_AND_LABEL_FORMAT_XML.replace(r#"<c:numFmt formatCode="0%" sourceLinked="0"/>"#, "");
    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.value_axis_number_format, None);
}

/// The cache format survives alongside them, so the data table still has an
/// answer where no axis or label format is stated.
#[test]
fn test_the_cache_format_is_kept_beside_the_declared_ones() {
    let chart =
        parse_chart_xml(AXIS_AND_LABEL_FORMAT_XML, &SchemeColors::empty()).expect("chart parses");
    assert_eq!(chart.series[0].number_format.as_deref(), Some("0.00%"));
}

/// `<c:autoTitleDeleted val="1"/>` is how a chart declines the automatic title
/// Office would otherwise draw — from its single series' name here, or from
/// the placeholder where nothing names one (issues #883 and #1146).
fn single_series_chart_xml(extra: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>{extra}<c:plotArea><c:barChart>
                <c:barDir val="col"/>
                <c:ser>
                    <c:idx val="0"/>
                    <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Serie 1</c:v></c:pt></c:strCache></c:strRef></c:tx>
                    <c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat>
                    <c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val>
                </c:ser>
            </c:barChart></c:plotArea></c:chart>
        </c:chartSpace>"#
    )
}

#[test]
fn test_auto_title_deleted_is_read() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(r#"<c:autoTitleDeleted val="1"/>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert!(chart.auto_title_deleted);
}

#[test]
fn test_explicit_auto_title_deleted_leaf_is_read() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(r#"<c:autoTitleDeleted val="1"></c:autoTitleDeleted>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert!(chart.auto_title_deleted);
}

/// A chart that says nothing keeps the automatic title, so the flag is read
/// rather than assumed.
#[test]
fn test_a_chart_without_the_flag_keeps_its_automatic_title() {
    let chart = parse_chart_xml(&single_series_chart_xml(""), &SchemeColors::empty())
        .expect("chart parses");
    assert!(!chart.auto_title_deleted);
}

/// `val="0"` is the explicit "keep it", which must not read as deleted.
#[test]
fn test_auto_title_deleted_zero_keeps_the_title() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(r#"<c:autoTitleDeleted val="0"/>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");
    assert!(!chart.auto_title_deleted);
}

/// A `<c:title>` that carries no `<c:tx>` is Office's *automatic* title: the
/// element supplies the formatting and the application supplies the string.
/// `tests/fixtures/xlsx/any_sheets.xlsx` writes exactly that — a `c:layout`,
/// an `c:overlay`, an `c:spPr` and a `c:txPr`, and no text anywhere (#1146).
const AUTOMATIC_TITLE_XML: &str = r#"<c:title><c:layout/><c:overlay val="0"/>
    <c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1400"/></a:pPr>
    <a:endParaRPr lang="ru-RU"/></a:p></c:txPr></c:title>"#;

#[test]
fn test_a_title_naming_no_text_is_automatic() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(&format!(
            r#"{AUTOMATIC_TITLE_XML}<c:autoTitleDeleted val="0"/>"#
        )),
        &SchemeColors::empty(),
    )
    .expect("chart parses");

    assert!(chart.has_automatic_title);
    assert_eq!(chart.title, None);
}

/// Triangulation: a title that names its own text is not automatic, so the
/// flag cannot simply follow the element's presence.
#[test]
fn test_a_title_naming_its_own_text_is_not_automatic() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(
            r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>Quarterly sales</a:t></a:r></a:p></c:rich></c:tx></c:title>"#,
        ),
        &SchemeColors::empty(),
    )
    .expect("chart parses");

    assert!(!chart.has_automatic_title);
    assert_eq!(chart.title.as_deref(), Some("Quarterly sales"));
}

/// A `<c:title/>` with no children at all names no text either, so it asks for
/// the same automatic title the formatted one does.
#[test]
fn test_an_empty_title_element_is_automatic() {
    let chart = parse_chart_xml(
        &single_series_chart_xml(r#"<c:title/><c:autoTitleDeleted val="0"/>"#),
        &SchemeColors::empty(),
    )
    .expect("chart parses");

    assert!(chart.has_automatic_title);
    assert_eq!(chart.title, None);
}

/// A chart carrying no `<c:title>` at all declares no automatic title either.
#[test]
fn test_a_chart_without_a_title_element_declares_no_automatic_title() {
    let chart = parse_chart_xml(&single_series_chart_xml(""), &SchemeColors::empty())
        .expect("chart parses");

    assert!(!chart.has_automatic_title);
}

/// An axis title is a `<c:title>` too, and it names its own text — it must not
/// be read as the chart's, in either direction.
#[test]
fn test_an_axis_title_is_not_the_charts_automatic_title() {
    let xml = single_series_chart_xml("").replace(
        "</c:plotArea>",
        r#"<c:catAx><c:axId val="1"/><c:title><c:tx><c:rich><a:p><a:r><a:t>Quarter</a:t></a:r></a:p></c:rich></c:tx></c:title></c:catAx></c:plotArea>"#,
    );
    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert!(!chart.has_automatic_title);
    assert_eq!(chart.title, None);
    assert_eq!(chart.category_axis_title.as_deref(), Some("Quarter"));
}

/// A series that names its fill through a theme colour resolves it against the
/// host document's theme — the chart part declares none of its own (#876).
///
/// `002.CONTOSO.pptx` (issue #841) writes
/// `<c:spPr><a:solidFill><a:schemeClr val="accent5"/></a:solidFill>` on its
/// first series, where the deck's `accent5` is the lime `C6FC15`; the bars
/// rendered palette-blue.
#[test]
fn a_series_scheme_color_fill_resolves_against_the_host_theme() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "<c:tx>",
        r#"<c:spPr><a:solidFill><a:schemeClr val="accent5"/></a:solidFill><a:ln><a:noFill/></a:ln></c:spPr><c:tx>"#,
    );
    let colors: std::collections::HashMap<String, Color> =
        [("accent5".to_string(), Color::new(0xC6, 0xFC, 0x15))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    let chart = parse_chart_xml(&xml, &scheme).unwrap();

    assert_eq!(chart.series[0].fill, Some(Color::new(0xC6, 0xFC, 0x15)));
}

/// Triangulation: a different scheme name takes that theme entry, and a name
/// the theme does not carry stays unresolved so the palette still applies.
#[test]
fn a_series_scheme_color_follows_the_named_theme_entry() {
    let colors: std::collections::HashMap<String, Color> = [
        ("accent1".to_string(), Color::new(0x11, 0x22, 0x33)),
        ("accent5".to_string(), Color::new(0xC6, 0xFC, 0x15)),
    ]
    .into_iter()
    .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    for (name, expected) in [
        ("accent1", Some(Color::new(0x11, 0x22, 0x33))),
        ("accent5", Some(Color::new(0xC6, 0xFC, 0x15))),
        ("accent3", None),
    ] {
        let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
            "<c:tx>",
            &format!(
                r#"<c:spPr><a:solidFill><a:schemeClr val="{name}"/></a:solidFill></c:spPr><c:tx>"#
            ),
        );
        let chart = parse_chart_xml(&xml, &scheme).unwrap();
        assert_eq!(chart.series[0].fill, expected, "scheme colour {name}");
    }
}

/// A theme colour carrying a transform keeps it — `<a:schemeClr>` children are
/// the same markup everywhere else in the package.
#[test]
fn a_series_scheme_color_applies_its_luminance_transform() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "<c:tx>",
        r#"<c:spPr><a:solidFill><a:schemeClr val="accent1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill></c:spPr><c:tx>"#,
    );
    let colors: std::collections::HashMap<String, Color> =
        [("accent1".to_string(), Color::new(0x40, 0x80, 0xC0))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    let fill = parse_chart_xml(&xml, &scheme).unwrap().series[0]
        .fill
        .expect("the series states a fill");
    assert_ne!(
        fill,
        Color::new(0x40, 0x80, 0xC0),
        "lumMod 50% must darken the theme colour, not pass it through"
    );
    assert!(
        fill.r < 0x40 && fill.g < 0x80 && fill.b < 0xC0,
        "expected a darker colour, got {fill:?}"
    );
}

/// An axis and its gridlines carry the `<a:ln>` they declare (issue #900).
///
/// `002.CONTOSO.pptx` (#841) gives `<c:catAx>` and `<c:majorGridlines>` the
/// same white 12700 EMU line; both rendered as the automatic
/// `rgb(134, 134, 134)` at 0.75pt.
#[test]
fn an_axis_and_its_gridlines_keep_the_line_they_declare() {
    let line = r#"<c:spPr><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:round/></a:ln><a:effectLst/></c:spPr>"#;
    let gridlines = format!("<c:majorGridlines>{line}</c:majorGridlines>");
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#)
        .replace("</c:plotArea>", &format!(
            r#"<c:catAx><c:axId val="1"/>{line}</c:catAx><c:valAx><c:axId val="2"/>{gridlines}{line}</c:valAx></c:plotArea>"#
        ));
    let colors: std::collections::HashMap<String, Color> =
        [("bg1".to_string(), Color::new(0xFF, 0xFF, 0xFF))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");

    let expected = ChartLine::Explicit {
        width_pt: Some(1.0),
        color: Some(Color::new(0xFF, 0xFF, 0xFF)),
    };
    for (label, line) in [
        ("category axis", chart.category_axis_line),
        ("value axis", chart.value_axis_line),
        ("major gridlines", chart.major_gridline_line),
    ] {
        assert_eq!(line, expected, "{label}");
    }
}

/// The chart frame and the major gridlines resolve `tx1` the same way the
/// labels beside them do, even though nothing maps it onto a theme slot.
///
/// `tests/fixtures/xlsx/any_sheets.xlsx` states this one stroke three times —
/// on `c:chartSpace/c:spPr`, on `c:catAx/c:spPr` and on
/// `c:valAx/c:majorGridlines` — as `tx1` lifted by lumMod 15% / lumOff 85%,
/// which over the theme's black `dk1` is #D9D9D9. A workbook carries no
/// `<p:clrMap>` to turn `tx1` into `dk1`, so the colour resolved to nothing and
/// all three fell back to the renderer's built-in grey, RGB 134 (issue #1145).
#[test]
fn a_text_slot_line_colour_resolves_against_the_theme() {
    let line = r#"<c:spPr><a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="tx1"><a:lumMod val="15000"/><a:lumOff val="85000"/></a:schemeClr></a:solidFill><a:round/></a:ln><a:effectLst/></c:spPr>"#;
    let gridlines = format!("<c:majorGridlines>{line}</c:majorGridlines>");
    let xml = chart_space_with(line).replace(
        "</c:plotArea>",
        &format!(
            r#"<c:catAx><c:axId val="1"/>{line}</c:catAx><c:valAx><c:axId val="2"/>{gridlines}</c:valAx></c:plotArea>"#
        ),
    );
    // The theme keys its slots the way `<a:clrScheme>` does; `tx1` appears
    // nowhere in it.
    let colors: std::collections::HashMap<String, Color> =
        [("dk1".to_string(), Color::new(0, 0, 0))]
            .into_iter()
            .collect();
    let aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let scheme = SchemeColors {
        colors: &colors,
        aliases: &aliases,
    };

    let chart = parse_chart_xml(&xml, &scheme).expect("chart parses");

    let lifted = Color::new(0xD9, 0xD9, 0xD9);
    assert_eq!(
        chart.chart_area_outline,
        ChartAreaOutline::Explicit {
            width_pt: Some(0.75),
            color: Some(lifted),
            round_join: true,
        },
        "the chart frame"
    );
    let expected = ChartLine::Explicit {
        width_pt: Some(0.75),
        color: Some(lifted),
    };
    for (label, line) in [
        ("category axis", chart.category_axis_line),
        ("major gridlines", chart.major_gridline_line),
    ] {
        assert_eq!(line, expected, "{label}");
    }
}

/// `<a:ln><a:noFill/></a:ln>` suppresses the line, which is not the same as
/// declaring none — the deck in #841 writes exactly that on its value axis and
/// the reference draws no vertical rule there, where we drew the automatic
/// grey one.
#[test]
fn an_axis_no_fill_line_suppresses_it() {
    let suppressed = r#"<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln><a:effectLst/></c:spPr>"#;
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        &format!(r#"<c:valAx><c:axId val="2"/>{suppressed}</c:valAx></c:plotArea>"#),
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_line, ChartLine::Suppressed);
    assert_eq!(
        chart.category_axis_line,
        ChartLine::Automatic,
        "an axis that states nothing is not suppressed"
    );
}

/// An axis that declares no line at all leaves the automatic one to the
/// renderer.
#[test]
fn an_axis_without_a_line_is_automatic() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:catAx><c:axId val="1"/></c:catAx><c:valAx><c:axId val="2"/></c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.category_axis_line, ChartLine::Automatic);
    assert_eq!(chart.value_axis_line, ChartLine::Automatic);
    assert_eq!(chart.major_gridline_line, ChartLine::Automatic);
}

/// `<c:dLblPos>` reaches the model, and a bar chart that states none takes the
/// default its grouping implies (issue #901).
///
/// ECMA-376 §21.2.2.49: a `clustered` bar defaults to `outEnd`, a stacked one
/// to `ctr`. We centred every label whatever the grouping, so a clustered
/// bar's label sat inside the bar instead of just beyond its end.
#[test]
fn a_bar_chart_data_label_position_defaults_by_grouping() {
    for (grouping, expected) in [
        ("clustered", DataLabelPosition::OutsideEnd),
        ("stacked", DataLabelPosition::Center),
        ("percentStacked", DataLabelPosition::Center),
    ] {
        let xml = bar_chart_xml(&format!(
            r#"<c:barDir val="col"/><c:grouping val="{grouping}"/>"#
        ))
        .replace(
            "<c:cat>",
            r#"<c:dLbls><c:showVal val="1"/></c:dLbls><c:cat>"#,
        );

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

        assert_eq!(
            chart.series[0].data_labels.position, expected,
            "{grouping} grouping"
        );
    }
}

/// A stated `<c:dLblPos>` outranks the grouping's default.
#[test]
fn a_stated_data_label_position_wins_over_the_default() {
    for (stated, expected) in [
        ("ctr", DataLabelPosition::Center),
        ("outEnd", DataLabelPosition::OutsideEnd),
        ("inEnd", DataLabelPosition::InsideEnd),
        ("inBase", DataLabelPosition::InsideBase),
    ] {
        let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
            "<c:cat>",
            &format!(
                r#"<c:dLbls><c:dLblPos val="{stated}"/><c:showVal val="1"/></c:dLbls><c:cat>"#
            ),
        );

        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

        assert_eq!(
            chart.series[0].data_labels.position, expected,
            "dLblPos {stated}"
        );
    }
}

/// `<c:majorUnit>` sets the value axis' tick interval; the auto-scale applies
/// only when the part states none (issue #882).
///
/// `002.CONTOSO.pptx` (#841) declares `<c:majorUnit val="0.2"/>` and the
/// reference ticks 0/20/40/60/80%; we ticked every 10%, twice as often as the
/// file asks.
#[test]
fn a_stated_major_unit_reaches_the_model() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:valAx><c:axId val="2"/><c:majorUnit val="0.2"/></c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    let unit = chart
        .value_axis_major_unit
        .expect("a stated major unit reaches the model");
    assert!((unit - 0.2).abs() < 1e-9, "expected 0.2, got {unit}");
}

/// `<c:scaling><c:min>` and `<c:max>` fix the value axis to an interval, which
/// is how a chart puts zero inside its plot rather than on the edge.
///
/// `xl/charts/chart1.xml` of the workbook in #1123 scales its `january cash
/// flow:` bar around zero — `<c:min val="-400"/><c:max val="400"/>` — so
/// Excel draws the axis line down the middle of the plot. Reading the bounds
/// past left every plotted value measured against `[0, auto]`, so the negative
/// series had nowhere to go (issue #1184).
#[test]
fn a_stated_value_axis_scaling_reaches_the_model() {
    let xml = bar_chart_xml(r#"<c:barDir val="bar"/><c:grouping val="stacked"/>"#).replace(
        "</c:plotArea>",
        r#"<c:valAx><c:axId val="2"/>
             <c:scaling><c:orientation val="minMax"/><c:max val="400"/><c:min val="-400"/></c:scaling>
             <c:majorUnit val="400"/>
           </c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_min, Some(-400.0));
    assert_eq!(chart.value_axis_max, Some(400.0));
}

/// The two bounds are independent: `chart2.xml` of the same workbook fixes only
/// the maximum and leaves the minimum to the automatic scale (issue #1184).
#[test]
fn one_stated_bound_leaves_the_other_to_the_auto_scale() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:valAx><c:axId val="2"/>
             <c:scaling><c:orientation val="minMax"/><c:max val="1000"/></c:scaling>
           </c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_max, Some(1000.0));
    assert_eq!(chart.value_axis_min, None);
}

/// A category axis states its own `<c:scaling>` too, and it carries no bounds —
/// so the value axis' interval must not be picked up from it.
#[test]
fn a_category_axis_scaling_does_not_bound_the_value_axis() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:catAx><c:axId val="1"/>
             <c:scaling><c:orientation val="minMax"/></c:scaling>
           </c:catAx>
           <c:valAx><c:axId val="2"/>
             <c:scaling><c:orientation val="minMax"/><c:min val="-50"/></c:scaling>
           </c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_min, Some(-50.0));
    assert_eq!(chart.value_axis_max, None);
}

/// An axis that states no scaling bounds leaves both ends automatic.
#[test]
fn an_unstated_value_axis_scaling_is_none() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:valAx><c:axId val="2"/>
             <c:scaling><c:orientation val="minMax"/></c:scaling>
           </c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_min, None);
    assert_eq!(chart.value_axis_max, None);
}

/// An axis that states none leaves the interval to the auto-scale.
#[test]
fn an_unstated_major_unit_is_none() {
    let xml = bar_chart_xml(r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#).replace(
        "</c:plotArea>",
        r#"<c:valAx><c:axId val="2"/></c:valAx></c:plotArea>"#,
    );

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).expect("chart parses");

    assert_eq!(chart.value_axis_major_unit, None);
}

/// A data label's size comes from its own `<c:dLbls><c:txPr>`, which the
/// labels were never given — they were written at a literal 8pt, so the deck
/// of #841 drew its 11.97pt labels smaller than its own axis (issue #970).
#[test]
fn a_data_label_takes_the_size_its_own_tx_pr_declares() {
    let xml = r#"<?xml version="1.0"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea><c:barChart>
                <c:barDir val="col"/><c:grouping val="clustered"/>
                <c:ser>
                    <c:idx val="0"/>
                    <c:dLbls>
                        <c:dLbl><c:idx val="0"/>
                            <c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="600"/></a:pPr></a:p></c:txPr>
                        </c:dLbl>
                        <c:txPr><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1197" b="1"/></a:pPr></a:p></c:txPr>
                        <c:showVal val="1"/>
                    </c:dLbls>
                    <c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat>
                    <c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>100</c:v></c:pt></c:numCache></c:numRef></c:val>
                </c:ser>
            </c:barChart></c:plotArea></c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    let labels = &chart.series[0].data_labels;

    assert_eq!(labels.text_style.size_pt, Some(11.97), "sz is hundredths");
    assert_eq!(labels.text_style.bold, Some(true));
    // The per-point `<c:dLbl>` states 6pt, and a single point's override must
    // not become the group's setting — the same reason its `showVal` does not.
    assert!(labels.show_value);
}

/// A `<c:dLbls>` stating no `c:txPr` leaves the size to the chart space, which
/// is what every other string on the chart already resolves against.
#[test]
fn a_data_label_without_a_tx_pr_states_no_size_of_its_own() {
    let xml = r#"<?xml version="1.0"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea><c:barChart>
                <c:barDir val="col"/><c:grouping val="clustered"/>
                <c:ser>
                    <c:idx val="0"/>
                    <c:dLbls><c:showVal val="1"/></c:dLbls>
                    <c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat>
                    <c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>100</c:v></c:pt></c:numCache></c:numRef></c:val>
                </c:ser>
            </c:barChart></c:plotArea></c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();
    assert_eq!(chart.series[0].data_labels.text_style.size_pt, None);
}

// ── Combo plot areas (issue #1067) ─────────────────────────────────────

/// The plot area of `Gift Budget and Tracker1.xlsx`, reduced to the shape that
/// matters: three stacked columns per category with one line laid over them.
///
/// `<c:lineChart>` follows `<c:barChart>`, so whatever the parser takes from
/// the last family it reads is what a combo chart ends up drawn as.
fn combo_bar_and_line_chart_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea>
                <c:barChart>
                    <c:barDir val="col"/>
                    <c:grouping val="stacked"/>
                    <c:ser>
                        <c:idx val="0"/>
                        <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Birthday Budget</c:v></c:pt></c:strCache></c:strRef></c:tx>
                        <c:cat><c:strRef><c:strCache>
                            <c:pt idx="0"><c:v>May</c:v></c:pt>
                            <c:pt idx="1"><c:v>Jun</c:v></c:pt>
                        </c:strCache></c:strRef></c:cat>
                        <c:val><c:numRef><c:numCache>
                            <c:pt idx="0"><c:v>0</c:v></c:pt>
                            <c:pt idx="1"><c:v>50</c:v></c:pt>
                        </c:numCache></c:numRef></c:val>
                    </c:ser>
                    <c:ser>
                        <c:idx val="1"/>
                        <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Holiday Budget</c:v></c:pt></c:strCache></c:strRef></c:tx>
                        <c:val><c:numRef><c:numCache>
                            <c:pt idx="0"><c:v>0</c:v></c:pt>
                            <c:pt idx="1"><c:v>100</c:v></c:pt>
                        </c:numCache></c:numRef></c:val>
                    </c:ser>
                    <c:gapWidth val="150"/>
                    <c:overlap val="100"/>
                </c:barChart>
                <c:lineChart>
                    <c:grouping val="stacked"/>
                    <c:ser>
                        <c:idx val="2"/>
                        <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Amount Spent</c:v></c:pt></c:strCache></c:strRef></c:tx>
                        <c:val><c:numRef><c:numCache>
                            <c:pt idx="0"><c:v>25</c:v></c:pt>
                            <c:pt idx="1"><c:v>75</c:v></c:pt>
                        </c:numCache></c:numRef></c:val>
                    </c:ser>
                    <c:marker val="1"/>
                </c:lineChart>
            </c:plotArea></c:chart>
        </c:chartSpace>"#
        .to_string()
}

/// A series declared by a family other than the chart's own records that
/// family, so a combo plot area survives into the IR instead of collapsing to
/// one kind (issue #1067).
#[test]
fn a_combo_plot_area_records_the_family_that_declared_each_series() {
    let chart = parse_chart_xml(&combo_bar_and_line_chart_xml(), &SchemeColors::empty()).unwrap();

    assert_eq!(chart.series.len(), 3);
    // The bar family is the chart's own, so its series name no other.
    assert_eq!(chart.series[0].plot_type, None);
    assert_eq!(chart.series[1].plot_type, None);
    assert_eq!(chart.series[2].plot_type, Some(ChartType::Line));
}

/// The bar family governs the axis in a combo: it is what the value scale and
/// the category bands are drawn for, and a `<c:lineChart>` that follows must
/// not take the chart's type with it.
#[test]
fn a_trailing_line_chart_leaves_the_bar_family_governing() {
    let chart = parse_chart_xml(&combo_bar_and_line_chart_xml(), &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Column);
    // `<c:lineChart><c:grouping val="stacked"/>` is the line family's own; the
    // stacking that the columns are drawn with is the bar family's.
    assert_eq!(chart.grouping, ChartGrouping::Stacked);
}

/// The order is not what decides it: a `<c:barChart>` following a
/// `<c:lineChart>` governs the axis just the same.
#[test]
fn a_leading_line_chart_still_leaves_the_bar_family_governing() {
    let xml: String = combo_bar_and_line_chart_xml();
    let bar_start: usize = xml.find("<c:barChart>").unwrap();
    let bar_end: usize = xml.find("</c:barChart>").unwrap() + "</c:barChart>".len();
    let line_start: usize = xml.find("<c:lineChart>").unwrap();
    let line_end: usize = xml.find("</c:lineChart>").unwrap() + "</c:lineChart>".len();
    let swapped: String = format!(
        "{}{}{}{}{}",
        &xml[..bar_start],
        &xml[line_start..line_end],
        &xml[bar_end..line_start],
        &xml[bar_start..bar_end],
        &xml[line_end..]
    );

    let chart = parse_chart_xml(&swapped, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Column);
    assert_eq!(chart.grouping, ChartGrouping::Stacked);
    assert_eq!(chart.series[0].plot_type, Some(ChartType::Line));
    assert_eq!(chart.series[1].plot_type, None);
}

/// A single-family chart names no per-series family: every series is the
/// chart's own kind, which is what `None` says.
#[test]
fn a_single_family_chart_leaves_every_series_on_the_chart_type() {
    let xml = chart_with_bar_layout(r#"<c:gapWidth val="150"/>"#);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert!(!chart.series.is_empty());
    assert!(chart.series.iter().all(|series| series.plot_type.is_none()));
}

/// The cash-flow chart of `Monthly college budget1.xlsx`, reduced to three
/// categories: a `<c:lineChart>` over the months with a `<c:scatterChart>`
/// laid over it whose marker-only series highlights the selected one.
///
/// The scatter family is written last and shares the line's `<c:axId>` pair,
/// so it plots against the very category axis the line declared.
fn combo_line_and_scatter_chart_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart><c:plotArea>
                <c:lineChart>
                    <c:grouping val="standard"/>
                    <c:ser>
                        <c:idx val="0"/>
                        <c:tx><c:v>Cash Flow</c:v></c:tx>
                        <c:cat><c:strRef><c:strCache>
                            <c:pt idx="0"><c:v>jan</c:v></c:pt>
                            <c:pt idx="1"><c:v>feb</c:v></c:pt>
                            <c:pt idx="2"><c:v>mar</c:v></c:pt>
                        </c:strCache></c:strRef></c:cat>
                        <c:val><c:numRef><c:numCache>
                            <c:pt idx="0"><c:v>169</c:v></c:pt>
                            <c:pt idx="1"><c:v>69</c:v></c:pt>
                            <c:pt idx="2"><c:v>192</c:v></c:pt>
                        </c:numCache></c:numRef></c:val>
                    </c:ser>
                    <c:axId val="96119408"/>
                    <c:axId val="477185864"/>
                </c:lineChart>
                <c:scatterChart>
                    <c:scatterStyle val="lineMarker"/>
                    <c:ser>
                        <c:idx val="1"/>
                        <c:tx><c:v>Positive Selected Period</c:v></c:tx>
                        <c:marker><c:symbol val="circle"/><c:size val="14"/></c:marker>
                        <c:yVal><c:numRef><c:numCache>
                            <c:pt idx="0"><c:v>169</c:v></c:pt>
                        </c:numCache></c:numRef></c:yVal>
                    </c:ser>
                    <c:axId val="96119408"/>
                    <c:axId val="477185864"/>
                </c:scatterChart>
            </c:plotArea></c:chart>
        </c:chartSpace>"#
        .to_string()
}

/// A `<c:scatterChart>` carries no `<c:cat>` — its points state their own x
/// values — so it cannot own the category bands the family beside it declared.
/// Letting it take the chart's type dropped the whole combo to the data-table
/// fallback, because no scatter plot is drawn (issue #1123).
#[test]
fn a_trailing_scatter_chart_leaves_the_line_family_governing() {
    let chart =
        parse_chart_xml(&combo_line_and_scatter_chart_xml(), &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Line);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].plot_type, None);
    assert_eq!(chart.series[1].plot_type, Some(ChartType::Scatter));
}

/// The categories stay the line family's month labels rather than being
/// renumbered from the scatter series' implicit x values.
#[test]
fn a_trailing_scatter_chart_keeps_the_line_familys_categories() {
    let chart =
        parse_chart_xml(&combo_line_and_scatter_chart_xml(), &SchemeColors::empty()).unwrap();

    assert_eq!(chart.categories, vec!["jan", "feb", "mar"]);
}

/// Order is not what decides it: a `<c:lineChart>` following a
/// `<c:scatterChart>` governs the axis just the same.
#[test]
fn a_leading_scatter_chart_still_leaves_the_line_family_governing() {
    let xml: String = combo_line_and_scatter_chart_xml();
    let line_start: usize = xml.find("<c:lineChart>").unwrap();
    let line_end: usize = xml.find("</c:lineChart>").unwrap() + "</c:lineChart>".len();
    let scatter_start: usize = xml.find("<c:scatterChart>").unwrap();
    let scatter_end: usize = xml.find("</c:scatterChart>").unwrap() + "</c:scatterChart>".len();
    let swapped: String = format!(
        "{}{}{}{}{}",
        &xml[..line_start],
        &xml[scatter_start..scatter_end],
        &xml[line_end..scatter_start],
        &xml[line_start..line_end],
        &xml[scatter_end..]
    );

    let chart = parse_chart_xml(&swapped, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Line);
    assert_eq!(chart.series[0].plot_type, Some(ChartType::Scatter));
    assert_eq!(chart.series[1].plot_type, None);
}

/// A plot area holding nothing but a `<c:scatterChart>` still reads as one:
/// the guard only keeps a scatter from taking governance off another family.
#[test]
fn a_scatter_only_plot_area_still_reads_as_a_scatter_chart() {
    let xml: String = combo_line_and_scatter_chart_xml();
    let line_start: usize = xml.find("<c:lineChart>").unwrap();
    let line_end: usize = xml.find("</c:lineChart>").unwrap() + "</c:lineChart>".len();
    let scatter_only: String = format!("{}{}", &xml[..line_start], &xml[line_end..]);

    let chart = parse_chart_xml(&scatter_only, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Scatter);
    assert!(chart.series.iter().all(|series| series.plot_type.is_none()));
}

/// The bar family still outranks a scatter that follows it, as it does every
/// other family (issue #1067).
#[test]
fn a_trailing_scatter_chart_leaves_the_bar_family_governing() {
    let xml: String = combo_line_and_scatter_chart_xml()
        .replace("<c:lineChart>", r#"<c:barChart><c:barDir val="col"/>"#)
        .replace("</c:lineChart>", "</c:barChart>");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.chart_type, ChartType::Column);
    assert_eq!(chart.series[1].plot_type, Some(ChartType::Scatter));
}

/// The `january income:` bar chart of `tests/fixtures/xlsx/issue_1181_fit_to_height.xlsx`
/// (`xl/charts/chart3.xml`), whose plot area states where it sits inside the
/// chart area. `LAYOUT` is spliced out so a test can vary it (issue #1182).
fn manual_plot_layout_chart_xml(layout: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    {layout}
                    <c:barChart>
                        <c:barDir val="bar"/>
                        <c:grouping val="clustered"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat>
                                <c:strRef><c:strCache>
                                    <c:pt idx="0"><c:v>financial aid</c:v></c:pt>
                                    <c:pt idx="1"><c:v>from savings</c:v></c:pt>
                                </c:strCache></c:strRef>
                            </c:cat>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>0</c:v></c:pt>
                                    <c:pt idx="1"><c:v>0.40816326530612246</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#
    )
}

/// The layout element the reported workbook writes, verbatim.
const REPORTED_MANUAL_LAYOUT: &str = r#"<c:layout><c:manualLayout>
    <c:layoutTarget val="inner"/><c:xMode val="edge"/><c:yMode val="edge"/>
    <c:x val="0.30222298185081131"/><c:y val="0.34625485336714895"/>
    <c:w val="0.64139311135561661"/><c:h val="0.55711705789303645"/>
</c:manualLayout></c:layout>"#;

const REPORTED_TITLE_MANUAL_LAYOUT: &str = r#"<c:layout><c:manualLayout>
    <c:xMode val="edge"/><c:yMode val="edge"/>
    <c:x val="7.2127696791070986E-3"/><c:y val="1.182572865366946E-4"/>
</c:manualLayout></c:layout>"#;

fn manual_title_layout_chart_xml(layout: &str) -> String {
    manual_plot_layout_chart_xml("<c:layout/>").replace(
        "<c:plotArea>",
        &format!(
            "<c:title><c:tx><c:rich><a:p><a:r><a:t>Annual Income &amp; Gross Profit</a:t></a:r></a:p></c:rich></c:tx>{layout}</c:title><c:plotArea>"
        ),
    )
}

/// An edge-mode inner manual layout reaches the IR as the four fractions it
/// states (issue #1182).
#[test]
fn a_plot_area_manual_layout_reaches_the_ir_as_stated() {
    let xml: String = manual_plot_layout_chart_xml(REPORTED_MANUAL_LAYOUT);

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(
        chart.plot_area_layout,
        Some(crate::ir::ChartPlotAreaLayout {
            x: 0.3022229818508113,
            y: 0.34625485336714895,
            width: 0.6413931113556166,
            height: 0.5571170578930364,
        })
    );
}

/// A plot area that states no layout at all keeps the automatic one.
#[test]
fn a_plot_area_without_a_layout_states_no_rectangle() {
    let xml: String = manual_plot_layout_chart_xml("<c:layout/>");

    let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.plot_area_layout, None);
}

/// Only the modes this models are read. `factor` — the `ST_LayoutMode` default,
/// so also what an omitted `c:xMode` means — offsets the *automatic* layout by
/// the value rather than naming an edge, and `layoutTarget="outer"` measures the
/// plot area with its tick labels rather than the plotting rectangle. Reading
/// either as an edge fraction of the chart area would move the plot to a place
/// nothing asked for, so both keep the automatic layout.
#[test]
fn a_layout_this_does_not_model_keeps_the_automatic_rectangle() {
    let factor_mode: String = REPORTED_MANUAL_LAYOUT.replace(r#"<c:xMode val="edge"/>"#, "");
    let outer_target: String = REPORTED_MANUAL_LAYOUT.replace(
        r#"<c:layoutTarget val="inner"/>"#,
        r#"<c:layoutTarget val="outer"/>"#,
    );
    let no_target: String = REPORTED_MANUAL_LAYOUT.replace(r#"<c:layoutTarget val="inner"/>"#, "");
    let partial: String = REPORTED_MANUAL_LAYOUT
        .replace(r#"<c:w val="0.64139311135561661"/>"#, "")
        .replace(r#"<c:h val="0.55711705789303645"/>"#, "");

    for (case, layout) in [
        ("xMode omitted", factor_mode),
        ("outer target", outer_target),
        ("no layoutTarget", no_target),
        ("no size", partial),
    ] {
        let xml: String = manual_plot_layout_chart_xml(&layout);
        let chart = parse_chart_xml(&xml, &SchemeColors::empty()).unwrap();
        assert_eq!(chart.plot_area_layout, None, "{case}");
    }
}

/// `c:layout` is written by the title, the legend and every data-label group
/// too, and only the plot area's own says where the plot sits.
#[test]
fn a_title_manual_layout_is_not_the_plot_areas() {
    let titled: String = manual_title_layout_chart_xml(REPORTED_TITLE_MANUAL_LAYOUT);

    let chart = parse_chart_xml(&titled, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.plot_area_layout, None);
    assert_eq!(
        chart.title_layout,
        Some(crate::ir::ChartTitleLayout {
            x: 0.007212769679107099,
            y: 0.0001182572865366946,
        })
    );
}

/// Factor-mode coordinates offset the application-computed title position;
/// treating one as a chart-edge fraction would invent an absolute anchor.
#[test]
fn a_title_factor_layout_keeps_the_automatic_position() {
    for (case, layout) in [
        (
            "xMode omitted",
            REPORTED_TITLE_MANUAL_LAYOUT.replace(r#"<c:xMode val="edge"/>"#, ""),
        ),
        (
            "yMode factor",
            REPORTED_TITLE_MANUAL_LAYOUT
                .replace(r#"<c:yMode val="edge"/>"#, r#"<c:yMode val="factor"/>"#),
        ),
    ] {
        let chart = parse_chart_xml(
            &manual_title_layout_chart_xml(&layout),
            &SchemeColors::empty(),
        )
        .unwrap();
        assert_eq!(chart.title_layout, None, "{case}");
    }
}

/// The `january expenses:` chart of the workbook behind issue #1183 caches six
/// category labels, three of which hold an escaped `&`. The reader splits a
/// text node at every entity reference, so a label read one `Event::Text` at a
/// time arrives as two labels with the `&` — the reference between them —
/// dropped: six categories printed as nine, each `X & Y` bar labelled with a
/// neighbour's word.
fn escaped_ampersand_category_chart_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="bar"/>
                        <c:grouping val="clustered"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:tx><c:strRef><c:strCache>
                                <c:pt idx="0"><c:v>room &amp; board</c:v></c:pt>
                            </c:strCache></c:strRef></c:tx>
                            <c:cat>
                                <c:strRef><c:strCache>
                                    <c:pt idx="0"><c:v>room &amp; board</c:v></c:pt>
                                    <c:pt idx="1"><c:v>tuition &amp; fees</c:v></c:pt>
                                    <c:pt idx="2"><c:v>books &amp; supplies</c:v></c:pt>
                                    <c:pt idx="3"><c:v>transportation</c:v></c:pt>
                                    <c:pt idx="4"><c:v>discretionary</c:v></c:pt>
                                    <c:pt idx="5"><c:v>other expenses</c:v></c:pt>
                                </c:strCache></c:strRef>
                            </c:cat>
                            <c:val>
                                <c:numRef><c:numCache>
                                    <c:pt idx="0"><c:v>6780</c:v></c:pt>
                                    <c:pt idx="1"><c:v>2050</c:v></c:pt>
                                    <c:pt idx="2"><c:v>1155</c:v></c:pt>
                                    <c:pt idx="3"><c:v>2473</c:v></c:pt>
                                    <c:pt idx="4"><c:v>4053</c:v></c:pt>
                                    <c:pt idx="5"><c:v>2938</c:v></c:pt>
                                </c:numCache></c:numRef>
                            </c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#
        .to_string()
}

#[test]
fn a_category_label_holding_an_escaped_ampersand_stays_one_label() {
    let chart = parse_chart_xml(
        &escaped_ampersand_category_chart_xml(),
        &SchemeColors::empty(),
    )
    .unwrap();

    assert_eq!(
        chart.categories,
        vec![
            "room & board",
            "tuition & fees",
            "books & supplies",
            "transportation",
            "discretionary",
            "other expenses",
        ]
    );
}

/// One label per bar: the category count has to match the values it names, or
/// every label past the first `&` slides onto the wrong bar.
#[test]
fn escaped_category_labels_stay_one_per_value() {
    let chart = parse_chart_xml(
        &escaped_ampersand_category_chart_xml(),
        &SchemeColors::empty(),
    )
    .unwrap();

    assert_eq!(chart.categories.len(), chart.series[0].values.len());
}

/// A series name reads through the same `<c:v>`, so its `&` was deleted too —
/// silently, since a name is one string and never split into two.
#[test]
fn a_series_name_keeps_an_escaped_ampersand() {
    let chart = parse_chart_xml(
        &escaped_ampersand_category_chart_xml(),
        &SchemeColors::empty(),
    )
    .unwrap();

    assert_eq!(chart.series[0].name.as_deref(), Some("room & board"));
}

/// A chart title is `<a:t>` rather than `<c:v>`, and lost its `&` the same way.
#[test]
fn a_chart_title_keeps_an_escaped_ampersand() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                      xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
            <c:chart>
                <c:title><c:tx><c:rich><a:p><a:r>
                    <a:t>Room &amp; board</a:t>
                </a:r></a:p></c:rich></c:tx></c:title>
                <c:plotArea>
                    <c:barChart>
                        <c:barDir val="col"/>
                        <c:ser>
                            <c:idx val="0"/>
                            <c:cat><c:strRef><c:strCache>
                                <c:pt idx="0"><c:v>Q1</c:v></c:pt>
                            </c:strCache></c:strRef></c:cat>
                            <c:val><c:numRef><c:numCache>
                                <c:pt idx="0"><c:v>100</c:v></c:pt>
                            </c:numCache></c:numRef></c:val>
                        </c:ser>
                    </c:barChart>
                </c:plotArea>
            </c:chart>
        </c:chartSpace>"#;

    let chart = parse_chart_xml(xml, &SchemeColors::empty()).unwrap();

    assert_eq!(chart.title.as_deref(), Some("Room & board"));
}
