//! A disk as the partition map and the table show it: its partitions,
//! extended partitions and the unallocated space between them, by offset,
//! as it is now ([`current`]) or as it will be once the pending
//! operations have run (see [`crate::plan::Plan::preview`]).

use aizome_sys::disks::types::{flag_names, flags as bits};
use aizome_sys::disks::{BlockDevice, DisksState, Holds, Identity};

use crate::app::Opening;

pub const MIB: u64 = 1 << 20;

/// What a segment refers to: a block device UDisks has (by object path),
/// or a partition a pending operation will make (by the operation's id).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Block(String),
    New(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegKind {
    Partition,
    /// A DOS extended partition, holding logical ones.
    Extended,
    /// Unallocated space where a partition can go.
    Free,
    /// A filesystem (or encryption) on the whole disk, with no partition
    /// table: a "superfloppy", an image.
    Whole,
    /// A disk with neither a partition table nor a filesystem.
    Blank,
}

/// What a pending operation does to a segment, for the map's and the
/// table's marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    New,
    Format,
    Resize,
    Label,
    Flags,
    Repair,
}

/// An encrypted volume's state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Crypt {
    /// The unlocked device, when unlocked.
    pub unlocked: Option<Inner>,
}

/// What an unlocked encrypted volume holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Inner {
    pub path: String,
    pub device: String,
    pub fs: String,
    pub label: String,
    pub mount: Option<String>,
    pub used: Option<u64>,
    pub size: u64,
}

/// A stretch of the disk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Seg {
    pub kind: Option<SegKind>,
    pub offset: u64,
    pub size: u64,
    pub target: Option<Target>,
    /// `/dev/sdb1`; empty for unallocated space.
    pub device: String,
    pub number: u32,
    /// The filesystem (UDisks's `IdType`): `ext4`, `crypto_LUKS`, `swap`…
    pub fs: String,
    pub label: String,
    /// The GPT partition name.
    pub name: String,
    pub mount: Option<String>,
    /// Bytes in use, when known (mounted filesystems).
    pub used: Option<u64>,
    pub type_id: String,
    pub flag_bits: u64,
    pub crypt: Option<Crypt>,
    pub pending: Vec<Mark>,
    /// Inside an extended partition (a logical partition, or free space
    /// for one).
    pub logical: bool,
    /// Mounted, active swap, or unlocked with something mounted inside.
    pub in_use: bool,
    pub read_only: bool,
}

impl Seg {
    pub fn kind(&self) -> SegKind {
        self.kind.unwrap_or(SegKind::Free)
    }

    pub fn is_free(&self) -> bool {
        self.kind() == SegKind::Free
    }

    pub fn end(&self) -> u64 {
        self.offset + self.size
    }

    pub fn block(&self) -> Option<&str> {
        match &self.target {
            Some(Target::Block(p)) => Some(p),
            _ => None,
        }
    }

