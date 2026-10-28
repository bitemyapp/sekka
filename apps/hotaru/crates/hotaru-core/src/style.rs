//! Colors remain symbolic until painting, so palette changes affect old cells.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(Rgb),
}

pub mod attrs {
    pub const BOLD: u16 = 1;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const BLINK: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const HIDDEN: u16 = 1 << 6;
    pub const STRIKE: u16 = 1 << 7;
    pub const OVERLINE: u16 = 1 << 8;
    pub const UNDERLINE_STYLE_MASK: u16 = 7 << 9;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Underline {
    #[default]
    None = 0,
    Single = 1,
    Double = 2,
    Curly = 3,
    Dotted = 4,
    Dashed = 5,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub underline_color: Color,
    pub attrs: u16,
}

impl Style {
    pub fn underline(self) -> Underline {
        if self.attrs & attrs::UNDERLINE == 0 {
            return Underline::None;
        }
        match (self.attrs & attrs::UNDERLINE_STYLE_MASK) >> 9 {
            2 => Underline::Double,
            3 => Underline::Curly,
            4 => Underline::Dotted,
            5 => Underline::Dashed,
            _ => Underline::Single,
        }
    }
    pub fn set_underline(&mut self, value: Underline) {
        self.attrs &= !(attrs::UNDERLINE | attrs::UNDERLINE_STYLE_MASK);
        if value != Underline::None {
            self.attrs |= attrs::UNDERLINE;
        }
        if !matches!(value, Underline::None | Underline::Single) {
            self.attrs |= (value as u16) << 9;
        }
    }
    pub(crate) fn erased(self) -> Self {
        Self {
            background: self.background,
            ..Self::default()
        }
    }
    pub(crate) fn sgr(self) -> String {
        use std::fmt::Write;
        let mut value = String::from("0");
        for (flag, code) in [(attrs::BOLD, 1), (attrs::DIM, 2), (attrs::ITALIC, 3)] {
            if self.attrs & flag != 0 {
                let _ = write!(value, ";{code}");
            }
        }
        match self.underline() {
            Underline::None => {}
            Underline::Single => value.push_str(";4"),
            kind => {
                let _ = write!(value, ";4:{}", kind as u8);
            }
        }
        for (flag, code) in [
            (attrs::OVERLINE, 53),
            (attrs::BLINK, 5),
            (attrs::INVERSE, 7),
            (attrs::HIDDEN, 8),
            (attrs::STRIKE, 9),
        ] {
            if self.attrs & flag != 0 {
                let _ = write!(value, ";{code}");
            }
        }
        for (color, kind) in [(self.foreground, 38), (self.background, 48)] {
            match color {
                Color::Default => {}
                Color::Indexed(index) if index < 16 && kind != 58 => {
                    let base = if index < 8 { kind - 8 } else { kind + 52 };
                    let _ = write!(value, ";{}", base + u16::from(index % 8));
                }
                Color::Indexed(index) => {
                    let _ = write!(value, ";{kind}:5:{index}");
                }
                Color::Rgb(Rgb(r, g, b)) => {
                    let _ = write!(value, ";{kind}:2::{r}:{g}:{b}");
                }
            }
        }
        value.push('m');
        value
    }
}

pub fn default_palette() -> [Rgb; 256] {
    let mut colors = [Rgb(0, 0, 0); 256];
    colors[..16].copy_from_slice(&[
        Rgb(0, 0, 0),
        Rgb(205, 0, 0),
        Rgb(0, 205, 0),
        Rgb(205, 205, 0),
        Rgb(0, 0, 238),
        Rgb(205, 0, 205),
        Rgb(0, 205, 205),
        Rgb(229, 229, 229),
        Rgb(127, 127, 127),
        Rgb(255, 0, 0),
        Rgb(0, 255, 0),
        Rgb(255, 255, 0),
        Rgb(92, 92, 255),
        Rgb(255, 0, 255),
        Rgb(0, 255, 255),
        Rgb(255, 255, 255),
    ]);
    let steps = [0, 95, 135, 175, 215, 255];
    for i in 0..216 {
        colors[16 + i] = Rgb(steps[i / 36], steps[(i / 6) % 6], steps[i % 6]);
    }
    for i in 0..24 {
        let v = 8 + i as u8 * 10;
        colors[232 + i] = Rgb(v, v, v);
    }
    colors
}
