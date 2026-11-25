//! Colour text → [`Hsla`]: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(r, g, b)`,
//! `rgba(r, g, b, a)`, `hsl(h, s%, l%)` and `hsla(h, s%, l%, a)`. The same forms CSS and Zed's
//! theme files use; `rgb` channels are `0..=255` (or percentages), hue in degrees, alpha `0..=1`
//! (or a percentage).

use super::Hsla;

/// Why a colour could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorError(String);

impl std::fmt::Display for ColorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ColorError {}

fn err(text: &str) -> ColorError {
    ColorError(format!(
        "`{text}` is not a colour: use #rrggbb, #rrggbbaa, rgb(…), rgba(…), hsl(…) or hsla(…)"
    ))
}

/// Read one colour.
pub fn parse_color(text: &str) -> Result<Hsla, ColorError> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('#') {
        return parse_hex(hex).ok_or_else(|| err(text));
    }
    let lower = t.to_ascii_lowercase();
    let (func, args) = lower
        .strip_suffix(')')
        .and_then(|s| s.split_once('('))
        .ok_or_else(|| err(text))?;
    let parts: Vec<&str> = args
        .split([',', ' ', '/'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let alpha = |i: usize| -> Option<f32> {
        match parts.get(i) {
            None => Some(1.0),
            Some(p) => number(p, 1.0).filter(|a| (0.0..=1.0).contains(a)),
        }
    };
    let c = match (func.trim(), parts.len()) {
        ("rgb" | "rgba", 3 | 4) => {
            let ch = |i: usize| number(parts[i], 255.0).filter(|v| (0.0..=255.0).contains(v));
            rgb_to_hsla(
                ch(0).ok_or_else(|| err(text))? / 255.0,
                ch(1).ok_or_else(|| err(text))? / 255.0,
                ch(2).ok_or_else(|| err(text))? / 255.0,
                alpha(3).ok_or_else(|| err(text))?,
            )
        }
        ("hsl" | "hsla", 3 | 4) => {
            let h = parts[0]
                .trim_end_matches("deg")
                .parse::<f32>()
                .ok()
                .filter(|h| h.is_finite())
                .ok_or_else(|| err(text))?;
            let pct = |i: usize| {
                parts[i]
                    .strip_suffix('%')
                    .and_then(|p| p.parse::<f32>().ok())
                    .filter(|v| (0.0..=100.0).contains(v))
                    .map(|v| v / 100.0)
            };
            Hsla {
                h: h.rem_euclid(360.0) / 360.0,
                s: pct(1).ok_or_else(|| err(text))?,
                l: pct(2).ok_or_else(|| err(text))?,
                a: alpha(3).ok_or_else(|| err(text))?,
            }
        }
        _ => return Err(err(text)),
    };
    Ok(c)
}

/// A number, or a percentage of `full`.
fn number(p: &str, full: f32) -> Option<f32> {
    let v = match p.strip_suffix('%') {
        Some(pct) => pct.parse::<f32>().ok()? / 100.0 * full,
        None => p.parse::<f32>().ok()?,
    };
    v.is_finite().then_some(v)
}

fn parse_hex(hex: &str) -> Option<Hsla> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|d| d * 17);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let [r, g, b, a] = match hex.len() {
        3 => [digit(0)?, digit(1)?, digit(2)?, 255],
        4 => [digit(0)?, digit(1)?, digit(2)?, digit(3)?],
        6 => [pair(0)?, pair(2)?, pair(4)?, 255],
        8 => [pair(0)?, pair(2)?, pair(4)?, pair(6)?],
        _ => return None,
    };
    let f = |v: u8| f32::from(v) / 255.0;
    Some(rgb_to_hsla(f(r), f(g), f(b), f(a)))
}

/// RGB (each `0..=1`) to HSL, the standard conversion GPUI's `Rgba → Hsla` also uses.
fn rgb_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    Hsla { h, s, l, a }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Hsla, b: Hsla) -> bool {
        [(a.h, b.h), (a.s, b.s), (a.l, b.l), (a.a, b.a)]
            .iter()
            .all(|(x, y)| (x - y).abs() < 0.002)
    }

    #[test]
    fn hex_forms() {
        let red = Hsla {
            h: 0.0,
            s: 1.0,
            l: 0.5,
            a: 1.0,
        };
        assert!(close(parse_color("#ff0000").unwrap(), red));
        assert!(close(parse_color("#F00").unwrap(), red));
        assert!(close(
            parse_color(" #ff000080 ").unwrap(),
            Hsla {
                a: 128.0 / 255.0,
                ..red
            }
        ));
        assert!(close(
            parse_color("#f008").unwrap(),
            Hsla {
                a: 136.0 / 255.0,
                ..red
            }
        ));
        // Discord's dark background.
        let bg = parse_color("#313338").unwrap();
        assert!(
            close(
                bg,
                Hsla {
                    h: 0.619_05,
                    s: 0.066_67,
                    l: 0.205_88,
                    a: 1.0
                }
            ),
            "{bg:?}"
        );
        for bad in ["#12", "#12345", "#gg0000", "#", "#ff00001"] {
            assert!(parse_color(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn function_forms() {
        let green = Hsla {
            h: 1.0 / 3.0,
            s: 1.0,
            l: 0.5,
            a: 1.0,
        };
        assert!(close(parse_color("rgb(0, 255, 0)").unwrap(), green));
        assert!(close(
            parse_color("RGBA(0 255 0 / 50%)").unwrap(),
            Hsla { a: 0.5, ..green }
        ));
        assert!(close(parse_color("rgb(0%, 100%, 0%)").unwrap(), green));
        assert!(close(parse_color("hsl(120, 100%, 50%)").unwrap(), green));
        assert!(close(
            parse_color("hsla(480deg, 100%, 50%, 0.25)").unwrap(),
            Hsla { a: 0.25, ..green }
        ));
        for bad in [
            "rgb(0, 0)",
            "rgb(300, 0, 0)",
            "rgba(0, 0, 0, 2)",
            "hsl(0, 50, 50%)",
            "hsl(0, 150%, 50%)",
            "cmyk(0, 0, 0, 0)",
            "pink",
            "rgb(0, 0, 0",
        ] {
            assert!(parse_color(bad).is_err(), "{bad}");
        }
    }
}
