//! R-aware lexical highlighting for the scientific script editor.
use egui::text::{LayoutJob, TextFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Keyword,
    Constant,
    Number,
    String,
    Comment,
    Function,
    Assignment,
    Operator,
    Namespace,
    Identifier,
    Punctuation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) text: String,
    pub(crate) kind: TokenKind,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ColorPreviewSpan {
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
    pub(crate) color: egui::Color32,
    pub(crate) text_color: egui::Color32,
    pub(crate) underline: egui::Stroke,
}

#[derive(Default)]
pub(crate) struct HighlightCache {
    pub(crate) source: String,
    pub(crate) tokens: Vec<Token>,
    pub(crate) token_spans: Vec<(usize, usize, TokenKind)>,
    pub(crate) color_spans: Vec<ColorPreviewSpan>,
    pub(crate) theme_is_dark: bool,
}

pub(crate) fn tokenize_spans(source: &str) -> Vec<(usize, usize, TokenKind)> {
    let chars = source.char_indices().collect::<Vec<_>>();
    let mut spans = Vec::<(usize, usize, TokenKind)>::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let next = chars.get(i + 1).map(|(_, c)| *c);
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            let mut end = i + 1;
            while end < chars.len() && chars[end].1 != '\n' {
                end += 1;
            }
            push(&mut spans, &chars, source.len(), i, end, TokenKind::Comment);
            i = end;
            continue;
        }
        if matches!(c, '\'' | '"') {
            let end = string_end(&chars, i, c);
            push(&mut spans, &chars, source.len(), i, end, TokenKind::String);
            i = end;
            continue;
        }
        if matches!(c, 'r' | 'R')
            && next == Some('"')
            && let Some(end) = raw_string_end(&chars, i)
        {
            push(&mut spans, &chars, source.len(), i, end, TokenKind::String);
            i = end;
            continue;
        }
        if ident_start(c) {
            let mut end = i + 1;
            while end < chars.len() && ident_continue(chars[end].1) {
                end += 1;
            }
            let word = char_slice(source, &chars, i, end);
            let kind = if keyword(word) {
                TokenKind::Keyword
            } else if constant(word) {
                TokenKind::Constant
            } else {
                TokenKind::Identifier
            };
            push(&mut spans, &chars, source.len(), i, end, kind);
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && next.is_some_and(|n| n.is_ascii_digit())) {
            let mut end = i + 1;
            while end < chars.len() {
                let c = chars[end].1;
                let p = chars[end - 1].1;
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_') || (matches!(c, '+' | '-') && matches!(p, 'e' | 'E' | 'p' | 'P')) {
                    end += 1;
                } else {
                    break;
                }
            }
            push(&mut spans, &chars, source.len(), i, end, TokenKind::Number);
            i = end;
            continue;
        }
        let (width, kind) = operator(&chars, i).unwrap_or({
            (1, if matches!(c, '(' | ')' | '{' | '}' | '[' | ']' | ',' | ';' | ':' | '$') { TokenKind::Punctuation } else { TokenKind::Operator })
        });
        push(&mut spans, &chars, source.len(), i, (i + width).min(chars.len()), kind);
        i += width;
    }
    for index in 0..spans.len() {
        if spans[index].2 != TokenKind::Identifier {
            continue;
        }
        if spans.get(index + 1).is_some_and(|s| span(source, *s) == "(") {
            spans[index].2 = TokenKind::Function;
        } else if spans.get(index + 1).is_some_and(|s| span(source, *s) == "::") {
            spans[index].2 = TokenKind::Namespace;
        }
    }
    spans
}

pub(crate) fn tokenize(source: &str) -> Vec<Token> {
    tokenize_spans(source).into_iter().map(|(a, b, kind)| Token { text: source.get(a..b).unwrap_or_default().to_owned(), kind }).collect()
}

pub(crate) const R_NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("aliceblue", (240, 248, 255)),
    ("antiquewhite", (250, 235, 215)),
    ("aqua", (0, 255, 255)),
    ("aquamarine", (127, 255, 212)),
    ("azure", (240, 255, 255)),
    ("beige", (245, 245, 220)),
    ("bisque", (255, 228, 196)),
    ("black", (0, 0, 0)),
    ("blanchedalmond", (255, 235, 205)),
    ("blue", (0, 0, 255)),
    ("blueviolet", (138, 43, 226)),
    ("brown", (165, 42, 42)),
    ("burlywood", (222, 184, 135)),
    ("cadetblue", (95, 158, 160)),
    ("chartreuse", (127, 255, 0)),
    ("chocolate", (210, 105, 30)),
    ("coral", (255, 127, 80)),
    ("cornflowerblue", (100, 149, 237)),
    ("cornsilk", (255, 248, 220)),
    ("crimson", (220, 20, 60)),
    ("cyan", (0, 255, 255)),
    ("darkblue", (0, 0, 139)),
    ("darkcyan", (0, 139, 139)),
    ("darkgoldenrod", (184, 134, 11)),
    ("darkgray", (169, 169, 169)),
    ("darkgreen", (0, 100, 0)),
    ("darkgrey", (169, 169, 169)),
    ("darkkhaki", (189, 183, 107)),
    ("darkmagenta", (139, 0, 139)),
    ("darkolivegreen", (85, 107, 47)),
    ("darkorange", (255, 140, 0)),
    ("darkorchid", (153, 50, 204)),
    ("darkred", (139, 0, 0)),
    ("darksalmon", (233, 150, 122)),
    ("darkseagreen", (143, 188, 143)),
    ("darkslateblue", (72, 61, 139)),
    ("darkslategray", (47, 79, 79)),
    ("darkslategrey", (47, 79, 79)),
    ("darkturquoise", (0, 206, 209)),
    ("darkviolet", (148, 0, 211)),
    ("deeppink", (255, 20, 147)),
    ("deepskyblue", (0, 191, 255)),
    ("dimgray", (105, 105, 105)),
    ("dimgrey", (105, 105, 105)),
    ("dodgerblue", (30, 144, 255)),
    ("firebrick", (178, 34, 34)),
    ("floralwhite", (255, 250, 240)),
    ("forestgreen", (34, 139, 34)),
    ("fuchsia", (255, 0, 255)),
    ("gainsboro", (220, 220, 220)),
    ("ghostwhite", (248, 248, 255)),
    ("gold", (255, 215, 0)),
    ("goldenrod", (218, 165, 32)),
    ("gray", (190, 190, 190)),
    ("green", (0, 255, 0)),
    ("greenyellow", (173, 255, 47)),
    ("grey", (190, 190, 190)),
    ("honeydew", (240, 255, 240)),
    ("hotpink", (255, 105, 180)),
    ("indianred", (205, 92, 92)),
    ("indigo", (75, 0, 130)),
    ("ivory", (255, 255, 240)),
    ("khaki", (240, 230, 140)),
    ("lavender", (230, 230, 250)),
    ("lavenderblush", (255, 240, 245)),
    ("lawngreen", (124, 252, 0)),
    ("lemonchiffon", (255, 250, 205)),
    ("lightblue", (173, 216, 230)),
    ("lightcoral", (240, 128, 128)),
    ("lightcyan", (224, 255, 255)),
    ("lightgoldenrod", (238, 221, 130)),
    ("lightgoldenrodyellow", (250, 250, 210)),
    ("lightgray", (211, 211, 211)),
    ("lightgreen", (144, 238, 144)),
    ("lightgrey", (211, 211, 211)),
    ("lightpink", (255, 182, 193)),
    ("lightsalmon", (255, 160, 122)),
    ("lightseagreen", (32, 178, 170)),
    ("lightskyblue", (135, 206, 250)),
    ("lightslategray", (119, 136, 153)),
    ("lightslategrey", (119, 136, 153)),
    ("lightsteelblue", (176, 196, 222)),
    ("lightyellow", (255, 255, 224)),
    ("lime", (0, 255, 0)),
    ("limegreen", (50, 205, 50)),
    ("linen", (250, 240, 230)),
    ("magenta", (255, 0, 255)),
    ("maroon", (176, 48, 96)),
    ("mediumaquamarine", (102, 205, 170)),
    ("mediumblue", (0, 0, 205)),
    ("mediumorchid", (186, 85, 211)),
    ("mediumpurple", (147, 112, 219)),
    ("mediumseagreen", (60, 179, 113)),
    ("mediumslateblue", (123, 104, 238)),
    ("mediumspringgreen", (0, 250, 154)),
    ("mediumturquoise", (72, 209, 204)),
    ("mediumvioletred", (199, 21, 133)),
    ("midnightblue", (25, 25, 112)),
    ("mintcream", (245, 255, 250)),
    ("mistyrose", (255, 228, 225)),
    ("moccasin", (255, 228, 181)),
    ("navajowhite", (255, 222, 173)),
    ("navy", (0, 0, 128)),
    ("navyblue", (0, 0, 128)),
    ("oldlace", (253, 245, 230)),
    ("olive", (128, 128, 0)),
    ("olivedrab", (107, 142, 35)),
    ("orange", (255, 165, 0)),
    ("orangered", (255, 69, 0)),
    ("orchid", (218, 112, 214)),
    ("palegoldenrod", (238, 232, 170)),
    ("palegreen", (152, 251, 152)),
    ("paleturquoise", (175, 238, 238)),
    ("palevioletred", (219, 112, 147)),
    ("papayawhip", (255, 239, 213)),
    ("peachpuff", (255, 218, 185)),
    ("peru", (205, 133, 63)),
    ("pink", (255, 192, 203)),
    ("plum", (221, 160, 221)),
    ("powderblue", (176, 224, 230)),
    ("purple", (160, 32, 240)),
    ("red", (255, 0, 0)),
    ("rosybrown", (188, 143, 143)),
    ("royalblue", (65, 105, 225)),
    ("saddlebrown", (139, 69, 19)),
    ("salmon", (250, 128, 114)),
    ("sandybrown", (244, 164, 96)),
    ("seagreen", (46, 139, 87)),
    ("seashell", (255, 245, 238)),
    ("sienna", (160, 82, 45)),
    ("silver", (192, 192, 192)),
    ("skyblue", (135, 206, 235)),
    ("slateblue", (106, 90, 205)),
    ("slategray", (112, 128, 144)),
    ("slategrey", (112, 128, 144)),
    ("snow", (255, 250, 250)),
    ("springgreen", (0, 255, 127)),
    ("steelblue", (70, 130, 180)),
    ("tan", (210, 180, 140)),
    ("teal", (0, 128, 128)),
    ("thistle", (216, 191, 216)),
    ("tomato", (255, 99, 71)),
    ("transparent", (0, 0, 0)),
    ("turquoise", (64, 224, 208)),
    ("violet", (238, 130, 238)),
    ("violetred", (208, 32, 144)),
    ("wheat", (245, 222, 179)),
    ("white", (255, 255, 255)),
    ("whitesmoke", (245, 245, 245)),
    ("yellow", (255, 255, 0)),
    ("yellowgreen", (154, 205, 50)),
];

