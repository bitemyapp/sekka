//! True background blur behind the shell's frosted surfaces, under GLES
//! (the Full effects tier).
//!
//! The logic puts a [`crate::scene::SceneItem::Blur`] straight behind a
//! surface it wants blurred behind (the Deskbar, the shell's menus); the
//! core turns it into a [`BlurElement`]. Drawn, the element copies what
//! the frame holds under it so far (the backdrop: everything behind the
//! surface, already drawn), blurs it on the GPU (dual Kawase: the copy at
//! half size, two passes down to an eighth and two back up) and puts the
//! blur back where the surface will cover it, only where the surface's own
//! pixels are mostly opaque (alpha 0.75 and up: its frosted face, not its
//! hard shadow or the clear space around it), so the face, 90% opaque,
//! shows a blur through it rather than the sharp windows beneath.
//!
//! Blurring again every frame would be wasted work: a frame first looks
//! at what's under each blur ([`prepare`]), and only if any of it
//! changed does the element take a new copy; otherwise it draws the blur
//! it kept. A new copy redraws the whole region, [`MARGIN`] pixels beyond
//! the surface included, so the blur's edges sample what's really there
//! and not last frame's picture.
//!
//! Pixman draws nothing for it (the frost stays as the shell draws it), nor
//! does an output with a rotation or flip.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use smithay::backend::drm::DrmNode;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{Capability, GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::utils::{CommitCounter, with_renderer_surface_state};
use smithay::backend::renderer::{BufferType, Frame, Renderer, buffer_type};
use smithay::output::Output;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform};

use crate::render::OutputElement;

/// How far beyond a surface the blur samples, in pixels.
pub const MARGIN: i32 = 32;

/// How opaque a surface's pixel must be to have the blur under it.
const FACE_ALPHA: f32 = 0.75;

/// Each output's identity for a surface's blur, kept in the surface's
/// data (so a blur keeps its element id from frame to frame).
#[derive(Default)]
struct Slots(Mutex<HashMap<String, Slot>>);

struct Slot {
    id: Id,
    commit: CommitCounter,
    geometry: Option<Rectangle<i32, Physical>>,
}

/// A blur behind one surface on one output.
#[derive(Debug, Clone)]
pub struct BlurElement {
    id: Id,
    commit: CommitCounter,
    /// The surface and [`MARGIN`] around it.
    geometry: Rectangle<i32, Physical>,
    surface: WlSurface,
    output: String,
    /// The surface's texture coordinates of a pixel `p` of the element:
    /// `p * mask_scale + mask_offset`.
    mask_scale: [f32; 2],
    mask_offset: [f32; 2],
    /// Take a new copy of what's under it (else draw the blur kept).
    pub recompute: bool,
    /// The GPU that draws, with several (its texture of the surface).
    pub render_node: Option<DrmNode>,
}

