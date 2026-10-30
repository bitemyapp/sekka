//! The window: the toolbar (the boot, the level, Live, unlocking,
//! search), the sidebar, the banner that offers to unlock the system's
//! logs, the chips and suggestions, the timeline, the messages and the
//! details of the one selected, and the status line.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::{column as column_of, row as row_of};
use aizome_kit::{column, row};

use crate::app::{Access, Msg, Nisshi, Unlock};
use crate::entry::Priority;
use crate::filter::Scope;
use crate::query::Boot;

pub fn window(n: &Nisshi) -> Element<Msg> {
    let mut middle: Vec<Element<Msg>> = Vec::new();
    if let Some(b) = banner(n) {
        middle.push(b);
    }
    if let Some(c) = chips(n) {
        middle.push(c);
    }
    middle.push(super::timeline::view(n));
    let table = super::table::view(n);
    let body: Element<Msg> = if n.details {
        split(table, super::detail::view(n)).vertical().ratio(0.64).min_sizes(140.0, 120.0).into()
    } else {
        table
    };
    middle.push(body);
    let content = column_of(middle).width(Length::Fill).height(Length::Fill);
    let main: Element<Msg> = if n.sidebar {
        sidebar_layout(super::sidebar::view(n), content).first_size(214.0).min_sizes(170.0, 520.0).into()
    } else {
        content.into()
    };
    column![toolbar(n), main, status(n)].width(Length::Fill).height(Length::Fill).into()
}

/// The boots, as the toolbar's pop-up lists them: this one, the ones
/// before by when they began, and every boot.
pub fn boot_names(n: &Nisshi) -> Vec<String> {
    let mut names: Vec<String> = n
        .boots
        .iter()
        .map(|b| match b.index {
            0 => "This Boot".to_owned(),
            -1 => format!("Previous Boot · {}", n.clock.short(b.first_entry)),
            _ => format!("Boot of {}", n.clock.short(b.first_entry)),
        })
        .collect();
    if names.is_empty() {
        names.push("This Boot".into());
    }
    names.push("All Boots".into());
    names
}

/// Which of [`boot_names`] is shown.
pub fn boot_shown(n: &Nisshi) -> usize {
    let all = n.boots.len().max(1);
    match &n.boot {
        Boot::Current => n.boots.iter().position(|b| b.index == 0).unwrap_or(0),
        Boot::Offset(o) => n.boots.iter().position(|b| b.index == *o).unwrap_or(0),
        Boot::Id(id) => n.boots.iter().position(|b| &b.boot_id == id).unwrap_or(0),
        Boot::All => all,
    }
}

pub const LEVELS: [(&str, Option<Priority>); 5] = [
    ("All Levels", None),
    ("Notices & Worse", Some(Priority::Notice)),
    ("Warnings & Worse", Some(Priority::Warning)),
    ("Errors & Worse", Some(Priority::Error)),
    ("Critical & Worse", Some(Priority::Critical)),
];

fn toolbar(n: &Nisshi) -> Element<Msg> {
    let boots = boot_names(n);
    let count = boots.len();
    let level = LEVELS.iter().position(|(_, l)| *l == n.filter.level).unwrap_or(0);
    let journal = !n.in_file();
    let live_possible = journal && matches!(n.boot, Boot::Current | Boot::All);
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::PanelLeft)
            .toolbar()
            .selected(n.sidebar)
            .on_press(Msg::ToggleSidebar)
            .tooltip("Sidebar (⌃❖S)")
            .label("Sidebar")
            .into(),
        toolbar_separator(),
    ];
    if journal {
        items.push(
            dropdown(boots, Some(boot_shown(n)))
                .compact()
                .on_select(move |i| Msg::Boot(if i + 1 == count { None } else { Some(i) }))
                .id("boot")
                .label("Boot")
                .tooltip("Which boot's messages")
                .into(),
        );
    }
    items.push(
        dropdown(LEVELS.iter().map(|(l, _)| *l), Some(level))
            .compact()
            .on_select(|i| Msg::Level(LEVELS[i.min(LEVELS.len() - 1)].1))
            .id("level")
            .label("Level")
            .tooltip("The least severe level shown")
            .into(),
    );
    // Which end the newest message is at: said, not left to guess.
    items.push(
        button(if n.newest_first { "Newest First" } else { "Oldest First" })
            .icon(if n.newest_first { Icon::ArrowDown } else { Icon::ArrowUp })
            .toolbar()
            .on_press(Msg::NewestFirst(!n.newest_first))
            .tooltip(if n.newest_first {
                "The newest message is at the top: press for the oldest first"
            } else {
                "The oldest message is at the top: press for the newest first"
            })
            .id("order")
            .into(),
    );
    if journal {
        items.push(
            button(if n.live_on && live_possible { "Live" } else { "Paused" })
                .icon(if n.live_on && live_possible { Icon::RadioTower } else { Icon::Pause })
                .toolbar()
                .selected(n.live_on && live_possible)
                .on_press_maybe(live_possible.then_some(Msg::Live(!n.live_on)))
                .tooltip(if live_possible {
                    "New messages come in as they're written (❖L)"
                } else {
                    "Only the running boot's messages come in live"
                })
                .id("live")
                .into(),
        );
    }
    items.push(spacer().into());
    match (&n.unlock, n.access) {
        (Unlock::Unlocked, _) => items.push(
            button("Unlocked")
                .icon(Icon::LockOpen)
                .toolbar()
                .on_press(Msg::Lock)
                .tooltip("Reading the system's logs as an administrator: lock them again")
                .id("lock")
                .into(),
        ),
        (Unlock::Asking, _) => {
            items.push(row![progress_indeterminate().width(48.0), status_text("Unlocking…")].spacing(6.0).into())
        }
        _ => {}
    }
    items.push(
        search_field(&n.search_text)
            .placeholder("Search  (app:  level:  since:  /regex/)")
            .on_change(Msg::Search)
            .on_submit(Msg::SubmitSearch)
            .id("search")
            .width(300.0)
            .into(),
    );
    aizome_kit::toolbar(items)
}

