//! `--list-settings [--json]`: every pane and searchable option, for the
//! command palette, printed without opening a window.
//!
//! Each item of the JSON array:
//!
//! | Field | |
//! |---|---|
//! | `id` | `keyboard` for a pane, `keyboard.caps-lock` for an option: what `--pane` takes |
//! | `pane` | the pane's id |
//! | `entry` | the option's id in its pane (`caps-lock`); absent for a pane |
//! | `kind` | `pane` or `option` |
//! | `title` | `Caps Lock key` |
//! | `path` | `Keyboard › Caps Lock key` |
//! | `keywords` | other words it answers to |
//! | `icon` | the pane's Lucide glyph (`keyboard`) |
//! | `command` | the arguments that open it: `["aizome-settei", "--pane", "keyboard.caps-lock"]` |

use serde::Serialize;

use crate::pane::AnyPane;

/// One pane or option.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub id: String,
    pub pane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    pub kind: &'static str,
    pub title: String,
    pub path: String,
    pub keywords: Vec<String>,
    pub icon: &'static str,
    pub command: Vec<String>,
}

/// Every pane, then its options, in the sidebar's order.
pub fn items(panes: &[Box<dyn AnyPane>]) -> Vec<Item> {
    let mut order: Vec<usize> = (0..panes.len()).collect();
    order.sort_by_key(|&i| (panes[i].group(), i));
    let mut out = Vec::new();
    for i in order {
        let p = &panes[i];
        let command = |id: &str| vec!["aizome-settei".to_owned(), "--pane".to_owned(), id.to_owned()];
        out.push(Item {
            id: p.id().to_owned(),
            pane: p.id().to_owned(),
            entry: None,
            kind: "pane",
            title: p.title().to_owned(),
            path: p.title().to_owned(),
            keywords: p.keywords().iter().map(|k| (*k).to_owned()).collect(),
            icon: p.icon().name(),
            command: command(p.id()),
        });
        for e in p.entries() {
            let id = format!("{}.{}", p.id(), e.id);
            out.push(Item {
                command: command(&id),
                id,
                pane: p.id().to_owned(),
                entry: Some(e.id.clone()),
                kind: "option",
                path: e.path(p.title()),
                title: e.title,
                keywords: e.keywords,
                icon: p.icon().name(),
            });
        }
    }
    out
}

/// The JSON the palette reads.
pub fn json(panes: &[Box<dyn AnyPane>]) -> String {
    serde_json::to_string_pretty(&items(panes)).unwrap_or_else(|_| "[]".to_owned())
}

/// A table for people: path, then id.
pub fn table(panes: &[Box<dyn AnyPane>]) -> String {
    let items = items(panes);
    let width = items.iter().map(|i| i.path.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for i in items {
        let pad = width - i.path.chars().count();
        out.push_str(&format!("{}{}  {}\n", i.path, " ".repeat(pad), i.id));
    }
    out
}
