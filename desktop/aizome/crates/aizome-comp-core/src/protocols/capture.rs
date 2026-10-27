//! What the capture protocols share (wlr-screencopy in `screencopy`,
//! ext-image-copy-capture in `image_copy_capture`): streams, and copies
//! into clients' buffers.
//!
//! A stream is one client's captures of one source (an output region, or a
//! window), with or without the pointer. It keeps a damage tracker of its
//! own, so its first copy happens at once and later ones only once
//! something in it changed, with the damage that says what.
//!
//! A copy renders the source afresh into the client's buffer with the
//! backend's renderer:
//!
//! - a dmabuf is bound as the render target, so the picture never leaves
//!   the GPU; the protocol answers when the GPU's fence signals, without
//!   the compositor waiting for it ([`after_fence`]);
//! - wl_shm is rendered into an offscreen buffer kept for the stream and
//!   copied out row by row (under GLES read back as BGRA, so the GPU does
//!   the swizzle; under pixman straight from the image's memory).

use std::any::Any;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::fd::OwnedFd;

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::{Buffer as _, Fourcc};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::surface::render_elements_from_surface_tree;
use smithay::backend::renderer::element::utils::{Relocate, RelocateRenderElement};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::{Bind, BufferType, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, buffer_type};
use smithay::output::Output;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, Mode, PostAction};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::{wl_buffer::WlBuffer, wl_shm, wl_surface::WlSurface};
use smithay::utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Size, Transform};
use smithay::wayland::compositor::{TraversalAction, with_surface_tree_downward};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::shm::with_buffer_contents_mut;
use tracing::{debug, warn};

use crate::WinId;
use crate::input::Owner;
use crate::render::{OutputElement, clear_color, scene_elements};
use crate::scene::{Purpose, Scene};
use crate::state::{Aizome, State};

/// The damage rectangles one frame reports at most; past that, their
/// bounds.
const MAX_DAMAGE_RECTS: usize = 16;

/// What a stream captures.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// An output's scene (its region in [`Stream::region`]).
    Output(Output),
    /// A window's own surfaces, without what covers it or its frame.
    Window(WinId),
}

/// One client's captures of one source.
pub struct Stream {
    /// Whose stream this is: the wlr-screencopy manager, or the
    /// ext-image-copy-capture session.
    pub key: ObjectId,
    pub source: Source,
    /// In the source's pixels (upright).
    pub region: Rectangle<i32, Physical>,
    pub scale: f64,
    pub cursor: bool,
    tracker: OutputDamageTracker,
    /// The offscreen buffer wl_shm copies render into, the renderer's own
    /// type (a GLES renderbuffer, a pixman image), kept between frames.
    scratch: Option<Box<dyn Any>>,
}

impl Stream {
    pub fn new(key: ObjectId, source: Source, region: Rectangle<i32, Physical>, scale: f64, cursor: bool) -> Stream {
        let tracker = OutputDamageTracker::new(region.size, scale, Transform::Normal);
        Stream { key, source, region, scale, cursor, tracker, scratch: None }
    }
}

/// A copy for the backend to make with its renderer.
pub struct Job {
    pub buffer: WlBuffer,
    /// Only if something changed since the stream's last copy.
    pub with_damage: bool,
    pub stream: Stream,
    /// An output's scene, as the logic (or the recovery path) draws it:
    /// asked for before the backend's renderer is taken.
    pub scene: Option<Scene>,
}

impl Job {
    pub fn new(state: &mut State, buffer: WlBuffer, with_damage: bool, stream: Stream) -> Job {
        let scene = match &stream.source {
            Source::Output(output) => Some(state.scene(output, Purpose::Capture { cursor: stream.cursor })),
            Source::Window(_) => None,
        };
        Job { buffer, with_damage, stream, scene }
    }
}

/// What a copy came to.
pub enum Outcome {
    /// Nothing in the stream changed since its last copy.
    Unchanged,
    /// The buffer holds the frame (once `fence` signals, if there is one).
    Copied { damage: Vec<Rectangle<i32, Physical>>, fence: Option<OwnedFd> },
}

/// Where capture dmabufs live and what they may be: the render node, and
/// each format with the modifiers the renderer can render into.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DmabufTarget {
    pub device: u64,
    pub formats: Vec<(Fourcc, Vec<u64>)>,
}

