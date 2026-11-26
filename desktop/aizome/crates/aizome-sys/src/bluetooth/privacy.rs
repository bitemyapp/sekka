//! LE privacy: whether this computer uses a changing (resolvable private)
//! address over Bluetooth LE, and gives each device it pairs with the key
//! that recognises it by that address (its identity resolving key). BlueZ
//! hands the key over only with privacy on (main.conf's
//! `General.Privacy`). Xbox Series controllers need it: without it they
//! pair, connect, send nothing, and after a few minutes drop the link and
//! forget the key.
//!
//! The cost falls on devices paired while it was off. They never got the
//! key, and one that only answers the computer it knows may not recognise
//! the changing address: it stops reconnecting until it's forgotten and
//! paired again. So Aizome chooses for each computer, each time bluetoothd
//! starts (`aizome-bluetooth prepare`, as root, just before it; see
//! [`decide`]): what main.conf sets, if it sets it; else what the person
//! chose in Settings › Bluetooth ([`CHOICE`]); else on, unless a device
//! paired over LE may not have the key. [`PAIRINGS`] keeps which pairings
//! have it from one start to the next. bluetoothd reads [`CONFIG`]
//! (main.conf with that in it) in main.conf's place, and [`RUNNING`] keeps
//! what it started with.
//!
//! BlueZ reads main.conf only when it starts, and NixOS doesn't restart it
//! on a switch (that would drop Bluetooth keyboards and mice). [`Status`]
//! tells Settings whether a restart would change anything, which Settings
//! offers and does only when the person says so ([`change`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::service::{Task, Wake};

/// The person's choice: `on` or `off`, absent for automatic. Root's.
pub const CHOICE: &str = "/var/lib/aizome/bluetooth-privacy";
/// Which LE pairings have this computer's key ([`Pairings`]). Root's
/// alone: it names devices.
pub const PAIRINGS: &str = "/var/lib/aizome/bluetooth-pairings.toml";
/// What bluetoothd started with ([`Running`]).
pub const RUNNING: &str = "/run/aizome-bluetooth/state.toml";
/// The main.conf bluetoothd reads, root's alone, as the system's may be.
pub const CONFIG: &str = "/run/aizome-bluetooth/main.conf";
/// The system's main.conf.
pub const MAIN_CONF: &str = "/etc/bluetooth/main.conf";
/// BlueZ's pairings and keys, by adapter.
pub const STORAGE: &str = "/var/lib/bluetooth";
/// There when Aizome chooses for bluetoothd (its NixOS module).
pub const MANAGED: &str = "/etc/aizome/bluetooth-privacy.toml";
/// The helper that changes it, run as root.
pub const HELPER: &str = "aizome-bluetooth";

/// What main.conf gets for on: BlueZ's device privacy mode, which still
/// accepts a device's identity address after it has handed over its key.
const ON: &str = "device";

/// The groups of a BlueZ `info` file that hold an LE key.
const LE_KEYS: [&str; 3] = ["[LongTermKey]", "[PeripheralLongTermKey]", "[SlaveLongTermKey]"];

/// What the person chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Choice {
    /// Nothing: [`decide`] chooses for the computer.
    #[default]
    Automatic,
    On,
    Off,
}

impl Choice {
    pub fn word(self) -> &'static str {
        match self {
            Choice::Automatic => "automatic",
            Choice::On => "on",
            Choice::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<Choice> {
        [Choice::Automatic, Choice::On, Choice::Off].into_iter().find(|c| c.word() == s.trim())
    }

    /// The choice in `path` ([`CHOICE`]); automatic if there's none.
    pub fn read_from(path: &Path) -> Choice {
        std::fs::read_to_string(path).ok().and_then(|s| Choice::parse(&s)).unwrap_or_default()
    }
}

/// Why it's on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// main.conf sets it: the system configuration
    /// (`hardware.bluetooth.settings.General.Privacy`).
    System,
    /// The person chose it.
    Chosen,
    /// Automatic: nothing is paired over LE.
    NoPairings,
    /// Automatic: every device paired over LE has the key.
    AllHaveKey,
    /// Automatic: a device paired over LE may not have the key.
    EarlierPairings,
    /// `prepare` couldn't read main.conf, so bluetoothd read it as it is:
    /// privacy is whatever it says.
    Unprepared,
}

