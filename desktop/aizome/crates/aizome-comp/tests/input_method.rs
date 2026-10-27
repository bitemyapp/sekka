//! Input methods (zwp_input_method_v2) typing into text inputs
//! (zwp_text_input_v3), with an input method that counts its serial as
//! IBus 1.5's Wayland frontend does: from zero again on every global the
//! registry announces, such as a display plugged in. Its commits must
//! still reach a text input that, like foot, applies only a `done` whose
//! serial counts its own commits: that once lost every letter typed into
//! foot after a second display came, while Return (passed through IBus's
//! virtual keyboard) still worked.

mod common;

use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aizome_ipc::Request;
use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::{zwp_input_method_manager_v2, zwp_input_method_v2};

#[derive(Clone, Debug, Default)]
struct ImSeen {
    bound: bool,
    active: bool,
    /// `done` events since it last counted afresh: the serial it commits.
    serial: u32,
    /// Globals announced after it started (each restarts its count).
    globals: u32,
}

enum ImCmd {
    Commit(String),
    Quit,
}

struct InputMethod {
    tx: Sender<ImCmd>,
    seen: Arc<Mutex<ImSeen>>,
    join: Option<JoinHandle<()>>,
}

impl InputMethod {
    fn spawn(c: &Compositor) -> InputMethod {
        let stream = UnixStream::connect(c.dir.join(&c.socket)).expect("connect to the compositor");
        let (tx, rx) = channel();
        let seen = Arc::new(Mutex::new(ImSeen::default()));
        let seen2 = seen.clone();
        let join = std::thread::spawn(move || run_im(stream, rx, seen2));
        let im = InputMethod { tx, seen, join: Some(join) };
        im.wait("bound", |s| s.bound);
        im
    }

    fn commit(&self, text: &str) {
        self.tx.send(ImCmd::Commit(text.into())).unwrap();
    }

    fn seen(&self) -> ImSeen {
        self.seen.lock().unwrap().clone()
    }

    fn wait(&self, what: &str, f: impl Fn(&ImSeen) -> bool) -> ImSeen {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = self.seen();
            if f(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out waiting for the input method: {what}: {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for InputMethod {
    fn drop(&mut self) {
        let _ = self.tx.send(ImCmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct Im {
    seen: Arc<Mutex<ImSeen>>,
    pending_active: Option<bool>,
}

fn run_im(stream: UnixStream, rx: Receiver<ImCmd>, seen: Arc<Mutex<ImSeen>>) {
    let conn = Connection::from_socket(stream).expect("wayland connection");
    let (globals, mut queue): (_, EventQueue<Im>) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).expect("wl_seat");
    let manager: zwp_input_method_manager_v2::ZwpInputMethodManagerV2 =
        globals.bind(&qh, 1..=1, ()).expect("zwp_input_method_manager_v2");
    let im = manager.get_input_method(&seat, &qh, ());
    let mut state = Im { seen: seen.clone(), pending_active: None };
    queue.roundtrip(&mut state).expect("roundtrip");
    seen.lock().unwrap().bound = true;
    loop {
        match rx.try_recv() {
            Ok(ImCmd::Commit(text)) => {
                im.commit_string(text);
                im.commit(seen.lock().unwrap().serial);
            }
            Ok(ImCmd::Quit) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        let _ = conn.flush();
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut pfd = libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&fd), events: libc::POLLIN, revents: 0 };
            // Safety: one valid pollfd.
            let n = unsafe { libc::poll(&mut pfd, 1, 20) };
            if n > 0 {
                if guard.read().is_err() {
                    break;
                }
            } else {
                drop(guard);
            }
        }
        if queue.dispatch_pending(&mut state).is_err() {
            break;
        }
    }
    im.destroy();
    let _ = conn.flush();
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Im {
    fn event(
        im: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // ibuswaylandim.c's registry_handle_global: `priv->im_serial = 0`
        // for every global, whatever it is.
        if let wl_registry::Event::Global { .. } = event {
            let mut seen = im.seen.lock().unwrap();
            seen.globals += 1;
            seen.serial = 0;
        }
    }
}

impl Dispatch<zwp_input_method_v2::ZwpInputMethodV2, ()> for Im {
    fn event(
        im: &mut Self,
        _: &zwp_input_method_v2::ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_input_method_v2::Event;
        match event {
            Event::Activate => im.pending_active = Some(true),
            Event::Deactivate => im.pending_active = Some(false),
            Event::Done => {
                let mut seen = im.seen.lock().unwrap();
                seen.serial += 1;
                if let Some(a) = im.pending_active.take() {
                    seen.active = a;
                }
            }
            _ => {}
        }
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Im {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(wl_seat::WlSeat, zwp_input_method_manager_v2::ZwpInputMethodManagerV2);

fn config(name: &str, enabled: bool) -> Request {
    Request::OutputConfig {
        name: name.into(),
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: Some(enabled),
    }
}

#[test]
fn an_input_methods_text_reaches_a_text_input_after_a_display_comes() {
    let c = Compositor::start(Options { outputs: &["1280x800@1", "1280x800@1"], ..Default::default() });
    let im = InputMethod::spawn(&c);
    let term = c.client(Spec::new("foot", 0x2222cc).text_input());
    c.wait_window("foot");
    term.wait_seen("its text input enabled", |s| s.keyboard_focus && s.ime_enabled);
    im.wait("activated for it", |s| s.active);

    im.commit("a");
    term.wait_seen("the first letter", |s| s.ime_text == "a");

    // Repeated docking must keep working without restarting the input
    // method, reopening the terminal or moving focus away and back.
    let mut expected = String::from("a");
    for text in ["izome", " second dock", " third dock"] {
        let before = im.seen();
        c.ok(config("HEADLESS-2", false));
        c.ok(config("HEADLESS-2", true));
        im.wait("the new display's global", |s| s.globals > before.globals);
        assert!(im.seen().active, "still active: {before:?}");

        im.commit(text);
        expected.push_str(text);
        let seen = term.wait_seen("what it typed next", |s| s.ime_text == expected || s.ime_dropped > 0);
        assert_eq!(seen.ime_text, expected, "the text input dropped the input method's commit: {seen:?}");
        assert_eq!(seen.ime_dropped, 0, "{seen:?}");
    }
}

#[test]
fn an_input_method_that_was_there_first_types_into_a_new_window() {
    let c = Compositor::start(Options::default());
    let im = InputMethod::spawn(&c);
    let a = c.client(Spec::new("org.example.first", 0x22cc22).text_input());
    c.wait_window("org.example.first");
    a.wait_seen("enabled", |s| s.ime_enabled);
    let b = c.client(Spec::new("foot", 0x2222cc).text_input());
    c.wait_window("foot");
    b.wait_seen("the new window enabled", |s| s.keyboard_focus && s.ime_enabled);
    a.wait_seen("the first one left", |s| !s.ime_enabled);
    im.wait("active", |s| s.active);
    im.commit("ls");
    b.wait_seen("typed", |s| s.ime_text == "ls");
    assert_eq!(a.seen().ime_text, "", "the window without the keys got nothing");
}
