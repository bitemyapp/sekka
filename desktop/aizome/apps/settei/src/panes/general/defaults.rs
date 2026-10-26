//! Default Apps: which app opens links, mail, folders, text, pictures, PDF
//! documents, music and video, and which terminal ❖Return starts. Written
//! to `mimeapps.list` (and, for the terminal, `xdg-terminals.list` and the
//! compositor's `[keys] terminal`) in the user's config.

use std::sync::{Arc, Mutex};

use aizome_kit::prelude::*;
use aizome_sys::apps::{AppIndex, DefaultRole, DesktopEntry, LaunchOptions, command_lines};

use super::Msg as G;
use crate::form;
use crate::pane::{PaneCx, SettingEntry, ViewCx};

/// One role as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleRow {
    pub role: DefaultRole,
    /// The default's desktop file id.
    pub current: Option<String>,
    /// The apps that can, as (id, name).
    pub candidates: Vec<(String, String)>,
}

#[derive(Default)]
pub struct State {
    index: Option<Arc<Mutex<AppIndex>>>,
    rows: Option<Vec<RoleRow>>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Loaded(Arc<Mutex<AppIndex>>, Vec<RoleRow>),
    Set(DefaultRole, usize),
}

/// The roles the page lists, in its order.
pub const ROLES: [DefaultRole; 11] = DefaultRole::ALL;

fn role_id(role: DefaultRole) -> &'static str {
    match role {
        DefaultRole::Browser => "default-browser",
        DefaultRole::Mail => "default-mail",
        DefaultRole::Terminal => "default-terminal",
        DefaultRole::FileManager => "default-files",
        DefaultRole::TextEditor => "default-text-editor",
        DefaultRole::ImageViewer => "default-images",
        DefaultRole::VideoPlayer => "default-video",
        DefaultRole::MusicPlayer => "default-music",
        DefaultRole::PdfViewer => "default-pdf",
        DefaultRole::Calendar => "default-calendar",
        DefaultRole::ArchiveManager => "default-archives",
    }
}

fn role_icon(role: DefaultRole) -> Icon {
    match role {
        DefaultRole::Browser => Icon::Globe,
        DefaultRole::Mail => Icon::Mail,
        DefaultRole::Terminal => Icon::SquareTerminal,
        DefaultRole::FileManager => Icon::Folder,
        DefaultRole::TextEditor => Icon::FileText,
        DefaultRole::ImageViewer => Icon::Image,
        DefaultRole::VideoPlayer => Icon::Film,
        DefaultRole::MusicPlayer => Icon::EighthNotes,
        DefaultRole::PdfViewer => Icon::BookOpen,
        DefaultRole::Calendar => Icon::Calendar,
        DefaultRole::ArchiveManager => Icon::Archive,
    }
}

pub fn entries() -> Vec<SettingEntry> {
    ROLES
        .iter()
        .map(|r| {
            let kw: &[&str] = match r {
                DefaultRole::Browser => &["web", "firefox", "chrome", "links"],
                DefaultRole::Mail => &["email", "mailto"],
                DefaultRole::Terminal => &["shell", "foot", "console"],
                DefaultRole::FileManager => &["folders", "hako"],
                DefaultRole::TextEditor => &["sumi", "text files"],
                DefaultRole::ImageViewer => &["photos", "pictures"],
                DefaultRole::VideoPlayer => &["movies", "mpv"],
                DefaultRole::MusicPlayer => &["audio", "songs"],
                DefaultRole::PdfViewer => &["documents", "reader"],
                DefaultRole::Calendar => &["koyomi", "ics"],
                DefaultRole::ArchiveManager => &["zip", "tar"],
            };
            SettingEntry::new(role_id(*r), r.label()).under("Default Apps").keywords(kw)
        })
        .collect()
}

/// Each role's default and choices, from the index.
pub fn rows_of(index: &AppIndex) -> Vec<RoleRow> {
    ROLES
        .iter()
        .map(|&role| {
            let current = index.role_default(role);
            let candidates = index
                .role_candidates(role)
                .into_iter()
                .map(|id| {
                    let name = index
                        .get(&id)
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| id.trim_end_matches(".desktop").to_owned());
                    (id, name)
                })
                .collect();
            RoleRow { role, current, candidates }
        })
        .collect()
}

