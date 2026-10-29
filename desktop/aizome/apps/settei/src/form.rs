//! Forms in Aizome's look, as every pane draws them: sections with a mono
//! caption over a card, rows with the label left in `text2` and the
//! control right, `line` dividers between them, explanations in `text3`.
//! Everything lands on the 4pt grid.
//!
//! ```ignore
//! form::page(vec![
//!     vcx.section("Key Repeat", vec![
//!         form::row("Delay until repeat", slider(..)).entry("repeat-delay"),
//!         form::row("Repeat rate", slider(..)).entry("repeat-rate").help("How fast a held key types."),
//!     ]),
//! ])
//! ```

use aizome_kit::column;
use aizome_kit::prelude::*;

use crate::pane::ViewCx;
use crate::widgets::{card, highlight};

/// The widest a pane's content gets.
pub const PAGE_WIDTH: f64 = 640.0;
/// A form row's least height.
pub const ROW: f64 = 40.0;
/// Padding inside a row, left and right.
pub const INSET: f64 = 12.0;

/// One row of a section.
pub struct Row<Msg> {
    label: Option<String>,
    control: Option<Element<Msg>>,
    help: Option<String>,
    detail: Option<String>,
    leading: Option<Element<Msg>>,
    entry: Option<String>,
    whole: Option<Element<Msg>>,
    stacked: bool,
    compact: bool,
}

/// A label and its control.
pub fn row<Msg>(label: impl Into<String>, control: impl Into<Element<Msg>>) -> Row<Msg> {
    Row {
        label: Some(label.into()),
        control: Some(control.into()),
        help: None,
        detail: None,
        leading: None,
        entry: None,
        whole: None,
        stacked: false,
        compact: false,
    }
}

/// A label with a value read out in `text3` at the right.
pub fn value<Msg: 'static>(label: impl Into<String>, value: impl Into<String>) -> Row<Msg> {
    row(label, text(value.into()).role(Role::Text3).selectable())
}

/// A label with a value in mono (versions, sizes, addresses).
pub fn value_mono<Msg: 'static>(label: impl Into<String>, value: impl Into<String>) -> Row<Msg> {
    row(label, text(value.into()).mono().size(12.0).role(Role::Text2).selectable())
}

/// A row that is one element, the whole width (a list, a preview).
pub fn whole<Msg>(element: impl Into<Element<Msg>>) -> Row<Msg> {
    Row {
        label: None,
        control: None,
        help: None,
        detail: None,
        leading: None,
        entry: None,
        whole: Some(element.into()),
        stacked: false,
        compact: false,
    }
}

/// A setting the system configuration pins: its value, "Set by your
/// system configuration", and how to change it.
pub fn managed<Msg: 'static>(label: impl Into<String>, value: impl Into<String>, how: impl Into<String>) -> Row<Msg> {
    let v = value.into();
    let control = aizome_kit::row![
        icon(Icon::Lock).size(12.0).role(Role::Text3),
        text(if v.is_empty() { "Not set".to_owned() } else { v }).role(Role::Text3).selectable(),
    ]
    .spacing(6.0)
    .align(Align::Center);
    let mut r = row(label, control);
    let how = how.into();
    r.help = Some(if how.is_empty() {
        "Set by your system configuration.".to_owned()
    } else {
        format!("Set by your system configuration. {how}")
    });
    r
}

impl<Msg> Row<Msg> {
    /// The search entry this row is (its id in the pane's entries).
    pub fn entry(mut self, id: impl Into<String>) -> Self {
        self.entry = Some(id.into());
        self
    }

    /// Whether it's a search entry's row already.
    pub fn has_entry(&self) -> bool {
        self.entry.is_some()
    }

    /// An explanation under the label, in `text3`.
    pub fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }

    pub fn help_maybe(mut self, text: Option<String>) -> Self {
        self.help = text;
        self
    }

    /// A second line beside the label (a device's address, a state).
    pub fn detail(mut self, text: impl Into<String>) -> Self {
        self.detail = Some(text.into());
        self
    }

    /// Something before the label (an icon, an avatar).
    pub fn leading(mut self, el: impl Into<Element<Msg>>) -> Self {
        self.leading = Some(el.into());
        self
    }

    /// Shorter, for a sheet of facts (a spec list).
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// The control under the label, the whole width (a long slider, a
    /// list of choices).
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }
}

