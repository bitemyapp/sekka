//! OpenGL for the cores that draw with it (the Dreamcast's and the
//! PlayStation 2's): a context on the compositor's GPU through EGL, with
//! no window, and a framebuffer the core draws its frames in.
//!
//! Each frame is copied on the GPU (the right way up: GL's first line is
//! its picture's last) into one of a few textures shared with the
//! compositor as dmabufs, which the window's video sink shows as they
//! are, nothing read back. Where the compositor can't take them (another
//! GPU, a renderer in software), the frame is read back into shared
//! memory, as a software core's frames go ([`super::video::Screen`]).
//!
//! EGL is opened at run time (`libEGL.so.1`, libglvnd's), and every GL
//! function through it, as the cores' own are.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use aizome_kit::sidestep_ui::video::{Dmabuf, DmabufPlane, VideoBuffer, VideoCaps, VideoFrame, VideoSink, fourcc};

use super::ffi::*;

type EGLDisplay = *mut c_void;
type EGLContext = *mut c_void;
type EGLImage = *mut c_void;
type EGLDevice = *mut c_void;
type EGLint = i32;

const EGL_NONE: EGLint = 0x3038;
const EGL_EXTENSIONS: EGLint = 0x3055;
const EGL_OPENGL_API: c_uint = 0x30a2;
const EGL_OPENGL_ES_API: c_uint = 0x30a0;
const EGL_CONTEXT_MAJOR_VERSION: EGLint = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: EGLint = 0x30fb;
const EGL_CONTEXT_OPENGL_PROFILE_MASK: EGLint = 0x30fd;
const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: EGLint = 1;
const EGL_CONTEXT_OPENGL_COMPATIBILITY_PROFILE_BIT: EGLint = 2;
const EGL_PLATFORM_DEVICE_EXT: c_uint = 0x313f;
const EGL_PLATFORM_SURFACELESS_MESA: c_uint = 0x31dd;
const EGL_DRM_DEVICE_FILE_EXT: EGLint = 0x3233;
const EGL_DRM_RENDER_NODE_FILE_EXT: EGLint = 0x3377;
const EGL_GL_TEXTURE_2D: c_uint = 0x30b1;

const GL_FRAMEBUFFER: c_uint = 0x8d40;
const GL_READ_FRAMEBUFFER: c_uint = 0x8ca8;
const GL_DRAW_FRAMEBUFFER: c_uint = 0x8ca9;
const GL_READ_FRAMEBUFFER_BINDING: c_uint = 0x8caa;
const GL_DRAW_FRAMEBUFFER_BINDING: c_uint = 0x8ca6;
const GL_COLOR_ATTACHMENT0: c_uint = 0x8ce0;
const GL_DEPTH_ATTACHMENT: c_uint = 0x8d00;
const GL_DEPTH_STENCIL_ATTACHMENT: c_uint = 0x821a;
const GL_RENDERBUFFER: c_uint = 0x8d41;
const GL_DEPTH24_STENCIL8: c_uint = 0x88f0;
const GL_DEPTH_COMPONENT24: c_uint = 0x81a6;
const GL_FRAMEBUFFER_COMPLETE: c_uint = 0x8cd5;
const GL_TEXTURE_2D: c_uint = 0x0de1;
const GL_TEXTURE_BINDING_2D: c_uint = 0x8069;
const GL_RENDERBUFFER_BINDING: c_uint = 0x8ca7;
const GL_TEXTURE_MIN_FILTER: c_uint = 0x2801;
const GL_TEXTURE_MAG_FILTER: c_uint = 0x2800;
const GL_NEAREST: c_int = 0x2600;
const GL_RGBA8: c_int = 0x8058;
const GL_RGBA: c_uint = 0x1908;
const GL_BGRA: c_uint = 0x80e1;
const GL_UNSIGNED_BYTE: c_uint = 0x1401;
const GL_COLOR_BUFFER_BIT: c_uint = 0x4000;
const GL_SCISSOR_TEST: c_uint = 0x0c11;
const GL_RASTERIZER_DISCARD: c_uint = 0x8c89;
const GL_FRAMEBUFFER_SRGB: c_uint = 0x8db9;
const GL_PIXEL_PACK_BUFFER: c_uint = 0x88eb;
const GL_PIXEL_PACK_BUFFER_BINDING: c_uint = 0x88ed;
const GL_PACK_ALIGNMENT: c_uint = 0x0d05;
const GL_PACK_ROW_LENGTH: c_uint = 0x0d02;

