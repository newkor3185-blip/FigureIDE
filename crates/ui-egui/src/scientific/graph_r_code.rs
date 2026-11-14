//! Reproducible ggplot2 code for the Graph Inspector's stored GUI overrides.
//!
//! A missing field in `GraphStyle` means "keep the R plot's own value", so only present
//! overrides are emitted. Clearing an override therefore restores the source plot's setting.

use std::collections::BTreeMap;

use super::graph_style::{FontFace, GraphStyle, LegendPosition, TextDetails, TitlePosition};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GeneratedCode {
    pub code: Option<String>,
    pub unsupported: Vec<String>,
}

pub(crate) fn generate(style: &GraphStyle, variable: &str) -> Result<GeneratedCode, String> {
    style.validate()?;
    if variable.is_empty() || variable.chars().any(char::is_control) {
        return Err("The linked R object has no usable name".into());
    }

    let mut theme = BTreeMap::<String, String>::new();
    let mut unsupported = Vec::new();

    text_element(&mut theme, "plot.title", "Title visibility", style.plot_title_pt, style.plot_title_visible, &style.title_details, &mut unsupported);
    text_element(
        &mut theme,
        "plot.subtitle",
        "Subtitle visibility",
        style.subtitle_pt,
        style.subtitle_visible,
        &style.subtitle_details,
        &mut unsupported,
    );
    text_element(&mut theme, "plot.caption", "Caption visibility", style.caption_pt, style.caption_visible, &style.caption_details, &mut unsupported);
    text_element(
        &mut theme,
        "axis.title.x",
        "X-axis title visibility",
        style.axis_title_x_pt,
        style.axis_title_x_visible,
        &style.axis_title_x_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "axis.title.y",
        "Y-axis title visibility",
        style.axis_title_y_pt,
        style.axis_title_y_visible,
        &style.axis_title_y_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "axis.text.x",
        "X-axis tick-label visibility",
        style.tick_x_pt,
        style.tick_x_visible,
        &style.tick_x_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "axis.text.y",
        "Y-axis tick-label visibility",
        style.tick_y_pt,
        style.tick_y_visible,
        &style.tick_y_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "legend.title",
        "Legend-title visibility",
        style.legend_title_pt,
        style.legend_title_visible,
        &style.legend_title_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "legend.text",
        "Legend-text visibility",
        style.legend_text_pt,
        style.legend_text_visible,
        &style.legend_text_details,
        &mut unsupported,
    );
    text_element(
        &mut theme,
        "strip.text",
        "Facet-text visibility",
        style.strip_text_pt,
        style.strip_text_visible,
        &style.strip_text_details,
        &mut unsupported,
    );

    line_element(
        &mut theme,
        "axis.line.x",
        "X-axis line visibility",
        style.axis_x_line_mm,
        style.axis_x_visible,
        style.axis_x_line_colour.as_deref(),
        &mut unsupported,
    );
    line_element(
        &mut theme,
        "axis.line.y",
        "Y-axis line visibility",
        style.axis_y_line_mm,
        style.axis_y_visible,
        style.axis_y_line_colour.as_deref(),
        &mut unsupported,
    );
    line_element(
        &mut theme,
        "axis.ticks.x",
        "X-axis tick visibility",
        style.axis_x_tick_width_mm,
        style.axis_x_ticks_visible,
        style.axis_x_tick_colour.as_deref(),
        &mut unsupported,
    );
    line_element(
        &mut theme,
        "axis.ticks.y",
        "Y-axis tick visibility",
        style.axis_y_tick_width_mm,
        style.axis_y_ticks_visible,
        style.axis_y_tick_colour.as_deref(),
        &mut unsupported,
    );
    if let Some(length) = style.axis_x_tick_length_mm {
        theme.insert("axis.ticks.length.x".into(), format!("grid::unit({}, \"mm\")", number(length)));
    }
    if let Some(length) = style.axis_y_tick_length_mm {
        theme.insert("axis.ticks.length.y".into(), format!("grid::unit({}, \"mm\")", number(length)));
    }

    grid_element(
        &mut theme,
        "panel.grid.major.x",
        "Major X grid visibility",
        style.major_grid_x_visible.or(style.major_grid_visible),
        style.major_grid_width_mm,
        style.major_grid_colour.as_deref(),
        &mut unsupported,
    );
    grid_element(
        &mut theme,
        "panel.grid.major.y",
        "Major Y grid visibility",
        style.major_grid_y_visible.or(style.major_grid_visible),
        style.major_grid_width_mm,
        style.major_grid_colour.as_deref(),
        &mut unsupported,
    );
    grid_element(
        &mut theme,
        "panel.grid.minor.x",
        "Minor X grid visibility",
        style.minor_grid_x_visible,
        style.minor_grid_width_mm,
        style.minor_grid_colour.as_deref(),
        &mut unsupported,
    );
    grid_element(
        &mut theme,
        "panel.grid.minor.y",
        "Minor Y grid visibility",
        style.minor_grid_y_visible,
        style.minor_grid_width_mm,
        style.minor_grid_colour.as_deref(),
        &mut unsupported,
    );

    if let Some(position) = style.legend_position {
        theme.insert("legend.position".into(), r_string(legend_position(position)));
        if position == LegendPosition::Inside && (style.inside_x.is_some() || style.inside_y.is_some()) {
            let x = style.inside_x.unwrap_or(0.95);
            let y = style.inside_y.unwrap_or(0.95);
            theme.insert("legend.position.inside".into(), format!("c({}, {})", number(x), number(y)));
        }
    }
    if let Some(direction) = style.legend_direction {
        theme.insert(
            "legend.direction".into(),
            r_string(match direction {
                super::graph_style::LegendDirection::Horizontal => "horizontal",
                super::graph_style::LegendDirection::Vertical => "vertical",
            }),
        );
    }
    if let Some([x, y]) = style.legend_justification {
        theme.insert("legend.justification".into(), format!("c({}, {})", number(x), number(y)));
    }
    if let Some(size) = style.legend_key_mm {
        theme.insert("legend.key.size".into(), format!("grid::unit({}, \"mm\")", number(size)));
    }
    for (key, value) in [
        ("legend.key.width", style.legend_key_width_mm),
        ("legend.key.height", style.legend_key_height_mm),
        ("legend.spacing.x", style.legend_spacing_x_mm),
        ("legend.spacing.y", style.legend_spacing_y_mm),
        ("panel.spacing", style.panel_spacing_mm),
    ] {
        if let Some(value) = value {
            theme.insert(key.into(), format!("grid::unit({}, \"mm\")", number(value)));
        }
    }
    if let Some(margins) = style.legend_margin_mm {
        theme.insert("legend.margin".into(), margin(margins));
    }
    if let Some(margins) = style.plot_margin_mm {
        theme.insert("plot.margin".into(), margin(margins));
    }

    rect_element(&mut theme, "panel.background", style.panel_background.as_deref(), None, None, None, &mut unsupported);
    rect_element(&mut theme, "plot.background", style.plot_background.as_deref(), None, None, None, &mut unsupported);
    rect_element(
        &mut theme,
        "panel.border",
        None,
        style.panel_border_visible,
        style.panel_border_colour.as_deref(),
        style.panel_border_width_mm,
        &mut unsupported,
    );
    rect_element(&mut theme, "legend.key", style.legend_key_background.as_deref(), None, None, None, &mut unsupported);
    rect_element(&mut theme, "legend.background", style.legend_background.as_deref(), None, None, None, &mut unsupported);
    rect_element(&mut theme, "strip.background", style.strip_background.as_deref(), None, None, None, &mut unsupported);

    if let Some(position) = style.plot_title_position {
        theme.insert(
            "plot.title.position".into(),
            r_string(match position {
                TitlePosition::Panel => "panel",
                TitlePosition::Plot => "plot",
            }),
        );
    }

    // Labels themselves are not currently editable in GraphStyle. Don't derive them from the
    // artwork or SVG: those can contain vector-only edits that are not the source ggplot labels.
    unsupported.sort();
    unsupported.dedup();
    if theme.is_empty() {
        return Ok(GeneratedCode { code: None, unsupported });
    }
    let expression =
        format!("get({}, envir = parent.frame(), inherits = TRUE) + ggplot2::theme(\n{}\n)", r_string(variable), named_arguments(&theme, 6));
    let mut lines = vec![expression];
    if style.axis_x_line_mm.is_some()
        || style.axis_y_line_mm.is_some()
        || style.axis_x_tick_width_mm.is_some()
        || style.axis_y_tick_width_mm.is_some()
        || style.major_grid_width_mm.is_some()
        || style.minor_grid_width_mm.is_some()
        || style.panel_border_width_mm.is_some()
    {
        lines.insert(0, "# Line widths use the ggplot2 3.4.0+ linewidth argument.".into());
    }
    if style.legend_position == Some(LegendPosition::Inside) {
        lines.insert(0, "# Inside legend positioning requires ggplot2 3.5.0 or newer.".into());
    }
    Ok(GeneratedCode { code: Some(lines.join("\n")), unsupported })
}

