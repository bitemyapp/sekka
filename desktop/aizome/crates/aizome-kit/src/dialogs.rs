//! Alerts, the About panel, and file panels.
//!
//! An alert is a sheet the kit builds: a title, a message and buttons,
//! the default one answering Return and the cancel one Escape. File
//! panels are Aizome's own, built from the kit's widgets: a path bar, a
//! sidebar of places, and the folder's files.

use std::marker::PhantomData;
use std::path::PathBuf;

use crate::Layoutable;
use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::engine::{Core, KitMsg};
use crate::icons::Icon;
use crate::layout::{Align, Constraints, Length, Point, Size};
use crate::platform::{Platform, WinId};
use crate::program::WindowKey;
use crate::text::text;
use crate::theme::Role;
use crate::widget::{LayoutCx, Widget};
use crate::widgets::controls::{button, checkbox, icon_button};
use crate::widgets::layout::container;
use crate::widgets::layout::{column, row, spacer};
use crate::widgets::scroll::scroll;
use crate::widgets::text_field::text_field;

/// How an alert's button behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Normal,
    /// Answers Return, drawn filled.
    Default,
    /// Answers Escape.
    Cancel,
    /// Does something that can't be undone: drawn in red.
    Destructive,
}

#[derive(Clone, Debug)]
pub(crate) struct AlertButton {
    pub label: String,
    pub msg: Option<MsgFn>,
    pub kind: ButtonKind,
}

/// An alert: `Alert::new("Delete “notes.md”?", "It goes to the Trash.")
/// .destructive_button("Delete", Msg::Delete).cancel_button("Cancel")`.
pub struct Alert<Msg> {
    title: String,
    message: String,
    buttons: Vec<AlertButton>,
    _m: PhantomData<fn() -> Msg>,
}

pub(crate) struct AlertSpec {
    pub title: String,
    pub message: String,
    pub buttons: Vec<AlertButton>,
}

impl<Msg: Clone + Send + 'static> Alert<Msg> {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Alert<Msg> {
        Alert { title: title.into(), message: message.into(), buttons: Vec::new(), _m: PhantomData }
    }

    fn push(mut self, label: impl Into<String>, msg: Option<Msg>, kind: ButtonKind) -> Self {
        self.buttons.push(AlertButton { label: label.into(), msg: msg.map(MsgFn::new), kind });
        self
    }

    pub fn button(self, label: impl Into<String>, msg: Msg) -> Self {
        self.push(label, Some(msg), ButtonKind::Normal)
    }

    /// The button Return presses.
    pub fn default_button(self, label: impl Into<String>, msg: Msg) -> Self {
        self.push(label, Some(msg), ButtonKind::Default)
    }

    /// The button Escape presses; it only closes the alert.
    pub fn cancel_button(self, label: impl Into<String>) -> Self {
        self.push(label, None, ButtonKind::Cancel)
    }

    /// The cancel button, sending `msg` too.
    pub fn cancel_with(self, label: impl Into<String>, msg: Msg) -> Self {
        self.push(label, Some(msg), ButtonKind::Cancel)
    }

    pub fn destructive_button(self, label: impl Into<String>, msg: Msg) -> Self {
        self.push(label, Some(msg), ButtonKind::Destructive)
    }

    pub(crate) fn into_spec(self) -> AlertSpec {
        let mut buttons = self.buttons;
        if buttons.is_empty() {
            buttons.push(AlertButton { label: "OK".into(), msg: None, kind: ButtonKind::Default });
        }
        AlertSpec { title: self.title, message: self.message, buttons }
    }
}

pub(crate) fn show_alert(core: &mut Core, win: WinId, spec: AlertSpec) {
    let id = core.fresh_id();
    core.alerts.insert(
        id,
        crate::engine::AlertState { window: win, title: spec.title, message: spec.message, buttons: spec.buttons },
    );
    if let Some(w) = core.wins.get_mut(&win) {
        w.alerts.push(id);
        w.dirty = true;
    }
}

