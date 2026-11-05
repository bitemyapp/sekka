//! KTL-04: "Erase …?" named the drive chosen when the dialog opened, but OK
//! called `start_write`, which looked the chosen drive up again by name in
//! a list that keeps refreshing under the modal. A stick swapped for
//! another that got the same name was the one written, and the plan named
//! the new drive, so the helper's `still_there` passed for it. OK now hands
//! `start_write` the drive the dialog described, which must be found again
//! in the list (`kakehashi_core::choice`).
//!
//! The test runs the real `start_write` worker and `elevate::run` only
//! inside `repro_ktl_04.sh`, where bwrap replaces /run/wrappers/bin/pkexec
//! with a fake that copies the plan it is given and exits. The real pkexec
//! and the helper are never run; no device is opened.

use super::*;

const FAKE_PKEXEC: &str = "KAKEHASHI-REPRO-FAKE-PKEXEC";

fn window() -> Kakehashi {
    let (tx, rx) = mpsc::channel();
    Kakehashi {
        tx,
        rx,
        builds: Loading::Pending,
        image: None,
        download: None,
        drives: DriveList::new(),
        job: None,
        error: None,
        downloads: std::env::temp_dir(),
        _poll: Task::ready(()),
        _appearance: gpui_kit::Subscription::new(|| {}),
    }
}

fn require_fake_pkexec() {
    // The real setuid wrapper is not readable, and has no marker.
    let script = fs::read_to_string("/run/wrappers/bin/pkexec").unwrap_or_default();
    assert!(
        script.contains(FAKE_PKEXEC),
        "refusing to start a write: /run/wrappers/bin/pkexec is not the reproduction's fake; run repro_ktl_04.sh"
    );
}

fn run_to_end(k: &mut Kakehashi) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let msg = k.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("the job never ended");
        k.apply(msg);
        if k.job.as_ref().is_some_and(|j| j.ended) {
            return;
        }
    }
}

/// A listing of `drives`, as the window's listing thread sends it.
fn listed(k: &mut Kakehashi, at: Instant, drives: Vec<Drive>) {
    let ticket = k.drives.request(at).expect("a listing is due");
    k.apply(Msg::Drives(ticket, Ok(drives)));
}

#[test]
#[ignore = "needs the fake pkexec: run apps/kakehashi/app/repro_ktl_04.sh"]
fn repro_ktl_04_ok_writes_the_drive_the_dialog_named() {
    require_fake_pkexec();
    let out = PathBuf::from(std::env::var("KK_REPRO_OUT").expect("KK_REPRO_OUT, set by repro_ktl_04.sh"));
    let dir = tempfile::tempdir().unwrap();
    let mut k = window();

    let first = Drive {
        name: "sdb".into(),
        bytes: 32_015_679_488,
        vendor: "SanDisk".into(),
        model: "Ultra".into(),
        serial: "4C530001230529115034".into(),
        bus: "USB".into(),
        usb_or_removable: true,
        ..Drive::default()
    };
    let image = dir.path().join("sekka.iso");
    fs::write(&image, vec![0u8; 4096]).unwrap();
    k.image = Some(Image {
        path: image,
        bytes: 4096,
        sha: Some("0".repeat(64)),
        holds: None,
        check: Check::Verified { source: "the reproduction".into() },
        cancel: Arc::new(AtomicBool::new(false)),
    });
    let t0 = Instant::now();
    listed(&mut k, t0, vec![first.clone()]);
    k.drives.choose(&first);

    // The dialog opens: confirm describes the reviewed drive.
    let reviewed = k.drives.review(None).expect("the stick is offered");
    let named = reviewed.drive().clone();

    // Under the modal the 2 s refresh goes on: the stick was swapped for
    // another, which the kernel named sdb as well.
    let second = Drive {
        name: "sdb".into(),
        bytes: 64_023_052_288,
        vendor: "Kingston".into(),
        model: "DataTraveler".into(),
        serial: "E0D55EA574F4F7A0A9580D51".into(),
        bus: "USB".into(),
        usb_or_removable: true,
        ..Drive::default()
    };
    listed(&mut k, t0 + DriveList::REFRESH, vec![second.clone()]);
    let still_chosen = k.drives.chosen(None).map(|d| d.name.clone());

    // OK in the dialog.
    k.start_write(reviewed);
    let started = k.job.as_ref().map(|j| j.drive.clone());
    if started.is_some() {
        run_to_end(&mut k);
    }
    let plan: Option<Plan> =
        fs::read_to_string(out.join("plan.json")).ok().map(|text| serde_json::from_str(&text).unwrap());
    let erased = plan.as_ref().map(|p| p.drive().clone()).or(started.clone());

    assert!(
        erased.as_ref().is_none_or(|d| d.same(&named)),
        "the dialog asked \"Erase {}?\" ({}, {}); after the refresh chosen = {still_chosen:?}, and OK started a \
         write of {:?} ({:?}), and the helper's plan names {:?} ({:?}), which passes still_there for the new stick: {}",
        named.label(),
        named.name,
        named.serial_hint(),
        started.as_ref().map(Drive::label),
        started.as_ref().map(Drive::serial_hint),
        plan.as_ref().map(|p| p.drive().label()),
        plan.as_ref().map(|p| p.drive().serial_hint()),
        plan.as_ref().is_some_and(|p| p.drive().same(&second)),
    );
    // Nothing was started, and the window says why.
    assert!(started.is_none() && plan.is_none(), "{started:?} {plan:?}");
    let error = k.error.clone().unwrap_or_default();
    assert!(error.contains("SanDisk Ultra") && error.contains("nothing was written"), "{error}");
}
