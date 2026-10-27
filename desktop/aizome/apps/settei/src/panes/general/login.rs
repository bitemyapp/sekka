//! Login Items: the programs that start when you log in (XDG autostart).
//! The system's own are turned off with an override in
//! `~/.config/autostart` (`Hidden=true`), as the specification says; ones
//! you added can be removed.

use std::path::{Path, PathBuf};

use aizome_kit::prelude::*;
use aizome_kit::row;
use aizome_sys::apps::{AppIndex, DesktopEntry, Locale};

use super::Msg as G;
use crate::form;
use crate::pane::{Confirm, PaneCx, SettingEntry, ViewCx};

/// One program that may start at login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The file's name: `nm-applet.desktop`.
    pub id: String,
    pub name: String,
    pub comment: String,
    pub enabled: bool,
    /// The user's own (no system file of that name): it can be removed.
    pub mine: bool,
}

#[derive(Default)]
pub struct State {
    items: Option<Vec<Item>>,
    /// Apps that could be added, as (desktop file path, name).
    apps: Vec<(PathBuf, String)>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Loaded(Vec<Item>, Vec<(PathBuf, String)>),
    Toggle(String, bool),
    Add(usize),
    Remove(String),
    ConfirmRemove(String),
}

pub fn entries() -> Vec<SettingEntry> {
    vec![SettingEntry::new("startup-apps", "Open at login").under("Login Items").keywords(&[
        "autostart",
        "startup",
        "background",
    ])]
}

fn user_dir() -> PathBuf {
    aizome_config::config_dir().join("autostart")
}

