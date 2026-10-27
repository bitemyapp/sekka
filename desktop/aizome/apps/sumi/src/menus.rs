//! Sumi's menus: the app menu, File, Edit (with Find ▸), Format as the
//! board has it, View, Window and Help.
//!
//! ❖E is Code, as on the board's Format menu, so Use Selection for Find
//! (❖E on a Mac) moves to ⌥❖E, beside Find and Replace's ⌥❖F.

use aizome_kit::menu::{EDIT, HELP, WINDOW};
use aizome_kit::prelude::*;

use crate::app::{Command, Msg, Sumi};
use crate::files::{Encoding, LineEnding};
use crate::format::{Inline, List};
use crate::syntax::Language;

fn cmd(label: &str, c: Command) -> MenuItem<Msg> {
    MenuItem::new(label, Msg::Cmd(c))
}

fn recent_label(p: &std::path::Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let dir = p.parent().map(|d| {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        match home.as_deref().and_then(|h| d.strip_prefix(h).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Some(rest) => format!("~/{}", rest.display()),
            None => d.display().to_string(),
        }
    });
    match dir {
        Some(d) => format!("{name} — {d}"),
        None => name,
    }
}

pub fn menus(app: &Sumi) -> MenuBar<Msg> {
    let info = aizome_kit::Program::info(app);
    let doc = app.focused_doc();
    let has_doc = doc.is_some();
    let dirty = doc.is_some_and(|d| d.dirty());
    let has_path = doc.is_some_and(|d| d.path.is_some());
    let md = doc.is_some_and(|d| d.language == crate::syntax::Language::Markdown);
    let s = &app.settings;
    let name = info.name.clone();
    let role = |label: &str, r: MenuRole| MenuItem::<Msg>::role_item(label, r);

    let app_menu = Menu::new(name.clone())
        .item(role(&format!("About {name}"), MenuRole::About))
        .separator()
        .submenu(Menu::new("Services").item(role("No Services Apply", MenuRole::Services).enabled(false)))
        .separator()
        .item(role(&format!("Hide {name}"), MenuRole::Hide).shortcut(Shortcut::cmd('H')))
        .item(role("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
        .separator()
        // Quit keeps every document, saved or not, for next time.
        .item(MenuItem::new(format!("Quit {name}"), Msg::Quit).shortcut(Shortcut::cmd('Q')));

    let mut recent = Menu::new("Open Recent");
    for p in app.recent.iter().take(12) {
        recent.push(MenuItem::new(recent_label(p), Msg::OpenRecent(p.clone())));
    }
    let recent = if app.recent.is_empty() {
        recent.item(MenuItem::new("No Recent Files", Msg::Nothing).enabled(false))
    } else {
        recent.separator().item(MenuItem::new("Clear Menu", Msg::ClearRecent))
    };
    let current_enc = doc.map(|d| d.encoding);
    let current_eol = doc.map(|d| d.line_ending);
    let mut encodings = Menu::new("Encoding");
    let mut reopen = Menu::new("Reopen with Encoding");
    for e in Encoding::ALL {
        encodings.push(cmd(e.name(), Command::SetEncoding(e)).checked(current_enc == Some(e)).enabled(has_doc));
        reopen.push(cmd(e.name(), Command::Reopen(e)).enabled(has_path));
    }
    let mut endings = Menu::new("Line Endings");
    for e in LineEnding::ALL {
        endings.push(cmd(e.name(), Command::SetLineEnding(e)).checked(current_eol == Some(e)).enabled(has_doc));
    }
    let file = Menu::new("File")
        .item(MenuItem::new("New", Msg::New).shortcut(Shortcut::cmd('N')))
        .item(MenuItem::new("Open…", Msg::Open).shortcut(Shortcut::cmd('O')))
        .submenu(recent)
        .separator()
        .item(cmd("Save", Command::Save).shortcut(Shortcut::cmd('S')).enabled(has_doc))
        .item(cmd("Save As…", Command::SaveAs).shortcut(Shortcut::cmd('S').shift()).enabled(has_doc))
        .item(cmd("Revert to Saved", Command::Revert).enabled(dirty && has_path))
        .separator()
        .submenu(encodings)
        .submenu(endings)
        .submenu(reopen)
        .separator()
        .item(cmd("Close", Command::Close).shortcut(Shortcut::cmd('W')).enabled(has_doc));

    // Find's items go to the editor when it has the keyboard (its
    // FindRequests), and come as messages from the find bar's fields.
    let find = Menu::new("Find")
        .item(cmd("Find…", Command::FindShow).role(MenuRole::Find).shortcut(Shortcut::cmd('F')))
        .item(cmd("Find and Replace…", Command::ReplaceShow).role(MenuRole::Replace).shortcut(Shortcut::cmd('F').alt()))
        .item(cmd("Find Next", Command::FindNext).role(MenuRole::FindNext).shortcut(Shortcut::cmd('G')))
        .item(
            cmd("Find Previous", Command::FindPrevious)
                .role(MenuRole::FindPrevious)
                .shortcut(Shortcut::cmd('G').shift()),
        )
        .item(
            cmd("Use Selection for Find", Command::UseSelection)
                .role(MenuRole::UseSelectionForFind)
                .shortcut(Shortcut::cmd('E').alt()),
        );
    let edit = Menu::new(EDIT)
        .item(role("Undo", MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
        .item(role("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
        .separator()
        .item(role("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
        .item(role("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
        .item(role("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
        .item(role("Paste as Plain Text", MenuRole::PasteAsPlainText).shortcut(Shortcut::cmd('V').shift().alt()))
        .item(role("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
        .separator()
        .submenu(find)
        .item(cmd("Go to Line…", Command::GotoLine).shortcut(Shortcut::cmd('L')).enabled(has_doc));

    let f = |label: &str, c: Command| cmd(label, c).enabled(md);
    let font = Menu::new("Font")
        .item(MenuItem::new("IBM Plex Mono", Msg::Font(false)).checked(!s.proportional))
        .item(MenuItem::new("Archivo", Msg::Font(true)).checked(s.proportional))
        .separator()
        .item(f("Strikethrough", Command::Inline(Inline::Strike)).shortcut(Shortcut::cmd('X').shift()))
        .separator()
        .item(MenuItem::new("Bigger", Msg::Zoom(Some(true))))
        .item(MenuItem::new("Smaller", Msg::Zoom(Some(false))));
    let paragraph = Menu::new("Paragraph")
        .item(f("Bulleted List", Command::List(List::Bullet)))
        .item(f("Numbered List", Command::List(List::Numbered)))
        .item(f("Task List", Command::List(List::Task)))
        .separator()
        .item(f("Code Block", Command::CodeBlock))
        .item(f("Horizontal Rule", Command::Rule))
        .separator()
        .item(cmd("Shift Right", Command::ShiftRight).shortcut(Shortcut::cmd(']')).enabled(has_doc))
        .item(cmd("Shift Left", Command::ShiftLeft).shortcut(Shortcut::cmd('[')).enabled(has_doc));
    let heading = Menu::new("Heading")
        .item(f("Heading 1", Command::Heading(1)).shortcut(Shortcut::cmd('1').alt()))
        .item(f("Heading 2", Command::Heading(2)).shortcut(Shortcut::cmd('2').alt()))
        .item(f("Heading 3", Command::Heading(3)).shortcut(Shortcut::cmd('3').alt()))
        .item(f("Heading 4", Command::Heading(4)).shortcut(Shortcut::cmd('4').alt()))
        .separator()
        .item(f("Body Text", Command::Heading(0)).shortcut(Shortcut::cmd('0').alt()));
    let format = Menu::new("Format")
        .submenu(font)
        .submenu(paragraph)
        .separator()
        .item(f("Bold", Command::Inline(Inline::Bold)).shortcut(Shortcut::cmd('B')))
        .item(f("Italic", Command::Inline(Inline::Italic)).shortcut(Shortcut::cmd('I')))
        .item(f("Code", Command::Inline(Inline::Code)).shortcut(Shortcut::cmd('E')))
        .item(f("Link…", Command::Link).shortcut(Shortcut::cmd('K')))
        .separator()
        .submenu(heading)
        .item(f("Quote", Command::Quote).shortcut(Shortcut::cmd('\'').shift()))
        .separator()
        .item(f("Copy Style", Command::CopyStyle).shortcut(Shortcut::cmd('C').alt()))
        .item(
            f("Paste Style", Command::PasteStyle).shortcut(Shortcut::cmd('V').alt()).enabled(md && app.style.is_some()),
        );

    let current_lang = doc.map(|d| d.language);
    let mut languages = Menu::new("Language");
    for l in Language::ALL {
        languages.push(cmd(l.name(), Command::SetLanguage(l)).checked(current_lang == Some(l)).enabled(has_doc));
    }
    let view = Menu::new("View")
        .item(MenuItem::new("Line Numbers", Msg::ToggleNumbers).checked(s.line_numbers))
        .item(MenuItem::new("Wrap Lines", Msg::ToggleWrap).checked(s.wrap))
        .separator()
        .item(MenuItem::new("Zoom In", Msg::Zoom(Some(true))).shortcut(Shortcut::cmd('=')))
        .item(MenuItem::new("Zoom Out", Msg::Zoom(Some(false))).shortcut(Shortcut::cmd('-')))
        .item(MenuItem::new("Actual Size", Msg::Zoom(None)).shortcut(Shortcut::cmd('0')))
        .separator()
        .submenu(languages);

    MenuBar::standard(&info)
        .replace(&name, app_menu)
        .with_file_menu(file)
        .replace(EDIT, edit)
        .with_menu(format)
        .with_menu(view)
        .extend(HELP, std::iter::empty())
        .extend(WINDOW, std::iter::empty())
}
