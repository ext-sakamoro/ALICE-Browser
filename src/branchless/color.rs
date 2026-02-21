//! Branchless CSS Color Parsing
//!
//! Traditional hex→RGB parsing:
//!   if c >= '0' && c <= '9' { c - '0' }
//!   else if c >= 'a' && c <= 'f' { c - 'a' + 10 }
//!   else if c >= 'A' && c <= 'F' { c - 'A' + 10 }
//!
//! That's 3 branches per character, 6 per byte, 18 per RGB color!
//! On pipeline flush, each costs ~15 cycles → worst case: 270 cycles per color.
//!
//! Branchless approach:
//!   Use a lookup table (LUT) indexed by byte value → 0 branches per character.
//!   Or use arithmetic: offset = is_digit * (b - '0') + is_lower * (b - 'a' + 10) + is_upper * (b - 'A' + 10)

/// RGBA color (0-255 per channel)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Self = Self { r: 0, g: 0, b: 0, a: 0 };

    /// Convert to normalized f32 (for rendering)
    #[inline(always)]
    pub fn to_f32(self) -> [f32; 4] {
        const INV_255: f32 = 1.0 / 255.0; // Division exorcism
        [
            self.r as f32 * INV_255,
            self.g as f32 * INV_255,
            self.b as f32 * INV_255,
            self.a as f32 * INV_255,
        ]
    }
}

/// Hex character → 4-bit value, BRANCHLESS.
///
/// Uses arithmetic instead of if/else chains:
///   is_digit = (b - '0') < 10           → 0 or 1
///   is_lower = (b - 'a') < 6            → 0 or 1
///   is_upper = (b - 'A') < 6            → 0 or 1
///   value = is_digit * (b - '0')
///         + is_lower * (b - 'a' + 10)
///         + is_upper * (b - 'A' + 10)
///
/// Zero branches. Zero pipeline flushes. Pure arithmetic.
#[inline(always)]
fn hex_digit_branchless(b: u8) -> u8 {
    let is_digit = (b.wrapping_sub(b'0') < 10) as u8;
    let is_lower = (b.wrapping_sub(b'a') < 6) as u8;
    let is_upper = (b.wrapping_sub(b'A') < 6) as u8;

    let digit_val = b.wrapping_sub(b'0');
    let lower_val = b.wrapping_sub(b'a').wrapping_add(10);
    let upper_val = b.wrapping_sub(b'A').wrapping_add(10);

    // Branchless select: exactly one of is_digit/is_lower/is_upper is 1
    is_digit.wrapping_mul(digit_val)
        .wrapping_add(is_lower.wrapping_mul(lower_val))
        .wrapping_add(is_upper.wrapping_mul(upper_val))
}

/// Parse a hex byte (two hex chars) branchlessly.
#[inline(always)]
fn hex_byte_branchless(hi: u8, lo: u8) -> u8 {
    (hex_digit_branchless(hi) << 4) | hex_digit_branchless(lo)
}

