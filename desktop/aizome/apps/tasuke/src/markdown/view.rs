//! A parsed answer as the kit's elements: paragraphs as [`prose`] (links
//! open, words select), headings, code in a sunken box with its language
//! and a copy button, quotes and callouts edged, tables lined, lists with
//! their markers. Adapted from Omperor's `markdown/view.rs`.
//!
//! [`prose`]: crate::widgets::prose

use std::rc::Rc;

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::{column as column_of, row as row_of};
use aizome_kit::{column, row};

use super::{Align as CellAlign, Block, Callout, Doc, Item, Run};
use crate::widgets::prose::prose;

/// How a document is drawn, and what its links and buttons send.
pub struct Look<Msg> {
    /// Body text's size, in points.
    pub size: f64,
    pub open: Rc<dyn Fn(String) -> Msg>,
    pub copy: Rc<dyn Fn(String) -> Msg>,
    /// `Text` for an answer, quieter for thinking.
    pub role: Role,
}

impl<Msg> Clone for Look<Msg> {
    fn clone(&self) -> Self {
        Look { size: self.size, open: self.open.clone(), copy: self.copy.clone(), role: self.role }
    }
}

const LINE_HEIGHT: f64 = 1.5;
/// Room for a list item's marker.
const MARKER: f64 = 22.0;

fn span(run: &Run, size: f64) -> (Span, Option<String>) {
    let mut s = Span::new(run.text.clone());
    if run.bold {
        s = s.weight(650.0);
    }
    if run.italic {
        s = s.italic();
    }
    if run.strike {
        s = s.strikethrough();
    }
    if run.code {
        s = s.mono().role(Role::Success).size(size * 0.9);
    }
    if let Some(role) = run.role {
        s = s.role(role);
    }
    let link = run.link.as_ref().map(|l| l.to_string());
    if link.is_some() {
        s = s.role(Role::Accent).underline();
    }
    (s, link)
}

fn paragraph<Msg: Clone + Send + 'static>(runs: &[Run], look: &Look<Msg>, size: f64, weight: Option<f32>) -> Element<Msg> {
    let open = look.open.clone();
    let runs = runs.iter().map(|r| {
        let (s, l) = span(r, size);
        (if let Some(w) = weight { s.weight(w) } else { s }, l)
    });
    prose(runs)
        .font(FontSpec::ui(size))
        .role(look.role)
        .line_height(LINE_HEIGHT)
        .on_link(Rc::new(move |t: String| open(t)))
        .into()
}

/// The document.
pub fn doc<Msg: Clone + Send + 'static>(doc: &Doc, look: &Look<Msg>) -> Element<Msg> {
    blocks(&doc.blocks, look)
}

fn blocks<Msg: Clone + Send + 'static>(blocks: &[Block], look: &Look<Msg>) -> Element<Msg> {
    column_of(blocks.iter().map(|b| block(b, look)).collect()).spacing(10.0).width(Fill).into()
}

fn block<Msg: Clone + Send + 'static>(block: &Block, look: &Look<Msg>) -> Element<Msg> {
    match block {
        Block::Paragraph(runs) => paragraph(runs, look, look.size, None),
        Block::Heading { level, runs } => {
            let (scale, weight) = match level {
                1 => (1.4, 700.0),
                2 => (1.22, 680.0),
                3 => (1.08, 650.0),
                _ => (1.0, 650.0),
            };
            container(paragraph(runs, look, look.size * scale, Some(weight)))
                .padding([if *level <= 2 { 6.0 } else { 2.0 }, 0.0, 0.0, 0.0])
                .width(Fill)
                .into()
        }
        Block::Code { lang, text, lines } => code_block(lang, text, lines, look),
        Block::Quote(inner) => {
            container(blocks(inner, look)).edges(Edge::Line2, Sides::LEFT).padding([2.0, 0.0, 2.0, 14.0]).width(Fill).into()
        }
        Block::Callout { kind, blocks: inner } => callout(*kind, inner, look),
        Block::List { start, items } => list(*start, items, look),
        Block::Table { aligns, head, rows } => table_block(aligns, head, rows, look),
        Block::Rule => container(divider()).padding([4.0, 0.0]).width(Fill).into(),
    }
}

