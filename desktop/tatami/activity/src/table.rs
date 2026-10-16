// SPDX-License-Identifier: MIT OR Apache-2.0
//! The process list: a column view over a tree of rows (applications with
//! their processes, every process, or processes under their parents),
//! sorted by any column and filtered by the search. Each view of the window
//! (CPU, Memory, …) shows its own columns. Rows update in place, and the
//! sorted column re-sorts at each reading, as in Activity Monitor; the
//! selection stays with its row.
use crate::{
    entry::{self, Data, Entry},
    model::{self, Kind, Lens},
};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Applications, each with its processes.
    Apps,
    /// Every process.
    All,
    /// Processes under their parents.
    Tree,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Apps => "apps",
            Mode::All => "all",
            Mode::Tree => "tree",
        }
    }

    pub fn from_id(id: &str) -> Mode {
        match id {
            "all" => Mode::All,
            "tree" => Mode::Tree,
            _ => Mode::Apps,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Cpu,
    CpuTime,
    Threads,
    Wakeups,
    Memory,
    Private,
    Resident,
    Shared,
    Swap,
    Power,
    Average,
    Energy,
    Gpu,
    Video,
    Vram,
    GpuShared,
    Read,
    Write,
    ReadTotal,
    WriteTotal,
    Pid,
    User,
}

use Column::*;

const COLUMNS: [Column; 23] = [
    Name, Cpu, CpuTime, Threads, Wakeups, Memory, Private, Resident, Shared, Swap, Power, Average,
    Energy, Gpu, Video, Vram, GpuShared, Read, Write, ReadTotal, WriteTotal, Pid, User,
];

impl Column {
    fn title(self) -> &'static str {
        match self {
            Name => "Name",
            Cpu => "% CPU",
            CpuTime => "CPU Time",
            Threads => "Threads",
            Wakeups => "Wake-ups",
            Memory => "Memory",
            Private => "Private",
            Resident => "Resident",
            Shared => "Shared",
            Swap => "Swap",
            Power => "Power",
            Average => "Average",
            Energy => "Energy Used",
            Gpu => "% GPU",
            Video => "% Video",
            Vram => "Video Memory",
            GpuShared => "Shared GPU",
            Read => "Reading",
            Write => "Writing",
            ReadTotal => "Read",
            WriteTotal => "Written",
            Pid => "PID",
            User => "User",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Cpu => "Share of one processor core; a busy application can pass 100",
            CpuTime => "Processor time used since it started",
            Wakeups => {
                "Times a second it stopped to wait and woke again (voluntary context switches): wake-ups keep the processor from resting"
            }
            Memory => {
                "Proportional set size: its own memory, plus its share of memory it shares with others. Where that can't be read, its resident size"
            }
            Private => "Memory no other process shares (unique set size)",
            Resident => {
                "All memory in RAM it can reach, shared libraries included (resident set size)"
            }
            Shared => "Memory it shares with other processes",
            Swap => "Memory moved out to swap",
            Power => {
                "Estimated: the measured power of the processor and graphics, shared by how much of each it uses"
            }
            Average => "Estimated power, averaged over the time the window has been open",
            Energy => "Estimated energy used while the window has been open",
            Gpu => "Busiest drawing or compute engine of the graphics processors it uses",
            Video => "Busiest video encoding or decoding engine",
            Vram => "Graphics memory it holds",
            GpuShared => "System memory mapped for the graphics processor",
            Read | Write => "Storage read or written each second",
            ReadTotal | WriteTotal => "Storage read or written since it started",
            _ => "",
        }
    }

    fn lenses(self) -> &'static [Lens] {
        match self {
            Name | Pid => &Lens::ALL,
            Cpu => &[Lens::Cpu, Lens::Energy],
            CpuTime | Threads | Wakeups | User => &[Lens::Cpu],
            Memory | Private | Resident | Shared | Swap => &[Lens::Memory],
            Power | Average | Energy => &[Lens::Energy],
            Gpu => &[Lens::Gpu, Lens::Energy],
            Video | Vram | GpuShared => &[Lens::Gpu],
            Read | Write | ReadTotal | WriteTotal => &[Lens::Disk],
        }
    }

    /// The column a view sorts by first.
    pub fn primary(lens: Lens) -> Column {
        match lens {
            Lens::Cpu => Cpu,
            Lens::Memory => Memory,
            Lens::Energy => Power,
            Lens::Gpu => Gpu,
            Lens::Disk => Read,
        }
    }

    /// What breaks a tie in this column: the same use over a longer time.
    fn secondary(self) -> Option<Column> {
        Some(match self {
            Cpu => CpuTime,
            Wakeups => Cpu,
            Memory | Private | Shared => Resident,
            Power | Average => Energy,
            Gpu | Video => Vram,
            Read => ReadTotal,
            Write => WriteTotal,
            _ => return None,
        })
    }

    fn numeric(self) -> bool {
        !matches!(self, Name | User)
    }

    fn width(self) -> i32 {
        match self {
            Name => 220,
            User => 84,
            Pid => 72,
            Threads | Cpu | Gpu | Video => 72,
            Vram | Energy => 104,
            _ => 88,
        }
    }

    /// The value to sort by; missing values sort last.
    fn value(self, data: &Data) -> f64 {
        let u = &data.usage;
        let some = |value: Option<f64>| value.unwrap_or(-1.0);
        match self {
            Name | User => 0.0,
            Cpu => u.cpu,
            CpuTime => u.cpu_time,
            Threads => u.threads as f64,
            Wakeups => u.wakeups,
            Memory => u.memory() as f64,
            Private => some(u.private.map(|v| v as f64)),
            Resident => u.resident as f64,
            Shared => some(u.shared.map(|v| v as f64)),
            Swap => u.swap as f64,
            Power => some(u.power),
            Average => some(u.average_power()),
            Energy => u.energy,
            Gpu => u.gpu,
            Video => u.video,
            Vram => u.vram as f64,
            GpuShared => u.gpu_shared as f64,
            Read => some(u.read_rate),
            Write => some(u.write_rate),
            ReadTotal => some(u.read_total.map(|v| v as f64)),
            WriteTotal => some(u.write_total.map(|v| v as f64)),
            Pid => data.pid.map_or(-1.0, f64::from),
        }
    }

    /// The text to show; `None` shows a dash.
    fn text(self, data: &Data) -> Option<String> {
        let u = &data.usage;
        // An application of several processes has no one PID or user.
        if matches!(self, Pid | User) && data.group && data.count > 1 {
            return Some(String::new());
        }
        Some(match self {
            Name => data.name.clone(),
            Cpu => model::percent(u.cpu),
            CpuTime => model::cpu_time(u.cpu_time),
            Threads => u.threads.to_string(),
            Wakeups => format!("{:.0}", u.wakeups),
            Memory => model::bytes(u.memory()),
            Private => model::bytes(u.private?),
            Resident => model::bytes(u.resident),
            Shared => model::bytes(u.shared?),
            Swap => model::bytes(u.swap),
            Power => model::watts(u.power?),
            Average => model::watts(u.average_power()?),
            Energy if u.measured > 0.0 => model::energy(u.energy),
            Energy => return None,
            Gpu => model::percent(u.gpu),
            Video => model::percent(u.video),
            Vram => model::bytes(u.vram),
            GpuShared => model::bytes(u.gpu_shared),
            Read => model::rate(u.read_rate?),
            Write => model::rate(u.write_rate?),
            ReadTotal => model::bytes(u.read_total?),
            WriteTotal => model::bytes(u.write_total?),
            Pid => data.pid?.to_string(),
            User => {
                if data.user.is_empty() {
                    return None;
                }
                data.user.clone()
            }
        })
    }
}

