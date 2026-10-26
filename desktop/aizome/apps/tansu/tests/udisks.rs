//! Tansu's plans run through the real UDisks2 client (`aizome_sys::disks`)
//! against a fake UDisks2 on a private D-Bus daemon: an 8 GiB USB stick
//! with a GPT table, a FAT partition and an ext4 one. The confirmations
//! Tansu makes on the Apply sheet hold only for the device as shown: a
//! stick swapped meanwhile is refused. No real block device is touched.
//!
//! Skips (passes) where `dbus-daemon` isn't installed.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_sys::Wake;
use aizome_sys::disks::{Disks, Erase};
use tansu::backend::Backend;
use tansu::plan::{NewFs, Plan};
use tansu::run::{Outcome, Run};
use zbus::fdo;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;
const B: &str = "/org/freedesktop/UDisks2/block_devices/";
const DRIVE: &str = "/org/freedesktop/UDisks2/drives/stick";

/// A dbus-daemon of the test's own.
struct TestBus {
    child: Child,
    address: String,
    dir: std::path::PathBuf,
}

impl TestBus {
    fn start(tag: &str) -> Option<TestBus> {
        let dir = std::env::temp_dir().join(format!("tansu-bus-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy>
</busconfig>"#,
                dir.display()
            ),
        )
        .ok()?;
        let mut child = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: no dbus-daemon ({e})");
                return None;
            }
        };
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        Some(TestBus { child, address: line.trim().to_owned(), dir })
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p.to_owned()).unwrap()
}

fn bytes(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

type Log = Arc<Mutex<Vec<String>>>;

struct Manager;

#[zbus::interface(name = "org.freedesktop.UDisks2.Manager")]
impl Manager {
    #[zbus(property)]
    fn version(&self) -> String {
        "2.11.2".into()
    }
    #[zbus(property)]
    fn supported_filesystems(&self) -> Vec<String> {
        ["ext4", "vfat", "exfat"].map(String::from).to_vec()
    }
}

struct Drive;

#[zbus::interface(name = "org.freedesktop.UDisks2.Drive")]
impl Drive {
    #[zbus(property)]
    fn vendor(&self) -> String {
        "Kingston".into()
    }
    #[zbus(property)]
    fn model(&self) -> String {
        "DataTraveler".into()
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
    fn connection_bus(&self) -> String {
        "usb".into()
    }
}

struct Block {
    device: String,
    size: u64,
    drive: bool,
    id_type: String,
    id_label: String,
    id_uuid: String,
    log: Log,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Block")]
impl Block {
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
        path(if self.drive { DRIVE } else { "/" })
    }
    #[zbus(property)]
    fn id_type(&self) -> String {
        self.id_type.clone()
    }
    #[zbus(property)]
    fn id_usage(&self) -> String {
        if self.id_type.is_empty() { String::new() } else { "filesystem".into() }
    }
    #[zbus(property)]
    fn id_label(&self) -> String {
        self.id_label.clone()
    }
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> String {
        self.id_uuid.clone()
    }
    async fn format(
        &mut self,
        fs_type: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let label = options.get("label").and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default();
        self.log.lock().unwrap().push(format!("Format({fs_type}, {label}) {}", self.device));
        self.id_type = fs_type;
        self.id_label = label;
        self.id_uuid = format!("{}-new", self.id_uuid);
        self.id_type_changed(&emitter).await?;
        self.id_label_changed(&emitter).await?;
        self.id_u_u_i_d_changed(&emitter).await?;
        Ok(())
    }
}

struct Table;

#[zbus::interface(name = "org.freedesktop.UDisks2.PartitionTable")]
impl Table {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        "gpt".into()
    }
}

struct Part {
    number: u32,
    offset: u64,
    size: u64,
    log: Log,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Partition")]
impl Part {
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
        path(&format!("{B}sdx"))
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
        self.log.lock().unwrap().push(format!("Delete {me}"));
        server.remove::<Fs, _>(me.as_str()).await.ok();
        server.remove::<Block, _>(me.as_str()).await?;
        server.remove::<Part, _>(me.as_str()).await?;
        Ok(())
    }
}

struct Fs {
    mount_points: Vec<Vec<u8>>,
}

#[zbus::interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Fs {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        self.mount_points.clone()
    }
    async fn mount(
        &mut self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        self.mount_points = vec![bytes("/run/media/test/STICK")];
        self.mount_points_changed(&emitter).await?;
        Ok("/run/media/test/STICK".into())
    }
}

