//! ext-image-copy-capture-v1 with ext-image-capture-source-v1, for
//! outputs and for windows (through ext-foreign-toplevel-list handles):
//! what xdg-desktop-portal-wlr 0.8 uses for browsers' screen and window
//! sharing.
//!
//! The portal's wlr-screencopy path stalls after the first frame with
//! PipeWire 1.x: its stream drives the graph, and after a frame it only
//! queues the buffer, which runs one cycle and never calls it back for the
//! next capture. Its ext-image-copy-capture path triggers each cycle
//! itself, so sharing streams.
//!
//! A session is a [`Stream`] (`capture`): its first frame is copied at
//! once, with full damage; later ones once its source changes (an output
//! drew something, or the window's surfaces committed), with damage naming
//! what did. Frames are copied at the end of the loop iteration, into wl_shm
//! (XRGB8888 or ARGB8888) or dmabufs (on the render node, in the formats
//! and modifiers the GPU renders to), and are upright (transform normal).
//!
//! A window is captured without its frame or what covers it, with its
//! popups, at its output's scale; the pointer is painted onto outputs
//! only. When a source's size changes the session sends new constraints
//! (and frames already waiting with the old size fail with
//! `buffer_constraints`); when it goes, the session stops. Cursor sessions
//! are accepted but stop at once: the pointer is painted onto frames
//! instead.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use smithay::output::Output;
use smithay::reexports::wayland_protocols::ext::image_capture_source::v1::server::{
    ext_foreign_toplevel_image_capture_source_manager_v1 as toplevel_sources, ext_image_capture_source_v1 as source,
    ext_output_image_capture_source_manager_v1 as output_sources,
};
use smithay::reexports::wayland_protocols::ext::image_copy_capture::v1::server::{
    ext_image_copy_capture_cursor_session_v1 as cursor_session, ext_image_copy_capture_frame_v1 as frame,
    ext_image_copy_capture_manager_v1 as manager, ext_image_copy_capture_session_v1 as session,
};
use smithay::reexports::wayland_server::backend::{ClientId, ObjectId};
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_output, wl_shm};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};
use smithay::utils::{Physical, Rectangle, Size};
use tracing::{debug, warn};

use super::capture::{
    DmabufTarget, Job, Outcome, Source, Stream, after_fence, buffer_fits, damage_rects, output_size, source_serial,
    window_size,
};
use crate::state::{Aizome, State};

use cursor_session::ExtImageCopyCaptureCursorSessionV1 as CursorSession;
use frame::ExtImageCopyCaptureFrameV1 as Frame;
use manager::ExtImageCopyCaptureManagerV1 as Manager;
use output_sources::ExtOutputImageCaptureSourceManagerV1 as OutputSources;
use session::ExtImageCopyCaptureSessionV1 as Session;
use source::ExtImageCaptureSourceV1 as CaptureSource;
use toplevel_sources::ExtForeignToplevelImageCaptureSourceManagerV1 as ToplevelSources;

pub struct ImageCopyCaptureState {
    sessions: Vec<SessionState>,
    /// Captures waiting to be made: for the loop to come round, or for a
    /// change.
    pending: Vec<Pending>,
    /// The last session's number (shares are named by it).
    serial: u64,
    /// The shares last reported to IPC subscribers.
    reported: Vec<aizome_ipc::ShareInfo>,
}

struct SessionState {
    resource: Session,
    /// Its number, as a share.
    id: u64,
    /// `None` once stopped.
    source: Option<Source>,
    cursor: bool,
    /// The buffer size last sent, in pixels.
    size: Size<i32, Physical>,
    scale: f64,
    dmabuf: Option<DmabufTarget>,
    /// Taken out while a copy runs, and reset when the size changes.
    stream: Option<Stream>,
    /// Whether a frame was ready: later ones wait for changes.
    shown: bool,
    /// When its first frame was ready: it counts as a share a second
    /// later.
    shown_at: Option<Instant>,
    /// Its frame (a session has one at a time).
    frame: Option<ObjectId>,
}

struct Pending {
    frame: Frame,
    session: ObjectId,
    buffer: WlBuffer,
    /// The source's serial when last found unchanged.
    since: Option<u64>,
}

pub struct GlobalData {
    filter: Box<dyn Fn(&Client) -> bool + Send + Sync>,
}