/// What the filter shows.
#[derive(Default)]
pub struct Filter {
    pub query: String,
    pub mode: Option<Mode>,
    pub kernel: bool,
    pub everyone: bool,
    pub uid: u32,
}

impl Filter {
    fn matches(&self, data: &Data) -> bool {
        self.query.is_empty() || data.haystack.contains(&self.query)
    }

    /// Kernel threads show under the Kernel application, or everywhere when
    /// asked for; other users' processes unless hidden.
    fn allowed(&self, data: &Data) -> bool {
        let thread = !data.group && data.process.as_ref().is_some_and(|p| p.kernel);
        let kernel = thread || data.kind == Some(Kind::Kernel);
        if thread && !self.kernel && self.mode != Some(Mode::Apps) {
            return false;
        }
        let theirs = if data.group {
            data.kind == Some(Kind::System)
        } else {
            data.uid != self.uid
        };
        kernel || self.everyone || !theirs
    }
}

pub struct Table {
    pub view: gtk::ColumnView,
    pub selection: gtk::SingleSelection,
    pub filter: Rc<RefCell<Filter>>,
    custom_filter: gtk::CustomFilter,
    sort_model: gtk::SortListModel,
    sorters: RefCell<Vec<(Column, gtk::CustomSorter)>>,
    columns: RefCell<Vec<(Column, gtk::ColumnViewColumn)>>,
    lens: Cell<Lens>,
    /// Each view's sort, as last chosen.
    sorts: RefCell<std::collections::HashMap<Lens, (Column, gtk::SortType)>>,
    /// Set while the columns are swapped for another view.
    swapping: Cell<bool>,
    menu: gtk::PopoverMenu,
    /// The rows' expansion is lost when the root changes; the tree model
    /// is kept to know its mode.
    mode: Cell<Mode>,
}