    /// The filesystem that shows: an unlocked volume's inner one.
    pub fn shown_fs(&self) -> &str {
        match self.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) {
            Some(inner) if !inner.fs.is_empty() => &inner.fs,
            _ => &self.fs,
        }
    }

    pub fn shown_label(&self) -> &str {
        match self.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) {
            Some(inner) => &inner.label,
            None => &self.label,
        }
    }

    pub fn shown_mount(&self) -> Option<&str> {
        match self.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) {
            Some(inner) => inner.mount.as_deref(),
            None => self.mount.as_deref(),
        }
    }

    pub fn shown_used(&self) -> Option<u64> {
        match self.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) {
            Some(inner) => inner.used,
            None => self.used,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.crypt.as_ref().is_some_and(|c| c.unlocked.is_none())
    }

    pub fn is_unlocked(&self) -> bool {
        self.crypt.as_ref().is_some_and(|c| c.unlocked.is_some())
    }

    /// The short name: `sdb1`, `nvme0n1p2`, or what it is.
    pub fn short_name(&self) -> String {
        match self.kind() {
            SegKind::Free => "Unallocated".into(),
            SegKind::Blank => "No partition table".into(),
            _ if self.device.is_empty() => "New partition".into(),
            _ => self.device.trim_start_matches("/dev/").to_owned(),
        }
    }

    /// The flags by name, as GParted names them (and UDisks's others).
    pub fn flag_list(&self, table: &str) -> Vec<String> {
        if !matches!(self.kind(), SegKind::Partition | SegKind::Extended) {
            return Vec::new();
        }
        let f = Flags::of(table, &self.type_id, self.flag_bits);
        let mut out = Vec::new();
        if f.esp {
            out.push("esp".to_owned());
        }
        if f.bios_grub {
            out.push("bios_grub".to_owned());
        }
        if f.boot {
            out.push("boot".to_owned());
        }
        if f.hidden {
            out.push("hidden".to_owned());
        }
        for name in flag_names(table, self.flag_bits) {
            if !matches!(name, "legacy BIOS boot" | "hidden" | "boot") {
                out.push(name.to_owned());
            }
        }
        if self.kind() == SegKind::Extended {
            out.push("extended".to_owned());
        }
        out
    }
}

/// A disk's segments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Layout {
    /// The disk's block device (object path).
    pub disk: String,
    pub device: String,
    pub size: u64,
    /// `gpt`, `dos`, or empty without a table.
    pub table: String,
    pub segs: Vec<Seg>,
    /// A new partition table is pending.
    pub new_table: bool,
}

impl Layout {
    /// The segment for a target.
    pub fn find(&self, target: &Target) -> Option<usize> {
        self.segs.iter().position(|s| s.target.as_ref() == Some(target))
    }

    pub fn partitions(&self) -> impl Iterator<Item = &Seg> {
        self.segs.iter().filter(|s| matches!(s.kind(), SegKind::Partition | SegKind::Extended))
    }

    /// Unallocated bytes, aligned as partitions would use them.
    pub fn free_bytes(&self) -> u64 {
        self.segs.iter().filter(|s| s.is_free()).map(|s| s.size).sum()
    }

    /// Free space right after a partition (within 1 MiB of its end), for
    /// growing it.
    pub fn room_after(&self, i: usize) -> u64 {
        let Some(seg) = self.segs.get(i) else { return 0 };
        self.segs
            .iter()
            .filter(|f| f.is_free() && f.logical == seg.logical)
            .find(|f| f.offset >= seg.end() && f.offset <= seg.end() + MIB)
            .map_or(0, |f| f.end() - seg.end())
    }

    /// How many primary partitions a DOS table has (it holds four).
    pub fn primaries(&self) -> usize {
        self.partitions().filter(|s| !s.logical).count()
    }

    pub fn has_extended(&self) -> bool {
        self.segs.iter().any(|s| s.kind() == SegKind::Extended)
    }

    /// Replace the unallocated segments with the gaps between partitions
    /// (aligned to 1 MiB and at least 1 MiB long, the first MiB and GPT's
    /// backup header left out), as UDisks's `free_space` reckons them.
    pub fn fill_free(&mut self) {
        if self.table.is_empty() {
            return;
        }
        self.segs.retain(|s| !s.is_free());
        self.segs.sort_by_key(|s| (s.offset, s.logical));
        let end = if self.table == "gpt" { self.size.saturating_sub(33 * 512) } else { self.size };
        let primary: Vec<(u64, u64)> = self.segs.iter().filter(|s| !s.logical).map(|s| (s.offset, s.end())).collect();
        let mut free = gaps(MIB, end, &primary, false);
        for container in self.segs.iter().filter(|s| s.kind() == SegKind::Extended) {
            let logical: Vec<(u64, u64)> = self
                .segs
                .iter()
                .filter(|s| s.logical && s.offset >= container.offset && s.offset < container.end())
                // Each logical partition has its EBR in the MiB before it.
                .map(|s| (s.offset.saturating_sub(MIB), s.end()))
                .collect();
            free.extend(gaps(container.offset + MIB, container.end(), &logical, true));
        }
        self.segs.extend(free);
        self.segs.sort_by_key(|s| (s.offset, s.logical, s.is_free()));
    }
}

