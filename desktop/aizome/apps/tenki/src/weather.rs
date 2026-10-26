//! What the numbers mean: WMO weather codes as words and Lucide glyphs,
//! wind directions as compass points, UV as its WHO bands, and the dye
//! ramp temperatures are drawn in, from cold kamenozoki 甕覗 to warm kohaku
//! 琥珀 (amber).

use aizome_kit::theme::Theme;
use aizome_kit::{Color, Icon};

/// A weather code as Tenki says and draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Condition {
    pub text: &'static str,
    pub icon: Icon,
}

/// The condition for WMO code `code` (Open-Meteo's `weather_code`), by
/// day or by night.
pub fn condition(code: u8, is_day: bool) -> Condition {
    let c = |text, icon| Condition { text, icon };
    let sky = |day: Icon, night: Icon| if is_day { day } else { night };
    match code {
        0 => c("Clear", sky(Icon::Sun, Icon::Moon)),
        1 => c("Mostly Clear", sky(Icon::Sun, Icon::Moon)),
        2 => c("Partly Cloudy", sky(Icon::CloudSun, Icon::CloudMoon)),
        3 => c("Overcast", Icon::Cloudy),
        45 => c("Fog", Icon::CloudFog),
        48 => c("Freezing Fog", Icon::CloudFog),
        51 => c("Light Drizzle", Icon::CloudDrizzle),
        53 => c("Drizzle", Icon::CloudDrizzle),
        55 => c("Heavy Drizzle", Icon::CloudDrizzle),
        56 | 57 => c("Freezing Drizzle", Icon::CloudDrizzle),
        61 => c("Light Rain", Icon::CloudRain),
        63 => c("Rain", Icon::CloudRain),
        65 => c("Heavy Rain", Icon::CloudRain),
        66 | 67 => c("Freezing Rain", Icon::CloudHail),
        71 => c("Light Snow", Icon::CloudSnow),
        73 => c("Snow", Icon::CloudSnow),
        75 => c("Heavy Snow", Icon::CloudSnow),
        77 => c("Snow Grains", Icon::Snowflake),
        80 => c("Light Showers", sky(Icon::CloudSunRain, Icon::CloudMoonRain)),
        81 => c("Showers", sky(Icon::CloudSunRain, Icon::CloudMoonRain)),
        82 => c("Heavy Showers", Icon::CloudRain),
        85 => c("Snow Showers", Icon::CloudSnow),
        86 => c("Heavy Snow Showers", Icon::CloudSnow),
        95 => c("Thunderstorm", Icon::CloudLightning),
        96 | 99 => c("Thunderstorm, Hail", Icon::CloudLightning),
        _ => c("Unknown", Icon::Cloud),
    }
}

const POINTS: [&str; 16] =
    ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];

/// The compass point wind from `degrees` comes from: "NNW".
pub fn compass(degrees: f64) -> &'static str {
    let i = ((degrees.rem_euclid(360.0) / 22.5).round() as usize) % 16;
    POINTS[i]
}

/// The WHO's UV bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvBand {
    Low,
    Moderate,
    High,
    VeryHigh,
    Extreme,
}

impl UvBand {
    pub fn of(uv: f64) -> UvBand {
        match uv.round() as i64 {
            ..=2 => UvBand::Low,
            3..=5 => UvBand::Moderate,
            6..=7 => UvBand::High,
            8..=10 => UvBand::VeryHigh,
            _ => UvBand::Extreme,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UvBand::Low => "Low",
            UvBand::Moderate => "Moderate",
            UvBand::High => "High",
            UvBand::VeryHigh => "Very High",
            UvBand::Extreme => "Extreme",
        }
    }

    /// What to do about it.
    pub fn advice(self) -> &'static str {
        match self {
            UvBand::Low => "No protection needed",
            UvBand::Moderate => "Wear sunscreen at midday",
            UvBand::High => "Seek shade at midday",
            UvBand::VeryHigh => "Avoid the midday sun",
            UvBand::Extreme => "Stay out of the sun",
        }
    }

    /// Its colour, from the theme's.
    pub fn color(self, t: &Theme) -> Color {
        let p = |c| aizome_kit::theme::color(c);
        let pal = &t.palette;
        match self {
            UvBand::Low => p(pal.green),
            UvBand::Moderate => p(pal.amber),
            UvBand::High => p(pal.amber.mix(pal.red, 0.5)),
            UvBand::VeryHigh => p(pal.red),
            UvBand::Extreme => p(pal.violet),
        }
    }
}

/// The UV scale's stops (0 to 11+), for a bar along it.
pub fn uv_stops(t: &Theme) -> Vec<(f32, Color)> {
    [
        (0.0, UvBand::Low),
        (0.3, UvBand::Moderate),
        (0.55, UvBand::High),
        (0.75, UvBand::VeryHigh),
        (1.0, UvBand::Extreme),
    ]
    .into_iter()
    .map(|(at, b)| (at, b.color(t)))
    .collect()
}

// The dye ramp.

