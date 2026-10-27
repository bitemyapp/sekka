//! A stand-in for UDisks2: sample disks held in memory, changed by the same
//! operations, for the headless tests, the golden pictures and
//! `aizome-tansu --sample`. Nothing here touches a real device.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Arc;

use aizome_sys::disks::types::{flag_names, partition_type_name, type_for_filesystem};
use aizome_sys::disks::{
    BlockDevice, Confirm, DeletePartition, DisksState, Drive, Encrypted, Filesystem, Format, FormatSpec, Job,
    LoopDevice, NewPartitionTable, Partition, PartitionTable, Repair, Resize, Rotation, SelfTest, Smart,
    SmartAttribute, Swap, TableKind, Usage,
};
use aizome_sys::{Error, Task};

use crate::backend::Backend;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
const B: &str = "/org/freedesktop/UDisks2/block_devices/";
const D: &str = "/org/freedesktop/UDisks2/drives/";

/// Sample disks, changed in memory.
pub struct Mock {
    state: RefCell<DisksState>,
    generation: Cell<u64>,
    /// Every call, in order (`Format(ext4) on /dev/sdc1`).
    pub log: RefCell<Vec<String>>,
    /// The next call whose name starts with the first string fails with
    /// the second.
    fail: RefCell<Option<(String, String)>>,
    serial: Cell<u32>,
}

impl Default for Mock {
    fn default() -> Mock {
        Mock::new(sample())
    }
}

impl Mock {
    pub fn new(state: DisksState) -> Mock {
        Mock {
            state: RefCell::new(state),
            generation: Cell::new(1),
            log: RefCell::default(),
            fail: RefCell::default(),
            serial: Cell::new(0),
        }
    }

    /// Make the next `call` (`"Format"`, `"Delete"`…) fail with `message`.
    pub fn fail_next(&self, call: &str, message: &str) {
        *self.fail.borrow_mut() = Some((call.to_owned(), message.to_owned()));
    }

    /// Show a job in progress on `block` (for pictures of an apply).
    pub fn add_job(&self, operation: &str, block: &str, progress: f64, rate: u64) {
        self.change(|s| {
            s.jobs.push(Job {
                path: format!("/org/freedesktop/UDisks2/jobs/{}", s.jobs.len() + 1),
                operation: operation.into(),
                progress: Some(progress),
                bytes: 0,
                rate,
                remaining: Some(std::time::Duration::from_secs(95)),
                objects: vec![block.into()],
                cancelable: true,
                started_by_uid: 1000,
            })
        });
    }

    pub fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn change(&self, f: impl FnOnce(&mut DisksState)) {
        f(&mut self.state.borrow_mut());
        self.generation.set(self.generation.get() + 1);
    }

    /// Note a call; the error if it was told to fail.
    fn call(&self, what: String) -> Result<(), Error> {
        let failing = self.fail.borrow().as_ref().is_some_and(|(c, _)| what.starts_with(c.as_str()));
        self.log.borrow_mut().push(what);
        if failing {
            let (_, message) = self.fail.borrow_mut().take().unwrap_or_default();
            return Err(Error::Failed(message));
        }
        Ok(())
    }

    fn device(&self, path: &str) -> String {
        self.state.borrow().block(path).map(|b| b.device.clone()).unwrap_or_else(|| path.to_owned())
    }

    fn uuid(&self) -> String {
        self.serial.set(self.serial.get() + 1);
        format!("5a3c-{:04x}", self.serial.get())
    }

    /// The device a confirmation names, refusing it in use, as UDisks
    /// does.
    fn confirmed(&self, path: &str) -> Result<BlockDevice, Error> {
        let s = self.state.borrow();
        let b = s.block(path).cloned().ok_or_else(|| Error::NotFound(format!("{path} is gone")))?;
        if s.in_use(path) {
            return Err(Error::InvalidArgument(format!("{} is in use: unmount it first", b.device)));
        }
        Ok(b)
    }

    fn put_fs(b: &mut BlockDevice, spec: &FormatSpec, uuid: String) {
        b.id_uuid = uuid;
        b.filesystem = None;
        b.swap = None;
        b.encrypted = None;
        if spec.encrypt.is_some() {
            b.id_type = "crypto_LUKS".into();
            b.id_usage = "crypto".into();
            b.id_label.clear();
            b.encrypted = Some(Encrypted { cleartext_device: None, hint_type: "luks2".into() });
            return;
        }
        b.id_type = if spec.fs_type == "empty" { String::new() } else { spec.fs_type.clone() };
        b.id_label = spec.label.clone();
        b.id_usage = if b.id_type.is_empty() {
            String::new()
        } else if b.id_type == "swap" {
            "other".into()
        } else {
            "filesystem".into()
        };
        match b.id_type.as_str() {
            "" => {}
            "swap" => b.swap = Some(Swap { active: false }),
            _ => b.filesystem = Some(Filesystem::default()),
        }
    }
}

