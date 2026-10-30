//! Isolated transport ownership prototype. No production backend integration.
#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    marker::PhantomData,
    os::fd::OwnedFd,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    thread,
};

const RENDERING: u8 = 0;
const READY: u8 = 1;
const IMPORTING: u8 = 2;
const COMMITTED: u8 = 3;
const RECLAIM: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameId(u64);

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub windows: usize,
    pub leases: usize,
    pub leases_per_window: usize,
    pub bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            windows: 32,
            leases: 24,
            leases_per_window: 3,
            bytes: 128 << 20,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Limit,
    Closed,
    Stale,
    Exhausted,
    DriverFailed,
    InvalidDescriptor,
}

/// Complete immutable frame identity; the driver keeps the source until release.
pub struct Frame<F> {
    pub window: WindowId,
    pub id: FrameId,
    pub revision: u64,
    pub source: Arc<F>,
    pub source_bytes: usize,
    pub buffer_bytes: usize,
}
impl<F> Frame<F> {
    fn charge(&self) -> usize {
        self.source_bytes + self.buffer_bytes
    }
}

/// One-plane 32-bit export. Only OwnedFd, never an EGL handle, crosses threads.
pub struct Descriptor {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub offset: u32,
    pub fourcc: u32,
    pub modifier: u64,
}
impl Descriptor {
    fn fits(&self, reserved: usize) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= i32::MAX as u32
            && self.height <= i32::MAX as u32
            && self.width.checked_mul(4).is_some_and(|n| n <= self.stride)
            && (self.stride as usize)
                .checked_mul(self.height as usize)
                .and_then(|n| n.checked_add(self.offset as usize))
                .is_some_and(|n| n <= reserved)
    }
}

/// Constructed and used entirely on the worker. Neither Driver nor Resource needs Send.
#[derive(Debug)]
pub enum RenderError {
    Cancelled,
    Failed(String),
}
impl From<String> for RenderError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

pub trait Driver<F> {
    type Resource;
    fn render(
        &mut self,
        frame: &Frame<F>,
        cancelled: &Cancellation,
    ) -> Result<(Self::Resource, Descriptor), RenderError>;
}
pub struct Cancellation(Arc<Control>);
impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.0.phase.load(Ordering::Acquire) == RECLAIM
    }
}
struct Control {
    window: WindowId,
    frame: FrameId,
    revision: u64,
    charge: usize,
    phase: AtomicU8,
}
struct Window<F> {
    id: WindowId,
    open: bool,
    revision: u64,
    last_committed: Option<FrameId>,
    pending: Option<Frame<F>>,
}
struct Ready {
    control: Arc<Control>,
    descriptor: Descriptor,
}
struct State<F> {
    windows: Vec<Window<F>>,
    controls: Vec<Arc<Control>>,
    ready: VecDeque<Ready>,
    bytes: usize,
    next_id: u64,
    stopping: bool,
    failed: bool,
    round_robin: usize,
}
struct Shared<F> {
    state: Mutex<State<F>>,
    wake: Condvar,
    limits: Limits,
}
impl<F> Shared<F> {
    fn notify(&self) {
        self.wake.notify_one();
    }
}