/// What bluetoothd started with, as `prepare` wrote it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Running {
    /// Whether privacy is on (not known, and false, when unprepared).
    pub privacy: bool,
    pub why: Why,
    pub choice: Choice,
    /// The file main.conf led to then: another now means the system
    /// changed since (an update).
    pub main_conf: String,
}

impl Running {
    pub fn read_from(path: &Path) -> Option<Running> {
        Running::parse(&std::fs::read_to_string(path).ok()?)
    }

    pub fn parse(text: &str) -> Option<Running> {
        toml::from_str(text).ok()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// Which LE pairings have this computer's key, kept from one start of
/// bluetoothd to the next ([`PAIRINGS`]). A pairing is named by its
/// adapter's folder, its device's and a fingerprint of its LE keys, so a
/// device paired again (new keys) is a new pairing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Pairings {
    /// What bluetoothd last started with: pairings made since have the key
    /// if it was on. None: not known (it read the system's main.conf).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<bool>,
    #[serde(default)]
    pub keyed: BTreeSet<String>,
    #[serde(default)]
    pub unkeyed: BTreeSet<String>,
}

impl Pairings {
    pub fn read_from(path: &Path) -> Option<Pairings> {
        toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// The key of a `key = value` line, as BlueZ's key files have them.
fn key(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('#') || line.starts_with('[') {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

/// The system's main.conf, for `prepare`: none there is empty (BlueZ's
/// defaults). One that can't be read, or isn't UTF-8, is an error, so that
/// bluetoothd reads it itself rather than a copy that lost what it says.
pub fn read_main_conf(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// What main.conf's `[General]` sets `Privacy` to, if it does: on for any
/// of BlueZ's modes but off (BlueZ takes what it doesn't know as off).
pub fn system_privacy(main_conf: &str) -> Option<bool> {
    let mut general = false;
    let mut found = None;
    for line in main_conf.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            general = t == "[General]";
        } else if general && key(t) == Some("Privacy") {
            let value = t.split_once('=').map_or("", |(_, v)| v.trim());
            found = Some(matches!(value, "on" | "network" | "device" | "limited-network" | "limited-device"));
        }
    }
    found
}

/// main.conf with `Privacy` set in its `[General]`.
pub fn render(main_conf: &str, privacy: bool) -> String {
    let setting = format!("Privacy={}\n", if privacy { ON } else { "off" });
    let mut out = String::with_capacity(main_conf.len() + setting.len());
    let mut general = false;
    let mut placed = false;
    for line in main_conf.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            general = t == "[General]";
            out.push_str(line);
            out.push('\n');
            if general && !placed {
                out.push_str(&setting);
                placed = true;
            }
        } else if !(general && key(t) == Some("Privacy")) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if placed { out } else { format!("[General]\n{setting}\n{out}") }
}

/// A Bluetooth address, as BlueZ names a device's folder.
fn address(name: &str) -> bool {
    name.len() == 17
        && name.split(':').count() == 6
        && name.split(':').all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// An adapter's folder: its address, with `static-` before a random
/// static one's.
fn adapter_folder(name: &str) -> bool {
    address(name.strip_prefix("static-").unwrap_or(name))
}

/// The adapters' folders in `storage`; an error but its not being there
/// is `Err`.
fn adapters(storage: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(storage) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        if adapter_folder(&entry.file_name().to_string_lossy()) && entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}

/// Whether the adapter whose folder is `adapter` has BlueZ's key for this
/// computer, which BlueZ makes the first time it starts with privacy on.
pub fn adapter_has_key(adapter: &Path) -> bool {
    std::fs::read_to_string(adapter.join("identity"))
        .is_ok_and(|s| s.lines().any(|l| key(l) == Some("IdentityResolvingKey")))
}

/// FNV-1a over the keys in an `info` file's LE key groups: they're new
/// whenever the device pairs again.
fn fingerprint(info: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut le = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            le = LE_KEYS.contains(&line);
        } else if le && key(line) == Some("Key") {
            for b in line.bytes().chain(*b"\n") {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    hash
}

/// An LE pairing in BlueZ's storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    /// Its adapter's folder.
    pub adapter: String,
    /// `adapter/device#fingerprint`, as [`Pairings`] names it.
    pub id: String,
}

/// The LE pairings in `storage`, on every adapter. `Err` if some of it
/// can't be read (none there at all is none).
pub fn le_pairings(storage: &Path) -> std::io::Result<Vec<Pairing>> {
    let mut found = Vec::new();
    for adapter in adapters(storage)? {
        let adapter_name = adapter.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for device in std::fs::read_dir(&adapter)? {
            let device = device?;
            let device_name = device.file_name().to_string_lossy().into_owned();
            if !address(&device_name) {
                continue;
            }
            let info = match std::fs::read(device.path().join("info")) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if info.lines().any(|l| LE_KEYS.contains(&l.trim())) {
                let id = format!("{adapter_name}/{device_name}#{:016x}", fingerprint(&info));
                found.push(Pairing { adapter: adapter_name.clone(), id });
            }
        }
    }
    Ok(found)
}

/// What [`decide`] decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub privacy: bool,
    pub why: Why,
    /// What to keep for the next start ([`PAIRINGS`]).
    pub pairings: Pairings,
}

/// Whether bluetoothd starts with privacy, and why: what main.conf says,
/// if it says; else the person's choice; else on, unless a device paired
/// over LE may not have the key.
///
/// Which pairings have the key comes from `before`, what was kept at the
/// last start: one known then is as it was, and one made since has the key
/// if bluetoothd started with privacy then. With nothing kept (before
/// Aizome chose), a pairing has it if its own adapter has BlueZ's key for
/// this computer, which BlueZ makes only with privacy on: another
/// adapter's says nothing of it. If BlueZ's storage can't be read, what
/// was known stays, and automatic is off.
pub fn decide(choice: Choice, main_conf: &str, storage: &Path, before: Option<&Pairings>) -> Decision {
    let (keyed, unkeyed, known) = match le_pairings(storage) {
        Ok(found) => {
            let (mut keyed, mut unkeyed) = (BTreeSet::new(), BTreeSet::new());
            for p in found {
                let has_key = match before {
                    Some(b) if b.keyed.contains(&p.id) => true,
                    Some(b) if b.unkeyed.contains(&p.id) => false,
                    Some(b) => b.privacy == Some(true),
                    None => adapter_has_key(&storage.join(&p.adapter)),
                };
                if has_key {
                    keyed.insert(p.id)
                } else {
                    unkeyed.insert(p.id)
                };
            }
            (keyed, unkeyed, true)
        }
        Err(_) => {
            let b = before.cloned().unwrap_or_default();
            (b.keyed, b.unkeyed, false)
        }
    };
    let (privacy, why) = match (system_privacy(main_conf), choice) {
        (Some(on), _) => (on, Why::System),
        (None, Choice::On) => (true, Why::Chosen),
        (None, Choice::Off) => (false, Why::Chosen),
        (None, Choice::Automatic) if !known || !unkeyed.is_empty() => (false, Why::EarlierPairings),
        (None, Choice::Automatic) if keyed.is_empty() => (true, Why::NoPairings),
        (None, Choice::Automatic) => (true, Why::AllHaveKey),
    };
    Decision { privacy, why, pairings: Pairings { privacy: Some(privacy), keyed, unkeyed } }
}

/// Why a restart of Bluetooth would change it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    /// The system changed since bluetoothd started (an update, or this
    /// coming with one): its Bluetooth settings wait for a restart.
    Update,
    /// The person's choice changed since.
    Choice,
}

/// Where [`Status`] reads from: the system's files, or a test's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub managed: PathBuf,
    pub choice: PathBuf,
    pub running: PathBuf,
    pub main_conf: PathBuf,
}

impl Paths {
    pub fn system() -> Paths {
        Paths { managed: MANAGED.into(), choice: CHOICE.into(), running: RUNNING.into(), main_conf: MAIN_CONF.into() }
    }
}

/// What Settings shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// What bluetoothd started with; none if it started before Aizome
    /// chose for it.
    pub running: Option<Running>,
    pub choice: Choice,
    /// What main.conf sets, which the person's choice doesn't change.
    pub system: Option<bool>,
    pub restart: Option<Restart>,
}

