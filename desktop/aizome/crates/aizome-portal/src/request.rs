//! What a chooser is asked and what it answers: as the portal frontend
//! passes them (`org.freedesktop.impl.portal.FileChooser`'s options and
//! results, vardicts), and as the service hands them to a chooser (JSON
//! on its standard input and output).

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zbus::zvariant::{OwnedValue, Value};

/// Which of the interface's methods asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// `OpenFile`: files, or folders.
    #[default]
    Open,
    /// `SaveFile`: one file's name and place.
    Save,
    /// `SaveFiles`: a folder to save named files in.
    SaveFiles,
}

/// A filter's rule: a shell-style pattern (`*.png`) or a MIME type
/// (`image/png`, `image/*`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    Glob(String),
    Mime(String),
}

/// A named filter, `(sa(us))` on the bus: `("Images", [(0, "*.png"), (1, "image/jpeg")])`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub name: String,
    pub rules: Vec<Rule>,
}

type WireFilter = (String, Vec<(u32, String)>);
/// A choice on the bus, `(ssa(ss)s)`.
type WireChoice = (String, String, Vec<(String, String)>, String);

impl Filter {
    fn from_wire((name, rules): WireFilter) -> Filter {
        let rules = rules
            .into_iter()
            .filter_map(|(kind, rule)| match kind {
                0 => Some(Rule::Glob(rule)),
                1 => Some(Rule::Mime(rule)),
                _ => None,
            })
            .collect();
        Filter { name, rules }
    }

    fn to_wire(&self) -> WireFilter {
        let rules = self
            .rules
            .iter()
            .map(|r| match r {
                Rule::Glob(g) => (0, g.clone()),
                Rule::Mime(m) => (1, m.clone()),
            })
            .collect();
        (self.name.clone(), rules)
    }
}

/// An extra choice the app offers beside the files, `(ssa(ss)s)`: a
/// pop-up of options, or, with none, a checkbox whose value is `"true"`
/// or `"false"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    /// (id, label) of each option.
    pub options: Vec<(String, String)>,
    /// The option chosen at first (a checkbox's `"true"` or `"false"`).
    pub default: String,
}

impl Choice {
    /// A checkbox rather than a pop-up.
    pub fn is_toggle(&self) -> bool {
        self.options.is_empty()
    }

    /// The value it starts with: the default when it's one of the
    /// options, else the first.
    pub fn initial(&self) -> String {
        if self.is_toggle() {
            return if self.default == "true" { "true" } else { "false" }.into();
        }
        match self.options.iter().find(|(id, _)| *id == self.default) {
            Some((id, _)) => id.clone(),
            None => self.options.first().map(|(id, _)| id.clone()).unwrap_or_default(),
        }
    }
}

/// A request, as a chooser reads it. Paths are bytes: a path needn't be
/// UTF-8, and JSON can't carry other strings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub mode: Mode,
    /// The asking app's id (empty for apps outside a sandbox that didn't
    /// register one with the portal).
    pub app_id: String,
    /// The asking app's window (`wayland:<handle>`, `x11:<xid>`, or
    /// empty).
    pub parent_window: String,
    pub title: String,
    /// The accept button's label, its mnemonic underscores taken out.
    pub accept_label: Option<String>,
    pub multiple: bool,
    /// Folders rather than files.
    pub directory: bool,
    pub filters: Vec<Filter>,
    pub current_filter: Option<Filter>,
    pub choices: Vec<Choice>,
    /// A name to save as.
    pub current_name: Option<String>,
    pub current_folder: Option<Vec<u8>>,
    /// The file being saved again, for Save As.
    pub current_file: Option<Vec<u8>>,
    /// `SaveFiles`' names.
    pub files: Vec<Vec<u8>>,
}