/// Textures the compositor may hold at once.
const SLOTS: usize = 3;

/// The running core's `eglGetProcAddress` and framebuffer, for the two
/// functions it's given: a core may call them from a thread of its own.
static GET_PROC: AtomicUsize = AtomicUsize::new(0);
static FBO: AtomicU32 = AtomicU32::new(0);

/// The framebuffer the core draws in.
pub extern "C" fn current_framebuffer() -> usize {
    FBO.load(Ordering::Acquire) as usize
}

/// A GL function, for the core.
///
/// # Safety
///
/// `name` is null or NUL-terminated.
pub unsafe extern "C" fn proc_address(name: *const c_char) -> *mut c_void {
    let get = GET_PROC.load(Ordering::Acquire);
    if get == 0 || name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `eglGetProcAddress`, stored while its library is loaded; the
    // core's name is NUL-terminated.
    unsafe {
        let get: unsafe extern "C" fn(*const c_char) -> *mut c_void = std::mem::transmute(get);
        get(name)
    }
}

/// EGL's functions, from libglvnd.
struct Egl {
    _lib: libloading::Library,
    get_proc_address: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    get_platform_display: unsafe extern "C" fn(c_uint, *mut c_void, *const isize) -> EGLDisplay,
    initialize: unsafe extern "C" fn(EGLDisplay, *mut EGLint, *mut EGLint) -> c_uint,
    terminate: unsafe extern "C" fn(EGLDisplay) -> c_uint,
    query_string: unsafe extern "C" fn(EGLDisplay, EGLint) -> *const c_char,
    bind_api: unsafe extern "C" fn(c_uint) -> c_uint,
    create_context: unsafe extern "C" fn(EGLDisplay, *mut c_void, EGLContext, *const EGLint) -> EGLContext,
    destroy_context: unsafe extern "C" fn(EGLDisplay, EGLContext) -> c_uint,
    make_current: unsafe extern "C" fn(EGLDisplay, *mut c_void, *mut c_void, EGLContext) -> c_uint,
}