/// The formats (XRGB8888 first, then ARGB8888) and modifiers a renderer
/// can render dmabufs into.
pub fn renderable_dmabufs<R: Bind<Dmabuf>>(renderer: &R) -> Vec<(Fourcc, Vec<u64>)> {
    let Some(formats) = Bind::<Dmabuf>::supported_formats(renderer) else { return Vec::new() };
    [Fourcc::Xrgb8888, Fourcc::Argb8888]
        .into_iter()
        .filter_map(|f| {
            let mods: Vec<u64> = formats.iter().filter(|d| d.code == f).map(|d| d.modifier.into()).collect();
            (!mods.is_empty()).then_some((f, mods))
        })
        .collect()
}

/// What to offer for capture dmabufs: on `device`, what `renderer` can
/// render into (`None` if nothing).
pub fn dmabuf_target<R: Bind<Dmabuf>>(device: u64, renderer: &R) -> Option<DmabufTarget> {
    let formats = renderable_dmabufs(renderer);
    (!formats.is_empty()).then_some(DmabufTarget { device, formats })
}

/// A number that changes whenever what a source shows may have: an
/// output's count of frames that drew something; a hash of a window's
/// surfaces' commits (its popups' too), its size and its scale. `None`
/// once the source is gone.
pub fn source_serial(az: &Aizome, source: &Source) -> Option<u64> {
    match source {
        Source::Output(output) => az.out.get(output).map(|o| o.damaged_frames),
        Source::Window(id) => {
            let m = az.windows.get(id)?;
            let (size, scale) = window_size(az, *id)?;
            let mut h = DefaultHasher::new();
            (size.w, size.h, scale.to_bits()).hash(&mut h);
            let mut commits = |surface: &WlSurface| {
                with_surface_tree_downward(
                    surface,
                    (),
                    |_, _, _| TraversalAction::DoChildren(()),
                    |s, states, _| {
                        let commit = states
                            .data_map
                            .get::<RendererSurfaceStateUserData>()
                            .and_then(|d| d.lock().unwrap().current_commit().distance(Some(Default::default())));
                        (s.id(), commit).hash(&mut h);
                    },
                    |_, _, _| true,
                );
            };
            commits(m.surface());
            for p in az.placed_popups().into_iter().filter(|p| p.owner == Owner::Window(*id)) {
                commits(p.popup.wl_surface());
            }
            Some(h.finish())
        }
    }
}

/// An output's size in pixels, upright (its transform undone).
pub fn output_size(output: &Output) -> Option<Size<i32, Physical>> {
    let mode = output.current_mode()?;
    Some(output.current_transform().transform_size(mode.size))
}

/// A window's size in pixels and its scale (its output's), if it's mapped.
pub fn window_size(az: &Aizome, id: WinId) -> Option<(Size<i32, Physical>, f64)> {
    let m = az.windows.get(&id)?;
    let rect = az.window_rect(id)?;
    let scale = az
        .output_for_rect(rect)
        .or_else(|| az.outputs.first().cloned())
        .map_or(1.0, |o| o.current_scale().fractional_scale());
    let geo = m.window.geometry();
    let size: Size<i32, Physical> = geo.size.to_f64().to_physical_precise_round(scale);
    (size.w > 0 && size.h > 0).then_some((size, scale))
}

/// Damage rectangles for the client, inside the buffer, and not too many.
pub fn damage_rects(damage: Vec<Rectangle<i32, Physical>>, size: Size<i32, Physical>) -> Vec<Rectangle<i32, Physical>> {
    let bounds = Rectangle::from_size(size);
    let mut rects: Vec<_> =
        damage.into_iter().filter_map(|r| r.intersection(bounds)).filter(|r| !r.is_empty()).collect();
    if rects.len() > MAX_DAMAGE_RECTS {
        let all = rects.iter().skip(1).fold(rects[0], |a, r| a.merge(*r));
        rects = vec![all];
    }
    if rects.is_empty() {
        rects.push(bounds);
    }
    rects
}