fn gaps(start: u64, end: u64, used: &[(u64, u64)], logical: bool) -> Vec<Seg> {
    let mut used = used.to_vec();
    used.sort();
    let mut free = Vec::new();
    let mut cursor = start;
    let mut push = |from: u64, to: u64| {
        let from = from.div_ceil(MIB) * MIB;
        let to = to / MIB * MIB;
        if to > from && to - from >= MIB {
            free.push(Seg { kind: Some(SegKind::Free), offset: from, size: to - from, logical, ..Default::default() });
        }
    };
    for (from, to) in used {
        if from > cursor {
            push(cursor, from.min(end));
        }
        cursor = cursor.max(to);
    }
    if end > cursor {
        push(cursor, end);
    }
    free
}

/// A block device as a segment.
pub fn seg_of(state: &DisksState, b: &BlockDevice, kind: SegKind) -> Seg {
    let part = b.partition.as_ref();
    let fs = b.filesystem.as_ref();
    let crypt = b.encrypted.as_ref().map(|_| Crypt {
        unlocked: state.cleartext(&b.path).map(|c| {
            let cfs = c.filesystem.as_ref();
            Inner {
                path: c.path.clone(),
                device: c.device.clone(),
                fs: c.id_type.clone(),
                label: c.id_label.clone(),
                mount: cfs.and_then(|f| f.mount_points.first().cloned()),
                used: cfs.and_then(|f| f.usage).map(|u| u.used),
                size: c.size,
            }
        }),
    });
    Seg {
        kind: Some(kind),
        offset: part.map_or(0, |p| p.offset),
        size: part.map_or(b.size, |p| p.size),
        target: Some(Target::Block(b.path.clone())),
        device: b.device.clone(),
        number: part.map_or(0, |p| p.number),
        fs: b.id_type.clone(),
        label: b.id_label.clone(),
        name: part.map(|p| p.name.clone()).unwrap_or_default(),
        mount: fs.and_then(|f| f.mount_points.first().cloned()),
        used: fs.and_then(|f| f.usage).map(|u| u.used),
        type_id: part.map(|p| p.type_id.clone()).unwrap_or_default(),
        flag_bits: part.map_or(0, |p| p.flags),
        in_use: state.in_use(&b.path),
        crypt,
        pending: Vec::new(),
        logical: part.is_some_and(|p| p.is_contained),
        read_only: b.read_only,
    }
}

/// A disk as it is now.
pub fn current(state: &DisksState, disk: &str) -> Option<Layout> {
    let d = state.block(disk)?;
    let mut layout = Layout {
        disk: d.path.clone(),
        device: d.device.clone(),
        size: d.size,
        table: d.partition_table.as_ref().map(|t| t.kind.clone()).unwrap_or_default(),
        segs: Vec::new(),
        new_table: false,
    };
    if d.partition_table.is_some() {
        for p in state.partitions(disk) {
            let kind = if p.partition.as_ref().is_some_and(|p| p.is_container) {
                SegKind::Extended
            } else {
                SegKind::Partition
            };
            layout.segs.push(seg_of(state, p, kind));
        }
        layout.fill_free();
    } else if !d.id_type.is_empty() || d.encrypted.is_some() {
        layout.segs.push(seg_of(state, d, SegKind::Whole));
    } else {
        let mut blank = seg_of(state, d, SegKind::Blank);
        blank.device = d.device.clone();
        layout.segs.push(blank);
    }
    Some(layout)
}

