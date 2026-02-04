//! Lightweight CSS property extraction.
//!
//! Parses inline `style=""` attributes and extracts a small set of
//! visual properties that the SDF paint renderer can use.

/// Extracted CSS visual properties.
#[derive(Debug, Clone, Default)]
pub struct StyleProps {
    pub color: Option<[f32; 4]>,
    pub background_color: Option<[f32; 4]>,
    pub font_size: Option<f32>,
    pub border_radius: Option<f32>,
}

/// Parse an inline `style="..."` attribute value.
pub fn parse_inline_style(style: &str) -> StyleProps {
    let mut props = StyleProps::default();
    for decl in style.split(';') {
        let parts: Vec<&str> = decl.splitn(2, ':').collect();
        if parts.len() != 2 {
            continue;
        }
        let prop = parts[0].trim();
        let val = parts[1].trim();
        match prop {
            "color" => props.color = parse_css_color(val),
            "background-color" | "background" => props.background_color = parse_css_color(val),
            "font-size" => props.font_size = parse_css_size(val),
            "border-radius" => props.border_radius = parse_css_size(val),
            _ => {}
        }
    }
    props
}

/// Parse a CSS color value into [r, g, b, a] (0.0–1.0).
pub fn parse_css_color(val: &str) -> Option<[f32; 4]> {
    let v = val.trim().to_lowercase();

    // Named colours (common subset)
    let named = match v.as_str() {
        "black" => Some([0.0, 0.0, 0.0, 1.0]),
        "white" => Some([1.0, 1.0, 1.0, 1.0]),
        "red" => Some([1.0, 0.0, 0.0, 1.0]),
        "green" => Some([0.0, 0.5, 0.0, 1.0]),
        "blue" => Some([0.0, 0.0, 1.0, 1.0]),
        "yellow" => Some([1.0, 1.0, 0.0, 1.0]),
        "orange" => Some([1.0, 0.647, 0.0, 1.0]),
        "purple" => Some([0.5, 0.0, 0.5, 1.0]),
        "gray" | "grey" => Some([0.5, 0.5, 0.5, 1.0]),
        "transparent" => Some([0.0, 0.0, 0.0, 0.0]),
        _ => None,
    };
    if named.is_some() {
        return named;
    }

    // Hex: #rgb, #rrggbb, #rrggbbaa
    if v.starts_with('#') {
        let hex = &v[1..];
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0])
            }
            _ => None,
        };
    }

    // rgb(r, g, b) / rgba(r, g, b, a)
    if v.starts_with("rgb") {
        let inner = v
            .trim_start_matches("rgba(")
            .trim_start_matches("rgb(")
            .trim_end_matches(')');
        let nums: Vec<f32> = inner
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .collect();
        if nums.len() >= 3 {
            let r = nums[0] / 255.0;
            let g = nums[1] / 255.0;
            let b = nums[2] / 255.0;
            let a = if nums.len() >= 4 { nums[3] } else { 1.0 };
            return Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)]);
        }
    }

    None
}

/// Parse a CSS size value (px or plain number).
fn parse_css_size(val: &str) -> Option<f32> {
    let v = val.trim().to_lowercase();
    let num_str = v.trim_end_matches("px").trim_end_matches("em").trim_end_matches("rem");
    num_str.parse::<f32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_named_colors() {
        assert_eq!(parse_css_color("red"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_css_color("black"), Some([0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn parse_hex_colors() {
        let c = parse_css_color("#ff0000").unwrap();
        assert!((c[0] - 1.0).abs() < 0.01);
        assert!(c[1].abs() < 0.01);

        let c3 = parse_css_color("#f00").unwrap();
        assert!((c3[0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_rgb_colors() {
        let c = parse_css_color("rgb(128, 64, 0)").unwrap();
        assert!((c[0] - 0.502).abs() < 0.01);
        assert!((c[1] - 0.251).abs() < 0.01);
    }

    #[test]
    fn parse_inline() {
        let props = parse_inline_style("color: red; font-size: 20px; background-color: #333");
        assert!(props.color.is_some());
        assert!((props.font_size.unwrap() - 20.0).abs() < 0.01);
        assert!(props.background_color.is_some());
    }
}
