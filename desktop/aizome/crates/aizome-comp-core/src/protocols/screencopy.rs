//! wlr-screencopy-unstable-v1 (version 3), for wf-recorder, grim and
//! friends (xdg-desktop-portal-wlr prefers ext-image-copy-capture, in
//! `image_copy_capture`).
//!
//! Every frame is offered as wl_shm (XRGB8888, top row first) and, under
//! GLES, as a linux-dmabuf buffer too (XRGB8888, in any modifier the GPU
//! renders to). A copy renders the output's scene afresh, cropped to the
//! frame's region, into the client's buffer (`capture`).
//!
//! A stream is one client's captures of one output region, with or
//! without the pointer. It keeps a damage tracker of its own, so
//! `copy_with_damage` (what screencasts use) answers at once for a
//! stream's first frame, then only once something in its region has
//! changed, with damage events naming what did. An idle screen copies
//! nothing.
//!
//! This state also holds what both capture protocols share: the dmabuf
//! formats on offer, the copy statistics, and ext-image-copy-capture's
//! sessions.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};
use smithay::reexports::wayland_server::backend::{ClientId, ObjectId};
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm};
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};
use smithay::utils::{Physical, Rectangle};
use tracing::{debug, warn};

use super::capture::{DmabufTarget, Job, Outcome, Source, Stream, after_fence, damage_rects, output_size};
use super::image_copy_capture::ImageCopyCaptureState;
use crate::state::State;
use crate::stats::FrameStats;

use zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1 as Frame;
use zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1 as Manager;

/// Streams one manager keeps at most (a screenshot tool capturing many
/// regions in turn would otherwise collect them).
const STREAMS_PER_MANAGER: usize = 4;

pub struct ScreencopyManagerState {
    /// Copies waiting to be made: for damage, or for the loop to come round.
    pending: Vec<Pending>,
    streams: Vec<Stream>,
    /// Where and in what formats capture dmabufs may be (`None`: wl_shm
    /// only), once the backend has been asked.
    dmabuf: Option<Option<DmabufTarget>>,
    /// ext-image-copy-capture's sessions and frames.
    pub ext: ImageCopyCaptureState,
    /// CPU time per copy (collect, render, read back or submit), both
    /// protocols'.
    pub stats: FrameStats,
}

pub struct GlobalData {
    filter: Box<dyn Fn(&Client) -> bool + Send + Sync>,
}

struct Pending {
    frame: Frame,
    buffer: WlBuffer,
    with_damage: bool,
    /// The output's damaged-frame count when last found unchanged (`None`:
    /// not looked at yet).
    since: Option<u64>,
}

/// What a frame captures.
pub struct FrameData {
    inner: Mutex<FrameInner>,
}

#[derive(Clone)]
struct FrameInner {
    output: Option<Output>,
    /// In the output's pixels.
    region: Rectangle<i32, Physical>,
    cursor: bool,
    manager: Option<ObjectId>,
    used: bool,
}

impl ScreencopyManagerState {
    /// The wlr-screencopy global and ext-image-copy-capture's, for the
    /// clients `filter` lets see them.
    pub fn new<F>(dh: &DisplayHandle, filter: F) -> Self
    where
        F: Fn(&Client) -> bool + Clone + Send + Sync + 'static,
    {
        dh.create_global::<State, Manager, _>(3, GlobalData { filter: Box::new(filter.clone()) });
        ScreencopyManagerState {
            pending: Vec::new(),
            streams: Vec::new(),
            dmabuf: None,
            ext: ImageCopyCaptureState::new(dh, filter),
            stats: FrameStats::default(),
        }
    }

    /// Whether any copy has been made (for `stats`).
    pub fn active(&self) -> bool {
        self.stats.rendered > 0
    }
}

impl GlobalDispatch<Manager, GlobalData> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<Manager>,
        _: &GlobalData,
        init: &mut DataInit<'_, State>,
    ) {
        init.init(resource, ());
    }

    fn can_view(client: Client, data: &GlobalData) -> bool {
        (data.filter)(&client)
    }
}

