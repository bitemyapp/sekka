//! Small versioned C ABI: viewers need not link the compositor or toolkit.
//! All handles are thread-confined. Returned storage is borrowed until the
//! next paint/reset/free call; the viewer uploads only the damage rectangles.
use sidestep_engine::remote::Player;
use std::ffi::{CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

struct Renderer {
    player: Player,
    error: CString,
}

#[repr(C)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rgba: *const u8,
    pub bytes: usize,
    /// x,y,width,height in physical canvas pixels, four u32s per rectangle.
    pub damage: *const u32,
    pub damage_count: usize,
}

#[unsafe(no_mangle)]
pub extern "C" fn aizome_remote_renderer_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn aizome_remote_renderer_new() -> *mut c_void {
    Box::into_raw(Box::new(Renderer { player: Player::default(), error: CString::default() })).cast()
}

/// # Safety
/// `handle` must be null or an exclusively owned live handle from `new`, and
/// must not be used again after this call. No other call may overlap it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aizome_remote_renderer_free(handle: *mut c_void) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe { drop(Box::from_raw(handle.cast::<Renderer>())) }));
    }
}

/// # Safety
/// `handle` is a live handle owned by this thread. `data` names `len` readable
/// bytes for this call. No borrowed image storage may be used concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aizome_remote_renderer_paint(handle: *mut c_void, data: *const u8, len: usize) -> i32 {
    if handle.is_null() || data.is_null() || len > aizome_remote_protocol::MAX_FRAME_BYTES {
        return -1;
    }
    let renderer = unsafe { &mut *handle.cast::<Renderer>() };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match catch_unwind(AssertUnwindSafe(|| renderer.player.frame(bytes))) {
        Ok(Ok(())) => {
            renderer.error = CString::default();
            0
        }
        failure => {
            let error = match failure {
                Ok(Err(e)) => e,
                _ => "retained renderer failed".into(),
            };
            renderer.error = CString::new(error.replace('\0', " ")).unwrap_or_default();
            -1
        }
    }
}

/// # Safety
/// `handle` is a live handle owned by this thread, and `view` is writable.
/// Returned pointers are valid only until the next paint or free call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aizome_remote_renderer_view(handle: *mut c_void, view: *mut View) -> i32 {
    if handle.is_null() || view.is_null() {
        return -1;
    }
    let p = unsafe { &(*handle.cast::<Renderer>()).player };
    if p.pixels.is_empty() {
        return -1;
    }
    unsafe {
        view.write(View {
            width: p.width,
            height: p.height,
            stride: p.width * 4,
            rgba: p.pixels.as_ptr().cast(),
            bytes: p.pixels.len() * 4,
            damage: p.damage.as_ptr().cast(),
            damage_count: p.damage.len(),
        })
    };
    0
}

/// # Safety
/// `handle` is live on this thread. The string is borrowed until paint/free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aizome_remote_renderer_error(handle: *mut c_void) -> *const c_char {
    if handle.is_null() {
        return c"retained renderer unavailable".as_ptr();
    }
    unsafe { (*handle.cast::<Renderer>()).error.as_ptr() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_reports_invalid_frames_without_unwinding() {
        unsafe {
            let handle = aizome_remote_renderer_new();
            assert_eq!(aizome_remote_renderer_version(), 1);
            assert_eq!(aizome_remote_renderer_paint(handle, b"{}".as_ptr(), 2), -1);
            assert!(!std::ffi::CStr::from_ptr(aizome_remote_renderer_error(handle)).to_bytes().is_empty());
            aizome_remote_renderer_free(handle);
            aizome_remote_renderer_free(std::ptr::null_mut());
        }
    }
}