/// The About panel: the app's icon, name, what it is and its version.
pub(crate) fn show_about(core: &mut Core, win: WinId) {
    let info = core.info.clone();
    let mut message = info.generic_name.clone();
    if let Some(v) = &info.version {
        message.push_str(&format!("\nVersion {v}"));
    }
    message.push_str("\nPart of Aizome 藍染");
    let spec = AlertSpec {
        title: info.name.clone(),
        message,
        buttons: vec![AlertButton { label: "OK".into(), msg: None, kind: ButtonKind::Default }],
    };
    show_alert(core, win, spec);
}

/// An alert's sheet content.
pub(crate) fn alert_element(core: &Core, id: u64) -> Option<RawElement> {
    let a = core.alerts.get(&id)?;
    let m = &core.theme.metrics;
    let mut buttons = Vec::new();
    // Cancel first, the default last (at the right), as macOS orders them.
    let mut order: Vec<usize> = (0..a.buttons.len()).collect();
    order.sort_by_key(|&i| match a.buttons[i].kind {
        ButtonKind::Cancel => 0,
        ButtonKind::Normal => 1,
        ButtonKind::Destructive => 2,
        ButtonKind::Default => 3,
    });
    for i in order {
        let b = &a.buttons[i];
        let mut el = button(b.label.clone()).wrap_label().on_press(KitMsg::AlertChoice { alert: id, index: i }).key(i);
        el = match b.kind {
            ButtonKind::Default => el.default(),
            ButtonKind::Cancel => el.cancel(),
            ButtonKind::Destructive => el.destructive(),
            ButtonKind::Normal => el,
        };
        buttons.push(Element::from(el).into_raw());
    }
    let actions = Element::from_raw(RawElement::new(
        AlertActions { gap: m.gap },
        Common::sized(Length::Fill, Length::Shrink),
        buttons,
    ));
    let body: Element<KitMsg> = column(vec![
        text(a.title.clone()).size(m.title).weight(aizome_theme::typeface::WEIGHT_SEMIBOLD).wrap().into(),
        text(a.message.clone()).role(Role::Text2).wrap().into(),
        crate::widgets::layout::space(4.0),
        actions,
    ])
    .spacing(8.0)
    .padding([20.0, 20.0, 16.0, 20.0])
    .align(Align::Start)
    .width(Length::Fill)
    .into();
    Some(Element::from(scroll(body).width(400.0).height(Length::Shrink)).into_raw())
}

/// Keep a compact trailing row only when every action fits at its natural
/// width. Otherwise stack full-width actions, letting long labels wrap.
/// The same keyed children retain their focus and action semantics on resize.
struct AlertActions {
    gap: f64,
}

impl Widget for AlertActions {
    fn name(&self) -> &'static str {
        "alert-actions"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let n = cx.child_count();
        let sizes: Vec<_> = (0..n).map(|i| cx.layout_child(i, Constraints::UNBOUNDED)).collect();
        let row_width = sizes.iter().map(|s| s.width).sum::<f64>() + self.gap * n.saturating_sub(1) as f64;
        if row_width <= bc.max.width {
            let height = sizes.iter().map(|s| s.height).fold(0.0, f64::max);
            let size = bc.constrain(Size::new(row_width, height));
            let mut x = (size.width - row_width).max(0.0);
            for (i, child) in sizes.iter().enumerate() {
                cx.place_child(i, Point::new(x, (size.height - child.height) / 2.0));
                x += child.width + self.gap;
            }
            size
        } else {
            let width = bc.max.width;
            let mut y = 0.0;
            for i in 0..n {
                let child = cx
                    .layout_child(i, Constraints { min: Size::new(width, 0.0), max: Size::new(width, f64::INFINITY) });
                cx.place_child(i, Point::new(0.0, y));
                y += child.height + self.gap;
            }
            bc.constrain(Size::new(width, (y - self.gap).max(0.0)))
        }
    }
}

pub(crate) fn alert_chosen(core: &mut Core, alert: u64, index: usize) {
    let Some(a) = core.alerts.remove(&alert) else { return };
    if let Some(w) = core.wins.get_mut(&a.window) {
        w.alerts.retain(|x| *x != alert);
        w.dirty = true;
    }
    if let Some(m) = a.buttons.get(index).and_then(|b| b.msg.as_ref()) {
        core.queue.push_back(m.make());
    }
}

// File panels.

