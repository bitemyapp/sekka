//! Koyomi's window: the toolbar, the sidebar (a mini-month and the
//! calendars), the month, week, day and year views, the status line,
//! and the sheets.

pub mod colors;
pub mod editor;
pub mod mini;
pub mod month;
pub mod sheets;
pub mod week;

use std::cell::RefCell;
use std::sync::Arc;

use aizome_kit::element::{AnyMsg, MsgFn};
use aizome_kit::hash::PaintHasher;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key as KeyName, KeyEvent, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Constraints, column, row};

use crate::app::{Koyomi, Msg, View};
use crate::model::Dye;

/// Send a message made from a value (the kit's `emit` takes makers).
pub fn send(cx: &mut EventCx, m: AnyMsg) {
    let cell = RefCell::new(Some(m));
    cx.emit(&MsgFn::from_fn(move || cell.borrow_mut().take().unwrap_or_else(|| Box::new(()))));
}

/// The keys the calendar answers anywhere in the main view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Prev,
    Next,
    Today,
    Delete,
    Open,
    Deselect,
}

fn key_action(k: &KeyEvent) -> Option<KeyAction> {
    let m = k.modifiers;
    if m.super_key() || m.control() || m.alt() {
        return None;
    }
    match &k.key {
        KeyName::Named(NamedKey::ArrowLeft) => Some(KeyAction::Prev),
        KeyName::Named(NamedKey::ArrowRight) => Some(KeyAction::Next),
        KeyName::Named(NamedKey::Delete | NamedKey::Backspace) => Some(KeyAction::Delete),
        KeyName::Named(NamedKey::Enter) => Some(KeyAction::Open),
        KeyName::Named(NamedKey::Escape) => Some(KeyAction::Deselect),
        KeyName::Character(s) if s.eq_ignore_ascii_case("t") => Some(KeyAction::Today),
        _ => None,
    }
}

/// Around the main view: takes the focus on a click (so keys work),
/// and turns ← → T ⌫ ↩ ⎋ into the calendar's actions.
struct Keys;

impl Widget for Keys {
    fn name(&self) -> &'static str {
        "koyomi-keys"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Key(k) if k.pressed => match key_action(k) {
                Some(a) => {
                    cx.emit(&MsgFn::new(Msg::Key(a)));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            // A press anywhere in the calendar gives it the keys.
            Event::PointerDown { .. } => {
                if !cx.is_focused() {
                    cx.request_focus();
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }
}

/// A calendar in the sidebar: its colour (filled when shown), its name,
/// how many events it has.
#[derive(Clone, Debug, PartialEq, Hash)]
struct CalRow {
    name: Arc<str>,
    dye: Dye,
    shown: bool,
    default: bool,
    count: usize,
}

struct CalRowWidget {
    r: CalRow,
    on: MsgFn,
}

impl Widget for CalRowWidget {
    fn name(&self) -> &'static str {
        "koyomi-calendar-row"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width, 26.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        if cx.is_hovered() {
            cx.canvas.fill_rect(b, t.hover(t.sunk()));
        }
        let dye = colors::dye(t, self.r.dye);
        let sq = Rect::new(12.0, 7.0, 24.0, 19.0);
        if self.r.shown {
            cx.canvas.fill_rect(sq, dye);
            // A check, in what reads on the dye.
            let on = colors::on_dye(t, dye);
            aizome_kit::Icon::Check.draw(cx.canvas, Point::new(13.0, 8.0), 10.0, on);
        } else {
            aizome_kit::draw::border(cx.canvas, sq, dye, 1.5);
        }
        let mut f = FontSpec::ui(13.0);
        f.weight = if self.r.default { 600.0 } else { 400.0 };
        let fg = if self.r.shown { t.text() } else { t.text3() };
        let right = b.x1 - 10.0;
        let count = self.r.count.to_string();
        let w = cx.draw_text_in(
            &count,
            &TextSpec::new(FontSpec { tabular: true, ..FontSpec::mono(11.0) }, t.text3()),
            Rect::new(32.0, 0.0, right, b.y1),
            Align::End,
        );
        cx.draw_text_in(&self.r.name, &TextSpec::new(f, fg), Rect::new(32.0, 0.0, right - w - 8.0, b.y1), Align::Start);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.r);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, mods, .. } if !mods.control() => {
                cx.emit(&self.on);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&self.on);
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::CheckBox,
            label: Some(self.r.name.to_string()),
            checked: Some(self.r.shown),
            actions: vec![SemanticAction::Click],
            ..Default::default()
        })
    }

    fn text(&self) -> Option<&str> {
        Some(&self.r.name)
    }
}

