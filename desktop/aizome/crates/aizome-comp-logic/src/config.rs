//! Settings the logic follows: key bindings, Desks, and the caches that
//! depend on the theme and the installed apps.

use aizome_comp_core::aizome_config::Settings;

use crate::prelude::*;

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