/// What a capture source object stands for (`None`: it was gone already).
pub struct SourceData(Option<Source>);

/// A frame's buffer and whether it was captured.
pub struct FrameData {
    session: ObjectId,
    inner: Mutex<FrameInner>,
}

#[derive(Default)]
struct FrameInner {
    buffer: Option<WlBuffer>,
    captured: bool,
}

/// Whether a cursor session's capture session was asked for.
pub struct CursorData {
    taken: AtomicBool,
}

impl ImageCopyCaptureState {
    pub fn new<F>(dh: &DisplayHandle, filter: F) -> Self
    where
        F: Fn(&Client) -> bool + Clone + Send + Sync + 'static,
    {
        let data = |f: &F| GlobalData { filter: Box::new(f.clone()) };
        dh.create_global::<State, Manager, _>(1, data(&filter));
        dh.create_global::<State, OutputSources, _>(1, data(&filter));
        dh.create_global::<State, ToplevelSources, _>(1, data(&filter));
        ImageCopyCaptureState { sessions: Vec::new(), pending: Vec::new(), serial: 0, reported: Vec::new() }
    }
}

macro_rules! global {
    ($interface:ty) => {
        impl GlobalDispatch<$interface, GlobalData> for State {
            fn bind(
                _: &mut State,
                _: &DisplayHandle,
                _: &Client,
                resource: New<$interface>,
                _: &GlobalData,
                init: &mut DataInit<'_, State>,
            ) {
                init.init(resource, ());
            }

            fn can_view(client: Client, data: &GlobalData) -> bool {
                (data.filter)(&client)
            }
        }
    };
}

global!(Manager);
global!(OutputSources);
global!(ToplevelSources);

impl Dispatch<OutputSources, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &OutputSources,
        request: output_sources::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        if let output_sources::Request::CreateSource { source, output } = request {
            let output = Output::from_resource(&output).filter(|o| state.az.outputs.contains(o));
            init.init(source, SourceData(output.map(Source::Output)));
        }
    }
}

impl Dispatch<ToplevelSources, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &ToplevelSources,
        request: toplevel_sources::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        if let toplevel_sources::Request::CreateSource { source, toplevel_handle } = request {
            let win = super::foreign_toplevel::window_of(state, &toplevel_handle);
            init.init(source, SourceData(win.map(Source::Window)));
        }
    }
}

impl Dispatch<CaptureSource, SourceData> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &CaptureSource,
        _: source::Request,
        _: &SourceData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        manager: &Manager,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        match request {
            manager::Request::CreateSession { session, source, options } => {
                let session = init.init(session, ());
                let cursor = match options {
                    WEnum::Value(o) => o.contains(manager::Options::PaintCursors),
                    WEnum::Unknown(_) => {
                        manager.post_error(manager::Error::InvalidOption, "unknown options");
                        return;
                    }
                };
                let source = source.data::<SourceData>().and_then(|d| d.0.clone());
                start(state, session, source, cursor);
            }
            manager::Request::CreatePointerCursorSession { session, .. } => {
                init.init(session, CursorData { taken: AtomicBool::new(false) });
            }
            _ => {}
        }
    }
}

impl Dispatch<CursorSession, CursorData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        cursor: &CursorSession,
        request: cursor_session::Request,
        data: &CursorData,
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        if let cursor_session::Request::GetCaptureSession { session } = request {
            let session = init.init(session, ());
            if data.taken.swap(true, Ordering::Relaxed) {
                cursor.post_error(cursor_session::Error::DuplicateSession, "capture session already made");
                return;
            }
            // The pointer is painted onto frames instead.
            start(state, session, None, false);
        }
    }
}

impl Dispatch<Session, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        session: &Session,
        request: session::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        if let session::Request::CreateFrame { frame } = request {
            let frame = init.init(frame, FrameData { session: session.id(), inner: Mutex::default() });
            let ext = &mut state.az.screencopy.ext;
            let Some(s) = ext.sessions.iter_mut().find(|s| &s.resource == session) else { return };
            if let Some(old) = s.frame.replace(frame.id()) {
                // The portal makes a frame before destroying its last when
                // its stream pauses and resumes as a consumer links. Rather
                // than end its connection (duplicate_frame), the new frame
                // takes over and the old one is never answered.
                debug!("image copy capture: a second frame replaces the session's first");
                ext.pending.retain(|p| p.frame.id() != old);
            }
        }
    }

    fn destroyed(state: &mut State, _: ClientId, session: &Session, _: &()) {
        let id = session.id();
        let ext = &mut state.az.screencopy.ext;
        ext.sessions.retain(|s| s.resource.id() != id);
        // Its frames outlive it, but have nothing more to show.
        ext.pending.retain(|p| {
            if p.session == id {
                p.frame.failed(frame::FailureReason::Stopped);
            }
            p.session != id
        });
    }
}