/// A function of EGL's or GL's by name, through `eglGetProcAddress`.
///
/// # Safety
///
/// `T` is the function's type.
unsafe fn proc<T: Copy>(get: unsafe extern "C" fn(*const c_char) -> *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: as promised; null where it doesn't exist.
    let p = unsafe { get(name.as_ptr()) };
    (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

macro_rules! load {
    ($get:expr, $($name:ident: $c:literal => $t:ty),* $(,)?) => {
        // SAFETY: each type is its function's, as Khronos defines it.
        unsafe { ($(proc::<$t>($get, $c)?,)*) }
    };
}

impl Egl {
    fn open() -> Option<Egl> {
        // SAFETY: libglvnd's EGL, whose initialisers are well behaved.
        let lib = unsafe { libloading::Library::new("libEGL.so.1") }.ok()?;
        // SAFETY: the function EGL's every implementation has.
        let get_proc_address: unsafe extern "C" fn(*const c_char) -> *mut c_void =
            *unsafe { lib.get(b"eglGetProcAddress\0") }.ok()?;
        let (
            get_platform_display,
            initialize,
            terminate,
            query_string,
            bind_api,
            create_context,
            destroy_context,
            make_current,
        ) = load!(
            get_proc_address,
            a: c"eglGetPlatformDisplay" => unsafe extern "C" fn(c_uint, *mut c_void, *const isize) -> EGLDisplay,
            b: c"eglInitialize" => unsafe extern "C" fn(EGLDisplay, *mut EGLint, *mut EGLint) -> c_uint,
            c: c"eglTerminate" => unsafe extern "C" fn(EGLDisplay) -> c_uint,
            d: c"eglQueryString" => unsafe extern "C" fn(EGLDisplay, EGLint) -> *const c_char,
            e: c"eglBindAPI" => unsafe extern "C" fn(c_uint) -> c_uint,
            f: c"eglCreateContext" => unsafe extern "C" fn(EGLDisplay, *mut c_void, EGLContext, *const EGLint) -> EGLContext,
            g: c"eglDestroyContext" => unsafe extern "C" fn(EGLDisplay, EGLContext) -> c_uint,
            h: c"eglMakeCurrent" => unsafe extern "C" fn(EGLDisplay, *mut c_void, *mut c_void, EGLContext) -> c_uint,
        );
        Some(Egl {
            _lib: lib,
            get_proc_address,
            get_platform_display,
            initialize,
            terminate,
            query_string,
            bind_api,
            create_context,
            destroy_context,
            make_current,
        })
    }

    /// The display on the device the compositor renders with (`main`, a
    /// dev_t), else the first with a render node, else Mesa's software
    /// one; where devices can't be listed, Mesa's surfaceless display.
    /// With the device's render node, for a GPU.
    fn display(&self, main: u64) -> Option<(EGLDisplay, Option<String>)> {
        type Query = unsafe extern "C" fn(EGLint, *mut EGLDevice, *mut EGLint) -> c_uint;
        type QueryString = unsafe extern "C" fn(EGLDevice, EGLint) -> *const c_char;
        // SAFETY: EGL_EXT_device_enumeration's and EGL_EXT_device_query's.
        let fns = unsafe {
            proc::<Query>(self.get_proc_address, c"eglQueryDevicesEXT")
                .zip(proc::<QueryString>(self.get_proc_address, c"eglQueryDeviceStringEXT"))
        };
        let mut chosen: Option<(EGLDevice, Option<String>)> = None;
        if let Some((query, string)) = fns {
            let mut devices = [std::ptr::null_mut(); 16];
            let mut n = 0;
            // SAFETY: room for 16.
            if unsafe { query(16, devices.as_mut_ptr(), &mut n) } != 0 {
                let node = |d: EGLDevice, what: EGLint| {
                    // SAFETY: a device listed; null where it has none.
                    let p = unsafe { string(d, what) };
                    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
                };
                let rdev = |path: &str| {
                    use std::os::unix::fs::MetadataExt;
                    std::fs::metadata(path).map(|m| m.rdev()).ok()
                };
                for &d in &devices[..n.clamp(0, 16) as usize] {
                    let render = node(d, EGL_DRM_RENDER_NODE_FILE_EXT);
                    let card = node(d, EGL_DRM_DEVICE_FILE_EXT);
                    let here = [&render, &card].iter().any(|p| p.as_deref().and_then(rdev).is_some_and(|r| r == main));
                    if here && main != 0 && render.is_some() {
                        chosen = Some((d, render));
                        break;
                    }
                    if render.is_some() && chosen.as_ref().is_none_or(|(_, r)| r.is_none()) {
                        chosen = Some((d, render));
                    } else if chosen.is_none() {
                        chosen = Some((d, None));
                    }
                }
            }
        }
        let (platform, native, node) = match chosen {
            Some((d, node)) => (EGL_PLATFORM_DEVICE_EXT, d, node),
            None => (EGL_PLATFORM_SURFACELESS_MESA, std::ptr::null_mut(), None),
        };
        // SAFETY: a platform and its native display, no attributes.
        let display = unsafe { (self.get_platform_display)(platform, native, std::ptr::null()) };
        let (mut major, mut minor) = (0, 0);
        // SAFETY: a display just got.
        (!display.is_null() && unsafe { (self.initialize)(display, &mut major, &mut minor) } != 0)
            .then_some((display, node))
    }
}

/// GL's functions this module calls.
#[allow(clippy::type_complexity)]
struct Fns {
    gen_framebuffers: unsafe extern "C" fn(c_int, *mut c_uint),
    delete_framebuffers: unsafe extern "C" fn(c_int, *const c_uint),
    bind_framebuffer: unsafe extern "C" fn(c_uint, c_uint),
    framebuffer_texture_2d: unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint, c_int),
    framebuffer_renderbuffer: unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint),
    check_framebuffer_status: unsafe extern "C" fn(c_uint) -> c_uint,
    gen_textures: unsafe extern "C" fn(c_int, *mut c_uint),
    delete_textures: unsafe extern "C" fn(c_int, *const c_uint),
    bind_texture: unsafe extern "C" fn(c_uint, c_uint),
    tex_image_2d: unsafe extern "C" fn(c_uint, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint, *const c_void),
    tex_parameteri: unsafe extern "C" fn(c_uint, c_uint, c_int),
    gen_renderbuffers: unsafe extern "C" fn(c_int, *mut c_uint),
    delete_renderbuffers: unsafe extern "C" fn(c_int, *const c_uint),
    bind_renderbuffer: unsafe extern "C" fn(c_uint, c_uint),
    renderbuffer_storage: unsafe extern "C" fn(c_uint, c_uint, c_int, c_int),
    blit_framebuffer: unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint),
    enable: unsafe extern "C" fn(c_uint),
    disable: unsafe extern "C" fn(c_uint),
    is_enabled: unsafe extern "C" fn(c_uint) -> u8,
    get_integerv: unsafe extern "C" fn(c_uint, *mut c_int),
    pixel_storei: unsafe extern "C" fn(c_uint, c_int),
    bind_buffer: unsafe extern "C" fn(c_uint, c_uint),
    read_pixels: unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_uint, c_uint, *mut c_void),
    flush: unsafe extern "C" fn(),
}