fn body<Msg: 'static>(r: Row<Msg>) -> Element<Msg> {
    if let Some(w) = r.whole {
        return w;
    }
    let mut left = column![].spacing(3.0).width(Fill);
    if let Some(label) = r.label {
        left = left.push(text(label).role(Role::Text2).wrap());
    }
    if let Some(d) = r.detail {
        left = left.push(text(d).size(11.0).role(Role::Text3).wrap());
    }
    if let Some(h) = r.help.clone()
        && !r.stacked
    {
        left = left.push(text(h).size(11.0).role(Role::Text3).wrap().line_height(1.25));
    }
    let mut line = aizome_kit::row![].spacing(12.0).align(Align::Center).width(Fill);
    if let Some(l) = r.leading {
        line = line.push(l);
    }
    line = line.push(left);
    if r.stacked {
        let mut col = column![line].spacing(8.0).width(Fill);
        if let Some(c) = r.control {
            col = col.push(c);
        }
        if let Some(h) = r.help {
            col = col.push(text(h).size(11.0).role(Role::Text3).wrap().line_height(1.25));
        }
        return col.padding([10.0, INSET]).width(Fill).into();
    }
    if let Some(c) = r.control {
        line = line.push(c);
    }
    let (pad, min) = if r.compact { (5.0, 30.0) } else { (8.0, ROW) };
    container(line).padding([pad, INSET]).min_height(min).align_y(Align::Center).width(Fill).into()
}

impl ViewCx<'_> {
    /// A section: its caption (empty for none), then its rows on a card
    /// with dividers between them.
    pub fn section<Msg: 'static>(&self, title: &str, rows: Vec<Row<Msg>>) -> Element<Msg> {
        self.section_with(title, rows, None)
    }

    /// A section with a note under its card.
    pub fn section_with<Msg: 'static>(&self, title: &str, rows: Vec<Row<Msg>>, note: Option<String>) -> Element<Msg> {
        let mut items: Vec<Element<Msg>> = Vec::new();
        let n = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            let entry = r.entry.clone();
            let el = body(r);
            let el = match entry {
                Some(e) => {
                    let on = self.matches(&e);
                    highlight(el, on).id(self.entry_name(&e)).key(e)
                }
                None => el,
            };
            items.push(el);
            if i + 1 < n {
                items.push(divider().edge(Edge::Line).into());
            }
        }
        let mut col = column![].spacing(6.0).width(Fill);
        if !title.is_empty() {
            col = col.push(caption(title));
        }
        col = col.push(card(column(items).width(Fill)));
        if let Some(note) = note {
            col = col
                .push(container(text(note).size(11.0).role(Role::Text3).wrap().line_height(1.25)).padding([2.0, 2.0]));
        }
        col.into()
    }
}

/// A section's caption: mono, uppercase, `text3`, as the Deskbar's SHELF.
pub fn caption<Msg: 'static>(title: &str) -> Element<Msg> {
    container(text(title.to_uppercase()).mono().size(10.5).letter_spacing(0.8).role(Role::Text3))
        .padding([0.0, 2.0])
        .into()
}

/// A page of sections, spaced.
pub fn page<Msg>(sections: Vec<Element<Msg>>) -> Element<Msg> {
    column(sections).spacing(24.0).width(Fill).into()
}

/// A paragraph of explanation in `text3`.
pub fn note<Msg: 'static>(t: impl Into<String>) -> Element<Msg> {
    text(t.into()).size(12.0).role(Role::Text3).wrap().line_height(1.3).into()
}

/// The heading of a sub-page or a pane's hero: a title and a line under it.
pub fn heading<Msg: 'static>(title: impl Into<String>, sub: impl Into<String>) -> Element<Msg> {
    let sub = sub.into();
    let mut col = column![text(title.into()).size(20.0).weight(650.0)].spacing(4.0);
    if !sub.is_empty() {
        col = col.push(text(sub).role(Role::Text3).wrap().line_height(1.3));
    }
    col.into()
}

/// A dropdown of `options` with `selected` chosen, sending `f(index)`.
pub fn choice<Msg: Clone + Send + 'static>(
    options: &[&str],
    selected: usize,
    f: impl Fn(usize) -> Msg + 'static,
) -> Element<Msg> {
    dropdown(options.iter().map(|s| (*s).to_owned()), selected).on_select(f).min_width(160.0).into()
}

/// A segmented control of `options`, none lit when `selected` is `None`.
pub fn segments<Msg: Clone + Send + 'static>(
    options: &[&str],
    selected: impl Into<Option<usize>>,
    f: impl Fn(usize) -> Msg + 'static,
) -> Element<Msg> {
    segmented(options.iter().copied(), selected).on_select(f).into()
}

impl ViewCx<'_> {
    /// `el` as the place option `entry` is revealed at (a sub-page's
    /// heading, a tab's introduction), highlighted when search matches it.
    pub fn anchor<Msg: 'static>(&self, entry: &str, el: impl Into<Element<Msg>>) -> Element<Msg> {
        highlight(el, self.matches(entry)).id(self.entry_name(entry)).key(entry.to_owned())
    }
}