/// What the selection names, so it survives the layout changing: a
/// segment's target, or unallocated space by where it starts.
///
/// A sheet never holds a `Sel` (or an index): a `Sel` names a path, which
/// another device can take over, and resolving it when the sheet is
/// submitted acts on whatever has it then. A sheet holds the [`Chosen`]
/// made as it opened ([`Layout::choose`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Target(Target),
    Free { offset: u64, logical: bool },
}

impl Sel {
    /// What selecting `seg` names: for the selection, not for a sheet, which
    /// holds the [`Chosen`] made as it opened.
    pub fn of(seg: &Seg) -> Sel {
        match &seg.target {
            Some(t) => Sel::Target(t.clone()),
            None => Sel::Free { offset: seg.offset, logical: seg.logical },
        }
    }
}

/// What a sheet acts on: a segment of a disk (or the disk itself) as the
/// person chose it, with the identity of the devices then. A sheet holds
/// this, not an index into the layout: the layout is made again whenever
/// UDisks reports a change, and an index taken before then may name what
/// took the segment's place, even on another disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    /// The disk's block device, and its identity then.
    disk: String,
    disk_seen: Identity,
    spot: Spot,
    /// The segment's device, for a device UDisks has: its identity then.
    seen: Option<Identity>,
    /// What the segment was called (`sdc2`).
    name: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Spot {
    /// The disk itself (for a new partition table).
    Disk,
    /// A partition, a filesystem on the whole disk, or a pending new
    /// partition.
    Seg(Target),
    /// Unallocated space, by where it started.
    Free { offset: u64, logical: bool },
}