impl Dispatch<Manager, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        manager: &Manager,
        request: zwlr_screencopy_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, State>,
    ) {
        use zwlr_screencopy_manager_v1::Request;
        let (frame, cursor, output, region) = match request {
            Request::CaptureOutput { frame, overlay_cursor, output } => (frame, overlay_cursor != 0, output, None),
            Request::CaptureOutputRegion { frame, overlay_cursor, output, x, y, width, height } => {
                (frame, overlay_cursor != 0, output, Some((x, y, width, height)))
            }
            _ => return,
        };
        let output = Output::from_resource(&output).filter(|o| state.az.outputs.contains(o));
        let frame = init.init(
            frame,
            FrameData {
                inner: Mutex::new(FrameInner {
                    output: None,
                    region: Rectangle::default(),
                    cursor,
                    manager: None,
                    used: true,
                }),
            },
        );
        let Some(output) = output else {
            frame.failed();
            return;
        };
        let Some(region) = region_of(&output, region) else {
            frame.failed();
            return;
        };
        *frame.data::<FrameData>().unwrap().inner.lock().unwrap() =
            FrameInner { output: Some(output), region, cursor, manager: Some(manager.id()), used: false };
        let (w, h) = (region.size.w as u32, region.size.h as u32);
        frame.buffer(wl_shm::Format::Xrgb8888, w, h, w * 4);
        if frame.version() >= 3 {
            if let Some((format, _)) = dmabuf_target(state).and_then(|t| t.formats.first().cloned()) {
                frame.linux_dmabuf(format as u32, w, h);
            }
            frame.buffer_done();
        }
    }

    fn destroyed(state: &mut State, _: ClientId, manager: &Manager, _: &()) {
        let id = manager.id();
        state.az.screencopy.streams.retain(|s| s.key != id);
    }
}

impl Dispatch<Frame, FrameData> for State {
    fn request(
        state: &mut State,
        _: &Client,
        frame: &Frame,
        request: zwlr_screencopy_frame_v1::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        use zwlr_screencopy_frame_v1::Request;
        let (buffer, with_damage) = match request {
            Request::Copy { buffer } => (buffer, false),
            Request::CopyWithDamage { buffer } => (buffer, true),
            Request::Destroy => {
                state.az.screencopy.pending.retain(|p| &p.frame != frame);
                return;
            }
            _ => return,
        };
        {
            let mut inner = data.inner.lock().unwrap();
            if inner.used {
                frame.post_error(zwlr_screencopy_frame_v1::Error::AlreadyUsed, "frame already used");
                return;
            }
            inner.used = true;
        }
        // Made at the end of this loop iteration (`flush_pending`): at once
        // for `copy` and a stream's first frame, else once its region
        // changes.
        state.az.screencopy.pending.push(Pending { frame: frame.clone(), buffer, with_damage, since: None });
    }
}

/// A frame's region in the output's pixels (upright: the output's
/// transform undone), from a logical region relative to the output, or
/// all of it.
fn region_of(output: &Output, logical: Option<(i32, i32, i32, i32)>) -> Option<Rectangle<i32, Physical>> {
    let size = output_size(output)?;
    let scale = output.current_scale().fractional_scale();
    let region = match logical {
        None => Rectangle::from_size(size),
        Some((x, y, w, h)) => {
            let px = |v: i32| (v as f64 * scale).round() as i32;
            let (x0, y0) = (px(x).clamp(0, size.w), px(y).clamp(0, size.h));
            let (x1, y1) = (px(x.saturating_add(w)).clamp(0, size.w), px(y.saturating_add(h)).clamp(0, size.h));
            Rectangle::new((x0, y0).into(), ((x1 - x0).max(0), (y1 - y0).max(0)).into())
        }
    };
    (region.size.w > 0 && region.size.h > 0).then_some(region)
}

