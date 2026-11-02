//! Panel-specific ggplot rendering overrides. None means the R plot's own theme wins.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LegendPosition {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
    Inside,
    None,
}

impl LegendPosition {
    pub(super) const ALL: [Self; 6] = [Self::Right, Self::Left, Self::Top, Self::Bottom, Self::Inside, Self::None];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Right => "Right",
            Self::Left => "Left",
            Self::Top => "Top",
            Self::Bottom => "Bottom",
            Self::Inside => "Inside",
            Self::None => "None",
        }
    }

    fn wire(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Inside => "inside",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FontFace {
    Plain,
    Bold,
    Italic,
    BoldItalic,
}

impl FontFace {
    pub(super) const ALL: [Self; 4] = [Self::Plain, Self::Bold, Self::Italic, Self::BoldItalic];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Plain => "Plain",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::BoldItalic => "Bold italic",
        }
    }
    fn wire(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::BoldItalic => "bold.italic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LegendDirection {
    Horizontal,
    Vertical,
}

impl LegendDirection {
    pub(super) const ALL: [Self; 2] = [Self::Horizontal, Self::Vertical];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
        }
    }
    fn wire(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TitlePosition {
    Panel,
    Plot,
}

impl TitlePosition {
    pub(super) const ALL: [Self; 2] = [Self::Panel, Self::Plot];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Panel => "Panel",
            Self::Plot => "Plot",
        }
    }
    fn wire(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::Plot => "plot",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TextDetails {
    pub family: Option<String>,
    pub face: Option<FontFace>,
    pub colour: Option<String>,
    pub angle: Option<f64>,
    pub hjust: Option<f64>,
    pub vjust: Option<f64>,
}

impl TextDetails {
    fn validate(&self, label: &str) -> Result<(), String> {
        if let Some(family) = &self.family
            && (family.is_empty() || family.len() > 128 || family.chars().any(char::is_control))
        {
            return Err(format!("{label} font family must be 1–128 printable characters"));
        }
        if let Some(colour) = &self.colour {
            validate_colour(colour, label)?;
        }
        if self.angle.is_some_and(|value| !value.is_finite() || !(-360.0..=360.0).contains(&value)) {
            return Err(format!("{label} angle must be between -360 and 360 degrees"));
        }
        for (name, value) in [("horizontal", self.hjust), ("vertical", self.vjust)] {
            if value.is_some_and(|number| !number.is_finite() || !(0.0..=1.0).contains(&number)) {
                return Err(format!("{label} {name} justification must be between 0 and 1"));
            }
        }
        Ok(())
    }

    fn append_wire(&self, prefix: &str, fields: &mut Vec<String>) {
        push_string(fields, &format!("{prefix}.family"), self.family.as_deref());
        push_enum(fields, &format!("{prefix}.face"), self.face.map(FontFace::wire));
        push_string(fields, &format!("{prefix}.colour"), self.colour.as_deref());
        push_number(fields, &format!("{prefix}.angle"), self.angle);
        push_number(fields, &format!("{prefix}.hjust"), self.hjust);
        push_number(fields, &format!("{prefix}.vjust"), self.vjust);
    }
}

fn validate_colour(colour: &str, label: &str) -> Result<(), String> {
    let hex = colour.strip_prefix('#').is_some_and(|digits| matches!(digits.len(), 6 | 8) && digits.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let named = !colour.is_empty() && colour.len() <= 64 && colour.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if hex || named { Ok(()) } else { Err(format!("{label} colour must be an R colour name or #RRGGBB[AA]")) }
}

fn hex(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    encoded
}

fn push_string(fields: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.push(format!("{key}=h{}", hex(value)));
    }
}

fn push_enum(fields: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.push(format!("{key}={value}"));
    }
}

fn push_number(fields: &mut Vec<String>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        fields.push(format!("{key}={value:.6}"));
    }
}