impl Fns {
    fn load(get: unsafe extern "C" fn(*const c_char) -> *mut c_void) -> Option<Fns> {
        let (a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x) = load!(
            get,
            a: c"glGenFramebuffers" => unsafe extern "C" fn(c_int, *mut c_uint),
            b: c"glDeleteFramebuffers" => unsafe extern "C" fn(c_int, *const c_uint),
            c: c"glBindFramebuffer" => unsafe extern "C" fn(c_uint, c_uint),
            d: c"glFramebufferTexture2D" => unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint, c_int),
            e: c"glFramebufferRenderbuffer" => unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint),
            f: c"glCheckFramebufferStatus" => unsafe extern "C" fn(c_uint) -> c_uint,
            g: c"glGenTextures" => unsafe extern "C" fn(c_int, *mut c_uint),
            h: c"glDeleteTextures" => unsafe extern "C" fn(c_int, *const c_uint),
            i: c"glBindTexture" => unsafe extern "C" fn(c_uint, c_uint),
            j: c"glTexImage2D" => unsafe extern "C" fn(c_uint, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint, *const c_void),
            k: c"glTexParameteri" => unsafe extern "C" fn(c_uint, c_uint, c_int),
            l: c"glGenRenderbuffers" => unsafe extern "C" fn(c_int, *mut c_uint),
            m: c"glDeleteRenderbuffers" => unsafe extern "C" fn(c_int, *const c_uint),
            n: c"glBindRenderbuffer" => unsafe extern "C" fn(c_uint, c_uint),
            o: c"glRenderbufferStorage" => unsafe extern "C" fn(c_uint, c_uint, c_int, c_int),
            p: c"glBlitFramebuffer" => unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_int, c_uint, c_uint),
            q: c"glEnable" => unsafe extern "C" fn(c_uint),
            r: c"glDisable" => unsafe extern "C" fn(c_uint),
            s: c"glIsEnabled" => unsafe extern "C" fn(c_uint) -> u8,
            t: c"glGetIntegerv" => unsafe extern "C" fn(c_uint, *mut c_int),
            u: c"glPixelStorei" => unsafe extern "C" fn(c_uint, c_int),
            v: c"glBindBuffer" => unsafe extern "C" fn(c_uint, c_uint),
            w: c"glReadPixels" => unsafe extern "C" fn(c_int, c_int, c_int, c_int, c_uint, c_uint, *mut c_void),
            x: c"glFlush" => unsafe extern "C" fn(),
        );
        Some(Fns {
            gen_framebuffers: a,
            delete_framebuffers: b,
            bind_framebuffer: c,
            framebuffer_texture_2d: d,
            framebuffer_renderbuffer: e,
            check_framebuffer_status: f,
            gen_textures: g,
            delete_textures: h,
            bind_texture: i,
            tex_image_2d: j,
            tex_parameteri: k,
            gen_renderbuffers: l,
            delete_renderbuffers: m,
            bind_renderbuffer: n,
            renderbuffer_storage: o,
            blit_framebuffer: p,
            enable: q,
            disable: r,
            is_enabled: s,
            get_integerv: t,
            pixel_storei: u,
            bind_buffer: v,
            read_pixels: w,
            flush: x,
        })
    }
}

/// What the core asked for, as `retro_hw_render_callback` says.
#[derive(Clone, Copy, Debug)]
pub struct Wanted {
    pub kind: c_uint,
    pub major: c_uint,
    pub minor: c_uint,
    pub depth: bool,
    pub stencil: bool,
    /// Its picture's first line is the framebuffer's last (GL's way).
    pub bottom_left: bool,
}