impl Status {
    /// The system's; none where Aizome doesn't choose for bluetoothd.
    pub fn read() -> Option<Status> {
        Status::read_from(&Paths::system())
    }

    pub fn read_from(paths: &Paths) -> Option<Status> {
        if !paths.managed.exists() {
            return None;
        }
        let running = Running::read_from(&paths.running);
        let choice = Choice::read_from(&paths.choice);
        // main.conf may be root's alone: then what bluetoothd started with
        // says whether it sets privacy.
        let system = match std::fs::read(&paths.main_conf).ok().and_then(|b| String::from_utf8(b).ok()) {
            Some(text) => system_privacy(&text),
            None => running.as_ref().filter(|r| r.why == Why::System).map(|r| r.privacy),
        };
        let now = std::fs::canonicalize(&paths.main_conf).ok().map(|p| p.display().to_string());
        Some(Status::new(running, choice, system, now))
    }

    /// With `main_conf`, the file main.conf leads to now.
    pub fn new(running: Option<Running>, choice: Choice, system: Option<bool>, main_conf: Option<String>) -> Status {
        let restart = match &running {
            None => Some(Restart::Update),
            Some(r) if main_conf.is_some_and(|now| now != r.main_conf) => Some(Restart::Update),
            Some(r) if matches!(r.why, Why::System | Why::Unprepared) => None,
            Some(r) => {
                let next = match choice {
                    Choice::On => Some(true),
                    Choice::Off => Some(false),
                    Choice::Automatic if r.choice == Choice::Automatic => Some(r.privacy),
                    Choice::Automatic => None,
                };
                (next != Some(r.privacy)).then_some(Restart::Choice)
            }
        };
        Status { running, choice, system, restart }
    }