impl Chosen {
    /// What the segment was called when it was chosen (`sdc2`).
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Proof that a segment of this layout (or its disk) is the one the person
/// chose, re-identified now. Only [`Layout::locate`] makes one, and it
/// borrows the layout it was found in, so it can't be kept across a
/// refresh. [`Plan`](crate::plan::Plan)'s operations are queued only at
/// one, so a sheet can't queue at an index it took before the layout
/// changed (TSH-25: the stick's Format sheet queued a format of the
/// partition at the same index on the drive Tansu fell back to):
///
/// ```compile_fail,E0506
/// use aizome_sys::disks::{DisksState, Erase};
/// use tansu::layout::{Chosen, Layout};
/// use tansu::plan::{NewFs, Plan};
/// fn format_after_a_refresh(state: &DisksState, mut layout: Layout, now: Layout, chosen: &Chosen, plan: &mut Plan) {
///     let at = layout.locate(state, chosen).expect("the stick's sdc2");
///     // UDisks reports the stick gone: the layout is made again.
///     layout = now;
///     let ext4 = NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
///     let _ = plan.format(at, ext4);
/// }
/// ```
///
/// nor at a bare index:
///
/// ```compile_fail,E0308
/// use aizome_sys::disks::Erase;
/// use tansu::layout::Layout;
/// use tansu::plan::{NewFs, Plan};
/// fn format_at(layout: &Layout, seg: usize, plan: &mut Plan) {
///     let ext4 = NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
///     let _ = plan.format(seg, ext4);
/// }
/// ```
///
/// A sheet that kept the segment's index can't turn it into a place to
/// queue at when it's submitted, neither found at once:
///
/// ```compile_fail,E0599
/// use aizome_sys::disks::{DisksState, Erase};
/// use tansu::layout::Layout;
/// use tansu::plan::{NewFs, Plan};
/// struct FormatForm { seg: usize }
/// fn submit(state: &DisksState, layout: &Layout, plan: &mut Plan, f: &FormatForm) {
///     let ext4 = NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
///     let _ = plan.format(layout.at(state, f.seg).unwrap(), ext4);
/// }
/// ```
///
/// nor chosen then, even by code holding an opening (a choice is of the
/// selection, not of an index):
///
/// ```compile_fail,E0308
/// use aizome_sys::disks::{DisksState, Erase};
/// use tansu::app::Opening;
/// use tansu::layout::Layout;
/// use tansu::plan::{NewFs, Plan};
/// fn submit(state: &DisksState, layout: &Layout, plan: &mut Plan, seg: usize, opening: &Opening) {
///     let chosen = layout.choose(state, seg, opening).unwrap();
///     let ext4 = NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
///     let _ = plan.format(layout.locate(state, &chosen).unwrap(), ext4);
/// }
/// ```
///
/// and a proof can't be made from an index without finding it:
///
/// ```compile_fail,E0451
/// use tansu::layout::{Here, Layout};
/// fn forge(layout: &Layout, seg: usize) -> Here<'_> {
///     Here { layout, i: Some(seg), ..todo!() }
/// }
/// ```
///
/// A sheet keeps what was chosen and finds it again in the layout as it is
/// when the person submits; once its stick is gone, it isn't there:
///
/// ```
/// use aizome_sys::disks::Erase;
/// use tansu::backend::Backend;
/// use tansu::app::Opening;
/// use tansu::layout::Sel;
/// use tansu::mock::{self, Mock};
/// use tansu::plan::{NewFs, Plan};
/// let mock = Mock::new(mock::sample());
/// let stick = "/org/freedesktop/UDisks2/block_devices/sdc";
/// let (state, mut plan) = (mock.snapshot(), Plan::default());
/// let layout = plan.preview(&state, stick).unwrap();
/// let sdc1 = layout.segs.iter().find(|s| s.short_name() == "sdc1").unwrap();
/// let chosen = layout.choose(&state, &Sel::of(sdc1), &Opening::for_tests()).unwrap();
/// let ext4 = || NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
/// plan.format(layout.locate(&state, &chosen).unwrap(), ext4()).unwrap();
/// assert_eq!(plan.ops[0].device(), "/dev/sdc1");
///
/// let _ = mock.power_off("/org/freedesktop/UDisks2/drives/cruzer");
/// let state = mock.snapshot();
/// let fallback = plan.preview(&state, "/org/freedesktop/UDisks2/block_devices/mmcblk0").unwrap();
/// assert!(fallback.locate(&state, &chosen).is_err());
/// ```
pub struct Here<'l> {
    layout: &'l Layout,
    i: Option<usize>,
    /// The disk's identity now.
    disk_seen: Identity,
    /// The segment's device's identity now, for a device UDisks has.
    seen: Option<Identity>,
}

impl<'l> Here<'l> {
    /// The layout it was found in.
    pub fn layout(&self) -> &'l Layout {
        self.layout
    }

    /// The segment's index in that layout (none for the disk itself).
    pub fn index(&self) -> Option<usize> {
        self.i
    }

    pub fn seg(&self) -> Option<&'l Seg> {
        self.i.and_then(|i| self.layout.segs.get(i))
    }

    /// The disk, as it is now: what a new table or partition is queued on.
    pub fn disk(&self) -> &Identity {
        &self.disk_seen
    }

    /// The segment's device, as it is now: what an operation on it is
    /// queued on.
    pub fn device(&self) -> Option<&Identity> {
        self.seen.as_ref()
    }

    /// The segment's device, to act on at once (unlock it).
    pub fn volume(&self) -> Option<Volume<'_>> {
        self.seen.as_ref().map(|identity| Volume { identity })
    }
}

/// A device found where the person chose it, to act on at once: only
/// [`Here::volume`] gives one, and it borrows the proof. Unlock takes one,
/// so a passphrase goes only to the volume chosen (TSH-25: it went to
/// whatever was at the Unlock sheet's index after its disk went):
///
/// ```compile_fail,E0308
/// use tansu::backend::Backend;
/// use tansu::layout::Layout;
/// fn unlock(backend: &dyn Backend, layout: &Layout, seg: usize, pass: &str) {
///     let block = layout.segs[seg].block().unwrap().to_owned();
///     let _ = backend.unlock(&block, pass);
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Volume<'h> {
    identity: &'h Identity,
}

