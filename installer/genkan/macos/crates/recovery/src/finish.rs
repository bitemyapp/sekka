// SPDX-License-Identifier: GPL-3.0-or-later
//! The steps, in order, with every check before them.
//!
//! 1. The app is on the volume the configuration names, the ESP's handoff
//!    names the same installation, and the boot object and firmware are the
//!    bytes the installation wrote.
//! 2. This is the stub's own (paired) Recovery, started by holding the
//!    power button (one true recoveryOS): only there can the policy change.
//! 3. `bputil -nc` allows Sekka's startup software, for this volume group
//!    only; macOS's own security setting is untouched.
//! 4. `kmutil configure-boot` installs the checked boot object, Recovery
//!    stops opening this app at startup, and `bless` makes Sekka the startup
//!    disk, read back afterwards.
//!
//! The same steps as Asahi's `step2.sh` (MIT), with the checks it leaves to
//! the person made by the app.
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
    /// Stop Recovery opening this app at every start (rename its
    /// `.IAPhysicalMedia`), as `step2.sh` does once it has finished.
    fn disable_autolaunch(&self, volume: &Path) -> Result<(), String>;
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
    pub receipt: Receipt,
}

fn text(s: &str) -> Text {
    let clean: String = s.chars().filter(|c| !c.is_control()).take(1000).collect();
    clean.parse().unwrap_or_else(|_| "failed".parse().unwrap())
}

impl<S: System> Finisher<S> {
    pub fn open(system: S, install: Install) -> Result<Self, String> {
        let store = Store::at(&install.volume);
        let receipt = store.load(install.config.installation)?;
        Ok(Self {
            system,
            install,
            store,
            receipt,
        })
    }

    pub fn owner(&self) -> (&str, &str) {
        let o = &self.install.config.owner;
        (o.short_name.as_str(), o.full_name.as_str())
    }

    pub fn done(&self) -> bool {
        matches!(self.receipt.selection, Step::Completed { .. })
    }

    fn save(&self) -> Result<(), FinishError> {
        self.store.save(&self.receipt).map_err(FinishError::Failed)
    }

    /// Everything that must hold before a password is asked for.
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
        // A policy change that may have happened before a crash: believe
        // the Mac, not the receipt.
        if matches!(self.receipt.enrollment, Step::Started { .. }) && policy.to_lowercase().contains("permissive") {
            self.receipt.enrollment = Step::Completed { at: Timestamp::now() };
            let _ = self.store.save(&self.receipt);
        }
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
        self.receipt.attempts += 1;
        let c = self.install.config.clone();
        let user = c.owner.short_name.to_string();
        let group = c.stub.volume_group.to_upper();
        let volume = self.install.volume.to_string_lossy().into_owned();

        if !matches!(self.receipt.enrollment, Step::Completed { .. }) {
            progress(Phase::Allowing);
            self.receipt.enrollment = Step::Started { at: Timestamp::now() };
            self.save()?;
            match self.system.authorized(Program::Bputil, &["-nc", "-v", &group], &user, password, log) {
                Ok(()) => self.receipt.enrollment = Step::Completed { at: Timestamp::now() },
                Err(AuthError::PasswordRejected) => {
                    // Refused before anything changed.
                    self.receipt.enrollment = Step::NotStarted;
                    self.save()?;
                    return Err(FinishError::Password);
                }
                Err(AuthError::Failed(m)) => {
                    self.receipt.enrollment = Step::Failed {
                        at: Timestamp::now(),
                        message: text(&m),
                    };
                    self.save()?;
                    return Err(FinishError::Failed(m));
                }
            }
            self.save()?;
        }

        progress(Phase::Installing);
        self.install.verify_files().map_err(FinishError::Failed)?;
        self.receipt.selection = Step::Started { at: Timestamp::now() };
        self.save()?;
        let boot = self.install.path(&c.boot_object).to_string_lossy().into_owned();
        let steps: [(Program, Vec<&str>); 2] = [
            (
                Program::Kmutil,
                vec![
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
            ),
            (Program::Bless, vec!["--mount", &volume, "--setBoot", "--user", &user]),
        ];
        for (n, (program, args)) in steps.iter().enumerate() {
            if n == 1 {
                progress(Phase::Selecting);
                if let Err(m) = self.system.disable_autolaunch(&self.install.volume) {
                    log(&format!("Recovery will still open this app at startup: {m}"));
                }
            }
            match self.system.authorized(*program, args, &user, password, log) {
                Ok(()) => {}
                Err(e) => {
                    let message = match &e {
                        AuthError::PasswordRejected => "That password isn't right.".to_string(),
                        AuthError::Failed(m) => m.clone(),
                    };
                    self.receipt.selection = Step::Failed {
                        at: Timestamp::now(),
                        message: text(&message),
                    };
                    self.save()?;
                    return Err(match e {
                        AuthError::PasswordRejected => FinishError::Password,
                        AuthError::Failed(m) => FinishError::Failed(m),
                    });
                }
            }
        }
        let expected = self.system.device(&self.install.volume).map_err(FinishError::Failed)?;
        let selected = self.system.startup_device().map_err(FinishError::Failed)?;
        if !selected.trim_start_matches("/dev/").starts_with(&expected) {
            let m = format!("macOS reported success, but the Mac will start from {selected}, not Sekka's volume.");
            self.receipt.selection = Step::Failed {
                at: Timestamp::now(),
                message: text(&m),
            };
            self.save()?;
            return Err(FinishError::Failed(m));
        }
        self.receipt.selection = Step::Completed { at: Timestamp::now() };
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        volume: (Uuid, Uuid),
        handoff: Vec<u8>,
        policy: RefCell<String>,
        calls: RefCell<Vec<Program>>,
        refuse: RefCell<Vec<(Program, AuthError)>>,
        startup: String,
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
        fn disable_autolaunch(&self, _: &Path) -> Result<(), String> {
            Ok(())
        }
        fn startup_device(&self) -> Result<String, String> {
            Ok(self.startup.clone())
        }
    }