fn row_entry(item: &glib::Object) -> Option<(gtk::TreeListRow, Entry)> {
    let row = item.downcast_ref::<gtk::TreeListRow>()?;
    let entry = row.item().and_downcast::<Entry>()?;
    Some((row.clone(), entry))
}

/// The handler a bound cell keeps on its row, to let go when unbound.
const HANDLER: &str = "tatami-activity-handler";

fn keep(widget: &impl IsA<glib::Object>, entry: &Entry, handler: glib::SignalHandlerId) {
    // A cell bound again without being unbound lets go of its old row.
    release(widget);
    // SAFETY: only (Entry, SignalHandlerId) is ever stored under HANDLER.
    unsafe {
        widget.as_ref().set_data(HANDLER, (entry.clone(), handler));
    }
}

fn release(widget: &impl IsA<glib::Object>) {
    // SAFETY: as in `keep`.
    let kept = unsafe {
        widget
            .as_ref()
            .steal_data::<(Entry, glib::SignalHandlerId)>(HANDLER)
    };
    if let Some((entry, handler)) = kept {
        entry.disconnect(handler);
    }
}

fn fallback_icon(data: &Data) -> &'static str {
    match data.kind {
        Some(Kind::Kernel) => "computer-symbolic",
        Some(Kind::System) | Some(Kind::Service) => "emblem-system-symbolic",
        _ => "application-x-executable-symbolic",
    }
}

impl Table {
    pub fn new(menu: &gio::MenuModel) -> Rc<Table> {
        let filter = Rc::new(RefCell::new(Filter {
            everyone: true,
            mode: Some(Mode::Apps),
            ..Default::default()
        }));
        let shown = filter.clone();
        let custom_filter = gtk::CustomFilter::new(move |item| {
            let Some((row, entry)) = row_entry(item) else {
                return false;
            };
            let filter = shown.borrow();
            let data = entry.data();
            if !filter.allowed(&data) {
                return false;
            }
            if filter.query.is_empty() || filter.matches(&data) {
                return true;
            }
            // A row whose parent matches, or whose children do.
            let parent_matches = row
                .parent()
                .and_then(|parent| parent.item().and_downcast::<Entry>())
                .is_some_and(|parent| filter.matches(&parent.data()));
            let child_matches = || {
                let children = entry.children();
                (0..children.n_items()).any(|i| {
                    children
                        .item(i)
                        .and_downcast::<Entry>()
                        .is_some_and(|child| filter.matches(&child.data()))
                })
            };
            parent_matches || (entry.has_children() && child_matches())
        });
        let sort_model = gtk::SortListModel::new(None::<gio::ListModel>, None::<gtk::Sorter>);
        let filtered =
            gtk::FilterListModel::new(Some(sort_model.clone()), Some(custom_filter.clone()));
        let selection = gtk::SingleSelection::builder()
            .model(&filtered)
            .autoselect(false)
            .can_unselect(true)
            .build();
        let view = gtk::ColumnView::builder()
            .model(&selection)
            .show_column_separators(false)
            .show_row_separators(false)
            .reorderable(false)
            .tab_behavior(gtk::ListTabBehavior::Item)
            .css_classes(["processes"])
            .build();
        let popover = gtk::PopoverMenu::from_model(Some(menu));
        popover.set_parent(&view);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);