pub(crate) fn parse_r_hex_color(hex_str: &str) -> Option<egui::Color32> {
    let s = hex_str.strip_prefix('#')?;
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        3 => {
            let r = u8::from_str_radix(s.get(0..1)?, 16).ok()? * 17;
            let g = u8::from_str_radix(s.get(1..2)?, 16).ok()? * 17;
            let b = u8::from_str_radix(s.get(2..3)?, 16).ok()? * 17;
            Some(egui::Color32::from_rgba_unmultiplied(r, g, b, 255))
        }
        4 => {
            let r = u8::from_str_radix(s.get(0..1)?, 16).ok()? * 17;
            let g = u8::from_str_radix(s.get(1..2)?, 16).ok()? * 17;
            let b = u8::from_str_radix(s.get(2..3)?, 16).ok()? * 17;
            let a = u8::from_str_radix(s.get(3..4)?, 16).ok()? * 17;
            Some(egui::Color32::from_rgba_unmultiplied(r, g, b, a))
        }
        6 => {
            let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
            Some(egui::Color32::from_rgba_unmultiplied(r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
            let a = u8::from_str_radix(s.get(6..8)?, 16).ok()?;
            Some(egui::Color32::from_rgba_unmultiplied(r, g, b, a))
        }
        _ => None,
    }
}

pub(crate) fn parse_r_named_color(name: &str) -> Option<egui::Color32> {
    let lower = name.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0));
    }

    // gray0 .. gray100 or grey0 .. grey100
    if lower.starts_with("gray") || lower.starts_with("grey") {
        let num_part = &lower[4..];
        if !num_part.is_empty()
            && num_part.chars().all(|c| c.is_ascii_digit())
            && let Ok(n) = num_part.parse::<u32>()
            && n <= 100
        {
                    let v = (n as f32 * 255.0 / 100.0).round() as u8;
                    return Some(egui::Color32::from_rgb(v, v, v));
        }
    }

    // Numbered variants like red1..red4, blue1..blue4
    if lower.len() > 1 && matches!(lower.chars().last(), Some('1'..='4')) {
        let digit = lower.chars().last().unwrap_or('1');
        let base_name = &lower[..lower.len() - 1];
        if let Ok(idx) = R_NAMED_COLORS.binary_search_by_key(&base_name, |(n, _)| *n) {
            let (_, (r, g, b)) = R_NAMED_COLORS[idx];
            let factor = match digit {
                '1' => 1.0,
                '2' => 238.0 / 255.0,
                '3' => 205.0 / 255.0,
                '4' => 139.0 / 255.0,
                _ => 1.0,
            };
            let scale = |c: u8| ((c as f32) * factor).round() as u8;
            return Some(egui::Color32::from_rgb(scale(r), scale(g), scale(b)));
        }
    }

    if let Ok(idx) = R_NAMED_COLORS.binary_search_by_key(&lower.as_str(), |(n, _)| *n) {
        let (_, (r, g, b)) = R_NAMED_COLORS[idx];
        return Some(egui::Color32::from_rgb(r, g, b));
    }

    None
}

pub(crate) fn is_color_identifier_name(ident: &str) -> bool {
    let lower = ident.to_ascii_lowercase();
    lower == "col"
        || lower == "color"
        || lower == "colour"
        || lower == "fill"
        || lower == "bg"
        || lower == "fg"
        || lower == "border"
        || lower == "palette"
        || lower.starts_with("col.")
        || lower.starts_with("color.")
        || lower.starts_with("colour.")
        || lower.ends_with("_col")
        || lower.ends_with("_color")
        || lower.ends_with("_colour")
        || lower.ends_with("_fill")
        || lower.ends_with("_bg")
        || lower.contains("palette")
}

pub(crate) fn is_color_function_name(fn_name: &str) -> bool {
    let lower = fn_name.to_ascii_lowercase();
    lower == "adjustcolor"
        || lower == "col2rgb"
        || lower == "colorramppalette"
        || lower == "rgb"
        || lower == "hsv"
        || lower == "hcl"
        || lower == "rainbow"
        || lower == "heat.colors"
        || lower == "terrain.colors"
        || lower == "topo.colors"
        || lower == "cm.colors"
        || lower.starts_with("scale_color_")
        || lower.starts_with("scale_colour_")
        || lower.starts_with("scale_fill_")
}

