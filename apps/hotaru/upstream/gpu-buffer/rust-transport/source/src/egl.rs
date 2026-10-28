//! Linux-only diagnostic adapter. EGL/GL entry points and objects never leave
//! the worker thread; only a validated OwnedFd export crosses the boundary.
#![deny(unsafe_op_in_unsafe_fn)]
use hotaru_gpu_transport_prototype::{Cancellation, Descriptor, Driver, Frame, RenderError};
use libloading::Library;
use std::{
    ffi::{CStr, c_char, c_void},
    os::fd::{FromRawFd, OwnedFd},
    ptr,
    rc::Rc,
    time::{Duration, Instant},
};

type Handle = *mut c_void;
type GetProc = unsafe extern "C" fn(*const c_char) -> *const c_void;
macro_rules! api {
    ($( $name:ident : $ty:ty ),* $(,)?) => {
        #[allow(non_snake_case)]
        struct Api { _library: Library, $( $name: $ty, )* }
        #[allow(non_snake_case)]
        impl Api {
            fn load(path: &str) -> Result<Self, String> {
                // SAFETY: caller selects an EGL ABI library. The library stays
                // owned beside all copied function pointers until all objects die.
                let library = unsafe { Library::new(path) }.map_err(|e| e.to_string())?;
                // EGL 1.4 specifies this exact ABI; it is exported by libEGL.
                let get: GetProc = unsafe { *library.get::<GetProc>(b"eglGetProcAddress\0").map_err(|e| e.to_string())? };
                $(
                    let cname = concat!(stringify!($name), "\0").as_bytes();
                    // SAFETY: static nul-terminated names and declared Khronos ABI.
                    // Core symbols may be exported directly instead of through get.
                    let $name: $ty = unsafe {
                        if let Ok(symbol) = library.get::<$ty>(cname) { *symbol }
                        else {
                            let raw = get(cname.as_ptr().cast());
                            if raw.is_null() { return Err(format!("missing {}", stringify!($name))); }
                            // Linux EGL specifies function pointers returned by name.
                            std::mem::transmute::<*const c_void, $ty>(raw)
                        }
                    };
                )*
                Ok(Self { _library: library, $( $name, )* })
            }
        }
    };
}
api! {
    eglQueryDevicesEXT: unsafe extern "C" fn(i32, *mut Handle, *mut i32) -> u32,
    eglQueryDeviceStringEXT: unsafe extern "C" fn(Handle, i32) -> *const c_char,
    eglGetPlatformDisplayEXT: unsafe extern "C" fn(u32, Handle, *const i32) -> Handle,
    eglInitialize: unsafe extern "C" fn(Handle, *mut i32, *mut i32) -> u32,
    eglQueryString: unsafe extern "C" fn(Handle, i32) -> *const c_char,
    eglBindAPI: unsafe extern "C" fn(u32) -> u32,
    eglChooseConfig: unsafe extern "C" fn(Handle, *const i32, *mut Handle, i32, *mut i32) -> u32,
    eglCreateContext: unsafe extern "C" fn(Handle, Handle, Handle, *const i32) -> Handle,
    eglMakeCurrent: unsafe extern "C" fn(Handle, Handle, Handle, Handle) -> u32,
    eglDestroyContext: unsafe extern "C" fn(Handle, Handle) -> u32,
    eglTerminate: unsafe extern "C" fn(Handle) -> u32,
    eglGetError: unsafe extern "C" fn() -> u32,
    eglCreateImageKHR: unsafe extern "C" fn(Handle, Handle, u32, Handle, *const i32) -> Handle,
    eglDestroyImageKHR: unsafe extern "C" fn(Handle, Handle) -> u32,
    eglExportDMABUFImageQueryMESA: unsafe extern "C" fn(Handle, Handle, *mut i32, *mut i32, *mut u64) -> u32,
    eglExportDMABUFImageMESA: unsafe extern "C" fn(Handle, Handle, *mut i32, *mut i32, *mut i32) -> u32,
    eglCreateSyncKHR: unsafe extern "C" fn(Handle, u32, *const i32) -> Handle,
    eglClientWaitSyncKHR: unsafe extern "C" fn(Handle, Handle, i32, u64) -> i32,
    eglDestroySyncKHR: unsafe extern "C" fn(Handle, Handle) -> u32,
    glGetString: unsafe extern "C" fn(u32) -> *const u8,
    glGetError: unsafe extern "C" fn() -> u32,
    glGenTextures: unsafe extern "C" fn(i32, *mut u32),
    glBindTexture: unsafe extern "C" fn(u32, u32),
    glTexParameteri: unsafe extern "C" fn(u32, u32, i32),
    glTexImage2D: unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    glDeleteTextures: unsafe extern "C" fn(i32, *const u32),
    glGenFramebuffers: unsafe extern "C" fn(i32, *mut u32),
    glBindFramebuffer: unsafe extern "C" fn(u32, u32),
    glFramebufferTexture2D: unsafe extern "C" fn(u32, u32, u32, u32, i32),
    glCheckFramebufferStatus: unsafe extern "C" fn(u32) -> u32,
    glDeleteFramebuffers: unsafe extern "C" fn(i32, *const u32),
    glClearColor: unsafe extern "C" fn(f32, f32, f32, f32),
    glClear: unsafe extern "C" fn(u32),
    glEnable: unsafe extern "C" fn(u32),
    glScissor: unsafe extern "C" fn(i32, i32, i32, i32),
    glReadPixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
}
const NONE: i32 = 0x3038;
const TEX: u32 = 0x0de1;
const FB: u32 = 0x8d40;
const COLORS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 255, 255],
];
fn require(ok: bool, stage: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(stage.into()) }
}
// SAFETY: EGL/GL returns a static nul-terminated string for a valid enum, or null.
unsafe fn string(raw: *const c_char) -> String {
    if raw.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned()
    }
}
struct Context {
    api: Api,
    display: Handle,
    context: Handle,
    initialized: bool,
    shutdown: Option<std::sync::mpsc::Sender<bool>>,
}
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: Rc confines this owner to its creation thread. Resource Rc refs
        // keep the context/library alive until all GL objects have been destroyed.
        unsafe {
            let mut ok = true;
            if !self.context.is_null() {
                ok &= (self.api.eglMakeCurrent)(
                    self.display,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ) != 0;
                ok &= (self.api.eglDestroyContext)(self.display, self.context) != 0;
            }
            if self.initialized {
                ok &= (self.api.eglTerminate)(self.display) != 0;
            }
            println!(
                "{}",
                serde_json::json!({"event":"context_destroyed", "ok":ok, "thread":format!("{:?}",std::thread::current().id())})
            );
            if let Some(done) = &self.shutdown {
                let _ = done.send(ok);
            }
        }
    }
}
pub struct EglDriver {
    context: Rc<Context>,
}
pub struct EglResource {
    context: Rc<Context>,
    texture: u32,
    framebuffer: u32,
    image: Handle,
    // Original exporter reference survives descriptor transfer and compositor use.
    original: Option<OwnedFd>,
}
impl Drop for EglResource {
    fn drop(&mut self) {
        let c = &self.context;
        // SAFETY: resources/context are !Send via Rc, and owner context is current
        // for this worker. IDs are either zero or were created by this same API.
        unsafe {
            let mut ok = true;
            if !self.image.is_null() {
                ok &= (c.api.eglDestroyImageKHR)(c.display, self.image) != 0;
            }
            if self.framebuffer != 0 {
                (c.api.glDeleteFramebuffers)(1, &self.framebuffer);
            }
            if self.texture != 0 {
                (c.api.glDeleteTextures)(1, &self.texture);
            }
            ok &= (c.api.glGetError)() == 0;
            println!(
                "{}",
                serde_json::json!({"event":"resource_destroyed","ok":ok})
            );
        }
    }
}
struct Fence {
    context: Rc<Context>,
    sync: Handle,
}
impl Drop for Fence {
    fn drop(&mut self) {
        // SAFETY: owned live fence and its originating context stay on worker.
        let ok =
            unsafe { (self.context.api.eglDestroySyncKHR)(self.context.display, self.sync) } != 0;
        println!("{}", serde_json::json!({"event":"fence_destroyed","ok":ok}));
    }
}
impl EglDriver {
    pub fn new(
        library: &str,
        render_node: &str,
        shutdown: Option<std::sync::mpsc::Sender<bool>>,
    ) -> Result<Self, String> {
        let api = Api::load(library)?;
        let mut c = Context {
            api,
            display: ptr::null_mut(),
            context: ptr::null_mut(),
            initialized: false,
            shutdown,
        };
        // SAFETY: each output array and attribute list is live for the call and
        // correctly sized. Device index is validated before access. Null surfaces
        // require the checked surfaceless extension before make-current.
        unsafe {
            let mut devices = [ptr::null_mut(); 16];
            let mut count = 0;
            require(
                (c.api.eglQueryDevicesEXT)(16, devices.as_mut_ptr(), &mut count) != 0,
                "query devices",
            )?;
            require((0..=16).contains(&count), "device count")?;
            let device = devices[..count as usize]
                .iter()
                .copied()
                .find(|&device| {
                    string((c.api.eglQueryDeviceStringEXT)(device, 0x3377)) == render_node
                })
                .ok_or("requested render device absent")?;
            // Unknown optional device strings can set an EGL error; clear it.
            (c.api.eglGetError)();
            c.display = (c.api.eglGetPlatformDisplayEXT)(0x313f, device, ptr::null());
            require(!c.display.is_null(), "device display")?;
            let (mut major, mut minor) = (0, 0);
            require(
                (c.api.eglInitialize)(c.display, &mut major, &mut minor) != 0,
                "initialize",
            )?;
            c.initialized = true;
            let extensions = string((c.api.eglQueryString)(c.display, 0x3055));
            for needed in [
                "EGL_KHR_surfaceless_context",
                "EGL_KHR_image_base",
                "EGL_KHR_gl_texture_2D_image",
                "EGL_MESA_image_dma_buf_export",
                "EGL_KHR_fence_sync",
            ] {
                require(
                    extensions.split_ascii_whitespace().any(|s| s == needed),
                    needed,
                )?;
            }
            let attrs = [
                0x3033, 1, 0x3040, 0x40, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, NONE,
            ];
            let mut config = ptr::null_mut();
            let mut configs = 0;
            require((c.api.eglBindAPI)(0x30a0) != 0, "bind ES")?;
            require(
                (c.api.eglChooseConfig)(c.display, attrs.as_ptr(), &mut config, 1, &mut configs)
                    != 0
                    && configs == 1,
                "ES3 config",
            )?;
            c.context = (c.api.eglCreateContext)(
                c.display,
                config,
                ptr::null_mut(),
                [0x3098, 3, NONE].as_ptr(),
            );
            require(!c.context.is_null(), "create context")?;
            require(
                (c.api.eglMakeCurrent)(c.display, ptr::null_mut(), ptr::null_mut(), c.context) != 0,
                "make current",
            )?;
            println!(
                "{}",
                serde_json::json!({"event":"rust_context","render_node":render_node,"renderer":string((c.api.glGetString)(0x1f01).cast()),"version":string((c.api.glGetString)(0x1f02).cast()),"thread":format!("{:?}",std::thread::current().id())})
            );
        }
        Ok(Self {
            context: Rc::new(c),
        })
    }
}