impl Volume<'_> {
    /// Its block device's object path.
    pub fn path(&self) -> &str {
        self.identity.path()
    }
}

impl Layout {
    /// Where the selection is in this layout. Unallocated space moves as
    /// partitions change: the stretch that holds where it started, else a
    /// partition back there (an undone deletion), else the nearest.
    pub fn position(&self, sel: &Sel) -> Option<usize> {
        match sel {
            Sel::Target(t) => self.find(t),
            Sel::Free { offset, logical } => {
                let free = |s: &&Seg| s.is_free() && s.logical == *logical;
                let holds = |s: &Seg| s.logical == *logical && s.offset <= *offset && *offset < s.end();
                self.segs
                    .iter()
                    .position(|s| free(&s) && holds(s))
                    .or_else(|| self.segs.iter().position(|s| holds(s) && s.kind() != SegKind::Extended))
                    .or_else(|| {
                        self.segs
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| free(s))
                            .min_by_key(|(_, s)| s.offset.abs_diff(*offset))
                            .map(|(i, _)| i)
                    })
            }
        }
    }

    /// The selected segment of this layout, as the person chooses it now to
    /// act on it once they've filled in a sheet (`state` is what the layout
    /// shows). It takes the [`Opening`] that only the code opening a sheet
    /// (or acting on the selection at once) gets, so a sheet that kept a
    /// selection can't choose it when it is submitted:
    ///
    /// ```compile_fail,E0423
    /// use aizome_sys::disks::{DisksState, Erase};
    /// use tansu::app::Opening;
    /// use tansu::layout::{Layout, Sel};
    /// use tansu::plan::{NewFs, Plan};
    /// struct FormatForm { sel: Sel }
    /// fn submit(state: &DisksState, layout: &Layout, plan: &mut Plan, f: &FormatForm) {
    ///     let chosen = layout.choose(state, &f.sel, &Opening(())).unwrap();
    ///     let ext4 = NewFs { fs: "ext4".into(), label: String::new(), erase: Erase::Quick, encrypt: None };
    ///     let _ = plan.format(layout.locate(state, &chosen).unwrap(), ext4);
    /// }
    /// ```
    ///
    /// nor without one:
    ///
    /// ```compile_fail,E0061
    /// use aizome_sys::disks::DisksState;
    /// use tansu::layout::{Layout, Sel};
    /// fn submit(state: &DisksState, layout: &Layout, kept: &Sel) {
    ///     let _ = layout.choose(state, kept);
    /// }
    /// ```
    pub fn choose(&self, state: &DisksState, sel: &Sel, _: &Opening) -> Option<Chosen> {
        let seg = self.segs.get(self.position(sel)?)?;
        let spot = match &seg.target {
            Some(t) => Spot::Seg(t.clone()),
            None => Spot::Free { offset: seg.offset, logical: seg.logical },
        };
        let seen = match &seg.target {
            Some(Target::Block(path)) => Some(state.identity(path)?),
            _ => None,
        };
        let name = match seg.kind() {
            SegKind::Free => format!("The unallocated space on {}", self.device.trim_start_matches("/dev/")),
            _ => seg.short_name(),
        };
        Some(Chosen { disk: self.disk.clone(), disk_seen: state.identity(&self.disk)?, spot, seen, name })
    }

    /// The disk itself, as the person chooses it now (for a new partition
    /// table).
    pub fn choose_disk(&self, state: &DisksState, _: &Opening) -> Option<Chosen> {
        Some(Chosen {
            disk: self.disk.clone(),
            disk_seen: state.identity(&self.disk)?,
            spot: Spot::Disk,
            seen: None,
            name: self.device.trim_start_matches("/dev/").to_owned(),
        })
    }

    /// Where `chosen` is in this layout, if it's still what the person
    /// chose: on the same disk (the same drive at the same names), and the
    /// same device with the same filesystem on it, or for the disk itself, the
    /// same partition table.
    pub fn locate<'l>(&'l self, state: &DisksState, chosen: &Chosen) -> Result<Here<'l>, String> {
        let gone = || format!("{} is no longer there: close this and choose again.", chosen.name);
        let changed = || format!("{} changed since you chose it: close this and choose again.", chosen.name);
        if self.disk != chosen.disk {
            return Err(gone());
        }
        let disk_seen = state.identity(&self.disk).ok_or_else(gone)?;
        if !chosen.disk_seen.same(&disk_seen, Holds::Place) {
            return Err(gone());
        }
        // For the disk itself, also the partition table the person saw.
        if chosen.spot == Spot::Disk && !chosen.disk_seen.same(&disk_seen, Holds::Contents) {
            return Err(changed());
        }
        let (i, seen) = match &chosen.spot {
            Spot::Disk => (None, None),
            Spot::Seg(target) => {
                let i = self.find(target).ok_or_else(gone)?;
                let seen = match target {
                    Target::Block(path) => {
                        let now = state.identity(path).ok_or_else(gone)?;
                        if !chosen.seen.as_ref().is_some_and(|seen| seen.same(&now, Holds::Contents)) {
                            return Err(changed());
                        }
                        Some(now)
                    }
                    Target::New(_) => None,
                };
                (Some(i), seen)
            }
            Spot::Free { offset, logical } => {
                let i = self
                    .segs
                    .iter()
                    .position(|s| s.is_free() && s.logical == *logical && s.offset <= *offset && *offset < s.end())
                    .ok_or_else(gone)?;
                (Some(i), None)
            }
        };
        Ok(Here { layout: self, i, disk_seen, seen })
    }
}

