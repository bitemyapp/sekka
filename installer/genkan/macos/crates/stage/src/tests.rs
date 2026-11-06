// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use std::cell::RefCell;
use std::os::unix::fs::symlink;

/// Request number, start, end → a scripted answer, or none for the honest
/// one.
type Script = Box<dyn FnMut(usize, u64, u64) -> Option<Result<RangeResponse, TransportError>>>;

/// A server in memory whose behavior is scripted per request.
struct Fake {
    data: Vec<u8>,
    requests: RefCell<Vec<u64>>,
    script: RefCell<Script>,
}
impl Fake {
    fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            requests: RefCell::new(Vec::new()),
            script: RefCell::new(Box::new(|_, _, _| None)),
        }
    }
    fn scripted(
        data: Vec<u8>,
        script: impl FnMut(usize, u64, u64) -> Option<Result<RangeResponse, TransportError>> + 'static,
    ) -> Self {
        let f = Self::new(data);
        *f.script.borrow_mut() = Box::new(script);
        f
    }
    fn honest(&self, start: u64, end: u64) -> RangeResponse {
        RangeResponse {
            status: 206,
            content_range: Some(format!("bytes {start}-{end}/{}", self.data.len())),
            body: self.data[start as usize..=end as usize].to_vec(),
        }
    }
}
impl Transport for &Fake {
    fn get_range(&self, _url: &HttpsUrl, start: u64, end: u64) -> Result<RangeResponse, TransportError> {
        let n = self.requests.borrow().len();
        self.requests.borrow_mut().push(start);
        if let Some(r) = (self.script.borrow_mut())(n, start, end) {
            return r;
        }
        Ok(self.honest(start, end))
    }
}

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}
fn expect(d: &[u8]) -> Expect {
    Expect {
        size: d.len() as u64,
        digest: Digest::of(d),
    }
}
fn url() -> HttpsUrl {
    "https://downloads.sekka.computer/mac/v1/payload.zip".parse().unwrap()
}
fn staging() -> (tempfile::TempDir, StagingDir) {
    let dir = tempfile::tempdir().unwrap();
    let s = StagingDir::open(&dir.path().join("Staging")).unwrap();
    (dir, s)
}
fn stager<'a>(dir: &'a StagingDir, fake: &'a Fake, cancel: &'a AtomicBool) -> Stager<'a, &'a Fake> {
    let mut s = Stager::new(dir, fake, cancel);
    s.sleep = |_| {};
    s.space = |_| Ok(u64::MAX);
    s
}

const SIZE: usize = 40 * 1024 * 1024 + 123;

#[test]
fn a_download_in_parts_is_verified_and_reused() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let fake = Fake::new(d.clone());
    let cancel = AtomicBool::new(false);
    let mut events = Vec::new();
    let file = stager(&dir, &fake, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |e| events.push(e))
        .unwrap();
    assert_eq!(file.digest, Digest::of(&d));
    assert_eq!(fs::read(&file.path).unwrap(), d);
    assert_eq!(*fake.requests.borrow(), vec![0, PART, 2 * PART]);
    assert!(events.contains(&StageEvent::Verifying));
    assert_eq!(
        events.iter().filter(|e| matches!(e, StageEvent::Progress { .. })).count(),
        4
    );
    let mut events = Vec::new();
    stager(&dir, &fake, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |e| events.push(e))
        .unwrap();
    assert_eq!(events, vec![StageEvent::Reused]);
    assert_eq!(fake.requests.borrow().len(), 3);
}

#[test]
fn a_dropped_network_resumes_at_the_last_whole_part() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let cancel = AtomicBool::new(false);
    let dead = Fake::scripted(d.clone(), |n, _, _| {
        (n >= 1).then(|| Err(TransportError::Retryable("the network connection was lost".into())))
    });
    let err = stager(&dir, &dead, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, StageError::Network { attempts: 6, .. }));
    let partial = dir.path().join("payload.zip.partial");
    assert_eq!(fs::metadata(&partial).unwrap().len(), PART);
    // A torn tail beyond the last part is discarded on resume.
    let mut f = OpenOptions::new().append(true).open(&partial).unwrap();
    f.write_all(&[0xff; 4096]).unwrap();
    let alive = Fake::new(d.clone());
    stager(&dir, &alive, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |_| {})
        .unwrap();
    assert_eq!(*alive.requests.borrow(), vec![PART, 2 * PART]);
}

