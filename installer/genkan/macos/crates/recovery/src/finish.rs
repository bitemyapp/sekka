// SPDX-License-Identifier: GPL-3.0-or-later
//! The steps, in order, with every check before them.
//!
//! 1. The app is on the volume the configuration names, the ESP's handoff
//!    names the same installation, and the boot object and firmware are the
//!    bytes the installation wrote.
//! 2. This is the stub's own (paired) Recovery, started by holding the
//!    power button (one true recoveryOS): only there can the policy change.
//! 3. The stub's System volume, which recoveryOS mounts read-only, is made
//!    writable before anything is written to it.
//! 4. `bputil -nc` allows Sekka's startup software, for this volume group
//!    only; macOS's own security setting is untouched.
//! 5. `kmutil configure-boot` installs the checked boot object, the stub
//!    shows its SystemVersion again so that it starts Sekka, Recovery stops
//!    opening this app at startup, and `bless` makes Sekka the startup
//!    disk, read back afterwards.
//!
//! The same steps as Asahi's `step2.sh` (MIT), with the checks it leaves to
//! the person made by the app. `step2.sh` writes nothing to the volume until
//! `kmutil` has run, and remounts it then; this app keeps a receipt from the
//! first step, so it remounts first, as omacom/omarchy-mac-installer's
//! Recovery does before it writes its own state. What the Mac says decides
//! what is repeated; the receipt only records it.
use std::fs;
use std::path::Path;

use sekka_mac_contract::{Handoff, Text, Timestamp, Uuid};

use crate::install::Install;
use crate::pty::{AuthError, Program};
use crate::receipt::{Receipt, Step, Store};

pub trait System {
    /// The volume UUID and volume group UUID of the volume mounted at
    /// `path`.
    fn volume(&self, path: &Path) -> Result<(Uuid, Uuid), String>;
    /// The device of the volume mounted at `path` (`disk3s1`).
    fn device(&self, path: &Path) -> Result<String, String>;
    /// `bputil -d -v GROUP`.
    fn policy(&self, group: Uuid) -> Result<String, String>;
    /// `sekka/install.json` from the ESP with this partition UUID.
    fn handoff(&self, esp: Uuid) -> Result<Vec<u8>, String>;
    fn authorized(
        &self,
        program: Program,
        args: &[&str],
        user: &str,
        password: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), AuthError>;
    /// Make the volume mounted at `path` writable if it isn't
    /// (`mount -u -w`), and read that back.
    fn make_writable(&self, path: &Path) -> Result<(), String>;
    /// The device the Mac will start from next (`bless --getBoot`).
    fn startup_device(&self) -> Result<String, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    /// Sekka's own Recovery, started by holding the power button.
    Ready,
    /// Another system's Recovery (macOS's, usually).
    WrongRecovery,
    /// Sekka's Recovery, but not started by holding the power button.
    NotOneTrue,
}

/// Read `bputil -d`'s description of where the app is running.
pub fn environment(policy: &str) -> Environment {
    if !policy.contains(": Paired") {
        Environment::WrongRecovery
    } else if !policy.contains("one true recoveryOS") {
        Environment::NotOneTrue
    } else {
        Environment::Ready
    }
}

/// Whether `bputil -d`'s description says Sekka's startup software is
/// already allowed: its security mode is permissive, as `bputil -nc`
/// leaves it. When this can't tell, `bputil -nc` runs again, before
/// `kmutil` as always, which only repeats what it did.
pub fn allows(policy: &str) -> bool {
    policy
        .lines()
        .map(|l| l.trim().to_lowercase())
        .any(|l| l.starts_with("security mode") && l.contains("permissive"))
}