/// Where capture dmabufs may be and in what formats (asked of the backend
/// once), for both capture protocols.
pub fn dmabuf_target(state: &mut State) -> Option<DmabufTarget> {
    if let Some(t) = &state.az.screencopy.dmabuf {
        return t.clone();
    }
    let t = state.backend.screencopy_dmabuf();
    debug!("capture offers dmabufs: {t:?}");
    state.az.screencopy.dmabuf = Some(t.clone());
    t
}

/// Makes the copies whose time has come (after every loop iteration), for
/// both capture protocols.
pub fn flush_pending(state: &mut State) {
    super::image_copy_capture::flush(state);
    if state.az.screencopy.pending.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut state.az.screencopy.pending);
    for mut p in pending {
        if !p.frame.is_alive() {
            continue;
        }
        let Some(info) = p.frame.data::<FrameData>().map(|d| d.inner.lock().unwrap().clone()) else { continue };
        let (Some(output), Some(manager)) = (info.output.clone(), info.manager.clone()) else {
            p.frame.failed();
            continue;
        };
        if !state.az.outputs.contains(&output) {
            p.frame.failed();
            continue;
        }
        let drawn = state.az.out.get(&output).map(|o| o.damaged_frames).unwrap_or(0);
        if p.with_damage && p.since == Some(drawn) {
            // Nothing drew since it was last found unchanged.
            state.az.screencopy.pending.push(p);
            continue;
        }
        let stream = take_stream(state, &manager, &output, info.region, info.cursor);
        let mut job = Job::new(state, p.buffer.clone(), p.with_damage, stream);
        let started = Instant::now();
        let result = state.backend.screencopy(&mut state.az, &mut job);
        let cpu = started.elapsed();
        state.az.screencopy.streams.push(job.stream);
        match result {
            Ok(Outcome::Unchanged) => {
                p.since = Some(drawn);
                state.az.screencopy.pending.push(p);
            }
            Ok(Outcome::Copied { damage, fence }) => {
                state.az.screencopy.stats.record(cpu, None);
                let damage = if p.with_damage { damage_rects(damage, info.region.size) } else { Vec::new() };
                let frame = p.frame;
                after_fence(state, fence, move |state| ready(state, &frame, &damage));
            }
            Err(e) => {
                warn!("screencopy: {e}");
                p.frame.failed();
            }
        }
    }
}

/// The stream for a capture, made if new (taken out while its copy runs).
fn take_stream(
    state: &mut State,
    manager: &ObjectId,
    output: &Output,
    region: Rectangle<i32, Physical>,
    cursor: bool,
) -> Stream {
    let streams = &mut state.az.screencopy.streams;
    // Streams of outputs that went go with them.
    let outputs = &state.az.outputs;
    streams.retain(|s| matches!(&s.source, Source::Output(o) if outputs.contains(o)));
    let source = Source::Output(output.clone());
    if let Some(i) =
        streams.iter().position(|s| &s.key == manager && s.source == source && s.region == region && s.cursor == cursor)
    {
        return streams.remove(i);
    }
    // Make room: the manager's oldest stream goes.
    let mine: Vec<usize> = streams.iter().enumerate().filter(|(_, s)| &s.key == manager).map(|(i, _)| i).collect();
    if mine.len() >= STREAMS_PER_MANAGER {
        streams.remove(mine[0]);
    }
    let scale = output.current_scale().fractional_scale();
    Stream::new(manager.clone(), source, region, scale, cursor)
}

/// Tells the client its frame is in the buffer.
fn ready(state: &State, frame: &Frame, damage: &[Rectangle<i32, Physical>]) {
    if !frame.is_alive() {
        return;
    }
    frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
    if frame.version() >= 2 {
        for r in damage {
            frame.damage(r.loc.x as u32, r.loc.y as u32, r.size.w as u32, r.size.h as u32);
        }
    }
    let t: Duration = state.az.clock.now().into();
    let secs = t.as_secs();
    frame.ready((secs >> 32) as u32, secs as u32, t.subsec_nanos());
}