fn text_element(
    theme: &mut BTreeMap<String, String>,
    name: &str,
    visibility_name: &str,
    size: Option<f64>,
    visible: Option<bool>,
    details: &TextDetails,
    unsupported: &mut Vec<String>,
) {
    let has_details = details.family.is_some()
        || details.face.is_some()
        || details.colour.is_some()
        || details.angle.is_some()
        || details.hjust.is_some()
        || details.vjust.is_some();
    if visible == Some(false) {
        theme.insert(name.into(), "ggplot2::element_blank()".into());
        return;
    }
    if visible == Some(true) {
        unsupported.push(format!("{visibility_name} (restoring visibility without replacing the original theme style)"));
    }
    if size.is_none() && visible != Some(false) && !has_details {
        return;
    }
    let mut args = Vec::new();
    if let Some(size) = size {
        args.push(format!("size = {}", number(size)));
    }
    if let Some(family) = details.family.as_deref() {
        args.push(format!("family = {}", r_string(family)));
    }
    if let Some(face) = details.face {
        args.push(format!("face = {}", r_string(font_face(face))));
    }
    if let Some(colour) = details.colour.as_deref() {
        args.push(format!("colour = {}", r_string(colour)));
    }
    for (key, value) in [("angle", details.angle), ("hjust", details.hjust), ("vjust", details.vjust)] {
        if let Some(value) = value {
            args.push(format!("{key} = {}", number(value)));
        }
    }
    theme.insert(name.into(), format!("ggplot2::element_text({})", args.join(", ")));
}