/// Whether Chanoma makes contexts of this kind: OpenGL's, and OpenGL ES's
/// (Vulkan and Direct3D aren't).
pub fn offered(kind: c_uint) -> bool {
    matches!(
        kind,
        RETRO_HW_CONTEXT_OPENGL
            | RETRO_HW_CONTEXT_OPENGL_CORE
            | RETRO_HW_CONTEXT_OPENGLES2
            | RETRO_HW_CONTEXT_OPENGLES3
            | RETRO_HW_CONTEXT_OPENGLES_VERSION
    )
}

/// A texture shared with the compositor.
struct Slot {
    texture: c_uint,
    fbo: c_uint,
    buffer: Option<Arc<VideoBuffer>>,
    busy: Arc<AtomicBool>,
}

struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A core's OpenGL: the context, current on the session's thread, and the
/// framebuffers.
pub struct Gl {
    egl: Egl,
    display: EGLDisplay,
    context: EGLContext,
    fns: Fns,
    wanted: Wanted,
    desktop: bool,
    /// The core's framebuffer, and what it's made of.
    pub fbo: c_uint,
    color: c_uint,
    depth: c_uint,
    size: (u32, u32),
    /// The frames' textures, as large as the core's framebuffer.
    slots: Vec<Slot>,
    /// Whether they go to the compositor as dmabufs (else they're read
    /// back).
    shared: bool,
    caps: Option<VideoCaps>,
    /// The texture with the last frame, and its size.
    last: Option<(usize, u32, u32)>,
    pub frames: u64,
    pub dropped: u64,
}

// SAFETY: made, used and dropped on the session's thread alone.
unsafe impl Send for Gl {}

impl Gl {
    /// A context of the kind the core wants, current on this thread, on
    /// the compositor's GPU (`caps`, where known). None where EGL or the
    /// kind isn't there.
    pub fn new(wanted: Wanted, caps: Option<VideoCaps>) -> Option<Gl> {
        if !offered(wanted.kind) {
            return None;
        }
        let egl = Egl::open()?;
        let (display, node) = egl.display(caps.as_ref().map_or(0, |c| c.main_device))?;
        let desktop = matches!(wanted.kind, RETRO_HW_CONTEXT_OPENGL | RETRO_HW_CONTEXT_OPENGL_CORE);
        let mut attrs = Vec::new();
        if desktop {
            if wanted.major > 0 {
                attrs.extend([EGL_CONTEXT_MAJOR_VERSION, wanted.major as EGLint]);
                attrs.extend([EGL_CONTEXT_MINOR_VERSION, wanted.minor as EGLint]);
            }
            let profile = if wanted.kind == RETRO_HW_CONTEXT_OPENGL_CORE {
                EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT
            } else {
                EGL_CONTEXT_OPENGL_COMPATIBILITY_PROFILE_BIT
            };
            if wanted.major >= 3 {
                attrs.extend([EGL_CONTEXT_OPENGL_PROFILE_MASK, profile]);
            }
        } else {
            // ES 3 at least, for the copy (an ES 2 core's work runs as is).
            let major = if wanted.kind == RETRO_HW_CONTEXT_OPENGLES_VERSION { wanted.major.max(3) } else { 3 };
            attrs.extend([EGL_CONTEXT_MAJOR_VERSION, major as EGLint]);
            if wanted.kind == RETRO_HW_CONTEXT_OPENGLES_VERSION && wanted.major >= 3 {
                attrs.extend([EGL_CONTEXT_MINOR_VERSION, wanted.minor as EGLint]);
            }
        }
        attrs.push(EGL_NONE);
        // SAFETY: an initialised display; no config (EGL_KHR_no_config_context)
        // and no surface (EGL_KHR_surfaceless_context), as Mesa's have.
        let context = unsafe {
            (egl.bind_api)(if desktop { EGL_OPENGL_API } else { EGL_OPENGL_ES_API });
            (egl.create_context)(display, std::ptr::null_mut(), std::ptr::null_mut(), attrs.as_ptr())
        };
        if context.is_null()
            || unsafe { (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), context) } == 0
        {
            log::warn!("chanoma: no OpenGL context of the kind the game core asks for ({wanted:?})");
            // SAFETY: made above.
            unsafe {
                if !context.is_null() {
                    (egl.destroy_context)(display, context);
                }
                (egl.terminate)(display);
            }
            return None;
        }
        let Some(fns) = Fns::load(egl.get_proc_address) else {
            // SAFETY: made above.
            unsafe {
                (egl.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                (egl.destroy_context)(display, context);
                (egl.terminate)(display);
            }
            return None;
        };
        // SAFETY: an initialised display.
        let ext = unsafe { (egl.query_string)(display, EGL_EXTENSIONS) };
        let ext =
            if ext.is_null() { String::new() } else { unsafe { CStr::from_ptr(ext) }.to_string_lossy().into_owned() };
        let shared = node.is_some()
            && ext.split_whitespace().any(|e| e == "EGL_MESA_image_dma_buf_export")
            && caps.as_ref().is_some_and(|c| !c.dmabuf.is_empty());
        log::info!(
            "chanoma: OpenGL on {}, its frames {}",
            node.as_deref().unwrap_or("the processor (Mesa's software renderer)"),
            if shared { "shared with the compositor" } else { "read back" }
        );
        GET_PROC.store(egl.get_proc_address as usize, Ordering::Release);
        Some(Gl {
            egl,
            display,
            context,
            fns,
            wanted,
            desktop,
            fbo: 0,
            color: 0,
            depth: 0,
            size: (0, 0),
            slots: Vec::new(),
            shared,
            caps,
            last: None,
            frames: 0,
            dropped: 0,
        })
    }