impl Dispatch<Frame, FrameData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        frame: &Frame,
        request: frame::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let mut inner = data.inner.lock().unwrap();
        if inner.captured && !matches!(request, frame::Request::Destroy) {
            frame.post_error(frame::Error::AlreadyCaptured, "the frame was captured");
            return;
        }
        match request {
            frame::Request::AttachBuffer { buffer } => inner.buffer = Some(buffer),
            frame::Request::DamageBuffer { x, y, width, height } => {
                // Only checked: every copy redraws the whole buffer.
                if x < 0 || y < 0 || width <= 0 || height <= 0 {
                    frame.post_error(frame::Error::InvalidBufferDamage, "invalid buffer damage");
                }
            }
            frame::Request::Capture => {
                let Some(buffer) = inner.buffer.clone() else {
                    frame.post_error(frame::Error::NoBuffer, "capture without a buffer");
                    return;
                };
                inner.captured = true;
                drop(inner);
                capture(state, frame, &data.session, buffer);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, _: ClientId, frame: &Frame, data: &FrameData) {
        let id = frame.id();
        let ext = &mut state.az.screencopy.ext;
        ext.pending.retain(|p| p.frame.id() != id);
        if let Some(s) = ext.sessions.iter_mut().find(|s| s.resource.id() == data.session)
            && s.frame.as_ref() == Some(&id)
        {
            s.frame = None;
        }
    }
}

/// A source's size in pixels and its scale, while it's there.
fn source_size(az: &Aizome, source: &Source) -> Option<(Size<i32, Physical>, f64)> {
    match source {
        Source::Output(o) if az.outputs.contains(o) => {
            let size = output_size(o)?;
            (size.w > 0 && size.h > 0).then(|| (size, o.current_scale().fractional_scale()))
        }
        Source::Output(_) => None,
        Source::Window(id) => window_size(az, *id),
    }
}

/// Starts a session: sends its constraints, or stops it if its source is
/// gone.
fn start(state: &mut State, resource: Session, source: Option<Source>, cursor: bool) {
    let sized = source.as_ref().and_then(|s| source_size(&state.az, s));
    let dmabuf = super::screencopy::dmabuf_target(state);
    let (source, size, scale) = match (source, sized) {
        (Some(source), Some((size, scale))) => {
            send_constraints(&resource, size, dmabuf.as_ref());
            (Some(source), size, scale)
        }
        _ => {
            resource.stopped();
            (None, Size::default(), 1.0)
        }
    };
    let ext = &mut state.az.screencopy.ext;
    ext.serial += 1;
    let id = ext.serial;
    ext.sessions.push(SessionState {
        resource,
        id,
        source,
        cursor,
        size,
        scale,
        dmabuf,
        stream: None,
        shown: false,
        shown_at: None,
        frame: None,
    });
}

/// The buffers a session takes.
fn send_constraints(session: &Session, size: Size<i32, Physical>, dmabuf: Option<&DmabufTarget>) {
    session.buffer_size(size.w as u32, size.h as u32);
    session.shm_format(wl_shm::Format::Xrgb8888);
    session.shm_format(wl_shm::Format::Argb8888);
    if let Some(t) = dmabuf {
        session.dmabuf_device(t.device.to_ne_bytes().to_vec());
        for (format, modifiers) in &t.formats {
            session.dmabuf_format(*format as u32, modifiers.iter().flat_map(|m| m.to_ne_bytes()).collect());
        }
    }
    session.done();
}