/// Show the stub's SystemVersion again, so that starting it starts Sekka:
/// the installation hid it (Asahi's `prepare_for_step2`) so that starting
/// it opened Recovery, and this app. Read back afterwards.
pub fn show_system_version(volume: &Path) -> Result<(), String> {
    let services = volume.join("System/Library/CoreServices");
    let shown = services.join("SystemVersion.plist");
    let hidden = services.join("SystemVersion-disabled.plist");
    if hidden.exists() {
        fs::rename(&hidden, &shown).map_err(|e| e.to_string())?;
    }
    if !shown.is_file() {
        return Err(format!("{} isn't there", shown.display()));
    }
    Ok(())
}

/// Stop Recovery opening this app at every start: rename the stub's
/// `.IAPhysicalMedia`, as `step2.sh` does once it has finished.
pub fn disable_autolaunch(volume: &Path) -> Result<(), String> {
    let media = volume.join(".IAPhysicalMedia");
    if media.exists() {
        fs::rename(&media, volume.join("IAPhysicalMedia-disabled.plist")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Allowing,
    Installing,
    Selecting,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FinishError {
    #[error("That password isn't right.")]
    Password,
    #[error("{0}")]
    Failed(String),
}

pub struct Finisher<S: System> {
    pub system: S,
    pub install: Install,
    store: Store,
    /// What this app has done, as far as it could record it.
    pub receipt: Receipt,
    /// Why the receipt found at launch was set aside, for Details.
    set_aside: Option<String>,
    /// The policy last read already allows Sekka's startup software.
    allowed: bool,
    /// A save of the receipt has failed during this attempt.
    unrecorded: bool,
}

fn text(s: &str) -> Text {
    let clean: String = s.chars().filter(|c| !c.is_control()).take(1000).collect();
    clean.parse().unwrap_or_else(|_| "failed".parse().unwrap())
}

impl<S: System> Finisher<S> {
    /// Never refused for its receipt: nothing depends on it, so one that
    /// can't be read is set aside, said in Details, and replaced.
    pub fn open(system: S, install: Install) -> Self {
        let store = Store::at(&install.volume);
        let installation = install.config.installation;
        let (receipt, set_aside) = match store.load(installation) {
            Ok(r) => (r, None),
            Err(e) => (Receipt::new(installation), Some(e)),
        };
        Self {
            system,
            install,
            store,
            receipt,
            set_aside,
            allowed: false,
            unrecorded: false,
        }
    }

    pub fn owner(&self) -> (&str, &str) {
        let o = &self.install.config.owner;
        (o.short_name.as_str(), o.full_name.as_str())
    }

    pub fn done(&self) -> bool {
        matches!(self.receipt.selection, Step::Completed { .. })
    }

    /// Save the receipt. It is diagnostics, never authority, so one that
    /// can't be saved is said in Details, once an attempt, and the steps go
    /// on.
    fn record(&mut self, log: &mut dyn FnMut(&str)) {
        if let Err(e) = self.store.save(&self.receipt)
            && !std::mem::replace(&mut self.unrecorded, true)
        {
            log(&format!(
                "This app's record of its steps couldn't be saved ({e}). It's only kept for diagnostics, so the steps go on.\n"
            ));
        }
    }

    /// Record `e` as the selection's failure, and return it.
    fn selection_failed(&mut self, e: FinishError, log: &mut dyn FnMut(&str)) -> FinishError {
        self.receipt.selection = Step::Failed {
            at: Timestamp::now(),
            message: text(&e.to_string()),
        };
        self.record(log);
        e
    }

    /// Run one of the selection's tools; a failure is the selection's.
    fn select(
        &mut self,
        program: Program,
        args: &[&str],
        user: &str,
        password: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), FinishError> {
        match self.system.authorized(program, args, user, password, log) {
            Ok(()) => Ok(()),
            Err(AuthError::PasswordRejected) => Err(self.selection_failed(FinishError::Password, log)),
            Err(AuthError::Failed(m)) => Err(self.selection_failed(FinishError::Failed(m), log)),
        }
    }

    /// Everything that must hold before a password is asked for. It only
    /// reads: recoveryOS mounts the volume read-only, and nothing changes
    /// before the owner chooses Install.
    pub fn check(&mut self) -> Result<Environment, String> {
        let c = &self.install.config;
        let (volume, group) = self.system.volume(&self.install.volume)?;
        if volume != c.stub.system_volume || group != c.stub.volume_group {
            return Err("This app isn't on the volume its installation was made for. Start the installation again from macOS.".into());
        }
        let handoff = Handoff::parse(&self.system.handoff(c.esp)?)
            .map_err(|e| format!("Sekka's boot partition doesn't describe this installation ({e})."))?;
        if handoff.installation != c.installation || handoff.esp != c.esp || handoff.root != c.root {
            return Err("Sekka's boot partition belongs to another installation. Start the installation again from macOS.".into());
        }
        self.install.verify_files()?;
        let policy = self.system.policy(group)?;
        // Whether bputil has already run, after a crash part way or with a
        // receipt that couldn't be saved: believe the Mac, not the receipt.
        self.allowed = allows(&policy);
        Ok(environment(&policy))
    }

    /// Allow, install and select, skipping what is already done. Safe to
    /// call again after any failure: it repeats only these steps, for this
    /// volume, with the same checked boot object.
    pub fn finish(
        &mut self,
        password: &str,
        progress: &mut dyn FnMut(Phase),
        log: &mut dyn FnMut(&str),
    ) -> Result<(), FinishError> {
        if self.check().map_err(FinishError::Failed)? != Environment::Ready {
            return Err(FinishError::Failed("This isn't Sekka's own Recovery.".into()));
        }
        self.unrecorded = false;
        if let Some(why) = self.set_aside.take() {
            log(&format!("{why} A new one is kept from now on.\n"));
        }
        // Before anything is written to the stub's System volume, the
        // receipt included. Sekka can't start until its SystemVersion is
        // shown there again, so a volume that stays read-only stops here,
        // before the password is used.
        self.system.make_writable(&self.install.volume).map_err(|m| {
            FinishError::Failed(format!(
                "Sekka's startup volume can't be written to here ({m}), and this app needs to write to it to finish. Nothing was changed."
            ))
        })?;
        self.receipt.attempts += 1;
        let c = self.install.config.clone();
        let user = c.owner.short_name.to_string();
        let group = c.stub.volume_group.to_upper();
        let volume = self.install.volume.to_string_lossy().into_owned();

        if self.allowed {
            if !matches!(self.receipt.enrollment, Step::Completed { .. }) {
                self.receipt.enrollment = Step::Completed { at: Timestamp::now() };
            }
        } else {
            progress(Phase::Allowing);
            self.receipt.enrollment = Step::Started { at: Timestamp::now() };
            self.record(log);
            match self.system.authorized(Program::Bputil, &["-nc", "-v", &group], &user, password, log) {
                Ok(()) => self.receipt.enrollment = Step::Completed { at: Timestamp::now() },
                Err(AuthError::PasswordRejected) => {
                    // Refused before anything changed.
                    self.receipt.enrollment = Step::NotStarted;
                    self.record(log);
                    return Err(FinishError::Password);
                }
                Err(AuthError::Failed(m)) => {
                    self.receipt.enrollment = Step::Failed {
                        at: Timestamp::now(),
                        message: text(&m),
                    };
                    self.record(log);
                    return Err(FinishError::Failed(m));
                }
            }
            self.record(log);
        }

        progress(Phase::Installing);
        self.install.verify_files().map_err(FinishError::Failed)?;
        self.receipt.selection = Step::Started { at: Timestamp::now() };
        self.record(log);
        let boot = self.install.path(&c.boot_object).to_string_lossy().into_owned();
        self.select(
            Program::Kmutil,
            &[
                "configure-boot",
                "-c",
                &boot,
                "--raw",
                "--entry-point",
                "2048",
                "--lowest-virtual-address",
                "0",
                "-v",
                &volume,
            ],
            &user,
            password,
            log,
        )?;

        progress(Phase::Selecting);
        // As `step2.sh` does once kmutil has run, making the volume writable
        // again first as it does. SystemVersion first: stopped between the
        // two, the stub starts Sekka and Recovery would open this app again,
        // where the other way round it would open a Recovery without it.
        let shown = self
            .system
            .make_writable(&self.install.volume)
            .and_then(|()| show_system_version(&self.install.volume));
        if let Err(m) = shown {
            let e = FinishError::Failed(format!(
                "Sekka's startup volume couldn't be changed to start Sekka instead of Recovery ({m})."
            ));
            return Err(self.selection_failed(e, log));
        }
        if let Err(m) = disable_autolaunch(&self.install.volume) {
            log(&format!("Recovery will still open this app at startup: {m}\n"));
        }
        self.select(Program::Bless, &["--mount", &volume, "--setBoot", "--user", &user], &user, password, log)?;
        let expected = self.system.device(&self.install.volume).map_err(FinishError::Failed)?;
        let selected = self.system.startup_device().map_err(FinishError::Failed)?;
        if !selected.trim_start_matches("/dev/").starts_with(&expected) {
            let m = format!("macOS reported success, but the Mac will start from {selected}, not Sekka's volume.");
            return Err(self.selection_failed(FinishError::Failed(m), log));
        }
        self.receipt.selection = Step::Completed { at: Timestamp::now() };
        self.record(log);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sekka_mac_contract::{RECOVERY_RECEIPT_FILE, RECOVERY_STATE_DIR, canonical};
    use std::cell::{Cell, RefCell};
    use std::os::unix::fs::PermissionsExt;

    struct Fake {
        volume: (Uuid, Uuid),
        handoff: Vec<u8>,
        policy: RefCell<String>,
        calls: RefCell<Vec<Program>>,
        refuse: RefCell<Vec<(Program, AuthError)>>,
        startup: String,
        /// What `mount -u -w` answers.
        remount: Result<(), String>,
        remounts: Cell<usize>,
        /// The receipt on the volume as bputil starts.
        receipt_at_bputil: RefCell<Option<Receipt>>,
        store: Store,
        installation: Uuid,
    }
    impl System for &Fake {
        fn volume(&self, _: &Path) -> Result<(Uuid, Uuid), String> {
            Ok(self.volume)
        }
        fn device(&self, _: &Path) -> Result<String, String> {
            Ok("disk3s1".into())
        }
        fn policy(&self, _: Uuid) -> Result<String, String> {
            Ok(self.policy.borrow().clone())
        }
        fn handoff(&self, _: Uuid) -> Result<Vec<u8>, String> {
            Ok(self.handoff.clone())
        }
        fn authorized(&self, program: Program, args: &[&str], user: &str, password: &str, _: &mut dyn FnMut(&str)) -> Result<(), AuthError> {
            assert_eq!(user, "chris");
            assert_eq!(password, "right");
            if program == Program::Bputil {
                assert_eq!(args, ["-nc", "-v", "03030303-0303-0303-0303-030303030303"]);
                *self.receipt_at_bputil.borrow_mut() = self.store.load(self.installation).ok();
            }
            self.calls.borrow_mut().push(program);
            let mut refuse = self.refuse.borrow_mut();
            if let Some(i) = refuse.iter().position(|(p, _)| *p == program) {
                return Err(refuse.remove(i).1);
            }
            if program == Program::Bputil {
                *self.policy.borrow_mut() = PAIRED_1TR.replace("Full Security", "Permissive Security");
            }
            Ok(())
        }
        fn make_writable(&self, path: &Path) -> Result<(), String> {
            self.remounts.set(self.remounts.get() + 1);
            self.remount.clone()?;
            mount(path, true);
            Ok(())
        }
        fn startup_device(&self) -> Result<String, String> {
            Ok(self.startup.clone())
        }
    }

    const PAIRED_1TR: &str = "Current OS environment: one true recoveryOS\nLocal policy for volume group: Paired\nSecurity Mode: Full Security\n";

    /// recoveryOS's read-only mount, as permissions: nothing on the volume
    /// can be created, renamed or replaced until it is writable again.
    /// Permissions don't hold root back, so as root this shows less.
    fn mount(dir: &Path, writable: bool) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && !path.is_symlink() {
                mount(&path, writable);
            }
        }
        let mode = if writable { 0o755 } else { 0o555 };
        fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn fake(install: &Install) -> Fake {
        let c = &install.config;
        let handoff = Handoff {
            schema: sekka_mac_contract::HandoffSchema,
            installation: c.installation,
            release: c.release.clone(),
            source_revision: "4882dc33652612e7475f467f6373d8b607e5407c".parse().unwrap(),
            root: c.root,
            esp: c.esp,
            created: Timestamp::from_unix(1),
            hints: Default::default(),
        };
        Fake {
            volume: (c.stub.system_volume, c.stub.volume_group),
            handoff: handoff.to_bytes().unwrap(),
            policy: RefCell::new(PAIRED_1TR.into()),
            calls: RefCell::new(vec![]),
            refuse: RefCell::new(vec![]),
            startup: "/dev/disk3s1".into(),
            remount: Ok(()),
            remounts: Cell::new(0),
            receipt_at_bputil: RefCell::new(None),
            store: Store::at(&install.volume),
            installation: c.installation,
        }
    }

    /// An installation as macOS's part leaves the stub: its SystemVersion
    /// hidden, and Recovery told to open this app.
    fn setup() -> (tempfile::TempDir, Install) {
        let dir = tempfile::tempdir().unwrap();
        let install = crate::install::tests::fixture(dir.path());
        let services = dir.path().join("System/Library/CoreServices");
        fs::create_dir_all(&services).unwrap();
        fs::write(services.join("SystemVersion-disabled.plist"), b"<plist/>").unwrap();
        fs::write(dir.path().join(".IAPhysicalMedia"), b"<plist/>").unwrap();
        (dir, install)
    }

    fn system_version(volume: &Path) -> std::path::PathBuf {
        volume.join("System/Library/CoreServices/SystemVersion.plist")
    }

    #[test]
    fn the_steps_run_once_each_and_are_recorded() {
        let (dir, install) = setup();
        let f = fake(&install);
        mount(dir.path(), false);
        let mut finisher = Finisher::open(&f, install);
        assert_eq!(finisher.check().unwrap(), Environment::Ready);
        assert_eq!(f.remounts.get(), 0, "checking changes nothing");
        let mut phases = vec![];
        let mut log = String::new();
        finisher.finish("right", &mut |p| phases.push(p), &mut |l| log.push_str(l)).unwrap();
        assert_eq!(phases, [Phase::Allowing, Phase::Installing, Phase::Selecting]);
        assert_eq!(*f.calls.borrow(), [Program::Bputil, Program::Kmutil, Program::Bless]);
        let before = f.receipt_at_bputil.borrow().clone().expect("a receipt on the volume before bputil");
        assert!(
            matches!(before.enrollment, Step::Started { .. }),
            "written once the volume was writable, before bputil ran"
        );
        let again = Store::at(dir.path()).load(finisher.install.config.installation).unwrap();
        assert!(again.finished());
        assert_eq!(again.attempts, 1);
        assert!(finisher.done());
        assert!(system_version(dir.path()).is_file(), "the stub starts Sekka");
        assert!(!dir.path().join(".IAPhysicalMedia").exists());
        assert!(dir.path().join("IAPhysicalMedia-disabled.plist").is_file());
        assert_eq!(log, "", "nothing went wrong");
    }

    #[test]
    fn a_volume_that_stays_read_only_stops_before_any_password() {
        let (dir, install) = setup();
        let mut f = fake(&install);
        f.remount = Err("mount: Operation not permitted".into());
        mount(dir.path(), false);
        let mut finisher = Finisher::open(&f, install);
        let e = finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap_err();
        mount(dir.path(), true);
        assert!(
            matches!(&e, FinishError::Failed(m) if m.contains("Operation not permitted") && m.contains("Nothing was changed")),
            "{e}"
        );
        assert!(f.calls.borrow().is_empty());
        assert!(!dir.path().join(RECOVERY_STATE_DIR).exists());
        assert!(dir.path().join(".IAPhysicalMedia").exists());
    }

    #[test]
    fn a_receipt_that_cant_be_saved_never_stops_the_steps() {
        let (dir, install) = setup();
        let f = fake(&install);
        // Where its directory would be, something else.
        fs::write(dir.path().join(RECOVERY_STATE_DIR), b"").unwrap();
        let mut finisher = Finisher::open(&f, install);
        let mut log = String::new();
        finisher.finish("right", &mut |_| {}, &mut |l| log.push_str(l)).unwrap();
        assert_eq!(*f.calls.borrow(), [Program::Bputil, Program::Kmutil, Program::Bless]);
        assert_eq!(log.matches("couldn't be saved").count(), 1, "said once: {log}");
        assert!(system_version(dir.path()).is_file());
    }

    #[test]
    fn a_receipt_that_cant_be_used_is_set_aside() {
        let foreign = canonical::to_vec(&Receipt::new(Uuid::from_bytes([9; 16]))).unwrap();
        for found in [b"not a receipt".to_vec(), foreign] {
            let (dir, install) = setup();
            let f = fake(&install);
            let state = dir.path().join(RECOVERY_STATE_DIR);
            fs::create_dir_all(&state).unwrap();
            fs::write(state.join(RECOVERY_RECEIPT_FILE), &found).unwrap();
            let installation = install.config.installation;
            let mut finisher = Finisher::open(&f, install);
            let mut log = String::new();
            finisher.finish("right", &mut |_| {}, &mut |l| log.push_str(l)).unwrap();
            assert!(log.contains("A new one is kept"), "{log}");
            assert!(Store::at(dir.path()).load(installation).unwrap().finished());
        }
    }

    #[test]
    fn the_wrong_volume_or_installation_is_refused_before_any_password() {
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.volume.0 = Uuid::from_bytes([9; 16]);
        assert!(Finisher::open(&f, install).check().is_err());
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.handoff = {
            let mut h = Handoff::parse(&f.handoff).unwrap();
            h.installation = Uuid::from_bytes([9; 16]);
            h.to_bytes().unwrap()
        };
        assert!(Finisher::open(&f, install).check().is_err());
    }

    #[test]
    fn only_paired_one_true_recovery_will_do() {
        assert_eq!(environment("Local policy for volume group: Not paired"), Environment::WrongRecovery);
        assert_eq!(environment("Current OS environment: recoveryOS\nfoo: Paired"), Environment::NotOneTrue);
        let (_dir, install) = setup();
        let f = fake(&install);
        *f.policy.borrow_mut() = "Current OS environment: recoveryOS\nLocal policy: Paired".into();
        let mut finisher = Finisher::open(&f, install);
        assert!(finisher.finish("right", &mut |_| {}, &mut |_| {}).is_err());
        assert!(f.calls.borrow().is_empty());
        assert_eq!(f.remounts.get(), 0);
    }

    #[test]
    fn only_a_permissive_security_mode_allows_sekka() {
        assert!(allows(&PAIRED_1TR.replace("Full Security", "Permissive Security")));
        assert!(allows("  Security Mode: Permissive (smb0): present\n"));
        assert!(!allows(PAIRED_1TR));
        assert!(!allows("Security Mode: Full Security\nNot permissive.\n"));
    }

    #[test]
    fn a_wrong_password_changes_nothing_and_can_be_retried() {
        let (_dir, install) = setup();
        let f = fake(&install);
        f.refuse.borrow_mut().push((Program::Bputil, AuthError::PasswordRejected));
        let mut finisher = Finisher::open(&f, install);
        assert_eq!(finisher.finish("right", &mut |_| {}, &mut |_| {}), Err(FinishError::Password));
        assert_eq!(finisher.receipt.enrollment, Step::NotStarted);
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
    }

    #[test]
    fn a_retry_after_kmutil_fails_skips_what_is_done() {
        let (_dir, install) = setup();
        let f = fake(&install);
        f.refuse.borrow_mut().push((Program::Kmutil, AuthError::Failed("kmutil: busy".into())));
        let mut finisher = Finisher::open(&f, install);
        assert!(finisher.finish("right", &mut |_| {}, &mut |_| {}).is_err());
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(
            *f.calls.borrow(),
            [Program::Bputil, Program::Kmutil, Program::Kmutil, Program::Bless],
            "bputil isn't run again"
        );
    }

    #[test]
    fn an_uncertain_policy_change_is_read_from_the_mac() {
        let (dir, install) = setup();
        let f = fake(&install);
        *f.policy.borrow_mut() = PAIRED_1TR.replace("Full Security", "Permissive Security");
        let store = Store::at(dir.path());
        let installation = install.config.installation;
        let mut r = store.load(installation).unwrap();
        r.enrollment = Step::Started { at: Timestamp::from_unix(1) };
        store.save(&r).unwrap();
        let mut finisher = Finisher::open(&f, install);
        finisher.check().unwrap();
        assert_eq!(store.load(installation).unwrap(), r, "checking writes nothing");
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(*f.calls.borrow(), [Program::Kmutil, Program::Bless]);
        assert!(store.load(installation).unwrap().finished());
    }

    #[test]
    fn the_mac_not_the_receipt_says_whether_bputil_has_run() {
        let (dir, install) = setup();
        let f = fake(&install);
        let store = Store::at(dir.path());
        let mut r = store.load(install.config.installation).unwrap();
        r.enrollment = Step::Completed { at: Timestamp::from_unix(1) };
        store.save(&r).unwrap();
        let mut finisher = Finisher::open(&f, install);
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(
            *f.calls.borrow(),
            [Program::Bputil, Program::Kmutil, Program::Bless],
            "the Mac's policy is still Full Security"
        );
    }

    #[test]
    fn sekka_is_set_to_start_before_recovery_stops_opening_the_app() {
        let (dir, install) = setup();
        let f = fake(&install);
        // In the way, and not something a rename replaces.
        fs::create_dir(system_version(dir.path())).unwrap();
        let mut finisher = Finisher::open(&f, install);
        let e = finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(&e, FinishError::Failed(m) if m.contains("start Sekka")), "{e}");
        assert!(matches!(finisher.receipt.selection, Step::Failed { .. }));
        assert!(dir.path().join(".IAPhysicalMedia").exists(), "Recovery opens this app to try again");
        assert!(!f.calls.borrow().contains(&Program::Bless));
        fs::remove_dir(system_version(dir.path())).unwrap();
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(*f.calls.borrow(), [Program::Bputil, Program::Kmutil, Program::Kmutil, Program::Bless]);
        assert!(system_version(dir.path()).is_file());
    }

    #[test]
    fn a_changed_boot_object_is_never_installed() {
        let (_dir, install) = setup();
        let f = fake(&install);
        let boot = install.path(&install.config.boot_object);
        let mut finisher = Finisher::open(&f, install);
        std::fs::write(boot, b"something else").unwrap();
        assert!(finisher.finish("right", &mut |_| {}, &mut |_| {}).is_err());
        assert!(!f.calls.borrow().contains(&Program::Kmutil));
    }

    #[test]
    fn a_selection_that_didnt_take_is_reported() {
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.startup = "/dev/disk1s1".into();
        let mut finisher = Finisher::open(&f, install);
        let e = finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, FinishError::Failed(m) if m.contains("disk1s1")));
    }
}