fn caption_mono<Msg: 'static>(s: &str) -> Element<Msg> {
    aizome_kit::caption(s.to_owned())
}

fn sidebar(k: &Koyomi) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = vec![
        container(mini::mini_month(k.mini_model(), Msg::Mini)).padding([8.0, 9.0, 4.0, 9.0]).width(Length::Fill).into(),
    ];
    items.push(caption_mono("Calendars"));
    let default = k.default_calendar();
    for c in &k.store.calendars {
        let shown = !k.state.hidden.contains(&c.id);
        let r = CalRow {
            name: c.name.as_str().into(),
            dye: c.color,
            shown,
            default: default.as_deref() == Some(c.id.as_str()),
            count: c.events.len(),
        };
        let id = c.id.clone();
        let el = Element::new(CalRowWidget { r, on: MsgFn::new(Msg::ToggleCalendar(id.clone())) }, Vec::new())
            .key(("cal", id.clone()))
            .width(Length::Fill);
        let mut colours = Menu::new("Colour");
        for d in Dye::ALL {
            colours = colours.item(MenuItem::new(d.name(), Msg::SetColor(id.clone(), d)).checked(c.color == d));
        }
        let menu = Menu::new(c.name.clone())
            .item(MenuItem::new(if shown { "Hide" } else { "Show" }, Msg::ToggleCalendar(id.clone())))
            .item(MenuItem::new("Show Only This Calendar", Msg::Solo(id.clone())))
            .separator()
            .item(MenuItem::new("Rename…", Msg::RenameCalendar(id.clone())))
            .submenu(colours)
            .item(
                MenuItem::new("New Events Go Here", Msg::MakeDefault(id.clone()))
                    .checked(default.as_deref() == Some(id.as_str())),
            )
            .separator()
            .item(MenuItem::new("Import into This Calendar…", Msg::ImportInto(id.clone())))
            .item(MenuItem::new("Export…", Msg::Export(id.clone())))
            .separator()
            .item(MenuItem::new("Delete Calendar…", Msg::DeleteCalendar(id.clone())));
        items.push(context_menu(el, menu).width(Length::Fill).into());
    }
    items.push(
        container(button("New Calendar…").plain().icon(Icon::Plus).on_press(Msg::NewCalendar).id("new-calendar"))
            .padding([4.0, 6.0])
            .into(),
    );
    // What's next, from the next week.
    let next = k.up_next();
    if !next.is_empty() {
        items.push(caption_mono("Up Next"));
        for (dye, when, title) in next {
            items.push(
                container(
                    row![
                        Element::new(Swatch(dye), Vec::new()),
                        column![
                            text(title).size(12.0).semibold(),
                            text(when).mono().tabular().size(10.5).role(Role::Text3)
                        ]
                        .spacing(1.0)
                        .width(Length::Fill),
                    ]
                    .spacing(8.0)
                    .align(Align::Start),
                )
                .padding([3.0, 10.0, 3.0, 12.0])
                .width(Length::Fill)
                .into(),
            );
        }
    }
    items.push(spacer().into());
    scroll(aizome_kit::widgets::layout::column(items).width(Length::Fill)).into()
}