/// Single Wayland/event-thread endpoint. Drop cancels without joining the worker.
pub struct EventOwner<F> {
    shared: Arc<Shared<F>>,
    _event_thread: PhantomData<Rc<()>>,
}
/// Import-pending transfer. Dropping it cancels an uncommitted lease.
pub struct Import<F> {
    shared: Arc<Shared<F>>,
    control: Arc<Control>,
    descriptor: Option<Descriptor>,
    _event_thread: PhantomData<Rc<()>>,
}
impl<F> Import<F> {
    pub fn frame(&self) -> FrameId {
        self.control.frame
    }
    pub fn window(&self) -> WindowId {
        self.control.window
    }
    pub fn descriptor(&self) -> &Descriptor {
        self.descriptor.as_ref().expect("live import")
    }
}
impl<F> Drop for Import<F> {
    fn drop(&mut self) {
        let state = self.shared.state.lock().unwrap();
        let cancelled = self
            .control
            .phase
            .compare_exchange(IMPORTING, RECLAIM, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        drop(state);
        if cancelled {
            self.shared.notify();
        }
    }
}
/// Must remain in the adapter's wl_buffer table until release or disconnection.
/// Drop deliberately does not imply release: frame callbacks and window teardown
/// cannot prove the compositor finished reading it.
pub struct Committed {
    control: Arc<Control>,
    _event_thread: PhantomData<Rc<()>>,
}
impl Committed {
    pub fn frame(&self) -> FrameId {
        self.control.frame
    }
    pub fn window(&self) -> WindowId {
        self.control.window
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub windows: usize,
    pub leases: usize,
    pub pending: usize,
    pub bytes: usize,
    pub failed: bool,
}

pub fn spawn<F, D, Make>(limits: Limits, make: Make) -> std::io::Result<EventOwner<F>>
where
    F: Send + Sync + 'static,
    D: Driver<F> + 'static,
    Make: FnOnce() -> Result<D, String> + Send + 'static,
{
    spawn_with_wake(limits, make, || {})
}

/// The callback must only signal a coalesced event-loop wake. It executes on the
/// worker, outside metadata locks. A panic disables rendering without unwinding leases.
pub fn spawn_with_wake<F, D, Make, Wake>(
    limits: Limits,
    make: Make,
    wake: Wake,
) -> std::io::Result<EventOwner<F>>
where
    F: Send + Sync + 'static,
    D: Driver<F> + 'static,
    Make: FnOnce() -> Result<D, String> + Send + 'static,
    Wake: Fn() + Send + 'static,
{
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            windows: Vec::new(),
            controls: Vec::new(),
            ready: VecDeque::new(),
            bytes: 0,
            next_id: 1,
            stopping: false,
            failed: false,
            round_robin: 0,
        }),
        wake: Condvar::new(),
        limits,
    });
    let worker_shared = shared.clone();
    thread::Builder::new()
        .name("hotaru-gpu-prototype".into())
        .spawn(move || worker(worker_shared, make, wake))?;
    Ok(EventOwner {
        shared,
        _event_thread: PhantomData,
    })
}

impl<F> EventOwner<F> {
    pub fn open(&mut self) -> Result<WindowId, Error> {
        let mut state = self.shared.state.lock().unwrap();
        if state.stopping {
            return Err(Error::Closed);
        }
        if state.failed {
            return Err(Error::DriverFailed);
        }
        if state.windows.len() >= self.shared.limits.windows {
            return Err(Error::Limit);
        }
        let id = WindowId(next_id(&mut state)?);
        state.windows.push(Window {
            id,
            open: true,
            revision: id.0,
            last_committed: None,
            pending: None,
        });
        Ok(id)
    }

    /// Replace only this window's pending full frame. Old source drops outside mutex.
    pub fn submit(
        &mut self,
        window: WindowId,
        source: Arc<F>,
        source_bytes: usize,
        buffer_bytes: usize,
    ) -> Result<FrameId, Error> {
        let total = source_bytes.checked_add(buffer_bytes).ok_or(Error::Limit)?;
        if buffer_bytes == 0 || total > self.shared.limits.bytes {
            return Err(Error::Limit);
        }
        let mut state = self.shared.state.lock().unwrap();
        if state.stopping {
            return Err(Error::Closed);
        }
        if state.failed {
            return Err(Error::DriverFailed);
        }
        let index = state
            .windows
            .iter()
            .position(|w| w.id == window && w.open)
            .ok_or(Error::Closed)?;
        let old_charge = state.windows[index]
            .pending
            .as_ref()
            .map_or(0, Frame::charge);
        let bytes = state.bytes - old_charge;
        let bytes = bytes
            .checked_add(total)
            .filter(|&n| n <= self.shared.limits.bytes)
            .ok_or(Error::Limit)?;
        let id = FrameId(next_id(&mut state)?);
        let revision = state.windows[index].revision;
        let old = state.windows[index].pending.replace(Frame {
            window,
            id,
            revision,
            source,
            source_bytes,
            buffer_bytes,
        });
        state.bytes = bytes;
        drop(state);
        drop(old);
        self.shared.notify();
        Ok(id)
    }

    /// Bounded non-driver polling; calloop's coalesced wake will trigger this.
    pub fn poll_ready(&mut self) -> Option<Import<F>> {
        loop {
            let ready = self.shared.state.lock().unwrap().ready.pop_front()?;
            if ready
                .control
                .phase
                .compare_exchange(READY, IMPORTING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(Import {
                    shared: self.shared.clone(),
                    control: ready.control,
                    descriptor: Some(ready.descriptor),
                    _event_thread: PhantomData,
                });
            }
            // Stale FD closes outside the shared metadata mutex.
            drop(ready);
        }
    }