#[test]
fn transient_failures_are_retried_with_backoff() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let cancel = AtomicBool::new(false);
    let fake = Fake::scripted(d.clone(), |n, start, end| match n {
        0 => Some(Ok(RangeResponse {
            status: 503,
            content_range: None,
            body: vec![],
        })),
        1 => Some(Ok(RangeResponse {
            status: 206,
            content_range: Some(format!("bytes {start}-{end}/{SIZE}")),
            body: vec![0; 10],
        })),
        _ => None,
    });
    let mut waits = Vec::new();
    stager(&dir, &fake, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |e| {
            if let StageEvent::Retrying { wait, .. } = e {
                waits.push(wait)
            }
        })
        .unwrap();
    assert_eq!(waits, vec![Duration::from_secs(2), Duration::from_secs(4)]);
}

#[test]
fn wrong_ranges_and_ignored_ranges_are_refused() {
    let d = data(SIZE);
    let cancel = AtomicBool::new(false);
    let (_t, dir) = staging();
    let ignores = Fake::scripted(d.clone(), |_, _, _| {
        Some(Ok(RangeResponse {
            status: 200,
            content_range: None,
            body: vec![],
        }))
    });
    assert!(matches!(
        stager(&dir, &ignores, &cancel).stage("a.zip", &url(), &expect(&d), &mut |_| {}),
        Err(StageError::BadResponse(_))
    ));
    let shifted = Fake::scripted(d.clone(), move |_, start, end| {
        Some(Ok(RangeResponse {
            status: 206,
            content_range: Some(format!("bytes {}-{}/{SIZE}", start + 1, end + 1)),
            body: vec![0; (end - start + 1) as usize],
        }))
    });
    assert!(matches!(
        stager(&dir, &shifted, &cancel).stage("b.zip", &url(), &expect(&d), &mut |_| {}),
        Err(StageError::BadResponse(_))
    ));
    let missing = Fake::scripted(d.clone(), |_, _, _| {
        Some(Ok(RangeResponse {
            status: 404,
            content_range: None,
            body: vec![],
        }))
    });
    assert!(matches!(
        stager(&dir, &missing, &cancel).stage("c.zip", &url(), &expect(&d), &mut |_| {}),
        Err(StageError::Fatal(_))
    ));
    assert_eq!(missing.requests.borrow().len(), 1, "a 404 isn't retried");
}

#[test]
fn a_substituted_file_is_refused_after_one_fresh_try() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let mut wrong = d.clone();
    wrong[SIZE - 1] ^= 1;
    let cancel = AtomicBool::new(false);
    let fake = Fake::new(wrong);
    let err = stager(&dir, &fake, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |_| {})
        .unwrap_err();
    assert!(matches!(err, StageError::Corrupt { .. }));
    assert_eq!(fake.requests.borrow().len(), 6, "downloaded twice, then stopped");
    assert!(!dir.path().join("payload.zip").exists());
    assert!(!dir.path().join("payload.zip.partial").exists());

    // Corrupt once, right the second time: succeeds.
    let flip = std::cell::Cell::new(true);
    let good = d.clone();
    let once = Fake::scripted(d.clone(), move |_, start, end| {
        let mut body = good[start as usize..=end as usize].to_vec();
        if end as usize == SIZE - 1 && flip.replace(false) {
            body[0] ^= 1;
        }
        Some(Ok(RangeResponse {
            status: 206,
            content_range: Some(format!("bytes {start}-{end}/{SIZE}")),
            body,
        }))
    });
    stager(&dir, &once, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |_| {})
        .unwrap();
}

#[test]
fn a_stale_finished_file_is_replaced() {
    let (_t, dir) = staging();
    let d = data(1000);
    fs::write(dir.path().join("manifest.json"), b"stale").unwrap();
    let fake = Fake::new(d.clone());
    let cancel = AtomicBool::new(false);
    stager(&dir, &fake, &cancel)
        .stage("manifest.json", &url(), &expect(&d), &mut |_| {})
        .unwrap();
    assert_eq!(fs::read(dir.path().join("manifest.json")).unwrap(), d);
}