/// Why some messages aren't shown, and how to see them.
fn banner(n: &Nisshi) -> Option<Element<Msg>> {
    let (glyph, words, action): (Icon, String, Option<Element<Msg>>) = match (&n.unlock, n.access) {
        (Unlock::Locked | Unlock::Refused(_), _)
            if n.in_file() && n.files.iter().any(|f| Scope::File(f.path.clone()) == n.filter.scope && !f.readable) =>
        {
            (
                Icon::Lock,
                format!("Only an administrator can read {}.", n.filter.scope.title()),
                Some(button("Unlock…").compact().icon(Icon::Lock).on_press(Msg::Unlock).id("banner-unlock").into()),
            )
        }
        (Unlock::Refused(why), _) => (
            Icon::CircleAlert,
            format!("{why} You're seeing only your own session's messages."),
            Some(button("Try Again").compact().on_press(Msg::Unlock).into()),
        ),
        (Unlock::Locked, Access::Own) if !n.in_file() => (
            Icon::Lock,
            "You're seeing your own session's messages. The system's, and other people's, need an administrator's password.".into(),
            Some(button("Unlock…").compact().icon(Icon::Lock).on_press(Msg::Unlock).id("banner-unlock").into()),
        ),
        _ => return None,
    };
    let mut parts: Vec<Element<Msg>> = vec![
        icon(glyph).size(15.0).role(Role::Warning).into(),
        text(words).role(Role::Text2).max_lines(2).width(Length::Fill).into(),
    ];
    if let Some(a) = action {
        parts.push(a);
    }
    Some(
        container(row_of(parts).spacing(10.0).width(Length::Fill))
            .padding([7.0, 12.0])
            .fill(Tone::Raised)
            .edges(Edge::Line, Sides::BOTTOM)
            .width(Length::Fill)
            .id("banner")
            .into(),
    )
}

/// The filters in force, then what Nisshi suggests.
fn chips(n: &Nisshi) -> Option<Element<Msg>> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    for (i, c) in n.filter.chips.iter().enumerate() {
        items.push(
            button(c.label(&n.clock))
                .icon(Icon::X)
                .compact()
                .selected(true)
                .on_press(Msg::RemoveChip(i))
                .tooltip("Remove this filter")
                .key(format!("chip {}", c.key()))
                .into(),
        );
    }
    if let Some(g) = &n.journal_search {
        let _ = g;
        items.push(
            button("Searched the journal")
                .icon(Icon::X)
                .compact()
                .selected(true)
                .on_press(Msg::StopSearchingJournal)
                .tooltip("The journal was searched for what's typed: read all of the boot again")
                .key("journal search")
                .into(),
        );
    }
    if n.filter.is_narrowed() || n.journal_search.is_some() {
        items.push(
            button("Clear")
                .plain()
                .compact()
                .on_press(Msg::ClearFilters)
                .tooltip("Show everything (❖⌫)")
                .key("clear")
                .into(),
        );
    }
    if !n.suggestions.is_empty() {
        if !items.is_empty() {
            items.push(divider().vertical().height(18.0).into());
        }
        let completing = crate::search::completing(&n.search_text).is_some();
        let title = if completing {
            "Complete"
        } else if n.selection().is_some() {
            "For this message"
        } else {
            "Notable"
        };
        items.push(
            row![icon(Icon::ListFilter).size(13.0).role(Role::Accent), text(title).size(11.5).role(Role::Text3)]
                .spacing(4.0)
                .key("suggested")
                .into(),
        );
        for (i, s) in n.suggestions.iter().enumerate() {
            let label = match s.count {
                Some(c) => format!("{} · {}", s.label, crate::number(c)),
                None => s.label.clone(),
            };
            items.push(
                button(label)
                    .icon(s.icon)
                    .compact()
                    .on_press(Msg::Suggest(i))
                    .tooltip(s.why.clone())
                    .key(format!("suggestion {i} {}", s.label))
                    .id(format!("suggestion-{i}"))
                    .into(),
            );
        }
    }
    if items.is_empty() {
        return None;
    }
    let strip = row_of(items).spacing(6.0).padding([5.0, 10.0]);
    Some(
        container(scroll(strip).horizontal().width(Length::Fill).height(Length::Shrink))
            .fill(Tone::Chrome)
            .edges(Edge::Line, Sides::BOTTOM)
            .width(Length::Fill)
            .id("chips")
            .into(),
    )
}