    /// Whether bluetoothd couldn't be prepared, and reads the system's
    /// main.conf as it is: no choice of the person's reaches it.
    pub fn unprepared(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.why == Why::Unprepared)
    }

    /// Whether it's on after Bluetooth restarts, as far as can be told
    /// without root (automatic decides as it did, if it did).
    pub fn next(&self) -> Option<bool> {
        if let Some(on) = self.system {
            return Some(on);
        }
        if self.unprepared() {
            return None;
        }
        match self.choice {
            Choice::On => Some(true),
            Choice::Off => Some(false),
            Choice::Automatic => self.running.as_ref().filter(|r| r.why != Why::Chosen).map(|r| r.privacy),
        }
    }

    /// Whether the restart offered turns privacy on: devices paired while
    /// it was off may then stop connecting until they're paired again.
    pub fn restart_turns_on(&self) -> bool {
        self.restart == Some(Restart::Choice)
            && self.running.as_ref().is_some_and(|r| !r.privacy)
            && self.next() == Some(true)
    }
}

/// What Settings asks of the helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Keep this choice for bluetoothd's next start.
    Set(Choice),
    /// Restart bluetoothd, which then starts with what's chosen.
    Restart,
}

impl Change {
    pub fn args(self) -> Vec<String> {
        match self {
            Change::Set(c) => vec!["set".into(), c.word().into()],
            Change::Restart => vec!["restart".into()],
        }
    }
}