/// A frame's capture request: made at the end of this loop iteration
/// (`flush`), unless it can't be.
fn capture(state: &mut State, frame: &Frame, session: &ObjectId, buffer: WlBuffer) {
    let ext = &mut state.az.screencopy.ext;
    let Some(s) = ext.sessions.iter().find(|s| &s.resource.id() == session).filter(|s| s.source.is_some()) else {
        frame.failed(frame::FailureReason::Stopped);
        return;
    };
    if !buffer_fits(&buffer, s.size, s.dmabuf.as_ref()) {
        frame.failed(frame::FailureReason::BufferConstraints);
        return;
    }
    ext.pending.push(Pending { frame: frame.clone(), session: session.clone(), buffer, since: None });
}

fn session_mut<'a>(ext: &'a mut ImageCopyCaptureState, id: &ObjectId) -> Option<&'a mut SessionState> {
    ext.sessions.iter_mut().find(|s| &s.resource.id() == id)
}

/// Keeps sessions up with their sources, then makes the captures whose
/// time has come (after every loop iteration, from `screencopy`).
pub fn flush(state: &mut State) {
    if state.az.screencopy.ext.sessions.is_empty() {
        report_shares(state);
        return;
    }
    follow_sources(state);
    report_shares(state);
    let pending = std::mem::take(&mut state.az.screencopy.ext.pending);
    for mut p in pending {
        if !p.frame.is_alive() {
            continue;
        }
        let Some(s) = session_mut(&mut state.az.screencopy.ext, &p.session) else {
            p.frame.failed(frame::FailureReason::Stopped);
            continue;
        };
        let Some(source) = s.source.clone() else {
            p.frame.failed(frame::FailureReason::Stopped);
            continue;
        };
        let (first, size, scale, cursor) = (!s.shown, s.size, s.scale, s.cursor);
        let serial = source_serial(&state.az, &source);
        if !first && p.since.is_some() && p.since == serial {
            // Nothing changed since it was last found unchanged.
            state.az.screencopy.ext.pending.push(p);
            continue;
        }
        let s = session_mut(&mut state.az.screencopy.ext, &p.session).expect("found above");
        let stream = s
            .stream
            .take()
            .unwrap_or_else(|| Stream::new(p.session.clone(), source, Rectangle::from_size(size), scale, cursor));
        let mut job = Job::new(state, p.buffer.clone(), !first, stream);
        let started = Instant::now();
        let result = state.backend.screencopy(&mut state.az, &mut job);
        let cpu = started.elapsed();
        let Some(s) = session_mut(&mut state.az.screencopy.ext, &p.session) else { continue };
        s.stream = Some(job.stream);
        match result {
            Ok(Outcome::Unchanged) => {
                p.since = serial;
                state.az.screencopy.ext.pending.push(p);
            }
            Ok(Outcome::Copied { damage, fence }) => {
                s.shown = true;
                let first_shown = s.shown_at.is_none();
                s.shown_at.get_or_insert_with(Instant::now);
                if first_shown {
                    report_later(state);
                }
                state.az.screencopy.stats.record(cpu, None);
                // A session's first frame is all damage.
                let damage = if first { vec![Rectangle::from_size(size)] } else { damage_rects(damage, size) };
                let frame = p.frame;
                after_fence(state, fence, move |state| ready(state, &frame, &damage));
            }
            Err(e) => {
                warn!("image copy capture: {e}");
                p.frame.failed(frame::FailureReason::Unknown);
            }
        }
    }
}

/// Stops sessions whose sources went, and sends new constraints to those
/// whose sources changed size (failing frames waiting with old buffers).
fn follow_sources(state: &mut State) {
    let az = &state.az;
    let now: Vec<_> =
        az.screencopy.ext.sessions.iter().map(|s| s.source.as_ref().and_then(|src| source_size(az, src))).collect();
    let ext = &mut state.az.screencopy.ext;
    let (mut stopped, mut resized) = (Vec::new(), Vec::new());
    for (s, now) in ext.sessions.iter_mut().zip(now) {
        if s.source.is_none() {
            continue;
        }
        match now {
            None => {
                s.source = None;
                s.stream = None;
                s.resource.stopped();
                stopped.push(s.resource.id());
            }
            Some((size, scale)) if size != s.size || scale != s.scale => {
                if size != s.size {
                    send_constraints(&s.resource, size, s.dmabuf.as_ref());
                    resized.push(s.resource.id());
                }
                // Afresh: the next frame is copied at once, all damage.
                (s.size, s.scale, s.stream, s.shown) = (size, scale, None, false);
            }
            _ => {}
        }
    }
    if stopped.is_empty() && resized.is_empty() {
        return;
    }
    let sessions = &ext.sessions;
    ext.pending.retain(|p| {
        if stopped.contains(&p.session) {
            p.frame.failed(frame::FailureReason::Stopped);
            return false;
        }
        let fits = |s: &SessionState| buffer_fits(&p.buffer, s.size, s.dmabuf.as_ref());
        if resized.contains(&p.session) && !sessions.iter().find(|s| s.resource.id() == p.session).is_some_and(fits) {
            p.frame.failed(frame::FailureReason::BufferConstraints);
            return false;
        }
        true
    });
}

