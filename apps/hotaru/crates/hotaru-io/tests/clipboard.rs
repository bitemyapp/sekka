use hotaru_io::{ClipboardReader, ClipboardWriter, TransferState};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [-1; 2];
    // SAFETY: pipe2 initializes both descriptors on success.
    assert_eq!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) },
        0
    );
    // SAFETY: these two fresh descriptors each get exactly one owner.
    let pair = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    // Deliberately make the pipe smaller than the clipboard data. A successful
    // test must handle backpressure; it cannot accidentally fit in one write.
    assert!(unsafe { libc::fcntl(pair.1.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) } >= 4096);
    pair
}

#[test]
fn multi_mebibyte_selection_survives_slow_peer_and_partial_io() {
    for length in [512 * 1024, 1024 * 1024 + 17, 4 * 1024 * 1024 + 123] {
        let bytes: Arc<[u8]> = (0..length)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>()
            .into();
        let (read, write) = pipe();
        let mut reader = ClipboardReader::new(read, length, Duration::from_secs(10)).unwrap();
        let mut writer =
            ClipboardWriter::new(write, bytes.clone(), Duration::from_secs(10)).unwrap();
        let mut observed_backpressure = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while reader.state() == TransferState::Pending {
            assert!(Instant::now() < deadline, "transfer stalled");
            let previous = writer.bytes_written();
            let result = writer.pump(64 * 1024).unwrap();
            if result == TransferState::Pending && previous == writer.bytes_written() {
                observed_backpressure = true;
            }
            // This unequal, tiny read budget forces short writes and preserves
            // the invariant that compositor dispatch could run between pumps.
            reader.pump(997).unwrap();
        }
        assert!(observed_backpressure);
        assert_eq!(writer.state(), TransferState::Complete);
        assert_eq!(reader.state(), TransferState::Complete);
        assert_eq!(reader.bytes(), &*bytes);
        assert!(writer.fd().is_none());
        assert!(reader.fd().is_none());
        assert_eq!(
            Arc::strong_count(&bytes),
            1,
            "finished writer retained selection"
        );
    }
}

#[test]
fn stalled_receiver_returns_without_waiting_and_can_cancel() {
    let (_read, write) = pipe();
    let bytes: Arc<[u8]> = vec![b'x'; 2 * 1024 * 1024].into();
    let mut writer = ClipboardWriter::new(write, bytes.clone(), Duration::from_secs(5)).unwrap();
    assert_eq!(writer.pump(usize::MAX).unwrap(), TransferState::Pending);
    let sent = writer.bytes_written();
    assert!(sent > 0 && sent < bytes.len());
    let before = Instant::now();
    for _ in 0..100 {
        assert_eq!(writer.pump(usize::MAX).unwrap(), TransferState::Pending);
    }
    assert!(
        before.elapsed() < Duration::from_millis(500),
        "nonblocking writes waited for the receiver"
    );
    assert_eq!(writer.bytes_written(), sent);
    writer.cancellation().cancel();
    assert_eq!(writer.pump(1).unwrap(), TransferState::Cancelled);
    assert!(writer.fd().is_none());
    assert_eq!(Arc::strong_count(&bytes), 1);
}

#[test]
fn broken_receiver_is_a_terminal_state_without_sigpipe() {
    let (read, write) = pipe();
    drop(read);
    let mut writer =
        ClipboardWriter::new(write, Arc::from(&b"clipboard"[..]), Duration::from_secs(1)).unwrap();
    assert_eq!(writer.pump(4096).unwrap(), TransferState::ReceiverClosed);
    assert_eq!(writer.pump(4096).unwrap(), TransferState::ReceiverClosed);
    assert!(writer.fd().is_none());
}

#[test]
fn broken_receiver_survives_default_sigpipe_disposition() {
    const CHILD: &str = "HOTARU_SIGPIPE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "broken_receiver_survives_default_sigpipe_disposition",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "SIGPIPE child failed: {:?} {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    // SAFETY: this isolated subprocess runs only this one test. The default
    // disposition makes an unguarded pipe write terminate the subprocess.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    broken_receiver_is_a_terminal_state_without_sigpipe();
}

#[test]
fn writer_preserves_sigpipe_already_pending_for_caller() {
    // All mask/pending operations below are confined to this test thread;
    // pthread_kill targets it, so concurrent tests cannot consume its signal.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut previous: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGPIPE);
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut previous),
            0
        );
        assert_eq!(libc::pthread_kill(libc::pthread_self(), libc::SIGPIPE), 0);
        let (read, write) = pipe();
        drop(read);
        let mut writer =
            ClipboardWriter::new(write, Arc::from(&b"x"[..]), Duration::from_secs(1)).unwrap();
        let outcome = writer.pump(1);
        let mut pending: libc::sigset_t = std::mem::zeroed();
        let checked = libc::sigpending(&mut pending);
        let still_pending = libc::sigismember(&pending, libc::SIGPIPE);
        // Drain before restoring the caller's mask, even when the assertion
        // below fails, so the test never changes another test's disposition.
        let zero = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        libc::sigtimedwait(&set, std::ptr::null_mut(), &zero);
        libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
        assert_eq!(checked, 0);
        assert_eq!(still_pending, 1);
        assert_eq!(outcome.unwrap(), TransferState::ReceiverClosed);
    }
}