fn line_element(
    theme: &mut BTreeMap<String, String>,
    name: &str,
    visibility_name: &str,
    width_mm: Option<f64>,
    visible: Option<bool>,
    colour: Option<&str>,
    unsupported: &mut Vec<String>,
) {
    if visible == Some(false) {
        theme.insert(name.into(), "ggplot2::element_blank()".into());
        return;
    }
    if visible == Some(true) {
        unsupported.push(format!("{visibility_name} (restoring visibility without replacing the original theme style)"));
    }
    if width_mm.is_none() && visible != Some(false) && colour.is_none() {
        return;
    }
    let mut args = Vec::new();
    if let Some(width) = width_mm {
        args.push(format!("linewidth = {}", number(width)));
    }
    if let Some(colour) = colour {
        args.push(format!("colour = {}", r_string(colour)));
    }
    theme.insert(name.into(), format!("ggplot2::element_line({})", args.join(", ")));
}

fn grid_element(
    theme: &mut BTreeMap<String, String>,
    name: &str,
    visibility_name: &str,
    visible: Option<bool>,
    width_mm: Option<f64>,
    colour: Option<&str>,
    unsupported: &mut Vec<String>,
) {
    line_element(theme, name, visibility_name, width_mm, visible, colour, unsupported);
}

fn rect_element(
    theme: &mut BTreeMap<String, String>,
    name: &str,
    fill: Option<&str>,
    visible: Option<bool>,
    colour: Option<&str>,
    width: Option<f64>,
    unsupported: &mut Vec<String>,
) {
    if visible == Some(false) {
        theme.insert(name.into(), "ggplot2::element_blank()".into());
        return;
    }
    if name == "panel.border" && visible == Some(true) && colour.is_none() && width.is_none() {
        unsupported.push("Panel border visibility (restoring it without replacing the original border style)".into());
        return;
    }
    if fill.is_none() && visible.is_none() && colour.is_none() && width.is_none() {
        return;
    }
    let mut args = Vec::new();
    if let Some(fill) = fill {
        args.push(format!("fill = {}", r_string(fill)));
    } else if name == "panel.border" {
        args.push("fill = NA".into());
    }
    if let Some(colour) = colour {
        args.push(format!("colour = {}", r_string(colour)));
    }
    if let Some(width) = width {
        args.push(format!("linewidth = {}", number(width)));
    }
    theme.insert(name.into(), format!("ggplot2::element_rect({})", args.join(", ")));
}

fn margin([top, right, bottom, left]: [f64; 4]) -> String {
    format!("ggplot2::margin(t = {}, r = {}, b = {}, l = {}, unit = \"mm\")", number(top), number(right), number(bottom), number(left))
}

fn named_arguments(values: &BTreeMap<String, String>, indent: usize) -> String {
    let padding = " ".repeat(indent);
    values.iter().map(|(key, value)| format!("{padding}{key} = {value}")).collect::<Vec<_>>().join(",\n")
}

fn legend_position(position: LegendPosition) -> &'static str {
    match position {
        LegendPosition::Right => "right",
        LegendPosition::Left => "left",
        LegendPosition::Top => "top",
        LegendPosition::Bottom => "bottom",
        LegendPosition::Inside => "inside",
        LegendPosition::None => "none",
    }
}