/// What a file panel asks for.
#[derive(Clone, Debug, Default)]
pub struct FileDialog {
    pub title: Option<String>,
    /// The folder it opens in (else home).
    pub directory: Option<PathBuf>,
    /// For saving: the name it suggests.
    pub file_name: Option<String>,
    /// Extensions shown (empty: all), as `("Markdown", ["md", "markdown"])`.
    pub filters: Vec<(String, Vec<String>)>,
    pub multiple: bool,
    pub directories: bool,
}

impl FileDialog {
    pub fn new() -> FileDialog {
        FileDialog::default()
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    pub fn directory(mut self, d: impl Into<PathBuf>) -> Self {
        self.directory = Some(d.into());
        self
    }

    pub fn file_name(mut self, n: impl Into<String>) -> Self {
        self.file_name = Some(n.into());
        self
    }

    pub fn filter(mut self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push((name.into(), extensions.iter().map(|e| e.to_string()).collect()));
        self
    }

    pub fn multiple(mut self) -> Self {
        self.multiple = true;
        self
    }

    /// Choose folders rather than files.
    pub fn directories(mut self) -> Self {
        self.directories = true;
        self
    }
}

/// A file panel's own messages.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PanelMsg {
    Go(PathBuf),
    Up,
    Select(Vec<usize>),
    Activate(usize),
    Name(String),
    Hidden(bool),
    Confirm,
    Cancel,
}

/// A file or folder in a panel's listing.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

pub(crate) struct PanelState {
    pub window: WinId,
    pub save: bool,
    pub dialog: FileDialog,
    pub done: MsgMap<Option<Vec<PathBuf>>>,
    pub dir: PathBuf,
    pub entries: std::sync::Arc<Vec<Entry>>,
    pub selected: Vec<usize>,
    pub name: String,
    pub hidden: bool,
    pub error: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The places in a panel's sidebar: home, its usual folders that are
/// there, and the root.
fn places() -> Vec<(String, PathBuf, Icon)> {
    let h = home();
    let mut out = vec![("Home".to_owned(), h.clone(), Icon::House)];
    for (name, icon) in [
        ("Desktop", Icon::Monitor),
        ("Documents", Icon::FileText),
        ("Downloads", Icon::Download),
        ("Pictures", Icon::Image),
        ("Music", Icon::EighthNotes),
        ("Videos", Icon::Film),
    ] {
        let p = h.join(name);
        if p.is_dir() {
            out.push((name.to_owned(), p, icon));
        }
    }
    out.push(("Computer".to_owned(), PathBuf::from("/"), Icon::HardDrive));
    out
}

impl PanelState {
    /// Read `dir` (folders first, then by name; hidden ones as asked;
    /// files by the filters).
    fn list(&mut self) {
        let mut entries = Vec::new();
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !self.hidden && name.starts_with('.') {
                        continue;
                    }
                    let meta = e.metadata().ok();
                    let dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false) || e.path().is_dir();
                    if !dir {
                        if self.dialog.directories {
                            continue;
                        }
                        let ext = std::path::Path::new(&name)
                            .extension()
                            .and_then(|x| x.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if !self.dialog.filters.is_empty()
                            && !self
                                .dialog
                                .filters
                                .iter()
                                .any(|(_, exts)| exts.iter().any(|x| x.eq_ignore_ascii_case(&ext)))
                        {
                            continue;
                        }
                    }
                    entries.push(Entry { name, dir, size: meta.map_or(0, |m| m.len()) });
                }
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{}: {e}", self.dir.display())),
        }
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        self.entries = std::sync::Arc::new(entries);
        self.selected.clear();
    }

    fn go(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.list();
    }

    /// What confirming chooses, if anything yet (a folder selected while
    /// choosing files opens it instead).
    fn chosen(&mut self) -> Option<Vec<PathBuf>> {
        if self.save {
            let name = self.name.trim();
            if name.is_empty() {
                return None;
            }
            return Some(vec![self.dir.join(name)]);
        }
        if self.selected.is_empty() {
            return self.dialog.directories.then(|| vec![self.dir.clone()]);
        }
        if self.invalid_file_selection() {
            return None;
        }
        if let [i] = self.selected[..]
            && self.entries[i].dir
            && !self.dialog.directories
        {
            let d = self.dir.join(&self.entries[i].name);
            self.go(d);
            return None;
        }
        Some(self.selected.iter().map(|&i| self.dir.join(&self.entries[i].name)).collect())
    }

    fn invalid_file_selection(&self) -> bool {
        !self.save
            && !self.dialog.directories
            && self.selected.len() > 1
            && self.selected.iter().any(|&i| self.entries[i].dir)
    }
}

