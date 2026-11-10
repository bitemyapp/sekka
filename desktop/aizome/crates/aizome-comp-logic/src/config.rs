//! Settings the logic follows: key bindings, Desks, and the caches that
//! depend on the theme and the installed apps. The Desks go both ways:
//! added, renamed, moved or removed here, they're written to the settings,
//! which Settei shows and saves from, where the settings are this
//! compositor's to write ([`owns_settings`]).

use std::path::Path;

use aizome_comp_core::aizome_config::{self, DeskConfig, Settings, default_desks};
use aizome_comp_core::aizome_ipc::Event;

use crate::prelude::*;

/// Whether the settings are this compositor's to write. The login
/// session's compositor owns the person's settings. A headless or nested
/// one (a test, `scripts/headless-session`, Aizome tried inside another
/// desktop) runs as the same person beside their login session and reads
/// the same file, so it owns only settings given to it alone: a file named
/// with `--config`, or a configuration directory other than the person's
/// own (`XDG_CONFIG_HOME` away from `~/.config`). Otherwise a Desk removed
/// while trying something would be removed, its windows moved, in the
/// session the person is working in, which follows the file.
pub fn owns_settings(login_session: bool, given_a_file: bool, config_dir: &Path, persons: &Path) -> bool {
    login_session || given_a_file || !same_dir(config_dir, persons)
}

/// The same directory, by name or (where both are there) by where the
/// names lead.
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Whether this compositor runs the person's login session: what it tells
/// its shell and every program it starts (`AIZOME_SESSION=1`, only on a
/// TTY without `--no-session`), and what the shell goes by for Keep Awake
/// and logind.
fn login_session(az: &Aizome) -> bool {
    az.children.env.iter().rev().find(|(name, _)| name == "AIZOME_SESSION").is_some_and(|(_, value)| value == "1")
}

/// The Desks as they are here, as the settings keep them: by name and
/// folder, each keeping keys this version doesn't know from the entry of
/// the same name (else the same place). The four defaults are no entries.
pub fn desk_configs(old: &[DeskConfig], live: &[(String, Option<String>)]) -> Vec<DeskConfig> {
    let configs: Vec<DeskConfig> = live
        .iter()
        .enumerate()
        .map(|(i, (name, folder))| {
            let was = old.iter().find(|d| d.name == *name).or_else(|| old.get(i));
            DeskConfig {
                name: name.clone(),
                folder: folder.clone(),
                extra: was.map(|d| d.extra.clone()).unwrap_or_default(),
            }
        })
        .collect();
    if configs == default_desks() { Vec::new() } else { configs }
}

/// Write the Desks to the settings after a change made here (from the
/// overview, the bar's Desks menu, `aizomectl`), so Settei shows them, and
/// a change it saves keeps them rather than putting back the ones the file
/// had (#228). Only where the settings are this compositor's to write
/// ([`owns_settings`]). The file is read again first, so what another
/// program saved there outside the Desks stays, applied here yet or not;
/// the Desks become the ones here.
pub fn save_desks(st: &mut State) {
    let live: Vec<(String, Option<String>)> =
        st.lg.r().wm.desks().iter().map(|d| (d.name.clone(), d.folder.clone())).collect();
    let desks = desk_configs(&st.az.settings.desks_or_default(), &live);
    if desks == st.az.settings.desks {
        return;
    }
    let given = st.az.options.config.clone();
    let persons = aizome_config::home_dir().join(".config");
    if !owns_settings(login_session(&st.az), given.is_some(), &aizome_config::config_dir(), &persons) {
        return;
    }
    let path = given.unwrap_or_else(Settings::path);
    let mut file = match Settings::load_from(&path) {
        Ok(file) => file,
        Err(e) => {
            // Not over a file someone is in the middle of fixing.
            tracing::warn!("{}: {e}; the Desks aren't saved", path.display());
            return;
        }
    };
    file.desks = desks.clone();
    if let Err(e) = file.save_to(&path) {
        tracing::warn!("saving the Desks to {}: {e}", path.display());
        return;
    }
    // Only now that the file has them. Applying the settings brings the
    // Desks into line with the file when its Desks differ from the ones
    // kept here: had these been kept before a save that failed, the next
    // change to any setting would take the Desks back to the file's.
    st.az.settings.desks = desks;
    // Whoever holds the settings reads them again now, not at its next
    // look at the file: a whole-file save from a copy read before this
    // would put the old Desks back.
    st.az.ipc.emit(&Event::ConfigChanged);
}