/// Diagnostic immutable frame: actual scene rendering is separate future work.
pub struct Quadrants;
impl Driver<Quadrants> for EglDriver {
    type Resource = EglResource;
    fn render(
        &mut self,
        _: &Frame<Quadrants>,
        cancelled: &Cancellation,
    ) -> Result<(EglResource, Descriptor), RenderError> {
        let c = &self.context;
        let mut r = EglResource {
            context: c.clone(),
            texture: 0,
            framebuffer: 0,
            image: ptr::null_mut(),
            original: None,
        };
        // SAFETY: this context is current only on this worker. Fixed 32x32 RGBA
        // readback has exactly 4096 writable bytes, default packing is compatible.
        // Export query uses a four-element array (spec maximum four planes), then
        // requires exactly one before passing single-element fd/stride/offsets.
        unsafe {
            (c.api.glGenTextures)(1, &mut r.texture);
            (c.api.glBindTexture)(TEX, r.texture);
            (c.api.glTexParameteri)(TEX, 0x2801, 0x2600);
            (c.api.glTexParameteri)(TEX, 0x2800, 0x2600);
            (c.api.glTexImage2D)(TEX, 0, 0x8058, 32, 32, 0, 0x1908, 0x1401, ptr::null());
            (c.api.glGenFramebuffers)(1, &mut r.framebuffer);
            (c.api.glBindFramebuffer)(FB, r.framebuffer);
            (c.api.glFramebufferTexture2D)(FB, 0x8ce0, TEX, r.texture, 0);
            require(
                (c.api.glCheckFramebufferStatus)(FB) == 0x8cd5,
                "framebuffer",
            )?;
            (c.api.glEnable)(0x0c11);
            for (i, color) in COLORS.iter().enumerate() {
                (c.api.glScissor)((i as i32 % 2) * 16, (i as i32 / 2) * 16, 16, 16);
                (c.api.glClearColor)(
                    color[0] as f32 / 255.,
                    color[1] as f32 / 255.,
                    color[2] as f32 / 255.,
                    1.,
                );
                (c.api.glClear)(0x4000);
            }
            require((c.api.glGetError)() == 0, "draw")?;
            r.image = (c.api.eglCreateImageKHR)(
                c.display,
                c.context,
                0x30b1,
                r.texture as usize as Handle,
                [0x30bc, 0, 0x30d2, 1, NONE].as_ptr(),
            );
            require(!r.image.is_null(), "image")?;
            let (mut fourcc, mut planes) = (0, 0);
            let mut modifiers = [0u64; 4];
            require(
                (c.api.eglExportDMABUFImageQueryMESA)(
                    c.display,
                    r.image,
                    &mut fourcc,
                    &mut planes,
                    modifiers.as_mut_ptr(),
                ) != 0,
                "export query",
            )?;
            require(
                planes == 1 && fourcc as u32 == u32::from_le_bytes(*b"AB24") && modifiers[0] == 0,
                "one-plane linear AB24",
            )?;
            let (mut fd, mut stride, mut offset) = (-1, 0, 0);
            let exported = (c.api.eglExportDMABUFImageMESA)(
                c.display,
                r.image,
                &mut fd,
                &mut stride,
                &mut offset,
            );
            // SAFETY: a nonnegative exported fd is newly owned by this caller,
            // including an fd returned before a later driver error.
            if fd >= 0 {
                r.original = Some(OwnedFd::from_raw_fd(fd));
            }
            require(
                exported != 0 && r.original.is_some() && stride > 0 && offset >= 0,
                "export descriptor",
            )?;
            let sync = (c.api.eglCreateSyncKHR)(c.display, 0x30f9, ptr::null());
            require(!sync.is_null(), "fence")?;
            let fence = Fence {
                context: c.clone(),
                sync,
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if cancelled.is_cancelled() {
                    return Err(RenderError::Cancelled);
                }
                require(Instant::now() < deadline, "fence deadline")?;
                let status = (c.api.eglClientWaitSyncKHR)(c.display, sync, 1, 5_000_000);
                if status == 0x30f6 {
                    break;
                }
                require(status == 0x30f5, "fence wait")?;
            }
            drop(fence);
            // Validate after the fence, so this test does not rely on readback
            // having already completed the producer before synchronization.
            let mut pixels = [0u8; 4096];
            (c.api.glReadPixels)(0, 0, 32, 32, 0x1908, 0x1401, pixels.as_mut_ptr().cast());
            require((c.api.glGetError)() == 0, "readback")?;
            for y in 0..32 {
                for x in 0..32 {
                    require(
                        pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4]
                            == COLORS[usize::from(x >= 16) + 2 * usize::from(y >= 16)],
                        "pixel mismatch",
                    )?;
                }
            }
            println!(
                "{}",
                serde_json::json!({"event":"rust_producer_verified","rgba_bytes":4096,"readback_after_fence":true})
            );
            let desc = Descriptor {
                fd: r
                    .original
                    .as_ref()
                    .unwrap()
                    .try_clone()
                    .map_err(|e| e.to_string())?,
                width: 32,
                height: 32,
                stride: stride as u32,
                offset: offset as u32,
                fourcc: fourcc as u32,
                modifier: modifiers[0],
            };
            Ok((r, desc))
        }
    }
}
