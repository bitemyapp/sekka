//! Standalone development probe; compiled beside unicode.rs/unicode_tables.rs.
#![allow(dead_code)]
#[path = "unicode.rs"]
mod unicode;
use std::io::{self, Read, Write};

fn main() {
    let mut output = io::BufWriter::new(io::stdout().lock());
    if std::env::args().nth(1).as_deref() == Some("widths") {
        for value in 0..=0x10ffff {
            let width = char::from_u32(value)
                .map_or(255, |ch| unicode::properties(ch).width() as u8);
            output.write_all(&[width]).unwrap();
        }
        return;
    }
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes).unwrap();
    let mut at = 0;
    while at < bytes.len() {
        let count = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        at += 2;
        let mut chars = Vec::with_capacity(count);
        for _ in 0..count {
            chars.push(char::from_u32(u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())).unwrap());
            at += 4;
        }
        let (mut count, mut width) = (0, 0);
        if let Some(&first) = chars.first() {
            count = 1;
            width = unicode::properties(first).width();
            let mut previous = first;
            let mut state = unicode::BreakState::default();
            for &current in &chars[1..] {
                let before = state;
                if unicode::grapheme_break(previous, current, &mut state) {
                    break;
                }
                match unicode::width_effect(previous, current) {
                    unicode::WidthEffect::Ignore => state = before,
                    unicode::WidthEffect::Unchanged => previous = current,
                    unicode::WidthEffect::Wide => { width = 2; previous = current; }
                    unicode::WidthEffect::Narrow => { width = 1; previous = current; }
                }
                count += 1;
            }
        }
        output.write_all(&(count as u16).to_le_bytes()).unwrap();
        output.write_all(&[width as u8]).unwrap();
    }
}