#[test]
fn writer_closes_on_timeout_without_a_writable_event() {
    let (_read, write) = pipe();
    let mut writer =
        ClipboardWriter::new(write, Arc::from(&b"clipboard"[..]), Duration::ZERO).unwrap();
    assert_eq!(writer.pump(4096).unwrap(), TransferState::TimedOut);
    assert!(writer.fd().is_none());
}

#[test]
fn empty_selection_signals_eof() {
    let (read, write) = pipe();
    let mut writer =
        ClipboardWriter::new(write, Arc::from(&b""[..]), Duration::from_secs(1)).unwrap();
    let mut reader = ClipboardReader::new(read, 0, Duration::from_secs(1)).unwrap();
    assert_eq!(writer.pump(0).unwrap(), TransferState::Complete);
    assert_eq!(reader.pump(1).unwrap(), TransferState::Complete);
    assert!(reader.bytes().is_empty());
}

#[test]
fn receiver_closes_at_limit_without_unbounded_allocation() {
    let (read, write) = pipe();
    let mut writer =
        ClipboardWriter::new(write, vec![1_u8; 4096].into(), Duration::from_secs(1)).unwrap();
    let mut reader = ClipboardReader::new(read, 64, Duration::from_secs(1)).unwrap();
    writer.pump(4096).unwrap();
    assert_eq!(reader.pump(4096).unwrap(), TransferState::LimitExceeded);
    assert!(reader.bytes().len() <= 64);
    assert!(reader.fd().is_none());
}

#[test]
fn exactly_full_receiver_still_requires_eof() {
    let (read, write) = pipe();
    // SAFETY: write is live and the source slice contains the requested byte.
    assert_eq!(
        unsafe { libc::write(write.as_raw_fd(), b"a".as_ptr().cast(), 1) },
        1
    );
    let mut reader = ClipboardReader::new(read, 1, Duration::from_secs(1)).unwrap();
    assert_eq!(reader.pump(4096).unwrap(), TransferState::Pending);
    assert_eq!(reader.bytes(), b"a");
    drop(write);
    // Other tests may fork concurrently. CLOEXEC closes an inherited write end
    // at exec, not at fork, so EOF can legitimately arrive a little later.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let state = reader.pump(4096).unwrap();
        if state != TransferState::Pending {
            assert_eq!(state, TransferState::Complete);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "writer remained open");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn stalled_offer_returns_pending_and_supports_cancellation_and_timeout() {
    let (read, _write) = pipe();
    let mut reader = ClipboardReader::new(read, 4096, Duration::from_secs(5)).unwrap();
    assert_eq!(reader.pump(4096).unwrap(), TransferState::Pending);
    reader.cancellation().cancel();
    assert_eq!(reader.pump(4096).unwrap(), TransferState::Cancelled);
    assert!(reader.fd().is_none());

    let (read, _write) = pipe();
    let mut reader = ClipboardReader::new(read, 4096, Duration::ZERO).unwrap();
    assert_eq!(reader.pump(4096).unwrap(), TransferState::TimedOut);
    assert!(reader.fd().is_none());
}

#[test]
fn pump_obeys_byte_budget() {
    let (read, write) = pipe();
    let mut writer =
        ClipboardWriter::new(write, vec![7_u8; 4096].into(), Duration::from_secs(1)).unwrap();
    let mut reader = ClipboardReader::new(read, 4096, Duration::from_secs(1)).unwrap();
    assert_eq!(writer.pump(0).unwrap(), TransferState::Pending);
    assert_eq!(writer.bytes_written(), 0);
    assert_eq!(writer.pump(17).unwrap(), TransferState::Pending);
    assert_eq!(writer.bytes_written(), 17);
    assert_eq!(reader.pump(5).unwrap(), TransferState::Pending);
    assert_eq!(reader.bytes().len(), 5);
}

#[test]
fn reader_reports_wrong_descriptor_failure_and_closes_it() {
    let (_read, write) = pipe();
    let mut reader = ClipboardReader::new(write, 4096, Duration::from_secs(1)).unwrap();
    let error = reader.pump(4096).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    assert_eq!(reader.state(), TransferState::Failed);
    assert!(reader.fd().is_none());
    assert_ne!(error.kind(), io::ErrorKind::WouldBlock);
}
