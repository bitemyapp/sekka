//! A PTY and terminal state live on one worker, awakened with an eventfd.
//! The UI only exchanges commands and immutable, shared row snapshots.

use crate::app::Msg;
use aizome_kit::MsgSender;
use hotaru_core::{Terminal, TerminalColors, TerminalEvent};
use hotaru_io::{Pty, PtyCommand, PtySize};
use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(8);
const INPUT_LIMIT: usize = 32 * 1024 * 1024;
const REPLY_LIMIT: usize = 64 * 1024;

pub enum Command {
    Input(Arc<[u8]>),
    Key(Arc<hotaru_core::input::KeyEvent>),
    Mouse(crate::input::MouseInput),
    Wheel(crate::input::MouseInput, i32),
    Focus(bool),
    Paste(Arc<str>),
    Resize { cols: usize, rows: usize },
    Scroll(i32),
    CopyAll(u64),
    Colors(Arc<TerminalColors>),
}
struct Shared {
    wake: OwnedFd,
    frame_pending: AtomicBool,
    input_bytes: AtomicUsize,
    stopping: AtomicBool,
    controls: Mutex<Controls>,
}
#[derive(Default)]
struct Controls {
    resize: Option<(usize, usize)>,
    colors: Option<Arc<TerminalColors>>,
}
pub struct Session {
    tx: mpsc::SyncSender<Command>,
    shared: Arc<Shared>,
}

impl Shared {
    fn wake(&self) {
        let one = 1u64.to_ne_bytes();
        loop {
            // SAFETY: this Arc owns the live eventfd; one is eight readable bytes.
            // EAGAIN means it is already signaled. EINTR must not lose this wake.
            let result = unsafe { libc::write(self.wake.as_raw_fd(), one.as_ptr().cast(), one.len()) };
            if result >= 0 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break;
            }
        }
    }
}
impl Session {
    pub fn spawn(
        id: u64,
        command: PtyCommand,
        colors: TerminalColors,
        scrollback: usize,
        tx: MsgSender<Msg>,
    ) -> io::Result<Self> {
        let (session, receiver) = Self::channel()?;
        let worker = session.shared.clone();
        std::thread::Builder::new().name(format!("hotaru-pty-{id}")).spawn(move || {
            if let Err(e) = run(id, command, colors, scrollback, tx.clone(), receiver, worker.clone()) {
                tx.send(Msg::Failed(id, e.to_string()));
            }
            worker.stopping.store(true, Ordering::Release);
        })?;
        Ok(session)
    }

    fn channel() -> io::Result<(Self, mpsc::Receiver<Command>)> {
        // SAFETY: eventfd takes no pointers. A successful fd is taken once below.
        let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let shared = Arc::new(Shared {
            wake: unsafe { OwnedFd::from_raw_fd(raw) },
            frame_pending: AtomicBool::new(false),
            input_bytes: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
            controls: Mutex::new(Controls::default()),
        });
        let (sender, receiver) = mpsc::sync_channel(64);
        Ok((Self { tx: sender, shared }, receiver))
    }
    pub fn command(&self, command: Command) -> Result<(), &'static str> {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err("The terminal session has ended.");
        }
        let command = match command {
            Command::Resize { cols, rows } => {
                self.shared.controls.lock().unwrap_or_else(|e| e.into_inner()).resize = Some((cols, rows));
                self.shared.wake();
                return Ok(());
            }
            Command::Colors(colors) => {
                // Move and drop even the old Arc outside the critical section.
                let old = self.shared.controls.lock().unwrap_or_else(|e| e.into_inner()).colors.replace(colors);
                drop(old);
                self.shared.wake();
                return Ok(());
            }
            command => command,
        };
        let bytes = match &command {
            Command::Input(bytes) => bytes.len(),
            Command::Paste(text) => text.len().saturating_add(12),
            Command::Key(event) => event.text.len().saturating_mul(12).saturating_add(128),
            Command::Mouse(_) => 128,
            Command::Wheel(_, _) => 4096,
            Command::Focus(_) => 3,
            _ => 0,
        };
        if self
            .shared
            .input_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending.checked_add(bytes).filter(|n| *n <= INPUT_LIMIT)
            })
            .is_err()
        {
            return Err("Input exceeds the 32 MiB pending limit.");
        }
        self.tx.try_send(command).map_err(|e| {
            self.shared.input_bytes.fetch_sub(bytes, Ordering::AcqRel);
            match e {
                mpsc::TrySendError::Full(_) => "Terminal input queue is full; wait for the running program.",
                mpsc::TrySendError::Disconnected(_) => "The terminal session has ended.",
            }
        })?;
        self.shared.wake();
        Ok(())
    }
    pub fn received_frame(&self) {
        self.shared.frame_pending.store(false, Ordering::Release);
        self.shared.wake();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Independent of channel capacity and the precise sender-drop timing.
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.wake();
    }
}