fn ready<T>(r: Result<T, Error>) -> Task<T> {
    Task::ready(r)
}

impl Backend for Mock {
    fn snapshot(&self) -> Arc<DisksState> {
        Arc::new(self.state.borrow().clone())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn mount(&self, block: &str) -> Task<String> {
        let r = self.call(format!("Mount {}", self.device(block))).and_then(|()| {
            let mut out = Err(Error::NotFound(block.into()));
            self.change(|s| {
                if let Some(b) = s.blocks.iter_mut().find(|b| b.path == block)
                    && let Some(f) = &mut b.filesystem
                {
                    let name = if b.id_label.is_empty() { b.id_uuid.clone() } else { b.id_label.clone() };
                    let at = format!("/run/media/callen/{name}");
                    f.mount_points = vec![at.clone()];
                    let total = f.size.unwrap_or(b.size);
                    f.usage =
                        Some(Usage { total, used: total / 5, free: total - total / 5, available: total - total / 5 });
                    out = Ok(at);
                }
            });
            out
        });
        ready(r)
    }

    fn unmount(&self, block: &str) -> Task<()> {
        let r = self.call(format!("Unmount {}", self.device(block))).map(|()| {
            self.change(|s| {
                if let Some(f) = s.blocks.iter_mut().find(|b| b.path == block).and_then(|b| b.filesystem.as_mut()) {
                    f.mount_points.clear();
                    f.usage = None;
                }
            })
        });
        ready(r)
    }

    fn unlock(&self, block: &str, passphrase: &str) -> Task<String> {
        let r = self.call(format!("Unlock {}", self.device(block))).and_then(|()| {
            if passphrase.is_empty() || passphrase == "wrong" {
                return Err(Error::Rejected("Error unlocking: wrong passphrase".into()));
            }
            let clear = format!("{B}dm_2d{}", self.serial.get() + 7);
            let uuid = self.uuid();
            self.change(|s| {
                let Some(b) = s.blocks.iter_mut().find(|b| b.path == block) else { return };
                let size = b.size.saturating_sub(16 * MIB);
                if let Some(e) = &mut b.encrypted {
                    e.cleartext_device = Some(clear.clone());
                }
                s.blocks.push(BlockDevice {
                    path: clear.clone(),
                    device: "/dev/dm-1".into(),
                    size,
                    id_type: "ext4".into(),
                    id_usage: "filesystem".into(),
                    id_label: "vault".into(),
                    id_uuid: uuid,
                    crypto_backing_device: Some(block.into()),
                    filesystem: Some(Filesystem::default()),
                    ..Default::default()
                });
            });
            Ok(clear)
        });
        ready(r)
    }

    fn lock(&self, block: &str) -> Task<()> {
        let r = self.call(format!("Lock {}", self.device(block))).map(|()| {
            self.change(|s| {
                let clear = s.cleartext(block).map(|c| c.path.clone());
                if let Some(c) = clear {
                    s.blocks.retain(|b| b.path != c);
                }
                if let Some(e) = s.blocks.iter_mut().find(|b| b.path == block).and_then(|b| b.encrypted.as_mut()) {
                    e.cleartext_device = None;
                }
            })
        });
        ready(r)
    }

    fn set_swap(&self, block: &str, on: bool) -> Task<()> {
        let r = self.call(format!("Swap{} {}", if on { "On" } else { "Off" }, self.device(block))).map(|()| {
            self.change(|s| {
                if let Some(w) = s.blocks.iter_mut().find(|b| b.path == block).and_then(|b| b.swap.as_mut()) {
                    w.active = on;
                }
            })
        });
        ready(r)
    }

    fn power_off(&self, drive: &str) -> Task<()> {
        let r = self.call(format!("PowerOff {drive}")).map(|()| {
            self.change(|s| {
                let gone: Vec<String> = s
                    .blocks
                    .iter()
                    .filter(|b| s.drive_of(b).is_some_and(|d| d.path == drive))
                    .map(|b| b.path.clone())
                    .collect();
                s.blocks.retain(|b| !gone.contains(&b.path));
                s.drives.retain(|d| d.path != drive);
            })
        });
        ready(r)
    }

    fn loop_setup(&self, file: &Path, read_only: bool) -> Task<String> {
        let r = self.call(format!("LoopSetup {} read-only={read_only}", file.display())).and_then(|()| {
            let size = std::fs::metadata(file).map(|m| m.len()).map_err(|e| Error::Io(e.to_string()))?;
            let n = self.state.borrow().blocks.iter().filter(|b| b.loop_device.is_some()).count() + 1;
            let path = format!("{B}loop{n}");
            let iso = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("iso"));
            let uuid = self.uuid();
            self.change(|s| {
                s.blocks.push(BlockDevice {
                    path: path.clone(),
                    device: format!("/dev/loop{n}"),
                    size,
                    read_only,
                    id_type: if iso { "iso9660".into() } else { String::new() },
                    id_usage: if iso { "filesystem".into() } else { String::new() },
                    id_label: if iso { "IMAGE".into() } else { String::new() },
                    id_uuid: uuid,
                    filesystem: iso.then(Filesystem::default),
                    loop_device: Some(LoopDevice {
                        backing_file: file.display().to_string(),
                        autoclear: false,
                        setup_by_uid: 1000,
                    }),
                    ..Default::default()
                })
            });
            Ok(path)
        });
        ready(r)
    }

