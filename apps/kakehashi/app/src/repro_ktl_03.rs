//! KTL-03: a helper killed by a signal mid-write was reported as "Not
//! written" without the "partly written" warning: `elevate::run` maps a
//! signal death to `Ran::Failed("the helper was killed")`, and the window
//! turned every `Ran::Failed` into `Failed { touched: false }`, whatever
//! stage the helper reported last. The job now ends from the helper's last
//! report (`kakehashi_core::job`).
//!
//! The unit test feeds the window the messages its worker sends. The
//! ignored test runs the real `start_write` worker and `elevate::run`, but
//! only inside `repro_ktl_03.sh`, where bwrap replaces
//! /run/wrappers/bin/pkexec with a fake that appends a Writing line to the
//! progress file and dies by SIGKILL. The real pkexec and the helper are
//! never run; no device is opened.

use super::*;
use std::path::Path;

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

fn stick(serial: &str) -> Drive {
    Drive {
        name: "sdkk".into(),
        bytes: 32_000_000_000,
        vendor: "Repro".into(),
        model: "Stick".into(),
        serial: serial.into(),
        bus: "USB".into(),
        usb_or_removable: true,
        ..Drive::default()
    }
}

/// Lists `drive` and chooses it, as the window's listing and a click do.
fn list_and_choose(k: &mut Kakehashi, drive: Drive) {
    let ticket = k.drives.request(Instant::now()).expect("a listing is due");
    k.apply(Msg::Drives(DriveListing::new(ticket, Ok(vec![drive.clone()]))));
    k.drives.choose(&drive);
}

fn failure(k: &Kakehashi) -> (String, bool) {
    match k.job.as_ref().map(|j| j.stored()) {
        Some(JobState::Ended(End::Failed(f))) => (f.message().to_owned(), f.touched()),
        other => panic!("the job did not end as Failed: {other:?}"),
    }
}

#[test]
fn repro_ktl_03_killed_mid_write_is_reported_as_partly_written() {
    let mut k = window();
    list_and_choose(&mut k, stick("4C530001230529115034"));
    let image = ImageId { path: "sekka.iso".into(), sha256: "0".repeat(64) };
    let (review, shown) =
        k.drives.open_review(None, image.clone()).map(|(r, t)| (r, t.clone())).expect("the stick is chosen");
    let plan =
        Plan::new(k.drives.confirm(review, None, Some(image)).unwrap(), Pressed::for_tests(review, shown)).unwrap();
    // As start_write leaves it, and the lines the helper wrote before it
    // was killed, as the worker reads them.
    let dir = JobDir::new().unwrap();
    let progress = dir.progress();
    fs::write(
        &progress,
        concat!(r#"{"stage":"preparing"}"#, "\n", r#"{"stage":"writing","done":1073741824,"total":4294967296}"#, "\n"),
    )
    .unwrap();
    k.job = Some(Job::start(&plan, &dir));
    k.apply(Msg::Job(Event::Authorizing));
    let mut reports = ProgressFile::new(&dir);
    for report in reports.read() {
        k.apply(Msg::Job(Event::Report(report)));
    }
    // What elevate::run returns when the helper dies by a signal.
    k.apply(Msg::Job(Event::Exited(reports.finish(Ran::Failed("the helper was killed".into())))));
    let (message, touched) = failure(&k);
    assert!(
        touched,
        "the helper was killed after reporting Writing 1 GiB of 4 GiB, but the window ends with \
         Failed {{ message: {message:?}, touched: false }}: \"Not written\" and no \"partly written\" warning"
    );
}

fn require_fake_pkexec() {
    // The real setuid wrapper is not readable, and has no marker.
    let script = fs::read_to_string("/run/wrappers/bin/pkexec").unwrap_or_default();
    assert!(
        script.contains(FAKE_PKEXEC),
        "refusing to start a write: /run/wrappers/bin/pkexec is not the reproduction's fake; run repro_ktl_03.sh"
    );
}

fn ready(k: &mut Kakehashi, drive: Drive, dir: &Path) {
    let path = dir.join("sekka.iso");
    fs::write(&path, vec![0u8; 4096]).unwrap();
    k.image = Some(Image {
        path,
        bytes: 4096,
        sha: Some("0".repeat(64)),
        holds: None,
        check: Check::Verified { source: "the reproduction".into() },
        cancel: Arc::new(AtomicBool::new(false)),
    });
    list_and_choose(k, drive);
}

/// Apply the worker's messages until the job ends; the helper stages seen.
fn run_to_end(k: &mut Kakehashi) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    loop {
        let msg = k.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("the job never ended");
        if let Msg::Job(event) = &msg {
            seen.push(match event {
                Event::Report(report) => format!("{:?}", report.progress()),
                other => format!("{other:?}"),
            });
        }
        k.apply(msg);
        if k.job.as_ref().is_some_and(Job::is_ended) {
            return seen;
        }
    }
}

#[test]
#[ignore = "needs the fake pkexec: run apps/kakehashi/app/repro_ktl_03.sh"]
fn repro_ktl_03_killed_helper_through_start_write_and_elevate() {
    require_fake_pkexec();
    let dir = tempfile::tempdir().unwrap();
    let mut k = window();
    ready(&mut k, stick("KTL03-KILLED"), dir.path());
    let image = k.image_id().expect("the image is checked");
    let (review, shown) = k.drives.open_review(None, image).map(|(r, t)| (r, t.clone())).expect("the stick is chosen");
    k.start_write(Pressed::for_tests(review, shown));
    assert!(k.job.is_some(), "start_write did not start a job: {:?}", k.error);
    let seen = run_to_end(&mut k);
    // The Writing line comes as a report, or with the exit.
    assert!(seen.iter().any(|s| s.contains("Writing")), "the fake helper's Writing line was not read: {seen:?}");
    let (message, touched) = failure(&k);
    assert!(
        touched,
        "the helper died by SIGKILL after reporting Writing (worker messages: {seen:?}), and the window ends \
         with Failed {{ message: {message:?}, touched: false }}: no \"partly written\" warning"
    );
}
