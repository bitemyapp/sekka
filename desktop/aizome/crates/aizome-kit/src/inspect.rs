//! What a window shows, written where tests that drive a running app from
//! outside can read it (the NixOS VM tests click Settings' controls the
//! way a person would, and need to know where they are).
//!
//! With `AIZOME_KIT_INSPECT` naming a directory, each window writes
//! `<dir>/<app_id>.<pid>.<window>.json` after it paints (and as the pointer
//! moves over it): its title, kind, size, the pointer, the focused element,
//! and every element with a name (`.id`), text or semantics, with its
//! rectangle in the window's coordinates (points), the part of it that
//! shows, and its parts that aren't elements (a segmented control's
//! segments). Closing the window removes its file. Without the variable
//! nothing is written and nothing is measured.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};

use crate::layout::Rect;
use crate::platform::WinId;
use crate::tree::Tree;

/// The directory, if inspection is on.
fn dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| std::env::var_os("AIZOME_KIT_INSPECT").filter(|v| !v.is_empty()).map(PathBuf::from)).as_deref()
}

/// Whether windows write what they show.
pub fn enabled() -> bool {
    dir().is_some()
}

fn file(app_id: &str, win: WinId) -> Option<PathBuf> {
    Some(dir()?.join(format!("{app_id}.{}.{win}.json", std::process::id())))
}

fn rect(r: Rect) -> Value {
    json!([r.x0, r.y0, r.width(), r.height()])
}

/// A window's elements as JSON.
pub(crate) fn describe(tree: &Tree, focused: Option<crate::tree::NodeId>) -> Vec<Value> {
    let mut names = crate::hash::FxHashMap::default();
    for (name, id) in &tree.names {
        names.insert(*id, name.clone());
    }
    let mut out = Vec::new();
    for (i, n) in tree.nodes.iter().enumerate().filter(|(_, n)| !n.dead) {
        let name = names.get(&n.id);
        let text = n.widget.text();
        let semantics = crate::a11y::of(tree, i);
        if name.is_none() && text.is_none() && semantics.is_none() {
            continue;
        }
        let shows = n.clip.intersect(n.rect);
        let mut v = json!({
            "rect": rect(n.rect),
            "visible": shows.area() > 0.0,
            "shown": rect(if shows.area() > 0.0 { shows } else { Rect::ZERO }),
        });
        let o = v.as_object_mut().expect("an object");
        if let Some(name) = name {
            o.insert("name".into(), json!(name.as_ref()));
        }
        if let Some(text) = text {
            o.insert("text".into(), json!(text));
        }
        if focused == Some(n.id) {
            o.insert("focused".into(), json!(true));
        }
        let parts = n.widget.parts();
        if !parts.is_empty() {
            // In the window's coordinates; a part without a height spans
            // the widget's.
            let parts: Vec<Value> = parts
                .into_iter()
                .map(|(name, r)| {
                    let (y0, y1) = if r.height() > 0.0 { (r.y0, r.y1) } else { (0.0, n.rect.height()) };
                    let r = Rect::new(r.x0, y0, r.x1, y1) + n.rect.origin().to_vec2();
                    json!({ "name": name, "rect": rect(r) })
                })
                .collect();
            o.insert("parts".into(), Value::Array(parts));
        }
        if let Some(s) = semantics {
            o.insert("role".into(), json!(format!("{:?}", s.role).to_lowercase()));
            if let Some(l) = s.label {
                o.insert("label".into(), json!(l));
            }
            if let Some(val) = s.value {
                o.insert("value".into(), json!(val));
            }
            if let Some(c) = s.checked {
                o.insert("checked".into(), json!(c));
            }
            if let Some(sel) = s.selected {
                o.insert("selected".into(), json!(sel));
            }
            if s.disabled {
                o.insert("disabled".into(), json!(true));
            }
            if let Some(e) = s.expanded {
                o.insert("expanded".into(), json!(e));
            }
            if let Some((min, max, value)) = s.range {
                o.insert("range".into(), json!([min, max, value]));
            }
        }
        out.push(v);
    }
    out
}

/// Write window `win`'s file (it just painted).
pub(crate) fn painted(app_id: &str, win: WinId, header: Value, tree: &Tree, focused: Option<crate::tree::NodeId>) {
    let Some(path) = file(app_id, win) else { return };
    let mut doc = header;
    if let Some(o) = doc.as_object_mut() {
        o.insert("app_id".into(), json!(app_id));
        o.insert("pid".into(), json!(std::process::id()));
        o.insert("window".into(), json!(win));
        o.insert("elements".into(), Value::Array(describe(tree, focused)));
    }
    let tmp = path.with_extension("json.tmp");
    let written = serde_json::to_vec(&doc)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&tmp, bytes))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::warn!("inspect: {}: {e}", path.display());
    }
}

/// Window `win` closed: its file goes.
pub(crate) fn closed(app_id: &str, win: WinId) {
    if let Some(path) = file(app_id, win) {
        let _ = std::fs::remove_file(path);
    }
}