    /// The core's framebuffer and the frames' textures, `width` by
    /// `height` (its largest picture); made again only when that grows.
    pub fn configure(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if self.fbo != 0 && width <= self.size.0 && height <= self.size.1 {
            return;
        }
        self.free();
        self.size = (width, height);
        let f = &self.fns;
        let (w, h) = (width as c_int, height as c_int);
        // SAFETY: the context is current; names made here, bound only
        // while they're set up.
        unsafe {
            let mut saved = [0; 2];
            (f.get_integerv)(GL_TEXTURE_BINDING_2D, &mut saved[0]);
            (f.get_integerv)(GL_RENDERBUFFER_BINDING, &mut saved[1]);
            self.color = texture(f, w, h);
            (f.gen_framebuffers)(1, &mut self.fbo);
            (f.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (f.framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, self.color, 0);
            if self.wanted.depth || self.wanted.stencil {
                (f.gen_renderbuffers)(1, &mut self.depth);
                (f.bind_renderbuffer)(GL_RENDERBUFFER, self.depth);
                let both = self.wanted.stencil;
                (f.renderbuffer_storage)(
                    GL_RENDERBUFFER,
                    if both { GL_DEPTH24_STENCIL8 } else { GL_DEPTH_COMPONENT24 },
                    w,
                    h,
                );
                let at = if both { GL_DEPTH_STENCIL_ATTACHMENT } else { GL_DEPTH_ATTACHMENT };
                (f.framebuffer_renderbuffer)(GL_FRAMEBUFFER, at, GL_RENDERBUFFER, self.depth);
            }
            if (f.check_framebuffer_status)(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                log::warn!("chanoma: the game core's framebuffer isn't complete");
            }
            for _ in 0..SLOTS {
                let texture = texture(f, w, h);
                let mut fbo = 0;
                (f.gen_framebuffers)(1, &mut fbo);
                (f.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
                (f.framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
                self.slots.push(Slot { texture, fbo, buffer: None, busy: Arc::new(AtomicBool::new(false)) });
            }
            (f.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (f.bind_texture)(GL_TEXTURE_2D, saved[0] as c_uint);
            (f.bind_renderbuffer)(GL_RENDERBUFFER, saved[1] as c_uint);
        }
        FBO.store(self.fbo, Ordering::Release);
        if self.shared {
            for i in 0..self.slots.len() {
                let buffer = self.export(self.slots[i].texture, width, height);
                if buffer.is_none() {
                    log::warn!("chanoma: the compositor can't take this GPU's frames; reading them back");
                    self.shared = false;
                    break;
                }
                self.slots[i].buffer = buffer;
            }
        }
    }

    /// `texture` as a dmabuf the compositor takes, said to be XBGR8888 (its
    /// bytes are RGBA's; the core's alpha isn't the picture's).
    fn export(&self, texture: c_uint, width: u32, height: u32) -> Option<Arc<VideoBuffer>> {
        type CreateImage = unsafe extern "C" fn(EGLDisplay, EGLContext, c_uint, *mut c_void, *const isize) -> EGLImage;
        type DestroyImage = unsafe extern "C" fn(EGLDisplay, EGLImage) -> c_uint;
        type Query = unsafe extern "C" fn(EGLDisplay, EGLImage, *mut c_int, *mut c_int, *mut u64) -> c_uint;
        type Export = unsafe extern "C" fn(EGLDisplay, EGLImage, *mut c_int, *mut EGLint, *mut EGLint) -> c_uint;
        let get = self.egl.get_proc_address;
        // SAFETY: EGL 1.5's and EGL_MESA_image_dma_buf_export's, by their
        // types; the image is made from a texture of this context's and
        // destroyed here, the fds it gives taken.
        unsafe {
            let (create, destroy, query, export) = (
                proc::<CreateImage>(get, c"eglCreateImage")?,
                proc::<DestroyImage>(get, c"eglDestroyImage")?,
                proc::<Query>(get, c"eglExportDMABUFImageQueryMESA")?,
                proc::<Export>(get, c"eglExportDMABUFImageMESA")?,
            );
            let attrs = [EGL_NONE as isize];
            let image =
                create(self.display, self.context, EGL_GL_TEXTURE_2D, texture as usize as *mut c_void, attrs.as_ptr());
            if image.is_null() {
                return None;
            }
            let (mut format, mut planes, mut modifier) = (0, 0, 0u64);
            let ok = query(self.display, image, &mut format, &mut planes, &mut modifier) != 0 && planes == 1;
            let (mut fd, mut stride, mut offset) = (-1, 0, 0);
            let ok = ok && export(self.display, image, &mut fd, &mut stride, &mut offset) != 0 && fd >= 0;
            destroy(self.display, image);
            if !ok {
                return None;
            }
            let fd = <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd);
            let xbgr = fourcc(b"XB24");
            if !self.caps.as_ref().is_some_and(|c| c.takes_dmabuf(xbgr, modifier)) {
                return None;
            }
            let plane = DmabufPlane { fd, offset: offset as u32, stride: stride as u32 };
            Some(Arc::new(VideoBuffer::Dmabuf(Dmabuf { width, height, fourcc: xbgr, modifier, planes: vec![plane] })))
        }
    }

    /// The core's frame, `width` by `height` in its framebuffer: copied
    /// into a free texture the right way up, which goes to `sink`; or, the
    /// textures not shared, read back into `read` (XRGB8888, a line after
    /// another), which the caller shows.
    pub fn present(&mut self, width: u32, height: u32, sink: Option<&VideoSink>, read: &mut Vec<u32>) -> bool {
        let (w, h) = (width.min(self.size.0), height.min(self.size.1));
        if self.fbo == 0 || w == 0 || h == 0 {
            return false;
        }
        self.frames += 1;
        let i = if self.shared {
            match self
                .slots
                .iter()
                .position(|s| s.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok())
            {
                Some(i) => i,
                None => {
                    // The compositor holds every one: this frame goes.
                    self.dropped += 1;
                    return false;
                }
            }
        } else {
            0
        };
        self.copy(i, w, h);
        self.last = Some((i, w, h));
        if self.shared {
            let slot = &self.slots[i];
            let Some(buffer) = slot.buffer.clone() else { return false };
            let mut frame = VideoFrame::new(buffer);
            frame.crop = [0.0, 0.0, w as f32, h as f32];
            frame.keep = Some(Box::new(Release(slot.busy.clone())));
            match sink {
                Some(s) => s.show(frame),
                None => slot.busy.store(false, Ordering::Release),
            }
            true
        } else {
            self.read(i, w, h, read);
            false
        }
    }

    /// Copy the core's picture into slot `i`, its first line first; GL's
    /// state as the core left it.
    fn copy(&self, i: usize, w: u32, h: u32) {
        let f = &self.fns;
        let (w, h) = (w as c_int, h as c_int);
        // The rows reversed where the core draws GL's way up.
        let (y0, y1) = if self.wanted.bottom_left { (h, 0) } else { (0, h) };
        // SAFETY: the context is current; what's changed is put back.
        unsafe {
            let (mut read_fbo, mut draw_fbo) = (0, 0);
            (f.get_integerv)(GL_READ_FRAMEBUFFER_BINDING, &mut read_fbo);
            (f.get_integerv)(GL_DRAW_FRAMEBUFFER_BINDING, &mut draw_fbo);
            let mut caps = vec![GL_SCISSOR_TEST, GL_RASTERIZER_DISCARD];
            if self.desktop {
                caps.push(GL_FRAMEBUFFER_SRGB);
            }
            let on: Vec<bool> = caps.iter().map(|&c| (f.is_enabled)(c) != 0).collect();
            for &c in &caps {
                (f.disable)(c);
            }
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.fbo);
            (f.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, self.slots[i].fbo);
            (f.blit_framebuffer)(0, 0, w, h, 0, y0, w, y1, GL_COLOR_BUFFER_BIT, GL_NEAREST as c_uint);
            (f.flush)();
            for (&c, &was) in caps.iter().zip(&on) {
                if was {
                    (f.enable)(c);
                }
            }
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, read_fbo as c_uint);
            (f.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, draw_fbo as c_uint);
        }
    }

    /// Slot `i`'s picture as XRGB8888, its first line first.
    fn read(&self, i: usize, w: u32, h: u32, out: &mut Vec<u32>) {
        let f = &self.fns;
        out.resize(w as usize * h as usize, 0);
        // SAFETY: the context is current; `out` holds the picture; what's
        // changed is put back.
        unsafe {
            let (mut read_fbo, mut pack, mut align, mut row) = (0, 0, 0, 0);
            (f.get_integerv)(GL_READ_FRAMEBUFFER_BINDING, &mut read_fbo);
            (f.get_integerv)(GL_PIXEL_PACK_BUFFER_BINDING, &mut pack);
            (f.get_integerv)(GL_PACK_ALIGNMENT, &mut align);
            (f.get_integerv)(GL_PACK_ROW_LENGTH, &mut row);
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.slots[i].fbo);
            (f.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            (f.pixel_storei)(GL_PACK_ALIGNMENT, 4);
            (f.pixel_storei)(GL_PACK_ROW_LENGTH, 0);
            // BGRA's bytes are XRGB8888's word.
            (f.read_pixels)(0, 0, w as c_int, h as c_int, GL_BGRA, GL_UNSIGNED_BYTE, out.as_mut_ptr().cast());
            (f.pixel_storei)(GL_PACK_ALIGNMENT, align);
            (f.pixel_storei)(GL_PACK_ROW_LENGTH, row);
            (f.bind_buffer)(GL_PIXEL_PACK_BUFFER, pack as c_uint);
            (f.bind_framebuffer)(GL_READ_FRAMEBUFFER, read_fbo as c_uint);
        }
    }

    /// The last frame as RGBA, for a picture of where a game was left.
    pub fn last_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (i, w, h) = self.last?;
        let mut px = Vec::new();
        self.read(i, w, h, &mut px);
        let mut rgba = Vec::with_capacity(px.len() * 4);
        for p in &px {
            rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
        }
        Some((w, h, rgba))
    }