impl BlurElement {
    /// A blur behind `surface`, whose origin is at `loc` on `output`; none
    /// unless it shows a shared-memory buffer (its pixels mask the blur).
    pub fn new(surface: &WlSurface, loc: Point<i32, Physical>, output: &str, scale: f64) -> Option<BlurElement> {
        let (view, buffer_scale, size) = with_renderer_surface_state(surface, |s| {
            let b = s.buffer()?;
            if !matches!(buffer_type(b), Some(BufferType::Shm)) || s.buffer_transform() != Transform::Normal {
                return None;
            }
            Some((s.view()?, s.buffer_scale(), s.buffer_size()?))
        })??;
        let face: Rectangle<i32, Physical> = Rectangle::new(loc, view.dst.to_f64().to_physical(scale).to_i32_round());
        if face.is_empty() || view.dst.w <= 0 || view.dst.h <= 0 {
            return None;
        }
        let geometry =
            Rectangle::new(face.loc - Point::from((MARGIN, MARGIN)), face.size + Size::from((2 * MARGIN, 2 * MARGIN)));
        // Element pixel → surface logical → buffer logical → texel.
        let tex = (size.w as f64 * buffer_scale as f64, size.h as f64 * buffer_scale as f64);
        let k = (view.src.size.w / view.dst.w as f64, view.src.size.h / view.dst.h as f64);
        let bs = buffer_scale as f64;
        let d = ((geometry.loc.x - loc.x) as f64, (geometry.loc.y - loc.y) as f64);
        let mask_scale = [(k.0 * bs / (scale * tex.0)) as f32, (k.1 * bs / (scale * tex.1)) as f32];
        let mask_offset = [
            ((d.0 / scale * k.0 + view.src.loc.x) * bs / tex.0) as f32,
            ((d.1 / scale * k.1 + view.src.loc.y) * bs / tex.1) as f32,
        ];
        let (id, commit, recompute) = smithay::wayland::compositor::with_states(surface, |states| {
            let slots = states.data_map.get_or_insert_threadsafe(Slots::default);
            let mut slots = slots.0.lock().unwrap();
            let slot = slots.entry(output.to_owned()).or_insert_with(|| Slot {
                id: Id::new(),
                commit: CommitCounter::default(),
                geometry: None,
            });
            let moved = slot.geometry != Some(geometry);
            if moved {
                slot.geometry = Some(geometry);
                slot.commit.increment();
            }
            (slot.id.clone(), slot.commit, moved)
        });
        Some(BlurElement {
            id,
            commit,
            geometry,
            surface: surface.clone(),
            output: output.to_owned(),
            mask_scale,
            mask_offset,
            recompute,
            render_node: None,
        })
    }

    /// The blur must be taken again: its whole region is redrawn.
    fn renew(&mut self) {
        self.recompute = true;
        smithay::wayland::compositor::with_states(&self.surface, |states| {
            if let Some(slots) = states.data_map.get::<Slots>()
                && let Some(slot) = slots.0.lock().unwrap().get_mut(&self.output)
            {
                slot.commit.increment();
                self.commit = slot.commit;
            }
        });
    }

    fn key(&self) -> (ObjectId, String) {
        (self.surface.id(), self.output.clone())
    }
}

impl Element for BlurElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size((self.geometry.size.w as f64, self.geometry.size.h as f64).into())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

/// Renderers the blur can draw with (it does under GLES; it's never made
/// for pixman).
pub trait BlurRenderer: Renderer {
    fn draw_blur<'frame, 'buffer>(
        frame: &mut Self::Frame<'frame, 'buffer>,
        blur: &BlurElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>
    where
        'buffer: 'frame,
        Self: 'frame;
}

impl<R: BlurRenderer> RenderElement<R> for BlurElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_blur(frame, self, dst, damage)
    }

    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

