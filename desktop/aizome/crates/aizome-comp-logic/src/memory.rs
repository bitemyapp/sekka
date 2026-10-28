//! Where each app's windows last were: their size and position, by
//! app_id, kept in the state directory so new windows open where their
//! app left them.

use std::collections::HashMap;
use std::path::PathBuf;

use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{SurfaceCachedState, ToplevelSurface};

use crate::wm::Rect;

/// Client constraints distinguish fixed-size helpers from normal windows.
/// Steam, for example, uses one app ID for menus, sign-in and its main UI.
#[derive(Debug, Default, Clone, Copy)]
pub struct SizeConstraints {
    min: (i32, i32),
    max: (i32, i32),
}

impl SizeConstraints {
    pub fn of(toplevel: &ToplevelSurface) -> Self {
        with_states(toplevel.wl_surface(), |s| {
            let c = *s.cached_state.get::<SurfaceCachedState>().current();
            Self { min: (c.min_size.w, c.min_size.h), max: (c.max_size.w, c.max_size.h) }
        })
    }

    pub fn fixed(self) -> bool {
        self.min.0 > 0 && self.min.1 > 0 && self.min == self.max
    }

    pub fn remembers(self, rect: Rect) -> bool {
        // A fixed-size toplevel owns its size; it must neither inherit nor
        // replace an app-wide size. Zero means unconstrained, not fixed.
        !self.fixed()
            && rect.w >= self.min.0.max(1)
            && rect.h >= self.min.1.max(1)
            && (self.max.0 == 0 || rect.w <= self.max.0)
            && (self.max.1 == 0 || rect.h <= self.max.1)
    }
}

#[derive(Debug, Default)]
pub struct Remembered {
    rects: HashMap<String, [i32; 4]>,
    path: Option<PathBuf>,
}

impl Remembered {
    pub fn load() -> Remembered {
        let path = aizome_config::state_dir().join("windows.json");
        let rects = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Remembered { rects, path: Some(path) }
    }

    pub fn get(&self, app_id: &str) -> Option<Rect> {
        if app_id.is_empty() {
            return None;
        }
        self.rects.get(app_id).map(|r| Rect::new(r[0], r[1], r[2], r[3]))
    }

    pub fn set(&mut self, app_id: &str, r: Rect) {
        if app_id.is_empty() || r.w < 32 || r.h < 32 {
            return;
        }
        self.rects.insert(app_id.to_string(), [r.x, r.y, r.w, r.h]);
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string(&self.rects) {
                let tmp = p.with_extension("json.tmp");
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, p);
                }
            }
        }
    }
}
