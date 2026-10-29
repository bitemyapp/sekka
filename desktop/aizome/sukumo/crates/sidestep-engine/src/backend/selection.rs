//! The clipboard on the render thread: a wl_data_device per seat.
//!
//! Offering: the main thread's contents become a wl_data_source offering
//! each MIME type plus a marker type naming this process, set as the
//! selection with the serial of the latest input event. Other clients'
//! requests are written from the event loop through non-blocking pipes, as
//! much as each takes per wakeup, so a slow reader never blocks this
//! thread.
//!
//! Types the program promised are offered like the rest; when a client
//! asks for one, its pipe waits here while the main thread asks the owner
//! for the data (`FromRender::ProvideSelection`), and is written when the
//! answer comes (`ToRender::SelectionData`).
//!
//! Receiving: when another client's selection arrives, its types, and its
//! text read ahead through a pipe serviced by the event loop, land in the
//! shared state `NSPasteboard` reads (see [`crate::clipboard`]). Selections
//! carrying our marker are our own and aren't read back. A read that takes
//! too long is abandoned, its pipe closed, so a client that never finishes
//! doesn't leave pipes behind. The drag pasteboard reads the offer of the
//! drag it holds the same way, while that offer is there to read;
//! [`super::dnd`] follows the drag itself.

use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceData, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferData, DataOfferHandler, DragOffer, receive};
use smithay_client_toolkit::data_device_manager::data_source::{CopyPasteSource, DataSourceData, DataSourceHandler};
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, ReadPipe, WritePipe};
use smithay_client_toolkit::globals::GlobalData;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{PostAction, RegistrationToken};
use smithay_client_toolkit::reexports::client::backend::ObjectData;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_data_device::{self, WlDataDevice};
use smithay_client_toolkit::reexports::client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use smithay_client_toolkit::reexports::client::protocol::wl_data_offer::WlDataOffer;
use smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource;
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle, delegate_dispatch};

use super::{State, dnd, drag_out, primary};
use crate::clipboard::{
    Contents, READ_AHEAD_LIMIT, READ_TIMEOUT, ReadAhead, Source, TRANSFER_TIMEOUT, owner_mime, shared, shared_for,
    text_mime,
};
use crate::protocol::FromRender;

/// How long a read ahead may go without data before it's abandoned. Longer
/// than the main thread waits: text that comes late is still kept for
/// later reads.
const READ_AHEAD_GIVE_UP: Duration = Duration::from_secs(5);
const PIPE_BYTE_BUDGET: usize = 256 * 1024;
const PIPE_SYSCALL_BUDGET: usize = 16;
const PIPE_CANCEL_INTERVAL: Duration = Duration::from_millis(50);

pub struct Selection {
    pub(super) manager: Option<DataDeviceManagerState>,
    pub(super) devices: Vec<(WlSeat, DataDevice)>,
    /// Our offer and what it holds.
    source: Option<(CopyPasteSource, Arc<Contents>)>,
    /// Contents to offer once there is an input serial to offer them with.
    pending: Option<Option<Arc<Contents>>>,
    /// We cleared the selection; the compositor's word of it is no news.
    cleared_by_us: bool,
    /// Clients' pipes waiting for data the main thread is making, by the
    /// token it answers under.
    promised: std::collections::HashMap<u64, WritePipe>,
    next_token: u64,
    /// Drag and drop under way.
    pub(super) dnd: dnd::Dnd,
    /// A drag out of one of our windows, under way.
    pub(super) out: Option<drag_out::Out>,
    /// The primary selection, beside the clipboard.
    pub(super) primary: primary::Primary,
}

impl Selection {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<State>) -> Self {
        Selection {
            manager: DataDeviceManagerState::bind(globals, qh).ok(),
            devices: Vec::new(),
            source: None,
            pending: None,
            cleared_by_us: false,
            promised: std::collections::HashMap::new(),
            next_token: 1,
            dnd: dnd::Dnd::default(),
            out: None,
            primary: primary::Primary::new(globals, qh),
        }
    }
}

pub(super) fn add_seat(state: &mut State, seat: &WlSeat) {
    if let Some(manager) = &state.selection.manager {
        let device = manager.get_data_device(&state.qh, seat);
        state.selection.devices.push((seat.clone(), device));
    }
    primary::add_seat(state, seat);
}

