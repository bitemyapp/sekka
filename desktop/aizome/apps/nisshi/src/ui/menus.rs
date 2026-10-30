//! Nisshi's menus: File (export), Edit (copy, find), View (the places,
//! the level, what shows), Journal (the boots, reading earlier, unlocking)
//! and Message (what to do with the one selected).

use aizome_kit::prelude::*;

use crate::app::{Access, Msg, Nisshi, Unlock};
use crate::filter::Scope;
use crate::query::Boot;

pub fn menus(n: &Nisshi) -> MenuBar<Msg> {
    let file = Menu::new("File")
        .item(
            MenuItem::new("Export Messages Shown…", Msg::Export)
                .shortcut(Shortcut::cmd('E').shift())
                .enabled(!n.view.rows.is_empty()),
        )
        .separator()
        .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
    let edit = Menu::new("Edit")
        .item(MenuItem::role_item("Undo", MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
        .item(MenuItem::role_item("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
        .separator()
        .item(MenuItem::role_item("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
        .item(MenuItem::new("Copy", Msg::Copy).role(MenuRole::Copy).shortcut(Shortcut::cmd('C')))
        .item(
            MenuItem::new("Copy Message Only", Msg::CopyMessage)
                .shortcut(Shortcut::cmd('C').alt())
                .enabled(n.selection().is_some()),
        )
        .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
        .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
        .separator()
        .item(MenuItem::new("Find…", Msg::FocusSearch).role(MenuRole::Find).shortcut(Shortcut::cmd('F')))
        .item(
            MenuItem::new("Search the Whole Boot", Msg::SearchJournal)
                .shortcut(Shortcut::cmd('F').shift())
                .enabled(n.can_search_journal()),
        )
        .item(
            MenuItem::new("Clear Filters", Msg::ClearFilters)
                .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace))
                .enabled(n.filter.is_narrowed() || n.journal_search.is_some()),
        );
    let mut view = Menu::new("View");
    for (i, s) in Scope::JOURNAL.iter().enumerate() {
        view = view.item(
            MenuItem::new(s.title(), Msg::Scope(s.clone()))
                .checked(n.filter.scope == *s)
                .shortcut(Shortcut::cmd(char::from(b'1' + i as u8))),
        );
    }
    let levels = crate::ui::view::LEVELS.iter().fold(Menu::new("Level"), |m, (label, l)| {
        m.item(MenuItem::new(*label, Msg::Level(*l)).checked(n.filter.level == *l))
    });
    let view = view
        .separator()
        .submenu(levels)
        .separator()
        .item(
            MenuItem::new(if n.sidebar { "Hide Sidebar" } else { "Show Sidebar" }, Msg::ToggleSidebar)
                .shortcut(Shortcut::cmd('S').ctrl()),
        )
        .item(
            MenuItem::new(if n.details { "Hide Details" } else { "Show Details" }, Msg::ToggleDetails)
                .shortcut(Shortcut::cmd('I')),
        )
        .item(MenuItem::new("Newest First", Msg::NewestFirst(!n.newest_first)).checked(n.newest_first))
        .separator()
        .item(MenuItem::new("Go to the Newest", Msg::JumpToNewest).shortcut(Shortcut::cmd_key(if n.newest_first {
            ShortcutKey::Up
        } else {
            ShortcutKey::Down
        })));
    let live_possible = !n.in_file() && matches!(n.boot, Boot::Current | Boot::All);
    let names = crate::ui::view::boot_names(n);
    let count = names.len();
    let shown = crate::ui::view::boot_shown(n);
    let boots = names.into_iter().enumerate().fold(Menu::new("Boot"), |m, (i, name)| {
        m.item(
            MenuItem::new(name, Msg::Boot(if i + 1 == count { None } else { Some(i) }))
                .checked(i == shown && !n.in_file()),
        )
    });
    let mut journal = Menu::new("Journal")
        .submenu(boots)
        .item(
            MenuItem::new("Live", Msg::Live(!n.live_on))
                .checked(n.live_on && live_possible)
                .enabled(live_possible)
                .shortcut(Shortcut::cmd('L')),
        )
        .item(
            MenuItem::new("Read Earlier Messages", Msg::LoadOlder)
                .enabled(!n.in_file() && !n.store.complete && !n.loading),
        )
        .item(MenuItem::new("Read Again", Msg::Reload).shortcut(Shortcut::cmd('R')))
        .separator();
    journal = match (&n.unlock, n.access) {
        (Unlock::Unlocked, _) => journal.item(MenuItem::new("Lock the System's Logs", Msg::Lock)),
        (Unlock::Asking, _) => journal.item(MenuItem::new("Unlocking…", Msg::Unlock).enabled(false)),
        (_, Access::System) => journal.item(MenuItem::new("Unlock the System's Logs…", Msg::Unlock)),
        _ => journal.item(MenuItem::new("Unlock the System's Logs…", Msg::Unlock).shortcut(Shortcut::cmd('U').shift())),
    };
    let sel = n.selection();
    let mut message = Menu::new("Message")
        .item(
            MenuItem::new("Messages Like This", Msg::FilterField("MESSAGE".into(), String::new()))
                .enabled(sel.is_some()),
        )
        .item(
            MenuItem::new("Around This Time", sel.map_or(Msg::JumpToNewest, |e| Msg::Context(e.seq)))
                .enabled(sel.is_some())
                .shortcut(Shortcut::cmd('T').alt()),
        );
    if let Some(e) = sel {
        message = message.item(MenuItem::new(format!("Only {}", e.source), Msg::OnlySource(e.source.clone())));
        if e.pid > 0 {
            message = message
                .separator()
                .item(MenuItem::new(format!("Show Process {} in Activity", e.pid), Msg::OpenActivity(e.pid)));
        }
    }
    MenuBar::standard(&n.info())
        .replace("Edit", edit)
        .with_file_menu(file)
        .with_menu(view)
        .with_menu(journal)
        .with_menu(message)
}