pub(crate) fn show_panel(
    core: &mut Core,
    window: WindowKey,
    dialog: FileDialog,
    save: bool,
    done: MsgMap<Option<Vec<PathBuf>>>,
    platform: &mut dyn Platform,
) {
    let _ = platform;
    let Some(win) = core.window_of(window) else {
        core.queue.push_back(done.make(None));
        return;
    };
    let dir = dialog.directory.clone().filter(|d| d.is_dir()).unwrap_or_else(home);
    let name = dialog.file_name.clone().unwrap_or_default();
    let mut st = PanelState {
        window: win,
        save,
        dialog,
        done,
        dir,
        entries: Default::default(),
        selected: Vec::new(),
        name,
        hidden: false,
        error: None,
    };
    st.list();
    let id = core.fresh_id();
    core.panels.insert(id, st);
    if let Some(w) = core.wins.get_mut(&win) {
        w.panels.push(id);
        w.dirty = true;
    }
    core.pending_focus.push((None, if save { "panel-name".into() } else { "panel-files".into() }));
}

fn finish(core: &mut Core, panel: u64, result: Option<Vec<PathBuf>>) {
    let Some(p) = core.panels.remove(&panel) else { return };
    if let Some(w) = core.wins.get_mut(&p.window) {
        w.panels.retain(|x| *x != panel);
        w.dirty = true;
    }
    core.queue.push_back(p.done.make(result));
}

