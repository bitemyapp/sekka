//! The pages' words, and what goes under them: the essentials of the
//! command key, Settei's doors, the footer.

use aizome_kit::prelude::*;
use aizome_kit::row;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::widget::Event;

use crate::app::{Annai, Msg, PAGES as COUNT};

pub struct Page {
    pub title: &'static str,
    pub body: &'static str,
}

pub const PAGES: [Page; COUNT] = [
    Page {
        title: "Welcome to Aizome",
        body: "藍染, aizome, is indigo dyeing: cloth dipped again and again, each dip adding depth. \
               Aizome is Sekka's desktop, made the same way. These pages show what's different, in a minute or two.",
    },
    Page {
        title: "Stack windows by their tabs",
        body: "Every window wears a tab only as wide as its title. Drag one window's tab onto another's and they \
               stack, their tabs side by side, one window showing at a time, whatever apps they belong to. \
               ⇧-drag a tab to slide it along the top edge, so windows that overlap stay easy to find.",
    },
    Page {
        title: "Menus where you are",
        body: "The app in front keeps its menus in the bar at the top. ❖-right-click anywhere and the whole \
               menu opens at the pointer, as on NeXT (right-click the empty desktop for the same). Drag a menu \
               by its title strip to tear it off into a palette that stays.",
    },
    Page {
        title: "Find anything with ❖Space",
        body: "The palette searches every command of the app in front, your windows, files, apps and settings \
               at once. Each command shows its keys, and when you've reached for one in the menus a few times, \
               the palette points them out.",
    },
    Page {
        title: "The Toolchest, Desks and the Deskbar",
        body: "The Toolchest at the left of the bar opens every app and place. Desks keep work apart: ⌃❖← and \
               ⌃❖→ move between them, ⌃❖↑ shows them all. The Deskbar lists apps and their windows, files \
               notifications under the app that sent them, and its Shelf holds what you drag there halfway.",
    },
    Page {
        title: "❖ is the command key",
        body: "Super does what ⌘ does on a Mac, in every app. Control stays free for terminals and Emacs, and apps \
               that expect Control are given it.",
    },
    Page {
        title: "Make it yours",
        body: "Night or Day, a focus colour, how large things are, your wallpaper and the rest: Settei \
               (Settings) arranges the room. Start here:",
    },
];

/// The essentials of the command key, two columns of them.
pub const ESSENTIALS: [(&[&str], &str); 10] = [
    (&["❖", "C"], "Copy"),
    (&["❖", "V"], "Paste"),
    (&["❖", "Z"], "Undo"),
    (&["❖", "W"], "Close the window"),
    (&["❖", "Q"], "Quit the app"),
    (&["❖", "Tab"], "Switch apps"),
    (&["❖", "Space"], "The palette"),
    (&["❖", "↩"], "A terminal"),
    (&["⌃", "❖", "Q"], "Lock the screen"),
    (&["⇧", "❖", "4"], "Screenshot an area"),
];

/// Settei's panes the last page opens.
pub const DOORS: [(Icon, &str, &str); 4] = [
    (Icon::Palette, "Appearance", "appearance"),
    (Icon::Monitor, "Displays", "displays"),
    (Icon::Wifi, "Network", "network"),
    (Icon::Keyboard, "Keyboard", "keyboard"),
];

fn keys(ks: &[&str]) -> Element<Msg> {
    aizome_kit::widgets::layout::row(ks.iter().map(|k| keycap(*k).into()).collect()).spacing(3.0).width(84.0).into()
}

/// The page's title, words, and what goes under them.
pub fn words(page: usize) -> Element<Msg> {
    let p = &PAGES[page];
    let mut parts: Vec<Element<Msg>> = vec![
        text(p.title).size(24.0).semibold().into(),
        text(p.body).size(14.0).role(Role::Text2).wrap().max_width(640.0).into(),
    ];
    match page {
        5 => {
            let col = |items: &[(&[&str], &str)]| -> Element<Msg> {
                aizome_kit::widgets::layout::column(
                    items
                        .iter()
                        .map(|(ks, what)| row![keys(ks), text(*what).role(Role::Text2)].spacing(8.0).into())
                        .collect(),
                )
                .spacing(2.0)
                .into()
            };
            parts.push(
                row![col(&ESSENTIALS[..5]), col(&ESSENTIALS[5..])].spacing(40.0).padding([6.0, 0.0, 0.0, 0.0]).into(),
            );
        }
        6 => {
            let doors = DOORS
                .iter()
                .map(|(icon, label, pane)| {
                    button(*label).icon(*icon).on_press(Msg::Settings(pane)).id(format!("door-{pane}")).into()
                })
                .collect();
            parts.push(aizome_kit::widgets::layout::row(doors).spacing(10.0).padding([6.0, 0.0, 0.0, 0.0]).into());
        }
        _ => {}
    }
    aizome_kit::widgets::layout::column(parts)
        .spacing(10.0)
        .padding([18.0, 60.0, 0.0, 60.0])
        .width(Fill)
        .height(Fill)
        .into()
}

/// Where the pages are: a square each, the one showing in the focus
/// colour; a click goes to one.
fn dots(page: usize) -> Element<Msg> {
    const STEP: f64 = 18.0;
    canvas(page, |c, b, page, theme| {
        let x0 = b.x0 + ((b.width() - STEP * COUNT as f64) / 2.0).round();
        let y = b.y0 + ((b.height() - 8.0) / 2.0).round();
        for i in 0..COUNT {
            let x = x0 + i as f64 * STEP + 5.0;
            let r = Rect::new(x, y, x + 8.0, y + 8.0);
            if i == *page {
                c.fill_rect(r, theme.focus());
            } else {
                aizome_kit::draw::border(c, r, theme.line2(), 1.0);
            }
        }
    })
    .on_event(|ev, size, _| match ev {
        Event::PointerDown { pos, button: PointerButton::Left, .. } => {
            let x0 = ((size.width - STEP * COUNT as f64) / 2.0).round();
            let i = ((pos.x - x0) / STEP).floor();
            (i >= 0.0 && (i as usize) < COUNT).then_some(Msg::Go(i as usize))
        }
        _ => None,
    })
    .width(STEP * COUNT as f64)
    .height(24.0)
    .label("Pages")
    .into()
}

pub fn footer(app: &Annai) -> Element<Msg> {
    let last = app.page + 1 == COUNT;
    container(
        row![
            checkbox("Show at login", app.state.show_at_login).on_toggle(Msg::ShowAtLogin).id("show-at-login"),
            spacer(),
            dots(app.page),
            spacer(),
            button("Back").on_press(Msg::Back).enabled(app.page > 0).width(84.0),
            button(if last { "Done" } else { "Next" }).default().on_press(Msg::Next).width(84.0).id("next"),
        ]
        .spacing(8.0),
    )
    .fill(Tone::Chrome)
    .edges(Edge::BevelLo, Sides::TOP)
    .padding([10.0, 16.0])
    .width(Fill)
    .into()
}
