//! Settings the logic follows: key bindings, Desks, and the caches that
//! depend on the theme and the installed apps. The Desks go both ways:
//! added, renamed, moved or removed here, they're written to the settings,
//! which Settei shows and saves from.

use aizome_comp_core::aizome_config::{DeskConfig, Settings, default_desks};

use crate::prelude::*;

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
/// had (#228). The file is read again first: another program's save that
/// hasn't been applied yet stays.
pub fn save_desks(st: &mut State) {
    let live: Vec<(String, Option<String>)> =
        st.lg.r().wm.desks().iter().map(|d| (d.name.clone(), d.folder.clone())).collect();
    let desks = desk_configs(&st.az.settings.desks_or_default(), &live);
    if desks == st.az.settings.desks {
        return;
    }
    st.az.settings.desks = desks.clone();
    let path = st.az.options.config.clone().unwrap_or_else(Settings::path);
    let mut file = match Settings::load_from(&path) {
        Ok(file) => file,
        Err(e) => {
            // Not over a file someone is in the middle of fixing.
            tracing::warn!("{}: {e}; the Desks aren't saved", path.display());
            return;
        }
    };
    file.desks = desks;
    if let Err(e) = file.save_to(&path) {
        tracing::warn!("saving the Desks to {}: {e}", path.display());
    }
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
}