    fn loop_delete(&self, block: &str) -> Task<()> {
        let r = self.call(format!("LoopDelete {}", self.device(block))).map(|()| {
            self.change(|s| {
                let gone: Vec<String> = std::iter::once(block.to_owned())
                    .chain(s.dependents(block).iter().map(|b| b.path.clone()))
                    .collect();
                s.blocks.retain(|b| !gone.contains(&b.path));
            })
        });
        ready(r)
    }

    fn check(&self, block: &str) -> Task<bool> {
        ready(self.call(format!("Check {}", self.device(block))).map(|()| true))
    }

    fn smart_update(&self, drive: &str) -> Task<()> {
        ready(self.call(format!("SmartUpdate {drive}")))
    }

    fn smart_attributes(&self, drive: &str) -> Task<Vec<SmartAttribute>> {
        let nvme =
            self.state.borrow().drive(drive).and_then(|d| d.smart.as_ref()).is_some_and(|s| s.bad_sectors.is_none());
        let a = |id: Option<u8>, name: &str, value: &str, n: Option<(i32, i32, i32)>, failing: bool, pre_fail: bool| {
            SmartAttribute {
                id,
                name: name.into(),
                value: value.into(),
                normalized: n.map(|n| n.0),
                worst: n.map(|n| n.1),
                threshold: n.map(|n| n.2),
                failing,
                pre_fail,
            }
        };
        let list = if nvme {
            vec![
                a(None, "Available spare", "100 % (threshold 10 %)", None, false, false),
                a(None, "Life used", "2 %", None, false, false),
                a(None, "Data written", "18.4 TB", None, false, false),
                a(None, "Data read", "22.9 TB", None, false, false),
                a(None, "Temperature sensor 1", "41 °C", None, false, false),
                a(None, "Warning temperature", "83 °C", None, false, false),
                a(None, "Power cycles", "412", None, false, false),
                a(None, "Unsafe shutdowns", "37", None, false, false),
                a(None, "Media and data integrity errors", "0", None, false, false),
            ]
        } else {
            let bad = self
                .state
                .borrow()
                .drive(drive)
                .and_then(|d| d.smart.as_ref())
                .and_then(|s| s.bad_sectors)
                .unwrap_or(0);
            vec![
                a(Some(1), "Raw read error rate", "0", Some((200, 200, 51)), false, true),
                a(Some(3), "Spin up time", "6.9 s", Some((175, 172, 21)), false, true),
                a(Some(5), "Reallocated sector count", &format!("{bad} sectors"), Some((198, 198, 140)), false, true),
                a(Some(9), "Power on hours", "1,407 days", Some((52, 52, 0)), false, false),
                a(Some(12), "Power cycle count", "211", Some((100, 100, 0)), false, false),
                a(Some(194), "Temperature celsius", "36 °C", Some((114, 102, 0)), false, false),
                a(
                    Some(197),
                    "Current pending sector",
                    &format!("{} sectors", bad / 2),
                    Some((200, 200, 0)),
                    false,
                    false,
                ),
                a(Some(199), "UDMA CRC error count", "0", Some((200, 200, 0)), false, false),
            ]
        };
        ready(self.call(format!("SmartGetAttributes {drive}")).map(|()| list))
    }