/// A dye's small square.
struct Swatch(Dye);

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "koyomi-swatch"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(8.0, 14.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let c = colors::dye(cx.theme(), self.0);
        cx.canvas.fill_rect(Rect::new(0.0, 4.0, 8.0, 12.0), c);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.0);
    }
}

fn toolbar_view(k: &Koyomi) -> Element<Msg> {
    let (title, sub) = k.title();
    let view_index = match k.view {
        View::Day => 0,
        View::Week => 1,
        View::Month => 2,
        View::Year => 3,
    };
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::ChevronLeft).toolbar().on_press(Msg::Prev).tooltip("Previous (←)").label("Previous").into(),
        icon_button(Icon::ChevronRight).toolbar().on_press(Msg::Next).tooltip("Next (→)").label("Next").into(),
        button("Today").toolbar().on_press(Msg::Today).tooltip("Today (T)").into(),
        toolbar_separator(),
        text(title).semibold().size(15.0).into(),
    ];
    if let Some(s) = sub {
        items.push(space(6.0));
        items.push(text(s).mono().tabular().size(11.0).role(Role::Text3).into());
    }
    items.push(spacer().into());
    items.push(
        segmented(["Day", "Week", "Month", "Year"], view_index)
            .on_select(|i| Msg::SetView([View::Day, View::Week, View::Month, View::Year][i.min(3)]))
            .id("views")
            .into(),
    );
    items.push(toolbar_separator());
    items.push(
        icon_button(Icon::Plus).toolbar().on_press(Msg::NewEvent).tooltip("New Event (❖N)").label("New Event").into(),
    );
    toolbar(items)
}

fn status(k: &Koyomi) -> Element<Msg> {
    let zone = if k.here.id.is_empty() { "Local time".to_owned() } else { k.here.id.to_string() };
    let mut items: Vec<Element<Msg>> = vec![status_text(zone)];
    items.push(status_text(format!("Week {}", k.weeks().number(k.cursor))));
    items.push(spacer().into());
    match &k.note {
        Some((n, _)) => items.push(text(n.clone()).size(11.0).role(Role::Text2).into()),
        None => {
            let n = k.occ.iter().filter(|o| !o.cancelled).count();
            items.push(status_text(format!("{n} event{}", if n == 1 { "" } else { "s" })));
        }
    }
    status_bar(items)
}