pub(super) fn remove_seat(state: &mut State, seat: &WlSeat) {
    state.selection.devices.retain(|(s, _)| s != seat);
    primary::remove_seat(state, seat);
}

/// A window got the keyboard: contents written before there was an input
/// serial can be offered now.
pub(super) fn focus_changed(state: &mut State) {
    if let Some(contents) = state.selection.pending.take() {
        set(state, contents);
    }
    primary::focus_changed(state);
}

/// Offer `contents` as the selection, or clear it.
pub(super) fn set(state: &mut State, contents: Option<Arc<Contents>>) {
    let Some(manager) = &state.selection.manager else { return };
    let latest = state.seats.latest_serial();
    let device = latest.as_ref().and_then(|(seat, _)| state.selection.devices.iter().find(|(s, _)| s == seat));
    let (Some((_, device)), Some((_, serial))) = (device, latest) else {
        state.selection.pending = Some(contents);
        return;
    };
    match contents {
        Some(contents) => {
            let mimes = contents.items.iter().map(|(m, _)| m.clone()).chain([owner_mime().to_owned()]);
            let source = manager.create_copy_paste_source(&state.qh, mimes);
            source.set_selection(device, serial);
            state.selection.source = Some((source, contents));
        }
        None => {
            device.unset_selection(serial);
            state.selection.source = None;
            state.selection.cleared_by_us = true;
        }
    }
}

/// Read the selection (or drag `drag`'s offer) as `mime` for the main
/// thread, answering under `token`.
pub(super) fn read(state: &mut State, mime: String, token: u64, source: Source, drag: u64) {
    let shared = shared_for(source);
    let Some((maximum, deadline)) = shared.read_options(token) else {
        shared.read_arrived(token, None);
        return;
    };
    let pipe = match source {
        Source::Selection => state
            .selection
            .devices
            .iter()
            .filter_map(|(_, d)| d.data().selection_offer())
            .find(|o| o.with_mime_types(|m| m.contains(&mime)))
            .and_then(|o| o.receive(mime).ok()),
        // That drag's offer, not whichever the pointer has now; none once
        // it's finished or gone.
        Source::Drag => dnd::offer_of(state, drag, &mime).and_then(|o| receive(&o, mime).ok()),
        Source::Primary => primary::receive(state, &mime),
    };
    let Some(pipe) = pipe else {
        shared.read_arrived(token, None);
        return;
    };
    // The main thread stops waiting after READ_TIMEOUT without data; a
    // little later the read is abandoned.
    let progress = move || shared.read_progressed(token);
    let policy = ReadPolicy {
        maximum,
        idle: READ_TIMEOUT * 2,
        deadline,
        active: std::rc::Rc::new(move || shared.read_options(token).is_some()),
    };
    read_pipe_bounded(state, pipe, policy, progress, move |read| {
        shared.read_arrived(token, if let PipeRead::Done(data) = read { Some(data) } else { None });
    });
}

/// The main thread made promised data for the pipe waiting under `token`.
pub(super) fn provided(state: &mut State, token: u64, data: Option<Arc<[u8]>>) {
    let Some(fd) = state.selection.promised.remove(&token) else { return };
    // Without data the pipe closes here, and the reader gets nothing.
    if let Some(data) = data {
        write_pipe(state, fd, data);
    }
}

/// Write `data` to a client's pipe from the event loop, as much as it
/// takes each time it can.
pub(super) fn write_pipe(state: &State, fd: WritePipe, data: Arc<[u8]>) {
    // A pipe that polls writable then takes what it has room for without
    // blocking.
    if rustix::io::ioctl_fionbio(&fd, true).is_err() {
        return;
    }
    let mut written = 0;
    let deadline = Instant::now() + TRANSFER_TIMEOUT;
    let timer: std::rc::Rc<std::cell::Cell<Option<RegistrationToken>>> = Default::default();
    let timer_of_write = timer.clone();
    let writing = state.loop_handle.insert_source(fd, move |(), file, state| {
        if Instant::now() < deadline && matches!(write_available(&mut &**file, &data, &mut written), Ok(false)) {
            return PostAction::Continue;
        }
        if let Some(timer) = timer_of_write.take() {
            state.loop_handle.remove(timer);
        }
        PostAction::Remove
    });
    let Ok(writing) = writing else {
        return;
    };
    let installed = state.loop_handle.insert_source(Timer::from_duration(TRANSFER_TIMEOUT), move |_, _, state| {
        state.loop_handle.remove(writing);
        TimeoutAction::Drop
    });
    match installed {
        Ok(token) => timer.set(Some(token)),
        // A stalled pipe must not survive when its cleanup timer cannot run.
        Err(_) => {
            state.loop_handle.remove(writing);
        }
    }
}