    /// Call after params.created and immediately before attach/commit. Rejects stale jobs.
    pub fn commit(&mut self, mut import: Import<F>) -> Result<Committed, Error> {
        if !Arc::ptr_eq(&self.shared, &import.shared) {
            return Err(Error::Stale);
        }
        let mut state = self.shared.state.lock().unwrap();
        let current = state.windows.iter_mut().find(|w| {
            w.id == import.window()
                && w.open
                && w.revision == import.control.revision
                && w.last_committed.is_none_or(|id| id.0 < import.frame().0)
        });
        if current.is_none()
            || import
                .control
                .phase
                .compare_exchange(IMPORTING, COMMITTED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(Error::Stale);
        }
        current.unwrap().last_committed = Some(import.frame());
        let lease = Committed {
            control: import.control.clone(),
            _event_thread: PhantomData,
        };
        drop(state);
        // Protocol already received a duplicated FD; original export remains worker-owned.
        drop(import.descriptor.take());
        Ok(lease)
    }

    pub fn released(&mut self, lease: Committed) {
        let state = self.shared.state.lock().unwrap();
        let ours = state
            .controls
            .iter()
            .any(|c| Arc::ptr_eq(c, &lease.control));
        if ours {
            lease.control.phase.store(RECLAIM, Ordering::Release);
        }
        drop(state);
        if ours {
            self.shared.notify();
        }
    }

    /// Geometry/scale/scene replacement invalidates old uncommitted work. Ordinary
    /// incremental full-frame updates do not, so a busy producer cannot starve display.
    pub fn invalidate(&mut self, window: WindowId) -> Result<(), Error> {
        let mut state = self.shared.state.lock().unwrap();
        let index = state
            .windows
            .iter()
            .position(|w| w.id == window && w.open)
            .ok_or(Error::Closed)?;
        let revision = next_id(&mut state)?;
        state.windows[index].revision = revision;
        let pending = state.windows[index].pending.take();
        if let Some(frame) = &pending {
            state.bytes -= frame.charge();
        }
        for control in &state.controls {
            if control.window == window {
                cancel_uncommitted(control);
            }
        }
        drop(state);
        drop(pending);
        self.shared.notify();
        Ok(())
    }

    pub fn close(&mut self, window: WindowId) {
        let mut state = self.shared.state.lock().unwrap();
        let pending = state
            .windows
            .iter_mut()
            .find(|w| w.id == window)
            .and_then(|w| {
                w.open = false;
                w.pending.take()
            });
        if let Some(pending) = &pending {
            state.bytes -= pending.charge();
        }
        for control in &state.controls {
            if control.window == window {
                cancel_uncommitted(control);
            }
        }
        prune_windows(&mut state);
        drop(state);
        drop(pending);
        self.shared.notify();
    }

    /// Adapter must first end its Wayland connection. Stop all future allocation
    /// and mutation, then destroy our references; kernel/server references may
    /// outlive us. This is teardown, never permission to reuse a committed buffer.
    /// This method never joins.
    pub fn connection_lost(&mut self) {
        stop(&self.shared, true);
    }

    pub fn stats(&self) -> Stats {
        let state = self.shared.state.lock().unwrap();
        Stats {
            windows: state.windows.len(),
            leases: state.controls.len(),
            pending: state.windows.iter().filter(|w| w.pending.is_some()).count(),
            bytes: state.bytes,
            failed: state.failed,
        }
    }
}
impl<F> Drop for EventOwner<F> {
    fn drop(&mut self) {
        stop(&self.shared, false);
    }
}
fn next_id<F>(state: &mut State<F>) -> Result<u64, Error> {
    let id = state.next_id;
    state.next_id = id.checked_add(1).ok_or(Error::Exhausted)?;
    Ok(id)
}
fn cancel_uncommitted(control: &Control) {
    let mut phase = control.phase.load(Ordering::Acquire);
    while phase < COMMITTED {
        match control.phase.compare_exchange_weak(
            phase,
            RECLAIM,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(new) => phase = new,
        }
    }
}
fn prune_windows<F>(state: &mut State<F>) {
    state
        .windows
        .retain(|w| w.open || state.controls.iter().any(|c| c.window == w.id));
}
fn stop<F>(shared: &Shared<F>, connection_gone: bool) {
    let mut state = shared.state.lock().unwrap();
    state.stopping = true;
    let mut pending = Vec::new();
    for window in &mut state.windows {
        window.open = false;
        if let Some(frame) = window.pending.take() {
            pending.push(frame);
        }
    }
    for frame in &pending {
        state.bytes -= frame.charge();
    }
    for control in &state.controls {
        if connection_gone {
            control.phase.store(RECLAIM, Ordering::Release);
        } else {
            cancel_uncommitted(control);
        }
    }
    prune_windows(&mut state);
    drop(state);
    for frame in pending {
        let _ = drop_guarded(frame);
    }
    shared.notify();
}