    fn free(&mut self) {
        let f = &self.fns;
        // SAFETY: names made by `configure`, the context current.
        unsafe {
            for s in self.slots.drain(..) {
                (f.delete_framebuffers)(1, &s.fbo);
                (f.delete_textures)(1, &s.texture);
            }
            if self.fbo != 0 {
                (f.delete_framebuffers)(1, &self.fbo);
                (f.delete_textures)(1, &self.color);
            }
            if self.depth != 0 {
                (f.delete_renderbuffers)(1, &self.depth);
            }
        }
        (self.fbo, self.color, self.depth, self.last) = (0, 0, 0, None);
        FBO.store(0, Ordering::Release);
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        self.free();
        GET_PROC.store(0, Ordering::Release);
        // SAFETY: the context and display made by `new`.
        unsafe {
            (self.egl.make_current)(self.display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            (self.egl.destroy_context)(self.display, self.context);
            (self.egl.terminate)(self.display);
        }
    }
}

/// An RGBA8 texture `w` by `h`, sampled square-edged.
///
/// # Safety
///
/// A context is current; the texture bound is left bound to this one.
unsafe fn texture(f: &Fns, w: c_int, h: c_int) -> c_uint {
    let mut t = 0;
    // SAFETY: as promised.
    unsafe {
        (f.gen_textures)(1, &mut t);
        (f.bind_texture)(GL_TEXTURE_2D, t);
        (f.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        (f.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        (f.tex_image_2d)(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, std::ptr::null());
    }
    t
}