/// One readiness wake is finite even when a peer drains the pipe continuously.
fn write_available(writer: &mut impl Write, data: &[u8], written: &mut usize) -> std::io::Result<bool> {
    let mut budget = PIPE_BYTE_BUDGET;
    for _ in 0..PIPE_SYSCALL_BUDGET {
        if *written == data.len() {
            return Ok(true);
        }
        let end = data.len().min(*written + budget);
        match writer.write(&data[*written..end]) {
            Ok(0) => return Err(std::io::Error::new(ErrorKind::WriteZero, "clipboard pipe made no progress")),
            Ok(count) => {
                *written += count;
                budget -= count;
                if budget == 0 {
                    return Ok(*written == data.len());
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(*written == data.len())
}

/// How reading a pipe ended.
pub(super) enum PipeRead {
    Done(Vec<u8>),
    /// More than the limit came.
    TooLong,
    /// An error, or it took longer than it may.
    Failed,
}

/// Read a pipe to its end, `limit` bytes at most, from the event loop, and
/// hand over what came, telling `progress` whenever some arrives; after
/// `give_up` without any, the read is abandoned and the pipe closed.
pub(super) fn read_pipe(
    state: &State,
    pipe: ReadPipe,
    limit: usize,
    give_up: Duration,
    progress: impl FnMut() + 'static,
    done: impl FnOnce(PipeRead) + 'static,
) {
    read_pipe_bounded(
        state,
        pipe,
        ReadPolicy {
            maximum: limit,
            idle: give_up,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
            active: std::rc::Rc::new(|| true),
        },
        progress,
        done,
    );
}

struct ReadPolicy {
    maximum: usize,
    idle: Duration,
    deadline: Instant,
    active: std::rc::Rc<dyn Fn() -> bool>,
}

fn read_pipe_bounded(
    state: &State,
    pipe: ReadPipe,
    policy: ReadPolicy,
    mut progress: impl FnMut() + 'static,
    done: impl FnOnce(PipeRead) + 'static,
) {
    // A pipe that polls readable then gives what it has without blocking.
    if rustix::io::ioctl_fionbio(&pipe, true).is_err() {
        done(PipeRead::Failed);
        return;
    }
    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let finish = {
        let done = done.clone();
        move |read: PipeRead| {
            if let Some(done) = done.borrow_mut().take() {
                done(read);
            }
        }
    };
    // The timer and the pipe each end the other.
    let timer: std::rc::Rc<std::cell::Cell<Option<RegistrationToken>>> = Default::default();
    // When data last came.
    let active = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
    let mut data = Vec::new();
    let finish_read = finish.clone();
    let timer_of_read = timer.clone();
    let active_of_read = active.clone();
    let alive = policy.active.clone();
    let reading = state.loop_handle.insert_source(pipe, move |(), file, state| {
        let before = data.len();
        let ended = if Instant::now() >= policy.deadline || !alive() {
            Some(PipeRead::Failed)
        } else {
            read_available(&mut &**file, &mut data, policy.maximum)
        };
        let Some(read) = ended else {
            if data.len() > before {
                active_of_read.set(Instant::now());
                progress();
            }
            return PostAction::Continue;
        };
        if let Some(t) = timer_of_read.take() {
            state.loop_handle.remove(t);
        }
        finish_read(read);
        PostAction::Remove
    });
    let Ok(reading) = reading else {
        finish(PipeRead::Failed);
        return;
    };
    let first = PIPE_CANCEL_INTERVAL.min(policy.idle).min(policy.deadline.saturating_duration_since(Instant::now()));
    let token = state.loop_handle.insert_source(Timer::from_duration(first), move |_, _, state| {
        let now = Instant::now();
        let idle_until = (active.get() + policy.idle).min(policy.deadline);
        if now < idle_until && (policy.active)() {
            return TimeoutAction::ToInstant(idle_until.min(now + PIPE_CANCEL_INTERVAL));
        }
        state.loop_handle.remove(reading);
        finish(PipeRead::Failed);
        TimeoutAction::Drop
    });
    match token {
        Ok(token) => timer.set(Some(token)),
        Err(_) => {
            state.loop_handle.remove(reading);
            // The timer closure took its own finish clone; this one still
            // reports failure exactly once when timer installation fails.
            if let Some(done) = done.borrow_mut().take() {
                done(PipeRead::Failed);
            }
        }
    }
}

fn read_available(reader: &mut impl Read, data: &mut Vec<u8>, limit: usize) -> Option<PipeRead> {
    let mut chunk = [0_u8; 65536];
    let mut budget = PIPE_BYTE_BUDGET;
    for _ in 0..PIPE_SYSCALL_BUDGET {
        let count = chunk.len().min(budget).min(limit.saturating_sub(data.len()).saturating_add(1));
        match reader.read(&mut chunk[..count]) {
            Ok(0) => return Some(PipeRead::Done(std::mem::take(data))),
            Ok(count) if count > limit.saturating_sub(data.len()) => return Some(PipeRead::TooLong),
            Ok(count) => {
                if data.try_reserve(count).is_err() {
                    return Some(PipeRead::Failed);
                }
                data.extend_from_slice(&chunk[..count]);
                budget -= count;
                if budget == 0 {
                    return None;
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => return None,
            Err(_) => return Some(PipeRead::Failed),
        }
    }
    None
}

impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        device: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &WlSurface,
    ) {
        dnd::enter(self, device, x, y, surface);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice) {
        dnd::leave(self, device);
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice, x: f64, y: f64) {
        dnd::motion(self, device, x, y);
    }

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice) {
        dnd::dropped(self, device);
    }

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice) {
        let Some(offer) = device.data::<DataDeviceData>().and_then(|d| d.selection_offer()) else { return };
        let mimes = offer.with_mime_types(|m| m.to_vec());
        if mimes.iter().any(|m| m == owner_mime()) {
            // Our own offer, which the main thread already holds.
            return;
        }
        let text = text_mime(&mimes);
        let Some(read) = shared().foreign_offered(mimes, text.is_some()) else { return };
        let Some(pipe) = text.and_then(|mime| offer.receive(mime).ok()) else {
            shared().text_arrived(read, ReadAhead::Failed);
            return;
        };
        read_pipe(
            self,
            pipe,
            READ_AHEAD_LIMIT,
            READ_AHEAD_GIVE_UP,
            || {},
            move |outcome| {
                let outcome = match outcome {
                    PipeRead::Done(data) => ReadAhead::Text(Some(String::from_utf8_lossy(&data).into_owned())),
                    PipeRead::TooLong => ReadAhead::TooLong,
                    PipeRead::Failed => ReadAhead::Failed,
                };
                shared().text_arrived(read, outcome);
            },
        );
    }
}

impl DataOfferHandler for State {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, offer: &mut DragOffer, actions: DndAction) {
        dnd::source_actions(self, offer, actions);
    }

    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, offer: &mut DragOffer, action: DndAction) {
        dnd::selected_action(self, offer, action);
    }
}