/// Temperatures (°C) and the colours the ramp passes through at them, by
/// theme: hanada below freezing, kamenozoki at the cold end proper, kohaku
/// at the warm one. Day takes deeper dyes, to show on its pale grounds.
fn ramp_stops(t: &Theme) -> [(f64, [u8; 3]); 4] {
    if t.is_dark() {
        [(-20.0, [0x7a, 0xa2, 0xf7]), (0.0, [0xa9, 0xc4, 0xff]), (30.0, [0xe0, 0xaf, 0x68]), (40.0, [0xec, 0x9a, 0x5c])]
    } else {
        [(-20.0, [0x3d, 0x59, 0xa1]), (0.0, [0x6a, 0x8d, 0xe0]), (30.0, [0xc8, 0x8a, 0x1c]), (40.0, [0xc2, 0x6a, 0x22])]
    }
}

/// The ramp's colour at `celsius`.
pub fn temp_color(t: &Theme, celsius: f64) -> Color {
    let stops = ramp_stops(t);
    let rgb = if celsius <= stops[0].0 {
        stops[0].1
    } else if celsius >= stops[3].0 {
        stops[3].1
    } else {
        let i = stops.windows(2).position(|w| celsius <= w[1].0).unwrap_or(2);
        let (a, b) = (stops[i], stops[i + 1]);
        oklch_mix(a.1, b.1, ((celsius - a.0) / (b.0 - a.0)) as f32)
    };
    Color::rgba8(rgb[0], rgb[1], rgb[2], 255)
}

/// Gradient stops for a bar from `lo` to `hi` °C along its length.
pub fn ramp_gradient(t: &Theme, lo: f64, hi: f64) -> Vec<(f32, Color)> {
    let n = 8;
    (0..=n)
        .map(|i| {
            let f = i as f64 / n as f64;
            (f as f32, temp_color(t, lo + (hi - lo) * f))
        })
        .collect()
}

fn to_linear(c: u8) -> f32 {
    let v = f32::from(c) / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn to_oklab(c: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(to_linear);
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [u8; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        from_linear(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        from_linear(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        from_linear(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
    ]
}

/// Mixed in OKLCh: lightness and chroma straight across, the hue the
/// short way round, so kamenozoki reaches kohaku through a dusky rose
/// rather than through grey.
fn oklch_mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let lch = |c: [f32; 3]| (c[0], c[1].hypot(c[2]), c[2].atan2(c[1]));
    let (la, ca, ha) = lch(to_oklab(a));
    let (lb, cb, hb) = lch(to_oklab(b));
    let tau = std::f32::consts::TAU;
    let mut dh = (hb - ha).rem_euclid(tau);
    if dh > tau / 2.0 {
        dh -= tau;
    }
    // Across a wide turn of hue the chroma dips a little in the middle,
    // to a dusky rose rather than a bright pink.
    let dip = 1.0 - 0.55 * (dh.abs() / (tau / 2.0)) * (std::f32::consts::PI * t).sin();
    let (l, c, h) = (la + (lb - la) * t, (ca + (cb - ca) * t) * dip, ha + dh * t);
    from_oklab([l, c * h.cos(), c * h.sin()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_have_words_and_glyphs() {
        assert_eq!(condition(0, true).icon, Icon::Sun);
        assert_eq!(condition(0, false).icon, Icon::Moon);
        assert_eq!(condition(2, false).text, "Partly Cloudy");
        assert_eq!(condition(81, true).icon, Icon::CloudSunRain);
        assert_eq!(condition(95, true).icon, Icon::CloudLightning);
        assert_eq!(condition(42, true).text, "Unknown");
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(337.5), "NNW");
        assert_eq!(compass(90.0), "E");
        assert_eq!(compass(-90.0), "W");
        assert_eq!(compass(200.0), "SSW");
    }

    #[test]
    fn uv_bands() {
        assert_eq!(UvBand::of(0.0), UvBand::Low);
        assert_eq!(UvBand::of(2.4), UvBand::Low);
        assert_eq!(UvBand::of(2.6), UvBand::Moderate);
        assert_eq!(UvBand::of(7.0), UvBand::High);
        assert_eq!(UvBand::of(11.2), UvBand::Extreme);
    }

    #[test]
    fn the_ramp_runs_from_kamenozoki_to_kohaku() {
        let t = Theme::night();
        let at = |c| {
            let k = temp_color(&t, c);
            [(k.r * 255.0).round() as u8, (k.g * 255.0).round() as u8, (k.b * 255.0).round() as u8]
        };
        assert_eq!(at(0.0), [0xa9, 0xc4, 0xff]);
        assert_eq!(at(30.0), [0xe0, 0xaf, 0x68]);
        assert_eq!(at(-40.0), [0x7a, 0xa2, 0xf7]);
        // Colder is bluer, all the way along.
        let mut last = at(-20.0);
        for c in (-19..=40).map(f64::from) {
            let now = at(c);
            assert!(now[2] as i32 <= last[2] as i32 + 1, "{c}: {now:?} after {last:?}");
            last = now;
        }
    }
}
