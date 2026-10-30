use hotaru_io::{Pty, PtyCommand, PtySize};
use std::io;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

fn command(script: &str) -> PtyCommand {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), script.into()];
    command
}

fn collect(pty: &Pty) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut result = Vec::new();
    let mut bytes = [0_u8; 8192];
    loop {
        assert!(Instant::now() < deadline, "PTY output timed out");
        match pty.read(&mut bytes) {
            Ok(0) => return result,
            Ok(n) => result.extend_from_slice(&bytes[..n]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut fd = libc::pollfd {
                    fd: pty.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: poll sees one initialized live descriptor.
                unsafe {
                    libc::poll(&mut fd, 1, 20);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("PTY read failed: {error}"),
        }
    }
}

#[test]
fn spawned_child_has_controlling_terminal_and_requested_size() {
    let size = PtySize {
        rows: 37,
        cols: 109,
        ..PtySize::default()
    };
    let mut pty = Pty::spawn(command("test -t 0 && test -t 1 && test -t 2 || exit 9; stty size; printf '%s' \"$TERM:$COLORTERM:$TERM_PROGRAM\""), size).unwrap();
    let output = String::from_utf8(collect(&pty)).unwrap();
    assert!(output.contains("37 109\r\n"), "{output:?}");
    assert!(
        output.contains("xterm-256color:truecolor:Hotaru"),
        "{output:?}"
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = pty.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn pty_resize_and_input_reach_child() {
    let pty = Pty::spawn(
        command("read line; stty size; printf 'got:%s' \"$line\""),
        PtySize::default(),
    )
    .unwrap();
    pty.resize(PtySize {
        rows: 43,
        cols: 132,
        ..PtySize::default()
    })
    .unwrap();
    assert_eq!(pty.write(b"hotaru\n").unwrap(), 7);
    let output = String::from_utf8(collect(&pty)).unwrap();
    assert!(output.contains("43 132\r\n"), "{output:?}");
    assert!(output.contains("got:hotaru"), "{output:?}");
}

#[test]
fn explicit_environment_overrides_defaults() {
    let mut cmd = command("printf '%s' \"$TERM\"");
    cmd.env.push(("TERM".into(), "vt100".into()));
    let pty = Pty::spawn(cmd, PtySize::default()).unwrap();
    assert_eq!(collect(&pty), b"vt100");
}

#[test]
fn backpressure_on_terminal_input_is_nonblocking() {
    let pty = Pty::spawn(
        command("stty -echo -icanon; printf ready; sleep 10"),
        PtySize::default(),
    )
    .unwrap();
    // Wait for the child to disable echo, without filling the output side.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = [0_u8; 32];
    loop {
        if matches!(pty.read(&mut ready), Ok(n) if n > 0) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let bytes = [b'x'; 64 * 1024];
    let before = Instant::now();
    let mut observed_full = false;
    for _ in 0..1024 {
        match pty.write(&bytes) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                observed_full = true;
                break;
            }
            Ok(_) => {}
            Err(error) => panic!("write failed: {error}"),
        }
    }
    assert!(observed_full);
    assert!(before.elapsed() < Duration::from_secs(1));
}

#[test]
fn drop_is_nonblocking_and_reaps_hangup_ignoring_child() {
    let pty = Pty::spawn(
        command("trap '' HUP; printf ready; while :; do sleep 1; done"),
        PtySize::default(),
    )
    .unwrap();
    let child = pty.child_id();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = [0_u8; 32];
    loop {
        if matches!(pty.read(&mut ready), Ok(n) if n > 0) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let before = Instant::now();
    drop(pty);
    assert!(before.elapsed() < Duration::from_millis(100));
    while std::path::Path::new(&format!("/proc/{child}")).exists() {
        assert!(Instant::now() < deadline, "child was not reaped");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn invalid_dimensions_and_missing_program_fail_before_returning_a_session() {
    assert!(
        Pty::spawn(
            command("exit 0"),
            PtySize {
                rows: 0,
                ..PtySize::default()
            }
        )
        .is_err()
    );
    assert!(
        Pty::spawn(
            PtyCommand::new("/hotaru/nonexistent-program"),
            PtySize::default()
        )
        .is_err()
    );
}

#[test]
fn session_can_move_to_worker_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<Pty>();
}