fn serve(bus: &TestBus) -> (zbus::blocking::Connection, Log) {
    let log: Log = Arc::default();
    let block = |dev: &str, size: u64, drive: bool, fs: &str, label: &str| Block {
        device: dev.into(),
        size,
        drive,
        id_type: fs.into(),
        id_label: label.into(),
        id_uuid: format!("uuid-{dev}"),
        log: log.clone(),
    };
    let conn = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.UDisks2")
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2", zbus::fdo::ObjectManager)
        .unwrap()
        .serve_at("/org/freedesktop/UDisks2/Manager", Manager)
        .unwrap()
        .serve_at(DRIVE, Drive)
        .unwrap()
        .serve_at(format!("{B}sdx"), block("/dev/sdx", 8 * GIB, true, "", ""))
        .unwrap()
        .serve_at(format!("{B}sdx"), Table)
        .unwrap()
        .serve_at(format!("{B}sdx1"), block("/dev/sdx1", GIB, false, "vfat", "STICK"))
        .unwrap()
        .serve_at(format!("{B}sdx1"), Part { number: 1, offset: MIB, size: GIB, log: log.clone() })
        .unwrap()
        .serve_at(format!("{B}sdx1"), Fs { mount_points: Vec::new() })
        .unwrap()
        .serve_at(format!("{B}sdx2"), block("/dev/sdx2", 2 * GIB, false, "ext4", "old"))
        .unwrap()
        .serve_at(format!("{B}sdx2"), Part { number: 2, offset: GIB + MIB, size: 2 * GIB, log: log.clone() })
        .unwrap()
        .serve_at(format!("{B}sdx2"), Fs { mount_points: Vec::new() })
        .unwrap()
        .build()
        .unwrap();
    (conn, log)
}

fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

fn disks(bus: &TestBus) -> Disks {
    let d = Disks::with_bus(Some(aizome_sys::Bus::at(&bus.address).unwrap()), Wake::none());
    assert!(eventually(|| d.snapshot().blocks.len() == 3), "{:?}", d.snapshot());
    d
}

/// Run until it ends, as the window's ticks would.
fn finish(run: &mut Run, backend: &dyn Backend) -> Outcome {
    let start = Instant::now();
    while run.is_running() && start.elapsed() < Duration::from_secs(10) {
        run.advance(backend);
        std::thread::sleep(Duration::from_millis(10));
    }
    run.outcome.clone().expect("the run ended")
}

fn format_and_delete(d: &Disks) -> Plan {
    let state = d.snapshot();
    let mut plan = Plan::default();
    let l = plan.preview(&state, &format!("{B}sdx")).unwrap();
    let one = l.segs.iter().position(|s| s.short_name() == "sdx1").unwrap();
    plan.format(&l, one, NewFs { fs: "ext4".into(), label: "DATA".into(), erase: Erase::Quick, encrypt: None })
        .unwrap();
    let l = plan.preview(&state, &format!("{B}sdx")).unwrap();
    let two = l.segs.iter().position(|s| s.short_name() == "sdx2").unwrap();
    plan.delete(&l, two).unwrap();
    plan
}

#[test]
fn a_plan_runs_through_udisks() {
    let Some(bus) = TestBus::start("run") else { return };
    let (_fake, log) = serve(&bus);
    let d = disks(&bus);
    let plan = format_and_delete(&d);
    let mut run = Run::agree(&plan, &d.snapshot());
    assert_eq!(finish(&mut run, &d), Outcome::Done);
    assert_eq!(*log.lock().unwrap(), ["Format(ext4, DATA) /dev/sdx1".to_owned(), format!("Delete {B}sdx2")]);
    let state = d.snapshot();
    assert_eq!(state.block(&format!("{B}sdx1")).unwrap().id_type, "ext4");
    assert!(state.block(&format!("{B}sdx2")).is_none());
}

#[test]
fn a_device_changed_since_the_apply_sheet_is_refused() {
    let Some(bus) = TestBus::start("stale") else { return };
    let (fake, log) = serve(&bus);
    let d = disks(&bus);
    let plan = format_and_delete(&d);
    // The person agrees to the plan as the sheet shows it…
    let mut run = Run::agree(&plan, &d.snapshot());
    // …then another stick is in the port: a new filesystem at sdx1.
    let iface = fake.object_server().interface::<_, Block>(format!("{B}sdx1").as_str()).unwrap();
    {
        let mut b = iface.get_mut();
        b.id_uuid = "uuid-another-stick".into();
        zbus::block_on(b.id_u_u_i_d_changed(iface.signal_emitter())).unwrap();
    }
    assert!(eventually(|| d.snapshot().block(&format!("{B}sdx1")).is_some_and(|b| b.id_uuid == "uuid-another-stick")));
    let outcome = finish(&mut run, &d);
    match &outcome {
        Outcome::Failed { error, .. } => assert!(error.contains("changed since you agreed"), "{error}"),
        other => panic!("{other:?}"),
    }
    assert!(log.lock().unwrap().is_empty(), "nothing was formatted or deleted");
    assert_eq!(run.done_ids(), Vec::<u64>::new());
}

#[test]
fn a_device_mounted_since_it_was_queued_stops_the_apply() {
    let Some(bus) = TestBus::start("mounted") else { return };
    let (_fake, log) = serve(&bus);
    let d = disks(&bus);
    let plan = format_and_delete(&d);
    assert!(plan.problems(&d.snapshot()).is_empty());
    d.mount(&format!("{B}sdx1")).wait().unwrap();
    assert!(eventually(|| d.snapshot().in_use(&format!("{B}sdx1"))));
    let problems = plan.problems(&d.snapshot());
    assert_eq!(problems, ["/dev/sdx1 is in use now: unmount or lock it first."]);
    // And were it run anyway, UDisks's client refuses before asking.
    let mut run = Run::agree(&plan, &d.snapshot());
    assert!(matches!(finish(&mut run, &d), Outcome::Failed { error, .. } if error.contains("in use")));
    assert!(log.lock().unwrap().is_empty());
}
