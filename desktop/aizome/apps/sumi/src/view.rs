//! What a Sumi window shows: the board's toolbar (the language pop-up, B,
//! I and </>, the status), the find bar when it's up, and the editor; and
//! the Link and Go to Line sheets.

use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::element::MsgFn;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, KeyEvent, NamedKey};
use aizome_kit::widget::{Event, EventCx, LayoutCx, Status, Widget};
use aizome_kit::{Constraints, Highlighter, column, row};

use crate::app::{Command, Msg, Sheet};
use crate::doc::Doc;
use crate::format::Inline;
use crate::settings::Settings;
use crate::syntax::{self, Language};

/// The highlighter the editor calls for the lines it shows: the
/// document's line states, worked out as far as they're needed.
pub fn highlighter(doc: &Doc) -> Highlighter {
    let cache = doc.highlight.clone();
    let rope = doc.content.rope().clone();
    Arc::new(move |line: usize, text: &str| {
        let Ok(mut h) = cache.lock() else { return Vec::new() };
        let tokens = h.tokens(line, text, &rope);
        if h.incomplete && !h.scheduled {
            h.scheduled = true;
            if let Some(n) = &h.notify {
                n();
            }
        }
        syntax::line_styles(&tokens)
    })
}

fn doc_toolbar(doc: &Doc) -> Element<Msg> {
    let w = doc.window;
    let names: Vec<&str> = Language::ALL.iter().map(|l| l.name()).collect();
    let md = doc.language == Language::Markdown;
    let format = |label: &'static str, cmd: Command, tip: &'static str, name: &'static str| {
        button(label).toolbar().enabled(md).on_press(Msg::On(w, cmd)).tooltip(tip).label(name)
    };
    let mut items: Vec<Element<Msg>> = vec![
        dropdown(names, Some(doc.language.index()))
            .compact()
            .on_select(move |i| Msg::On(w, Command::SetLanguage(Language::ALL[i.min(Language::ALL.len() - 1)])))
            .label("Language")
            .id("language")
            .into(),
        toolbar_separator(),
        format("B", Command::Inline(Inline::Bold), "Bold (❖B)", "Bold").weight(800.0).into(),
        format("I", Command::Inline(Inline::Italic), "Italic (❖I)", "Italic").italic().weight(600.0).into(),
        format("</>", Command::Inline(Inline::Code), "Code (❖E)", "Code").mono().into(),
        spacer().into(),
    ];
    if let Some(n) = &doc.notice {
        items.push(text(n.clone()).size(11.0).role(Role::Warning).into());
        items.push(space(12.0));
    }
    if doc.loading.is_some() {
        items.push(status_text("Opening…"));
    } else {
        items.push(status_text(doc.status()));
    }
    toolbar(items)
}

fn find_bar(doc: &Doc) -> Element<Msg> {
    let w = doc.window;
    let f = &doc.find;
    let on = move |c: Command| Msg::On(w, c);
    let toggle = |label: &'static str, selected: bool, cmd: Command, tip: &'static str| {
        button(label).toolbar().selected(selected).on_press(on(cmd)).tooltip(tip).label(tip)
    };
    let summary = f.summary().unwrap_or_default();
    let summary_role = if f.error.is_some() { Role::Danger } else { Role::Text3 };
    let first = row![
        text_field(&f.query.text)
            .placeholder("Find")
            .mono()
            .on_change(move |s| Msg::On(w, Command::FindQuery(s)))
            .on_submit(on(Command::FindNext))
            .id("find")
            .width(Length::Fill),
        toggle("Aa", f.query.case, Command::FindCase, "Match Case"),
        toggle("W", f.query.word, Command::FindWord, "Whole Words"),
        toggle(".*", f.query.regex, Command::FindRegex, "Regular Expression").mono(),
        text(summary).mono().tabular().size(11.0).role(summary_role).min_width(84.0).max_width(220.0),
        icon_button(Icon::ChevronLeft)
            .toolbar()
            .on_press(on(Command::FindPrevious))
            .tooltip("Find Previous (⇧❖G)")
            .label("Find Previous"),
        icon_button(Icon::ChevronRight)
            .toolbar()
            .on_press(on(Command::FindNext))
            .tooltip("Find Next (❖G)")
            .label("Find Next"),
        button("Done").compact().on_press(on(Command::FindHide)),
    ]
    .spacing(4.0)
    .width(Length::Fill);
    let mut rows: Vec<Element<Msg>> = vec![first.into()];
    if f.replace {
        rows.push(
            row![
                text_field(&f.replacement)
                    .placeholder(if f.query.regex { "Replace ($1 for groups)" } else { "Replace" })
                    .mono()
                    .on_change(move |s| Msg::On(w, Command::FindReplacement(s)))
                    .on_submit(on(Command::ReplaceOne))
                    .id("replace")
                    .width(Length::Fill),
                button("Replace").compact().enabled(!f.matches.is_empty()).on_press(on(Command::ReplaceOne)),
                button("All")
                    .compact()
                    .enabled(!f.matches.is_empty())
                    .on_press(on(Command::ReplaceAll))
                    .tooltip("Replace All"),
            ]
            .spacing(4.0)
            .width(Length::Fill)
            .into(),
        );
    }
    container(aizome_kit::column(rows).spacing(4.0).width(Length::Fill))
        .padding([4.0, 8.0])
        .fill(Tone::Chrome)
        .edges(Edge::BevelLo, Sides::BOTTOM)
        .width(Length::Fill)
        .key("find-bar")
        .into()
}