fn contain<T>(result: std::thread::Result<T>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(payload) => {
            // A panic payload may itself panic on Drop. Do not let exceptional
            // cleanup unwind unrelated committed resources. Rendering stops after
            // this failure; leaked payload count is bounded by outstanding work.
            std::mem::forget(payload);
            None
        }
    }
}
fn drop_guarded<T>(value: T) -> bool {
    contain(catch_unwind(AssertUnwindSafe(|| drop(value)))).is_some()
}

struct Held<F, R> {
    control: Arc<Control>,
    _source: Arc<F>,
    _resource: R,
}
fn worker<F, D, Make, Wake>(shared: Arc<Shared<F>>, make: Make, wake: Wake)
where
    F: Send + Sync + 'static,
    D: Driver<F> + 'static,
    Make: FnOnce() -> Result<D, String>,
    Wake: Fn(),
{
    let mut driver = match contain(catch_unwind(AssertUnwindSafe(make))) {
        Some(Ok(driver)) => Some(driver),
        _ => None,
    };
    let mut failed = driver.is_none();
    let mut held: Vec<Held<F, D::Resource>> = Vec::new();
    loop {
        // Driver destructors and source destruction occur outside the metadata lock.
        let mut index = 0;
        while index < held.len() {
            if held[index].control.phase.load(Ordering::Acquire) == RECLAIM {
                let dead = held.swap_remove(index);
                let control = dead.control.clone();
                let stale_ready = {
                    let mut state = shared.state.lock().unwrap();
                    state
                        .ready
                        .iter()
                        .position(|r| Arc::ptr_eq(&r.control, &control))
                        .and_then(|i| state.ready.remove(i))
                };
                drop(stale_ready);
                let Held {
                    control: _,
                    _source: source,
                    _resource: resource,
                } = dead;
                // Guard separately: a failing destructor must not unwind held
                // committed resources or cause another field to panic while unwinding.
                failed |= !drop_guarded(resource);
                failed |= !drop_guarded(source);
                let mut state = shared.state.lock().unwrap();
                state.controls.retain(|c| !Arc::ptr_eq(c, &control));
                state.bytes -= control.charge;
                prune_windows(&mut state);
            } else {
                index += 1;
            }
        }
        let mut state = shared.state.lock().unwrap();
        if failed && !state.failed {
            state.failed = true;
            drop(state);
            stop(&shared, false);
            let _ = contain(catch_unwind(AssertUnwindSafe(&wake)));
            continue;
        }
        if state.stopping && state.controls.is_empty() {
            break;
        }
        let mut selected = None;
        if !state.stopping && state.controls.len() < shared.limits.leases {
            let len = state.windows.len();
            for n in 0..len {
                let index = (state.round_robin + n) % len;
                let w = &state.windows[index];
                if w.pending.is_some() {
                    let count = state.controls.iter().filter(|c| c.window == w.id).count();
                    if count < shared.limits.leases_per_window {
                        selected = Some(index);
                        break;
                    }
                }
            }
        }
        if let Some(index) = selected {
            let frame = state.windows[index].pending.take().unwrap();
            state.round_robin = index + 1;
            let control = Arc::new(Control {
                window: frame.window,
                frame: frame.id,
                revision: frame.revision,
                charge: frame.charge(),
                phase: AtomicU8::new(RENDERING),
            });
            state.controls.push(control.clone());
            drop(state);
            let rendered = catch_unwind(AssertUnwindSafe(|| {
                driver
                    .as_mut()
                    .unwrap()
                    .render(&frame, &Cancellation(control.clone()))
            }));
            let mut published = false;
            match contain(rendered) {
                Some(Ok((resource, descriptor))) => {
                    if descriptor.fits(frame.buffer_bytes) {
                        held.push(Held {
                            control: control.clone(),
                            _source: frame.source.clone(),
                            _resource: resource,
                        });
                        let mut state = shared.state.lock().unwrap();
                        if control
                            .phase
                            .compare_exchange(RENDERING, READY, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                        {
                            state.ready.push_back(Ready {
                                control: control.clone(),
                                descriptor,
                            });
                        } else {
                            drop(state);
                            drop(descriptor);
                        }
                        published = true;
                    } else {
                        let _ = drop_guarded(resource);
                        drop(descriptor);
                        failed = true;
                    }
                }
                Some(Err(RenderError::Cancelled))
                    if control.phase.load(Ordering::Acquire) == RECLAIM => {}
                _ => {
                    failed = true;
                }
            }
            if !published {
                control.phase.store(RECLAIM, Ordering::Release);
                let mut state = shared.state.lock().unwrap();
                state.controls.retain(|c| !Arc::ptr_eq(c, &control));
                state.bytes -= control.charge;
            }
            failed |= !drop_guarded(frame);
            if published {
                failed |= contain(catch_unwind(AssertUnwindSafe(&wake))).is_none();
            }
            continue;
        }
        // Recheck release while holding the predicate mutex to avoid a lost wake.
        if state
            .controls
            .iter()
            .any(|c| c.phase.load(Ordering::Acquire) == RECLAIM)
        {
            continue;
        }
        drop(shared.wake.wait(state).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{self, Read},
        os::unix::net::UnixStream,
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct Source {
        id: u64,
        block: bool,
        panic: bool,
        invalid: bool,
        drop_panic: bool,
        source_drop_panic: bool,
        payload_drop_panic: bool,
        respect_cancel: bool,
    }
    impl Drop for Source {
        fn drop(&mut self) {
            assert!(!self.source_drop_panic, "simulated source drop panic");
        }
    }
    struct PanickingPayload;
    impl Drop for PanickingPayload {
        fn drop(&mut self) {
            panic!("panic payload destructor");
        }
    }
    type Log = Arc<Mutex<Vec<(u64, thread::ThreadId)>>>;
    struct Resource {
        id: u64,
        drop_panic: bool,
        payload_drop_panic: bool,
        log: Log,
        _fd: UnixStream,
        _not_send: Rc<()>,
    }
    impl Drop for Resource {
        fn drop(&mut self) {
            self.log
                .lock()
                .unwrap()
                .push((self.id, thread::current().id()));
            if self.payload_drop_panic {
                std::panic::panic_any(PanickingPayload);
            }
            assert!(!self.drop_panic, "simulated resource drop panic");
        }
    }
    struct Mock {
        started: mpsc::Sender<u64>,
        unblock: mpsc::Receiver<()>,
        log: Log,
        context_dropped: mpsc::Sender<thread::ThreadId>,
        _not_send: Rc<()>,
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            let _ = self.context_dropped.send(thread::current().id());
        }
    }
    impl Driver<Source> for Mock {
        type Resource = Resource;
        fn render(
            &mut self,
            frame: &Frame<Source>,
            cancelled: &Cancellation,
        ) -> Result<(Resource, Descriptor), RenderError> {
            self.started.send(frame.source.id).unwrap();
            if frame.source.block {
                self.unblock.recv().unwrap();
            }
            if frame.source.respect_cancel && cancelled.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            assert!(!frame.source.panic, "simulated driver panic");
            let (reader, writer) = UnixStream::pair().unwrap();
            let descriptor = Descriptor {
                fd: reader.into(),
                width: 2,
                height: 2,
                stride: if frame.source.invalid { 7 } else { 8 },
                offset: 0,
                fourcc: u32::from_le_bytes(*b"AB24"),
                modifier: 0,
            };
            Ok((
                Resource {
                    id: frame.source.id,
                    drop_panic: frame.source.drop_panic,
                    payload_drop_panic: frame.source.payload_drop_panic,
                    log: self.log.clone(),
                    _fd: writer,
                    _not_send: Rc::new(()),
                },
                descriptor,
            ))
        }
    }
    struct Fixture {
        owner: EventOwner<Source>,
        started: mpsc::Receiver<u64>,
        unblock: mpsc::Sender<()>,
        log: Log,
        dropped: mpsc::Receiver<thread::ThreadId>,
    }
    fn fixture(limits: Limits) -> Fixture {
        let (started_tx, started) = mpsc::channel();
        let (unblock, unblock_rx) = mpsc::channel();
        let (dropped_tx, dropped) = mpsc::channel();
        let log = Arc::new(Mutex::new(Vec::new()));
        let worker_log = log.clone();
        let owner = spawn(limits, move || {
            Ok(Mock {
                started: started_tx,
                unblock: unblock_rx,
                log: worker_log,
                context_dropped: dropped_tx,
                _not_send: Rc::new(()),
            })
        })
        .unwrap();
        Fixture {
            owner,
            started,
            unblock,
            log,
            dropped,
        }
    }
    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "worker did not make progress");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn ready(owner: &mut EventOwner<Source>) -> Import<Source> {
        let mut result = None;
        until(|| {
            result = owner.poll_ready();
            result.is_some()
        });
        result.unwrap()
    }
    fn source(id: u64) -> Arc<Source> {
        Arc::new(Source {
            id,
            ..Source::default()
        })
    }

    #[test]
    fn blocked_driver_never_holds_event_lock_or_blocks_close_and_drop() {
        let (completed_tx, completed) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut f = fixture(Limits {
                bytes: 1024,
                ..Limits::default()
            });
            let w = f.owner.open().unwrap();
            f.owner
                .submit(
                    w,
                    Arc::new(Source {
                        block: true,
                        ..Source::default()
                    }),
                    8,
                    16,
                )
                .unwrap();
            f.started.recv_timeout(Duration::from_secs(3)).unwrap();
            // Model a clipboard/event counter while the driver remains blocked.
            for id in 1..1001 {
                f.owner.submit(w, source(id), 8, 16).unwrap();
                assert_eq!(f.owner.stats().pending, 1);
            }
            assert_eq!(f.owner.stats().bytes, 48); // active 24 + full pending reservation 24
            f.owner.close(w);
            assert_eq!(f.owner.stats().bytes, 24);
            drop(f.owner);
            completed_tx
                .send((f.unblock, f.dropped, thread::current().id()))
                .unwrap();
        });
        let (unblock, dropped, owner_thread) = completed
            .recv_timeout(Duration::from_secs(3))
            .expect("event owner blocked on driver");
        unblock.send(()).unwrap();
        assert_ne!(
            dropped.recv_timeout(Duration::from_secs(3)).unwrap(),
            owner_thread
        );
        handle.join().unwrap();
    }

    #[test]
    fn committed_lease_survives_close_and_counts_against_global_budget() {
        let mut f = fixture(Limits {
            windows: 2,
            leases: 1,
            leases_per_window: 3,
            bytes: 48,
        });
        let w = f.owner.open().unwrap();
        let a = source(1);
        let weak = Arc::downgrade(&a);
        f.owner.submit(w, a, 8, 16).unwrap();
        let import = ready(&mut f.owner);
        let mut reader = UnixStream::from(import.descriptor().fd.try_clone().unwrap());
        reader.set_nonblocking(true).unwrap();
        let lease = f.owner.commit(import).unwrap();
        f.owner.close(w);
        let w2 = f.owner.open().unwrap();
        assert_ne!(w, w2);
        f.owner.submit(w2, source(2), 8, 16).unwrap();
        assert_eq!(f.owner.stats().bytes, 48);
        assert!(weak.upgrade().is_some());
        assert_eq!(
            reader.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(f.log.lock().unwrap().is_empty());
        // A frame callback has no release method or token. Only this release frees capacity.
        f.owner.released(lease);
        let next = ready(&mut f.owner);
        assert_eq!(next.window(), w2);
        until(|| weak.upgrade().is_none());
        assert_eq!(reader.read(&mut [0]).unwrap(), 0);
        drop(next);
        until(|| f.owner.stats().leases == 0);
        assert_eq!(f.owner.stats().bytes, 0);
        f.owner.connection_lost();
    }

    #[test]
    fn newer_frame_rejects_import_and_keeps_exact_source_identity() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        let first = f.owner.submit(w, source(1), 8, 16).unwrap();
        let old = ready(&mut f.owner);
        assert_eq!(old.frame(), first);
        f.owner.invalidate(w).unwrap();
        let next = f.owner.submit(w, source(2), 8, 16).unwrap();
        assert!(matches!(f.owner.commit(old), Err(Error::Stale)));
        let import = ready(&mut f.owner);
        assert_eq!(import.frame(), next);
        let lease = f.owner.commit(import).unwrap();
        assert_eq!(f.started.recv().unwrap(), 1);
        assert_eq!(f.started.recv().unwrap(), 2);
        f.owner.released(lease);
        f.owner.connection_lost();
    }

    #[test]
    fn discarded_ready_descriptors_do_not_accumulate_without_event_polling() {
        let mut f = fixture(Limits {
            leases: 2,
            ..Limits::default()
        });
        let w = f.owner.open().unwrap();
        for id in 0..200 {
            f.owner.invalidate(w).unwrap();
            f.owner.submit(w, source(id), 8, 16).unwrap();
            until(|| {
                f.owner
                    .shared
                    .state
                    .lock()
                    .unwrap()
                    .ready
                    .iter()
                    .any(|r| r.control.phase.load(Ordering::Acquire) == READY)
            });
            assert!(f.owner.shared.state.lock().unwrap().ready.len() <= 2);
        }
        f.owner.close(w);
        until(|| f.owner.stats().leases == 0);
        assert!(f.owner.shared.state.lock().unwrap().ready.is_empty());
    }

    #[test]
    fn import_drop_cancellation_wakes_an_idle_worker() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        for id in 0..200 {
            f.owner.submit(w, source(id), 8, 16).unwrap();
            drop(ready(&mut f.owner));
            until(|| f.owner.stats().leases == 0);
            assert_eq!(f.owner.stats().bytes, 0);
        }
    }

    #[test]
    fn driver_panic_does_not_destroy_context_or_committed_resources_early() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner.submit(w, source(1), 8, 16).unwrap();
        let import = ready(&mut f.owner);
        let lease = f.owner.commit(import).unwrap();
        f.owner
            .submit(
                w,
                Arc::new(Source {
                    id: 2,
                    panic: true,
                    ..Source::default()
                }),
                8,
                16,
            )
            .unwrap();
        until(|| f.owner.stats().failed);
        assert!(f.dropped.try_recv().is_err());
        assert!(f.log.lock().unwrap().is_empty());
        assert_eq!(f.owner.stats().bytes, 24);
        f.owner.released(lease);
        f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(f.log.lock().unwrap().len(), 1);
    }

    #[test]
    fn descriptor_bounds_and_charge_overflow_reject_without_leaking_budget() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        assert_eq!(
            f.owner.submit(w, source(1), usize::MAX, 16),
            Err(Error::Limit)
        );
        f.owner
            .submit(
                w,
                Arc::new(Source {
                    invalid: true,
                    ..Source::default()
                }),
                8,
                16,
            )
            .unwrap();
        until(|| f.owner.stats().failed);
        assert_eq!(f.owner.stats().bytes, 0);
        f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_ne!(f.log.lock().unwrap()[0].1, thread::current().id());
    }

    #[test]
    fn monotonic_ids_fail_explicitly_at_exhaustion() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner.shared.state.lock().unwrap().next_id = u64::MAX - 1;
        assert_eq!(
            f.owner.submit(w, source(1), 8, 16).unwrap(),
            FrameId(u64::MAX - 1)
        );
        assert_eq!(f.owner.submit(w, source(2), 8, 16), Err(Error::Exhausted));
        assert_eq!(f.owner.open(), Err(Error::Exhausted));
        f.owner.close(w);
    }

    #[test]
    fn connection_loss_releases_committed_resources_off_event_thread() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner.submit(w, source(1), 8, 16).unwrap();
        let import = ready(&mut f.owner);
        let lease = f.owner.commit(import).unwrap();
        drop(lease); // Dropping a token is deliberately not an authoritative release.
        assert_eq!(f.owner.stats().bytes, 24);
        f.owner.connection_lost();
        let tid = f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(f.log.lock().unwrap()[0].1, tid);
        assert_ne!(tid, thread::current().id());
        assert_eq!(f.owner.stats().bytes, 0);
    }
    #[test]
    fn continuous_updates_do_not_cancel_completed_progress() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        let first = f.owner.submit(w, source(1), 8, 16).unwrap();
        let old = ready(&mut f.owner);
        let second = f.owner.submit(w, source(2), 8, 16).unwrap();
        // Pending newer output must not invalidate the useful completed frame.
        let lease1 = f.owner.commit(old).unwrap();
        assert_eq!(lease1.frame(), first);
        let next = ready(&mut f.owner);
        assert_eq!(next.frame(), second);
        let lease2 = f.owner.commit(next).unwrap();
        f.owner.released(lease1);
        f.owner.released(lease2);
        f.owner.connection_lost();
    }

    #[test]
    fn out_of_order_import_completion_cannot_present_older_frame() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner.submit(w, source(1), 8, 16).unwrap();
        let old = ready(&mut f.owner);
        f.owner.submit(w, source(2), 8, 16).unwrap();
        let next = ready(&mut f.owner);
        let lease = f.owner.commit(next).unwrap();
        assert!(matches!(f.owner.commit(old), Err(Error::Stale)));
        f.owner.released(lease);
        f.owner.connection_lost();
    }
    #[test]
    fn destructor_panics_do_not_unwind_other_committed_leases() {
        for source_drop in [false, true] {
            let mut f = fixture(Limits::default());
            let w = f.owner.open().unwrap();
            f.owner
                .submit(
                    w,
                    Arc::new(Source {
                        id: 1,
                        drop_panic: !source_drop,
                        source_drop_panic: source_drop,
                        ..Source::default()
                    }),
                    8,
                    16,
                )
                .unwrap();
            let import = ready(&mut f.owner);
            let first = f.owner.commit(import).unwrap();
            f.owner.submit(w, source(2), 8, 16).unwrap();
            let import = ready(&mut f.owner);
            let second = f.owner.commit(import).unwrap();
            f.owner.released(first);
            until(|| f.owner.stats().failed);
            assert_eq!(f.owner.stats().bytes, 24);
            assert_eq!(
                f.log
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|x| x.0)
                    .collect::<Vec<_>>(),
                vec![1]
            );
            assert!(f.dropped.try_recv().is_err());
            f.owner.released(second);
            f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(
                f.log
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|x| x.0)
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
        }
    }

    #[test]
    fn invalid_descriptor_drop_panic_preserves_committed_lease() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner.submit(w, source(1), 8, 16).unwrap();
        let import = ready(&mut f.owner);
        let lease = f.owner.commit(import).unwrap();
        f.owner
            .submit(
                w,
                Arc::new(Source {
                    id: 2,
                    invalid: true,
                    drop_panic: true,
                    ..Source::default()
                }),
                8,
                16,
            )
            .unwrap();
        until(|| f.owner.stats().failed);
        assert_eq!(
            f.log
                .lock()
                .unwrap()
                .iter()
                .map(|x| x.0)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(f.owner.stats().bytes, 24);
        f.owner.released(lease);
        f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    #[test]
    fn panic_payload_destructor_cannot_unwind_committed_leases() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner
            .submit(
                w,
                Arc::new(Source {
                    id: 1,
                    payload_drop_panic: true,
                    ..Source::default()
                }),
                8,
                16,
            )
            .unwrap();
        let import = ready(&mut f.owner);
        let first = f.owner.commit(import).unwrap();
        f.owner.submit(w, source(2), 8, 16).unwrap();
        let import = ready(&mut f.owner);
        let second = f.owner.commit(import).unwrap();
        f.owner.released(first);
        until(|| f.owner.stats().failed);
        assert_eq!(f.owner.stats().bytes, 24);
        assert!(f.dropped.try_recv().is_err());
        f.owner.released(second);
        f.dropped.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(f.log.lock().unwrap().len(), 2);
    }
    #[test]
    fn requested_cancellation_does_not_disable_future_rendering() {
        let mut f = fixture(Limits::default());
        let w = f.owner.open().unwrap();
        f.owner
            .submit(
                w,
                Arc::new(Source {
                    block: true,
                    respect_cancel: true,
                    ..Source::default()
                }),
                8,
                16,
            )
            .unwrap();
        f.started.recv_timeout(Duration::from_secs(3)).unwrap();
        f.owner.invalidate(w).unwrap();
        f.owner.submit(w, source(2), 8, 16).unwrap();
        f.unblock.send(()).unwrap();
        let import = ready(&mut f.owner);
        assert!(!f.owner.stats().failed);
        assert_eq!(import.window(), w);
        let lease = f.owner.commit(import).unwrap();
        f.owner.released(lease);
        f.owner.connection_lost();
    }
}
