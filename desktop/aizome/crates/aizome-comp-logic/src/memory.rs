//! Where each app's windows last were: their size and position, by
//! app_id, kept in the state directory so new windows open where their
//! app left them.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::wm::Rect;

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