/// A document's window.
pub fn window(doc: &Doc, settings: &Settings) -> Element<Msg> {
    let w = doc.window;
    // ❖-click on a link opens it.
    let rope = doc.content.rope().clone();
    let md = doc.language == Language::Markdown;
    let editor = text_editor(&doc.content)
        .on_action(move |a| Msg::Edit(w, a))
        .on_click(move |at, mods| {
            if !md || !mods.super_key() || at >= rope.len_chars() {
                return None;
            }
            let line = rope.char_to_line(at);
            let text = crate::syntax::cache::line_text(&rope, line);
            let col = at - rope.line_to_char(line);
            let byte = text.char_indices().nth(col).map_or(text.len(), |(b, _)| b);
            crate::syntax::markdown::link_at(&text, byte).map(Msg::OpenUrl)
        })
        .on_find(move |f| Msg::Find(w, f))
        .line_numbers(settings.line_numbers)
        .wrap(settings.wrap)
        .mono(!settings.proportional)
        .size(settings.size)
        .highlighter(highlighter(doc), doc.styles)
        .marks(doc.find.matches.clone(), if doc.find.shown { doc.find.current } else { None })
        .id("editor")
        .key("editor");
    let mut parts: Vec<Element<Msg>> = vec![doc_toolbar(doc)];
    if doc.find.shown {
        parts.push(find_bar(doc));
    }
    parts.push(editor.into());
    let body = aizome_kit::column(parts).width(Length::Fill).height(Length::Fill);
    let find_shown = doc.find.shown;
    keys(body, move |k| key_command(k, find_shown).map(|c| Msg::On(w, c)))
}

/// Keys the menus can't name: ⇧❖' typed as ⇧❖" (Quote), ❖+ (Zoom In),
/// Escape (the find bar away).
fn key_command(k: &KeyEvent, find_shown: bool) -> Option<Command> {
    let m = k.modifiers;
    match &k.key {
        Key::Named(NamedKey::Escape) if find_shown && !m.super_key() && !m.alt() && !m.control() => {
            Some(Command::FindHide)
        }
        Key::Character(c) if m.super_key() && !m.control() => match c.as_str() {
            "\"" if m.shift() && !m.alt() => Some(Command::Quote),
            "+" if !m.alt() => Some(Command::ZoomIn),
            "_" if m.shift() && !m.alt() => Some(Command::ZoomOut),
            _ => None,
        },
        _ => None,
    }
}

/// A sheet's view.
pub fn sheet(key: WindowKey, sheet: &Sheet) -> Element<Msg> {
    let _ = key;
    match sheet {
        Sheet::Link { text: t, url, .. } => column![
            text("Link").semibold().size(15.0),
            row![
                text("Text").role(Role::Text2).width(40.0),
                text_field(t)
                    .on_change(Msg::LinkText)
                    .on_submit(Msg::LinkDone(true))
                    .id("link-text")
                    .width(Length::Fill)
            ]
            .spacing(8.0)
            .width(Length::Fill),
            row![
                text("URL").role(Role::Text2).width(40.0),
                text_field(url)
                    .mono()
                    .placeholder("https://")
                    .on_change(Msg::LinkUrl)
                    .on_submit(Msg::LinkDone(true))
                    .id("link-url")
                    .width(Length::Fill)
            ]
            .spacing(8.0)
            .width(Length::Fill),
            row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::LinkDone(false)),
                button("Insert Link").default().on_press(Msg::LinkDone(true))
            ]
            .spacing(8.0)
            .width(Length::Fill),
        ]
        .spacing(12.0)
        .padding(16.0)
        .width(380.0)
        .into(),
        Sheet::Goto { line, max, .. } => column![
            text("Go to Line").semibold().size(15.0),
            text_field(line)
                .mono()
                .placeholder(format!("1–{max}, or line:column"))
                .on_change(Msg::GotoText)
                .on_submit(Msg::GotoDone(true))
                .id("goto-line")
                .width(Length::Fill),
            row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::GotoDone(false)),
                button("Go").default().on_press(Msg::GotoDone(true))
            ]
            .spacing(8.0)
            .width(Length::Fill),
        ]
        .spacing(12.0)
        .padding(16.0)
        .width(300.0)
        .into(),
    }
}

/// A widget round the window that hears keys nothing in it took.
pub fn keys<Msg: Clone + Send + 'static>(
    child: impl Into<Element<Msg>>,
    on_key: impl Fn(&KeyEvent) -> Option<Msg> + 'static,
) -> Element<Msg> {
    Element::new(Catcher { on_key: Rc::new(on_key) }, vec![child.into()]).width(Length::Fill).height(Length::Fill)
}

type OnKey<Msg> = Rc<dyn Fn(&KeyEvent) -> Option<Msg>>;

struct Catcher<Msg> {
    on_key: OnKey<Msg>,
}

impl<Msg: Clone + Send + 'static> Widget for Catcher<Msg> {
    fn name(&self) -> &'static str {
        "sumi-keys"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let mut size = Size::ZERO;
        for i in 0..cx.child_count() {
            let s = cx.layout_child(i, bc);
            cx.place_child(i, Point::ZERO);
            size = Size::new(size.width.max(s.width), size.height.max(s.height));
        }
        bc.constrain(size)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::Key(k) = event
            && k.pressed
            && let Some(msg) = (self.on_key)(k)
        {
            cx.emit(&MsgFn::new(msg));
            return Status::Handled;
        }
        Status::Ignored
    }
}