pub(crate) fn panel_message(core: &mut Core, panel: u64, msg: PanelMsg, platform: &mut dyn Platform) {
    let _ = platform;
    let Some(p) = core.panels.get_mut(&panel) else { return };
    let mut result = None;
    match msg {
        PanelMsg::Go(d) => p.go(d),
        PanelMsg::Up => {
            if let Some(parent) = p.dir.parent().map(PathBuf::from) {
                p.go(parent);
            }
        }
        PanelMsg::Select(s) => {
            p.selected = s;
            if p.save
                && let [i] = p.selected[..]
                && !p.entries[i].dir
            {
                p.name = p.entries[i].name.clone();
            }
        }
        PanelMsg::Activate(i) => {
            // Return activates the list cursor, which can be a row just
            // toggled off. Multiple-open must use the selection, even empty.
            // A pointer activation sends Select before Activate when needed.
            if !p.dialog.multiple || p.save {
                p.selected = vec![i];
            }
            result = p.chosen().map(Some);
        }
        PanelMsg::Name(n) => p.name = n,
        PanelMsg::Hidden(h) => {
            p.hidden = h;
            p.list();
        }
        PanelMsg::Confirm => result = p.chosen().map(Some),
        PanelMsg::Cancel => result = Some(None),
    }
    let window = p.window;
    match result {
        Some(r) => finish(core, panel, r),
        None => {
            if let Some(w) = core.wins.get_mut(&window) {
                w.dirty = true;
            }
        }
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u + 1 < UNITS.len() {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// A panel's sheet content: a path bar, the places, the folder's files,
/// a name to save as, and the buttons.
pub(crate) fn panel_element(core: &Core, id: u64) -> Option<RawElement> {
    use crate::widgets::panes::{caption, sidebar_item, sidebar_layout};
    use crate::widgets::scroll::virtual_list;
    let p = core.panels.get(&id)?;
    let m = move |msg: PanelMsg| KitMsg::Panel { panel: id, msg };
    let title = p.dialog.title.clone().unwrap_or_else(|| if p.save { "Save".into() } else { "Open".into() });
    // The path bar: each folder up to here.
    let mut crumbs: Vec<Element<KitMsg>> =
        vec![icon_button(Icon::ArrowUp).on_press(m(PanelMsg::Up)).tooltip("Enclosing folder").into()];
    let mut acc = PathBuf::new();
    let parts: Vec<_> = p.dir.components().collect();
    for (i, c) in parts.iter().enumerate() {
        acc.push(c);
        let label = match c {
            std::path::Component::RootDir => "/".to_owned(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        if i > 1 {
            crumbs.push(text("›").role(Role::Faint).into());
        }
        let mut b = button(label).toolbar().on_press(m(PanelMsg::Go(acc.clone()))).key(i);
        if i + 1 == parts.len() {
            b = b.selected(true);
        }
        crumbs.push(b.into());
    }
    let mut side: Vec<Element<KitMsg>> = vec![caption("Places")];
    for (name, path, icon) in places() {
        let sel = p.dir == path;
        side.push(sidebar_item(icon, name.clone(), sel).on_press(m(PanelMsg::Go(path))).key(name).into());
    }
    let entries = p.entries.clone();
    let files: Element<KitMsg> = if let Some(e) = &p.error {
        container(text(e.clone()).role(Role::Danger).wrap()).padding(12.0).width(Length::Fill).into()
    } else {
        let names = p.entries.clone();
        let mut list = virtual_list(entries.len(), core.theme.metrics.row, move |i| {
            let e = &entries[i];
            let icon = if e.dir { Icon::Folder } else { Icon::File };
            let size = if e.dir { String::new() } else { human(e.size) };
            crate::row![
                crate::icons::icon(icon).size(16.0),
                text(e.name.clone()),
                spacer(),
                text(size).mono().tabular().size(11.0).role(Role::Text3)
            ]
            .spacing(8.0)
            .padding([0.0, 10.0])
        });
        if p.dialog.multiple && !p.save {
            list = list.multiple();
        }
        list.selected(p.selected.iter().copied())
            .on_select(move |s| m(PanelMsg::Select(s)))
            .on_activate(move |i| KitMsg::Panel { panel: id, msg: PanelMsg::Activate(i) })
            .type_select(move |i| names[i].name.clone())
            .id("panel-files")
            .into()
    };
    let invalid_files = p.invalid_file_selection();
    let can = if p.save {
        !p.name.trim().is_empty()
    } else {
        (!p.selected.is_empty() || p.dialog.directories) && !invalid_files
    };
    let confirm = if p.save { "Save" } else { "Open" };
    let mut bottom: Vec<Element<KitMsg>> = Vec::new();
    if p.save {
        bottom.push(text("Save as").role(Role::Text2).into());
        bottom.push(
            text_field(&p.name)
                .on_change(move |s| KitMsg::Panel { panel: id, msg: PanelMsg::Name(s) })
                .on_submit(m(PanelMsg::Confirm))
                .id("panel-name")
                .width(Length::Fixed(240.0))
                .into(),
        );
    }
    bottom.push(
        checkbox("Show hidden files", p.hidden)
            .on_toggle(move |b| KitMsg::Panel { panel: id, msg: PanelMsg::Hidden(b) })
            .into(),
    );
    bottom.push(spacer().into());
    bottom.push(button("Cancel").cancel().on_press(m(PanelMsg::Cancel)).into());
    bottom.push(button(confirm).default().enabled(can).on_press(m(PanelMsg::Confirm)).into());
    let mut contents: Vec<Element<KitMsg>> = vec![
        row(vec![text(title).weight(aizome_theme::typeface::WEIGHT_SEMIBOLD).size(core.theme.metrics.title).into()])
            .padding([12.0, 16.0, 8.0, 16.0])
            .into(),
        container(row(crumbs).spacing(2.0)).padding([0.0, 12.0, 8.0, 12.0]).width(Length::Fill).into(),
        container(sidebar_layout(column(side), files).first_size(170.0))
            .edges(
                crate::widgets::layout::Edge::Line,
                crate::widgets::layout::Sides { top: true, right: false, bottom: true, left: false },
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    ];
    if invalid_files {
        contents.push(
            container(text("Select files, or open one folder.").role(Role::Danger).wrap())
                .padding([6.0, 16.0, 0.0, 16.0])
                .width(Length::Fill)
                .into(),
        );
    }
    contents.push(row(bottom).spacing(10.0).padding([10.0, 16.0, 12.0, 16.0]).width(Length::Fill).into());
    let body: Element<KitMsg> = column(contents).width(Length::Fixed(640.0)).height(Length::Fixed(420.0)).into();
    Some(body.into_raw())
}
