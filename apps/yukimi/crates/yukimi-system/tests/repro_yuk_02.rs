//! YUK-02: an `activating` receipt left by a Yukimi activation that failed
//! past its commit point must not outlive a later, healthy boot into a
//! system the receipt doesn't name (repaired with `nixos-rebuild switch`,
//! or the previous system booted again). Only Yukimi's own change and
//! rollback rewrite the receipt, so without that the advice stays
//! "Activation did not finish successfully" for good.
use std::path::{Path, PathBuf};

use yukimi_system::completion::{Action, Receipt, system_state};

/// Complete systems with the same boot artefacts, so only the receipt can
/// make the answer anything but "applied".
struct Systems {
    root: PathBuf,
}

impl Systems {
    fn new(tag: &str) -> Systems {
        let root = std::env::temp_dir().join(format!("repro-yuk-02-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("artefacts")).unwrap();
        for part in ["kernel", "initrd", "kernel-modules"] {
            std::fs::write(root.join("artefacts").join(part), part).unwrap();
        }
        for name in ["S0", "S1", "S2"] {
            let system = root.join(name);
            std::fs::create_dir_all(system.join("etc/yukimi")).unwrap();
            for part in ["kernel", "initrd", "kernel-modules"] {
                std::os::unix::fs::symlink(root.join("artefacts").join(part), system.join(part)).unwrap();
            }
            std::fs::write(system.join("kernel-params"), "quiet").unwrap();
            std::fs::write(system.join("init-interface-version"), "2").unwrap();
            std::fs::write(system.join("etc/yukimi/activation.json"), "{\"version\":1}").unwrap();
        }
        Systems { root }
    }
    fn get(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Systems {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What the helper leaves when `switch-to-configuration` (or `nix-env
/// --set`) fails after commit: `activating` for the system it built, in
/// the boot it ran in, as `completion::read` finds it on disk.
fn failed_activation(system: &Path) -> Receipt {
    let record = serde_json::json!({"system": system, "boot_id": "boot-1", "state": "activating", "reasons": []});
    serde_json::from_value(record).unwrap()
}

#[test]
fn repro_yuk_02_rebuilt_and_rebooted_into_another_system() {
    let s = Systems::new("rebuilt");
    let (s1, s2) = (s.get("S1"), s.get("S2"));
    let receipt = failed_activation(&s1);
    // `nixos-rebuild switch` to S2, then a reboot into it: booted, active
    // and selected are all S2, in a later boot than the receipt's.
    let after = system_state(&s2, &s2, &s2, receipt.bearing("boot-2"));
    assert_ne!(
        after.action,
        Action::Unknown,
        "a later healthy boot into S2 still reports the old failed activation of S1: {after:?}"
    );
}

#[test]
fn repro_yuk_02_rebooted_into_the_previous_system() {
    // Extension of the entry: `nix-env --set` failed (the profile still
    // selects S0) and the person simply restarts into S0.
    let s = Systems::new("previous");
    let (s0, s1) = (s.get("S0"), s.get("S1"));
    let receipt = failed_activation(&s1);
    let after = system_state(&s0, &s0, &s0, receipt.bearing("boot-2"));
    assert_ne!(
        after.action,
        Action::Unknown,
        "a later healthy boot into S0 still reports the old failed activation of S1: {after:?}"
    );
}

#[test]
fn repro_yuk_02_the_failed_boot_itself_still_reports_it() {
    // Until a restart, the failed activation is still the last word, even
    // after a repair in the same boot: nothing shows the repair finished.
    let s = Systems::new("same-boot");
    let (s1, s2) = (s.get("S1"), s.get("S2"));
    let receipt = failed_activation(&s1);
    for (booted, active) in [(&s1, &s1), (&s2, &s2)] {
        let now = system_state(booted, active, active, receipt.bearing("boot-1"));
        assert_eq!(now.action, Action::Unknown, "{now:?}");
    }
}