        let table = Rc::new(Table {
            view: view.clone(),
            selection,
            filter,
            custom_filter,
            sort_model: sort_model.clone(),
            sorters: RefCell::default(),
            columns: RefCell::default(),
            lens: Cell::new(Lens::Cpu),
            sorts: RefCell::default(),
            swapping: Cell::new(false),
            menu: popover,
            mode: Cell::new(Mode::Apps),
        });
        let mut sorters = Vec::new();
        let mut columns = Vec::new();
        for column in COLUMNS {
            let factory = gtk::SignalListItemFactory::new();
            let weak = Rc::downgrade(&table);
            factory.connect_setup(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let cell: gtk::Widget = if column == Name {
                    name_cell().upcast()
                } else {
                    let label = gtk::Label::builder()
                        .xalign(if column.numeric() { 1.0 } else { 0.0 })
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .build();
                    if column.numeric() {
                        label.add_css_class("numeric");
                    }
                    label.upcast()
                };
                // Right-click selects the row and opens its menu.
                let click = gtk::GestureClick::builder()
                    .button(gdk::BUTTON_SECONDARY)
                    .build();
                let weak = weak.clone();
                let list_item = item.downgrade();
                click.connect_pressed(move |gesture, _, x, y| {
                    let (Some(table), Some(item)) = (weak.upgrade(), list_item.upgrade()) else {
                        return;
                    };
                    table.selection.set_selected(item.position());
                    if let Some(widget) = gesture.widget()
                        && let Some(point) = widget.compute_point(
                            &table.view,
                            &gtk::graphene::Point::new(x as f32, y as f32),
                        )
                    {
                        table.menu.set_pointing_to(Some(&gdk::Rectangle::new(
                            point.x() as i32,
                            point.y() as i32,
                            1,
                            1,
                        )));
                        table.menu.popup();
                    }
                });
                cell.add_controller(click);
                item.set_child(Some(&cell));
            });
            let weak = Rc::downgrade(&table);
            factory.connect_bind(move |_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                    return;
                };
                let (Some((row, entry)), Some(cell)) =
                    (item.item().as_ref().and_then(row_entry), item.child())
                else {
                    return;
                };
                let mode = weak.upgrade().map_or(Mode::Apps, |table| table.mode.get());
                if column == Name {
                    let Some(expander) = cell.downcast_ref::<gtk::TreeExpander>() else {
                        return;
                    };
                    expander.set_list_row(Some(&row));
                    let depth = row.depth();
                    let draw = {
                        let expander = expander.clone();
                        let row = row.clone();
                        move |entry: &Entry| fill_name(&expander, &row, entry, depth, mode)
                    };
                    draw(&entry);
                    let handler = entry.connect_changed(draw);
                    keep(&cell, &entry, handler);
                } else {
                    let Some(label) = cell.downcast_ref::<gtk::Label>() else {
                        return;
                    };
                    let draw = {
                        let label = label.clone();
                        move |entry: &Entry| fill_value(&label, column, &entry.data())
                    };
                    draw(&entry);
                    let handler = entry.connect_changed(draw);
                    keep(&cell, &entry, handler);
                }
            });
            factory.connect_unbind(move |_, item| {
                if let Some(cell) = item
                    .downcast_ref::<gtk::ListItem>()
                    .and_then(|item| item.child())
                {
                    release(&cell);
                    if let Some(expander) = cell.downcast_ref::<gtk::TreeExpander>() {
                        expander.set_list_row(None);
                    }
                }
            });
            let sorter = gtk::CustomSorter::new(move |a, b| {
                let (Some(a), Some(b)) = (a.downcast_ref::<Entry>(), b.downcast_ref::<Entry>())
                else {
                    return gtk::Ordering::Equal;
                };
                let (a, b) = (a.data(), b.data());
                let order = match column {
                    Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    User => a.user.cmp(&b.user),
                    // Equal now (often nothing): what used more over time.
                    _ => {
                        let by = |column: Column| column.value(&a).total_cmp(&column.value(&b));
                        let order = by(column);
                        match column.secondary() {
                            Some(next) => order.then_with(|| by(next)),
                            None => order,
                        }
                    }
                };
                // Ties in a stable order, so rows don't swap places.
                order.then_with(|| a.key.cmp(&b.key)).into()
            });
            let view_column = gtk::ColumnViewColumn::builder()
                .title(column.title())
                .factory(&factory)
                .sorter(&sorter)
                .resizable(true)
                .build();
            if column == Name {
                view_column.set_expand(true);
            }
            view_column.set_fixed_width(column.width());
            view.append_column(&view_column);
            sorters.push((column, sorter));
            columns.push((column, view_column));
        }
        *table.sorters.borrow_mut() = sorters;
        *table.columns.borrow_mut() = columns;
        // Tooltips on the headers explain the less familiar columns.
        table.set_header_tooltips();
        // Sorting by a number puts the largest first, as people look for
        // what uses the most.
        let row_sorter = gtk::TreeListRowSorter::new(view.sorter());
        sort_model.set_sorter(Some(&row_sorter));
        if let Some(sorter) = view.sorter().and_downcast::<gtk::ColumnViewSorter>() {
            let last: Rc<RefCell<Option<gtk::ColumnViewColumn>>> = Rc::default();
            let weak = Rc::downgrade(&table);
            sorter.connect_changed(move |sorter, _| {
                let Some(table) = weak.upgrade() else {
                    return;
                };
                if table.swapping.get() {
                    return;
                }
                let primary = sorter.primary_sort_column();
                let changed = *last.borrow() != primary;
                *last.borrow_mut() = primary.clone();
                let column = primary
                    .as_ref()
                    .and_then(|primary| table.column_of(primary));
                if let Some(column) = column {
                    table
                        .sorts
                        .borrow_mut()
                        .insert(table.lens.get(), (column, sorter.primary_sort_order()));
                }
                if let Some(primary) = primary
                    && changed
                    && sorter.primary_sort_order() == gtk::SortType::Ascending
                    && column.is_some_and(Column::numeric)
                {
                    let view = table.view.clone();
                    glib::idle_add_local_once(move || {
                        view.sort_by_column(Some(&primary), gtk::SortType::Descending);
                    });
                }
            });
        }
        table
    }

    /// Tooltips on the column headers, found by their titles: the column
    /// view has no property for them.
    fn set_header_tooltips(&self) {
        fn walk(widget: &gtk::Widget, tip: &dyn Fn(&str) -> Option<&'static str>) {
            let mut child = widget.first_child();
            while let Some(current) = child {
                if let Some(label) = current.downcast_ref::<gtk::Label>()
                    && let Some(text) = tip(&label.label())
                    && let Some(title) = label.parent()
                {
                    title.set_tooltip_text(Some(text));
                }
                walk(&current, tip);
                child = current.next_sibling();
            }
        }
        let tip = |title: &str| {
            COLUMNS
                .iter()
                .find(|column| column.title() == title)
                .map(|column| column.tooltip())
                .filter(|tip| !tip.is_empty())
        };
        walk(self.view.upcast_ref(), &tip);
    }

    fn column_of(&self, view_column: &gtk::ColumnViewColumn) -> Option<Column> {
        self.columns
            .borrow()
            .iter()
            .find(|(_, column)| column == view_column)
            .map(|(column, _)| *column)
    }

    /// Show these rows: applications, every process, or the tree, which
    /// opens fully expanded as htop's does.
    pub fn set_root(&self, root: &gio::ListStore, mode: Mode) {
        self.mode.set(mode);
        self.filter.borrow_mut().mode = Some(mode);
        let tree = gtk::TreeListModel::new(root.clone(), false, mode == Mode::Tree, move |item| {
            let entry = item.downcast_ref::<Entry>()?;
            let expandable = match mode {
                Mode::Apps => entry.is_group() && entry.data().count > 1,
                Mode::All => false,
                Mode::Tree => true,
            };
            expandable.then(|| entry.children().upcast())
        });
        let selected = self.selected().map(|entry| entry.key());
        self.sort_model.set_model(Some(&tree));
        self.custom_filter.changed(gtk::FilterChange::Different);
        // The selection stays in view; without one, the top.
        if !selected.is_some_and(|key| self.select(&key)) {
            self.to_top();
        }
    }

    /// Show the view's columns. They are taken out and put back rather than
    /// hidden: hidden columns keep their cells bound and updating.
    pub fn set_lens(&self, lens: Lens) {
        self.swapping.set(true);
        self.lens.set(lens);
        let columns = self.columns.borrow();
        let shown = self.view.columns();
        while let Some(column) = shown.item(0).and_downcast::<gtk::ColumnViewColumn>() {
            self.view.remove_column(&column);
        }
        for (column, view_column) in columns.iter() {
            if column.lenses().contains(&lens) {
                self.view.append_column(view_column);
            }
        }
        // The view's own sort, or its main column, largest first.
        let (sort, order) = self
            .sorts
            .borrow()
            .get(&lens)
            .copied()
            .unwrap_or((Column::primary(lens), gtk::SortType::Descending));
        if let Some((_, view_column)) = columns.iter().find(|(column, _)| *column == sort) {
            self.view.sort_by_column(Some(view_column), order);
        }
        drop(columns);
        self.swapping.set(false);
        self.set_header_tooltips();
        // Another view starts from its busiest.
        self.to_top();
    }

    /// Sort again after a reading changed the values.
    pub fn resort(&self) {
        let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>() else {
            return;
        };
        let Some(primary) = sorter.primary_sort_column() else {
            return;
        };
        if let Some(column) = self.column_of(&primary) {
            let custom = self
                .sorters
                .borrow()
                .iter()
                .find(|(c, _)| *c == column)
                .map(|(_, custom)| custom.clone());
            if let Some(custom) = custom {
                // The list keeps its top row in place as rows move: one
                // scrolled to the top stays there, with the busiest first.
                let at_top = self.at_top();
                custom.changed(gtk::SorterChange::Different);
                if at_top {
                    self.to_top();
                }
            }
        }
    }

    fn at_top(&self) -> bool {
        self.view
            .vadjustment()
            .is_none_or(|adjustment| adjustment.value() <= 1.0)
    }

    fn to_top(&self) {
        if self.selection.n_items() > 0 {
            self.view
                .scroll_to(0, None, gtk::ListScrollFlags::NONE, None);
        }
    }

    pub fn refilter(&self) {
        self.custom_filter.changed(gtk::FilterChange::Different);
        // Show the processes that match inside their applications.
        if !self.filter.borrow().query.is_empty() {
            let model = self.selection.model();
            if let Some(model) = model {
                let mut i = 0;
                while i < model.n_items() {
                    if let Some(row) = model.item(i).and_downcast::<gtk::TreeListRow>()
                        && row.is_expandable()
                        && !row.is_expanded()
                        && row.depth() == 0
                    {
                        row.set_expanded(true);
                    }
                    i += 1;
                }
            }
        }
        self.to_top();
    }

    pub fn selected(&self) -> Option<Entry> {
        let row = self
            .selection
            .selected_item()
            .and_downcast::<gtk::TreeListRow>()?;
        row.item().and_downcast::<Entry>()
    }

    /// Select the row with this key, if it is showing.
    pub fn select(&self, key: &str) -> bool {
        let Some(model) = self.selection.model() else {
            return false;
        };
        for i in 0..model.n_items() {
            if let Some((_, entry)) = model.item(i).as_ref().and_then(row_entry)
                && entry.key() == key
            {
                self.selection.set_selected(i);
                self.view
                    .scroll_to(i, None, gtk::ListScrollFlags::FOCUS, None);
                return true;
            }
        }
        false
    }

    /// Open the row with this key, if it is showing and has rows under it.
    pub fn expand(&self, key: &str) -> bool {
        let Some(model) = self.selection.model() else {
            return false;
        };
        for i in 0..model.n_items() {
            if let Some((row, entry)) = model.item(i).as_ref().and_then(row_entry)
                && entry.key() == key
            {
                if row.is_expandable() {
                    row.set_expanded(true);
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn popup_menu(&self) {
        let position = self.selection.selected();
        if position == gtk::INVALID_LIST_POSITION {
            return;
        }
        self.menu
            .set_pointing_to(Some(&gdk::Rectangle::new(24, 48, 1, 1)));
        self.menu.popup();
    }
}

fn name_cell() -> gtk::TreeExpander {
    let icon = gtk::Image::builder().pixel_size(16).build();
    let name = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let detail = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label", "row-detail"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&icon);
    content.append(&name);
    content.append(&detail);
    let expander = gtk::TreeExpander::new();
    expander.set_child(Some(&content));
    expander.set_indent_for_icon(true);
    expander
}

fn fill_name(
    expander: &gtk::TreeExpander,
    row: &gtk::TreeListRow,
    entry: &Entry,
    depth: u32,
    mode: Mode,
) {
    let Some(content) = expander.child() else {
        return;
    };
    let icon = content.first_child().and_downcast::<gtk::Image>();
    let name = icon
        .as_ref()
        .and_then(|i| i.next_sibling())
        .and_downcast::<gtk::Label>();
    let detail = name
        .as_ref()
        .and_then(|n| n.next_sibling())
        .and_downcast::<gtk::Label>();
    let (Some(icon), Some(name), Some(detail)) = (icon, name, detail) else {
        return;
    };
    let data = entry.data();
    name.set_label(&data.name);
    let secondary = if data.group && data.count > 1 {
        data.count.to_string()
    } else {
        data.detail.clone()
    };
    // A group's window title is for the inspector; here it would crowd.
    let secondary = if data.group && data.count <= 1 {
        String::new()
    } else {
        secondary
    };
    detail.set_label(&secondary);
    detail.set_visible(!secondary.is_empty());
    let nested = depth > 0 && mode == Mode::Apps;
    icon.set_visible(!nested);
    // Applications always have an icon; a process has its application's,
    // or keeps the space so names line up.
    let gicon = entry::gicon(&data.icon);
    icon.set_opacity(if gicon.is_some() || data.group {
        1.0
    } else {
        0.0
    });
    match gicon {
        Some(gicon) if !nested => icon.set_from_gicon(&gicon),
        _ => icon.set_icon_name(Some(fallback_icon(&data))),
    }
    let paused = data.process.as_ref().is_some_and(|p| p.state == 'T');
    name.set_css_classes(if paused { &["paused"] } else { &[] });
    // The tree's arrows show only where there is something to open.
    let empty = mode == Mode::Tree && !entry.has_children();
    expander.set_hide_expander(empty || !row.is_expandable());
}

/// A value, dimmed when it is missing or nothing, so what is busy stands out.
fn fill_value(label: &gtk::Label, column: Column, data: &Data) {
    let text = column.text(data);
    let idle = column.numeric() && column != Pid && column.value(data) <= 0.0;
    label.set_label(text.as_deref().unwrap_or("—"));
    if text.is_none() || idle {
        label.add_css_class("dim-label");
    } else {
        label.remove_css_class("dim-label");
    }
}