/// Settings were applied (the old ones given).
pub fn settings_changed(st: &mut State, old: &Settings) {
    let s = st.az.settings.clone();
    // Installed apps and icon themes may have changed too.
    st.az.icons.clear();
    // The theme and focus colour: labels are drawn again.
    if old.appearance != s.appearance {
        st.az.labels.clear();
    }
    let l = st.lg.m();
    if old.keys != s.keys {
        l.bindings = crate::bindings::Bindings::new(&s.keys.bindings);
    }
    // Desks.
    let wanted: Vec<(String, Option<String>)> = s.desks_or_default().into_iter().map(|d| (d.name, d.folder)).collect();
    if old.desks_or_default() != s.desks_or_default() && l.wm.sync_desks(&wanted) {
        st.az.window_outputs_dirty = true;
        st.az.queue_redraw_all();
        st.emit_desks();
        let l = st.lg.r();
        let r = l.wm.focused().filter(|f| l.wm.is_shown(*f));
        if r.is_none() {
            let next = l.wm.next_focus(None);
            st.focus_window(next);
        }
    }
    st.az.queue_redraw_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(names: &[&str]) -> Vec<(String, Option<String>)> {
        names.iter().map(|n| (n.to_string(), None)).collect()
    }

    #[test]
    fn desks_are_kept_as_the_settings_have_them() {
        // The four defaults are no entries; a fifth makes all five.
        assert!(desk_configs(&default_desks(), &live(&["1", "2", "3", "4"])).is_empty());
        let five = desk_configs(&default_desks(), &live(&["1", "2", "3", "4", "5"]));
        assert_eq!(five.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4", "5"]);
        // Keys this version doesn't know stay with their Desk, by name, else by place.
        let mut mail = DeskConfig::named("Mail");
        mail.extra.insert("wallpaper".into(), "sea.png".into());
        let old = vec![DeskConfig::named("1"), mail];
        let moved = desk_configs(&old, &[("Mail".into(), Some("/home/a/Mail".into())), ("1".into(), None)]);
        assert_eq!(moved[0].extra.get("wallpaper").and_then(|v| v.as_str()), Some("sea.png"));
        assert_eq!(moved[0].folder.as_deref(), Some("/home/a/Mail"));
        assert!(moved[1].extra.is_empty());
        let renamed = desk_configs(&old, &live(&["1", "Post"]));
        assert_eq!(renamed[1].extra.get("wallpaper").and_then(|v| v.as_str()), Some("sea.png"));
    }

    #[test]
    fn only_settings_of_its_own_are_this_compositors_to_write() {
        let persons = Path::new("/home/a/.config");
        // The login session's compositor: the person's settings are its own.
        assert!(owns_settings(true, false, persons, persons));
        // A headless or nested one beside it reads the same settings, and
        // leaves them as they are,
        assert!(!owns_settings(false, false, persons, persons));
        assert!(!owns_settings(false, false, Path::new("/home/a/.config/"), persons));
        // unless they were given to it: a file (`--config`), or a
        // configuration directory of its own.
        assert!(owns_settings(false, true, persons, persons));
        assert!(owns_settings(false, false, Path::new("/tmp/aizome-test-1-0/config"), persons));
    }

    #[test]
    fn another_name_for_the_persons_directory_is_still_theirs() {
        let dir = std::env::temp_dir().join(format!("aizome-owns-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home/.config")).unwrap();
        std::os::unix::fs::symlink(dir.join("home/.config"), dir.join("config")).unwrap();
        assert!(!owns_settings(false, false, &dir.join("config"), &dir.join("home/.config")));
        // One that isn't there yet is compared by name.
        assert!(owns_settings(false, false, &dir.join("other"), &dir.join("home/.config")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