fn push_flag(fields: &mut Vec<String>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        fields.push(format!("{key}={}", if value { "1" } else { "0" }));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GraphStyle {
    pub axis_x_line_mm: Option<f64>,
    pub axis_y_line_mm: Option<f64>,
    pub axis_x_visible: Option<bool>,
    pub axis_y_visible: Option<bool>,
    pub major_grid_visible: Option<bool>,
    pub tick_x_pt: Option<f64>,
    pub tick_y_pt: Option<f64>,
    pub plot_title_pt: Option<f64>,
    pub axis_title_x_pt: Option<f64>,
    pub axis_title_y_pt: Option<f64>,
    pub legend_title_pt: Option<f64>,
    pub legend_text_pt: Option<f64>,
    pub plot_title_visible: Option<bool>,
    pub axis_title_x_visible: Option<bool>,
    pub axis_title_y_visible: Option<bool>,
    pub tick_x_visible: Option<bool>,
    pub tick_y_visible: Option<bool>,
    pub legend_text_visible: Option<bool>,
    pub legend_position: Option<LegendPosition>,
    pub legend_title_visible: Option<bool>,
    pub legend_key_mm: Option<f64>,
    pub inside_x: Option<f64>,
    pub inside_y: Option<f64>,
    pub title_details: TextDetails,
    pub subtitle_details: TextDetails,
    pub caption_details: TextDetails,
    pub axis_title_x_details: TextDetails,
    pub axis_title_y_details: TextDetails,
    pub tick_x_details: TextDetails,
    pub tick_y_details: TextDetails,
    pub legend_title_details: TextDetails,
    pub legend_text_details: TextDetails,
    pub strip_text_details: TextDetails,
    pub subtitle_pt: Option<f64>,
    pub caption_pt: Option<f64>,
    pub strip_text_pt: Option<f64>,
    pub subtitle_visible: Option<bool>,
    pub caption_visible: Option<bool>,
    pub strip_text_visible: Option<bool>,
    pub plot_title_position: Option<TitlePosition>,
    pub axis_x_line_colour: Option<String>,
    pub axis_y_line_colour: Option<String>,
    pub axis_x_ticks_visible: Option<bool>,
    pub axis_y_ticks_visible: Option<bool>,
    pub axis_x_tick_colour: Option<String>,
    pub axis_y_tick_colour: Option<String>,
    pub axis_x_tick_width_mm: Option<f64>,
    pub axis_y_tick_width_mm: Option<f64>,
    pub axis_x_tick_length_mm: Option<f64>,
    pub axis_y_tick_length_mm: Option<f64>,
    pub major_grid_x_visible: Option<bool>,
    pub major_grid_y_visible: Option<bool>,
    pub minor_grid_x_visible: Option<bool>,
    pub minor_grid_y_visible: Option<bool>,
    pub major_grid_colour: Option<String>,
    pub minor_grid_colour: Option<String>,
    pub major_grid_width_mm: Option<f64>,
    pub minor_grid_width_mm: Option<f64>,
    pub panel_background: Option<String>,
    pub plot_background: Option<String>,
    pub panel_border_visible: Option<bool>,
    pub panel_border_colour: Option<String>,
    pub panel_border_width_mm: Option<f64>,
    pub legend_direction: Option<LegendDirection>,
    pub legend_justification: Option<[f64; 2]>,
    pub legend_key_width_mm: Option<f64>,
    pub legend_key_height_mm: Option<f64>,
    pub legend_key_background: Option<String>,
    pub legend_background: Option<String>,
    pub legend_spacing_x_mm: Option<f64>,
    pub legend_spacing_y_mm: Option<f64>,
    pub legend_margin_mm: Option<[f64; 4]>,
    pub plot_margin_mm: Option<[f64; 4]>,
    pub strip_background: Option<String>,
    pub panel_spacing_mm: Option<f64>,
}

impl GraphStyle {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("X tick text", self.tick_x_pt),
            ("Y tick text", self.tick_y_pt),
            ("Plot title", self.plot_title_pt),
            ("X-axis title", self.axis_title_x_pt),
            ("Y-axis title", self.axis_title_y_pt),
            ("Legend title", self.legend_title_pt),
            ("Legend text", self.legend_text_pt),
            ("Plot subtitle", self.subtitle_pt),
            ("Plot caption", self.caption_pt),
            ("Facet strip", self.strip_text_pt),
        ] {
            if value.is_some_and(|number| !number.is_finite() || !(0.1..=100.0).contains(&number)) {
                return Err(format!("{name} must be between 0.1 and 100 pt"));
            }
        }
        for (name, value) in [("X-axis line width", self.axis_x_line_mm), ("Y-axis line width", self.axis_y_line_mm)] {
            if value.is_some_and(|number| !number.is_finite() || !(0.1..=20.0).contains(&number)) {
                return Err(format!("{name} must be between 0.1 and 20 mm"));
            }
        }
        if self.legend_key_mm.is_some_and(|number| !number.is_finite() || !(0.1..=100.0).contains(&number)) {
            return Err("Legend key must be between 0.1 and 100 mm".into());
        }
        for (name, value) in [("Inside legend X", self.inside_x), ("Inside legend Y", self.inside_y)] {
            if value.is_some_and(|number| !number.is_finite() || !(0.0..=1.0).contains(&number)) {
                return Err(format!("{name} must be between 0 and 1"));
            }
        }
        for (name, details) in [
            ("Plot title", &self.title_details),
            ("Plot subtitle", &self.subtitle_details),
            ("Plot caption", &self.caption_details),
            ("X-axis title", &self.axis_title_x_details),
            ("Y-axis title", &self.axis_title_y_details),
            ("X tick labels", &self.tick_x_details),
            ("Y tick labels", &self.tick_y_details),
            ("Legend title", &self.legend_title_details),
            ("Legend text", &self.legend_text_details),
            ("Facet strip", &self.strip_text_details),
        ] {
            details.validate(name)?;
        }
        for (name, colour) in [
            ("X-axis line", &self.axis_x_line_colour),
            ("Y-axis line", &self.axis_y_line_colour),
            ("X tick", &self.axis_x_tick_colour),
            ("Y tick", &self.axis_y_tick_colour),
            ("Major grid", &self.major_grid_colour),
            ("Minor grid", &self.minor_grid_colour),
            ("Panel background", &self.panel_background),
            ("Plot background", &self.plot_background),
            ("Panel border", &self.panel_border_colour),
            ("Legend key", &self.legend_key_background),
            ("Legend background", &self.legend_background),
            ("Facet strip background", &self.strip_background),
        ] {
            if let Some(colour) = colour {
                validate_colour(colour, name)?;
            }
        }
        for (name, value) in [
            ("X tick width", self.axis_x_tick_width_mm),
            ("Y tick width", self.axis_y_tick_width_mm),
            ("Major grid width", self.major_grid_width_mm),
            ("Minor grid width", self.minor_grid_width_mm),
            ("Panel border width", self.panel_border_width_mm),
        ] {
            if value.is_some_and(|number| !number.is_finite() || !(0.01..=20.0).contains(&number)) {
                return Err(format!("{name} must be between 0.01 and 20 mm"));
            }
        }
        for (name, value) in [
            ("X tick length", self.axis_x_tick_length_mm),
            ("Y tick length", self.axis_y_tick_length_mm),
            ("Legend key width", self.legend_key_width_mm),
            ("Legend key height", self.legend_key_height_mm),
            ("Legend X spacing", self.legend_spacing_x_mm),
            ("Legend Y spacing", self.legend_spacing_y_mm),
            ("Panel spacing", self.panel_spacing_mm),
        ] {
            if value.is_some_and(|number| !number.is_finite() || !(0.0..=100.0).contains(&number)) {
                return Err(format!("{name} must be between 0 and 100 mm"));
            }
        }
        if self.legend_justification.is_some_and(|values| values.iter().any(|number| !number.is_finite() || !(0.0..=1.0).contains(number))) {
            return Err("Legend justification must be between 0 and 1".into());
        }
        for (name, margins) in [("Legend margins", self.legend_margin_mm), ("Plot margins", self.plot_margin_mm)] {
            if margins.is_some_and(|values| values.iter().any(|value| !value.is_finite() || !(-100.0..=100.0).contains(value))) {
                return Err(format!("{name} must be between -100 and 100 mm"));
            }
        }
        Ok(())
    }

    /// Preserve the 23 legacy positions; append named ASCII extension fields without JSON.
    pub(crate) fn wire(&self) -> Result<String, String> {
        self.validate()?;
        fn number(value: Option<f64>) -> String {
            value.map_or_else(|| "-".into(), |value| format!("{value:.6}"))
        }
        fn boolean(value: Option<bool>) -> &'static str {
            match value {
                Some(true) => "1",
                Some(false) => "0",
                None => "-",
            }
        }
        let mut fields = vec![
            number(self.axis_x_line_mm),
            number(self.axis_y_line_mm),
            boolean(self.axis_x_visible).into(),
            boolean(self.axis_y_visible).into(),
            boolean(self.major_grid_visible).into(),
            number(self.tick_x_pt),
            number(self.tick_y_pt),
            number(self.plot_title_pt),
            number(self.axis_title_x_pt),
            number(self.axis_title_y_pt),
            number(self.legend_title_pt),
            number(self.legend_text_pt),
            self.legend_position.map_or("-", LegendPosition::wire).into(),
            boolean(self.legend_title_visible).into(),
            number(self.legend_key_mm),
            number(self.inside_x),
            number(self.inside_y),
            boolean(self.plot_title_visible).into(),
            boolean(self.axis_title_x_visible).into(),
            boolean(self.axis_title_y_visible).into(),
            boolean(self.tick_x_visible).into(),
            boolean(self.tick_y_visible).into(),
            boolean(self.legend_text_visible).into(),
        ];
        for (prefix, details) in [
            ("plot.title", &self.title_details),
            ("plot.subtitle", &self.subtitle_details),
            ("plot.caption", &self.caption_details),
            ("axis.title.x", &self.axis_title_x_details),
            ("axis.title.y", &self.axis_title_y_details),
            ("axis.text.x", &self.tick_x_details),
            ("axis.text.y", &self.tick_y_details),
            ("legend.title", &self.legend_title_details),
            ("legend.text", &self.legend_text_details),
            ("strip.text", &self.strip_text_details),
        ] {
            details.append_wire(prefix, &mut fields);
        }
        for (key, value) in [
            ("plot.subtitle.size", self.subtitle_pt),
            ("plot.caption.size", self.caption_pt),
            ("strip.text.size", self.strip_text_pt),
            ("axis.ticks.x.width", self.axis_x_tick_width_mm),
            ("axis.ticks.y.width", self.axis_y_tick_width_mm),
            ("axis.ticks.x.length", self.axis_x_tick_length_mm),
            ("axis.ticks.y.length", self.axis_y_tick_length_mm),
            ("panel.grid.major.width", self.major_grid_width_mm),
            ("panel.grid.minor.width", self.minor_grid_width_mm),
            ("panel.border.width", self.panel_border_width_mm),
            ("legend.key.width", self.legend_key_width_mm),
            ("legend.key.height", self.legend_key_height_mm),
            ("legend.spacing.x", self.legend_spacing_x_mm),
            ("legend.spacing.y", self.legend_spacing_y_mm),
            ("panel.spacing", self.panel_spacing_mm),
        ] {
            push_number(&mut fields, key, value);
        }
        for (key, value) in [
            ("plot.subtitle.visible", self.subtitle_visible),
            ("plot.caption.visible", self.caption_visible),
            ("strip.text.visible", self.strip_text_visible),
            ("axis.ticks.x.visible", self.axis_x_ticks_visible),
            ("axis.ticks.y.visible", self.axis_y_ticks_visible),
            ("panel.grid.major.x.visible", self.major_grid_x_visible),
            ("panel.grid.major.y.visible", self.major_grid_y_visible),
            ("panel.grid.minor.x.visible", self.minor_grid_x_visible),
            ("panel.grid.minor.y.visible", self.minor_grid_y_visible),
            ("panel.border.visible", self.panel_border_visible),
        ] {
            push_flag(&mut fields, key, value);
        }
        for (key, value) in [
            ("axis.line.x.colour", &self.axis_x_line_colour),
            ("axis.line.y.colour", &self.axis_y_line_colour),
            ("axis.ticks.x.colour", &self.axis_x_tick_colour),
            ("axis.ticks.y.colour", &self.axis_y_tick_colour),
            ("panel.grid.major.colour", &self.major_grid_colour),
            ("panel.grid.minor.colour", &self.minor_grid_colour),
            ("panel.background.fill", &self.panel_background),
            ("plot.background.fill", &self.plot_background),
            ("panel.border.colour", &self.panel_border_colour),
            ("legend.key.fill", &self.legend_key_background),
            ("legend.background.fill", &self.legend_background),
            ("strip.background.fill", &self.strip_background),
        ] {
            push_string(&mut fields, key, value.as_deref());
        }
        push_enum(&mut fields, "plot.title.position", self.plot_title_position.map(TitlePosition::wire));
        push_enum(&mut fields, "legend.direction", self.legend_direction.map(LegendDirection::wire));
        if let Some([x, y]) = self.legend_justification {
            push_number(&mut fields, "legend.justification.x", Some(x));
            push_number(&mut fields, "legend.justification.y", Some(y));
        }
        for (prefix, margins) in [("legend.margin", self.legend_margin_mm), ("plot.margin", self.plot_margin_mm)] {
            if let Some([top, right, bottom, left]) = margins {
                for (side, value) in [("top", top), ("right", right), ("bottom", bottom), ("left", left)] {
                    push_number(&mut fields, &format!("{prefix}.{side}"), Some(value));
                }
            }
        }
        Ok(fields.join("|"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_has_fixed_fields_and_rejects_invalid_numbers() {
        assert_eq!(GraphStyle::default().wire().unwrap().split('|').count(), 23);
        let style = GraphStyle { axis_x_line_mm: Some(f64::NAN), ..Default::default() };
        assert!(style.wire().is_err());
    }

    #[test]
    fn visibility_is_independent_of_typography_and_old_saved_styles_remain_valid() {
        let mut style = GraphStyle { tick_x_pt: Some(18.0), tick_x_visible: Some(false), ..Default::default() };
        let fields: Vec<_> = style.wire().unwrap().split('|').map(str::to_owned).collect();
        assert_eq!(fields[5], "18.000000");
        assert_eq!(fields[20], "0");
        style.tick_x_visible = Some(true);
        assert_eq!(style.tick_x_pt, Some(18.0));
        assert_eq!(style.wire().unwrap().split('|').nth(20), Some("1"));
        style.tick_x_visible = None;
        assert_eq!(style.wire().unwrap().split('|').nth(20), Some("-"));
        let old: GraphStyle = serde_json::from_str(r#"{"tick_x_pt":18.0,"legend_title_visible":false}"#).unwrap();
        assert_eq!(old.tick_x_pt, Some(18.0));
        assert_eq!(old.tick_x_visible, None);
        assert_eq!(old.legend_title_visible, Some(false));
    }

    #[test]
    fn advanced_fields_append_without_changing_the_legacy_wire_prefix() {
        let legacy = GraphStyle { plot_title_pt: Some(13.0), tick_x_visible: Some(false), ..Default::default() };
        let mut advanced = legacy.clone();
        advanced.tick_x_details = TextDetails {
            family: Some("Noto Sans KR".into()),
            face: Some(FontFace::BoldItalic),
            colour: Some("#336699".into()),
            angle: Some(45.0),
            hjust: Some(1.0),
            vjust: Some(0.5),
        };
        advanced.major_grid_x_visible = Some(false);
        advanced.minor_grid_y_visible = Some(true);
        advanced.panel_background = Some("ivory".into());
        advanced.plot_margin_mm = Some([2.0, 3.0, 4.0, 5.0]);
        let legacy_wire = legacy.wire().unwrap();
        let advanced_wire = advanced.wire().unwrap();
        assert_eq!(advanced_wire.split('|').take(23).collect::<Vec<_>>(), legacy_wire.split('|').collect::<Vec<_>>());
        assert!(advanced_wire.contains("axis.text.x.family=h4e6f746f2053616e73204b52"));
        assert!(advanced_wire.contains("axis.text.x.face=bold.italic"));
        assert!(advanced_wire.contains("panel.grid.major.x.visible=0"));
        let saved = serde_json::to_string(&advanced).unwrap();
        assert_eq!(serde_json::from_str::<GraphStyle>(&saved).unwrap(), advanced);
        let old: GraphStyle = serde_json::from_str(r#"{"plot_title_pt":13.0}"#).unwrap();
        assert!(old.title_details.family.is_none() && old.plot_margin_mm.is_none());
    }

    #[test]
    fn advanced_validation_rejects_bad_colours_and_numbers_without_losing_inherit() {
        let mut style = GraphStyle::default();
        assert_eq!(style.wire().unwrap().split('|').count(), 23);
        style.plot_background = Some("bad colour!".into());
        assert!(style.validate().is_err());
        style.plot_background = None;
        style.tick_x_details.angle = Some(f64::NAN);
        assert!(style.validate().is_err());
        style.tick_x_details.angle = None;
        style.legend_margin_mm = Some([0.0, 0.0, f64::INFINITY, 0.0]);
        assert!(style.validate().is_err());
        style.legend_margin_mm = None;
        assert!(style.is_default());
    }
}