/// Tells the client its frame is in the buffer.
fn ready(state: &State, frame: &Frame, damage: &[Rectangle<i32, Physical>]) {
    if !frame.is_alive() {
        return;
    }
    // Not for a frame another replaced while its copy ran: the client
    // would take it for the new one.
    let session = frame.data::<FrameData>().map(|d| d.session.clone());
    let current = state.az.screencopy.ext.sessions.iter().find(|s| Some(s.resource.id()) == session);
    if current.is_some_and(|s| s.frame.as_ref() != Some(&frame.id())) {
        return;
    }
    frame.transform(wl_output::Transform::Normal);
    for r in damage {
        frame.damage(r.loc.x, r.loc.y, r.size.w, r.size.h);
    }
    let t: Duration = state.az.clock.now().into();
    let secs = t.as_secs();
    frame.presentation_time((secs >> 32) as u32, secs as u32, t.subsec_nanos());
    frame.ready();
}

/// How long a session shows frames before it counts as a share: a
/// screenshot tool's one frame never does, and nothing is reported while
/// the portal and the app's PipeWire stream are still starting (in a VM,
/// an IPC event in that moment left the stream stalled after its first
/// frame).
const SHARE_SETTLE: Duration = Duration::from_secs(1);

/// What's being shared: sessions that have shown frames for a while, of a
/// source still there.
pub fn shares(az: &Aizome) -> Vec<aizome_ipc::ShareInfo> {
    az.screencopy
        .ext
        .sessions
        .iter()
        .filter(|s| s.shown_at.is_some_and(|t| t.elapsed() >= SHARE_SETTLE))
        .filter_map(|s| {
            let (output, window) = match s.source.as_ref()? {
                Source::Output(o) => (Some(o.name()), None),
                Source::Window(w) => (None, Some(*w)),
            };
            Some(aizome_ipc::ShareInfo { id: s.id, output, window, cursor: s.cursor })
        })
        .collect()
}

/// Tells IPC subscribers when what's shared changed (the shell's
/// indicator).
fn report_shares(state: &mut State) {
    let now = shares(&state.az);
    if now != state.az.screencopy.ext.reported {
        state.az.ipc.emit(&aizome_ipc::Event::SharesChanged { shares: now.clone() });
        state.az.screencopy.ext.reported = now;
    }
}

/// Stops a share (every one without `id`), as if its source had gone: the
/// session says `stopped` and a frame waiting fails, so the portal ends
/// the stream and the app sharing it hears the track ended. How many
/// stopped.
pub fn stop(state: &mut State, id: Option<u64>) -> usize {
    let ext = &mut state.az.screencopy.ext;
    let mut stopped = Vec::new();
    for s in ext.sessions.iter_mut().filter(|s| s.source.is_some() && id.is_none_or(|id| s.id == id)) {
        s.source = None;
        s.stream = None;
        s.resource.stopped();
        stopped.push(s.resource.id());
    }
    ext.pending.retain(|p| {
        let stop = stopped.contains(&p.session);
        if stop {
            p.frame.failed(frame::FailureReason::Stopped);
        }
        !stop
    });
    report_shares(state);
    stopped.len()
}

/// Reports the shares once a session that just showed its first frame has
/// lasted (unless it's gone by then).
fn report_later(state: &mut State) {
    use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
    let timer = Timer::from_duration(SHARE_SETTLE + Duration::from_millis(10));
    let res = state.az.lh.insert_source(timer, |_, _, state: &mut State| {
        report_shares(state);
        TimeoutAction::Drop
    });
    if let Err(e) = res {
        warn!("image copy capture: a timer for the shares: {}", e.error);
    }
}