    fn smart_selftest(&self, drive: &str, test: SelfTest) -> Task<()> {
        let r = self.call(format!("SmartSelftestStart({test:?}) {drive}")).map(|()| {
            self.change(|s| {
                if let Some(smart) = s.drives.iter_mut().find(|d| d.path == drive).and_then(|d| d.smart.as_mut()) {
                    smart.selftest_status = "inprogress".into();
                    smart.selftest_percent_remaining = Some(90);
                }
            })
        });
        ready(r)
    }

    fn cancel_job(&self, job: &str) -> Task<()> {
        let r = self.call(format!("Cancel {job}")).map(|()| self.change(|s| s.jobs.retain(|j| j.path != job)));
        ready(r)
    }

    fn can_format(&self, fs: &str) -> Task<(bool, String)> {
        let ok = self.state.borrow().supported_filesystems.iter().any(|s| s == fs);
        ready(Ok(if ok { (true, String::new()) } else { (false, format!("mkfs.{fs}")) }))
    }

    fn create_partition_table(&self, confirm: Confirm<NewPartitionTable>, kind: TableKind) -> Task<()> {
        let path = confirm.block().to_owned();
        let r = self
            .call(format!("Format({}) on {}", if kind == TableKind::Gpt { "gpt" } else { "dos" }, self.device(&path)))
            .and_then(|()| {
                let disk = self.confirmed(&path)?;
                if disk.partition.is_some() {
                    return Err(Error::InvalidArgument(
                        "A partition table goes on a whole disk, not a partition".into(),
                    ));
                }
                self.change(|s| {
                    let gone: Vec<String> = s.dependents(&path).iter().map(|b| b.path.clone()).collect();
                    s.blocks.retain(|b| !gone.contains(&b.path));
                    if let Some(b) = s.blocks.iter_mut().find(|b| b.path == path) {
                        b.partition_table = Some(PartitionTable {
                            kind: if kind == TableKind::Gpt { "gpt".into() } else { "dos".into() },
                            partitions: Vec::new(),
                        });
                        b.id_type.clear();
                        b.id_usage.clear();
                        b.id_label.clear();
                        b.filesystem = None;
                        b.encrypted = None;
                    }
                });
                Ok(())
            });
        ready(r)
    }