/// Where the helper is: `../libexec` beside this program in Aizome's
/// package (the path its polkit action names), or the same folder in a
/// build.
fn helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../libexec").join(HELPER), dir.join(HELPER)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// Run `change` through pkexec (polkit asks the session's agent, if it
/// asks), off this thread.
pub fn change(wake: &Wake, change: Change) -> Task<()> {
    let Some(helper) = helper() else {
        return Task::ready(Err(Error::Unavailable(format!("{HELPER} isn't installed"))));
    };
    Task::spawn("bluetooth-privacy", wake, move || {
        // On NixOS only the wrapper is setuid.
        let wrapper = Path::new("/run/wrappers/bin/pkexec");
        let pkexec = if wrapper.exists() { wrapper } else { Path::new("pkexec") };
        let out = std::process::Command::new(pkexec)
            .arg("--disable-internal-agent")
            .arg(helper)
            .args(change.args())
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| Error::Unavailable(format!("pkexec: {e}")))?;
        match out.status.code() {
            Some(0) => Ok(()),
            // pkexec's: dismissed, or not authorised.
            Some(126) => Err(Error::Cancelled),
            Some(127) => Err(Error::NotAuthorized("Not allowed to change Bluetooth".into())),
            _ => Err(Error::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIXOS: &str = "[General]\nControllerMode=dual\nFastConnectable=true\n\n[Policy]\nAutoEnable=true\n";
    const A: &str = "A8:7E:EA:4C:19:D1";
    const B: &str = "00:AA:01:00:00:23";

    fn storage(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aizome-bt-privacy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn device(storage: &Path, adapter: &str, device: &str, info: &str) {
        let dir = storage.join(adapter).join(device);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("info"), info).unwrap();
    }

    fn keyboard(storage: &Path, adapter: &str, key: &str) {
        device(storage, adapter, "F4:4E:FD:2A:03:6F", &format!("[General]\nName=K3\n\n[LongTermKey]\nKey={key}\n"));
    }

    fn identity(storage: &Path, adapter: &str) {
        std::fs::create_dir_all(storage.join(adapter)).unwrap();
        std::fs::write(storage.join(adapter).join("identity"), "[General]\nIdentityResolvingKey=0011\n").unwrap();
    }

    fn automatic(storage: &Path, before: Option<&Pairings>) -> (bool, Why) {
        let d = decide(Choice::Automatic, NIXOS, storage, before);
        (d.privacy, d.why)
    }

    #[test]
    fn privacy_goes_in_general_once() {
        let on = render(NIXOS, true);
        assert_eq!(
            on,
            "[General]\nPrivacy=device\nControllerMode=dual\nFastConnectable=true\n\n[Policy]\nAutoEnable=true\n"
        );
        assert_eq!(system_privacy(&on), Some(true));
        assert_eq!(system_privacy(&render(&on, false)), Some(false));
        assert_eq!(render(&on, false).matches("Privacy=").count(), 1);
        // No [General]: one at the top.
        assert_eq!(
            render("[Policy]\nAutoEnable=true\n", false),
            "[General]\nPrivacy=off\n\n[Policy]\nAutoEnable=true\n"
        );
        // Another section's key of the name stays.
        assert!(render("[General]\n[Other]\nPrivacy=x\n", true).ends_with("[Other]\nPrivacy=x\n"));
    }

    #[test]
    fn what_main_conf_sets_is_read_as_bluez_reads_it() {
        assert_eq!(system_privacy(NIXOS), None);
        assert_eq!(system_privacy("[General]\nPrivacy = network\n"), Some(true));
        assert_eq!(system_privacy("[General]\nPrivacy=limited-device\n"), Some(true));
        assert_eq!(system_privacy("[General]\nPrivacy=off\n"), Some(false));
        assert_eq!(system_privacy("[General]\nPrivacy=sometimes\n"), Some(false));
        assert_eq!(system_privacy("[General]\n#Privacy=device\n"), None);
        assert_eq!(system_privacy("[LE]\nPrivacy=device\n"), None);
    }

    #[test]
    fn main_conf_prepare_cannot_read_is_left_to_bluetoothd() {
        let s = storage("main-conf");
        assert_eq!(read_main_conf(&s.join("none")), Ok(String::new()));
        // A Latin-1 comment: GLib reads the rest; a copy would lose it.
        std::fs::write(s.join("latin1"), b"# caf\xe9\n[General]\nPrivacy=off\nControllerMode=bredr\n").unwrap();
        assert!(read_main_conf(&s.join("latin1")).is_err());
        std::fs::write(s.join("main.conf"), NIXOS).unwrap();
        assert_eq!(read_main_conf(&s.join("main.conf")).as_deref(), Ok(NIXOS));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn automatic_keeps_what_was_paired_working() {
        let s = storage("automatic");
        // Nothing paired, nothing there at all.
        assert_eq!(automatic(&s.join("none"), None), (true, Why::NoPairings));
        assert_eq!(automatic(&s, None), (true, Why::NoPairings));
        // A classic (BR/EDR) pairing: privacy is LE's only.
        device(&s, A, "F4:4E:FD:2A:01:25", "[General]\nName=WH-1000XM5\n\n[LinkKey]\nKey=00\n");
        std::fs::create_dir_all(s.join(A).join("cache")).unwrap();
        assert_eq!(automatic(&s, None), (true, Why::NoPairings));
        // A keyboard paired over LE before privacy was ever on.
        keyboard(&s, A, "00");
        assert_eq!(automatic(&s, None), (false, Why::EarlierPairings));
        // The person's choice comes first, and main.conf before that.
        assert_eq!(decide(Choice::On, NIXOS, &s, None).why, Why::Chosen);
        assert!(!decide(Choice::Off, NIXOS, &s.join("none"), None).privacy);
        let system = decide(Choice::On, "[General]\nPrivacy=off\n", &s, None);
        assert_eq!((system.privacy, system.why), (false, Why::System));
        // Its adapter has BlueZ's key for this computer: privacy was on there.
        identity(&s, A);
        assert_eq!(automatic(&s, None), (true, Why::AllHaveKey));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn one_adapters_key_says_nothing_of_another_adapters_pairings() {
        let s = storage("adapters");
        identity(&s, A);
        keyboard(&s, B, "00");
        assert_eq!(automatic(&s, None), (false, Why::EarlierPairings));
        // A random static adapter's folder.
        let s2 = storage("static");
        keyboard(&s2, &format!("static-{B}"), "00");
        assert_eq!(automatic(&s2, None), (false, Why::EarlierPairings));
        identity(&s2, &format!("static-{B}"));
        assert_eq!(automatic(&s2, None), (true, Why::AllHaveKey));
        let _ = std::fs::remove_dir_all(&s);
        let _ = std::fs::remove_dir_all(&s2);
    }

    #[test]
    fn pairings_made_while_off_are_remembered_as_without_the_key() {
        let s = storage("history");
        // Started with nothing paired: on.
        let first = decide(Choice::Automatic, NIXOS, &s, None);
        assert_eq!((first.privacy, first.why), (true, Why::NoPairings));
        // A controller paired meanwhile has the key.
        device(&s, A, "78:86:2E:DC:05:B1", "[LongTermKey]\nKey=AA\n");
        let second = decide(Choice::Automatic, NIXOS, &s, Some(&first.pairings));
        assert_eq!((second.privacy, second.why), (true, Why::AllHaveKey));
        // Turned off, and a keyboard paired meanwhile: it hasn't, though
        // the adapter has BlueZ's key from before.
        identity(&s, A);
        let off = decide(Choice::Off, NIXOS, &s, Some(&second.pairings));
        keyboard(&s, A, "00");
        let back = decide(Choice::Automatic, NIXOS, &s, Some(&off.pairings));
        assert_eq!((back.privacy, back.why), (false, Why::EarlierPairings));
        assert_eq!((back.pairings.keyed.len(), back.pairings.unkeyed.len()), (1, 1));
        // Paired again with privacy chosen on (new keys): it has it now.
        let on = decide(Choice::On, NIXOS, &s, Some(&back.pairings));
        keyboard(&s, A, "11");
        let after = decide(Choice::Automatic, NIXOS, &s, Some(&on.pairings));
        assert_eq!((after.privacy, after.why), (true, Why::AllHaveKey));
        // What's forgotten is forgotten.
        let _ = std::fs::remove_dir_all(s.join(A).join("F4:4E:FD:2A:03:6F"));
        assert_eq!(decide(Choice::Automatic, NIXOS, &s, Some(&after.pairings)).pairings.keyed.len(), 1);
        // Started from the system's main.conf, unknown: none since has it.
        let unknown = Pairings { privacy: None, ..after.pairings.clone() };
        keyboard(&s, A, "22");
        assert_eq!(automatic(&s, Some(&unknown)), (false, Why::EarlierPairings));
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn older_and_peripheral_keys_are_pairings_too() {
        for group in ["[PeripheralLongTermKey]", "[SlaveLongTermKey]"] {
            let s = storage("groups");
            device(&s, B, "11:22:33:44:55:66", &format!("{group}\nKey=00\n"));
            assert_eq!(le_pairings(&s).unwrap().len(), 1, "{group}");
            let _ = std::fs::remove_dir_all(&s);
        }
    }

    #[test]
    fn the_kept_pairings_read_back() {
        let s = storage("kept");
        keyboard(&s, A, "00");
        let d = decide(Choice::Off, NIXOS, &s, None);
        std::fs::write(s.join("kept.toml"), d.pairings.to_toml()).unwrap();
        assert_eq!(Pairings::read_from(&s.join("kept.toml")), Some(d.pairings));
        assert_eq!(Pairings::read_from(&s.join("none.toml")), None);
        let _ = std::fs::remove_dir_all(&s);
    }

    fn running(privacy: bool, why: Why, choice: Choice) -> Running {
        Running { privacy, why, choice, main_conf: "/nix/store/a-main.conf".into() }
    }

    #[test]
    fn a_restart_is_offered_only_when_it_would_change_something() {
        let same = Some("/nix/store/a-main.conf".to_owned());
        let status = |r: Option<Running>, c: Choice, now: Option<String>| Status::new(r, c, None, now).restart;
        // Started before Aizome chose for it, or the system changed since.
        assert_eq!(status(None, Choice::Automatic, same.clone()), Some(Restart::Update));
        let r = running(true, Why::NoPairings, Choice::Automatic);
        assert_eq!(
            status(Some(r.clone()), Choice::Automatic, Some("/nix/store/b-main.conf".into())),
            Some(Restart::Update)
        );
        // Nothing changed.
        assert_eq!(status(Some(r.clone()), Choice::Automatic, same.clone()), None);
        // On chosen while automatic has it on: nothing to restart for.
        assert_eq!(status(Some(r.clone()), Choice::On, same.clone()), None);
        assert_eq!(status(Some(r), Choice::Off, same.clone()), Some(Restart::Choice));
        let off = running(false, Why::Chosen, Choice::Off);
        assert_eq!(status(Some(off.clone()), Choice::On, same.clone()), Some(Restart::Choice));
        // Back to automatic: unknown without root, so offered.
        assert_eq!(status(Some(off), Choice::Automatic, same.clone()), Some(Restart::Choice));
        // main.conf decides, or was read as it is: no choice changes it.
        assert_eq!(status(Some(running(false, Why::System, Choice::Automatic)), Choice::On, same.clone()), None);
        assert_eq!(status(Some(running(false, Why::Unprepared, Choice::Automatic)), Choice::On, same), None);
    }

    #[test]
    fn a_restart_that_turns_it_on_is_known_as_one() {
        let off = Some(running(false, Why::EarlierPairings, Choice::Automatic));
        assert!(Status::new(off.clone(), Choice::On, None, None).restart_turns_on());
        assert!(!Status::new(off, Choice::Off, None, None).restart_turns_on());
        let on = Some(running(true, Why::NoPairings, Choice::Automatic));
        assert!(!Status::new(on, Choice::Off, None, None).restart_turns_on());
    }

    #[test]
    fn what_comes_next_is_what_settings_shows() {
        let r = Some(running(false, Why::EarlierPairings, Choice::Automatic));
        assert_eq!(Status::new(r.clone(), Choice::Automatic, None, None).next(), Some(false));
        assert_eq!(Status::new(r.clone(), Choice::On, None, None).next(), Some(true));
        assert_eq!(Status::new(r, Choice::On, Some(false), None).next(), Some(false));
        assert_eq!(Status::new(None, Choice::Automatic, None, None).next(), None);
        let unprepared = Status::new(Some(running(false, Why::Unprepared, Choice::On)), Choice::On, None, None);
        assert!(unprepared.unprepared() && unprepared.next().is_none());
    }

    #[test]
    fn the_record_and_the_choice_read_back() {
        let r = running(true, Why::AllHaveKey, Choice::Automatic);
        assert_eq!(Running::parse(&r.to_toml()), Some(r));
        let s = storage("choice");
        assert_eq!(Choice::read_from(&s.join("missing")), Choice::Automatic);
        std::fs::write(s.join("choice"), "off\n").unwrap();
        assert_eq!(Choice::read_from(&s.join("choice")), Choice::Off);
        std::fs::write(s.join("choice"), "maybe\n").unwrap();
        assert_eq!(Choice::read_from(&s.join("choice")), Choice::Automatic);
        assert_eq!(Change::Set(Choice::Off).args(), ["set", "off"]);
        assert_eq!(Change::Restart.args(), ["restart"]);
        let _ = std::fs::remove_dir_all(&s);
    }

    #[test]
    fn status_reads_only_where_aizome_chooses() {
        let s = storage("status");
        let paths = Paths {
            managed: s.join("managed"),
            choice: s.join("choice"),
            running: s.join("state.toml"),
            main_conf: s.join("main.conf"),
        };
        assert_eq!(Status::read_from(&paths), None);
        std::fs::write(&paths.managed, "").unwrap();
        std::fs::write(&paths.main_conf, NIXOS).unwrap();
        let now = std::fs::canonicalize(&paths.main_conf).unwrap().display().to_string();
        let r = Running { main_conf: now.clone(), ..running(true, Why::NoPairings, Choice::Automatic) };
        std::fs::write(&paths.running, r.to_toml()).unwrap();
        let status = Status::read_from(&paths).unwrap();
        assert_eq!((status.restart, status.next(), status.system), (None, Some(true), None));
        std::fs::write(&paths.choice, "off").unwrap();
        assert_eq!(Status::read_from(&paths).unwrap().restart, Some(Restart::Choice));
        // main.conf that can't be read here (root's alone, or not UTF-8):
        // the record says the system sets it.
        std::fs::write(&paths.main_conf, b"# caf\xe9\n[General]\nPrivacy=off\n").unwrap();
        let r = Running { main_conf: now, ..running(false, Why::System, Choice::Automatic) };
        std::fs::write(&paths.running, r.to_toml()).unwrap();
        assert_eq!(Status::read_from(&paths).unwrap().system, Some(false));
        let _ = std::fs::remove_dir_all(&s);
    }
}
