//! Pinned Ghostty/uucode Unicode 18 terminal grapheme semantics.
//! Adapted from MIT-licensed uucode (Jacob Sandlund, 2026) and Ghostty.
//! The terminal filters controls before calling this tailored break algorithm.

#[path = "unicode_tables.rs"]
mod tables;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Properties(u16);
impl Properties {
    #[inline]
    pub fn width(self) -> usize {
        usize::from(self.0 & 3)
    }
    #[inline]
    fn class(self) -> u8 {
        ((self.0 >> 3) & 31) as u8
    }
    fn zero_in_grapheme(self) -> bool {
        self.0 & 4 != 0
    }
    pub fn emoji_vs_base(self) -> bool {
        self.0 & 256 != 0
    }
    pub fn extended_pictographic(self) -> bool {
        self.class() == EP
    }
}

#[inline]
pub(crate) fn properties(ch: char) -> Properties {
    let cp = ch as usize;
    let block = usize::from(tables::STAGE1[cp >> 8]);
    let index = usize::from(tables::STAGE2[block + (cp & 255)]);
    Properties(tables::PROPERTIES[index])
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct BreakState(u8);

#[inline]
pub(crate) fn grapheme_break(previous: char, current: char, state: &mut BreakState) -> bool {
    break_properties(properties(previous), properties(current), state)
}

#[inline]
pub(crate) fn break_properties(
    previous: Properties,
    current: Properties,
    state: &mut BreakState,
) -> bool {
    let index = usize::from(state.0)
        | (usize::from(previous.class()) << 3)
        | (usize::from(current.class()) << 8);
    let result = BREAKS[index];
    state.0 = result >> 1;
    result & 1 != 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WidthEffect {
    Ignore,
    Unchanged,
    Wide,
    Narrow,
}

pub(crate) fn width_effect(previous: char, current: char) -> WidthEffect {
    if matches!(current, '\u{fe0e}' | '\u{fe0f}') {
        if !properties(previous).emoji_vs_base() {
            WidthEffect::Ignore
        } else if current == '\u{fe0f}' {
            WidthEffect::Wide
        } else {
            WidthEffect::Narrow
        }
    } else if properties(current).zero_in_grapheme() {
        WidthEffect::Unchanged
    } else {
        WidthEffect::Wide
    }
}

const PREPEND: u8 = 1;
const RI: u8 = 2;
const SPACING_MARK: u8 = 3;
const L: u8 = 4;
const V: u8 = 5;
const T: u8 = 6;
const LV: u8 = 7;
const LVT: u8 = 8;
const ZWJ: u8 = 9;
const ZWNJ: u8 = 10;
const EP: u8 = 11;
const EMOJI_BASE: u8 = 12;
const EMOJI_MODIFIER: u8 = 13;
const INDIC_EXTEND: u8 = 14;
const LINKER_EXTEND: u8 = 15;
const LINKER_OTHER: u8 = 16;
const CONSONANT: u8 = 17;

const fn extend(class: u8) -> bool {
    matches!(class, ZWNJ | INDIC_EXTEND | LINKER_EXTEND)
}
const fn pictographic(class: u8) -> bool {
    matches!(class, EP | EMOJI_BASE)
}
const fn indic_extend(class: u8) -> bool {
    matches!(class, INDIC_EXTEND | ZWJ)
}
const fn emoji_sequence(class: u8) -> bool {
    matches!(
        class,
        INDIC_EXTEND | LINKER_EXTEND | ZWNJ | ZWJ | EP | EMOJI_BASE | EMOJI_MODIFIER
    )
}

// State is after_linker in bit0 plus a two-bit base: default0, emoji1, RI2.
const fn transition(first: u8, second: u8, old: u8) -> u8 {
    let after_linker =
        matches!(first, LINKER_EXTEND | LINKER_OTHER) || (old & 1 != 0 && indic_extend(first));
    let linker = after_linker && indic_extend(second);
    let mut base = old >> 1;
    if (base == 2 && (first != RI || second != RI))
        || (base == 1 && (!emoji_sequence(first) || !emoji_sequence(second)))
    {
        base = 0;
    }
    let mut boundary = false;
    if (first == L && matches!(second, L | V | LV | LVT))
        || (matches!(first, LV | V) && matches!(second, V | T))
        || (matches!(first, LVT | T) && second == T)
        || second == SPACING_MARK
        || first == PREPEND
    {
        // GB6–9b.
    } else if after_linker && second == CONSONANT {
        base = 0; // GB9c.
    } else if pictographic(first) && (extend(second) || second == ZWJ)
        || first == EMOJI_BASE && second == EMOJI_MODIFIER
    {
        base = 1;
    } else {
        let mut emoji_join = false;
        if !pictographic(first) && base == 1 {
            if (extend(first) || first == EMOJI_MODIFIER) && (extend(second) || second == ZWJ) {
                emoji_join = true;
            } else if first == ZWJ && pictographic(second) {
                base = 0;
                emoji_join = true;
            } else {
                base = 0;
            }
        }
        if !emoji_join {
            if first == RI && second == RI {
                if base == 0 {
                    base = 2;
                } else {
                    base = 0;
                    boundary = true;
                }
            } else {
                boundary = !(extend(second) || second == ZWJ);
            }
        }
    }
    let state = (base << 1) | linker as u8;
    (state << 1) | boundary as u8
}

const fn make_breaks() -> [u8; 8192] {
    let mut result = [1; 8192];
    let mut first = 0;
    while first < 18 {
        let mut second = 0;
        while second < 18 {
            let mut state = 0;
            while state < 5 {
                let index = state as usize | ((first as usize) << 3) | ((second as usize) << 8);
                result[index] = transition(first, second, state);
                state += 1;
            }
            second += 1;
        }
        first += 1;
    }
    result
}

static BREAKS: [u8; 8192] = make_breaks();