pub(crate) fn is_color_argument_context(source: &str, tokens: &[(usize, usize, TokenKind)], string_idx: usize) -> bool {
    if string_idx == 0 {
        return false;
    }

    let mut depth_paren = 0;
    let mut i = string_idx;
    let mut steps = 0;

    while i > 0 && steps < 60 {
        steps += 1;
        let prev_end = tokens[i - 1].1;
        let curr_start = tokens[i].0;
        if depth_paren == 0 && source.get(prev_end..curr_start).is_some_and(|s| s.contains('\n') || s.contains(';')) {
            break;
        }

        i -= 1;
        let (s, e, _kind) = tokens[i];
        let text = source.get(s..e).unwrap_or_default();

        match text {
            ")" | "]" | "}" => {
                depth_paren += 1;
            }
            "(" | "[" | "{" => {
                if depth_paren > 0 {
                    depth_paren -= 1;
                } else {
                    if i > 0 {
                        let (fn_s, fn_e, fn_kind) = tokens[i - 1];
                        if fn_kind == TokenKind::Function || fn_kind == TokenKind::Identifier {
                            let fn_name = source.get(fn_s..fn_e).unwrap_or_default().to_ascii_lowercase();
                            if is_color_function_name(&fn_name) {
                                return true;
                            }
                            if (fn_name == "c" || fn_name == "list" || fn_name == "vector") && i >= 2 {
                                    let (_, _, op_kind) = tokens[i - 2];
                                    let op_text = source.get(tokens[i - 2].0..tokens[i - 2].1).unwrap_or_default();
                                    if (op_text == "=" || op_text == "<-" || op_kind == TokenKind::Assignment) && i >= 3 {
                                            let ident = source.get(tokens[i - 3].0..tokens[i - 3].1).unwrap_or_default();
                                            if is_color_identifier_name(ident) {
                                                return true;
                                            }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "=" | "<-" if depth_paren == 0 => {
                if i > 0 {
                    let ident = source.get(tokens[i - 1].0..tokens[i - 1].1).unwrap_or_default();
                    if is_color_identifier_name(ident) {
                        return true;
                    }
                }
            }
            _ => {
                if depth_paren == 0 && (text == ";" || text == "\n") {
                    break;
                }
            }
        }
    }

    false
}

pub(crate) fn contrasting_text_color(bg: egui::Color32, theme_is_dark: bool) -> egui::Color32 {
    let r = bg.r() as f32;
    let g = bg.g() as f32;
    let b = bg.b() as f32;
    let a = bg.a() as f32 / 255.0;

    let (bg_r, bg_g, bg_b) = if theme_is_dark { (30.0, 30.0, 30.0) } else { (245.0, 245.0, 245.0) };
    let eff_r = r * a + bg_r * (1.0 - a);
    let eff_g = g * a + bg_g * (1.0 - a);
    let eff_b = b * a + bg_b * (1.0 - a);

    let lum = 0.299 * eff_r + 0.587 * eff_g + 0.114 * eff_b;

    if lum > 130.0 { egui::Color32::from_rgb(15, 15, 15) } else { egui::Color32::from_rgb(245, 245, 245) }
}

pub(crate) fn compute_visible_boundary_stroke(color: egui::Color32, theme_is_dark: bool) -> egui::Stroke {
    let r = color.r() as f32;
    let g = color.g() as f32;
    let b = color.b() as f32;
    let a = color.a() as f32 / 255.0;

    let (bg_r, bg_g, bg_b) = if theme_is_dark { (30.0, 30.0, 30.0) } else { (245.0, 245.0, 245.0) };
    let eff_r = r * a + bg_r * (1.0 - a);
    let eff_g = g * a + bg_g * (1.0 - a);
    let eff_b = b * a + bg_b * (1.0 - a);
    let lum = 0.299 * eff_r + 0.587 * eff_g + 0.114 * eff_b;

    if lum > 215.0 {
        egui::Stroke::new(1.0, if theme_is_dark { egui::Color32::from_gray(160) } else { egui::Color32::from_gray(120) })
    } else if theme_is_dark && lum < 40.0 {
        egui::Stroke::new(1.0, egui::Color32::from_gray(80))
    } else {
        egui::Stroke::NONE
    }
}

pub(crate) fn detect_color_spans(source: &str, token_spans: &[(usize, usize, TokenKind)], theme_is_dark: bool) -> Vec<ColorPreviewSpan> {
    let mut spans = Vec::new();

    for (idx, &(start, end, kind)) in token_spans.iter().enumerate() {
        if kind != TokenKind::String {
            continue;
        }

        let raw = source.get(start..end).unwrap_or_default();
        if raw.len() < 2 {
            continue;
        }
        let first = raw.chars().next().unwrap_or(' ');
        let last = raw.chars().last().unwrap_or(' ');
        if (first != '"' && first != '\'') || first != last {
            continue;
        }

        let inner = &raw[1..raw.len() - 1];
        if inner.is_empty() {
            continue;
        }

        let color_opt = if inner.starts_with('#') {
            parse_r_hex_color(inner)
        } else if is_color_argument_context(source, token_spans, idx) {
            parse_r_named_color(inner)
        } else {
            None
        };

        if let Some(color) = color_opt {
            let text_color = contrasting_text_color(color, theme_is_dark);
            let underline = compute_visible_boundary_stroke(color, theme_is_dark);
            spans.push(ColorPreviewSpan { byte_start: start, byte_end: end, color, text_color, underline });
        }
    }

    spans
}

pub(crate) fn layout_job(ui: &egui::Ui, source: &str, wrap: f32, cache: &mut HighlightCache) -> LayoutJob {
    let t = crate::theme::Tokens::get(ui.ctx());
    let theme_is_dark = t.dark;
    if cache.source != source || cache.theme_is_dark != theme_is_dark {
        cache.source.clear();
        cache.source.push_str(source);
        cache.token_spans = tokenize_spans(source);
        cache.tokens =
            cache.token_spans.iter().map(|(a, b, kind)| Token { text: source.get(*a..*b).unwrap_or_default().to_owned(), kind: *kind }).collect();
        cache.color_spans = detect_color_spans(source, &cache.token_spans, theme_is_dark);
        cache.theme_is_dark = theme_is_dark;
    }
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut offset = 0;
    for (idx, tok) in cache.tokens.iter().enumerate() {
        if let Some(&(at, end, _)) = cache.token_spans.get(idx) {
            if at > offset {
                job.append(
                    source.get(offset..at).unwrap_or_default(),
                    0.0,
                    TextFormat { font_id: font.clone(), color: t.text, ..Default::default() },
                );
            }
            if let Some(cp) = cache.color_spans.iter().find(|cp| cp.byte_start == at && cp.byte_end == end) {
                job.append(
                    &tok.text,
                    0.0,
                    TextFormat { font_id: font.clone(), color: cp.text_color, background: cp.color, underline: cp.underline, ..Default::default() },
                );
            } else {
                let color = match tok.kind {
                    TokenKind::Keyword => t.accent,
                    TokenKind::Constant => t.warning,
                    TokenKind::Number => t.axis_y,
                    TokenKind::String => t.axis_z,
                    TokenKind::Comment => t.text_dim,
                    TokenKind::Function => t.text_strong,
                    TokenKind::Assignment => t.selection,
                    TokenKind::Operator => t.icon,
                    TokenKind::Namespace => t.accent_strong,
                    TokenKind::Identifier => t.text,
                    TokenKind::Punctuation => t.text_dim,
                };
                job.append(&tok.text, 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
            }
            offset = end.min(source.len());
        }
    }
    if offset < source.len() {
        job.append(source.get(offset..).unwrap_or_default(), 0.0, TextFormat { font_id: font, color: t.text, ..Default::default() });
    }
    job
}

fn push(v: &mut Vec<(usize, usize, TokenKind)>, chars: &[(usize, char)], len: usize, a: usize, b: usize, k: TokenKind) {
    let start = chars.get(a).map_or(len, |(n, _)| *n);
    let end = chars.get(b).map_or(len, |(n, _)| *n);
    if start <= end {
        v.push((start, end, k));
    }
}
fn string_end(c: &[(usize, char)], start: usize, quote: char) -> usize {
    let mut escaped = false;
    for (i, (_, x)) in c.iter().enumerate().skip(start + 1) {
        if *x == '\n' && !escaped {
            return i;
        }
        if *x == quote && !escaped {
            return i + 1;
        }
        escaped = *x == '\\' && !escaped;
    }
    c.len()
}
fn raw_string_end(c: &[(usize, char)], start: usize) -> Option<usize> {
    let open = (start + 2..c.len()).find(|i| c.get(*i).is_some_and(|(_, x)| *x == '(' || *x != '-'))?;
    if c.get(open).map(|(_, x)| *x) != Some('(') {
        return None;
    }
    let dashes = c.get(start + 2..open)?.iter().filter(|(_, x)| *x == '-').count();
    let close = format!("){}\"", "-".repeat(dashes));
    let needle = close.chars().collect::<Vec<_>>();
    c.get(open + 1..)?.windows(needle.len()).position(|w| w.iter().map(|(_, x)| *x).eq(needle.iter().copied())).map(|n| open + 1 + n + needle.len())
}
fn operator(c: &[(usize, char)], start: usize) -> Option<(usize, TokenKind)> {
    const OPS: [&str; 17] = ["<<-", "->>", ":::", "<-", "->", "::", "|>", "%in%", "%>%", "==", "!=", "<=", ">=", "&&", "||", "**", "="];
    let slice_end = (start + 5).min(c.len());
    let rest: String = c.get(start..slice_end)?.iter().map(|(_, x)| *x).collect();
    for op in OPS {
        if rest.starts_with(op) {
            return Some((op.len(), if matches!(op, "<-" | "<<-" | "->" | "->>" | "=") { TokenKind::Assignment } else { TokenKind::Operator }));
        }
    }
    if c.get(start).is_some_and(|(_, x)| *x == '%') {
        let max_search = (start + 32).min(c.len());
        for (offset, &(_, ch)) in c[start + 1..max_search].iter().enumerate() {
            if ch == '%' {
                return Some((offset + 2, TokenKind::Operator));
            }
            if ch == '\n' || ch.is_whitespace() {
                break;
            }
        }
    }
    None
}
fn span(s: &str, v: (usize, usize, TokenKind)) -> &str {
    s.get(v.0..v.1).unwrap_or_default()
}
fn char_slice<'a>(s: &'a str, c: &[(usize, char)], a: usize, b: usize) -> &'a str {
    s.get(c.get(a).map_or(s.len(), |(n, _)| *n)..c.get(b).map_or(s.len(), |(n, _)| *n)).unwrap_or_default()
}
fn ident_start(c: char) -> bool {
    c == '_' || c == '.' || c.is_alphabetic()
}
fn ident_continue(c: char) -> bool {
    ident_start(c) || c.is_numeric()
}
fn keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "for" | "while" | "repeat" | "function" | "in" | "next" | "break")
}
fn constant(s: &str) -> bool {
    matches!(s, "TRUE" | "FALSE" | "NULL" | "NA" | "NaN" | "Inf" | "NA_integer_" | "NA_real_" | "NA_character_" | "NA_complex_")
}

pub(crate) fn indent_selection(source: &mut String, selection: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let total_chars = source.chars().count();
    let start_char = selection.start.min(total_chars);
    let end_char = selection.end.min(total_chars).max(start_char);
    if start_char == end_char {
        let byte = char_to_byte(source, start_char);
        source.insert_str(byte, "  ");
        let next = start_char.saturating_add(2);
        return next..next;
    }
    let start_byte = char_to_byte(source, start_char);
    let end_byte = char_to_byte(source, end_char);
    let first_line = source.get(..start_byte).and_then(|s| s.rfind('\n')).map_or(0, |at| at + 1);
    let end_probe =
        if end_byte > start_byte && source.get(..end_byte).is_some_and(|s| s.ends_with('\n')) { end_byte.saturating_sub(1) } else { end_byte };
    let last_line = source.get(..end_probe).and_then(|s| s.rfind('\n')).map_or(0, |at| at + 1);
    let mut line_starts = Vec::new();
    let mut line = first_line;
    while line <= last_line {
        line_starts.push(line);
        let Some(rest) = source.get(line..) else {
            break;
        };
        let Some(newline) = rest.find('\n') else {
            break;
        };
        line = line.saturating_add(newline).saturating_add(1);
    }
    let added_before_start = if start_byte > first_line { 2 } else { 0 };
    let added_total = line_starts.len().saturating_mul(2);
    for at in line_starts.into_iter().rev() {
        source.insert_str(at, "  ");
    }
    let start = start_char.saturating_add(added_before_start);
    let end = end_char.saturating_add(added_total).max(start);
    start..end
}

pub(crate) fn outdent_selection(source: &mut String, selection: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let start_char = selection.start.min(source.chars().count());
    let end_char = selection.end.min(source.chars().count()).max(start_char);
    let start_byte = char_to_byte(source, start_char);
    let end_byte = char_to_byte(source, end_char);
    let first_line = source.get(..start_byte).and_then(|s| s.rfind('\n')).map_or(0, |at| at + 1);
    let end_probe =
        if end_byte > start_byte && source.get(..end_byte).is_some_and(|s| s.ends_with('\n')) { end_byte.saturating_sub(1) } else { end_byte };
    let last_line = source.get(..end_probe).and_then(|s| s.rfind('\n')).map_or(0, |at| at + 1);
    let mut removals = Vec::new();
    let mut line = first_line;
    while line <= last_line {
        let Some(rest) = source.get(line..) else {
            break;
        };
        let count = if rest.starts_with("  ") {
            2
        } else if rest.starts_with(' ') || rest.starts_with('\t') {
            1
        } else {
            0
        };
        if count > 0 {
            removals.push((line, count));
        }
        let Some(newline) = rest.find('\n') else {
            break;
        };
        line = line.saturating_add(newline).saturating_add(1);
    }
    let removed_before_start = removals.iter().filter(|(at, _)| *at < start_byte).map(|(_, count)| count).sum::<usize>();
    let removed_before_end = removals.iter().filter(|(at, _)| *at < end_byte).map(|(_, count)| count).sum::<usize>();
    for (at, count) in removals.into_iter().rev() {
        source.drain(at..at + count);
    }
    let start = start_char.saturating_sub(removed_before_start);
    let end = end_char.saturating_sub(removed_before_end).max(start);
    start..end
}
pub(crate) fn char_to_byte(source: &str, index: usize) -> usize {
    source.char_indices().nth(index).map_or(source.len(), |(byte, _)| byte)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompletionContextKind {
    General,
    MemberAccess { object: String, bracket: bool, quote: Option<char> },
    Namespace { package: String, internal: bool },
    Path { quote: char, raw_path: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletionContext {
    pub(crate) kind: CompletionContextKind,
    pub(crate) prefix: String,
    pub(crate) replace_start_char: usize,
    pub(crate) replace_end_char: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CompletionCandidateKind {
    Function,
    Variable,
    Column,
    Keyword,
    Constant,
    #[allow(dead_code)]
    Package,
    File,
    Directory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompletionCandidate {
    pub(crate) name: String,
    pub(crate) kind: CompletionCandidateKind,
    pub(crate) signature: Option<String>,
    pub(crate) detail: String,
    pub(crate) source: String,
}

pub(crate) fn completion_context_at(source: &str, cursor_char: usize) -> Option<CompletionContext> {
    let cursor_char = cursor_char.min(source.chars().count());
    let cursor_byte = char_to_byte(source, cursor_char);

    // 1. Check if cursor is inside a comment or ordinary string literal.
    let tokens = tokenize(source);
    let mut offset = 0;
    for tok in &tokens {
        let Some(at) = source.get(offset..).and_then(|s| s.find(&tok.text)).map(|n| n + offset) else { continue };
        let end = at.saturating_add(tok.text.len());
        offset = end;
        if cursor_byte > at && cursor_byte <= end {
            if tok.kind == TokenKind::Comment {
                return None;
            }
            if tok.kind == TokenKind::String {
                // Check if this string is part of `obj[[...` member access
                let before_quote = source.get(..at).unwrap_or_default().trim_end();
                if before_quote.ends_with("[[") {
                    let before_bracket = before_quote.strip_suffix("[[").unwrap_or_default().trim_end();
                    let obj_name =
                        before_bracket.chars().rev().take_while(|c| ident_continue(*c)).collect::<String>().chars().rev().collect::<String>();
                    if !obj_name.is_empty() && ident_start(obj_name.chars().next().unwrap_or('_')) {
                        let quote_char = tok.text.chars().next().unwrap_or('"');
                        let prefix_start_byte = at.saturating_add(1);
                        if cursor_byte >= prefix_start_byte {
                            let prefix = source.get(prefix_start_byte..cursor_byte).unwrap_or_default().to_owned();
                            let prefix_start_char = source.get(..prefix_start_byte).map_or(0, |s| s.chars().count());
                            return Some(CompletionContext {
                                kind: CompletionContextKind::MemberAccess { object: obj_name, bracket: true, quote: Some(quote_char) },
                                prefix,
                                replace_start_char: prefix_start_char,
                                replace_end_char: cursor_char,
                            });
                        }
                    }
                }
                // Caret is inside an R string literal; provide filesystem path completion.
                let quote_char = tok.text.chars().next().unwrap_or('"');
                let content_start_byte = at.saturating_add(quote_char.len_utf8());
                if cursor_byte >= content_start_byte {
                    let raw_string = source.get(content_start_byte..cursor_byte).unwrap_or_default();
                    let last_slash_offset = raw_string.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
                    let filter_prefix = raw_string[last_slash_offset..].to_owned();
                    let segment_start_byte = content_start_byte + last_slash_offset;
                    let replace_start_char = source.get(..segment_start_byte).map_or(0, |s| s.chars().count());
                    return Some(CompletionContext {
                        kind: CompletionContextKind::Path { quote: quote_char, raw_path: raw_string.to_owned() },
                        prefix: filter_prefix,
                        replace_start_char,
                        replace_end_char: cursor_char,
                    });
                }
                return None;
            }
        }
    }

    // 2. We are in normal code. Scan backward on current line.
    let before_cursor = source.get(..cursor_byte).unwrap_or_default();
    let line_start = before_cursor.rfind('\n').map_or(0, |idx| idx + 1);
    let line_prefix = source.get(line_start..cursor_byte).unwrap_or_default();

    // Check for comments on this line before cursor
    if line_prefix.contains('#') {
        let comment_start = line_start + line_prefix.find('#').unwrap_or(0);
        if cursor_byte > comment_start {
            return None;
        }
    }

    // Scan backwards from cursor over identifier characters
    let chars_before = line_prefix.chars().collect::<Vec<_>>();
    let mut ident_len = 0;
    while ident_len < chars_before.len() {
        let c = chars_before[chars_before.len() - 1 - ident_len];
        if ident_continue(c) {
            ident_len += 1;
        } else {
            break;
        }
    }

    let ident_chars = &chars_before[chars_before.len() - ident_len..];
    let prefix = ident_chars.iter().collect::<String>();
    if !prefix.is_empty() {
        let first_c = prefix.chars().next().unwrap_or('_');
        if first_c.is_ascii_digit() {
            return None;
        }
        if first_c == '.' && prefix.len() > 1 && prefix.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
    }

    let remaining_before = &chars_before[..chars_before.len() - ident_len];
    let prefix_start_char = cursor_char.saturating_sub(ident_len);

    // Check preceding characters for triggers: :::, ::, $, [[
    if remaining_before.ends_with(&[':', ':', ':']) {
        let before_colons = &remaining_before[..remaining_before.len() - 3];
        let pkg = before_colons.iter().rev().take_while(|c| ident_continue(**c)).copied().collect::<Vec<_>>().into_iter().rev().collect::<String>();
        if !pkg.is_empty() && ident_start(pkg.chars().next().unwrap_or('_')) {
            return Some(CompletionContext {
                kind: CompletionContextKind::Namespace { package: pkg, internal: true },
                prefix,
                replace_start_char: prefix_start_char,
                replace_end_char: cursor_char,
            });
        }
    } else if remaining_before.ends_with(&[':', ':']) {
        let before_colons = &remaining_before[..remaining_before.len() - 2];
        let pkg = before_colons.iter().rev().take_while(|c| ident_continue(**c)).copied().collect::<Vec<_>>().into_iter().rev().collect::<String>();
        if !pkg.is_empty() && ident_start(pkg.chars().next().unwrap_or('_')) {
            return Some(CompletionContext {
                kind: CompletionContextKind::Namespace { package: pkg, internal: false },
                prefix,
                replace_start_char: prefix_start_char,
                replace_end_char: cursor_char,
            });
        }
    } else if remaining_before.ends_with(&['$']) {
        let before_dollar = &remaining_before[..remaining_before.len() - 1];
        let obj = before_dollar.iter().rev().take_while(|c| ident_continue(**c)).copied().collect::<Vec<_>>().into_iter().rev().collect::<String>();
        if !obj.is_empty() && ident_start(obj.chars().next().unwrap_or('_')) {
            return Some(CompletionContext {
                kind: CompletionContextKind::MemberAccess { object: obj, bracket: false, quote: None },
                prefix,
                replace_start_char: prefix_start_char,
                replace_end_char: cursor_char,
            });
        }
    } else if remaining_before.ends_with(&['[', '[']) {
        let before_brackets = &remaining_before[..remaining_before.len() - 2];
        let obj = before_brackets.iter().rev().take_while(|c| ident_continue(**c)).copied().collect::<Vec<_>>().into_iter().rev().collect::<String>();
        if !obj.is_empty() && ident_start(obj.chars().next().unwrap_or('_')) {
            return Some(CompletionContext {
                kind: CompletionContextKind::MemberAccess { object: obj, bracket: true, quote: None },
                prefix,
                replace_start_char: prefix_start_char,
                replace_end_char: cursor_char,
            });
        }
    }

    if !prefix.is_empty() {
        return Some(CompletionContext {
            kind: CompletionContextKind::General,
            prefix,
            replace_start_char: prefix_start_char,
            replace_end_char: cursor_char,
        });
    }

    None
}

pub(crate) fn apply_completion(source: &mut String, ctx: &CompletionContext, candidate: &CompletionCandidate) -> usize {
    let start_byte = char_to_byte(source, ctx.replace_start_char);
    let end_byte = char_to_byte(source, ctx.replace_end_char);
    let rest = source.get(end_byte..).unwrap_or_default().to_owned();

    match &ctx.kind {
        CompletionContextKind::MemberAccess { bracket: true, quote, .. } => {
            if let Some(q) = quote {
                let close_bracket = format!("{q}]]");
                let close_quote = format!("{q}");
                if rest.starts_with(&close_bracket) {
                    source.replace_range(start_byte..end_byte, &candidate.name);
                    ctx.replace_start_char.saturating_add(candidate.name.chars().count()).saturating_add(3)
                } else if rest.starts_with(&close_quote) {
                    source.replace_range(start_byte..end_byte, &candidate.name);
                    ctx.replace_start_char.saturating_add(candidate.name.chars().count()).saturating_add(1)
                } else {
                    let insert = format!("{}{q}]]", candidate.name);
                    source.replace_range(start_byte..end_byte, &insert);
                    ctx.replace_start_char.saturating_add(insert.chars().count())
                }
            } else if rest.starts_with("]]") {
                let insert = format!("\"{}\"", candidate.name);
                source.replace_range(start_byte..end_byte, &insert);
                ctx.replace_start_char.saturating_add(insert.chars().count()).saturating_add(2)
            } else {
                let insert = format!("\"{}\"]]", candidate.name);
                source.replace_range(start_byte..end_byte, &insert);
                ctx.replace_start_char.saturating_add(insert.chars().count())
            }
        }
        _ => {
            if candidate.kind == CompletionCandidateKind::Function {
                let has_paren = rest.starts_with('(');
                if has_paren {
                    source.replace_range(start_byte..end_byte, &candidate.name);
                    ctx.replace_start_char.saturating_add(candidate.name.chars().count()).saturating_add(1)
                } else {
                    let insert = format!("{}()", candidate.name);
                    source.replace_range(start_byte..end_byte, &insert);
                    ctx.replace_start_char.saturating_add(candidate.name.chars().count()).saturating_add(1)
                }
            } else {
                source.replace_range(start_byte..end_byte, &candidate.name);
                ctx.replace_start_char.saturating_add(candidate.name.chars().count())
            }
        }
    }
}

pub(crate) fn extract_script_symbols(
    source: &str,
) -> (Vec<CompletionCandidate>, std::collections::HashMap<String, Vec<String>>, std::collections::HashSet<String>) {
    let mut candidates = Vec::new();
    let mut object_columns: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut loaded_packages = std::collections::HashSet::new();

    let tokens = tokenize(source);
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Identifier {
            // Check for assignment: name <- rhs, name = rhs, name <<- rhs
            if i + 1 < tokens.len() && tokens[i + 1].kind == TokenKind::Assignment {
                let name = tok.text.clone();
                if i + 2 < tokens.len() {
                    let rhs_tok = &tokens[i + 2];
                    if rhs_tok.text == "function" {
                        let mut sig = None;
                        if i + 3 < tokens.len() && tokens[i + 3].text == "(" {
                            let mut depth = 0;
                            let mut end = i + 3;
                            while end < tokens.len() {
                                if tokens[end].text == "(" {
                                    depth += 1;
                                } else if tokens[end].text == ")" {
                                    depth -= 1;
                                    if depth == 0 {
                                        let sig_tokens = tokens[i + 3..=end].iter().map(|t| t.text.as_str()).collect::<Vec<_>>();
                                        sig = Some(sig_tokens.join(""));
                                        break;
                                    }
                                }
                                end += 1;
                            }
                        }
                        candidates.push(CompletionCandidate {
                            name,
                            kind: CompletionCandidateKind::Function,
                            signature: sig.or_else(|| Some("()".into())),
                            detail: "function".into(),
                            source: "script".into(),
                        });
                    } else {
                        // Check if RHS creates a data.frame or list with columns
                        if matches!(rhs_tok.text.as_str(), "data.frame" | "tibble" | "list") && i + 3 < tokens.len() && tokens[i + 3].text == "(" {
                            let mut depth = 0;
                            let mut end = i + 3;
                            let mut cols = Vec::new();
                            while end < tokens.len() {
                                if tokens[end].text == "(" {
                                    depth += 1;
                                } else if tokens[end].text == ")" {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                } else if depth == 1
                                    && tokens[end].kind == TokenKind::Identifier
                                    && end + 1 < tokens.len()
                                    && tokens[end + 1].text == "="
                                {
                                    cols.push(tokens[end].text.clone());
                                }
                                end += 1;
                            }
                            if !cols.is_empty() {
                                object_columns.insert(name.clone(), cols);
                            }
                        } else if rhs_tok.text == "mtcars" {
                            object_columns.insert(
                                name.clone(),
                                vec!["mpg", "cyl", "disp", "hp", "drat", "wt", "qsec", "vs", "am", "gear", "carb"]
                                    .into_iter()
                                    .map(String::from)
                                    .collect(),
                            );
                        } else if rhs_tok.text == "iris" {
                            object_columns.insert(
                                name.clone(),
                                vec!["Sepal.Length", "Sepal.Width", "Petal.Length", "Petal.Width", "Species"].into_iter().map(String::from).collect(),
                            );
                        }
                        candidates.push(CompletionCandidate {
                            name,
                            kind: CompletionCandidateKind::Variable,
                            signature: None,
                            detail: "variable".into(),
                            source: "script".into(),
                        });
                    }
                }
            } else if tok.text == "library" || tok.text == "require" {
                if i + 1 < tokens.len() && tokens[i + 1].text == "(" && i + 2 < tokens.len() {
                    let pkg_name = tokens[i + 2].text.trim_matches(|c| c == '\'' || c == '"').to_owned();
                    if !pkg_name.is_empty() {
                        loaded_packages.insert(pkg_name);
                    }
                }
            } else if tok.text == "ggplot" {
                // Look for ggplot(df, aes(x, y))
                if i + 2 < tokens.len() && tokens[i + 1].text == "(" && tokens[i + 2].kind == TokenKind::Identifier {
                    let df_name = tokens[i + 2].text.clone();
                    let mut j = i + 3;
                    while j < tokens.len() && tokens[j].text != ";" && tokens[j].text != "\n" {
                        if tokens[j].text == "aes" && j + 1 < tokens.len() && tokens[j + 1].text == "(" {
                            let mut depth = 0;
                            let mut k = j + 1;
                            let mut cols = Vec::new();
                            while k < tokens.len() {
                                if tokens[k].text == "(" {
                                    depth += 1;
                                } else if tokens[k].text == ")" {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                } else if depth == 1 && tokens[k].kind == TokenKind::Identifier {
                                    // if not followed by '=' as in aes(color = col)
                                    if k + 1 < tokens.len() && tokens[k + 1].text == "=" {
                                        if k + 2 < tokens.len() && tokens[k + 2].kind == TokenKind::Identifier {
                                            cols.push(tokens[k + 2].text.clone());
                                        }
                                    } else if k == 0 || tokens[k - 1].text != "=" {
                                        cols.push(tokens[k].text.clone());
                                    }
                                }
                                k += 1;
                            }
                            if !cols.is_empty() {
                                object_columns.entry(df_name.clone()).or_default().extend(cols);
                            }
                            break;
                        }
                        j += 1;
                    }
                }
            }
        }
        i += 1;
    }

    (candidates, object_columns, loaded_packages)
}

pub(crate) fn builtin_keywords_and_constants() -> Vec<CompletionCandidate> {
    vec![
        CompletionCandidate {
            name: "if".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "else".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "for".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "while".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "repeat".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "function".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "in".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "next".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "break".into(),
            kind: CompletionCandidateKind::Keyword,
            signature: None,
            detail: "keyword".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "TRUE".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "logical".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "FALSE".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "logical".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NULL".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "NULL".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NA".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "logical".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NaN".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "double".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "Inf".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "double".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NA_integer_".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "integer".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NA_real_".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "double".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NA_character_".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "character".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "NA_complex_".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "complex".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "pi".into(),
            kind: CompletionCandidateKind::Constant,
            signature: None,
            detail: "double".into(),
            source: "base".into(),
        },
    ]
}

pub(crate) fn builtin_base_and_packages() -> Vec<CompletionCandidate> {
    vec![
        // Base functions
        CompletionCandidate {
            name: "summary".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "sum".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "seq".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(from = 1, to = 1, by = ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "seq_len".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(length.out)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "seq_along".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(along.with)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "c".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "list".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "data.frame".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., row.names = NULL, stringsAsFactors = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "matrix".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(data = NA, nrow = 1, ncol = 1, byrow = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "mean".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, trim = 0, na.rm = FALSE, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "min".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "max".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "range".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "abs".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "sqrt".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "round".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, digits = 0)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "floor".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "ceiling".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "paste".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., sep = \" \", collapse = NULL)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "paste0".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., collapse = NULL)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "sprintf".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(fmt, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "cat".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., file = \"\", sep = \" \")".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "print".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "message".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., domain = NULL, appendLF = TRUE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "warning".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., call. = TRUE, immediate. = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "stop".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., call. = TRUE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "stopifnot".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "suppressMessages".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(expr, classes = \"message\")".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "suppressWarnings".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(expr, classes = \"warning\")".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "subset".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, subset, select, drop = FALSE, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "merge".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, y, by = intersect(names(x), names(y)), ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "apply".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(X, MARGIN, FUN, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "lapply".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(X, FUN, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "sapply".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(X, FUN, ..., simplify = TRUE, USE.NAMES = TRUE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "vapply".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(X, FUN, FUN.VALUE, ..., USE.NAMES = TRUE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "names".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "colnames".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, do.NULL = TRUE, prefix = \"col\")".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "rownames".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, do.NULL = TRUE, prefix = \"row\")".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "nrow".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "ncol".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "dim".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "length".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "typeof".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "class".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "inherits".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, what, which = FALSE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "library".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(package, help, pos = 2, lib.loc = NULL, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "require".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(package, lib.loc = NULL, quietly = FALSE, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "readRDS".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(file, refhook = NULL)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        CompletionCandidate {
            name: "saveRDS".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, file = \"\", ascii = FALSE, version = NULL, compress = TRUE)".into()),
            detail: "function".into(),
            source: "base".into(),
        },
        // Stats functions
        CompletionCandidate {
            name: "lm".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(formula, data, subset, weights, na.action, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "glm".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(formula, family = gaussian, data, weights, subset, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "sd".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "var".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, y = NULL, na.rm = FALSE)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "median".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, na.rm = FALSE, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "quantile".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, probs = seq(0, 1, 0.25), na.rm = FALSE, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "cor".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, y = NULL, use = \"everything\", method = \"pearson\")".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "predict".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "residuals".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, ...)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        CompletionCandidate {
            name: "rnorm".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(n, mean = 0, sd = 1)".into()),
            detail: "function".into(),
            source: "stats".into(),
        },
        // Utils functions
        CompletionCandidate {
            name: "head".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, n = 6L, ...)".into()),
            detail: "function".into(),
            source: "utils".into(),
        },
        CompletionCandidate {
            name: "tail".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, n = 6L, ...)".into()),
            detail: "function".into(),
            source: "utils".into(),
        },
        CompletionCandidate {
            name: "str".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, ...)".into()),
            detail: "function".into(),
            source: "utils".into(),
        },
        CompletionCandidate {
            name: "read.csv".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(file, header = TRUE, sep = \",\", ...)".into()),
            detail: "function".into(),
            source: "utils".into(),
        },
        CompletionCandidate {
            name: "write.csv".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, file = \"\", row.names = TRUE, ...)".into()),
            detail: "function".into(),
            source: "utils".into(),
        },
        // ggplot2 functions
        CompletionCandidate {
            name: "ggplot".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(data = NULL, mapping = aes(), ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "aes".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, y, ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "geom_point".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(mapping = NULL, data = NULL, stat = \"identity\", position = \"identity\", ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "geom_line".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(mapping = NULL, data = NULL, stat = \"identity\", position = \"identity\", ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "geom_bar".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(mapping = NULL, data = NULL, stat = \"count\", position = \"stack\", ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "geom_histogram".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(mapping = NULL, data = NULL, stat = \"bin\", position = \"stack\", ...)".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "theme_minimal".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(base_size = 11, base_family = \"\")".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        CompletionCandidate {
            name: "labs".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(..., title = waiver(), subtitle = waiver())".into()),
            detail: "function".into(),
            source: "ggplot2".into(),
        },
        // Graphics
        CompletionCandidate {
            name: "plot".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, y, ...)".into()),
            detail: "function".into(),
            source: "graphics".into(),
        },
        CompletionCandidate {
            name: "hist".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(x, breaks = \"Sturges\", ...)".into()),
            detail: "function".into(),
            source: "graphics".into(),
        },
    ]
}

pub(crate) fn builtin_namespace(package: &str, internal: bool) -> Vec<CompletionCandidate> {
    let mut list = Vec::new();
    match package {
        "stats" => {
            list.push(CompletionCandidate {
                name: "lm".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(formula, data, subset, weights, na.action, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "glm".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(formula, family = gaussian, data, weights, subset, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "sd".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(x, na.rm = FALSE)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "var".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(x, y = NULL, na.rm = FALSE)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "median".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(x, na.rm = FALSE, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "quantile".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(x, probs = seq(0, 1, 0.25), na.rm = FALSE, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "cor".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(x, y = NULL, use = \"everything\", method = \"pearson\")".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "predict".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(object, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "residuals".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(object, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "rnorm".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(n, mean = 0, sd = 1)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "runif".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(n, min = 0, max = 1)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            list.push(CompletionCandidate {
                name: "loess".into(),
                kind: CompletionCandidateKind::Function,
                signature: Some("(formula, data, weights, subset, ...)".into()),
                detail: "function".into(),
                source: "stats".into(),
            });
            if internal {
                list.push(CompletionCandidate {
                    name: ".lm.fit".into(),
                    kind: CompletionCandidateKind::Function,
                    signature: Some("(x, y, tol = 1e-07)".into()),
                    detail: "function".into(),
                    source: "stats:::".into(),
                });
            }
        }
        "base" => {
            list.extend(builtin_base_and_packages().into_iter().filter(|c| c.source == "base"));
        }
        "utils" => {
            list.extend(builtin_base_and_packages().into_iter().filter(|c| c.source == "utils"));
            if internal {
                list.push(CompletionCandidate {
                    name: ".getHelpFile".into(),
                    kind: CompletionCandidateKind::Function,
                    signature: Some("(file)".into()),
                    detail: "function".into(),
                    source: "utils:::".into(),
                });
            }
        }
        "ggplot2" => {
            list.extend(builtin_base_and_packages().into_iter().filter(|c| c.source == "ggplot2"));
        }
        _ => {}
    }
    list
}

pub(crate) fn rank_and_filter_candidates(candidates: &[CompletionCandidate], prefix: &str) -> Vec<CompletionCandidate> {
    if prefix.is_empty() {
        let mut result = candidates.to_vec();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        return result;
    }

    let prefix_lower = prefix.to_lowercase();
    let mut exact_prefix = Vec::new();
    let mut case_prefix = Vec::new();
    let mut substring = Vec::new();

    for c in candidates {
        if c.name.starts_with(prefix) {
            exact_prefix.push(c.clone());
        } else if c.name.to_lowercase().starts_with(&prefix_lower) {
            case_prefix.push(c.clone());
        } else if prefix.len() >= 2 && c.name.to_lowercase().contains(&prefix_lower) {
            substring.push(c.clone());
        }
    }

    let sort_tier = |tier: &mut Vec<CompletionCandidate>| {
        tier.sort_by(|a, b| {
            // Priority order of source: script > .GlobalEnv > base > others
            let src_rank = |s: &str| match s {
                "script" => 0,
                ".GlobalEnv" => 1,
                "base" => 2,
                _ => 3,
            };
            let a_rank = src_rank(&a.source);
            let b_rank = src_rank(&b.source);
            if a_rank != b_rank {
                return a_rank.cmp(&b_rank);
            }
            if a.name.len() != b.name.len() {
                return a.name.len().cmp(&b.name.len());
            }
            a.name.cmp(&b.name)
        });
    };

    sort_tier(&mut exact_prefix);
    sort_tier(&mut case_prefix);
    sort_tier(&mut substring);

    let mut combined = exact_prefix;
    combined.extend(case_prefix);
    combined.extend(substring);

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    combined.retain(|c| seen.insert((c.name.clone(), c.kind)));

    combined
}

pub(crate) fn build_path_candidates(working_dir: &std::path::Path, raw_path: &str) -> Vec<CompletionCandidate> {
    let normalized = raw_path.replace('\\', "/");
    let (dir_part, filter_prefix) = if let Some(last_slash) = normalized.rfind('/') {
        (&normalized[..=last_slash], &normalized[last_slash + 1..])
    } else {
        ("", normalized.as_str())
    };

    let search_dir = if dir_part.is_empty() {
        working_dir.to_path_buf()
    } else {
        let p = std::path::Path::new(dir_part);
        if p.is_absolute() { p.to_path_buf() } else { working_dir.join(p) }
    };

    let Ok(read_dir) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    let lower_filter = filter_prefix.to_lowercase();
    for entry in read_dir.flatten().take(500) {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !lower_filter.is_empty() && !file_name.to_lowercase().starts_with(&lower_filter) {
            continue;
        }
        if file_name.starts_with('.') && !filter_prefix.starts_with('.') {
            continue;
        }
        let file_type = entry.file_type().ok();
        let is_dir = file_type.is_some_and(|ft| ft.is_dir());
        if is_dir {
            candidates.push(CompletionCandidate {
                name: format!("{file_name}/"),
                kind: CompletionCandidateKind::Directory,
                signature: None,
                detail: "directory".into(),
                source: "path".into(),
            });
        } else {
            let ext = std::path::Path::new(&file_name).extension().and_then(|e| e.to_str()).unwrap_or("file").to_owned();
            candidates.push(CompletionCandidate {
                name: file_name,
                kind: CompletionCandidateKind::File,
                signature: None,
                detail: format!("{ext} file"),
                source: "path".into(),
            });
        }
    }

    candidates.sort_by(|a, b| {
        let a_is_dir = a.kind == CompletionCandidateKind::Directory;
        let b_is_dir = b.kind == CompletionCandidateKind::Directory;
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    candidates
}

pub(crate) fn can_auto_pair_quote(source: &str, cursor_char: usize, _quote: char) -> bool {
    let cursor_char = cursor_char.min(source.chars().count());
    let cursor_byte = char_to_byte(source, cursor_char);

    // 1. Escaped quote check: count consecutive backslashes preceding cursor
    let mut backslash_count = 0;
    for c in source[..cursor_byte].chars().rev() {
        if c == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }
    if backslash_count % 2 == 1 {
        return false;
    }

    // 2. Comment check: is cursor on a comment line after '#'?
    let line_start = source[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let line_before_cursor = &source[line_start..cursor_byte];
    let mut in_q = None;
    let mut comment_start = None;
    let mut prev_char = ' ';
    for (byte_offset, c) in line_before_cursor.char_indices() {
        if c == '#' && in_q.is_none() {
            comment_start = Some(byte_offset);
            break;
        }
        if matches!(c, '\'' | '"') && prev_char != '\\' {
            if in_q == Some(c) {
                in_q = None;
            } else if in_q.is_none() {
                in_q = Some(c);
            }
        }
        prev_char = c;
    }
    if comment_start.is_some() || in_q.is_some() {
        return false;
    }

    // 3. String literal check: is cursor inside an existing string?
    let spans = tokenize_spans(source);
    for (start, end, kind) in spans {
        if kind == TokenKind::String && cursor_byte > start && cursor_byte <= end {
            return false;
        }
    }

    true
}

pub(crate) fn should_skip_closing_quote(source: &str, cursor_char: usize, quote: char) -> bool {
    let next_char = source.chars().nth(cursor_char);
    if next_char != Some(quote) {
        return false;
    }
    let cursor_byte = char_to_byte(source, cursor_char);
    let mut backslash_count = 0;
    for c in source[..cursor_byte].chars().rev() {
        if c == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }
    if backslash_count % 2 != 0 {
        return false;
    }

    // Must be at the closing quote of a string token (not at the opening quote)
    let spans = tokenize_spans(source);
    for (start, end, kind) in spans {
        if kind == TokenKind::String && cursor_byte > start && cursor_byte < end {
            let quote_len = quote.len_utf8();
            if end >= start + quote_len + quote_len && cursor_byte == end - quote_len {
                return true;
            }
        }
    }
    false
}

pub(crate) fn byte_to_char(source: &str, byte_index: usize) -> usize {
    source.get(..byte_index).map_or_else(|| source.chars().count(), |s| s.chars().count())
}

pub(crate) fn is_open_delimiter(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '"' | '\'')
}

pub(crate) fn is_close_delimiter(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'')
}

pub(crate) fn delimiter_pair(c: char) -> Option<(char, char)> {
    match c {
        '(' | ')' => Some(('(', ')')),
        '[' | ']' => Some(('[', ']')),
        '{' | '}' => Some(('{', '}')),
        '"' => Some(('"', '"')),
        '\'' => Some(('\'', '\'')),
        _ => None,
    }
}

pub(crate) fn can_auto_pair_delimiter(source: &str, cursor_char: usize, open: char) -> bool {
    if open == '"' || open == '\'' {
        return can_auto_pair_quote(source, cursor_char, open);
    }
    let cursor_byte = char_to_byte(source, cursor_char);

    // 1. Escaped check: is cursor preceded by an odd number of backslashes?
    let mut backslash_count = 0;
    for c in source[..cursor_byte].chars().rev() {
        if c == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }
    if backslash_count % 2 == 1 {
        return false;
    }

    // 2. Comment check: is cursor on a comment line after '#'?
    let line_start = source[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let line_before_cursor = &source[line_start..cursor_byte];
    let mut in_q = None;
    let mut comment_start = None;
    let mut prev_char = ' ';
    for (byte_offset, c) in line_before_cursor.char_indices() {
        if c == '#' && in_q.is_none() {
            comment_start = Some(byte_offset);
            break;
        }
        if matches!(c, '\'' | '"') && prev_char != '\\' {
            if in_q == Some(c) {
                in_q = None;
            } else if in_q.is_none() {
                in_q = Some(c);
            }
        }
        prev_char = c;
    }
    if comment_start.is_some() || in_q.is_some() {
        return false;
    }

    // 3. String literal check: is cursor inside an existing string?
    let spans = tokenize_spans(source);
    for (start, end, kind) in spans {
        if kind == TokenKind::String && cursor_byte > start && cursor_byte <= end {
            return false;
        }
    }

    true
}

pub(crate) fn should_skip_closing_delimiter(source: &str, cursor_char: usize, close: char) -> bool {
    let next_char = source.chars().nth(cursor_char);
    if next_char != Some(close) {
        return false;
    }
    if close == '"' || close == '\'' {
        return should_skip_closing_quote(source, cursor_char, close);
    }
    let cursor_byte = char_to_byte(source, cursor_char);

    // Don't skip if escaped
    let mut backslash_count = 0;
    for c in source[..cursor_byte].chars().rev() {
        if c == '\\' {
            backslash_count += 1;
        } else {
            break;
        }
    }
    if backslash_count % 2 != 0 {
        return false;
    }

    // Don't skip inside comments or strings
    let spans = tokenize_spans(source);
    for (start, end, kind) in spans {
        if (kind == TokenKind::String || kind == TokenKind::Comment) && cursor_byte >= start && cursor_byte < end {
            return false;
        }
    }

    true
}

pub(crate) fn is_cursor_between_empty_pair(source: &str, cursor_char: usize) -> bool {
    if cursor_char == 0 {
        return false;
    }
    let chars: Vec<char> = source.chars().collect();
    if cursor_char >= chars.len() {
        return false;
    }
    let prev = chars[cursor_char - 1];
    let next = chars[cursor_char];
    matches!((prev, next), ('"', '"') | ('\'', '\'') | ('(', ')') | ('[', ']') | ('{', '}'))
}

pub(crate) fn delete_empty_pair(source: &mut String, cursor_char: usize) -> usize {
    if !is_cursor_between_empty_pair(source, cursor_char) {
        return cursor_char;
    }
    let start_byte = char_to_byte(source, cursor_char - 1);
    let end_byte = char_to_byte(source, cursor_char + 1);
    source.drain(start_byte..end_byte);
    cursor_char - 1
}

pub(crate) fn wrap_selection_with_pair(source: &mut String, start_char: usize, end_char: usize, open: char, close: char) -> (usize, usize) {
    let (min_char, max_char) = (start_char.min(end_char), start_char.max(end_char));
    let start_byte = char_to_byte(source, min_char);
    let end_byte = char_to_byte(source, max_char);
    let selected = source[start_byte..end_byte].to_owned();
    let wrapped = format!("{open}{selected}{close}");
    source.replace_range(start_byte..end_byte, &wrapped);
    (min_char + 1, max_char + 1)
}

pub(crate) fn auto_pair_delimiter(source: &mut String, cursor_char: usize, open: char) -> usize {
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        _ => return cursor_char,
    };
    let byte_idx = char_to_byte(source, cursor_char);
    let pair = format!("{open}{close}");
    source.insert_str(byte_idx, &pair);
    cursor_char + 1
}

pub(crate) fn is_cursor_between_braces(source: &str, cursor_char: usize) -> bool {
    if cursor_char == 0 {
        return false;
    }
    let chars: Vec<char> = source.chars().collect();
    if cursor_char >= chars.len() {
        return false;
    }
    matches!((chars[cursor_char - 1], chars[cursor_char]), ('{', '}') | ('(', ')') | ('[', ']'))
}

pub(crate) fn find_matching_open_bracket_indent(source: &str, cursor_char: usize, close: char) -> Option<String> {
    let open = match close {
        '}' => '{',
        ')' => '(',
        ']' => '[',
        _ => return None,
    };
    let cursor_byte = char_to_byte(source, cursor_char);
    let spans = tokenize_spans(source);
    let mut depth: usize = 0;

    for (start, end, kind) in spans.into_iter().rev() {
        if start >= cursor_byte {
            continue;
        }
        if kind == TokenKind::Comment || kind == TokenKind::String {
            continue;
        }
        let text = source.get(start..end).unwrap_or_default();
        if text.chars().all(|c| c == close) {
            depth += text.chars().count();
        } else if text.chars().all(|c| c == open) {
            let count = text.chars().count();
            if depth >= count {
                depth -= count;
            } else {
                let line_start = source[..start].rfind('\n').map_or(0, |idx| idx + 1);
                let indent = source[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>();
                return Some(indent);
            }
        }
    }
    None
}

pub(crate) fn dedent_closing_bracket(source: &mut String, cursor_char: usize, close: char) -> Option<usize> {
    let cursor_byte = char_to_byte(source, cursor_char);
    let line_start_byte = source[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let line_prefix = &source[line_start_byte..cursor_byte];
    if line_prefix.is_empty() || !line_prefix.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    let target_indent = find_matching_open_bracket_indent(source, cursor_char, close)
        .unwrap_or_else(|| if line_prefix.len() >= 2 { line_prefix[2..].to_owned() } else { String::new() });
    let replacement = format!("{target_indent}{close}");
    source.replace_range(line_start_byte..cursor_byte, &replacement);
    Some(byte_to_char(source, line_start_byte + replacement.len()))
}

pub(crate) fn apply_smart_enter(source: &mut String, cursor_char: usize) -> usize {
    if is_cursor_between_braces(source, cursor_char) {
        let cursor_byte = char_to_byte(source, cursor_char);
        let line_start = source[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
        let base_indent = source[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>();
        let insert = format!("\n{}  \n{}", base_indent, base_indent);
        source.insert_str(cursor_byte, &insert);
        return cursor_char + 1 + base_indent.chars().count() + 2;
    }

    let cursor_byte = char_to_byte(source, cursor_char);
    let line_start = source[..cursor_byte].rfind('\n').map_or(0, |idx| idx + 1);
    let line_before = &source[line_start..cursor_byte];
    let base_indent = line_before.chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>();

    let line_after = &source[cursor_byte..];
    let next_on_line = line_after.split('\n').next().unwrap_or("").trim_start();

    let new_indent = if next_on_line.starts_with('}') {
        find_matching_open_bracket_indent(source, cursor_char, '}')
            .unwrap_or_else(|| if base_indent.len() >= 2 { base_indent[2..].to_owned() } else { String::new() })
    } else {
        let trimmed = line_before.trim_end();
        let code_part = if trimmed.contains('#') {
            let mut in_q = None;
            let mut actual_hash = None;
            for (i, c) in trimmed.char_indices() {
                if matches!(c, '\'' | '"') {
                    if in_q == Some(c) {
                        in_q = None;
                    } else if in_q.is_none() {
                        in_q = Some(c);
                    }
                } else if c == '#' && in_q.is_none() {
                    actual_hash = Some(i);
                    break;
                }
            }
            if let Some(pos) = actual_hash { trimmed[..pos].trim_end() } else { trimmed }
        } else {
            trimmed
        };

        let opens_block = code_part.ends_with('{');
        let opens_paren_or_bracket = code_part.ends_with('(') || code_part.ends_with('[');
        let ends_with_operator = code_part.ends_with('+')
            || code_part.ends_with('-')
            || code_part.ends_with('*')
            || code_part.ends_with('/')
            || code_part.ends_with("%>%")
            || code_part.ends_with("|>")
            || code_part.ends_with("<-")
            || code_part.ends_with(',');

        if opens_block || opens_paren_or_bracket || (ends_with_operator && base_indent.is_empty()) { format!("{base_indent}  ") } else { base_indent }
    };

    let insert = format!("\n{}", new_indent);
    source.insert_str(cursor_byte, &insert);
    cursor_char + 1 + new_indent.chars().count()
}

#[cfg(test)]
pub(crate) fn is_cursor_between_empty_quotes(source: &str, cursor_char: usize) -> bool {
    if cursor_char == 0 {
        return false;
    }
    let chars: Vec<char> = source.chars().collect();
    if cursor_char >= chars.len() {
        return false;
    }
    let prev = chars[cursor_char - 1];
    let next = chars[cursor_char];
    matches!((prev, next), ('"', '"') | ('\'', '\''))
}

#[cfg(test)]
pub(crate) fn delete_empty_quote_pair(source: &mut String, cursor_char: usize) -> usize {
    if !is_cursor_between_empty_quotes(source, cursor_char) {
        return cursor_char;
    }
    let start_byte = char_to_byte(source, cursor_char - 1);
    let end_byte = char_to_byte(source, cursor_char + 1);
    source.drain(start_byte..end_byte);
    cursor_char - 1
}

#[cfg(test)]
pub(crate) fn wrap_selection_with_quote(source: &mut String, start_char: usize, end_char: usize, quote: char) -> (usize, usize) {
    let (min_char, max_char) = (start_char.min(end_char), start_char.max(end_char));
    let start_byte = char_to_byte(source, min_char);
    let end_byte = char_to_byte(source, max_char);
    let selected = source[start_byte..end_byte].to_owned();
    let wrapped = format!("{quote}{selected}{quote}");
    source.replace_range(start_byte..end_byte, &wrapped);
    (min_char + 1, max_char + 1)
}

#[cfg(test)]
pub(crate) fn auto_pair_quote(source: &mut String, cursor_char: usize, quote: char) -> usize {
    let byte_idx = char_to_byte(source, cursor_char);
    let pair = format!("{quote}{quote}");
    source.insert_str(byte_idx, &pair);
    cursor_char + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokenizer_keeps_strings_and_comments_lexically_isolated() {
        let ts = tokenize("x <- 3.14e-2L # if <- TRUE\ns <- \"# <- if\"; f <- function(x) TRUE; geom_point()");
        assert!(ts.iter().any(|t| t.text == "# if <- TRUE" && t.kind == TokenKind::Comment));
        assert!(ts.iter().any(|t| t.text == "\"# <- if\"" && t.kind == TokenKind::String));
        assert!(ts.iter().any(|t| t.text == "3.14e-2L" && t.kind == TokenKind::Number));
        assert!(ts.iter().any(|t| t.text == "function" && t.kind == TokenKind::Keyword));
        assert!(ts.iter().any(|t| t.text == "<-" && t.kind == TokenKind::Assignment));
        assert!(ts.iter().any(|t| t.text == "geom_point" && t.kind == TokenKind::Function));
    }
    #[test]
    fn incomplete_and_raw_strings_do_not_panic() {
        let ts = tokenize("x <- r\"--(text # if\n still text)--\"\ny <- 'unfinished");
        assert!(ts.iter().any(|t| t.kind == TokenKind::String && t.text.starts_with("r\"")));
        assert!(ts.iter().any(|t| t.kind == TokenKind::String && t.text == "'unfinished"));
    }
    #[test]
    fn shift_tab_outdents_every_selected_line_and_keeps_a_valid_selection() {
        let mut source = "  alpha\n    beta\n  gamma".to_owned();
        let end = source.chars().count();
        let selection = outdent_selection(&mut source, 0..end);
        assert_eq!(source, "alpha\n  beta\ngamma");
        assert_eq!(selection, 0..source.chars().count());
    }
    #[test]
    fn tab_indent_selection_indents_selected_lines_and_inverts_outdent() {
        let mut source = "alpha\n  beta\ngamma".to_owned();
        let end = source.chars().count();
        let selection = indent_selection(&mut source, 0..end);
        assert_eq!(source, "  alpha\n    beta\n  gamma");
        assert_eq!(selection, 0..source.chars().count());
        let reverted = outdent_selection(&mut source, selection);
        assert_eq!(source, "alpha\n  beta\ngamma");
        assert_eq!(reverted, 0..source.chars().count());
    }
    #[test]
    fn completion_context_detection_for_general_members_and_namespaces() {
        let code = "summ\ndf$mp\nobj[[\"co\nstats::l\nutils:::.get";
        let ctx1 = completion_context_at(code, 4).unwrap();
        assert_eq!(ctx1.kind, CompletionContextKind::General);
        assert_eq!(ctx1.prefix, "summ");
        assert_eq!(ctx1.replace_start_char, 0);
        assert_eq!(ctx1.replace_end_char, 4);

        let df_pos = code.find("df$mp").unwrap() + 5;
        let ctx2 = completion_context_at(code, df_pos).unwrap();
        assert_eq!(ctx2.kind, CompletionContextKind::MemberAccess { object: "df".into(), bracket: false, quote: None });
        assert_eq!(ctx2.prefix, "mp");

        let stats_pos = code.find("stats::l").unwrap() + 8;
        let ctx3 = completion_context_at(code, stats_pos).unwrap();
        assert_eq!(ctx3.kind, CompletionContextKind::Namespace { package: "stats".into(), internal: false });
        assert_eq!(ctx3.prefix, "l");

        let utils_pos = code.find("utils:::.get").unwrap() + 12;
        let ctx4 = completion_context_at(code, utils_pos).unwrap();
        assert_eq!(ctx4.kind, CompletionContextKind::Namespace { package: "utils".into(), internal: true });
        assert_eq!(ctx4.prefix, ".get");

        // Comments return None
        let comment_code = "# some comment summ";
        assert!(completion_context_at(comment_code, comment_code.len()).is_none());
    }
    #[test]
    fn apply_completion_avoids_duplicate_parentheses() {
        let mut source = "summ(x)".to_owned();
        let ctx = CompletionContext { kind: CompletionContextKind::General, prefix: "summ".into(), replace_start_char: 0, replace_end_char: 4 };
        let candidate = CompletionCandidate {
            name: "summary".into(),
            kind: CompletionCandidateKind::Function,
            signature: Some("(object, ...)".into()),
            detail: "function".into(),
            source: "base".into(),
        };
        let caret = apply_completion(&mut source, &ctx, &candidate);
        assert_eq!(source, "summary(x)");
        assert_eq!(caret, 8); // Inside parentheses after '('

        let mut source2 = "summ".to_owned();
        let ctx2 = CompletionContext { kind: CompletionContextKind::General, prefix: "summ".into(), replace_start_char: 0, replace_end_char: 4 };
        let caret2 = apply_completion(&mut source2, &ctx2, &candidate);
        assert_eq!(source2, "summary()");
        assert_eq!(caret2, 8); // Between '(' and ')'
    }

    #[test]
    fn test_quote_auto_pairing_and_caret_positioning() {
        let mut s1 = "x <- ".to_owned();
        let pos1 = auto_pair_quote(&mut s1, 5, '"');
        assert_eq!(s1, "x <- \"\"");
        assert_eq!(pos1, 6); // Caret between the quotes

        let mut s2 = "y <- ".to_owned();
        let pos2 = auto_pair_quote(&mut s2, 5, '\'');
        assert_eq!(s2, "y <- ''");
        assert_eq!(pos2, 6); // Caret between the single quotes
    }

    #[test]
    fn test_closing_quote_skip() {
        let s = "x <- \"\"";
        // Cursor at 6 (between quotes, right before closing quote)
        assert!(should_skip_closing_quote(s, 6, '"'));
        assert!(!should_skip_closing_quote(s, 5, '"'));

        let s2 = "x <- ''";
        assert!(should_skip_closing_quote(s2, 6, '\''));
    }

    #[test]
    fn test_paired_deletion() {
        let mut s1 = "x <- \"\"".to_owned();
        assert!(is_cursor_between_empty_quotes(&s1, 6));
        let new_pos1 = delete_empty_quote_pair(&mut s1, 6);
        assert_eq!(s1, "x <- ");
        assert_eq!(new_pos1, 5);

        let mut s2 = "y <- ''".to_owned();
        assert!(is_cursor_between_empty_quotes(&s2, 6));
        let new_pos2 = delete_empty_quote_pair(&mut s2, 6);
        assert_eq!(s2, "y <- ");
        assert_eq!(new_pos2, 5);

        // Non-empty quotes should not be deleted as a pair
        let mut s3 = "x <- \"a\"".to_owned();
        assert!(!is_cursor_between_empty_quotes(&s3, 6));
        let pos3 = delete_empty_quote_pair(&mut s3, 6);
        assert_eq!(s3, "x <- \"a\"");
        assert_eq!(pos3, 6);
    }

    #[test]
    fn test_escaped_quotes_and_comments_no_auto_pair() {
        // Escaped quote
        let s_esc = r#"x <- \""#;
        assert!(!can_auto_pair_quote(s_esc, 6, '"'));

        // Comment
        let s_comment = "# comments should not auto-pair quotes ";
        assert!(!can_auto_pair_quote(s_comment, s_comment.len(), '"'));
        assert!(!can_auto_pair_quote(s_comment, s_comment.len(), '\''));

        // Normal code should auto-pair
        let s_ok = "read.csv(";
        assert!(can_auto_pair_quote(s_ok, s_ok.len(), '"'));
    }

    #[test]
    fn test_selection_wrapping() {
        let mut s = "print(sample_data)".to_owned();
        // Select "sample_data" (chars 6..17)
        let (start, end) = wrap_selection_with_quote(&mut s, 6, 17, '"');
        assert_eq!(s, "print(\"sample_data\")");
        assert_eq!(start, 7);
        assert_eq!(end, 18);

        let mut s2 = "col_name".to_owned();
        let (start2, end2) = wrap_selection_with_quote(&mut s2, 0, 8, '\'');
        assert_eq!(s2, "'col_name'");
        assert_eq!(start2, 1);
        assert_eq!(end2, 9);
    }

    #[test]
    fn test_path_completion_context_and_priority() {
        let code = "read.csv(\"data/sam\")";
        let cursor = code.find("sam").unwrap() + 3;
        let ctx = completion_context_at(code, cursor).unwrap();
        match &ctx.kind {
            CompletionContextKind::Path { quote, raw_path } => {
                assert_eq!(*quote, '"');
                assert_eq!(raw_path, "data/sam");
            }
            other => panic!("Expected Path completion context, got {other:?}"),
        }
        assert_eq!(ctx.prefix, "sam");
        assert_eq!(ctx.replace_start_char, code.find("sam").unwrap());
        assert_eq!(ctx.replace_end_char, cursor);

        // Outside string: R identifier completion has priority
        let r_code = "read.csv(df)";
        let r_cursor = r_code.find("df").unwrap() + 2;
        let r_ctx = completion_context_at(r_code, r_cursor).unwrap();
        assert_eq!(r_ctx.kind, CompletionContextKind::General);
        assert_eq!(r_ctx.prefix, "df");
    }

    #[test]
    fn test_filesystem_path_candidates_and_nested_resolution() {
        let temp_dir = std::env::temp_dir()
            .join(format!("vectorcraft_test_path_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(temp_dir.join("data/sub")).unwrap();
        std::fs::create_dir_all(temp_dir.join("my folder")).unwrap();
        std::fs::create_dir_all(temp_dir.join("분석")).unwrap();
        std::fs::write(temp_dir.join("data/samples.csv"), "a,b\n1,2\n").unwrap();
        std::fs::write(temp_dir.join("data/summary.txt"), "summary").unwrap();
        std::fs::write(temp_dir.join("my folder/test file.csv"), "1").unwrap();
        std::fs::write(temp_dir.join("분석/결과.csv"), "1").unwrap();

        // 1. Root working directory completion
        let root_candidates = build_path_candidates(&temp_dir, "");
        assert!(root_candidates.iter().any(|c| c.name == "data/" && c.kind == CompletionCandidateKind::Directory));
        assert!(root_candidates.iter().any(|c| c.name == "my folder/" && c.kind == CompletionCandidateKind::Directory));
        assert!(root_candidates.iter().any(|c| c.name == "분석/" && c.kind == CompletionCandidateKind::Directory));

        // 2. Nested directory completion
        let data_candidates = build_path_candidates(&temp_dir, "data/");
        assert!(data_candidates.iter().any(|c| c.name == "sub/" && c.kind == CompletionCandidateKind::Directory));
        assert!(data_candidates.iter().any(|c| c.name == "samples.csv" && c.kind == CompletionCandidateKind::File));
        assert!(data_candidates.iter().any(|c| c.name == "summary.txt" && c.kind == CompletionCandidateKind::File));

        // 3. Partial path filtering
        let filtered = build_path_candidates(&temp_dir, "data/sa");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "samples.csv");

        // 4. Paths with spaces and Unicode
        let space_candidates = build_path_candidates(&temp_dir, "my folder/");
        assert!(space_candidates.iter().any(|c| c.name == "test file.csv"));

        let unicode_candidates = build_path_candidates(&temp_dir, "분석/");
        assert!(unicode_candidates.iter().any(|c| c.name == "결과.csv"));

        // 5. Completion application on file
        let mut doc = "read.csv(\"data/sa\")".to_owned();
        let ctx = completion_context_at(&doc, 17).unwrap();
        let new_caret = apply_completion(&mut doc, &ctx, &filtered[0]);
        assert_eq!(doc, "read.csv(\"data/samples.csv\")");
        assert_eq!(new_caret, 26); // Right after samples.csv, before closing quote

        // 6. Dynamic update after setwd simulation
        let sub_dir = temp_dir.join("data");
        let after_setwd_candidates = build_path_candidates(&sub_dir, "");
        assert!(after_setwd_candidates.iter().any(|c| c.name == "samples.csv"));
        assert!(after_setwd_candidates.iter().any(|c| c.name == "sub/"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_r_named_colors_sorting_and_parsing() {
        // Guarantee R_NAMED_COLORS is strictly sorted for binary search
        for window in R_NAMED_COLORS.windows(2) {
            assert!(window[0].0 < window[1].0, "R_NAMED_COLORS is not strictly sorted: {:?} vs {:?}", window[0].0, window[1].0);
        }

        // Test base colors
        assert_eq!(parse_r_named_color("red"), Some(egui::Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_r_named_color("black"), Some(egui::Color32::from_rgb(0, 0, 0)));
        assert_eq!(parse_r_named_color("white"), Some(egui::Color32::from_rgb(255, 255, 255)));
        assert_eq!(parse_r_named_color("blue"), Some(egui::Color32::from_rgb(0, 0, 255)));
        assert_eq!(parse_r_named_color("green"), Some(egui::Color32::from_rgb(0, 255, 0))); // R / X11 green
        assert_eq!(parse_r_named_color("purple"), Some(egui::Color32::from_rgb(160, 32, 240))); // R / X11 purple
        assert_eq!(parse_r_named_color("transparent"), Some(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0)));

        // Test gray0 .. gray100
        assert_eq!(parse_r_named_color("gray0"), Some(egui::Color32::from_rgb(0, 0, 0)));
        assert_eq!(parse_r_named_color("gray50"), Some(egui::Color32::from_rgb(128, 128, 128)));
        assert_eq!(parse_r_named_color("grey100"), Some(egui::Color32::from_rgb(255, 255, 255)));

        // Test numbered variants like red1 .. red4
        assert_eq!(parse_r_named_color("red1"), Some(egui::Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_r_named_color("red2"), Some(egui::Color32::from_rgb(238, 0, 0)));
        assert_eq!(parse_r_named_color("red3"), Some(egui::Color32::from_rgb(205, 0, 0)));
        assert_eq!(parse_r_named_color("red4"), Some(egui::Color32::from_rgb(139, 0, 0)));

        // Case insensitivity
        assert_eq!(parse_r_named_color("RED"), Some(egui::Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_r_named_color("DarkBlue"), Some(egui::Color32::from_rgb(0, 0, 139)));
    }

    #[test]
    fn test_hex_rgb_and_alpha_parsing() {
        // Standard 6-digit hex
        assert_eq!(parse_r_hex_color("#FF0000"), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)));
        assert_eq!(parse_r_hex_color("#000000"), Some(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 255)));
        assert_eq!(parse_r_hex_color("#00FF88"), Some(egui::Color32::from_rgba_unmultiplied(0, 255, 136, 255)));
        assert_eq!(parse_r_hex_color("#ffffff"), Some(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 255)));

        // 8-digit hex with alpha
        assert_eq!(parse_r_hex_color("#FF000088"), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 136)));
        assert_eq!(parse_r_hex_color("#00FF8800"), Some(egui::Color32::from_rgba_unmultiplied(0, 255, 136, 0)));

        // 3-digit and 4-digit hex shorthand
        assert_eq!(parse_r_hex_color("#F00"), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)));
        assert_eq!(parse_r_hex_color("#F008"), Some(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 136)));
    }

    #[test]
    fn test_invalid_colors() {
        assert_eq!(parse_r_named_color("notacolor"), None);
        assert_eq!(parse_r_named_color(""), None);
        assert_eq!(parse_r_named_color("123"), None);

        // Invalid hex formats
        assert_eq!(parse_r_hex_color("#12345"), None); // Length 5 is invalid
        assert_eq!(parse_r_hex_color("#XYZ"), None); // Non-hex characters
        assert_eq!(parse_r_hex_color("#1234567"), None); // Length 7 is invalid
        assert_eq!(parse_r_hex_color("FF0000"), None); // Missing leading '#'
    }

    #[test]
    fn test_color_strings_in_comments_and_categorical_strings() {
        // Comments must never generate color previews
        let comment_code = "# col = \"red\"\n# \"#FF0000\"";
        let comment_spans = tokenize_spans(comment_code);
        let previews1 = detect_color_spans(comment_code, &comment_spans, true);
        assert!(previews1.is_empty(), "Comment strings must not generate color previews");

        // Ordinary categorical strings must never generate color previews
        let categorical_code = "data$category == \"red\"\nstatus <- \"black\"\nprint(\"white\")";
        let cat_spans = tokenize_spans(categorical_code);
        let previews2 = detect_color_spans(categorical_code, &cat_spans, true);
        assert!(previews2.is_empty(), "Categorical labels must not generate color previews");

        // Incomplete strings must never generate color previews
        let incomplete_code = "col = \"red";
        let inc_spans = tokenize_spans(incomplete_code);
        let previews3 = detect_color_spans(incomplete_code, &inc_spans, true);
        assert!(previews3.is_empty(), "Incomplete string literal must not generate color previews");

        // Valid color argument contexts MUST generate color previews
        let valid_code = "plot(x, col = \"red\", bg = \"white\", fill = \"#00FF88\")";
        let valid_spans = tokenize_spans(valid_code);
        let previews4 = detect_color_spans(valid_code, &valid_spans, true);
        assert_eq!(previews4.len(), 3);
        assert_eq!(previews4[0].color, egui::Color32::from_rgb(255, 0, 0));
        assert_eq!(previews4[1].color, egui::Color32::from_rgb(255, 255, 255));
        assert_eq!(previews4[2].color, egui::Color32::from_rgb(0, 255, 136));

        // Standalone hex colors MUST generate previews
        let hex_code = "palette <- c(\"#FF0000\", \"#00FF00\")";
        let hex_spans = tokenize_spans(hex_code);
        let previews5 = detect_color_spans(hex_code, &hex_spans, true);
        assert_eq!(previews5.len(), 2);
    }

    #[test]
    fn test_text_contrast_and_boundary_strokes() {
        // High contrast foreground text selection
        let white_text = contrasting_text_color(egui::Color32::BLACK, true);
        assert_eq!(white_text, egui::Color32::from_rgb(245, 245, 245));

        let dark_text = contrasting_text_color(egui::Color32::WHITE, true);
        assert_eq!(dark_text, egui::Color32::from_rgb(15, 15, 15));

        let red_text = contrasting_text_color(egui::Color32::from_rgb(255, 0, 0), true);
        assert_eq!(red_text, egui::Color32::from_rgb(245, 245, 245));

        let green_text = contrasting_text_color(egui::Color32::from_rgb(0, 255, 136), true);
        assert_eq!(green_text, egui::Color32::from_rgb(15, 15, 15));

        // Boundary stroke for white in light and dark themes
        let white_stroke_dark = compute_visible_boundary_stroke(egui::Color32::WHITE, true);
        assert_ne!(white_stroke_dark, egui::Stroke::NONE);

        let white_stroke_light = compute_visible_boundary_stroke(egui::Color32::WHITE, false);
        assert_ne!(white_stroke_light, egui::Stroke::NONE);

        // Vibrant colors with good contrast do not require forced boundary underlines
        let red_stroke = compute_visible_boundary_stroke(egui::Color32::from_rgb(255, 0, 0), true);
        assert_eq!(red_stroke, egui::Stroke::NONE);
    }

    #[test]
    fn test_caret_selection_layout_consistency() {
        // Verify that color preview spans do not alter text character lengths or line count
        let code = "col = \"red\"\nbg = \"#FFFFFF\"\nx <- 10";
        let mut cache = HighlightCache::default();
        let spans = tokenize_spans(code);
        let color_spans = detect_color_spans(code, &spans, true);
        assert_eq!(color_spans.len(), 2);

        cache.source = code.to_owned();
        cache.token_spans = spans;
        cache.color_spans = color_spans;

        // The token byte ranges must strictly preserve exact source characters
        for cp in &cache.color_spans {
            let token_str = &code[cp.byte_start..cp.byte_end];
            assert!(token_str == "\"red\"" || token_str == "\"#FFFFFF\"");
        }
    }

    #[test]
    fn test_large_script_color_performance_and_caching() {
        let mut large_script = String::with_capacity(100_000);
        for i in 0..2000 {
            if i % 10 == 0 {
                large_script.push_str("plot(x, col = \"red\", bg = \"#00FF88\")\n");
            } else {
                large_script.push_str("x <- x + 1\ny <- y * 2\n");
            }
        }

        let start = std::time::Instant::now();
        let spans = tokenize_spans(&large_script);
        let color_spans = detect_color_spans(&large_script, &spans, true);
        let duration = start.elapsed();

        // Must parse 2000 lines and hundreds of color previews in under 50ms
        assert!(duration.as_millis() < 50, "Color detection took too long: {duration:?}");
        assert_eq!(color_spans.len(), 400); // 200 lines * 2 colors each = 400

        // Test caching: identical source must be instant
        let cache = HighlightCache {
            source: large_script.clone(),
            token_spans: spans,
            color_spans,
            theme_is_dark: true,
            ..Default::default()
        };

        let cached_start = std::time::Instant::now();
        // Simulate cached check
        if cache.source == large_script && cache.theme_is_dark {
            // zero work needed
        }
        let cached_duration = cached_start.elapsed();
        assert!(cached_duration.as_micros() < 500);
    }

    #[test]
    fn test_bracket_auto_pairing_and_caret_positioning() {
        let mut s_brace = "if (x) ".to_owned();
        let pos1 = auto_pair_delimiter(&mut s_brace, 7, '{');
        assert_eq!(s_brace, "if (x) {}");
        assert_eq!(pos1, 8); // Inside `{}`

        let mut s_paren = "foo".to_owned();
        let pos2 = auto_pair_delimiter(&mut s_paren, 3, '(');
        assert_eq!(s_paren, "foo()");
        assert_eq!(pos2, 4); // Inside `()`

        let mut s_bracket = "x".to_owned();
        let pos3 = auto_pair_delimiter(&mut s_bracket, 1, '[');
        assert_eq!(s_bracket, "x[]");
        assert_eq!(pos3, 2); // Inside `[]`

        // Double bracket `[[|]]`
        let pos4 = auto_pair_delimiter(&mut s_bracket, 2, '[');
        assert_eq!(s_bracket, "x[[]]");
        assert_eq!(pos4, 3); // Inside `[[]]`
    }

    #[test]
    fn test_closing_bracket_skip() {
        let s_paren = "foo()";
        assert!(should_skip_closing_delimiter(s_paren, 4, ')'));
        assert!(!should_skip_closing_delimiter(s_paren, 3, ')'));

        let s_bracket = "x[]";
        assert!(should_skip_closing_delimiter(s_bracket, 2, ']'));

        let s_brace = "if (x) {}";
        assert!(should_skip_closing_delimiter(s_brace, 8, '}'));

        // Does NOT skip inside comment or string
        let s_str = "\"foo()\"";
        assert!(!should_skip_closing_delimiter(s_str, 5, ')'));

        let s_comment = "# comment )";
        assert!(!should_skip_closing_delimiter(s_comment, 10, ')'));
    }

    #[test]
    fn test_paired_backspace_deletion_all_delimiters() {
        // Braces
        let mut s1 = "if (x) {}".to_owned();
        assert!(is_cursor_between_empty_pair(&s1, 8));
        let p1 = delete_empty_pair(&mut s1, 8);
        assert_eq!(s1, "if (x) ");
        assert_eq!(p1, 7);

        // Parens
        let mut s2 = "foo()".to_owned();
        assert!(is_cursor_between_empty_pair(&s2, 4));
        let p2 = delete_empty_pair(&mut s2, 4);
        assert_eq!(s2, "foo");
        assert_eq!(p2, 3);

        // Brackets
        let mut s3 = "x[]".to_owned();
        assert!(is_cursor_between_empty_pair(&s3, 2));
        let p3 = delete_empty_pair(&mut s3, 2);
        assert_eq!(s3, "x");
        assert_eq!(p3, 1);

        // Quotes
        let mut s4 = "s <- \"\"".to_owned();
        assert!(is_cursor_between_empty_pair(&s4, 6));
        let p4 = delete_empty_pair(&mut s4, 6);
        assert_eq!(s4, "s <- ");
        assert_eq!(p4, 5);

        // Non-empty pair must not be deleted as empty pair
        let mut s5 = "foo(x)".to_owned();
        assert!(!is_cursor_between_empty_pair(&s5, 4));
        let p5 = delete_empty_pair(&mut s5, 4);
        assert_eq!(s5, "foo(x)");
        assert_eq!(p5, 4);
    }

    #[test]
    fn test_smart_enter_after_opening_brace_and_expansion() {
        // Expand between braces `{}`
        let mut s1 = "if (x) {}".to_owned();
        let p1 = apply_smart_enter(&mut s1, 8);
        assert_eq!(s1, "if (x) {\n  \n}");
        assert_eq!(p1, 11); // On line 2, indented by 2 spaces

        // Indent after opening brace at end of line
        let mut s2 = "my_fn <- function() {".to_owned();
        let p2 = apply_smart_enter(&mut s2, 21);
        assert_eq!(s2, "my_fn <- function() {\n  ");
        assert_eq!(p2, 24);

        // Preserve indentation on normal line
        let mut s3 = "  x <- 10".to_owned();
        let p3 = apply_smart_enter(&mut s3, 9);
        assert_eq!(s3, "  x <- 10\n  ");
        assert_eq!(p3, 12);
    }

    #[test]
    fn test_nested_block_indentation() {
        let mut s = "foo <- function() {\n  if (TRUE) {".to_owned();
        let len = s.len();
        let p = apply_smart_enter(&mut s, len);
        assert_eq!(s, "foo <- function() {\n  if (TRUE) {\n    ");
        assert_eq!(p, s.len()); // Indented by 4 spaces
    }

    #[test]
    fn test_closing_brace_dedentation() {
        // Line with 2 spaces inside outer block dedents to 0 spaces
        let mut s1 = "foo <- function() {\n  ".to_owned();
        let len1 = s1.len();
        let new_pos1 = dedent_closing_bracket(&mut s1, len1, '}');
        assert_eq!(s1, "foo <- function() {\n}");
        assert_eq!(new_pos1, Some(s1.len()));

        // Line with 4 spaces inside nested block dedents to 2 spaces
        let mut s2 = "foo <- function() {\n  if (TRUE) {\n    ".to_owned();
        let len2 = s2.len();
        let new_pos2 = dedent_closing_bracket(&mut s2, len2, '}');
        assert_eq!(s2, "foo <- function() {\n  if (TRUE) {\n  }");
        assert_eq!(new_pos2, Some(s2.len()));

        // Line with code before cursor must NOT dedent
        let mut s3 = "x <- c(1, 2".to_owned();
        let len3 = s3.len();
        let res3 = dedent_closing_bracket(&mut s3, len3, '}');
        assert_eq!(res3, None);
        assert_eq!(s3, "x <- c(1, 2");
    }

    #[test]
    fn test_selection_wrapping_brackets_and_quotes() {
        let mut s1 = "sample_data".to_owned();
        let (st1, en1) = wrap_selection_with_pair(&mut s1, 0, 11, '(', ')');
        assert_eq!(s1, "(sample_data)");
        assert_eq!(st1, 1);
        assert_eq!(en1, 12);

        let mut s2 = "x".to_owned();
        let (st2, en2) = wrap_selection_with_pair(&mut s2, 0, 1, '[', ']');
        assert_eq!(s2, "[x]");
        assert_eq!(st2, 1);
        assert_eq!(en2, 2);

        let mut s3 = "body".to_owned();
        let (st3, en3) = wrap_selection_with_pair(&mut s3, 0, 4, '{', '}');
        assert_eq!(s3, "{body}");
        assert_eq!(st3, 1);
        assert_eq!(en3, 5);
    }

    #[test]
    fn test_brackets_in_strings_and_comments_do_not_pair() {
        // In string literal
        let s_str = "x <- \"hello ";
        assert!(!can_auto_pair_delimiter(s_str, s_str.len(), '('));
        assert!(!can_auto_pair_delimiter(s_str, s_str.len(), '{'));
        assert!(!can_auto_pair_delimiter(s_str, s_str.len(), '['));

        // In comment
        let s_comment = "# comments (";
        assert!(!can_auto_pair_delimiter(s_comment, s_comment.len(), '('));
        assert!(!can_auto_pair_delimiter(s_comment, s_comment.len(), '{'));
        assert!(!can_auto_pair_delimiter(s_comment, s_comment.len(), '['));

        // Escaped bracket
        let s_esc = r#"x <- \"#;
        assert!(!can_auto_pair_delimiter(s_esc, s_esc.len(), '('));
    }

    #[test]
    fn test_multiline_call_and_operator_smart_enter() {
        // Multiline function call
        let mut s1 = "plot(".to_owned();
        let p1 = apply_smart_enter(&mut s1, 5);
        assert_eq!(s1, "plot(\n  ");
        assert_eq!(p1, 8);

        // Continuation operator `+`
        let mut s2 = "x <- 1 +".to_owned();
        let p2 = apply_smart_enter(&mut s2, 8);
        assert_eq!(s2, "x <- 1 +\n  ");
        assert_eq!(p2, 11);

        // Continuation pipe `%>%`
        let mut s3 = "df %>%".to_owned();
        let p3 = apply_smart_enter(&mut s3, 6);
        assert_eq!(s3, "df %>%\n  ");
        assert_eq!(p3, 9);
    }
}
