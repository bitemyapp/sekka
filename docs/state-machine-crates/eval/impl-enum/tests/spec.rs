//! Conformance of the reference implementation. (Its differential tests
//! would compare it with itself, so there are none.)

use impl_enum as m;
use spec::testing::conformance;
use spec::{install, lock, pointer, polkit, transfer, wrap};

#[test]
fn pointer_conformance() {
    conformance::<pointer::traces::Spec, m::pointer::Pointer>(&[]);
}
#[test]
fn wrap_conformance() {
    conformance::<wrap::traces::Spec, m::wrap::Wrap>(&[]);
}
#[test]
fn polkit_conformance() {
    conformance::<polkit::traces::Spec, m::polkit::Polkit>(&[]);
}
#[test]
fn transfer_conformance() {
    conformance::<transfer::traces::Spec, m::transfer::Transfer>(&[]);
}
#[test]
fn lock_conformance() {
    conformance::<lock::traces::Spec, m::lock::Lock>(&[]);
}
#[test]
fn install_conformance() {
    conformance::<install::traces::Spec, m::install::Install>(&[]);
}

/// The random environments reach every state of every target.
#[test]
fn environments_reach_every_state() {
    use spec::testing::coverage;
    fn check(name: &str, seen: std::collections::BTreeMap<String, u64>, all: &[&str]) {
        eprintln!("{name}: {seen:?}");
        let missing: Vec<_> = all.iter().filter(|s| !seen.contains_key(**s)).collect();
        assert!(missing.is_empty(), "{name}: never reached {missing:?}");
    }
    check(
        "pointer",
        coverage::<pointer::traces::Spec, m::pointer::Pointer>(300),
        &["Outside", "Hovering", "Pressed", "DragsOnlyPressed", "DragsOnlyDragging", "NativeDrag"],
    );
    check(
        "wrap",
        coverage::<wrap::traces::Spec, m::wrap::Wrap>(300),
        &["Normal", "PendingWrap", "NormalNoAutowrap", "PendingNoAutowrap"],
    );
    check(
        "polkit",
        coverage::<polkit::traces::Spec, m::polkit::Polkit>(300),
        &["Idle", "Starting", "Prompted", "Checking", "RetryWait", "Unavailable"],
    );
    check(
        "transfer",
        coverage::<transfer::traces::Spec, m::transfer::Transfer>(300),
        &[
            "Idle",
            "Probing",
            "Asking",
            "Placing",
            "SettingAside",
            "PlacingReplacement",
            "Disposing",
            "Restoring",
            "Listing",
            "Finished",
        ],
    );
    let mut lock_states = Vec::new();
    for l in ["Unlocked", "Locking", "Locked", "Checking"] {
        for d in ["NotHeld", "Held", "SleepPending"] {
            if d != "SleepPending" || l == "Locking" {
                lock_states.push(format!("StateName {{ lock: {l}, delay: {d} }}"));
            }
        }
    }
    let lock_refs: Vec<&str> = lock_states.iter().map(String::as_str).collect();
    check("lock", coverage::<lock::traces::Spec, m::lock::Lock>(300), &lock_refs);
    check(
        "install",
        coverage::<install::traces::Spec, m::install::Install>(300),
        &[
            "Idle",
            "Checks",
            "Configuration",
            "Evaluation",
            "Build",
            "Cache",
            "Prepared",
            "Cancelling",
            "Erase",
            "Format",
            "Files",
            "Copy",
            "BuildOnTarget",
            "Bootloader",
            "Record",
            "Flush",
            "Complete",
            "Failed",
            "Refused",
            "Cancelled",
        ],
    );
}

/// The random environments make the reference emit every effect variant.
#[test]
fn environments_produce_every_effect() {
    use spec::testing::coverage;
    fn check(name: &str, seen: std::collections::BTreeMap<String, u64>, all: &[&str]) {
        let missing: Vec<_> = all.iter().filter(|e| !seen.contains_key(&format!("effect {e}"))).collect();
        assert!(missing.is_empty(), "{name}: never emitted {missing:?}");
    }
    check(
        "pointer",
        coverage::<pointer::traces::Spec, m::pointer::Pointer>(300),
        &["HoverOn", "HoverOff", "UnderOn", "UnderOff", "Move", "Down", "Up"],
    );
    check(
        "wrap",
        coverage::<wrap::traces::Spec, m::wrap::Wrap>(300),
        &["Put", "SpacerHead", "SoftWrap", "ScrollUp", "ScrollDown", "Unlink"],
    );
    check(
        "polkit",
        coverage::<polkit::traces::Spec, m::polkit::Polkit>(300),
        &[
            "OpenDialog",
            "CloseDialog",
            "StartHelper",
            "EndHelper",
            "WriteAnswer",
            "ShowPrompt",
            "ShowNote",
            "ArmRetry",
            "CancelRetry",
            "Reply",
        ],
    );
    check(
        "transfer",
        coverage::<transfer::traces::Spec, m::transfer::Transfer>(300),
        &[
            "Probe",
            "Ask",
            "PlaceAt",
            "SetAside",
            "Trash",
            "Delete",
            "RestoreAside",
            "List",
            "RemoveEmptySource",
            "Problem",
            "Outcome",
            "Finished",
        ],
    );
    check(
        "lock",
        coverage::<lock::traces::Spec, m::lock::Lock>(300),
        &[
            "RequestLock",
            "ReleaseLock",
            "OpenSurface",
            "CloseSurface",
            "LockedHint",
            "StartCheck",
            "ShowNote",
            "ShakeStep",
            "ArmShake",
            "CancelShake",
            "TakeDelay",
            "ReleaseDelay",
        ],
    );
    check(
        "install",
        coverage::<install::traces::Spec, m::install::Install>(300),
        &[
            "Run",
            "Prepared",
            "Progress",
            "StayAwake",
            "AllowSleep",
            "Interrupt",
            "Refused",
            "Warn",
            "Failed",
            "Cancelled",
            "Complete",
        ],
    );
}