    const PAIRED_1TR: &str = "Current OS environment: one true recoveryOS\nLocal policy for volume group: Paired\nSecurity Mode: Full Security\n";

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
        }
    }

    fn setup() -> (tempfile::TempDir, Install) {
        let dir = tempfile::tempdir().unwrap();
        let install = crate::install::tests::fixture(dir.path());
        (dir, install)
    }

    #[test]
    fn the_steps_run_once_each_and_are_recorded() {
        let (dir, install) = setup();
        let f = fake(&install);
        let mut finisher = Finisher::open(&f, install).unwrap();
        assert_eq!(finisher.check().unwrap(), Environment::Ready);
        let mut phases = vec![];
        finisher.finish("right", &mut |p| phases.push(p), &mut |_| {}).unwrap();
        assert_eq!(phases, [Phase::Allowing, Phase::Installing, Phase::Selecting]);
        assert_eq!(*f.calls.borrow(), [Program::Bputil, Program::Kmutil, Program::Bless]);
        let again = Store::at(dir.path()).load(finisher.install.config.installation).unwrap();
        assert!(matches!(again.selection, Step::Completed { .. }));
        assert!(finisher.done());
    }

    #[test]
    fn the_wrong_volume_or_installation_is_refused_before_any_password() {
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.volume.0 = Uuid::from_bytes([9; 16]);
        assert!(Finisher::open(&f, install).unwrap().check().is_err());
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.handoff = {
            let mut h = Handoff::parse(&f.handoff).unwrap();
            h.installation = Uuid::from_bytes([9; 16]);
            h.to_bytes().unwrap()
        };
        assert!(Finisher::open(&f, install).unwrap().check().is_err());
    }

    #[test]
    fn only_paired_one_true_recovery_will_do() {
        assert_eq!(environment("Local policy for volume group: Not paired"), Environment::WrongRecovery);
        assert_eq!(environment("Current OS environment: recoveryOS\nfoo: Paired"), Environment::NotOneTrue);
        let (_dir, install) = setup();
        let f = fake(&install);
        *f.policy.borrow_mut() = "Current OS environment: recoveryOS\nLocal policy: Paired".into();
        let mut finisher = Finisher::open(&f, install).unwrap();
        assert!(finisher.finish("right", &mut |_| {}, &mut |_| {}).is_err());
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn a_wrong_password_changes_nothing_and_can_be_retried() {
        let (_dir, install) = setup();
        let f = fake(&install);
        f.refuse.borrow_mut().push((Program::Bputil, AuthError::PasswordRejected));
        let mut finisher = Finisher::open(&f, install).unwrap();
        assert_eq!(finisher.finish("right", &mut |_| {}, &mut |_| {}), Err(FinishError::Password));
        assert_eq!(finisher.receipt.enrollment, Step::NotStarted);
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
    }

    #[test]
    fn a_retry_after_kmutil_fails_skips_what_is_done() {
        let (_dir, install) = setup();
        let f = fake(&install);
        f.refuse.borrow_mut().push((Program::Kmutil, AuthError::Failed("kmutil: busy".into())));
        let mut finisher = Finisher::open(&f, install).unwrap();
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
        let mut r = store.load(install.config.installation).unwrap();
        r.enrollment = Step::Started { at: Timestamp::from_unix(1) };
        store.save(&r).unwrap();
        let mut finisher = Finisher::open(&f, install).unwrap();
        finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap();
        assert_eq!(*f.calls.borrow(), [Program::Kmutil, Program::Bless]);
    }

    #[test]
    fn a_changed_boot_object_is_never_installed() {
        let (_dir, install) = setup();
        let f = fake(&install);
        let boot = install.path(&install.config.boot_object);
        let mut finisher = Finisher::open(&f, install).unwrap();
        std::fs::write(boot, b"something else").unwrap();
        assert!(finisher.finish("right", &mut |_| {}, &mut |_| {}).is_err());
        assert!(!f.calls.borrow().contains(&Program::Kmutil));
    }

    #[test]
    fn a_selection_that_didnt_take_is_reported() {
        let (_dir, install) = setup();
        let mut f = fake(&install);
        f.startup = "/dev/disk1s1".into();
        let mut finisher = Finisher::open(&f, install).unwrap();
        let e = finisher.finish("right", &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(matches!(e, FinishError::Failed(m) if m.contains("disk1s1")));
    }
}