impl Request {
    /// A request from the frontend's arguments. Options of the wrong type
    /// are left out, as if not given.
    pub fn from_bus(mode: Mode, app_id: &str, parent_window: &str, title: &str, options: &Options) -> Request {
        Request {
            mode,
            app_id: app_id.to_owned(),
            parent_window: parent_window.to_owned(),
            title: title.to_owned(),
            accept_label: get::<String>(options, "accept_label").map(|l| plain_label(&l)).filter(|l| !l.is_empty()),
            multiple: mode == Mode::Open && get::<bool>(options, "multiple").unwrap_or(false),
            directory: mode == Mode::Open && get::<bool>(options, "directory").unwrap_or(false),
            filters: get::<Vec<WireFilter>>(options, "filters")
                .unwrap_or_default()
                .into_iter()
                .map(Filter::from_wire)
                .collect(),
            current_filter: get::<WireFilter>(options, "current_filter").map(Filter::from_wire),
            choices: get::<Vec<WireChoice>>(options, "choices")
                .unwrap_or_default()
                .into_iter()
                .map(|(id, label, options, default)| Choice { id, label: plain_label(&label), options, default })
                .collect(),
            current_name: get::<String>(options, "current_name").filter(|_| mode == Mode::Save),
            current_folder: get::<Vec<u8>>(options, "current_folder").map(nul_terminated).filter(|b| !b.is_empty()),
            current_file: get::<Vec<u8>>(options, "current_file")
                .map(nul_terminated)
                .filter(|b| !b.is_empty() && mode == Mode::Save),
            files: if mode == Mode::SaveFiles {
                get::<Vec<Vec<u8>>>(options, "files")
                    .unwrap_or_default()
                    .into_iter()
                    .map(nul_terminated)
                    .filter(|b| !b.is_empty())
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    /// The window's title: the app's, else what the dialog does.
    pub fn window_title(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.clone();
        }
        match (self.mode, self.directory) {
            (Mode::Open, true) => "Choose a Folder".into(),
            (Mode::Open, false) if self.multiple => "Open Files".into(),
            (Mode::Open, false) => "Open File".into(),
            (Mode::Save, _) => "Save File".into(),
            (Mode::SaveFiles, _) => "Save Files".into(),
        }
    }

    /// The accept button's label: the app's, else the verb.
    pub fn accept(&self) -> String {
        if let Some(l) = &self.accept_label {
            return l.clone();
        }
        match (self.mode, self.directory) {
            (Mode::Open, true) => "Choose".into(),
            (Mode::Open, false) => "Open".into(),
            (Mode::Save | Mode::SaveFiles, _) => "Save".into(),
        }
    }

    /// `current_folder` as a path.
    pub fn folder(&self) -> Option<PathBuf> {
        self.current_folder.as_deref().map(path_of_bytes)
    }

    /// `current_file` as a path.
    pub fn file(&self) -> Option<PathBuf> {
        self.current_file.as_deref().map(path_of_bytes)
    }

    /// The Wayland handle of the asking app's window, if it gave one.
    pub fn wayland_parent(&self) -> Option<&str> {
        self.parent_window.strip_prefix("wayland:").filter(|h| !h.is_empty())
    }
}

/// A chooser's answer: what was chosen, and how the choices and the
/// filter were left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub uris: Vec<String>,
    /// (choice id, value).
    pub choices: Vec<(String, String)>,
    pub current_filter: Option<Filter>,
    /// Opened files may be written to (OpenFile only).
    pub writable: bool,
}

impl Answer {
    /// The results vardict for the frontend.
    pub fn to_bus(&self, mode: Mode) -> Options {
        let mut out = Options::new();
        let mut put = |key: &str, value: Value<'_>| {
            if let Ok(v) = OwnedValue::try_from(value) {
                out.insert(key.to_owned(), v);
            }
        };
        put("uris", Value::from(self.uris.clone()));
        if !self.choices.is_empty() {
            put("choices", Value::from(self.choices.clone()));
        }
        if let Some(f) = &self.current_filter
            && mode != Mode::SaveFiles
        {
            put("current_filter", Value::from(f.to_wire()));
        }
        if mode == Mode::Open {
            put("writable", Value::from(self.writable));
        }
        out
    }
}

/// A vardict, `a{sv}`.
pub type Options = HashMap<String, OwnedValue>;

fn get<T>(options: &Options, key: &str) -> Option<T>
where
    T: TryFrom<Value<'static>>,
{
    let v: Value<'static> = options.get(key)?.try_clone().ok()?.into();
    T::try_from(v).ok()
}

/// A byte string without the NUL the portal puts at its end.
fn nul_terminated(mut b: Vec<u8>) -> Vec<u8> {
    while b.last() == Some(&0) {
        b.pop();
    }
    b
}

/// The path some bytes name.
pub fn path_of_bytes(b: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(b.to_vec()))
}

/// A path's bytes.
pub fn bytes_of_path(p: &Path) -> Vec<u8> {
    p.as_os_str().as_bytes().to_vec()
}