fn system_dirs() -> Vec<PathBuf> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/etc/xdg".into());
    dirs.split(':').map(|d| PathBuf::from(d).join("autostart")).collect()
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "desktop")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Whether an entry starts in Aizome.
fn starts(e: &DesktopEntry, text: &str) -> bool {
    let gnome_off = text.lines().any(|l| l.trim().eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false"));
    !e.hidden && !gnome_off && e.shown_in(&["Aizome".to_owned()])
}

/// The login items: the user's files over the system's, by name.
pub fn read() -> Vec<Item> {
    let locale = Locale::from_env();
    let mut out: Vec<Item> = Vec::new();
    let user = user_dir();
    let mut ids: Vec<(String, Option<PathBuf>, Option<PathBuf>)> = Vec::new();
    for p in files(&user) {
        let id = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        ids.push((id, Some(p), None));
    }
    for d in system_dirs() {
        for p in files(&d) {
            let id = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
            match ids.iter_mut().find(|(i, _, _)| *i == id) {
                Some(slot) => {
                    if slot.2.is_none() {
                        slot.2 = Some(p);
                    }
                }
                None => ids.push((id, None, Some(p))),
            }
        }
    }
    for (id, user_file, system_file) in ids {
        let Some(path) = user_file.clone().or(system_file.clone()) else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(entry) = DesktopEntry::parse(&text, &id, &locale) else { continue };
        // Entries for other desktops only (and never shown here) stay out.
        if !entry.only_show_in.is_empty()
            && !entry.only_show_in.iter().any(|d| d == "Aizome")
            && !starts(&entry, &text)
            && system_file.is_some()
        {
            continue;
        }
        out.push(Item {
            id,
            name: if entry.name.is_empty() { path.display().to_string() } else { entry.name.clone() },
            comment: entry.comment.clone().unwrap_or_default(),
            enabled: starts(&entry, &text),
            mine: system_file.is_none(),
        });
    }
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// `text` with `key=value` set in its `[Desktop Entry]` group.
pub fn set_key(text: &str, key: &str, value: &str) -> String {
    let mut out = Vec::new();
    let mut in_main = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_main && !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            in_main = t == "[Desktop Entry]";
        } else if in_main && t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            if !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            continue;
        }
        out.push(line.to_owned());
    }
    if !done {
        if !text.contains("[Desktop Entry]") {
            out.insert(0, "[Desktop Entry]".into());
        }
        out.push(format!("{key}={value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Turn a login item on or off for this user.
pub fn set_enabled(id: &str, on: bool) -> std::io::Result<()> {
    let user = user_dir().join(id);
    let text = match std::fs::read_to_string(&user) {
        Ok(t) => t,
        Err(_) => {
            let sys = system_dirs()
                .into_iter()
                .map(|d| d.join(id))
                .find(|p| p.is_file())
                .ok_or(std::io::ErrorKind::NotFound)?;
            std::fs::read_to_string(sys)?
        }
    };
    let mut text = set_key(&text, "Hidden", if on { "false" } else { "true" });
    if on {
        text = set_key(&text, "X-GNOME-Autostart-enabled", "true");
    }
    std::fs::create_dir_all(user_dir())?;
    std::fs::write(user, text)
}

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if cx.sample() {
            if self.items.is_none() {
                let i = |id: &str, name: &str, comment: &str, enabled: bool, mine: bool| Item {
                    id: id.into(),
                    name: name.into(),
                    comment: comment.into(),
                    enabled,
                    mine,
                };
                self.items = Some(vec![
                    i("org.aizome.Fusen.desktop", "Fusen", "Your stickies, back where you left them", true, true),
                    i("nm-applet.desktop", "Network", "Network connections in the tray", false, false),
                    i("syncthing-start.desktop", "Syncthing", "File synchronisation", true, false),
                    i("1password.desktop", "1Password", "Password manager", true, true),
                ]);
                self.apps = vec![(
                    PathBuf::from("/run/current-system/sw/share/applications/org.aizome.Tokei.desktop"),
                    "Tokei".into(),
                )];
            }
            return;
        }
        cx.spawn(|| {
            let index = AppIndex::load();
            let apps = index.apps().iter().filter_map(|a| a.path.clone().map(|p| (p, a.name.clone()))).collect();
            G::Login(Msg::Loaded(read(), apps))
        });
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Loaded(items, apps) => {
                self.items = Some(items);
                self.apps = apps;
            }
            Msg::Toggle(id, on) => {
                if !cx.sample()
                    && let Err(e) = set_enabled(&id, on)
                {
                    cx.failed("Login item", e);
                    return;
                }
                if let Some(it) = self.items.as_mut().and_then(|v| v.iter_mut().find(|i| i.id == id)) {
                    it.enabled = on;
                }
            }
            Msg::Add(i) => {
                let Some((path, name)) = self.apps.get(i).cloned() else { return };
                let id = path.file_name().and_then(|n| n.to_str()).unwrap_or("app.desktop").to_owned();
                if !cx.sample() {
                    let copied = std::fs::create_dir_all(user_dir())
                        .and_then(|()| std::fs::copy(&path, user_dir().join(&id)).map(|_| ()));
                    if let Err(e) = copied {
                        cx.failed("Add a login item", e);
                        return;
                    }
                }
                let items = self.items.get_or_insert_with(Vec::new);
                items.retain(|it| it.id != id);
                items.push(Item { id, name: name.clone(), comment: String::new(), enabled: true, mine: true });
                items.sort_by_key(|i| i.name.to_lowercase());
                cx.say(format!("{name} opens when you log in"));
            }
            Msg::Remove(id) => {
                let name = self
                    .items
                    .as_ref()
                    .and_then(|v| v.iter().find(|i| i.id == id))
                    .map(|i| i.name.clone())
                    .unwrap_or_default();
                cx.confirm(
                    Confirm::new(
                        format!("Stop opening {name} at login?"),
                        "It stays installed; only its login item goes.",
                        "Remove",
                        G::Login(Msg::ConfirmRemove(id)),
                    )
                    .destructive(),
                );
            }
            Msg::ConfirmRemove(id) => {
                if !cx.sample()
                    && let Err(e) = std::fs::remove_file(user_dir().join(&id))
                {
                    cx.failed("Remove the login item", e);
                    return;
                }
                if let Some(v) = &mut self.items {
                    v.retain(|i| i.id != id);
                }
            }
        }
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let mut rows: Vec<form::Row<Msg>> = match &self.items {
            None => vec![form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET]))],
            Some(items) if items.is_empty() => vec![form::whole(
                container(text("Nothing opens at login.").role(Role::Text3)).padding([12.0, form::INSET]),
            )],
            Some(items) => items
                .iter()
                .map(|it| {
                    let id = it.id.clone();
                    let mut controls = row![].spacing(8.0).align(Align::Center);
                    if it.mine {
                        controls = controls.push(
                            icon_button(Icon::Trash).toolbar().on_press(Msg::Remove(id.clone())).tooltip("Remove"),
                        );
                    }
                    controls = controls.push(toggle(it.enabled).on_toggle(move |on| Msg::Toggle(id.clone(), on)));
                    let mut r = form::row(it.name.clone(), controls)
                        .leading(icon(crate::widgets::app_glyph(&it.id)).size(16.0).role(Role::Text2));
                    if !it.comment.is_empty() {
                        r = r.detail(it.comment.clone());
                    }
                    r
                })
                .collect(),
        };
        if !rows.is_empty() {
            let first = rows.remove(0);
            rows.insert(0, first.entry("startup-apps"));
        }
        let names: Vec<String> = self.apps.iter().map(|a| a.1.clone()).collect();
        rows.push(form::row(
            "Add an app",
            dropdown(names, None).placeholder("Choose an app…").on_select(Msg::Add).min_width(200.0),
        ));
        form::page(vec![vcx.section_with(
            "Open at Login",
            rows,
            Some("Items the system starts can be turned off for you; ones you added can be removed.".into()),
        )])
    }
}