impl DataSourceHandler for State {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>) {}

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &WlDataSource,
        mime: String,
        fd: WritePipe,
    ) {
        // The drag out's, or the clipboard's.
        let (drag, data) = match drag_out::data(self, source, &mime) {
            Some((drag, data)) => (Some(drag), data),
            None => (
                None,
                self.selection.source.as_ref().filter(|(s, _)| s.inner() == source).and_then(|(_, c)| c.data(&mime)),
            ),
        };
        match data {
            // Without data the pipe closes here, and the reader gets nothing.
            None => {}
            Some(Some(data)) => write_pipe(self, fd, data),
            // Promised: the main thread makes it.
            Some(None) => {
                let token = self.selection.next_token;
                self.selection.next_token += 1;
                self.selection.promised.insert(token, fd);
                match drag {
                    Some(drag) => self.send(FromRender::ProvideDragData { drag, mime, token }),
                    None => self.send(FromRender::ProvideSelection { mime, token }),
                }
            }
        }
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        if drag_out::end(self, Some(source), 0) {
            return;
        }
        if self.selection.source.as_ref().is_some_and(|(s, _)| s.inner() == source) {
            self.selection.source = None;
        }
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        drag_out::end(self, Some(source), 1);
    }

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource, action: DndAction) {
        drag_out::action(self, source, action);
    }
}