/// A GTK label without its mnemonic: `_Open` → `Open`, `Save __As` →
/// `Save _As`.
pub fn plain_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(pairs: Vec<(&str, Value<'static>)>) -> Options {
        pairs.into_iter().map(|(k, v)| (k.to_owned(), OwnedValue::try_from(v).unwrap())).collect()
    }

    #[test]
    fn reads_every_option_of_open_file() {
        let filters: Vec<WireFilter> = vec![
            ("Images".into(), vec![(0, "*.png".into()), (1, "image/jpeg".into())]),
            ("All".into(), vec![(0, "*".into())]),
        ];
        let choices: Vec<WireChoice> = vec![
            ("encoding".into(), "_Encoding".into(), vec![("utf8".into(), "UTF-8".into())], "utf8".into()),
            ("reencode".into(), "Re-encode".into(), vec![], "true".into()),
        ];
        let o = options(vec![
            ("accept_label", Value::from("_Import")),
            ("multiple", Value::from(true)),
            ("filters", Value::from(filters.clone())),
            ("current_filter", Value::from(filters[1].clone())),
            ("choices", Value::from(choices)),
            ("current_folder", Value::from(b"/tmp/a b\0".to_vec())),
            // Save's alone: ignored here.
            ("current_name", Value::from("x.txt")),
        ]);
        let r = Request::from_bus(Mode::Open, "org.example.App", "wayland:abc", "Import", &o);
        assert_eq!(r.accept_label.as_deref(), Some("Import"));
        assert!(r.multiple && !r.directory);
        assert_eq!(r.filters.len(), 2);
        assert_eq!(r.filters[0].rules, vec![Rule::Glob("*.png".into()), Rule::Mime("image/jpeg".into())]);
        assert_eq!(r.current_filter.as_ref().map(|f| f.name.as_str()), Some("All"));
        assert_eq!(r.choices[0].label, "Encoding");
        assert!(r.choices[1].is_toggle());
        assert_eq!(r.choices[1].initial(), "true");
        assert_eq!(r.folder(), Some(PathBuf::from("/tmp/a b")));
        assert_eq!(r.current_name, None);
        assert_eq!(r.wayland_parent(), Some("abc"));
    }

    #[test]
    fn wrong_types_are_left_out() {
        let o = options(vec![("multiple", Value::from("yes")), ("filters", Value::from(3u32))]);
        let r = Request::from_bus(Mode::Open, "", "x11:1a", "", &o);
        assert!(!r.multiple);
        assert!(r.filters.is_empty());
        assert_eq!(r.wayland_parent(), None);
        assert_eq!(r.window_title(), "Open File");
        assert_eq!(r.accept(), "Open");
    }

    #[test]
    fn reads_save_and_save_files() {
        let o = options(vec![
            ("current_name", Value::from("report.pdf")),
            ("current_file", Value::from(b"/home/a/report.pdf\0".to_vec())),
        ]);
        let r = Request::from_bus(Mode::Save, "", "", "", &o);
        assert_eq!(r.current_name.as_deref(), Some("report.pdf"));
        assert_eq!(r.file(), Some(PathBuf::from("/home/a/report.pdf")));
        assert_eq!(r.accept(), "Save");
        let files: Vec<Vec<u8>> = vec![b"a.txt\0".to_vec(), b"b.txt\0".to_vec()];
        let o = options(vec![("files", Value::from(files))]);
        let r = Request::from_bus(Mode::SaveFiles, "", "", "", &o);
        assert_eq!(r.files, vec![b"a.txt".to_vec(), b"b.txt".to_vec()]);
        assert_eq!(r.window_title(), "Save Files");
    }

    #[test]
    fn answers_as_the_frontend_reads_them() {
        let a = Answer {
            uris: vec!["file:///tmp/a.png".into()],
            choices: vec![("encoding".into(), "utf8".into())],
            current_filter: Some(Filter { name: "Images".into(), rules: vec![Rule::Glob("*.png".into())] }),
            writable: true,
        };
        let out = a.to_bus(Mode::Open);
        assert_eq!(get::<Vec<String>>(&out, "uris"), Some(vec!["file:///tmp/a.png".into()]));
        assert_eq!(get::<Vec<(String, String)>>(&out, "choices"), Some(vec![("encoding".into(), "utf8".into())]));
        assert_eq!(get::<WireFilter>(&out, "current_filter"), Some(("Images".into(), vec![(0, "*.png".into())])));
        assert_eq!(get::<bool>(&out, "writable"), Some(true));
        let out = a.to_bus(Mode::SaveFiles);
        assert!(!out.contains_key("current_filter") && !out.contains_key("writable"));
    }

    #[test]
    fn mnemonics_come_out() {
        assert_eq!(plain_label("_Open"), "Open");
        assert_eq!(plain_label("Save __As"), "Save _As");
        assert_eq!(plain_label("Plain"), "Plain");
    }

    #[test]
    fn a_choice_starts_with_a_real_option() {
        let c = Choice {
            id: "c".into(),
            label: "C".into(),
            options: vec![("a".into(), "A".into()), ("b".into(), "B".into())],
            default: "zz".into(),
        };
        assert_eq!(c.initial(), "a");
    }

    #[test]
    fn requests_travel_as_json_with_any_path() {
        let r = Request { current_folder: Some(vec![b'/', 0xff, b'x']), ..Request::default() };
        let back: Request = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        assert_eq!(bytes_of_path(&back.folder().unwrap()), vec![b'/', 0xff, b'x']);
    }
}