fn font_face(face: FontFace) -> &'static str {
    match face {
        FontFace::Plain => "plain",
        FontFace::Bold => "bold",
        FontFace::Italic => "italic",
        FontFace::BoldItalic => "bold.italic",
    }
}

fn number(value: f64) -> String {
    let mut result = format!("{value:.6}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.push('0');
    }
    result
}

fn r_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value.is_control() => {
                let code = u32::from(value);
                if code <= 0xffff {
                    output.push_str(&format!("\\u{code:04x}"));
                } else {
                    output.push_str(&format!("\\U{code:08x}"));
                }
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_overrides_produce_no_code_and_reverting_clears_the_code() {
        let mut style = GraphStyle::default();
        assert_eq!(generate(&style, "p").unwrap(), GeneratedCode::default());
        style.plot_title_pt = Some(14.0);
        assert!(generate(&style, "p").unwrap().code.is_some());
        style.plot_title_pt = None;
        assert_eq!(generate(&style, "p").unwrap(), GeneratedCode::default());
    }

    #[test]
    fn code_is_deterministic_and_maps_independent_axes_legend_and_grid() {
        let style = GraphStyle {
            tick_x_pt: Some(10.0),
            tick_y_pt: Some(12.0),
            legend_position: Some(LegendPosition::Top),
            major_grid_visible: Some(false),
            minor_grid_x_visible: Some(false),
            ..Default::default()
        };
        let a = generate(&style, "plot_A").unwrap();
        let b = generate(&style, "plot_A").unwrap();
        assert_eq!(a, b);
        let code = a.code.unwrap();
        assert!(code.contains("axis.text.x = ggplot2::element_text(size = 10.0)"));
        assert!(code.contains("axis.text.y = ggplot2::element_text(size = 12.0)"));
        assert!(code.contains("legend.position = \"top\""));
        assert!(code.contains("panel.grid.major.x = ggplot2::element_blank()"));
        assert!(code.contains("panel.grid.minor.x = ggplot2::element_blank()"));
        assert!(code.contains("get(\"plot_A\", envir = parent.frame(), inherits = TRUE) + ggplot2::theme("));
        assert!(!code.contains("\n  + ggplot2::theme("), "the continuation operator must follow the plot expression");
    }

    #[test]
    fn text_and_line_overrides_escape_strings_and_keep_millimetre_units() {
        let style = GraphStyle {
            title_details: TextDetails { family: Some("A \"quoted\" \\ family".into()), ..Default::default() },
            axis_x_line_mm: Some(0.8),
            axis_x_line_colour: Some("#12abef".into()),
            axis_x_tick_length_mm: Some(1.5),
            ..Default::default()
        };
        let code = generate(&style, "p").unwrap().code.unwrap();
        assert!(code.contains("family = \"A \\\"quoted\\\" \\\\ family\""));
        assert!(code.contains("linewidth = 0.8"));
        assert!(code.contains("colour = \"#12abef\""));
        assert!(code.contains("grid::unit(1.5, \"mm\")"));
    }

    #[test]
    fn title_visibility_and_layout_units_map_to_theme_elements() {
        let style = GraphStyle { plot_title_visible: Some(false), plot_margin_mm: Some([1.0, 2.0, 3.0, 4.0]), ..Default::default() };
        let code = generate(&style, "p").unwrap().code.unwrap();
        assert!(code.contains("plot.title = ggplot2::element_blank()"));
        assert!(code.contains("ggplot2::margin(t = 1.0, r = 2.0, b = 3.0, l = 4.0, unit = \"mm\")"));
    }

    #[test]
    fn showing_hidden_elements_does_not_replace_original_style_with_defaults() {
        let style = GraphStyle {
            plot_title_visible: Some(true),
            axis_x_visible: Some(true),
            major_grid_visible: Some(true),
            panel_border_visible: Some(true),
            ..Default::default()
        };
        let result = generate(&style, "p").unwrap();
        assert_eq!(result.code, None);
        assert_eq!(result.unsupported.len(), 5);
        assert!(result.unsupported.iter().any(|item| item.starts_with("Panel border visibility")));
        assert!(result.unsupported.iter().all(|item| item.contains("restoring")));
    }

    #[test]
    fn invalid_or_unusable_input_is_reported_without_generated_code() {
        let style = GraphStyle { axis_x_line_mm: Some(f64::NAN), ..Default::default() };
        assert!(generate(&style, "p").is_err());
        assert!(generate(&GraphStyle::default(), "bad\nname").is_err());
    }
}