/// Runs `done` once the GPU has finished with a copy (at once without a
/// fence), from the event loop, without waiting for it here.
pub fn after_fence(state: &mut State, fence: Option<OwnedFd>, done: impl FnOnce(&mut State) + 'static) {
    let Some(fence) = fence else {
        done(state);
        return;
    };
    let shared = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let in_loop = shared.clone();
    let source = Generic::new(fence, Interest::READ, Mode::OneShot);
    let res = state.az.lh.insert_source(source, move |_, _, state: &mut State| {
        if let Some(done) = in_loop.borrow_mut().take() {
            done(state);
        }
        Ok(PostAction::Remove)
    });
    if let Err(e) = res {
        // Can't watch the fence: wait for it here rather than never answer.
        warn!("capture: watching the fence: {}", e.error);
        let fence = e.inserted;
        let mut fd =
            libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(fence.get_ref()), events: libc::POLLIN, revents: 0 };
        // Safety: polling a sync file we own.
        unsafe { libc::poll(&mut fd, 1, 1000) };
        if let Some(done) = shared.borrow_mut().take() {
            done(state);
        }
    }
}

/// The source's scene for a stream: an output's region moved to the
/// origin, or a window's surfaces at the origin.
fn stream_elements<R>(renderer: &mut R, az: &mut Aizome, job: &Job) -> Vec<RelocateRenderElement<OutputElement<R>>>
where
    R: Renderer + ImportAll + ImportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    let stream = &job.stream;
    let offset = stream.region.loc;
    let elements = match &stream.source {
        Source::Output(output) => {
            let scale = output.current_scale().fractional_scale();
            job.scene.as_ref().map(|scene| scene_elements(renderer, scale, scene)).unwrap_or_default()
        }
        Source::Window(id) => {
            let (Some(m), Some(rect)) = (az.windows.get(id), az.window_rect(*id)) else { return Vec::new() };
            let origin = Point::<i32, Logical>::from((rect.x, rect.y));
            let geo = m.window.geometry();
            let scale = stream.scale;
            let mut out = Vec::new();
            // Its popups in front, newest first, clipped to the window.
            for p in az.placed_popups().into_iter().rev().filter(|p| p.owner == Owner::Window(*id)) {
                let loc = (p.loc - origin).to_f64().to_physical_precise_round(scale);
                out.extend(
                    render_elements_from_surface_tree(
                        renderer,
                        p.popup.wl_surface(),
                        loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(OutputElement::Surface),
                );
            }
            let loc = Point::<i32, Logical>::from((-geo.loc.x, -geo.loc.y)).to_f64().to_physical_precise_round(scale);
            out.extend(
                render_elements_from_surface_tree(renderer, m.surface(), loc, scale, 1.0, Kind::Unspecified)
                    .into_iter()
                    .map(OutputElement::Surface),
            );
            out
        }
    };
    elements
        .into_iter()
        .map(|e| RelocateRenderElement::from_element(e, (-offset.x, -offset.y), Relocate::Relative))
        .collect()
}

/// What changed in the stream since its last copy: `None` when nothing did
/// and the client only wants changes.
fn stream_damage<E>(job: &mut Job, elements: &[E]) -> Result<Option<Vec<Rectangle<i32, Physical>>>, String>
where
    E: smithay::backend::renderer::element::Element,
{
    let (damage, _) = job.stream.tracker.damage_output(1, elements).map_err(|e| format!("damage: {e:?}"))?;
    let damage = damage.cloned().unwrap_or_default();
    if job.with_damage && damage.is_empty() {
        return Ok(None);
    }
    Ok(Some(damage))
}

/// The colour behind a capture: the desktop's for an output, none (clear)
/// for a window.
fn clear_for(az: &Aizome, stream: &Stream) -> [f32; 4] {
    match stream.source {
        Source::Output(_) => clear_color(az),
        Source::Window(_) => [0.0, 0.0, 0.0, 0.0],
    }
}

