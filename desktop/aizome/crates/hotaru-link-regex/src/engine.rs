//! Checked ownership boundary for the exact Oniguruma 6.9.9 dependency.
#![allow(unsafe_code)]
use crate::{Error, Limit};
use std::{
    ffi::{c_int, c_uint, c_ulong, c_void},
    marker::PhantomData,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{Mutex, OnceLock},
};
#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}
#[repr(C)]
struct Region {
    allocated: c_int,
    count: c_int,
    begins: *mut c_int,
    ends: *mut c_int,
    history: *mut c_void,
}
const _: () = {
    assert!(std::mem::offset_of!(Region, count) == 4);
    assert!(std::mem::offset_of!(Region, begins) == 8);
    assert!(std::mem::size_of::<Region>() == 8 + 3 * std::mem::size_of::<*mut c_void>());
};
unsafe extern "C" {
    static mut OnigEncodingUTF8: Opaque;
    static mut OnigDefaultSyntax: *mut Opaque;
    fn onig_initialize(encodings: *mut *mut Opaque, count: c_int) -> c_int;
    fn onig_new(
        out: *mut *mut Opaque,
        start: *const u8,
        end: *const u8,
        options: c_uint,
        encoding: *mut Opaque,
        syntax: *mut Opaque,
        error: *mut c_void,
    ) -> c_int;
    fn onig_free(regex: *mut Opaque);
    fn onig_region_free(region: *mut Region, free_self: c_int);
    fn onig_new_match_param() -> *mut Opaque;
    fn onig_free_match_param(param: *mut Opaque);
    fn onig_set_retry_limit_in_search_of_match_param(param: *mut Opaque, limit: c_ulong) -> c_int;
    fn onig_set_match_stack_limit_size_of_match_param(param: *mut Opaque, limit: c_uint) -> c_int;
    fn onig_search_with_param(
        regex: *mut Opaque,
        start: *const u8,
        end: *const u8,
        from: *const u8,
        to: *const u8,
        region: *mut Region,
        options: c_uint,
        param: *mut Opaque,
    ) -> c_int;
}
static INIT: OnceLock<Result<(), i32>> = OnceLock::new();
static COMPILE: Mutex<()> = Mutex::new(());
struct Param(NonNull<Opaque>);
impl Drop for Param {
    fn drop(&mut self) {
        // SAFETY: checked unique allocation from onig_new_match_param, freed once.
        unsafe { onig_free_match_param(self.0.as_ptr()) }
    }
}
pub(crate) struct Engine {
    regex: NonNull<Opaque>,
    region: Region,
    // Engine/region ownership stays on its constructing worker; no Send/Sync assertion.
    _thread: PhantomData<Rc<()>>,
}
impl Engine {
    pub(crate) fn new() -> Result<Self, Error> {
        let result = INIT.get_or_init(|| {
            // SAFETY: OnceLock serializes global initialization. The array contains
            // the exact library's static UTF8 encoding; it is borrowed for this call.
            // Global tables live until process exit; we never call onig_end.
            let code = unsafe {
                let mut enc = [&raw mut OnigEncodingUTF8];
                onig_initialize(enc.as_mut_ptr(), 1)
            };
            if code == 0 { Ok(()) } else { Err(code) }
        });
        result.map_err(Error::Engine)?;
        let _compile = COMPILE.lock().unwrap_or_else(|p| p.into_inner());
        let pattern = crate::DEFAULT_REGEX.as_bytes();
        let mut raw = ptr::null_mut();
        // SAFETY: fixed trusted pattern is a live UTF8 slice, end is one-past.
        // Output has pointer storage; error-info NULL is explicitly allowed.
        // Initialized process globals are never changed by this crate. Compilation
        // is serialized because the engine can mutate global intern tables.
        let code = unsafe {
            onig_new(
                &mut raw,
                pattern.as_ptr(),
                pattern.as_ptr().add(pattern.len()),
                0,
                &raw mut OnigEncodingUTF8,
                OnigDefaultSyntax,
                ptr::null_mut(),
            )
        };
        // onig_new cleans and nulls its output on error (verified exact regcomp.c).
        if code != 0 {
            return Err(Error::Engine(code));
        }
        let regex = NonNull::new(raw).ok_or(Error::EngineContract)?;
        Ok(Self {
            regex,
            region: Region {
                allocated: 0,
                count: 0,
                begins: ptr::null_mut(),
                ends: ptr::null_mut(),
                history: ptr::null_mut(),
            },
            _thread: PhantomData,
        })
    }
    pub(crate) fn search(
        &mut self,
        text: &str,
        retries: u32,
        stack_entries: u32,
    ) -> Result<Option<std::ops::Range<usize>>, Error> {
        if text.len() > crate::MAX_BYTES || retries == 0 || !(512..=16_384).contains(&stack_entries)
        {
            return Err(Error::InvalidInput);
        }
        // SAFETY: constructor returns an owned allocation or NULL. We check NULL
        // before every initialization/use; it already initializes the structure.
        let param =
            Param(NonNull::new(unsafe { onig_new_match_param() }).ok_or(Error::Allocation)?);
        // SAFETY: unique live param and finite positive limits, no global mutation.
        unsafe {
            for code in [
                onig_set_retry_limit_in_search_of_match_param(param.0.as_ptr(), retries.into()),
                onig_set_match_stack_limit_size_of_match_param(param.0.as_ptr(), stack_entries),
            ] {
                if code != 0 {
                    return Err(Error::Engine(code));
                }
            }
        }
        let start = text.as_ptr();
        // SAFETY: length checked before pointer arithmetic, borrowed slice remains
        // valid until return, and all endpoints are start or one-past. Region is an
        // exclusively borrowed initialized C-layout value; engine owns only its
        // internal arrays. Rust never converts C allocations into Rust containers.
        let code = unsafe {
            let end = start.add(text.len());
            onig_search_with_param(
                self.regex.as_ptr(),
                start,
                end,
                start,
                end,
                &mut self.region,
                0,
                param.0.as_ptr(),
            )
        };
        if code == -1 {
            return Ok(None);
        }
        if matches!(code, -15 | -17 | -18 | -19) {
            return Err(Error::Budget(Limit::Engine));
        }
        if code < 0 {
            return Err(Error::Engine(code));
        }
        if self.region.count < 1
            || self.region.allocated < self.region.count
            || self.region.begins.is_null()
            || self.region.ends.is_null()
        {
            return Err(Error::EngineContract);
        }
        // SAFETY: successful engine search guarantees count allocated entries. We
        // checked nonnull and count; only entry zero is read while exclusively owned.
        let (a, b) = unsafe { (*self.region.begins, *self.region.ends) };
        let start = usize::try_from(a).map_err(|_| Error::EngineContract)?;
        let end = usize::try_from(b).map_err(|_| Error::EngineContract)?;
        if start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
            || start != code as usize
        {
            return Err(Error::EngineContract);
        }
        Ok(Some(start..end))
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: both allocations are uniquely owned; free_self=0 releases only
        // region internals, never the Rust struct. Global tables are not terminated.
        unsafe {
            onig_region_free(&mut self.region, 0);
            onig_free(self.regex.as_ptr());
        }
    }
}