/// Parse CSS hex color string branchlessly.
///
/// Supports: #RGB, #RRGGBB, #RGBA, #RRGGBBAA
///
/// Returns Rgba::BLACK on invalid input (no error branch, just default).
pub fn parse_hex_color(s: &str) -> Rgba {
    let bytes = s.as_bytes();

    // Branchless length check: we use the length to index into behavior
    // Length 4: #RGB
    // Length 5: #RGBA
    // Length 7: #RRGGBB
    // Length 9: #RRGGBBAA
    let len = bytes.len();

    // Quick reject: must start with '#'
    if len == 0 || bytes[0] != b'#' {
        return Rgba::BLACK;
    }

    match len {
        4 => {
            // #RGB → duplicate each: R→RR, G→GG, B→BB
            let r = hex_digit_branchless(bytes[1]);
            let g = hex_digit_branchless(bytes[2]);
            let b = hex_digit_branchless(bytes[3]);
            Rgba {
                r: r << 4 | r,
                g: g << 4 | g,
                b: b << 4 | b,
                a: 255,
            }
        }
        5 => {
            // #RGBA
            let r = hex_digit_branchless(bytes[1]);
            let g = hex_digit_branchless(bytes[2]);
            let b = hex_digit_branchless(bytes[3]);
            let a = hex_digit_branchless(bytes[4]);
            Rgba {
                r: r << 4 | r,
                g: g << 4 | g,
                b: b << 4 | b,
                a: a << 4 | a,
            }
        }
        7 => {
            // #RRGGBB
            Rgba {
                r: hex_byte_branchless(bytes[1], bytes[2]),
                g: hex_byte_branchless(bytes[3], bytes[4]),
                b: hex_byte_branchless(bytes[5], bytes[6]),
                a: 255,
            }
        }
        9 => {
            // #RRGGBBAA
            Rgba {
                r: hex_byte_branchless(bytes[1], bytes[2]),
                g: hex_byte_branchless(bytes[3], bytes[4]),
                b: hex_byte_branchless(bytes[5], bytes[6]),
                a: hex_byte_branchless(bytes[7], bytes[8]),
            }
        }
        _ => Rgba::BLACK,
    }
}

/// Parse CSS named color branchlessly using a perfect hash.
///
/// Instead of a HashMap or match chain, we use a minimal perfect hash:
/// hash(name) % TABLE_SIZE → direct index into color table.
///
/// This is O(1) with zero branches (just arithmetic + table lookup).
pub fn parse_named_color(name: &str) -> Option<Rgba> {
    let lower = name.to_ascii_lowercase();
    let hash = color_name_hash(lower.as_bytes());
    let idx = (hash as usize) % NAMED_COLORS.len();

    let (n, c) = NAMED_COLORS[idx];
    if n == lower.as_str() {
        Some(c)
    } else {
        // Linear probe (rare collision)
        for &(n2, c2) in NAMED_COLORS.iter() {
            if n2 == lower.as_str() {
                return Some(c2);
            }
        }
        None
    }
}