/// A copy with a GPU renderer (GLES, or GLES across GPUs): dmabufs are
/// render targets; wl_shm goes through an offscreen buffer of type `T`
/// read back as BGRA.
pub fn copy_gpu<R, T>(renderer: &mut R, az: &mut Aizome, job: &mut Job) -> Result<Outcome, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<Dmabuf> + ExportMem + crate::blur::BlurRenderer,
    R::TextureId: Clone + Send + 'static,
    T: 'static,
{
    let elements = stream_elements(renderer, az, job);
    let Some(damage) = stream_damage(job, &elements)? else { return Ok(Outcome::Unchanged) };
    let clear = clear_for(az, &job.stream);
    let size = job.stream.region.size;
    match buffer_type(&job.buffer) {
        Some(BufferType::Dma) => {
            let mut dmabuf = get_dmabuf(&job.buffer).map_err(|_| "not a dmabuf")?.clone();
            if dmabuf.size() != Size::<i32, BufferCoords>::from((size.w, size.h)) {
                return Err(format!("the dmabuf is {:?}, the frame {:?}", dmabuf.size(), size));
            }
            let mut fb = renderer.bind(&mut dmabuf).map_err(|e| format!("binding the dmabuf: {e:?}"))?;
            // Every pixel: the client's buffer holds nothing we know.
            let res = job
                .stream
                .tracker
                .render_output(renderer, &mut fb, 0, &elements, clear)
                .map_err(|e| format!("render: {e:?}"))?;
            let fence = match res.sync.export() {
                Some(fd) => Some(fd),
                None => {
                    let _ = res.sync.wait();
                    None
                }
            };
            Ok(Outcome::Copied { damage, fence })
        }
        Some(BufferType::Shm) => {
            let buffer_size: Size<i32, BufferCoords> = (size.w, size.h).into();
            let mut scratch =
                match job.stream.scratch.take().and_then(|s| s.downcast::<(Size<i32, BufferCoords>, T)>().ok()) {
                    Some(s) if s.0 == buffer_size => s,
                    _ => Box::new((
                        buffer_size,
                        renderer
                            .create_buffer(Fourcc::Abgr8888, buffer_size)
                            .map_err(|e| format!("offscreen: {e:?}"))?,
                    )),
                };
            let result = (|| {
                let mut fb = renderer.bind(&mut scratch.1).map_err(|e| format!("bind: {e:?}"))?;
                job.stream
                    .tracker
                    .render_output(renderer, &mut fb, 0, &elements, clear)
                    .map_err(|e| format!("render: {e:?}"))?;
                let all = Rectangle::from_size(buffer_size);
                // BGRA is the client's byte order: the GPU swizzles as it
                // reads (EXT_read_format_bgra), else the CPU does.
                let (mapping, bgra) = match renderer.copy_framebuffer(&fb, all, Fourcc::Argb8888) {
                    Ok(m) => (m, true),
                    Err(e) => {
                        debug!("capture: no BGRA read-back ({e:?}): swizzling on the CPU");
                        let m = renderer
                            .copy_framebuffer(&fb, all, Fourcc::Abgr8888)
                            .map_err(|e| format!("read back: {e:?}"))?;
                        (m, false)
                    }
                };
                let pixels = renderer.map_texture(&mapping).map_err(|e| format!("map: {e:?}"))?;
                write_shm(&job.buffer, pixels, size.w as usize * 4, size, bgra)
            })();
            job.stream.scratch = Some(scratch);
            result.map(|()| Outcome::Copied { damage, fence: None })
        }
        _ => Err("the buffer is neither wl_shm nor a dmabuf".into()),
    }
}

/// A copy with pixman: into an image kept for the stream, then into the
/// client's wl_shm memory (pixman can't draw there directly: it reads
/// other clients' wl_shm while compositing, and smithay guards one pool
/// at a time).
pub fn copy_pixman(renderer: &mut PixmanRenderer, az: &mut Aizome, job: &mut Job) -> Result<Outcome, String> {
    use smithay::reexports::pixman::{FormatCode, Image};
    let elements = stream_elements(renderer, az, job);
    let Some(damage) = stream_damage(job, &elements)? else { return Ok(Outcome::Unchanged) };
    if !matches!(buffer_type(&job.buffer), Some(BufferType::Shm)) {
        return Err("pixman copies only into wl_shm buffers".into());
    }
    let clear = clear_for(az, &job.stream);
    let size = job.stream.region.size;
    let mut image = match job.stream.scratch.take().and_then(|s| s.downcast::<Image<'static, 'static>>().ok()) {
        Some(i) if i.width() == size.w as usize && i.height() == size.h as usize => i,
        _ => Box::new(
            Image::new(FormatCode::A8R8G8B8, size.w as usize, size.h as usize, false)
                .map_err(|_| "an offscreen image")?,
        ),
    };
    let result = (|| {
        let mut fb = renderer.bind(&mut *image).map_err(|e| format!("bind: {e:?}"))?;
        job.stream
            .tracker
            .render_output(renderer, &mut fb, 0, &elements, clear)
            .map_err(|e| format!("render: {e:?}"))?;
        drop(fb);
        let stride = image.stride();
        // Safety: the image owns `stride × height` bytes, alive while `image` is.
        let pixels = unsafe { std::slice::from_raw_parts(image.data() as *const u8, stride * size.h as usize) };
        // A8R8G8B8 in memory is BGRA: the client's byte order.
        write_shm(&job.buffer, pixels, stride, size, true)
    })();
    job.stream.scratch = Some(image);
    result.map(|()| Outcome::Copied { damage, fence: None })
}