/// A partition's flags, as Tansu offers them: the boot flag, an EFI
/// System Partition, a BIOS boot partition (for GRUB on GPT), hidden.
/// Some are bits, some partition types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags {
    pub boot: bool,
    pub esp: bool,
    pub bios_grub: bool,
    pub hidden: bool,
}

pub const ESP_GPT: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
pub const BIOS_BOOT_GPT: &str = "21686148-6449-6e6f-744e-656564454649";
pub const ESP_DOS: &str = "0xef";

/// DOS types and their hidden twins.
const DOS_HIDDEN: [(&str, &str); 7] = [
    ("0x01", "0x11"),
    ("0x04", "0x14"),
    ("0x06", "0x16"),
    ("0x07", "0x17"),
    ("0x0b", "0x1b"),
    ("0x0c", "0x1c"),
    ("0x0e", "0x1e"),
];

fn dos(id: &str) -> String {
    let n = id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")).and_then(|h| u8::from_str_radix(h, 16).ok());
    match n {
        Some(n) => format!("0x{n:02x}"),
        None => id.to_ascii_lowercase(),
    }
}

impl Flags {
    pub fn of(table: &str, type_id: &str, flag_bits: u64) -> Flags {
        if table == "dos" {
            let t = dos(type_id);
            Flags {
                boot: flag_bits & bits::DOS_BOOTABLE != 0,
                esp: t == ESP_DOS,
                bios_grub: false,
                hidden: DOS_HIDDEN.iter().any(|(_, h)| *h == t),
            }
        } else {
            let t = type_id.to_ascii_lowercase();
            Flags {
                boot: flag_bits & bits::GPT_LEGACY_BOOT != 0,
                esp: t == ESP_GPT,
                bios_grub: t == BIOS_BOOT_GPT,
                hidden: flag_bits & bits::GPT_HIDDEN != 0,
            }
        }
    }

    /// Which of the flags a table has.
    pub fn offered(table: &str) -> Flags {
        Flags { boot: true, esp: true, bios_grub: table != "dos", hidden: true }
    }

