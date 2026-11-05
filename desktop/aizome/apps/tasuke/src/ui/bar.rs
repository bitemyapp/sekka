//! The memory-fit bar: a model's weights, its context and its working
//! memory, side by side against the budget of where it would run, with
//! what's left (or what's too much) after them.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::row as row_of;
use aizome_kit::{column, row};

use crate::app::Msg;
use crate::models::{FitBar, Part, size};
use tasuke_models::Bytes;

fn part_role(p: Part) -> Option<Role> {
    match p {
        Part::Weights => Some(Role::Accent),
        Part::Context => Some(Role::Cyan),
        Part::Working => Some(Role::Violet),
        Part::Spare => None,
        Part::Over => Some(Role::Danger),
    }
}

fn code(p: Part) -> u8 {
    match p {
        Part::Weights => 0,
        Part::Context => 1,
        Part::Working => 2,
        Part::Spare => 3,
        Part::Over => 4,
    }
}

fn part_of(c: u8) -> Part {
    match c {
        0 => Part::Weights,
        1 => Part::Context,
        2 => Part::Working,
        3 => Part::Spare,
        _ => Part::Over,
    }
}

#[derive(Hash)]
struct Shape {
    /// Each part and its share, in ten-thousandths.
    parts: Vec<(u8, u32)>,
    /// Where the budget ends, in ten-thousandths (10,000 unless it overflows).
    mark: u32,
}

/// The bar alone, `height` tall.
pub fn bar(b: &FitBar, height: f64) -> Element<Msg> {
    let shape = Shape {
        parts: b.shares().iter().map(|(p, s)| (code(*p), (s * 10_000.0).round() as u32)).collect(),
        mark: (b.budget_mark() * 10_000.0).round() as u32,
    };
    drawn(shape, height)
}

/// Parts as a bar, each its share of the whole.
fn parts_bar(parts: &[(Part, Bytes)], height: f64) -> Element<Msg> {
    let whole: u64 = parts.iter().map(|(_, b)| b.0).sum::<u64>().max(1);
    let shape = Shape {
        parts: parts.iter().map(|(p, b)| (code(*p), (b.0 as f64 / whole as f64 * 10_000.0).round() as u32)).collect(),
        mark: 10_000,
    };
    drawn(shape, height)
}

fn drawn(shape: Shape, height: f64) -> Element<Msg> {
    canvas(shape, |c, r, s, theme| {
        c.fill_rect(r, theme.sunk());
        let mut x = r.x0;
        for &(p, share) in &s.parts {
            let w = (r.width() * f64::from(share) / 10_000.0).max(0.0);
            let x1 = (x + w).round().min(r.x1);
            if let Some(role) = part_role(part_of(p)) {
                c.fill_rect(Rect::new(x.round(), r.y0, x1, r.y1), theme.role(role));
            }
            // A hairline between parts.
            if x > r.x0 + 0.5 {
                c.fill_rect(Rect::new(x.round(), r.y0, x.round() + 1.0, r.y1), theme.sunk());
            }
            x = x1;
        }
        let line = theme.line2();
        c.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 1.0), line);
        c.fill_rect(Rect::new(r.x0, r.y1 - 1.0, r.x1, r.y1), line);
        c.fill_rect(Rect::new(r.x0, r.y0, r.x0 + 1.0, r.y1), line);
        c.fill_rect(Rect::new(r.x1 - 1.0, r.y0, r.x1, r.y1), line);
        if s.mark < 10_000 {
            let mx = (r.x0 + r.width() * f64::from(s.mark) / 10_000.0).round();
            c.fill_rect(Rect::new(mx - 1.0, r.y0 - 3.0, mx + 1.0, r.y1 + 3.0), theme.text());
        }
    })
    .height(height)
    .width(Fill)
    .into()
}

fn swatch(p: Part) -> Element<Msg> {
    let k = code(p);
    canvas(k, |c, r, k, theme| match part_role(part_of(*k)) {
        Some(role) => c.fill_rect(r, theme.role(role)),
        None => {
            c.fill_rect(r, theme.sunk());
            let l = theme.line2();
            c.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 1.0), l);
            c.fill_rect(Rect::new(r.x0, r.y1 - 1.0, r.x1, r.y1), l);
            c.fill_rect(Rect::new(r.x0, r.y0, r.x0 + 1.0, r.y1), l);
            c.fill_rect(Rect::new(r.x1 - 1.0, r.y0, r.x1, r.y1), l);
        }
    })
    .width(9.0)
    .height(9.0)
    .into()
}

/// What each part is, with its size.
pub fn legend(b: &FitBar) -> Element<Msg> {
    let items: Vec<Element<Msg>> = b
        .parts
        .iter()
        .map(|&(p, bytes)| {
            // Split between the GPU and system memory, the bar is the GPU's.
            let name = if p == Part::Weights && b.in_ram.is_some() { "Model on the GPU" } else { p.words() };
            let mut words = format!("{name} {}", size(bytes));
            if p == Part::Context
                && let Some(ctx) = b.context
            {
                words.push_str(&format!(" ({ctx} tokens)"));
            }
            row![swatch(p), text(words).size(11.5).role(Role::Text3)].spacing(5.0).into()
        })
        .collect();
    row_of(items).spacing(14.0).into()
}

/// The verdict in words, coloured by how comfortable it is.
pub fn verdict(b: &FitBar) -> Element<Msg> {
    use crate::models::Verdict;
    let (glyph, role) = match b.verdict {
        Verdict::Fits => (Icon::CircleCheck, Role::Success),
        Verdict::Tight | Verdict::Split => (Icon::TriangleAlert, Role::Warning),
        Verdict::Processor => (Icon::Cpu, Role::Warning),
        Verdict::TooLarge => (Icon::CircleX, Role::Danger),
    };
    row![icon(glyph).size(14.0).role(role), text(b.words()).size(12.5).role(Role::Text)].spacing(7.0).into()
}

/// For a model split between the GPU and system memory, the memory side:
/// what of the model is there, against what system memory allows it.
pub fn in_memory(b: &FitBar) -> Option<Element<Msg>> {
    let parts = b.ram_parts()?;
    let budget = b.ram_budget?;
    let words: Vec<Element<Msg>> = parts
        .iter()
        .map(|&(p, bytes)| {
            let name = match p {
                Part::Weights => "Model in system memory",
                other => other.words(),
            };
            row![swatch(p), text(format!("{name} {}", size(bytes))).size(11.5).role(Role::Text3)].spacing(5.0).into()
        })
        .chain(std::iter::once(text(format!("of the {} it may use", size(budget))).size(11.5).role(Role::Text3).into()))
        .collect();
    Some(column![parts_bar(&parts, 8.0), row_of(words).spacing(14.0)].spacing(6.0).width(Fill).into())
}