fn status(n: &Nisshi) -> Element<Msg> {
    let v = &n.view;
    let store = n.shown();
    let shown = v.rows.len();
    let total = store.entries.len();
    let mut left = if n.loading && total == 0 {
        "Reading…".to_owned()
    } else if shown == total {
        crate::count(shown, "message", "messages")
    } else {
        format!("{} of {}", crate::number(shown), crate::count(total, "message", "messages"))
    };
    if v.errors > 0 {
        left.push_str(&format!(" · {}", crate::count(v.errors, "error", "errors")));
    }
    if v.warnings > 0 {
        left.push_str(&format!(" · {}", crate::count(v.warnings, "warning", "warnings")));
    }
    let mut items: Vec<Element<Msg>> = Vec::new();
    match &n.notice {
        Some((words, error)) => {
            items.push(
                text(words.clone())
                    .size(11.5)
                    .role(if *error { Role::Danger } else { Role::Text2 })
                    .width(Length::Fill)
                    .into(),
            );
            items.push(icon_button(Icon::X).plain().compact().on_press(Msg::DismissNotice).tooltip("Dismiss").into());
        }
        None => items.push(text(left).size(11.5).tabular().role(Role::Text3).width(Length::Fill).id("count").into()),
    }
    if n.loading && total > 0 {
        items.push(
            row![progress_indeterminate().width(60.0), status_text(format!("Reading… {}", crate::number(total)))]
                .spacing(6.0)
                .into(),
        );
    }
    if !n.in_file() && !store.complete && !n.loading && total > 0 {
        items.push(
            button("Read Earlier")
                .compact()
                .plain()
                .icon(Icon::ArrowUp)
                .on_press(Msg::LoadOlder)
                .tooltip("Read the boot's earlier messages")
                .id("older")
                .into(),
        );
    }
    if n.can_search_journal() {
        items.push(
            button("Search the Whole Boot")
                .compact()
                .plain()
                .icon(Icon::Search)
                .on_press(Msg::SearchJournal)
                .tooltip("Only the newest messages are read: ask the journal for all that match (Return)")
                .id("search-journal")
                .into(),
        );
    }
    if !n.following || !n.selected.is_empty() {
        let words = if n.unseen > 0 { format!("{} new", crate::number(n.unseen)) } else { "Newest".into() };
        if n.live_on && !n.in_file() && matches!(n.boot, Boot::Current | Boot::All) {
            items.push(
                button(words)
                    .compact()
                    .plain()
                    .icon(if n.newest_first { Icon::ArrowUp } else { Icon::ArrowDown })
                    .on_press(Msg::JumpToNewest)
                    .tooltip("Go to the newest message")
                    .id("newest")
                    .into(),
            );
        }
    }
    let right = if n.in_file() {
        n.filter.scope.title()
    } else {
        let (Some(first), Some(last)) = (store.first_time(), store.last_time()) else {
            return aizome_kit::status_bar(items);
        };
        let which = match n.boot {
            Boot::Current => "This boot".to_owned(),
            Boot::All => "All boots".to_owned(),
            _ => "Earlier boot".to_owned(),
        };
        let tail = if !store.complete { " (the newest)" } else { "" };
        format!(
            "{which}{tail}: {} – {} · {}",
            n.clock.short(first),
            n.clock.short(last),
            crate::time::Clock::span(last - first)
        )
    };
    items.push(status_text(right));
    aizome_kit::status_bar(items)
}