    /// The partition type and flag bits that give these flags, from the
    /// current ones (`fs` decides the type when ESP or BIOS boot is turned
    /// off).
    pub fn apply(&self, table: &str, fs: &str, type_id: &str, flag_bits: u64) -> (String, u64) {
        let was = Flags::of(table, type_id, flag_bits);
        let plain =
            || aizome_sys::disks::types::type_for_filesystem(table, if fs.is_empty() { "ext4" } else { fs }).to_owned();
        if table == "dos" {
            let mut t = dos(type_id);
            if self.esp {
                t = ESP_DOS.into();
            } else if was.esp {
                t = plain();
            }
            if self.hidden && !self.esp {
                if let Some((_, h)) = DOS_HIDDEN.iter().find(|(p, _)| *p == t) {
                    t = (*h).into();
                }
            } else if let Some((p, _)) = DOS_HIDDEN.iter().find(|(_, h)| *h == t) {
                t = (*p).into();
            }
            let b = if self.boot { flag_bits | bits::DOS_BOOTABLE } else { flag_bits & !bits::DOS_BOOTABLE };
            (t, b)
        } else {
            let mut t = type_id.to_ascii_lowercase();
            if self.esp {
                t = ESP_GPT.into();
            } else if self.bios_grub {
                t = BIOS_BOOT_GPT.into();
            } else if was.esp || was.bios_grub {
                t = plain();
            }
            let mut b = flag_bits;
            b = if self.boot { b | bits::GPT_LEGACY_BOOT } else { b & !bits::GPT_LEGACY_BOOT };
            b = if self.hidden { b | bits::GPT_HIDDEN } else { b & !bits::GPT_HIDDEN };
            (t, b)
        }
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.esp {
            v.push("ESP");
        }
        if self.bios_grub {
            v.push("BIOS boot");
        }
        if self.boot {
            v.push("boot");
        }
        if self.hidden {
            v.push("hidden");
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_flags_are_bits_and_types() {
        let f = Flags::of("gpt", ESP_GPT, 0);
        assert!(f.esp && !f.boot);
        let off = Flags { esp: false, ..f };
        let (t, b) = off.apply("gpt", "vfat", ESP_GPT, 0);
        assert_eq!(t, "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7", "back to basic data for FAT");
        assert_eq!(b, 0);
        let grub = Flags { bios_grub: true, boot: true, ..Flags::default() };
        let (t, b) = grub.apply("gpt", "", "0fc63daf-8483-4772-8e79-3d69d8477de4", 0);
        assert_eq!(t, BIOS_BOOT_GPT);
        assert_eq!(b, bits::GPT_LEGACY_BOOT);
    }

    #[test]
    fn dos_flags_hide_by_type() {
        let hidden = Flags { hidden: true, ..Flags::default() };
        let (t, _) = hidden.apply("dos", "vfat", "0x0c", 0);
        assert_eq!(t, "0x1c");
        assert!(Flags::of("dos", "0x1C", 0).hidden);
        let (t, b) = Flags { boot: true, ..Flags::default() }.apply("dos", "vfat", "0x1c", 0);
        assert_eq!((t.as_str(), b), ("0x0c", 0x80));
    }

    #[test]
    fn free_space_fills_the_gaps() {
        let g = 1u64 << 30;
        let mut l = Layout { size: 10 * g, table: "gpt".into(), ..Default::default() };
        l.segs.push(Seg { kind: Some(SegKind::Partition), offset: MIB, size: g, ..Default::default() });
        l.segs.push(Seg { kind: Some(SegKind::Partition), offset: 4 * g, size: g, ..Default::default() });
        l.fill_free();
        let free: Vec<(u64, u64)> = l.segs.iter().filter(|s| s.is_free()).map(|s| (s.offset, s.end())).collect();
        assert_eq!(free, [(g + MIB, 4 * g), (5 * g, 10 * g - MIB)]);
        assert_eq!(l.room_after(0), 4 * g - (g + MIB));
    }
}
