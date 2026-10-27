//! Disks against a fake UDisks2 on a private bus: an 8 GiB USB stick with a
//! GPT table, a FAT partition and a LUKS partition. No real block device is
//! touched.

mod common;

use aizome_sys::disks::{
    Confirm, DeletePartition, Disks, Erase, Format, FormatSpec, NewPartitionTable, Repair, TableKind,
};
use aizome_sys::{Error, Wake};
use common::{eventually, path};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
const B: &str = "/org/freedesktop/UDisks2/block_devices/";
const DRIVE: &str = "/org/freedesktop/UDisks2/drives/usb";

fn block_path(name: &str) -> String {
    format!("{B}{name}")
}

fn bytes(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn option_str(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options.get(key).and_then(|v| String::try_from(v.try_clone().ok()?).ok())
}

fn option_bool(options: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    options.get(key).and_then(|v| bool::try_from(v.try_clone().ok()?).ok())
}

/// What the fake was asked to do, in order.
#[derive(Default)]
struct Log {
    calls: Vec<String>,
    format_options: Vec<HashMap<String, OwnedValue>>,
    loop_no_filesystem: bool,
    loop_already_mounted: bool,
    mount_error: bool,
    autoclear_error: bool,
    delete_error: bool,
}

type Shared = Arc<Mutex<Log>>;

fn note(log: &Shared, call: String) {
    log.lock().unwrap().calls.push(call);
}

struct Manager {
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Manager")]
impl Manager {
    #[zbus(property)]
    fn version(&self) -> String {
        "2.11.2".into()
    }
    #[zbus(property)]
    fn supported_filesystems(&self) -> Vec<String> {
        ["ext4", "vfat", "exfat", "swap", "iso9660"].map(String::from).to_vec()
    }
    fn can_format(&self, fs_type: String) -> (bool, String) {
        (fs_type != "btrfs", if fs_type == "btrfs" { "mkfs.btrfs".into() } else { String::new() })
    }
    async fn loop_setup(
        &self,
        fd: zbus::zvariant::OwnedFd,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<OwnedObjectPath> {
        use std::os::fd::AsRawFd;
        let backing = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).unwrap_or_default();
        note(&self.log, format!("LoopSetup(read-only={:?})", option_bool(&options, "read-only")));
        let p = block_path("loop0");
        server
            .at(p.as_str(), FakeBlock::new("/dev/loop0", 4 * MIB, None, "iso9660", "IMAGE", self.log.clone()))
            .await?;
        let no_filesystem = self.log.lock().unwrap().loop_no_filesystem;
        if !no_filesystem {
            let mut filesystem = FakeFs::default_with(self.log.clone(), "IMAGE");
            if self.log.lock().unwrap().loop_already_mounted {
                filesystem.mount_points = vec![bytes("/run/media/test/IMAGE")];
            }
            server.at(p.as_str(), filesystem).await?;
        }
        server
            .at(
                p.as_str(),
                FakeLoop { backing: bytes(&backing.to_string_lossy()), autoclear: false, log: self.log.clone() },
            )
            .await?;
        Ok(path(&p))
    }
}

struct FakeDrive {
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl FakeDrive {
    #[zbus(property)]
    fn vendor(&self) -> String {
        "SanDisk".into()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        "Cruzer".into()
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        8 * GIB
    }
    #[zbus(property)]
    fn removable(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn ejectable(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_power_off(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn connection_bus(&self) -> String {
        "usb".into()
    }
    fn power_off(&self, _options: HashMap<String, OwnedValue>) {
        note(&self.log, "PowerOff".into());
    }
    fn eject(&self, _options: HashMap<String, OwnedValue>) {
        note(&self.log, "Eject".into());
    }
}

struct FakeBlock {
    device: String,
    size: u64,
    drive: Option<OwnedObjectPath>,
    id_type: String,
    id_label: String,
    id_uuid: String,
    crypto_backing: Option<OwnedObjectPath>,
    log: Shared,
}

impl FakeBlock {
    fn new(device: &str, size: u64, drive: Option<&str>, id_type: &str, label: &str, log: Shared) -> FakeBlock {
        FakeBlock {
            device: device.into(),
            size,
            drive: drive.map(path),
            id_type: id_type.into(),
            id_label: label.into(),
            id_uuid: format!("uuid-{device}"),
            crypto_backing: None,
            log,
        }
    }
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl FakeBlock {
    #[zbus(property)]
    fn device(&self) -> Vec<u8> {
        bytes(&self.device)
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        self.size
    }
    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        self.drive.clone().unwrap_or_else(|| path("/"))
    }
    #[zbus(property)]
    fn id_type(&self) -> String {
        self.id_type.clone()
    }
    #[zbus(property)]
    fn id_usage(&self) -> String {
        match self.id_type.as_str() {
            "" => String::new(),
            "crypto_LUKS" => "crypto".into(),
            _ => "filesystem".into(),
        }
    }
    #[zbus(property)]
    fn id_label(&self) -> String {
        self.id_label.clone()
    }
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> String {
        self.id_uuid.clone()
    }
    #[zbus(property)]
    fn crypto_backing_device(&self) -> OwnedObjectPath {
        self.crypto_backing.clone().unwrap_or_else(|| path("/"))
    }

    /// Format: with erase, a job shows progress for a moment first.
    async fn format(
        &mut self,
        fs_type: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let me = header.path().map(|p| p.to_string()).unwrap_or_default();
        note(&self.log, format!("Format({fs_type}) on {me}"));
        let erase = option_str(&options, "erase").is_some();
        self.log.lock().unwrap().format_options.push(options);
        if erase {
            let job = "/org/freedesktop/UDisks2/jobs/1";
            server.at(job, FakeJob { progress: 0.5, objects: vec![path(&me)] }).await?;
            async_io::Timer::after(Duration::from_millis(400)).await;
            server.remove::<FakeJob, _>(job).await?;
        }
        self.id_type = fs_type;
        self.id_uuid = format!("{}-new", self.id_uuid);
        self.id_type_changed(&emitter).await?;
        self.id_u_u_i_d_changed(&emitter).await?;
        Ok(())
    }
}

struct FakeFs {
    mount_points: Vec<Vec<u8>>,
    label: String,
    log: Shared,
}

impl FakeFs {
    fn default_with(log: Shared, label: &str) -> FakeFs {
        FakeFs { mount_points: Vec::new(), label: label.into(), log }
    }
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl FakeFs {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        self.mount_points.clone()
    }
    async fn mount(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let me = header.path().map(|p| p.to_string()).unwrap_or_default();
        note(&self.log, format!("Mount {me}"));
        if self.log.lock().unwrap().mount_error {
            return Err(fdo::Error::Failed("mount refused".into()));
        }
        let at = format!("/run/media/test/{}", self.label);
        self.mount_points = vec![bytes(&at)];
        self.mount_points_changed(&emitter).await?;
        Ok(at)
    }
    async fn unmount(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let me = header.path().map(|p| p.to_string()).unwrap_or_default();
        note(&self.log, format!("Unmount {me}"));
        self.mount_points.clear();
        self.mount_points_changed(&emitter).await?;
        Ok(())
    }
    fn set_label(&mut self, label: String, _options: HashMap<String, OwnedValue>) {
        note(&self.log, format!("SetLabel({label})"));
        self.label = label;
    }
    fn check(&self, _options: HashMap<String, OwnedValue>) -> bool {
        note(&self.log, "Check".into());
        true
    }
    fn repair(&self, _options: HashMap<String, OwnedValue>) -> bool {
        note(&self.log, "Repair".into());
        true
    }
}

struct FakeTable {
    partitions: Vec<OwnedObjectPath>,
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.PartitionTable")]
impl FakeTable {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        "gpt".into()
    }
    #[zbus(property)]
    fn partitions(&self) -> Vec<OwnedObjectPath> {
        self.partitions.clone()
    }
    #[allow(clippy::too_many_arguments)]
    async fn create_partition(
        &mut self,
        offset: u64,
        size: u64,
        type_id: String,
        name: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        note(&self.log, format!("CreatePartition({offset}, {size}, {type_id}, {name})"));
        let p = block_path("sdb2");
        server.at(p.as_str(), FakeBlock::new("/dev/sdb2", size, None, "", "", self.log.clone())).await?;
        server
            .at(
                p.as_str(),
                FakePartition { number: 2, offset, size, table: path(&block_path("sdb")), log: self.log.clone() },
            )
            .await?;
        self.partitions.push(path(&p));
        self.partitions_changed(&emitter).await?;
        Ok(path(&p))
    }
}

struct FakePartition {
    number: u32,
    offset: u64,
    size: u64,
    table: OwnedObjectPath,
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Partition")]
impl FakePartition {
    #[zbus(property)]
    fn number(&self) -> u32 {
        self.number
    }
    #[zbus(property)]
    fn offset(&self) -> u64 {
        self.offset
    }
    #[zbus(property)]
    fn size(&self) -> u64 {
        self.size
    }
    #[zbus(property)]
    fn table(&self) -> OwnedObjectPath {
        self.table.clone()
    }
    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        "0fc63daf-8483-4772-8e79-3d69d8477de4".into()
    }
    async fn delete(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        let me = header.path().map(|p| p.to_string()).unwrap_or_default();
        note(&self.log, format!("Delete {me}"));
        server.remove::<FakeFs, _>(me.as_str()).await.ok();
        server.remove::<FakeEncrypted, _>(me.as_str()).await.ok();
        server.remove::<FakeBlock, _>(me.as_str()).await?;
        server.remove::<FakePartition, _>(me.as_str()).await?;
        Ok(())
    }
}

struct FakeEncrypted {
    cleartext: Option<OwnedObjectPath>,
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Encrypted")]
impl FakeEncrypted {
    #[zbus(property)]
    fn cleartext_device(&self) -> OwnedObjectPath {
        self.cleartext.clone().unwrap_or_else(|| path("/"))
    }
    async fn unlock(
        &mut self,
        passphrase: String,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        if passphrase != "secret" {
            return Err(fdo::Error::Failed(
                "Error unlocking: Failed to activate device: Operation not permitted".into(),
            ));
        }
        let p = block_path("dm_2d0");
        let mut clear = FakeBlock::new("/dev/dm-0", GIB - 16 * MIB, None, "ext4", "VAULT", self.log.clone());
        clear.crypto_backing = Some(path(&block_path("sdb3")));
        server.at(p.as_str(), clear).await?;
        server.at(p.as_str(), FakeFs::default_with(self.log.clone(), "VAULT")).await?;
        self.cleartext = Some(path(&p));
        self.cleartext_device_changed(&emitter).await?;
        Ok(path(&p))
    }
    async fn lock(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        note(&self.log, "Lock".into());
        let p = block_path("dm_2d0");
        server.remove::<FakeFs, _>(p.as_str()).await?;
        server.remove::<FakeBlock, _>(p.as_str()).await?;
        self.cleartext = None;
        self.cleartext_device_changed(&emitter).await?;
        Ok(())
    }
}

struct FakeLoop {
    backing: Vec<u8>,
    autoclear: bool,
    log: Shared,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Loop")]
impl FakeLoop {
    #[zbus(property)]
    fn backing_file(&self) -> Vec<u8> {
        self.backing.clone()
    }
    #[zbus(property)]
    fn autoclear(&self) -> bool {
        self.autoclear
    }
    fn set_autoclear(&mut self, on: bool, _options: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        note(&self.log, format!("SetAutoclear({on})"));
        if self.log.lock().unwrap().autoclear_error {
            return Err(fdo::Error::Failed("autoclear refused".into()));
        }
        self.autoclear = on;
        Ok(())
    }
    async fn delete(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> fdo::Result<()> {
        note(&self.log, "LoopDelete".into());
        if self.log.lock().unwrap().delete_error {
            return Err(fdo::Error::Failed("detach refused".into()));
        }
        if !self.log.lock().unwrap().loop_no_filesystem {
            server.remove::<FakeFs, _>(block_path("loop0")).await?;
        }
        server.remove::<FakeBlock, _>(block_path("loop0")).await?;
        Ok(())
    }
}

struct FakeJob {
    progress: f64,
    objects: Vec<OwnedObjectPath>,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Job")]
impl FakeJob {
    #[zbus(property)]
    fn operation(&self) -> String {
        "format-erase".into()
    }
    #[zbus(property)]
    fn progress(&self) -> f64 {
        self.progress
    }
    #[zbus(property)]
    fn progress_valid(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn objects(&self) -> Vec<OwnedObjectPath> {
        self.objects.clone()
    }
    #[zbus(property)]
    fn cancelable(&self) -> bool {
        true
    }
}

/// The fake: sdb (8 GiB, GPT) with sdb1 (FAT, 1 GiB at 1 MiB) and sdb3
/// (LUKS, 1 GiB at 4 GiB).
fn serve(bus: &common::TestBus) -> (zbus::blocking::Connection, Shared) {
    let log: Shared = Arc::default();
    let conn = bus
        .builder()
        .name("org.freedesktop.UDisks2")
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/Manager", Manager { log: log.clone() })
        .unwrap()
        .serve_at(DRIVE, FakeDrive { log: log.clone() })
        .unwrap()
        .serve_at(block_path("sdb"), FakeBlock::new("/dev/sdb", 8 * GIB, Some(DRIVE), "", "", log.clone()))
        .unwrap()
        .serve_at(
            block_path("sdb"),
            FakeTable { partitions: vec![path(&block_path("sdb1")), path(&block_path("sdb3"))], log: log.clone() },
        )
        .unwrap()
        .serve_at(block_path("sdb1"), FakeBlock::new("/dev/sdb1", GIB, None, "vfat", "STICK", log.clone()))
        .unwrap()
        .serve_at(
            block_path("sdb1"),
            FakePartition { number: 1, offset: MIB, size: GIB, table: path(&block_path("sdb")), log: log.clone() },
        )
        .unwrap()
        .serve_at(block_path("sdb1"), FakeFs::default_with(log.clone(), "STICK"))
        .unwrap()
        .serve_at(block_path("sdb3"), FakeBlock::new("/dev/sdb3", GIB, None, "crypto_LUKS", "", log.clone()))
        .unwrap()
        .serve_at(
            block_path("sdb3"),
            FakePartition { number: 3, offset: 4 * GIB, size: GIB, table: path(&block_path("sdb")), log: log.clone() },
        )
        .unwrap()
        .serve_at(block_path("sdb3"), FakeEncrypted { cleartext: None, log: log.clone() })
        .unwrap()
        .build()
        .unwrap();
    (conn, log)
}

fn calls(log: &Shared) -> Vec<String> {
    log.lock().unwrap().calls.clone()
}

fn start(bus: &common::TestBus) -> Disks {
    let disks = Disks::with_bus(Some(bus.bus()), Wake::none());
    assert!(eventually(|| disks.snapshot().blocks.len() == 3), "{:?}", disks.snapshot());
    disks
}

#[test]
fn the_stick_its_partitions_and_free_space() {
    let bus = test_bus!();
    let (_fake, _log) = serve(&bus);
    let disks = start(&bus);
    let state = disks.snapshot();
    assert_eq!(state.supported_filesystems.len(), 5);
    let drive = &state.drives[0];
    assert_eq!(drive.display_name(), "SanDisk Cruzer");
    assert!(drive.external() && drive.can_power_off);
    let disk = state.drive_block(DRIVE).unwrap();
    assert_eq!(disk.device, "/dev/sdb");
    let free = state.free_space(&disk.path);
    assert_eq!(free.len(), 2, "{free:?}");
    assert_eq!((free[0].offset, free[0].offset + free[0].size), (GIB + MIB, 4 * GIB));
    assert_eq!(free[1].offset, 5 * GIB);
    let volumes = state.volumes();
    let names: Vec<&str> = volumes.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["1.1 GB Encrypted", "STICK"]);
    assert!(disks.can_format("btrfs").wait().unwrap() == (false, "mkfs.btrfs".to_owned()));
}

#[test]
fn mounting_and_safe_removal() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let mounted = disks.mount(&block_path("sdb1")).wait().unwrap();
    assert_eq!(mounted, "/run/media/test/STICK");
    assert!(eventually(|| disks
        .snapshot()
        .volumes()
        .iter()
        .any(|v| v.mount_point.as_deref() == Some("/run/media/test/STICK"))));
    // Unlock the LUKS partition and mount what is inside.
    assert!(disks.unlock(&block_path("sdb3"), "wrong").wait().is_err());
    let clear = disks.unlock(&block_path("sdb3"), "secret").wait().unwrap();
    assert!(eventually(|| disks.snapshot().block(&clear).is_some()));
    disks.mount(&clear).wait().unwrap();
    assert!(eventually(|| disks.snapshot().in_use(&block_path("sdb3"))));
    // Safe removal: unmount everything, lock, then power off.
    disks.power_off(DRIVE).wait().unwrap();
    let calls = calls(&log);
    let tail: Vec<&str> = calls.iter().map(String::as_str).skip_while(|c| !c.starts_with("Unmount")).collect();
    assert_eq!(tail.len(), 4, "{calls:?}");
    assert!(tail[..2].iter().all(|c| c.starts_with("Unmount")));
    assert_eq!(&tail[2..], ["Lock", "PowerOff"]);
}

#[test]
fn formatting_needs_a_fresh_confirmation_and_shows_progress() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let stick = disks.snapshot().block(&block_path("sdb1")).unwrap().clone();

    // Mounted: refused before UDisks is asked.
    disks.mount(&stick.path).wait().unwrap();
    assert!(eventually(|| disks.snapshot().in_use(&stick.path)));
    let refused = disks.format(
        Confirm::<Format>::new(&disks.snapshot().block(&stick.path).unwrap().clone()),
        FormatSpec::new("ext4", "DATA"),
    );
    assert!(matches!(refused.wait(), Err(Error::InvalidArgument(m)) if m.contains("in use")));
    disks.unmount(&stick.path, false).wait().unwrap();
    assert!(eventually(|| !disks.snapshot().in_use(&stick.path)));

    // Erasing shows a job with progress while it runs.
    let mut spec = FormatSpec::new("ext4", "DATA");
    spec.erase = Erase::Zeros;
    let task = disks.format(Confirm::<Format>::new(&stick), spec);
    assert!(eventually(|| disks.snapshot().jobs_for(&stick.path).any(|j| j.progress == Some(0.5))));
    task.wait().unwrap();
    {
        let log = log.lock().unwrap();
        let options = log.format_options.last().unwrap();
        assert_eq!(option_str(options, "label").as_deref(), Some("DATA"));
        assert_eq!(option_str(options, "erase").as_deref(), Some("zero"));
        assert_eq!(option_bool(options, "take-ownership"), Some(true));
    }
    assert!(eventually(|| disks.snapshot().jobs.is_empty()));
    assert!(eventually(|| disks.snapshot().block(&stick.path).is_some_and(|b| b.id_type == "ext4")));

    // The old confirmation was for the FAT filesystem that is gone.
    let stale = Confirm::<Format>::new(&stick);
    assert!(
        matches!(disks.format(stale, FormatSpec::new("vfat", "X")).wait(), Err(Error::InvalidArgument(m)) if m.contains("changed"))
    );
    // A filesystem this system can't make is refused.
    let now = disks.snapshot().block(&stick.path).unwrap().clone();
    assert!(disks.format(Confirm::<Format>::new(&now), FormatSpec::new("zfs", "")).wait().is_err());
    // Repair needs its own confirmation.
    assert!(disks.repair(Confirm::<Repair>::new(&now)).wait().unwrap());
    assert!(disks.check(&now.path).wait().unwrap());
}

#[test]
fn partitions_are_made_in_free_space_and_deleted_on_confirmation() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let disk = block_path("sdb");
    // Over an existing partition: refused without asking UDisks.
    assert!(disks.create_partition(&disk, MIB, GIB, None, "", None).wait().is_err());
    let made = disks.create_partition(&disk, 2 * GIB, GIB, None, "Data", None).wait().unwrap();
    assert_eq!(made, block_path("sdb2"));
    assert!(calls(&log).contains(&format!(
        "CreatePartition({}, {}, 0fc63daf-8483-4772-8e79-3d69d8477de4, Data)",
        2 * GIB,
        GIB
    )));
    assert!(eventually(|| disks.snapshot().partitions(&disk).len() == 3));
    let new = disks.snapshot().block(&made).unwrap().clone();
    disks.delete_partition(Confirm::<DeletePartition>::new(&new)).wait().unwrap();
    assert!(eventually(|| disks.snapshot().partitions(&disk).len() == 2));
    // A new partition table on the whole disk.
    let whole = disks.snapshot().block(&disk).unwrap().clone();
    disks.create_partition_table(Confirm::<NewPartitionTable>::new(&whole), TableKind::Dos).wait().unwrap();
    assert!(calls(&log).contains(&format!("Format(dos) on {disk}")));
    // …but not on a partition.
    let part = disks.snapshot().block(&block_path("sdb1")).unwrap().clone();
    assert!(disks.create_partition_table(Confirm::<NewPartitionTable>::new(&part), TableKind::Gpt).wait().is_err());
}

#[test]
fn images_mount_through_a_loop_device() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let image = std::env::temp_dir().join(format!("aizome-sys-image-{}.iso", std::process::id()));
    std::fs::write(&image, vec![0u8; 4096]).unwrap();
    let mounted = disks.mount_image(&image).wait().unwrap();
    std::fs::remove_file(&image).unwrap();
    assert_eq!(mounted, "/run/media/test/IMAGE");
    let calls = calls(&log);
    assert!(calls.contains(&"LoopSetup(read-only=Some(true))".to_owned()), "{calls:?}");
    assert!(calls.contains(&"SetAutoclear(true)".to_owned()));
    assert!(
        calls.iter().position(|c| c.starts_with("Mount ")).unwrap()
            < calls.iter().position(|c| c == "SetAutoclear(true)").unwrap(),
        "{calls:?}"
    );
    // The snapshot follows UDisks2's signals, which can trail the mount's
    // reply on a busy machine.
    assert!(eventually(|| disks.snapshot().volumes().iter().any(|v| v.name == "IMAGE")));
    let state = disks.snapshot();
    let volume = state.volumes().into_iter().find(|v| v.name == "IMAGE").unwrap();
    assert_eq!(volume.icon_name, "media-optical");
    assert!(volume.image_file.unwrap().ends_with(".iso"));
}

#[test]
fn manual_images_stay_attached_until_explicit_delete() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let image = std::env::temp_dir().join(format!("aizome-manual-image-{}.img", std::process::id()));
    std::fs::write(&image, vec![0u8; 4096]).unwrap();
    for read_only in [true, false] {
        let device = disks.loop_setup(&image, read_only).wait().unwrap();
        assert!(eventually(|| disks.snapshot().block(&device).is_some_and(|b| b.loop_device.is_some())));
        assert!(!disks.snapshot().block(&device).unwrap().loop_device.as_ref().unwrap().autoclear);
        assert!(!calls(&log).iter().any(|c| c.starts_with("SetAutoclear")));
        disks.loop_delete(&device).wait().unwrap();
        assert!(eventually(|| disks.snapshot().block(&device).is_none()));
    }
    std::fs::remove_file(image).unwrap();
}

#[test]
fn failed_image_mounts_release_their_loop_and_preserve_errors() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    let image = std::env::temp_dir().join(format!("aizome-failed-image-{}.img", std::process::id()));
    std::fs::write(&image, vec![0u8; 4096]).unwrap();
    log.lock().unwrap().mount_error = true;
    let error = disks.mount_image(&image).wait().unwrap_err();
    assert!(error.to_string().contains("mount refused"));
    assert!(calls(&log).contains(&"LoopDelete".into()));
    assert!(!calls(&log).iter().any(|c| c.starts_with("SetAutoclear")));
    assert!(eventually(|| disks.snapshot().block(&block_path("loop0")).is_none()));

    {
        let mut log = log.lock().unwrap();
        log.calls.clear();
        log.mount_error = false;
        log.autoclear_error = true;
        log.delete_error = true;
    }
    let error = disks.mount_image(&image).wait().unwrap_err().to_string();
    assert!(error.contains("autoclear refused") && error.contains("detach refused"), "{error}");
    let calls = calls(&log);
    let unmount = calls.iter().position(|c| c.starts_with("Unmount ")).unwrap();
    let delete = calls.iter().position(|c| c == "LoopDelete").unwrap();
    assert!(unmount < delete, "{calls:?}");
    std::fs::remove_file(image).unwrap();
}

#[test]
fn image_without_a_filesystem_releases_its_loop() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    log.lock().unwrap().loop_no_filesystem = true;
    let image = std::env::temp_dir().join(format!("aizome-empty-image-{}.img", std::process::id()));
    std::fs::write(&image, vec![0u8; 4096]).unwrap();
    let error = disks.mount_image(&image).wait().unwrap_err();
    assert!(error.to_string().contains("No filesystem found"));
    assert!(calls(&log).contains(&"LoopDelete".into()));
    assert!(eventually(|| disks.snapshot().block(&block_path("loop0")).is_none()));
    std::fs::remove_file(image).unwrap();
}

#[test]
fn cleanup_does_not_unmount_a_mount_made_by_someone_else() {
    let bus = test_bus!();
    let (_fake, log) = serve(&bus);
    let disks = start(&bus);
    {
        let mut log = log.lock().unwrap();
        log.loop_already_mounted = true;
        log.autoclear_error = true;
        log.delete_error = true;
    }
    let image = std::env::temp_dir().join(format!("aizome-automounted-image-{}.img", std::process::id()));
    std::fs::write(&image, vec![0u8; 4096]).unwrap();
    let error = disks.mount_image(&image).wait().unwrap_err().to_string();
    assert!(error.contains("autoclear refused") && error.contains("detach refused"), "{error}");
    assert!(!calls(&log).iter().any(|c| c.starts_with("Mount ") || c.starts_with("Unmount ")));
    std::fs::remove_file(image).unwrap();
}

#[test]
fn without_udisks() {
    let bus = test_bus!();
    let disks = Disks::with_bus(Some(bus.bus()), Wake::none());
    std::thread::sleep(Duration::from_millis(200));
    assert!(!disks.available());
    assert!(disks.mount("/org/freedesktop/UDisks2/block_devices/sda1").wait().is_err());
}
