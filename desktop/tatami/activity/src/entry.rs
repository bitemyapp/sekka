// SPDX-License-Identifier: MIT OR Apache-2.0
//! A row of the process list: an application (a group) or a process. Rows
//! live as long as what they show, and are updated in place at each
//! reading, so the list keeps its selection, expanded rows and scroll
//! position; each one says "changed" and its cells redraw. A row keeps a
//! few minutes of its own history for the inspector's graphs.
use crate::model::{Group, Icon, Kind, Process, Usage};
use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use std::{
    cell::{Ref, RefCell},
    collections::VecDeque,
};

/// Readings of history kept: five minutes at the usual two seconds.
pub const HISTORY: usize = 150;

#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub cpu: f64,
    pub memory: f64,
    pub power: f64,
    pub gpu: f64,
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Default)]
pub struct Data {
    pub key: String,
    pub group: bool,
    pub kind: Option<Kind>,
    pub name: String,
    /// A process's role, or an application's window title or unit.
    pub detail: String,
    pub icon: Icon,
    /// The process, or the only process of a one-process group.
    pub pid: Option<i32>,
    pub user: String,
    pub uid: u32,
    pub count: usize,
    pub usage: Usage,
    pub process: Option<Process>,
    pub members: Vec<i32>,
    pub windows: usize,
    /// What search matches against, lowercased.
    pub haystack: String,
    pub history: VecDeque<Point>,
}

mod imp {
    use super::*;
    use std::sync::OnceLock;

    #[derive(Default)]
    pub struct Entry {
        pub data: RefCell<Data>,
        pub children: RefCell<Option<gio::ListStore>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Entry {
        const NAME: &'static str = "TatamiActivityEntry";
        type Type = super::Entry;
    }

    impl ObjectImpl for Entry {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("changed").build()])
        }
    }
}

glib::wrapper! {
    pub struct Entry(ObjectSubclass<imp::Entry>);
}

impl Entry {
    pub fn new(key: &str, group: bool) -> Entry {
        let entry: Entry = glib::Object::new();
        {
            let mut data = entry.imp().data.borrow_mut();
            data.key = key.to_owned();
            data.group = group;
        }
        entry
    }

    pub fn data(&self) -> Ref<'_, Data> {
        self.imp().data.borrow()
    }

    pub fn key(&self) -> String {
        self.data().key.clone()
    }

    pub fn is_group(&self) -> bool {
        self.data().group
    }

    /// The rows under this one: a group's processes, or a process's
    /// children in the tree.
    pub fn children(&self) -> gio::ListStore {
        self.imp()
            .children
            .borrow_mut()
            .get_or_insert_with(gio::ListStore::new::<Entry>)
            .clone()
    }

    pub fn has_children(&self) -> bool {
        self.imp()
            .children
            .borrow()
            .as_ref()
            .is_some_and(|children| children.n_items() > 0)
    }

    fn record(data: &mut Data) {
        let usage = &data.usage;
        data.history.push_back(Point {
            cpu: usage.cpu,
            memory: usage.memory() as f64,
            power: usage.power.unwrap_or(0.0),
            gpu: usage.gpu,
            read: usage.read_rate.unwrap_or(0.0),
            write: usage.write_rate.unwrap_or(0.0),
        });
        while data.history.len() > HISTORY {
            data.history.pop_front();
        }
    }

    pub fn update_process(&self, process: &Process, icon: &Icon) {
        {
            let mut data = self.imp().data.borrow_mut();
            data.kind = None;
            data.name = process.name.clone();
            data.detail = process.role.clone();
            data.icon = icon.clone();
            data.pid = Some(process.pid);
            data.user = process.user.clone();
            data.uid = process.uid;
            data.count = 1;
            data.usage = process.usage.clone();
            data.members = vec![process.pid];
            if data.haystack.is_empty()
                || data
                    .process
                    .as_ref()
                    .is_none_or(|p| p.command != process.command)
            {
                data.haystack = format!(
                    "{} {} {} {} {} {}",
                    process.name,
                    process.role,
                    process.command,
                    process.pid,
                    process.user,
                    process.unit
                )
                .to_lowercase();
            }
            data.process = Some(process.clone());
            Self::record(&mut data);
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    pub fn update_group(&self, group: &Group, only: Option<&Process>) {
        {
            let mut data = self.imp().data.borrow_mut();
            data.kind = group.kind;
            data.name = group.name.clone();
            data.detail = group.detail.clone();
            data.icon = group.icon.clone();
            data.pid = only.map(|p| p.pid);
            data.user = only.map(|p| p.user.clone()).unwrap_or_default();
            data.uid = only.map_or(u32::MAX, |p| p.uid);
            data.count = group.members.len();
            data.usage = group.usage.clone();
            data.members = group.members.clone();
            data.windows = group.windows;
            data.process = only.cloned();
            data.haystack = format!(
                "{} {} {}",
                group.name,
                group.detail,
                only.map(|p| format!("{} {} {}", p.command, p.pid, p.user))
                    .unwrap_or_default()
            )
            .to_lowercase();
            Self::record(&mut data);
        }
        self.emit_by_name::<()>("changed", &[]);
    }

    /// Call `changed` whenever the row is updated.
    pub fn connect_changed(&self, changed: impl Fn(&Entry) + 'static) -> glib::SignalHandlerId {
        self.connect_local("changed", false, move |values| {
            if let Some(entry) = values.first().and_then(|value| value.get::<Entry>().ok()) {
                changed(&entry);
            }
            None
        })
    }
}

/// The icon to show, as GIO wants it.
pub fn gicon(icon: &Icon) -> Option<gio::Icon> {
    match icon {
        Icon::None => None,
        Icon::Named(name) => Some(gio::ThemedIcon::new(name).upcast()),
        Icon::File(path) => Some(gio::FileIcon::new(&gio::File::for_path(path)).upcast()),
    }
}

/// Keep `store` holding `wanted`, in any order, moving nothing that stays.
pub fn sync(store: &gio::ListStore, wanted: &[Entry]) {
    let keys: std::collections::HashSet<String> = wanted.iter().map(Entry::key).collect();
    let mut present = std::collections::HashSet::new();
    let mut index = 0;
    while index < store.n_items() {
        let key = store
            .item(index)
            .and_downcast::<Entry>()
            .map(|entry| entry.key())
            .unwrap_or_default();
        if keys.contains(&key) {
            present.insert(key);
            index += 1;
        } else {
            // Runs of rows that went together go in one change.
            let mut end = index + 1;
            while end < store.n_items()
                && store
                    .item(end)
                    .and_downcast::<Entry>()
                    .is_some_and(|entry| !keys.contains(&entry.key()))
            {
                end += 1;
            }
            store.splice(index, end - index, &[] as &[glib::Object]);
        }
    }
    let added: Vec<Entry> = wanted
        .iter()
        .filter(|entry| !present.contains(&entry.key()))
        .cloned()
        .collect();
    if !added.is_empty() {
        store.extend_from_slice(&added);
    }
}