struct PendingWrite {
    bytes: Arc<[u8]>,
    user: bool,
}

fn encoded_input(
    output: &mut VecDeque<PendingWrite>,
    shared: &Shared,
    bytes: Vec<u8>,
    reserved: usize,
) -> io::Result<()> {
    let unused =
        reserved.checked_sub(bytes.len()).ok_or_else(|| io::Error::other("Encoded input exceeded its reservation"))?;
    shared.input_bytes.fetch_sub(unused, Ordering::AcqRel);
    if !bytes.is_empty() {
        output.push_back(PendingWrite { bytes: bytes.into(), user: true });
    }
    Ok(())
}

fn publish_events(terminal: &mut Terminal, id: u64, tx: &MsgSender<Msg>) {
    let (mut title, mut bell, mut clipboard) = (None, false, false);
    for event in terminal.take_events() {
        match event {
            TerminalEvent::Title(value) => title = Some(value),
            TerminalEvent::Bell => bell = true,
            TerminalEvent::Clipboard { .. } => clipboard = true,
        }
    }
    if let Some(title) = title {
        tx.send(Msg::Title(id, title));
    }
    if bell {
        tx.send(Msg::Bell(id));
    }
    if clipboard {
        tx.send(Msg::Notice(id, "A program requested clipboard access; OSC 52 is disabled.".into()));
    }
}

