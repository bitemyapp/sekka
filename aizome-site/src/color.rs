//! Colour parsing and WCAG 2 contrast, so the site checks and states ratios
//! it computed from Aizome's palettes rather than numbers copied by hand.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`: the tests write colours this way.
#[cfg(test)]
pub fn parse(hex: &str) -> Option<Rgba> {
    let h = hex.strip_prefix('#')?;
    let digits: Vec<u8> = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).map(|c| c as u8).collect(),
        6 | 8 => h.bytes().collect(),
        _ => return None,
    };
    let byte = |i: usize| -> Option<f64> {
        let s = std::str::from_utf8(&digits[i..i + 2]).ok()?;
        Some(u8::from_str_radix(s, 16).ok()? as f64 / 255.0)
    };
    Some(Rgba {
        r: byte(0)?,
        g: byte(2)?,
        b: byte(4)?,
        a: if digits.len() == 8 { byte(6)? } else { 1.0 },
    })
}

/// A translucent colour as it appears over an opaque ground.
pub fn over(fg: Rgba, bg: Rgba) -> Rgba {
    let mix = |f: f64, b: f64| f * fg.a + b * (1.0 - fg.a);
    Rgba {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
        a: 1.0,
    }
}

pub fn luminance(c: Rgba) -> f64 {
    let lin = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// Contrast of `fg` on `bg`, compositing a translucent `fg` first.
pub fn contrast(fg: Rgba, bg: Rgba) -> f64 {
    let fg = if fg.a < 1.0 { over(fg, bg) } else { fg };
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ratios() {
        let white = parse("#fff").unwrap();
        let black = parse("#000000").unwrap();
        assert!((contrast(white, black) - 21.0).abs() < 1e-9);
        // Night's secondary text on its surface.
        let ratio = contrast(parse("#a9b1d6").unwrap(), parse("#1a1b26").unwrap());
        assert!((ratio - 8.1).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn alpha_is_composited() {
        let half = parse("#ffffff80").unwrap();
        let ground = parse("#000000").unwrap();
        let seen = over(half, ground);
        assert!((seen.r - 128.0 / 255.0).abs() < 1e-9);
        assert!(contrast(half, ground) < 21.0);
    }
}