/// The day view's side: the selected event, or the day's agenda.
fn details(k: &Koyomi) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    match k.selected_event() {
        Some((o, cal, e)) => {
            items.push(
                text(if e.summary.is_empty() { "New Event".to_owned() } else { e.summary.clone() })
                    .semibold()
                    .size(17.0)
                    .wrap()
                    .into(),
            );
            items.push(
                row![Element::new(Swatch(cal.color), Vec::new()), text(cal.name.clone()).size(12.0).role(Role::Text2)]
                    .spacing(6.0)
                    .into(),
            );
            items.push(divider().into());
            items.push(text(crate::format::span(o.start, o.end, o.all_day, k.h24())).mono().size(12.0).into());
            if let Some(rule) = &k.master_rule(o) {
                items.push(text(rule.clone()).size(12.0).role(Role::Text2).wrap().into());
            }
            if !e.location.is_empty() {
                items.push(
                    row![icon(Icon::MapPin).size(14.0).role(Role::Text3), text(e.location.clone()).size(12.0).wrap()]
                        .spacing(6.0)
                        .align(Align::Start)
                        .into(),
                );
            }
            for a in e.alarms.iter().filter(|a| a.is_active()) {
                let label = match a.minutes_before() {
                    Some(m) if o.all_day && m < 0 => format!(
                        "On the day, at {}",
                        crate::format::time(
                            jiff::civil::Time::midnight().wrapping_add(jiff::SignedDuration::from_mins(-m)),
                            k.h24()
                        )
                    ),
                    Some(0) => "At the start".to_owned(),
                    Some(m) if m > 0 => format!("{} before", crate::format::minutes(m)),
                    Some(m) => format!("{} after", crate::format::minutes(m)),
                    None => "At a set time".to_owned(),
                };
                items.push(
                    row![icon(Icon::Bell).size(14.0).role(Role::Text3), text(label).size(12.0)].spacing(6.0).into(),
                );
            }
            if !e.description.is_empty() {
                items.push(divider().into());
                items.push(text(e.description.clone()).size(12.0).role(Role::Text2).wrap().selectable().into());
            }
            items.push(space(4.0));
            items.push(
                row![button("Edit…").on_press(Msg::Edit), button("Delete").destructive().on_press(Msg::Delete)]
                    .spacing(8.0)
                    .into(),
            );
        }
        None => {
            items.push(text("Agenda").mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3).into());
            let day: Vec<_> = k.occ.iter().filter(|o| o.first_day() <= k.cursor && o.last_day() >= k.cursor).collect();
            if day.is_empty() {
                items.push(text("Nothing on this day.").role(Role::Text3).size(12.0).into());
                items.push(
                    text("Double-click the grid, or drag across it, to add an event.")
                        .role(Role::Text3)
                        .size(11.0)
                        .wrap()
                        .into(),
                );
            }
            for o in day {
                let cal = &k.store.calendars[o.cal];
                let e = &cal.events[o.event];
                let when = if o.all_day || o.spans_days() {
                    "all day".to_owned()
                } else {
                    crate::format::time_range(o.start.time(), o.end.time(), k.h24())
                };
                items.push(
                    row![
                        Element::new(Swatch(cal.color), Vec::new()),
                        column![
                            text(e.summary.clone()).size(12.0).semibold(),
                            text(when).mono().size(10.5).role(Role::Text3)
                        ]
                        .spacing(1.0)
                        .width(Length::Fill),
                    ]
                    .spacing(8.0)
                    .align(Align::Start)
                    .into(),
                );
            }
        }
    }
    container(aizome_kit::widgets::layout::column(items).spacing(8.0).width(Length::Fill))
        .padding(16.0)
        .fill(Tone::Chrome)
        .edges(Edge::Line, Sides::LEFT)
        .width(280.0)
        .height(Length::Fill)
        .into()
}

fn main_view(k: &Koyomi) -> Element<Msg> {
    match k.view {
        View::Month => month::grid(k.month_model(), Msg::Grid),
        View::Year => mini::year(k.year_model(), Msg::Grid),
        View::Week | View::Day => {
            let (header, strip, grid) = k.week_models();
            let col: Element<Msg> =
                column![week::header(header, Msg::GoTo), week::strip(strip, Msg::Grid), week::grid(grid, Msg::Grid)]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
            if k.view == View::Day {
                row![col, details(k)].width(Length::Fill).height(Length::Fill).align(Align::Start).into()
            } else {
                col
            }
        }
    }
}

pub fn view(k: &Koyomi) -> Element<Msg> {
    let main = Element::new(Keys, vec![main_view(k)]).id("calendar").width(Length::Fill).height(Length::Fill);
    let body: Element<Msg> = if k.show_sidebar {
        sidebar_layout(sidebar(k), main)
            .first_size(k.state.sidebar.unwrap_or(212.0))
            .min_sizes(200.0, 420.0)
            .on_resize(Msg::SidebarWidth)
            .into()
    } else {
        main
    };
    column![toolbar_view(k), body, status(k)].width(Length::Fill).height(Length::Fill).into()
}

/// A text field's sheet: a title, a field, buttons.
pub fn name_sheet(title: &str, value: &str, ok: &str, on: fn(String) -> Msg, done: Msg, cancel: Msg) -> Element<Msg> {
    column![
        text(title.to_owned()).semibold().size(15.0),
        text_field(value).on_change(on).on_submit(done.clone()).id("name-field"),
        row![spacer(), button("Cancel").cancel().on_press(cancel), button(ok.to_owned()).default().on_press(done)]
            .spacing(8.0)
            .width(Length::Fill),
    ]
    .spacing(12.0)
    .padding(18.0)
    .width(380.0)
    .into()
}