fn run(
    id: u64,
    command: PtyCommand,
    colors: TerminalColors,
    history: usize,
    tx: MsgSender<Msg>,
    commands: mpsc::Receiver<Command>,
    shared: Arc<Shared>,
) -> io::Result<()> {
    let mut pty = Pty::spawn(command, PtySize::default())?;
    let mut terminal = Terminal::new(80, 24, history);
    terminal.set_default_colors(colors);
    let mut output = VecDeque::<PendingWrite>::new();
    let (mut output_at, mut reply_bytes) = (0usize, 0usize);
    let mut buffer = [0u8; 65_536];
    let (mut buffered_at, mut buffered_end) = (0usize, 0usize);
    let mut readable = true;
    let (mut eof, mut hung_up, mut notified_exit) = (false, false, false);
    let mut changed = true;
    let mut last_frame = Instant::now() - FRAME;
    let mut scrollback = 0usize;
    let mut mouse = hotaru_core::input::MouseEncoder::default();
    let mut focused = None;
    let mut last_activity = Instant::now();
    let mut compress_more = false;
    loop {
        if shared.stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        // These latest-state controls cannot be rejected by a full input queue.
        // Never reflow, allocate, or call the PTY while holding the UI's mutex.
        let controls = std::mem::take(&mut *shared.controls.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some((cols, rows)) = controls.resize {
            let (cols, rows) = (cols.clamp(1, 4096), rows.clamp(1, 4096));
            if !eof && !hung_up {
                pty.resize(PtySize { cols: cols as u16, rows: rows as u16, pixel_width: 0, pixel_height: 0 })?;
            }
            terminal.resize(cols, rows);
            last_activity = Instant::now();
            compress_more = true;
            changed = true;
        }
        if let Some(colors) = controls.colors {
            terminal.set_default_colors((*colors).clone());
            changed = true;
        }
        for _ in 0..64 {
            if shared.stopping.load(Ordering::Acquire) {
                return Ok(());
            }
            match commands.try_recv() {
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => break,
                Ok(Command::Input(bytes)) => {
                    if eof || hung_up {
                        shared.input_bytes.fetch_sub(bytes.len(), Ordering::AcqRel);
                    } else if !bytes.is_empty() {
                        output.push_back(PendingWrite { bytes, user: true });
                    }
                    if scrollback != 0 {
                        scrollback = 0;
                        changed = true;
                    }
                }
                Ok(Command::Key(event)) => {
                    let reserved = event.text.len().saturating_mul(12).saturating_add(128);
                    let bytes = if eof || hung_up {
                        Vec::new()
                    } else {
                        hotaru_core::input::encode_key(&event, terminal.keyboard_options())
                    };
                    encoded_input(&mut output, &shared, bytes, reserved)?;
                    if scrollback != 0 && event.action != hotaru_core::input::KeyAction::Release {
                        scrollback = 0;
                        changed = true;
                    }
                }
                Ok(Command::Mouse(event)) => {
                    let (tracking, format) = terminal.mouse_modes();
                    let bytes = if eof || hung_up { Vec::new() } else { event.encode(&mut mouse, tracking, format) };
                    encoded_input(&mut output, &shared, bytes, 128)?;
                }
                Ok(Command::Wheel(mut event, lines)) => {
                    use hotaru_core::input::{Key, KeyEvent, MouseButton, MouseTracking};
                    let (tracking, format) = terminal.mouse_modes();
                    let modes = terminal.modes();
                    let mut bytes = Vec::new();
                    if !eof && !hung_up && tracking != MouseTracking::None {
                        event.event.button =
                            Some(if lines > 0 { MouseButton::WheelUp } else { MouseButton::WheelDown });
                        for _ in 0..lines.unsigned_abs().min(32) {
                            bytes.extend(event.encode(&mut mouse, tracking, format));
                        }
                    } else if !eof && !hung_up && modes.alternate_screen && modes.mouse_alternate_scroll {
                        let key = KeyEvent::new(if lines > 0 { Key::ArrowUp } else { Key::ArrowDown });
                        for _ in 0..lines.unsigned_abs().min(32) {
                            bytes.extend(hotaru_core::input::encode_key(&key, terminal.keyboard_options()));
                        }
                    } else {
                        scrollback = scrollback.saturating_add_signed(lines as isize).min(terminal.scrollback_len());
                        changed = true;
                    }
                    encoded_input(&mut output, &shared, bytes, 4096)?;
                }
                Ok(Command::Focus(value)) => {
                    let bytes = if !eof && !hung_up && focused != Some(value) && terminal.modes().focus_events {
                        if value { b"\x1b[I" } else { b"\x1b[O" }.to_vec()
                    } else {
                        Vec::new()
                    };
                    focused = Some(value);
                    encoded_input(&mut output, &shared, bytes, 3)?;
                }
                Ok(Command::Paste(text)) => {
                    if eof || hung_up {
                        shared.input_bytes.fetch_sub(text.len() + 12, Ordering::AcqRel);
                    } else {
                        let bytes = crate::input::paste_bytes(&text, terminal.modes().bracketed_paste);
                        shared.input_bytes.fetch_sub(text.len() + 12 - bytes.len(), Ordering::AcqRel);
                        if !bytes.is_empty() {
                            output.push_back(PendingWrite { bytes: bytes.into(), user: true });
                        }
                    }
                    if scrollback != 0 {
                        scrollback = 0;
                        changed = true;
                    }
                }
                Ok(Command::Scroll(lines)) => {
                    scrollback = scrollback.saturating_add_signed(lines as isize).min(terminal.scrollback_len());
                    changed = true;
                }
                Ok(Command::CopyAll(token)) => {
                    tx.send(Msg::CopySnapshot(token, Arc::new(terminal.snapshot_all())));
                }
                Ok(Command::Resize { .. } | Command::Colors(_)) => unreachable!("controls use the mailbox"),
            }
        }

        // Keep unread bytes in this fixed buffer until bounded reply storage has
        // space. No live peer's protocol reply is dropped to make room for paste.
        let mut fed = 0;
        while fed < 1024 * 1024 && (hung_up || reply_bytes < REPLY_LIMIT) {
            if buffered_at == buffered_end {
                if eof || !readable {
                    break;
                }
                match pty.read(&mut buffer) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        buffered_at = 0;
                        buffered_end = n;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        readable = false;
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            let budget = if hung_up { REPLY_LIMIT } else { REPLY_LIMIT - reply_bytes };
            let consumed = terminal.feed_bounded(&buffer[buffered_at..buffered_end], budget);
            buffered_at += consumed;
            fed += consumed;
            changed |= consumed != 0;
            publish_events(&mut terminal, id, &tx);
            let replies = terminal.take_responses();
            if !hung_up && !replies.is_empty() {
                reply_bytes += replies.len();
                output.push_back(PendingWrite { bytes: replies.into(), user: false });
            }
            if consumed == 0 {
                break;
            }
        }

        // Input and replies retain ordering, with independent memory reservations.
        if fed != 0 {
            last_activity = Instant::now();
            compress_more = true;
        }
        let mut written = 0;
        while written < 256 * 1024 && !hung_up {
            let Some(front) = output.front() else { break };
            match pty.write(&front.bytes[output_at..]) {
                Ok(0) => break,
                Ok(n) => {
                    output_at += n;
                    written += n;
                    if front.user {
                        shared.input_bytes.fetch_sub(n, Ordering::AcqRel);
                    } else {
                        reply_bytes -= n;
                    }
                    if output_at == front.bytes.len() {
                        output.pop_front();
                        output_at = 0;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.raw_os_error() == Some(libc::EIO) => {
                    hung_up = true;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        if hung_up {
            // Every slave descriptor closed: no recipient can receive queued
            // input/replies. Continue draining final output instead of spinning
            // on a perpetual POLLHUP with reply throttling still engaged.
            for (index, pending) in output.drain(..).enumerate() {
                if pending.user {
                    shared
                        .input_bytes
                        .fetch_sub(pending.bytes.len() - if index == 0 { output_at } else { 0 }, Ordering::AcqRel);
                }
            }
            output_at = 0;
            reply_bytes = 0;
        }
        if changed && !shared.frame_pending.load(Ordering::Acquire) && (eof || last_frame.elapsed() >= FRAME) {
            shared.frame_pending.store(true, Ordering::Release);
            if !tx.send(Msg::Frame(id, Arc::new(terminal.snapshot_scrolled(scrollback)))) {
                return Ok(());
            }
            changed = false;
            last_frame = Instant::now();
        }
        if eof
            && !changed
            && !notified_exit
            && let Some(status) = pty.try_wait()?
        {
            tx.send(Msg::Exited(id, status.to_string()));
            notified_exit = true;
            // Retain history for copying/scrolling until the tab is closed.
        }
        let can_feed = hung_up || reply_bytes < REPLY_LIMIT;
        let ready_work = can_feed && (buffered_at < buffered_end || (!eof && readable));
        if compress_more && !ready_work && output.is_empty() && last_activity.elapsed() >= Duration::from_millis(100) {
            compress_more = terminal.compress_history_step();
        }
        let timeout = if ready_work {
            0
        } else if changed && !shared.frame_pending.load(Ordering::Acquire) {
            FRAME.saturating_sub(last_frame.elapsed()).as_millis().clamp(1, 8) as i32
        } else if eof && !notified_exit {
            100
        } else if compress_more && output.is_empty() {
            Duration::from_millis(100).saturating_sub(last_activity.elapsed()).as_millis().clamp(5, 100) as i32
        } else {
            -1
        };
        let mut polls = [
            libc::pollfd { fd: shared.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd {
                fd: if eof { -1 } else { pty.as_fd().as_raw_fd() },
                events: if can_feed { libc::POLLIN } else { 0 } | if output.is_empty() { 0 } else { libc::POLLOUT },
                revents: 0,
            },
        ];
        // SAFETY: initialized pollfd storage and owned fds remain alive throughout.
        if unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, timeout) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if polls[0].revents & libc::POLLIN != 0 {
            let mut value = 0u64;
            // SAFETY: value is eight writable bytes and the eventfd is still owned.
            unsafe {
                libc::read(shared.wake.as_raw_fd(), (&mut value as *mut u64).cast(), 8);
            }
        }
        if polls[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            readable = true;
        }
        if polls[1].revents & libc::POLLHUP != 0 {
            hung_up = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aizome_kit::prelude::*;
    use aizome_kit::testing::Harness;
    use hotaru_core::{Rgb, Snapshot};

    struct Probe {
        session: Session,
        sender: Option<MsgSender<Msg>>,
        frame: Option<Arc<Snapshot>>,
        exited: bool,
    }

    impl Program for Probe {
        type Msg = Msg;
        fn info(&self) -> AppInfo {
            AppInfo::new("org.aizome.Hotaru.ControlTest", "Control test", "Control test")
        }
        fn launched(&mut self, cx: &mut Cx<Msg>) {
            self.sender = Some(cx.stream());
            cx.open_window(WindowSpec::new("Control test").size(100.0, 100.0));
        }
        fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
            match msg {
                Msg::Frame(_, frame) => {
                    self.frame = Some(frame);
                    self.session.received_frame();
                }
                Msg::Exited(..) => self.exited = true,
                Msg::Failed(_, error) => panic!("PTY control test failed: {error}"),
                _ => {}
            }
        }
        fn view(&self, _window: WindowKey) -> Element<Msg> {
            text("Control test").into()
        }
    }

    #[test]
    fn latest_resize_and_colors_survive_a_full_input_queue() {
        let (session, commands) = Session::channel().unwrap();
        for _ in 0..64 {
            session.command(Command::Input(Arc::from(&b""[..]))).unwrap();
        }
        assert!(session.command(Command::Input(Arc::from(&b""[..]))).unwrap_err().contains("queue is full"));
        for cols in 80..180 {
            session.command(Command::Resize { cols, rows: 50 }).unwrap();
            session.command(Command::Colors(Arc::new(TerminalColors::default()))).unwrap();
        }
        let colors =
            TerminalColors { foreground: Rgb(17, 29, 43), background: Rgb(59, 71, 89), ..TerminalColors::default() };
        session.command(Command::Resize { cols: 201, rows: 51 }).unwrap();
        session.command(Command::Colors(Arc::new(colors.clone()))).unwrap();
        assert!(session.command(Command::Input(Arc::from(&b""[..]))).is_err());
        assert_eq!(session.shared.input_bytes.load(Ordering::Acquire), 0);

        // Begin consumption only after saturation, making the regression
        // deterministic without sleeps, thread suspension, or a production hook.
        let shared = session.shared.clone();
        let mut harness = Harness::new(Probe { session, sender: None, frame: None, exited: false });
        let sender = harness.program().sender.clone().unwrap();
        let mut child = PtyCommand::new(std::env::current_exe().unwrap());
        child.args =
            ["--ignored", "--exact", "session::tests::resize_control_child", "--nocapture", "--test-threads=1"]
                .into_iter()
                .map(Into::into)
                .collect();
        child.env.push(("HOTARU_RESIZE_CONTROL_CHILD".into(), "1".into()));
        let worker =
            std::thread::spawn(move || run(1, child, TerminalColors::default(), 100, sender, commands, shared));
        assert!(harness.settle(Duration::from_secs(5), |probe| probe.exited));
        let frame = harness.program().frame.as_ref().unwrap();
        assert_eq!((frame.cols, frame.rows), (201, 51));
        assert_eq!(frame.default_foreground, colors.foreground);
        assert_eq!(frame.default_background, colors.background);
        assert!(frame.plain_text().contains("KERNEL-SIZE-201x51"));
        drop(harness);
        worker.join().unwrap().unwrap();
    }

    #[test]
    #[ignore]
    fn resize_control_child() {
        if std::env::var_os("HOTARU_RESIZE_CONTROL_CHILD").is_none() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
            // SAFETY: fd0 is this isolated child's PTY; size is writable and
            // TIOCGWINSZ initializes it synchronously before returning.
            assert_eq!(unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut size) }, 0);
            if size.ws_col == 201 && size.ws_row == 51 {
                println!("KERNEL-SIZE-201x51");
                return;
            }
            assert!(Instant::now() < deadline, "latest PTY resize was lost");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