fn code_block<Msg: Clone + Send + 'static>(
    lang: &str,
    source: &std::sync::Arc<str>,
    lines: &std::sync::Arc<Vec<Vec<Run>>>,
    look: &Look<Msg>,
) -> Element<Msg> {
    let size = look.size * 0.9;
    let mut spans = Vec::with_capacity(lines.iter().map(Vec::len).sum::<usize>() + lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            spans.push((Span::new("\n"), None));
        }
        spans.extend(line.iter().map(|run| {
            let s = Span::new(run.text.clone());
            (
                match run.role {
                    Some(role) => s.role(role),
                    None => s,
                },
                None,
            )
        }));
    }
    let label = if lang.is_empty() { "code".to_owned() } else { lang.to_owned() };
    let header = row![
        text(label).mono().size(11.0).role(Role::Text3),
        spacer(),
        icon_button(Icon::Copy).on_press((look.copy)(source.to_string())).tooltip("Copy the code"),
    ]
    .spacing(6.0)
    .padding([2.0, 4.0, 2.0, 10.0]);
    let code = prose::<Msg>(spans).font(FontSpec::mono(size)).line_height(1.4);
    let body = container(code).padding([8.0, 12.0, 10.0, 12.0]).width(Fill);
    container(column![header, divider(), body].width(Fill)).fill(Tone::Sunk).border(Edge::Line).width(Fill).into()
}

fn callout<Msg: Clone + Send + 'static>(kind: Callout, inner: &[Block], look: &Look<Msg>) -> Element<Msg> {
    let glyph = match kind {
        Callout::Note => Icon::Info,
        Callout::Tip => Icon::Star,
        Callout::Important => Icon::CircleAlert,
        Callout::Warning => Icon::TriangleAlert,
        Callout::Caution => Icon::CircleX,
    };
    let title =
        row![icon(glyph).size(15.0).role(kind.role()), text(kind.title()).semibold().role(kind.role())].spacing(8.0);
    container(column![title, blocks(inner, look)].spacing(6.0).width(Fill))
        .fill(Tone::Chrome)
        .border(Edge::Line)
        .padding([10.0, 14.0])
        .width(Fill)
        .into()
}

fn list<Msg: Clone + Send + 'static>(start: Option<u64>, items: &[Item], look: &Look<Msg>) -> Element<Msg> {
    let rows = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker: Element<Msg> = match (item.task, start) {
                (Some(done), _) => icon(if done { Icon::SquareCheck } else { Icon::Square })
                    .size(15.0)
                    .role(if done { Role::Success } else { Role::Text3 })
                    .into(),
                (None, Some(first)) => text(format!("{}.", first + index as u64))
                    .size(look.size)
                    .line_height(LINE_HEIGHT)
                    .tabular()
                    .role(Role::Text3)
                    .into(),
                (None, None) => text("•").size(look.size).line_height(LINE_HEIGHT).role(Role::Text3).into(),
            };
            let top = if item.task.is_some() { 4.0 } else { 0.0 };
            row![container(marker).width(MARKER).padding([top, 0.0, 0.0, 0.0]), blocks(&item.blocks, look)]
                .align(Align::Start)
                .width(Fill)
                .into()
        })
        .collect();
    column_of(rows).spacing(6.0).width(Fill).into()
}

fn table_block<Msg: Clone + Send + 'static>(
    aligns: &[CellAlign],
    head: &[Vec<Run>],
    rows: &[Vec<Vec<Run>>],
    look: &Look<Msg>,
) -> Element<Msg> {
    let columns = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return space(0.0);
    }
    // Each column's share of the width, by the longest text it holds.
    let widest = |c: usize| {
        std::iter::once(head.get(c))
            .chain(rows.iter().map(|r| r.get(c)))
            .flatten()
            .map(|runs| super::runs_text(runs).chars().count())
            .max()
            .unwrap_or(1)
    };
    let portions: Vec<u16> = (0..columns).map(|c| (widest(c) as u16).clamp(4, 40)).collect();
    let size = look.size * 0.95;
    let cell = |runs: Option<&Vec<Run>>, c: usize, header: bool| -> Element<Msg> {
        let runs = runs.cloned().unwrap_or_default();
        let content = rich_text(runs.iter().map(|r| {
            let (s, _) = span(r, size);
            if header { s.weight(650.0) } else { s }
        }))
        .font(FontSpec::ui(size))
        .role(if header { Role::Text2 } else { look.role })
        .wrap();
        let align = match aligns.get(c) {
            Some(CellAlign::Center) => Align::Center,
            Some(CellAlign::End) => Align::End,
            _ => Align::Start,
        };
        let portion = std::num::NonZeroU16::new(portions[c]).unwrap_or(std::num::NonZeroU16::MIN);
        container(content).padding([6.0, 10.0]).align_x(align).width(Length::FillPortion(portion)).into()
    };
    let line = |cells: &[Vec<Run>], header: bool| -> Element<Msg> {
        row_of((0..columns).map(|c| cell(cells.get(c), c, header)).collect()).align(Align::Start).width(Fill).into()
    };
    let mut lines: Vec<Element<Msg>> = vec![container(line(head, true)).fill(Tone::Chrome).width(Fill).into()];
    for r in rows {
        lines.push(divider().into());
        lines.push(line(r, false));
    }
    container(column_of(lines).width(Fill)).border(Edge::Line).padding(1.0).width(Fill).into()
}