/// Whether a client's buffer fits a capture of `size`: wl_shm XRGB8888 or
/// ARGB8888, or a dmabuf in a format and modifier on offer, of that size.
pub fn buffer_fits(buffer: &WlBuffer, size: Size<i32, Physical>, dmabuf: Option<&DmabufTarget>) -> bool {
    match buffer_type(buffer) {
        Some(BufferType::Dma) => get_dmabuf(buffer).is_ok_and(|d| {
            let format = d.format();
            let offered = dmabuf.is_some_and(|t| {
                t.formats.iter().any(|(f, mods)| *f == format.code && mods.contains(&u64::from(format.modifier)))
            });
            offered && d.size() == Size::<i32, BufferCoords>::from((size.w, size.h))
        }),
        Some(BufferType::Shm) => smithay::wayland::shm::with_buffer_contents(buffer, |_, _, b| {
            b.width == size.w
                && b.height == size.h
                && b.stride >= size.w * 4
                && matches!(b.format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)
        })
        .unwrap_or(false),
        _ => false,
    }
}

/// Copies rows of pixels (BGRA, or RGBA to swizzle) into a client's
/// XRGB8888 or ARGB8888 wl_shm buffer.
fn write_shm(
    buffer: &WlBuffer,
    pixels: &[u8],
    src_stride: usize,
    size: Size<i32, Physical>,
    bgra: bool,
) -> Result<(), String> {
    let (w, h) = (size.w as usize, size.h as usize);
    let res = with_buffer_contents_mut(buffer, |ptr, len, b| {
        if b.width as usize != w || b.height as usize != h || (b.stride as usize) < w * 4 {
            return Err(format!("the buffer is {}×{}, the frame {w}×{h}", b.width, b.height));
        }
        if !matches!(b.format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888) {
            return Err(format!("unsupported buffer format {:?}", b.format));
        }
        let (offset, stride) = (b.offset as usize, b.stride as usize);
        if offset + stride * (h - 1) + w * 4 > len || pixels.len() < src_stride * (h - 1) + w * 4 {
            return Err("the buffer is too small".into());
        }
        // Safety: the pool is mapped `len` bytes long and the extent fits.
        let dst = unsafe { std::slice::from_raw_parts_mut(ptr.add(offset), len - offset) };
        for y in 0..h {
            let src = &pixels[y * src_stride..y * src_stride + w * 4];
            let row = &mut dst[y * stride..y * stride + w * 4];
            if bgra {
                row.copy_from_slice(src);
            } else {
                for (d, s) in row.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
                    *d = [s[2], s[1], s[0], s[3]];
                }
            }
        }
        Ok(())
    });
    match res {
        Ok(r) => r,
        Err(e) => Err(format!("{e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_is_clipped_and_bounded() {
        let size = Size::from((100, 50));
        let r = |x, y, w, h| Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into());
        assert_eq!(damage_rects(vec![r(90, 40, 20, 20)], size), vec![r(90, 40, 10, 10)]);
        // Nothing inside: the whole buffer, rather than no damage at all.
        assert_eq!(damage_rects(vec![r(200, 0, 5, 5)], size), vec![r(0, 0, 100, 50)]);
        let many: Vec<_> = (0..40).map(|i| r(i, i, 1, 1)).collect();
        assert_eq!(damage_rects(many, size), vec![r(0, 0, 40, 40)]);
    }
}