pub fn sample_rows() -> Vec<RoleRow> {
    let c = |pairs: &[(&str, &str)]| pairs.iter().map(|(i, n)| ((*i).to_owned(), (*n).to_owned())).collect::<Vec<_>>();
    let row = |role, current: &str, cands: Vec<(String, String)>| RoleRow {
        role,
        current: Some(current.into()),
        candidates: cands,
    };
    vec![
        row(
            DefaultRole::Browser,
            "firefox.desktop",
            c(&[("firefox.desktop", "Firefox"), ("chromium-browser.desktop", "Chromium")]),
        ),
        row(DefaultRole::Mail, "thunderbird.desktop", c(&[("thunderbird.desktop", "Thunderbird")])),
        row(
            DefaultRole::Terminal,
            "foot.desktop",
            c(&[("foot.desktop", "Foot"), ("org.wezfurlong.wezterm.desktop", "WezTerm")]),
        ),
        row(DefaultRole::FileManager, "org.aizome.Hako.desktop", c(&[("org.aizome.Hako.desktop", "Hako")])),
        row(
            DefaultRole::TextEditor,
            "org.aizome.Sumi.desktop",
            c(&[("org.aizome.Sumi.desktop", "Sumi"), ("helix.desktop", "Helix")]),
        ),
        row(DefaultRole::ImageViewer, "imv.desktop", c(&[("imv.desktop", "imv"), ("firefox.desktop", "Firefox")])),
        row(DefaultRole::VideoPlayer, "mpv.desktop", c(&[("mpv.desktop", "mpv")])),
        row(DefaultRole::MusicPlayer, "mpv.desktop", c(&[("mpv.desktop", "mpv")])),
        row(
            DefaultRole::PdfViewer,
            "org.pwmt.zathura.desktop",
            c(&[("org.pwmt.zathura.desktop", "Zathura"), ("firefox.desktop", "Firefox")]),
        ),
        row(DefaultRole::Calendar, "org.aizome.Koyomi.desktop", c(&[("org.aizome.Koyomi.desktop", "Koyomi")])),
        RoleRow { role: DefaultRole::ArchiveManager, current: None, candidates: Vec::new() },
    ]
}

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if cx.sample() {
            if self.rows.is_none() {
                self.rows = Some(sample_rows());
            }
            return;
        }
        cx.spawn(|| {
            let index = AppIndex::load();
            let rows = rows_of(&index);
            G::Defaults(Msg::Loaded(Arc::new(Mutex::new(index)), rows))
        });
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Loaded(index, rows) => {
                self.index = Some(index);
                self.rows = Some(rows);
            }
            Msg::Set(role, i) => {
                let Some(rows) = &mut self.rows else { return };
                let Some(row) = rows.iter_mut().find(|r| r.role == role) else { return };
                let Some((id, name)) = row.candidates.get(i).cloned() else { return };
                match &self.index {
                    Some(index) => {
                        let mut index = index.lock().unwrap_or_else(|e| e.into_inner());
                        match index.set_role_default(role, &id) {
                            Ok(()) => {
                                row.current = index.role_default(role);
                                cx.say(format!("{name} opens {} now", role.label().to_lowercase()));
                                // ❖Return starts the compositor's terminal command.
                                let command = (role == DefaultRole::Terminal)
                                    .then(|| index.get(&id).and_then(terminal_command))
                                    .flatten();
                                drop(index);
                                if let Some(command) = command {
                                    cx.change(|s| s.keys.terminal = command);
                                }
                            }
                            Err(e) => cx.failed(role.label(), e),
                        }
                    }
                    // The sample.
                    None => {
                        row.current = Some(id);
                        cx.say(format!("{name} opens {} now", role.label().to_lowercase()));
                    }
                }
            }
        }
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let rows: Vec<form::Row<Msg>> = match &self.rows {
            None => vec![form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET]))],
            Some(rows) => rows
                .iter()
                .map(|r| {
                    let role = r.role;
                    let control: Element<Msg> = if r.candidates.is_empty() {
                        text("None installed").role(Role::Text3).into()
                    } else {
                        let names: Vec<String> = r.candidates.iter().map(|c| c.1.clone()).collect();
                        let sel = r.current.as_ref().and_then(|c| r.candidates.iter().position(|(id, _)| id == c));
                        dropdown(names, sel)
                            .placeholder("Choose…")
                            .on_select(move |i| Msg::Set(role, i))
                            .min_width(200.0)
                            .into()
                    };
                    form::row(r.role.label(), control)
                        .leading(icon(role_icon(role)).size(16.0).role(Role::Text2))
                        .entry(role_id(role))
                })
                .collect(),
        };
        form::page(vec![vcx.section_with(
            "Open With",
            rows,
            Some("Apps that can open these are listed; an app chosen here also opens what's like it (an image viewer every kind of image it reads).".into()),
        )])
    }
}

/// The command ❖Return runs for a terminal's desktop entry (its `Exec`,
/// without the field codes it takes files by).
pub fn terminal_command(entry: &DesktopEntry) -> Option<Vec<String>> {
    command_lines(entry, &LaunchOptions::default()).ok()?.into_iter().next().filter(|argv| !argv.is_empty())
}

#[cfg(test)]
mod tests {
    use aizome_sys::apps::{DesktopEntry, Locale};

    use super::terminal_command;

    #[test]
    fn a_terminals_command_drops_its_field_codes() {
        let text = "[Desktop Entry]\nType=Application\nName=Kitty\nExec=kitty --single-instance %U\nCategories=TerminalEmulator;\n";
        let entry = DesktopEntry::parse(text, "kitty.desktop", &Locale::parse("C")).unwrap();
        assert_eq!(terminal_command(&entry), Some(vec!["kitty".to_owned(), "--single-instance".to_owned()]));
    }
}