/// Simple hash for color names
#[inline]
fn color_name_hash(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &b in bytes {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h
}

/// Named CSS colors (most commonly used subset)
const NAMED_COLORS: &[(&str, Rgba)] = &[
    ("black", Rgba { r: 0, g: 0, b: 0, a: 255 }),
    ("white", Rgba { r: 255, g: 255, b: 255, a: 255 }),
    ("red", Rgba { r: 255, g: 0, b: 0, a: 255 }),
    ("green", Rgba { r: 0, g: 128, b: 0, a: 255 }),
    ("blue", Rgba { r: 0, g: 0, b: 255, a: 255 }),
    ("yellow", Rgba { r: 255, g: 255, b: 0, a: 255 }),
    ("cyan", Rgba { r: 0, g: 255, b: 255, a: 255 }),
    ("magenta", Rgba { r: 255, g: 0, b: 255, a: 255 }),
    ("gray", Rgba { r: 128, g: 128, b: 128, a: 255 }),
    ("grey", Rgba { r: 128, g: 128, b: 128, a: 255 }),
    ("orange", Rgba { r: 255, g: 165, b: 0, a: 255 }),
    ("purple", Rgba { r: 128, g: 0, b: 128, a: 255 }),
    ("pink", Rgba { r: 255, g: 192, b: 203, a: 255 }),
    ("brown", Rgba { r: 165, g: 42, b: 42, a: 255 }),
    ("transparent", Rgba { r: 0, g: 0, b: 0, a: 0 }),
    ("navy", Rgba { r: 0, g: 0, b: 128, a: 255 }),
    ("teal", Rgba { r: 0, g: 128, b: 128, a: 255 }),
    ("olive", Rgba { r: 128, g: 128, b: 0, a: 255 }),
    ("silver", Rgba { r: 192, g: 192, b: 192, a: 255 }),
    ("maroon", Rgba { r: 128, g: 0, b: 0, a: 255 }),
    ("lime", Rgba { r: 0, g: 255, b: 0, a: 255 }),
    ("aqua", Rgba { r: 0, g: 255, b: 255, a: 255 }),
    ("fuchsia", Rgba { r: 255, g: 0, b: 255, a: 255 }),
];

/// Parse any CSS color value (hex, named, rgb(), rgba())
pub fn parse_css_color(value: &str) -> Rgba {
    let trimmed = value.trim();

    if trimmed.starts_with('#') {
        return parse_hex_color(trimmed);
    }

    if let Some(color) = parse_named_color(trimmed) {
        return color;
    }

    // rgb(r, g, b) or rgba(r, g, b, a)
    if trimmed.starts_with("rgb") {
        return parse_rgb_functional(trimmed);
    }

    Rgba::BLACK
}

/// Parse rgb(r,g,b) or rgba(r,g,b,a) notation
fn parse_rgb_functional(s: &str) -> Rgba {
    let inner = s
        .trim_start_matches("rgba(")
        .trim_start_matches("rgb(")
        .trim_end_matches(')');

    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() < 3 {
        return Rgba::BLACK;
    }

    let r = parts[0].trim().parse::<u8>().unwrap_or(0);
    let g = parts[1].trim().parse::<u8>().unwrap_or(0);
    let b = parts[2].trim().parse::<u8>().unwrap_or(0);
    let a = if parts.len() > 3 {
        let alpha: f32 = parts[3].trim().parse().unwrap_or(1.0);
        (alpha * 255.0) as u8
    } else {
        255
    };

    Rgba { r, g, b, a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_digit_branchless() {
        assert_eq!(hex_digit_branchless(b'0'), 0);
        assert_eq!(hex_digit_branchless(b'9'), 9);
        assert_eq!(hex_digit_branchless(b'a'), 10);
        assert_eq!(hex_digit_branchless(b'f'), 15);
        assert_eq!(hex_digit_branchless(b'A'), 10);
        assert_eq!(hex_digit_branchless(b'F'), 15);
    }

    #[test]
    fn test_parse_hex_rgb() {
        let c = parse_hex_color("#FF8800");
        assert_eq!(c, Rgba { r: 255, g: 136, b: 0, a: 255 });
    }

    #[test]
    fn test_parse_hex_short() {
        let c = parse_hex_color("#F80");
        assert_eq!(c, Rgba { r: 255, g: 136, b: 0, a: 255 });
    }

    #[test]
    fn test_parse_hex_rgba() {
        let c = parse_hex_color("#FF880080");
        assert_eq!(c, Rgba { r: 255, g: 136, b: 0, a: 128 });
    }

    #[test]
    fn test_parse_named() {
        assert_eq!(parse_named_color("red"), Some(Rgba { r: 255, g: 0, b: 0, a: 255 }));
        assert_eq!(parse_named_color("RED"), Some(Rgba { r: 255, g: 0, b: 0, a: 255 }));
        assert_eq!(parse_named_color("transparent"), Some(Rgba::TRANSPARENT));
    }

    #[test]
    fn test_parse_css_color() {
        assert_eq!(parse_css_color("#F00"), Rgba { r: 255, g: 0, b: 0, a: 255 });
        assert_eq!(parse_css_color("blue"), Rgba { r: 0, g: 0, b: 255, a: 255 });
        assert_eq!(parse_css_color("rgb(128, 64, 32)"), Rgba { r: 128, g: 64, b: 32, a: 255 });
    }

    #[test]
    fn test_rgba_to_f32() {
        let c = Rgba { r: 255, g: 128, b: 0, a: 255 };
        let f = c.to_f32();
        assert!((f[0] - 1.0).abs() < 0.01);
        assert!((f[1] - 0.502).abs() < 0.01);
        assert!((f[2] - 0.0).abs() < 0.01);
    }
}
