//! A PTY and terminal state live on one worker, awakened with an eventfd; a
//! second thread reads the PTY's output for it, so reading overlaps parsing.
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
// A failing command that ends this soon is abnormal (Ghostty's
// `abnormal-command-exit-runtime` on Linux): its pane stays to be read.
const ABNORMAL_EXIT: Duration = Duration::from_millis(250);
// A paste event's listing is one data packet of at most 4 KiB, encoded, in
// three packets carrying the password.
const PASTE_EVENT: usize = 6 * 1024;

pub enum Command {
    Input(Arc<[u8]>),
    Key(Arc<hotaru_core::input::KeyEvent>),
    Mouse(crate::input::MouseInput),
    Wheel(crate::input::MouseInput, i32),
    Focus(bool),
    Paste(Arc<str>),
    Resize { cols: usize, rows: usize },
    ResizePixels { cols: usize, rows: usize, width: u16, height: u16 },
    Scroll(i32),
    CopyAll(u64),
    Directory(mpsc::Sender<Option<std::path::PathBuf>>),
    Search(SearchControl),
    Colors(Arc<TerminalColors>),
    // The cursor programs get until they ask for another, and whether it blinks.
    Cursor(hotaru_core::CursorShape, bool),
    // Clear the scrollback and screen; at a marked prompt the shell redraws it.
    Clear,
    // A hard reset of the terminal, as RIS.
    Reset,
    // A program's answer from the clipboard (OSC 52 or 5522), written whole.
    Reply(Arc<[u8]>),
    // Let the program's Kitty clipboard requests with this password go
    // without asking, that way, until the terminal resets.
    KittyGrant(Arc<str>, hotaru_core::kitty_clipboard::Direction),
    // A user's paste, for a program that asked for paste events (mode
    // 5522): the clipboard's types, which it may read once.
    PasteEvent { primary: bool, types: Arc<[String]> },
}
/// What a new pane's terminal starts with.
#[derive(Clone)]
pub struct Defaults {
    pub colors: TerminalColors,
    pub scrollback: usize,
    pub cursor: (hotaru_core::CursorShape, bool),
}
pub enum SearchControl {
    Capture { token: u64, cancel: Arc<AtomicBool> },
    Navigate { token: u64, results: Arc<hotaru_core::search::Results>, index: usize, cancel: Arc<AtomicBool> },
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
    resize: Option<(usize, usize, u16, u16)>,
    colors: Option<Arc<TerminalColors>>,
    cursor: Option<(hotaru_core::CursorShape, bool)>,
    search_capture: Option<SearchControl>,
    search_navigation: Option<SearchControl>,
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
    pub fn spawn(id: u64, command: PtyCommand, defaults: Defaults, tx: MsgSender<Msg>) -> io::Result<Self> {
        let (session, receiver) = Self::channel()?;
        let worker = session.shared.clone();
        std::thread::Builder::new().name(format!("hotaru-pty-{id}")).spawn(move || {
            if let Err(e) = run(id, command, defaults, tx.clone(), receiver, worker.clone()) {
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
                self.shared.controls.lock().unwrap_or_else(|e| e.into_inner()).resize = Some((cols, rows, 0, 0));
                self.shared.wake();
                return Ok(());
            }
            Command::ResizePixels { cols, rows, width, height } => {
                self.shared.controls.lock().unwrap_or_else(|e| e.into_inner()).resize =
                    Some((cols, rows, width, height));
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
            Command::Cursor(shape, blinking) => {
                self.shared.controls.lock().unwrap_or_else(|e| e.into_inner()).cursor = Some((shape, blinking));
                self.shared.wake();
                return Ok(());
            }
            Command::Search(control) => {
                let old = {
                    let mut controls = self.shared.controls.lock().unwrap_or_else(|e| e.into_inner());
                    match control {
                        SearchControl::Capture { .. } => controls.search_capture.replace(control),
                        SearchControl::Navigate { .. } => controls.search_navigation.replace(control),
                    }
                };
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
            // The form feed a prompt's redraw takes.
            Command::Clear => 1,
            Command::Reply(bytes) => bytes.len(),
            Command::PasteEvent { .. } => PASTE_EVENT,
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

// PTY output is gathered on a thread of its own, as Ghostty's io-gather does.
// Reading is mostly kernel time, a few KiB per read; overlapping it with
// parsing on the session's thread is most of a bulk-output speedup.
const GATHER_CHUNK: usize = 64 * 1024;
// Bounded, so a terminal that stops consuming (its reply budget) stops the
// reader, and with it the writing program.
const GATHER_QUEUE: usize = 4;

enum Gathered {
    Output(Vec<u8>),
    /// The PTY ended: EOF (Linux's EIO once the last slave closes), or an error.
    End(io::Result<()>),
}

/// The session's side of its PTY reader thread.
struct Gather {
    output: mpsc::Receiver<Gathered>,
    spare: mpsc::SyncSender<Vec<u8>>,
    stop: OwnedFd,
}

impl Gather {
    fn spawn(id: u64, pty: &Pty, shared: Arc<Shared>) -> io::Result<Self> {
        // The reader owns a duplicate, so the session keeps the PTY itself for
        // writes, resizes and its child.
        let fd = pty.as_fd().try_clone_to_owned()?;
        // SAFETY: eventfd takes no pointers. A successful fd is taken once below.
        let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let stop = unsafe { OwnedFd::from_raw_fd(raw) };
        let reader_stop = stop.try_clone()?;
        let (sender, output) = mpsc::sync_channel(GATHER_QUEUE);
        let (spare, spares) = mpsc::sync_channel(GATHER_QUEUE);
        std::thread::Builder::new()
            .name(format!("hotaru-read-{id}"))
            .spawn(move || gather(fd, reader_stop, sender, spares, shared))?;
        Ok(Self { output, spare, stop })
    }

    /// A drained chunk's buffer goes back to the reader.
    fn recycle(&self, buffer: Vec<u8>) {
        if buffer.capacity() != 0 {
            let _ = self.spare.try_send(buffer);
        }
    }
}

impl Drop for Gather {
    fn drop(&mut self) {
        let one = 1u64.to_ne_bytes();
        // SAFETY: this owns the live eventfd; one is eight readable bytes.
        unsafe {
            libc::write(self.stop.as_raw_fd(), one.as_ptr().cast(), one.len());
        }
    }
}

/// Reads a PTY until it ends, in chunks of whatever it has ready, and hands
/// them to the session. Stops when the session drops its side.
fn gather(
    fd: OwnedFd,
    stop: OwnedFd,
    output: mpsc::SyncSender<Gathered>,
    spares: mpsc::Receiver<Vec<u8>>,
    shared: Arc<Shared>,
) {
    loop {
        let mut buffer = spares.try_recv().unwrap_or_default();
        buffer.clear();
        buffer.reserve_exact(GATHER_CHUNK);
        let mut ended = None;
        while buffer.len() < GATHER_CHUNK {
            let room = GATHER_CHUNK - buffer.len();
            // SAFETY: the descriptor is live; the reserved capacity is writable.
            let read = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().add(buffer.len()).cast(), room) };
            if read > 0 {
                // SAFETY: read initialized this many more bytes.
                unsafe { buffer.set_len(buffer.len() + read as usize) };
                continue;
            }
            let error = io::Error::last_os_error();
            if read == 0 || error.raw_os_error() == Some(libc::EIO) {
                ended = Some(Ok(()));
            } else if error.kind() == io::ErrorKind::Interrupted {
                continue;
            } else if error.kind() != io::ErrorKind::WouldBlock {
                ended = Some(Err(error));
            }
            break;
        }
        let full = buffer.len() == GATHER_CHUNK;
        if !buffer.is_empty() {
            // Blocks while the session's queue is full; fails once it has gone.
            if output.send(Gathered::Output(buffer)).is_err() {
                return;
            }
            shared.wake();
        }
        if let Some(result) = ended {
            let _ = output.send(Gathered::End(result));
            shared.wake();
            return;
        }
        if full {
            continue;
        }
        let mut polls = [
            libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: initialized pollfd storage and owned fds remain alive throughout.
        if unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, -1) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                let _ = output.send(Gathered::End(Err(error)));
                shared.wake();
                return;
            }
            continue;
        }
        if polls[1].revents != 0 {
            return;
        }
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

/// Bytes for a one-time password: far more than its 22 characters need,
/// though rejection sampling discards some.
fn secure_random() -> Option<Vec<u8>> {
    use std::io::Read;
    let mut random = vec![0; 128];
    std::fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut random)).ok()?;
    Some(random)
}

fn publish_events(
    terminal: &mut Terminal,
    id: u64,
    tx: &MsgSender<Msg>,
    hostname: Option<&str>,
    seen_title: &mut bool,
    clipboard: &Arc<crate::selections::Bounds>,
    replies: &mut Vec<u8>,
) {
    let (mut title, mut directory, mut bell, mut notification) = (None, None, false, None);
    for event in terminal.take_events() {
        match event {
            TerminalEvent::Title(value) => {
                *seen_title = true;
                title = Some(value);
            }
            TerminalEvent::WorkingDirectory(uri) => {
                if uri.is_empty() {
                    directory = Some(std::path::PathBuf::new());
                } else if let Some(path) = hostname.and_then(|host| hotaru_io::launch::directory_from_osc7(&uri, host))
                {
                    // Derive the cwd title as native text. Shell hooks never
                    // emit untrusted filename bytes inside an OSC title.
                    if !*seen_title {
                        title = Some(path.to_string_lossy().into_owned());
                    }
                    directory = Some(path);
                }
            }
            TerminalEvent::Bell => bell = true,
            // In order, as they came. Past the pane's bounds a write is
            // dropped and a read answered empty, so its program doesn't wait.
            TerminalEvent::Clipboard { selection, data } => {
                match crate::selections::request(id, selection, &data, clipboard) {
                    Some(request) => {
                        tx.send(Msg::Clipboard(request));
                    }
                    None if data == b"?" => replies.extend_from_slice(&crate::selections::reply(selection, b"")),
                    None => {}
                }
            }
            TerminalEvent::KittyClipboard(request) => match crate::selections::kitty(id, request, clipboard) {
                Ok(request) => {
                    tx.send(Msg::Clipboard(request));
                }
                Err(answer) => replies.extend_from_slice(&answer),
            },
            TerminalEvent::Notification { title, body } => notification = Some((title, body)),
        }
    }
    if let Some(title) = title {
        tx.send(Msg::Title(id, title));
    }
    if let Some(directory) = directory {
        tx.send(Msg::WorkingDirectory(id, directory));
    }
    if bell {
        tx.send(Msg::Bell(id));
    }
    if let Some((title, body)) = notification {
        tx.send(Msg::Notify(id, title, body));
    }
}

fn run(
    id: u64,
    command: PtyCommand,
    defaults: Defaults,
    tx: MsgSender<Msg>,
    commands: mpsc::Receiver<Command>,
    shared: Arc<Shared>,
) -> io::Result<()> {
    let mut pty = Pty::spawn(command, PtySize::default())?;
    let gather = Gather::spawn(id, &pty, shared.clone())?;
    let started = Instant::now();
    let clipboard = Arc::new(crate::selections::Bounds::default());
    let hostname = hotaru_io::launch::local_hostname();
    let mut seen_title = false;
    let mut terminal = Terminal::new(80, 24, defaults.scrollback);
    terminal.set_default_cursor(defaults.cursor.0, defaults.cursor.1);
    terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
    terminal.set_default_colors(defaults.colors);
    // One Kitty write can be as large as a pane's requests in waiting.
    terminal.set_kitty_clipboard_write_limit(crate::selections::MAX_PENDING_BYTES);
    let mut output = VecDeque::<PendingWrite>::new();
    let (mut output_at, mut reply_bytes) = (0usize, 0usize);
    // The reader's latest chunk, and how much of it the terminal has taken.
    let mut chunk = Vec::new();
    let mut chunk_at = 0usize;
    // The reader may have queued output.
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
        if let Some((cols, rows, width, height)) = controls.resize {
            let (cols, rows) = (cols.clamp(1, 4096), rows.clamp(1, 4096));
            if !eof && !hung_up {
                pty.resize(PtySize { cols: cols as u16, rows: rows as u16, pixel_width: width, pixel_height: height })?;
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
        if let Some((shape, blinking)) = controls.cursor {
            terminal.set_default_cursor(shape, blinking);
            changed = true;
        }
        for control in controls.search_navigation.into_iter().chain(controls.search_capture) {
            match control {
                SearchControl::Capture { token, cancel } if !cancel.load(Ordering::Acquire) => {
                    tx.send(Msg::SearchSnapshot(token, id, Arc::new(terminal.content_snapshot())));
                }
                SearchControl::Navigate { token, results, index, cancel } if !cancel.load(Ordering::Acquire) => {
                    let position = terminal.content_position(0);
                    let found = results.resolve_match(index, &position, terminal.generation());
                    if let Some(found) = found {
                        scrollback =
                            position.history_rows.saturating_sub(found.start.row).min(terminal.scrollback_len());
                        changed = true;
                    }
                    tx.send(Msg::SearchNavigated(token, id, found.is_some()));
                }
                _ => {}
            }
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
                    tx.send(Msg::CopySnapshot(token, id, Arc::new(terminal.content_snapshot())));
                }
                Ok(Command::Directory(reply)) => {
                    let directory = (!notified_exit)
                        .then(|| std::fs::read_link(format!("/proc/{}/cwd", pty.child_id())).ok())
                        .flatten();
                    let _ = reply.send(directory);
                }
                Ok(Command::Clear) => {
                    // Control-L asks the shell to clear and draw its prompt.
                    let redraw = terminal.clear_screen() && !eof && !hung_up;
                    encoded_input(&mut output, &shared, if redraw { vec![0x0c] } else { Vec::new() }, 1)?;
                    scrollback = 0;
                    changed = true;
                }
                Ok(Command::Reset) => {
                    terminal.reset();
                    scrollback = 0;
                    changed = true;
                }
                Ok(Command::Reply(bytes)) => {
                    if eof || hung_up {
                        shared.input_bytes.fetch_sub(bytes.len(), Ordering::AcqRel);
                    } else if !bytes.is_empty() {
                        output.push_back(PendingWrite { bytes, user: true });
                    }
                }
                Ok(Command::KittyGrant(pw, direction)) => terminal.kitty_clipboard_grant(&pw, direction),
                Ok(Command::PasteEvent { primary, types }) => {
                    // Nothing goes if the program turned paste events off
                    // meanwhile, as in Ghostty.
                    let bytes = if eof || hung_up {
                        None
                    } else {
                        secure_random().and_then(|random| terminal.kitty_paste_event(primary, &types, &random))
                    };
                    encoded_input(&mut output, &shared, bytes.unwrap_or_default(), PASTE_EVENT)?;
                    if scrollback != 0 {
                        scrollback = 0;
                        changed = true;
                    }
                }
                Ok(
                    Command::Resize { .. }
                    | Command::ResizePixels { .. }
                    | Command::Colors(_)
                    | Command::Cursor(..)
                    | Command::Search(_),
                ) => {
                    unreachable!("controls use the mailbox")
                }
            }
        }

        // Keep unread bytes in the current chunk until bounded reply storage has
        // space; the reader's queue then fills and it stops reading. No live
        // peer's protocol reply is dropped to make room for paste.
        let mut fed = 0;
        while fed < 1024 * 1024 && (hung_up || reply_bytes < REPLY_LIMIT) {
            if chunk_at == chunk.len() {
                if eof || !readable {
                    break;
                }
                match gather.output.try_recv() {
                    Ok(Gathered::Output(bytes)) => {
                        gather.recycle(std::mem::replace(&mut chunk, bytes));
                        chunk_at = 0;
                    }
                    Ok(Gathered::End(result)) => {
                        result?;
                        eof = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        readable = false;
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        eof = true;
                        break;
                    }
                }
            }
            let budget = if hung_up { REPLY_LIMIT } else { REPLY_LIMIT - reply_bytes };
            let consumed = terminal.feed_bounded(&chunk[chunk_at..], budget);
            chunk_at += consumed;
            fed += consumed;
            changed |= consumed != 0;
            let mut replies = terminal.take_responses();
            publish_events(&mut terminal, id, &tx, hostname.as_deref(), &mut seen_title, &clipboard, &mut replies);
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
            tx.send(Msg::Exited(id, status.to_string(), !status.success() && started.elapsed() < ABNORMAL_EXIT));
            notified_exit = true;
            // Retain history for copying/scrolling until the tab is closed.
        }
        let can_feed = hung_up || reply_bytes < REPLY_LIMIT;
        let ready_work = can_feed && (chunk_at < chunk.len() || (!eof && readable));
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
            // The reader thread reads; this watches for writes and hangup.
            libc::pollfd {
                fd: if eof { -1 } else { pty.as_fd().as_raw_fd() },
                events: if output.is_empty() { 0 } else { libc::POLLOUT },
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
            // Commands, a received frame or the reader: look at its queue again.
            readable = true;
        }
        if polls[1].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
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
    fn search_capture_and_navigation_coalesce_independently_under_input_saturation() {
        let (session, commands) = Session::channel().unwrap();
        for _ in 0..64 {
            session.command(Command::Input(Arc::from(&b""[..]))).unwrap();
        }
        let mut terminal = Terminal::new(10, 2, 0);
        terminal.feed(b"needle");
        let results = Arc::new(
            hotaru_core::search::find(
                terminal.content_snapshot(),
                b"needle",
                Default::default(),
                &AtomicBool::new(false),
            )
            .unwrap(),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        for token in 0..100 {
            session.command(Command::Search(SearchControl::Capture { token, cancel: cancel.clone() })).unwrap();
            session
                .command(Command::Search(SearchControl::Navigate {
                    token: token + 100,
                    results: results.clone(),
                    index: 0,
                    cancel: cancel.clone(),
                }))
                .unwrap();
        }
        assert!(session.command(Command::Input(Arc::from(&b""[..]))).is_err());
        let controls = std::mem::take(&mut *session.shared.controls.lock().unwrap());
        assert!(matches!(controls.search_capture, Some(SearchControl::Capture { token: 99, .. })));
        assert!(matches!(controls.search_navigation, Some(SearchControl::Navigate { token: 199, .. })));
        assert_eq!(commands.try_iter().count(), 64);
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
        session.command(Command::ResizePixels { cols: 201, rows: 51, width: 2211, height: 1275 }).unwrap();
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
        let defaults = Defaults {
            colors: TerminalColors::default(),
            scrollback: 100,
            cursor: (hotaru_core::CursorShape::Block, true),
        };
        let worker = std::thread::spawn(move || run(1, child, defaults, sender, commands, shared));
        assert!(harness.settle(Duration::from_secs(5), |probe| probe.exited));
        let frame = harness.program().frame.as_ref().unwrap();
        assert_eq!((frame.cols, frame.rows), (201, 51));
        assert_eq!(frame.default_foreground, colors.foreground);
        assert_eq!(frame.default_background, colors.background);
        assert!(frame.plain_text().contains("KERNEL-SIZE-201x51-2211x1275"));
        drop(harness);
        worker.join().unwrap().unwrap();
    }

    const BULK_LINES: usize = 100_000;

    #[test]
    fn bulk_output_arrives_whole_before_the_pane_ends() {
        // Far more than the reader's queue holds, so the reader waits on the
        // session; the end is reported only after the last of it is parsed.
        let (session, commands) = Session::channel().unwrap();
        let shared = session.shared.clone();
        let mut harness = Harness::new(Probe { session, sender: None, frame: None, exited: false });
        let sender = harness.program().sender.clone().unwrap();
        let mut child = PtyCommand::new(std::env::current_exe().unwrap());
        child.args = ["--ignored", "--exact", "session::tests::bulk_output_child", "--nocapture", "--test-threads=1"]
            .into_iter()
            .map(Into::into)
            .collect();
        child.env.push(("HOTARU_BULK_OUTPUT_CHILD".into(), "1".into()));
        let defaults = Defaults {
            colors: TerminalColors::default(),
            scrollback: 100,
            cursor: (hotaru_core::CursorShape::Block, true),
        };
        let worker = std::thread::spawn(move || run(1, child, defaults, sender, commands, shared));
        assert!(harness.settle(Duration::from_secs(30), |probe| probe.exited));
        let text = harness.program().frame.as_ref().unwrap().plain_text();
        let mut at = 0;
        for line in (BULK_LINES - 5..BULK_LINES).map(|n| format!("bulk {n:07}")).chain(["BULK-END".into()]) {
            let found = text[at..].find(&line).unwrap_or_else(|| panic!("{line} missing or out of order:\n{text}"));
            at += found + line.len();
        }
        drop(harness);
        worker.join().unwrap().unwrap();
    }

    #[test]
    #[ignore]
    fn bulk_output_child() {
        use std::io::Write;
        if std::env::var_os("HOTARU_BULK_OUTPUT_CHILD").is_none() {
            return;
        }
        let mut out = std::io::stdout().lock();
        for n in 0..BULK_LINES {
            write!(out, "bulk {n:07}\r\n").unwrap();
        }
        write!(out, "BULK-END\r\n").unwrap();
        out.flush().unwrap();
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
            if size.ws_col == 201 && size.ws_row == 51 && size.ws_xpixel == 2211 && size.ws_ypixel == 1275 {
                println!("KERNEL-SIZE-201x51-2211x1275");
                return;
            }
            assert!(Instant::now() < deadline, "latest PTY resize was lost");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