impl BlurRenderer for GlesRenderer {
    fn draw_blur<'frame, 'buffer>(
        frame: &mut GlesFrame<'frame, 'buffer>,
        blur: &BlurElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError>
    where
        'buffer: 'frame,
        Self: 'frame,
    {
        let id = frame.context_id();
        let mask = with_renderer_surface_state(&blur.surface, |s| s.texture::<GlesTexture>(id).cloned()).flatten();
        match mask {
            Some(mask) => draw_gles(frame, blur, &mask, dst, damage),
            None => Ok(()),
        }
    }
}

impl BlurRenderer for PixmanRenderer {
    fn draw_blur<'frame, 'buffer>(
        _frame: &mut Self::Frame<'frame, 'buffer>,
        _blur: &BlurElement,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>
    where
        'buffer: 'frame,
        Self: 'frame,
    {
        Ok(())
    }
}

/// Several GPUs (the udev backend): the GPU that draws does it.
pub type GbmGles =
    smithay::backend::renderer::multigpu::gbm::GbmGlesBackend<GlesRenderer, smithay::backend::drm::DrmDeviceFd>;

impl BlurRenderer for smithay::backend::renderer::multigpu::MultiRenderer<'_, '_, GbmGles, GbmGles> {
    fn draw_blur<'frame, 'buffer>(
        frame: &mut Self::Frame<'frame, 'buffer>,
        blur: &BlurElement,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>
    where
        'buffer: 'frame,
        Self: 'frame,
    {
        use smithay::backend::renderer::multigpu::MultiTexture;
        let Some(node) = blur.render_node else { return Ok(()) };
        let id = frame.context_id();
        let mask = with_renderer_surface_state(&blur.surface, |s| {
            s.texture::<MultiTexture>(id).and_then(|t| t.get::<GbmGles>(&node))
        })
        .flatten();
        if let Some(mask) = mask {
            let gles: &mut GlesFrame<'_, '_> = frame.as_mut();
            if let Err(e) = draw_gles(gles, blur, &mask, dst, damage) {
                tracing::debug!("blur: {e}");
            }
        }
        Ok(())
    }
}

/// What was under each blur on an output last frame, to tell whether it
/// changed.
#[derive(Default)]
pub struct Probe {
    under: HashMap<ObjectId, Vec<Seen>>,
    /// Their identities, in stacking order.
    ids: HashMap<ObjectId, Vec<Id>>,
}

/// An element under a blur, as it was.
#[derive(Clone, Copy, PartialEq)]
struct Seen {
    commit: CommitCounter,
    geometry: Rectangle<i32, Physical>,
    alpha: f32,
}

/// Before a frame: which blurs must be taken again, because something
/// under them (within their region, margin included) changed since the
/// last frame: an element came, went, moved, changed its alpha or its
/// place in the stack, or drew there. `render_node` is the GPU that draws,
/// with several.
///
/// (A damage tracker over the elements would say the same, but it works
/// out what every element hides of every other: 0.3–0.8 ms a frame with
/// ten windows up. This looks only at what's under the blur.)
pub fn prepare<R>(probe: &mut Probe, output: &Output, elements: &mut [OutputElement<R>], render_node: Option<DrmNode>)
where
    R: Renderer + smithay::backend::renderer::ImportAll + smithay::backend::renderer::ImportMem + BlurRenderer,
    R::TextureId: Clone + Send + 'static,
{
    if !elements.iter().any(|e| matches!(e, OutputElement::Blur(_))) {
        probe.under.clear();
        return;
    }
    let scale: Scale<f64> = output.current_scale().fractional_scale().into();
    let mut renew = Vec::new();
    let mut under = HashMap::new();
    for (i, e) in elements.iter().enumerate() {
        let OutputElement::Blur(b) = e else { continue };
        let region = b.geometry;
        let mut ids = Vec::new();
        let mut now = Vec::new();
        for e in &elements[i + 1..] {
            if matches!(e, OutputElement::Blur(_)) {
                continue;
            }
            let geometry = e.geometry(scale);
            if geometry.overlaps(region) {
                ids.push(e.id().clone());
                now.push((e, Seen { commit: e.current_commit(), geometry, alpha: e.alpha() }));
            }
        }
        let key = b.surface.id();
        let changed = match probe.under.get(&key) {
            None => true,
            Some(before) => {
                before.len() != now.len()
                    || before.iter().zip(&now).any(|(was, (e, is))| {
                        was.geometry != is.geometry
                            || was.alpha != is.alpha
                            || (was.commit != is.commit
                                && e.damage_since(scale, Some(was.commit))
                                    .iter()
                                    .any(|d| Rectangle::new(d.loc + is.geometry.loc, d.size).overlaps(region)))
                    })
                    || probe.ids.get(&key).is_none_or(|before| *before != ids)
            }
        };
        if changed || b.recompute {
            renew.push(i);
        }
        under.insert(key.clone(), (now.into_iter().map(|(_, s)| s).collect::<Vec<_>>(), ids));
    }
    probe.under.clear();
    probe.ids.clear();
    for (k, (seen, ids)) in under {
        probe.under.insert(k.clone(), seen);
        probe.ids.insert(k, ids);
    }
    for (i, e) in elements.iter_mut().enumerate() {
        if let OutputElement::Blur(b) = e {
            b.render_node = render_node;
            if renew.contains(&i) {
                b.renew();
            }
        }
    }
}

// ---- GLES ---------------------------------------------------------------------

/// The passes' programs, and each blur's textures, per GL context (in its
/// EGL context's data).
#[derive(Default)]
struct Gl {
    programs: Option<Programs>,
    blurs: HashMap<(ObjectId, String), Chain>,
    /// Frames drawn, to forget blurs no longer drawn.
    frame: u64,
}

struct Programs {
    down: Program,
    up: Program,
    show: Program,
}

struct Program {
    id: ffi::types::GLuint,
    tex: ffi::types::GLint,
    halfpixel: ffi::types::GLint,
    mask: ffi::types::GLint,
    fb: ffi::types::GLint,
    region: ffi::types::GLint,
    mask_map: ffi::types::GLint,
}

/// A blur's textures: half, quarter and eighth size, each with a
/// framebuffer; the blur ends up in the first.
struct Chain {
    size: Size<i32, Physical>,
    levels: Vec<(ffi::types::GLuint, ffi::types::GLuint, i32, i32)>,
    /// The region the blur in it was taken from (none until it has one).
    region: Option<Rectangle<i32, Physical>>,
    used: u64,
    /// Its surface, to free it once the surface is gone.
    surface: smithay::reexports::wayland_server::Weak<WlSurface>,
}

const VERTEX: &str = r#"#version 100
attribute vec2 pos;
varying vec2 uv;
void main() {
    uv = pos;
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const DOWN: &str = r#"#version 100
precision mediump float;
uniform sampler2D tex;
uniform vec2 halfpixel;
varying vec2 uv;
void main() {
    vec4 sum = texture2D(tex, uv) * 4.0;
    sum += texture2D(tex, uv - halfpixel);
    sum += texture2D(tex, uv + halfpixel);
    sum += texture2D(tex, uv + vec2(halfpixel.x, -halfpixel.y));
    sum += texture2D(tex, uv - vec2(halfpixel.x, -halfpixel.y));
    gl_FragColor = sum / 8.0;
}
"#;

const UP: &str = r#"#version 100
precision mediump float;
uniform sampler2D tex;
uniform vec2 halfpixel;
varying vec2 uv;
void main() {
    vec4 sum = texture2D(tex, uv + vec2(-halfpixel.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(-halfpixel.x, halfpixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, halfpixel.y * 2.0));
    sum += texture2D(tex, uv + vec2(halfpixel.x, halfpixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(halfpixel.x * 2.0, 0.0));
    sum += texture2D(tex, uv + vec2(halfpixel.x, -halfpixel.y)) * 2.0;
    sum += texture2D(tex, uv + vec2(0.0, -halfpixel.y * 2.0));
    sum += texture2D(tex, uv + vec2(-halfpixel.x, -halfpixel.y)) * 2.0;
    gl_FragColor = sum / 12.0;
}
"#;

/// Puts the blur on the frame where `pos` (frame pixels) lies on the
/// surface's face.
const SHOW_VERTEX: &str = r#"#version 100
attribute vec2 pos;
uniform vec2 fb;
uniform vec4 region;
uniform vec4 mask_map;
varying vec2 uv;
varying vec2 mask_uv;
void main() {
    uv = (pos - region.xy) / region.zw;
    mask_uv = pos * mask_map.xy + mask_map.zw;
    gl_Position = vec4(pos / fb * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const SHOW: &str = r#"#version 100
precision mediump float;
uniform sampler2D tex;
uniform sampler2D mask;
varying vec2 uv;
varying vec2 mask_uv;
void main() {
    // Only on the surface (its texture would repeat beyond it).
    if (mask_uv.x < 0.0 || mask_uv.y < 0.0 || mask_uv.x > 1.0 || mask_uv.y > 1.0
            || texture2D(mask, mask_uv).a < FACE_ALPHA) {
        discard;
    }
    gl_FragColor = vec4(texture2D(tex, uv).rgb, 1.0);
}
"#;

unsafe fn compile(gl: &ffi::Gles2, vertex: &str, fragment: &str) -> Option<Program> {
    unsafe fn shader(gl: &ffi::Gles2, kind: ffi::types::GLenum, src: &str) -> Option<ffi::types::GLuint> {
        unsafe {
            let s = gl.CreateShader(kind);
            let ptr = src.as_ptr() as *const ffi::types::GLchar;
            let len = src.len() as ffi::types::GLint;
            gl.ShaderSource(s, 1, &ptr, &len);
            gl.CompileShader(s);
            let mut ok = 0;
            gl.GetShaderiv(s, ffi::COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut log = vec![0u8; 1024];
                let mut n = 0;
                gl.GetShaderInfoLog(s, log.len() as i32, &mut n, log.as_mut_ptr() as *mut _);
                tracing::warn!("blur shader: {}", String::from_utf8_lossy(&log[..n.max(0) as usize]));
                gl.DeleteShader(s);
                return None;
            }
            Some(s)
        }
    }
    unsafe {
        let fragment = fragment.replace("FACE_ALPHA", &format!("{FACE_ALPHA:.3}"));
        let vs = shader(gl, ffi::VERTEX_SHADER, vertex)?;
        let fs = shader(gl, ffi::FRAGMENT_SHADER, &fragment)?;
        let p = gl.CreateProgram();
        gl.AttachShader(p, vs);
        gl.AttachShader(p, fs);
        gl.BindAttribLocation(p, 0, c"pos".as_ptr());
        gl.LinkProgram(p);
        gl.DetachShader(p, vs);
        gl.DetachShader(p, fs);
        gl.DeleteShader(vs);
        gl.DeleteShader(fs);
        let mut ok = 0;
        gl.GetProgramiv(p, ffi::LINK_STATUS, &mut ok);
        if ok == 0 {
            tracing::warn!("blur program didn't link");
            gl.DeleteProgram(p);
            return None;
        }
        let u = |name: &std::ffi::CStr| gl.GetUniformLocation(p, name.as_ptr());
        Some(Program {
            id: p,
            tex: u(c"tex"),
            halfpixel: u(c"halfpixel"),
            mask: u(c"mask"),
            fb: u(c"fb"),
            region: u(c"region"),
            mask_map: u(c"mask_map"),
        })
    }
}

impl Chain {
    unsafe fn new(gl: &ffi::Gles2, size: Size<i32, Physical>, surface: &WlSurface) -> Chain {
        let mut levels = Vec::new();
        let (mut w, mut h) = (size.w, size.h);
        for _ in 0..3 {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            unsafe {
                let mut tex = 0;
                gl.GenTextures(1, &mut tex);
                gl.BindTexture(ffi::TEXTURE_2D, tex);
                gl.TexImage2D(
                    ffi::TEXTURE_2D,
                    0,
                    ffi::RGBA as i32,
                    w,
                    h,
                    0,
                    ffi::RGBA,
                    ffi::UNSIGNED_BYTE,
                    std::ptr::null(),
                );
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_S, ffi::CLAMP_TO_EDGE as i32);
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_WRAP_T, ffi::CLAMP_TO_EDGE as i32);
                let mut fbo = 0;
                gl.GenFramebuffers(1, &mut fbo);
                gl.BindFramebuffer(ffi::FRAMEBUFFER, fbo);
                gl.FramebufferTexture2D(ffi::FRAMEBUFFER, ffi::COLOR_ATTACHMENT0, ffi::TEXTURE_2D, tex, 0);
                levels.push((tex, fbo, w, h));
            }
        }
        Chain { size, levels, region: None, used: 0, surface: surface.downgrade() }
    }

    unsafe fn delete(&self, gl: &ffi::Gles2) {
        for (tex, fbo, ..) in &self.levels {
            unsafe {
                gl.DeleteFramebuffers(1, fbo);
                gl.DeleteTextures(1, tex);
            }
        }
    }
}

/// The GL state the frame relies on, kept across our passes.
struct Saved {
    fbo: i32,
    viewport: [i32; 4],
    scissor: bool,
    scissor_box: [i32; 4],
    blend: bool,
    program: i32,
    active: i32,
}

unsafe fn save(gl: &ffi::Gles2) -> Saved {
    unsafe {
        let mut s = Saved {
            fbo: 0,
            viewport: [0; 4],
            scissor: gl.IsEnabled(ffi::SCISSOR_TEST) != 0,
            scissor_box: [0; 4],
            blend: gl.IsEnabled(ffi::BLEND) != 0,
            program: 0,
            active: 0,
        };
        gl.GetIntegerv(ffi::FRAMEBUFFER_BINDING, &mut s.fbo);
        gl.GetIntegerv(ffi::VIEWPORT, s.viewport.as_mut_ptr());
        gl.GetIntegerv(ffi::SCISSOR_BOX, s.scissor_box.as_mut_ptr());
        gl.GetIntegerv(ffi::CURRENT_PROGRAM, &mut s.program);
        gl.GetIntegerv(ffi::ACTIVE_TEXTURE, &mut s.active);
        s
    }
}

unsafe fn restore(gl: &ffi::Gles2, s: &Saved) {
    unsafe {
        gl.BindFramebuffer(ffi::FRAMEBUFFER, s.fbo as u32);
        gl.Viewport(s.viewport[0], s.viewport[1], s.viewport[2], s.viewport[3]);
        gl.Scissor(s.scissor_box[0], s.scissor_box[1], s.scissor_box[2], s.scissor_box[3]);
        if s.scissor {
            gl.Enable(ffi::SCISSOR_TEST);
        } else {
            gl.Disable(ffi::SCISSOR_TEST);
        }
        if s.blend {
            gl.Enable(ffi::BLEND);
            gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        } else {
            gl.Disable(ffi::BLEND);
        }
        gl.ActiveTexture(ffi::TEXTURE1);
        gl.BindTexture(ffi::TEXTURE_2D, 0);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindTexture(ffi::TEXTURE_2D, 0);
        gl.ActiveTexture(s.active as u32);
        gl.UseProgram(s.program as u32);
        gl.DisableVertexAttribArray(0);
        gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
    }
}

/// A full-target quad for the passes.
const QUAD: [f32; 12] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];

unsafe fn pass(gl: &ffi::Gles2, p: &Program, from: (u32, i32, i32), to: (u32, i32, i32)) {
    unsafe {
        gl.BindFramebuffer(ffi::FRAMEBUFFER, to.0);
        gl.Viewport(0, 0, to.1, to.2);
        gl.UseProgram(p.id);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindTexture(ffi::TEXTURE_2D, from.0);
        gl.Uniform1i(p.tex, 0);
        gl.Uniform2f(p.halfpixel, 0.5 / from.1 as f32, 0.5 / from.2 as f32);
        gl.VertexAttribPointer(0, 2, ffi::FLOAT, ffi::FALSE, 0, QUAD.as_ptr() as *const _);
        gl.EnableVertexAttribArray(0);
        gl.DrawArrays(ffi::TRIANGLES, 0, 6);
    }
}

fn draw_gles(
    frame: &mut GlesFrame<'_, '_>,
    blur: &BlurElement,
    mask: &GlesTexture,
    dst: Rectangle<i32, Physical>,
    damage: &[Rectangle<i32, Physical>],
) -> Result<(), GlesError> {
    if frame.transformation() != Transform::Normal || !frame.capabilities().contains(&Capability::Blit) {
        return Ok(());
    }
    let gl_state: Arc<Mutex<Gl>> =
        frame.egl_context().user_data().get_or_insert_threadsafe(|| Arc::new(Mutex::new(Gl::default()))).clone();
    let mut st = gl_state.lock().unwrap();
    let mask_id = mask.tex_id();
    let key = blur.key();
    frame.with_context(|gl| unsafe {
        let saved = save(gl);
        let fb = Rectangle::<i32, Physical>::new(
            (saved.viewport[0], saved.viewport[1]).into(),
            (saved.viewport[2], saved.viewport[3]).into(),
        );
        let Some(region) = dst.intersection(fb) else { return };
        if st.programs.is_none() {
            let down = compile(gl, VERTEX, DOWN);
            let up = compile(gl, VERTEX, UP);
            let show = compile(gl, SHOW_VERTEX, SHOW);
            match (down, up, show) {
                (Some(down), Some(up), Some(show)) => st.programs = Some(Programs { down, up, show }),
                _ => {
                    restore(gl, &saved);
                    return;
                }
            }
        }
        st.frame += 1;
        let now = st.frame;
        // Forget blurs whose surfaces went (menus come and go), or not
        // drawn for a while.
        let stale: Vec<_> = st
            .blurs
            .iter()
            .filter(|(_, c)| now - c.used > 600 || c.surface.upgrade().is_err())
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            if let Some(c) = st.blurs.remove(&k) {
                c.delete(gl);
            }
        }
        if st.blurs.get(&key).is_some_and(|c| c.size != region.size) {
            let c = st.blurs.remove(&key).unwrap();
            c.delete(gl);
        }
        if !st.blurs.contains_key(&key) {
            let c = Chain::new(gl, region.size, &blur.surface);
            st.blurs.insert(key.clone(), c);
        }
        let Gl { programs, blurs, .. } = &mut *st;
        let programs = programs.as_ref().unwrap();
        let chain = blurs.get_mut(&key).unwrap();
        chain.used = now;

        gl.Disable(ffi::SCISSOR_TEST);
        gl.Disable(ffi::BLEND);
        if blur.recompute || chain.region != Some(region) {
            // What's under the surface so far, at half size.
            let l = &chain.levels;
            gl.BindFramebuffer(ffi::READ_FRAMEBUFFER, saved.fbo as u32);
            gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, l[0].1);
            gl.BlitFramebuffer(
                region.loc.x,
                region.loc.y,
                region.loc.x + region.size.w,
                region.loc.y + region.size.h,
                0,
                0,
                l[0].2,
                l[0].3,
                ffi::COLOR_BUFFER_BIT,
                ffi::LINEAR,
            );
            let lv = |i: usize| (l[i].0, l[i].2, l[i].3);
            let fbo = |i: usize| (l[i].1, l[i].2, l[i].3);
            pass(gl, &programs.down, lv(0), fbo(1));
            pass(gl, &programs.down, lv(1), fbo(2));
            pass(gl, &programs.up, lv(2), fbo(1));
            pass(gl, &programs.up, lv(1), fbo(0));
            chain.region = Some(region);
        }

        // The blur, where the surface's face will be.
        let mut verts: Vec<f32> = Vec::with_capacity(damage.len() * 12);
        for d in damage {
            let r = Rectangle::new(d.loc + dst.loc, d.size);
            let Some(r) = r.intersection(region) else { continue };
            let (x0, y0, x1, y1) =
                (r.loc.x as f32, r.loc.y as f32, (r.loc.x + r.size.w) as f32, (r.loc.y + r.size.h) as f32);
            verts.extend_from_slice(&[x0, y0, x1, y0, x0, y1, x0, y1, x1, y0, x1, y1]);
        }
        if !verts.is_empty() {
            let p = &programs.show;
            gl.BindFramebuffer(ffi::FRAMEBUFFER, saved.fbo as u32);
            gl.Viewport(saved.viewport[0], saved.viewport[1], saved.viewport[2], saved.viewport[3]);
            gl.UseProgram(p.id);
            gl.ActiveTexture(ffi::TEXTURE0);
            gl.BindTexture(ffi::TEXTURE_2D, chain.levels[0].0);
            gl.Uniform1i(p.tex, 0);
            gl.ActiveTexture(ffi::TEXTURE1);
            gl.BindTexture(ffi::TEXTURE_2D, mask_id);
            gl.Uniform1i(p.mask, 1);
            gl.Uniform2f(p.fb, fb.size.w as f32, fb.size.h as f32);
            gl.Uniform4f(
                p.region,
                region.loc.x as f32,
                region.loc.y as f32,
                region.size.w as f32,
                region.size.h as f32,
            );
            // From frame pixels to the surface's texture: frame pixel x is
            // element pixel x - dst.x.
            let [sx, sy] = blur.mask_scale;
            let [ox, oy] = blur.mask_offset;
            gl.Uniform4f(p.mask_map, sx, sy, ox - dst.loc.x as f32 * sx, oy - dst.loc.y as f32 * sy);
            gl.VertexAttribPointer(0, 2, ffi::FLOAT, ffi::FALSE, 0, verts.as_ptr() as *const _);
            gl.EnableVertexAttribArray(0);
            gl.DrawArrays(ffi::TRIANGLES, 0, (verts.len() / 2) as i32);
        }
        restore(gl, &saved);
    })
}