/// The data device's events go through smithay-client-toolkit, which
/// doesn't report a cleared selection; this notices those first.
impl Dispatch<WlDataDevice, DataDeviceData> for State {
    fn event(
        state: &mut State,
        device: &WlDataDevice,
        event: wl_data_device::Event,
        data: &DataDeviceData,
        conn: &Connection,
        qh: &QueueHandle<State>,
    ) {
        let cleared = matches!(event, wl_data_device::Event::Selection { id: None });
        <DataDeviceManagerState as Dispatch<WlDataDevice, DataDeviceData, State>>::event(
            state, device, event, data, conn, qh,
        );
        if cleared && !std::mem::take(&mut state.selection.cleared_by_us) {
            shared().foreign_offered(Vec::new(), false);
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<State>) -> Arc<dyn ObjectData> {
        <DataDeviceManagerState as Dispatch<WlDataDevice, DataDeviceData, State>>::event_created_child(opcode, qh)
    }
}

delegate_dispatch!(State: [WlDataDeviceManager: GlobalData] => DataDeviceManagerState);
delegate_dispatch!(State: [WlDataOffer: DataOfferData] => DataDeviceManagerState);
delegate_dispatch!(State: [WlDataSource: DataSourceData] => DataDeviceManagerState);

#[cfg(test)]
mod pipe_tests {
    use super::*;

    #[test]
    fn continuously_ready_writer_yields_at_byte_budget() {
        let data = vec![17_u8; 4 * 1024 * 1024 + 31];
        let mut sink = Vec::new();
        let mut written = 0;
        assert!(!write_available(&mut sink, &data, &mut written).unwrap());
        assert_eq!(written, PIPE_BYTE_BUDGET);
        while !write_available(&mut sink, &data, &mut written).unwrap() {}
        assert_eq!(sink, data);
    }

    struct FailingWriter {
        kind: ErrorKind,
        calls: usize,
    }
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.kind == ErrorKind::WriteZero { Ok(0) } else { Err(std::io::Error::from(self.kind)) }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn interrupted_writer_yields_and_zero_or_broken_pipe_finishes() {
        let mut writer = FailingWriter { kind: ErrorKind::Interrupted, calls: 0 };
        assert!(!write_available(&mut writer, b"x", &mut 0).unwrap());
        assert_eq!(writer.calls, PIPE_SYSCALL_BUDGET);
        for kind in [ErrorKind::BrokenPipe, ErrorKind::WriteZero] {
            let mut writer = FailingWriter { kind, calls: 0 };
            assert_eq!(write_available(&mut writer, b"x", &mut 0).unwrap_err().kind(), kind);
            assert_eq!(writer.calls, 1);
        }
        let mut writer = FailingWriter { kind: ErrorKind::WouldBlock, calls: 0 };
        assert!(!write_available(&mut writer, b"x", &mut 0).unwrap());
        assert_eq!(writer.calls, 1);
    }

    #[test]
    fn continuously_ready_reader_yields_and_preserves_exact_bytes() {
        let source = vec![19_u8; 4 * 1024 * 1024 + 7];
        let mut reader = std::io::Cursor::new(&source);
        let mut data = Vec::new();
        assert!(read_available(&mut reader, &mut data, source.len()).is_none());
        assert_eq!(data.len(), PIPE_BYTE_BUDGET);
        loop {
            match read_available(&mut reader, &mut data, source.len()) {
                None => {}
                Some(PipeRead::Done(bytes)) => {
                    assert_eq!(bytes, source);
                    break;
                }
                Some(_) => panic!("valid selection failed"),
            }
        }
    }

    #[test]
    fn reader_limit_distinguishes_exact_size_from_one_byte_more() {
        let mut data = Vec::new();
        assert!(
            matches!(read_available(&mut &b"abc"[..], &mut data, 3), Some(PipeRead::Done(bytes)) if bytes == b"abc")
        );
        assert!(matches!(read_available(&mut &b"abcd"[..], &mut data, 3), Some(PipeRead::TooLong)));
    }
}