    fn create_partition(
        &self,
        table: &str,
        offset: u64,
        size: u64,
        type_id: Option<&str>,
        name: &str,
        format: Option<FormatSpec>,
    ) -> Task<String> {
        let r = self
            .call(format!(
                "CreatePartition({offset}, {size}, {}) on {}",
                format.as_ref().map_or("", |f| f.fs_type.as_str()),
                self.device(table)
            ))
            .and_then(|()| {
                let state = self.state.borrow().clone();
                let disk = state.block(table).cloned().ok_or_else(|| Error::NotFound(table.into()))?;
                let kind = disk
                    .partition_table
                    .as_ref()
                    .map(|t| t.kind.clone())
                    .ok_or_else(|| Error::InvalidArgument("no partition table".into()))?;
                let fits =
                    state.free_space(table).iter().any(|f| offset >= f.offset && offset + size <= f.offset + f.size);
                if size == 0 || !fits {
                    return Err(Error::InvalidArgument("The new partition must fit in free space".into()));
                }
                let parts = state.partitions(table);
                let number = parts.iter().filter_map(|p| p.partition.as_ref().map(|p| p.number)).max().unwrap_or(0) + 1;
                let sep = if disk.device.ends_with(|c: char| c.is_ascii_digit()) { "p" } else { "" };
                let device = format!("{}{sep}{number}", disk.device);
                let path = format!("{B}{}", device.trim_start_matches("/dev/"));
                let type_id = type_id.map(str::to_owned).unwrap_or_else(|| {
                    type_for_filesystem(&kind, format.as_ref().map_or("ext4", |f| f.fs_type.as_str())).to_owned()
                });
                let mut b = BlockDevice {
                    path: path.clone(),
                    device,
                    size,
                    partition: Some(Partition {
                        number,
                        type_name: partition_type_name(&type_id),
                        type_id,
                        offset,
                        size,
                        name: name.to_owned(),
                        table: table.to_owned(),
                        is_contained: state.blocks.iter().filter_map(|b| b.partition.as_ref()).any(|p| {
                            p.is_container && p.table == table && offset >= p.offset && offset < p.offset + p.size
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                if let Some(spec) = &format {
                    Mock::put_fs(&mut b, spec, self.uuid());
                }
                self.change(|s| {
                    s.blocks.push(b);
                    s.blocks.sort_by(|a, b| a.device.len().cmp(&b.device.len()).then(a.device.cmp(&b.device)));
                });
                Ok(path)
            });
        ready(r)
    }

    fn delete_partition(&self, confirm: Confirm<DeletePartition>) -> Task<()> {
        let path = confirm.block().to_owned();
        let r = self.call(format!("Delete {}", self.device(&path))).and_then(|()| {
            let b = self.confirmed(&path)?;
            if b.partition.is_none() {
                return Err(Error::InvalidArgument("Not a partition".into()));
            }
            self.change(|s| {
                s.blocks.retain(|b| b.path != path && b.crypto_backing_device.as_deref() != Some(path.as_str()))
            });
            Ok(())
        });
        ready(r)
    }

    fn format(&self, confirm: Confirm<Format>, spec: FormatSpec) -> Task<()> {
        let path = confirm.block().to_owned();
        let r = self.call(format!("Format({}) on {}", spec.fs_type, self.device(&path))).and_then(|()| {
            self.confirmed(&path)?;
            if spec.fs_type != "empty" && !self.state.borrow().supported_filesystems.contains(&spec.fs_type) {
                return Err(Error::InvalidArgument(format!("This system can't make {} filesystems", spec.fs_type)));
            }
            let uuid = self.uuid();
            self.change(|s| {
                if let Some(b) = s.blocks.iter_mut().find(|b| b.path == path) {
                    Mock::put_fs(b, &spec, uuid);
                }
            });
            Ok(())
        });
        ready(r)
    }

    fn resize_partition(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        let path = confirm.block().to_owned();
        let r = self.call(format!("ResizePartition({size}) {}", self.device(&path))).and_then(|()| {
            let b = self.confirmed(&path)?;
            let part = b.partition.clone().ok_or_else(|| Error::InvalidArgument("Not a partition".into()))?;
            if size > part.size {
                let state = self.state.borrow().clone();
                let end = part.offset + part.size;
                let room = state
                    .free_space(&part.table)
                    .iter()
                    .find(|f| f.offset >= end && f.offset <= end + MIB)
                    .map_or(0, |f| f.offset + f.size - end);
                if size - part.size > room {
                    return Err(Error::InvalidArgument("There isn't that much free space after the partition".into()));
                }
            }
            self.change(|s| {
                if let Some(b) = s.blocks.iter_mut().find(|b| b.path == path) {
                    b.size = size;
                    if let Some(p) = &mut b.partition {
                        p.size = size;
                    }
                }
            });
            Ok(())
        });
        ready(r)
    }

    fn resize_filesystem(&self, confirm: Confirm<Resize>, size: u64) -> Task<()> {
        let path = confirm.block().to_owned();
        let r = self.call(format!("ResizeFilesystem({size}) {}", self.device(&path))).and_then(|()| {
            self.confirmed(&path)?;
            self.change(|s| {
                if let Some(b) = s.blocks.iter_mut().find(|b| b.path == path) {
                    let whole = b.size;
                    if let Some(f) = &mut b.filesystem {
                        f.size = Some(if size == 0 { whole } else { size });
                    }
                }
            });
            Ok(())
        });
        ready(r)
    }

    fn repair(&self, confirm: Confirm<Repair>) -> Task<bool> {
        let path = confirm.block().to_owned();
        let r = self.call(format!("Repair {}", self.device(&path))).and_then(|()| self.confirmed(&path).map(|_| true));
        ready(r)
    }

    fn set_label(&self, block: &str, label: &str) -> Task<()> {
        let r = self.call(format!("SetLabel({label}) {}", self.device(block))).map(|()| {
            self.change(|s| {
                if let Some(b) = s.blocks.iter_mut().find(|b| b.path == block) {
                    b.id_label = label.into();
                }
            })
        });
        ready(r)
    }

    fn set_partition_type(&self, block: &str, type_id: &str) -> Task<()> {
        let r = self.call(format!("SetType({type_id}) {}", self.device(block))).map(|()| {
            self.change(|s| {
                if let Some(p) = s.blocks.iter_mut().find(|b| b.path == block).and_then(|b| b.partition.as_mut()) {
                    p.type_id = type_id.into();
                    p.type_name = partition_type_name(type_id);
                }
            })
        });
        ready(r)
    }

    fn set_partition_flags(&self, block: &str, flags: u64) -> Task<()> {
        let r = self.call(format!("SetFlags({flags:#x}) {}", self.device(block))).map(|()| {
            self.change(|s| {
                let kind = s
                    .block(block)
                    .and_then(|b| b.partition.as_ref())
                    .and_then(|p| s.block(&p.table))
                    .and_then(|t| t.partition_table.as_ref())
                    .map(|t| t.kind.clone())
                    .unwrap_or_default();
                if let Some(p) = s.blocks.iter_mut().find(|b| b.path == block).and_then(|b| b.partition.as_mut()) {
                    p.flags = flags;
                    p.flag_names = flag_names(&kind, flags);
                }
            })
        });
        ready(r)
    }
}

// --- The sample -------------------------------------------------------------

struct Part<'a> {
    name: &'a str,
    number: u32,
    offset: u64,
    size: u64,
    type_id: &'a str,
    flags: u64,
    fs: &'a str,
    label: &'a str,
    mount: Option<&'a str>,
    used: u64,
}

fn part(p: Part, disk: &str, table_kind: &str) -> BlockDevice {
    let fs = (!matches!(p.fs, "" | "swap" | "crypto_LUKS")).then(|| Filesystem {
        mount_points: p.mount.map(|m| vec![m.to_owned()]).unwrap_or_default(),
        size: Some(p.size),
        usage: p.mount.map(|_| Usage {
            total: p.size,
            used: p.used,
            free: p.size - p.used,
            available: p.size - p.used,
        }),
    });
    BlockDevice {
        path: format!("{B}{}", p.name),
        device: format!("/dev/{}", p.name),
        preferred_device: format!("/dev/{}", p.name),
        size: p.size,
        id_usage: match p.fs {
            "" => "",
            "crypto_LUKS" => "crypto",
            "swap" => "other",
            _ => "filesystem",
        }
        .into(),
        id_type: p.fs.into(),
        id_label: p.label.into(),
        id_uuid: format!("{:08x}-{}", p.offset / MIB, p.number),
        filesystem: fs,
        swap: (p.fs == "swap").then_some(Swap { active: p.mount.is_some() }),
        encrypted: (p.fs == "crypto_LUKS").then(|| Encrypted { cleartext_device: None, hint_type: "luks2".into() }),
        partition: Some(Partition {
            number: p.number,
            type_id: p.type_id.into(),
            type_name: partition_type_name(p.type_id),
            flags: p.flags,
            flag_names: flag_names(table_kind, p.flags),
            offset: p.offset,
            size: p.size,
            table: format!("{B}{disk}"),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn disk(name: &str, size: u64, drive: &str, table: &str) -> BlockDevice {
    BlockDevice {
        path: format!("{B}{name}"),
        device: format!("/dev/{name}"),
        preferred_device: format!("/dev/{name}"),
        size,
        drive: Some(format!("{D}{drive}")),
        partition_table: Some(PartitionTable { kind: table.into(), partitions: Vec::new() }),
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    id: &str,
    vendor: &str,
    model: &str,
    size: u64,
    rotation: Rotation,
    bus: &str,
    removable: bool,
    smart: Option<Smart>,
) -> Drive {
    Drive {
        path: format!("{D}{id}"),
        vendor: vendor.into(),
        model: model.into(),
        serial: format!("S{}", id.len() * 7919),
        size,
        rotation,
        removable,
        media_removable: removable && bus == "sdio",
        media_available: true,
        ejectable: removable,
        can_power_off: bus == "usb",
        connection_bus: bus.into(),
        sort_key: format!("{}{id}", if removable { "01hotplug/" } else { "00coldplug/" }),
        smart,
        ..Default::default()
    }
}

const LINUX: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const SWAP: &str = "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f";
const LUKS: &str = "ca7d7ccb-63ed-4c53-861c-1742536059cc";
const BASIC: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";

/// The sample: a laptop's NVMe system disk (an ESP, an unlocked LUKS root,
/// swap), a SATA SSD shared with Windows, an archive hard disk with a
/// warning in its SMART data, a USB stick, an SD card and an attached ISO.
pub fn sample() -> DisksState {
    let mut s = DisksState {
        available: true,
        version: "2.11.2".into(),
        supported_filesystems: ["ext4", "ext3", "ext2", "btrfs", "xfs", "vfat", "exfat", "swap", "f2fs"]
            .map(String::from)
            .to_vec(),
        ..Default::default()
    };
    let good = |temp: f64, hours: u64| Smart {
        supported: true,
        enabled: true,
        temperature_celsius: Some(temp),
        power_on_hours: Some(hours),
        selftest_status: "success".into(),
        updated: 1_792_000_000,
        ..Default::default()
    };
    // The NVMe system disk, 1 TB.
    let tb = 1_000_204_886_016u64;
    s.drives.push(drive(
        "nvme",
        "",
        "Samsung SSD 990 PRO 1TB",
        tb,
        Rotation::SolidState,
        "",
        false,
        Some(good(41.0, 2210)),
    ));
    s.blocks.push(disk("nvme0n1", tb, "nvme", "gpt"));
    s.blocks.push(part(
        Part {
            name: "nvme0n1p1",
            number: 1,
            offset: MIB,
            size: GIB,
            type_id: ESP,
            flags: 0,
            fs: "vfat",
            label: "ESP",
            mount: Some("/boot"),
            used: 248 * MIB,
        },
        "nvme0n1",
        "gpt",
    ));
    s.blocks.push(part(
        Part {
            name: "nvme0n1p2",
            number: 2,
            offset: MIB + GIB,
            size: 860 * GIB,
            type_id: LUKS,
            flags: 0,
            fs: "crypto_LUKS",
            label: "",
            mount: None,
            used: 0,
        },
        "nvme0n1",
        "gpt",
    ));
    s.blocks.push(part(
        Part {
            name: "nvme0n1p3",
            number: 3,
            offset: MIB + 861 * GIB,
            size: 32 * GIB,
            type_id: SWAP,
            flags: 0,
            fs: "swap",
            label: "swap",
            mount: Some("[SWAP]"),
            used: 0,
        },
        "nvme0n1",
        "gpt",
    ));
    s.blocks.push(BlockDevice {
        path: format!("{B}dm_2d0"),
        device: "/dev/dm-0".into(),
        size: 860 * GIB - 16 * MIB,
        id_type: "ext4".into(),
        id_usage: "filesystem".into(),
        id_label: "nixos".into(),
        id_uuid: "c0ffee00-1".into(),
        hint_system: true,
        crypto_backing_device: Some(format!("{B}nvme0n1p2")),
        filesystem: Some(Filesystem {
            mount_points: vec!["/".into()],
            size: Some(860 * GIB - 16 * MIB),
            usage: Some(Usage { total: 860 * GIB, used: 571 * GIB, free: 289 * GIB, available: 280 * GIB }),
        }),
        ..Default::default()
    });
    if let Some(b) = s.blocks.iter_mut().find(|b| b.device == "/dev/nvme0n1p2") {
        b.encrypted = Some(Encrypted { cleartext_device: Some(format!("{B}dm_2d0")), hint_type: "luks2".into() });
    }

    // A SATA SSD shared with Windows, 500 GB.
    let ssd = 500_107_862_016u64;
    s.drives.push(drive(
        "mx500",
        "Crucial",
        "CT500MX500SSD1",
        ssd,
        Rotation::SolidState,
        "",
        false,
        Some(Smart {
            bad_sectors: Some(0),
            attributes_failing: Some(0),
            attributes_failed_in_past: Some(0),
            ..good(33.0, 14_200)
        }),
    ));
    s.blocks.push(disk("sda", ssd, "mx500", "gpt"));
    s.blocks.push(part(
        Part {
            name: "sda1",
            number: 1,
            offset: MIB,
            size: 16 * MIB,
            type_id: "e3c9e316-0b5c-4db8-817d-f92df00215ae",
            flags: 0,
            fs: "",
            label: "",
            mount: None,
            used: 0,
        },
        "sda",
        "gpt",
    ));
    s.blocks.push(part(
        Part {
            name: "sda2",
            number: 2,
            offset: 17 * MIB,
            size: 280 * GIB,
            type_id: BASIC,
            flags: 0,
            fs: "ntfs",
            label: "Windows",
            mount: None,
            used: 0,
        },
        "sda",
        "gpt",
    ));
    s.blocks.push(part(
        Part {
            name: "sda3",
            number: 3,
            offset: 17 * MIB + 280 * GIB,
            size: 140 * GIB,
            type_id: LINUX,
            flags: 0,
            fs: "ext4",
            label: "games",
            mount: Some("/mnt/games"),
            used: 96 * GIB,
        },
        "sda",
        "gpt",
    ));

    // An archive hard disk with a few bad sectors, 4 TB.
    let hdd = 4_000_787_030_016u64;
    s.drives.push(drive(
        "wd40",
        "WDC",
        "WD40EFRX-68N32N0",
        hdd,
        Rotation::Rpm(5400),
        "",
        false,
        Some(Smart {
            bad_sectors: Some(8),
            attributes_failing: Some(0),
            attributes_failed_in_past: Some(0),
            ..good(36.0, 33_770)
        }),
    ));
    s.blocks.push(disk("sdb", hdd, "wd40", "gpt"));
    s.blocks.push(part(
        Part {
            name: "sdb1",
            number: 1,
            offset: MIB,
            size: 2400 * GIB,
            type_id: LINUX,
            flags: 0,
            fs: "btrfs",
            label: "archive",
            mount: Some("/srv/archive"),
            used: 1980 * GIB,
        },
        "sdb",
        "gpt",
    ));
    s.blocks.push(part(
        Part {
            name: "sdb2",
            number: 2,
            offset: 3000 * GIB,
            size: 700 * GIB,
            type_id: LUKS,
            flags: 0,
            fs: "crypto_LUKS",
            label: "",
            mount: None,
            used: 0,
        },
        "sdb",
        "gpt",
    ));

    // A USB stick, 32 GB, MBR.
    let stick = 31_457_280_000u64;
    s.drives.push(drive("cruzer", "SanDisk", "Cruzer Blade", stick, Rotation::Unknown, "usb", true, None));
    s.blocks.push(disk("sdc", stick, "cruzer", "dos"));
    s.blocks.push(part(
        Part {
            name: "sdc1",
            number: 1,
            offset: MIB,
            size: 14 * GIB,
            type_id: "0x0c",
            flags: 0x80,
            fs: "vfat",
            label: "STICK",
            mount: None,
            used: 0,
        },
        "sdc",
        "dos",
    ));
    s.blocks.push(part(
        Part {
            name: "sdc2",
            number: 2,
            offset: MIB + 14 * GIB,
            size: 12 * GIB,
            type_id: "0x07",
            flags: 0,
            fs: "exfat",
            label: "SHARE",
            mount: Some("/run/media/callen/SHARE"),
            used: 3 * GIB + 200 * MIB,
        },
        "sdc",
        "dos",
    ));

    // An SD card, 64 GB.
    let sd = 63_864_569_856u64;
    s.drives.push(drive("sd", "", "SD SC64G", sd, Rotation::Unknown, "sdio", true, None));
    s.blocks.push(disk("mmcblk0", sd, "sd", "dos"));
    s.blocks.push(part(
        Part {
            name: "mmcblk0p1",
            number: 1,
            offset: 4 * MIB,
            size: 256 * MIB,
            type_id: "0x0c",
            flags: 0x80,
            fs: "vfat",
            label: "bootfs",
            mount: None,
            used: 0,
        },
        "mmcblk0",
        "dos",
    ));
    s.blocks.push(part(
        Part {
            name: "mmcblk0p2",
            number: 2,
            offset: 260 * MIB,
            size: 8 * GIB,
            type_id: "0x83",
            flags: 0,
            fs: "ext4",
            label: "rootfs",
            mount: None,
            used: 0,
        },
        "mmcblk0",
        "dos",
    ));

    // An attached ISO.
    s.blocks.push(BlockDevice {
        path: format!("{B}loop0"),
        device: "/dev/loop0".into(),
        size: 2_181_038_080,
        read_only: true,
        id_type: "iso9660".into(),
        id_usage: "filesystem".into(),
        id_label: "SEKKA_NIGHTLY".into(),
        id_uuid: "2026-10-16-04-12-00-00".into(),
        filesystem: Some(Filesystem {
            mount_points: vec!["/run/media/callen/SEKKA_NIGHTLY".into()],
            size: Some(2_181_038_080),
            usage: Some(Usage { total: 2_181_038_080, used: 2_181_038_080, free: 0, available: 0 }),
        }),
        loop_device: Some(LoopDevice {
            backing_file: "/home/callen/Downloads/sekka-nightly.iso".into(),
            autoclear: true,
            setup_by_uid: 1000,
        }),
        ..Default::default()
    });
    // Each table lists its partitions.
    let tables: Vec<String> = s.blocks.iter().filter(|b| b.partition_table.is_some()).map(|b| b.path.clone()).collect();
    for table in tables {
        let parts: Vec<String> = s.partitions(&table).iter().map(|p| p.path.clone()).collect();
        if let Some(t) = s.blocks.iter_mut().find(|d| d.path == table).and_then(|d| d.partition_table.as_mut()) {
            t.partitions = parts;
        }
    }
    s
}

/// The real disks, read once (for pictures of this machine): UDisks's
/// snapshot as a mock nothing can change.
pub fn frozen(state: DisksState) -> Mock {
    Mock::new(state)
}