#[test]
fn cancelling_keeps_what_was_downloaded() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let cancel = AtomicBool::new(false);
    let fake = Fake::new(d.clone());
    let err = stager(&dir, &fake, &cancel)
        .stage("payload.zip", &url(), &expect(&d), &mut |e| {
            if matches!(e, StageEvent::Progress { done, .. } if done == PART) {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
    assert_eq!(err, StageError::Cancelled);
    assert_eq!(fs::metadata(dir.path().join("payload.zip.partial")).unwrap().len(), PART);
}

#[test]
fn not_enough_space_is_said_before_downloading() {
    let (_t, dir) = staging();
    let d = data(SIZE);
    let cancel = AtomicBool::new(false);
    let fake = Fake::new(d.clone());
    let mut s = stager(&dir, &fake, &cancel);
    s.space = |_| Ok(1024);
    assert!(matches!(
        s.stage("payload.zip", &url(), &expect(&d), &mut |_| {}),
        Err(StageError::NoSpace { available: 1024, .. })
    ));
    assert!(fake.requests.borrow().is_empty());
    let e = expect(&d);
    assert_eq!(dir.bytes_needed(&[("payload.zip", &e)]), SIZE as u64);
}

#[test]
fn staging_must_be_private() {
    let t = tempfile::tempdir().unwrap();
    let open = t.path().join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(StagingDir::open(&open), Err(StageError::UnsafeStaging(_))));
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let link = t.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(matches!(StagingDir::open(&link), Err(StageError::UnsafeStaging(_))));
    let (_t, dir) = staging();
    assert!(dir.file("../escape").is_err());
    assert!(dir.file(".hidden").is_err());
}

fn import_setup() -> (tempfile::TempDir, PathBuf, StagingDir, Vec<u8>) {
    let t = tempfile::tempdir().unwrap();
    let user_root = t.path().join("user");
    fs::create_dir(&user_root).unwrap();
    fs::set_permissions(&user_root, fs::Permissions::from_mode(0o700)).unwrap();
    let d = data(3 * 1024 * 1024);
    fs::write(user_root.join("payload.zip"), &d).unwrap();
    let dest = StagingDir::open(&t.path().join("helper")).unwrap();
    (t, user_root, dest, d)
}

#[test]
fn the_helper_imports_only_the_catalogs_bytes() {
    let (_t, root, dest, d) = import_setup();
    let me = effective_uid();
    let cancel = AtomicBool::new(false);
    let file = import(&root.join("payload.zip"), &root, me, &dest, "payload.zip", &expect(&d), &cancel).unwrap();
    assert_eq!(fs::read(file.path).unwrap(), d);
    let mut other = d.clone();
    other[0] ^= 1;
    assert!(matches!(
        import(&root.join("payload.zip"), &root, me, &dest, "x.zip", &expect(&other), &cancel),
        Err(StageError::Corrupt { .. })
    ));
    assert!(!dest.path().join(".import-x.zip").exists());
    assert!(matches!(
        import(&root.join("payload.zip"), &root, me + 1, &dest, "y.zip", &expect(&d), &cancel),
        Err(StageError::UnsafeStaging(_))
    ));
}

#[test]
fn links_traversal_and_other_places_are_refused() {
    let (t, root, dest, d) = import_setup();
    let me = effective_uid();
    let cancel = AtomicBool::new(false);
    let e = expect(&d);
    symlink(root.join("payload.zip"), root.join("link.zip")).unwrap();
    assert!(import(&root.join("link.zip"), &root, me, &dest, "a", &e, &cancel).is_err());
    fs::hard_link(root.join("payload.zip"), root.join("hard.zip")).unwrap();
    assert!(import(&root.join("hard.zip"), &root, me, &dest, "b", &e, &cancel).is_err());
    fs::remove_file(root.join("hard.zip")).unwrap();
    assert!(import(&root.join("../user/payload.zip"), &root, me, &dest, "c", &e, &cancel).is_err());
    let outside = t.path().join("outside.zip");
    fs::write(&outside, &d).unwrap();
    assert!(import(&outside, &root, me, &dest, "d", &e, &cancel).is_err());
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o777)).unwrap();
    fs::write(sub.join("payload.zip"), &d).unwrap();
    assert!(import(&sub.join("payload.zip"), &root, me, &dest, "e", &e, &cancel).is_err());
    let mut short = e.clone();
    short.size -= 1;
    assert!(import(&root.join("payload.zip"), &root, me, &dest, "f", &short, &cancel).is_err());
    assert!(import(&root.join("payload.zip"), &root, me, &dest, "g", &e, &cancel).is_ok());
}
