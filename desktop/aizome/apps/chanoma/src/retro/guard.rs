//! A core's calls, its C++ exceptions caught (`guard.cpp`): one let out of
//! a core ends its game, with what the exception said, rather than
//! aborting Chanoma (Rust can't unwind a foreign exception). Play! throws
//! out of `retro_run` when it can't boot a disc.

use std::ffi::{CStr, c_char, c_void};

use super::ffi::retro_game_info;

unsafe extern "C" {
    fn chanoma_guard_void(f: unsafe extern "C" fn(), why: *mut c_char, room: usize) -> i32;
    fn chanoma_guard_load(
        f: unsafe extern "C" fn(*const retro_game_info) -> bool,
        game: *const retro_game_info,
        ok: *mut bool,
        why: *mut c_char,
        room: usize,
    ) -> i32;
    fn chanoma_guard_state(
        f: unsafe extern "C" fn(*mut c_void, usize) -> bool,
        data: *mut c_void,
        size: usize,
        ok: *mut bool,
        why: *mut c_char,
        room: usize,
    ) -> i32;
}

const ROOM: usize = 512;

fn said(why: &[c_char; ROOM]) -> String {
    // SAFETY: guard.cpp ends what it writes with a NUL, within the room.
    let s = unsafe { CStr::from_ptr(why.as_ptr()) }.to_string_lossy().trim().to_owned();
    if s.is_empty() { "an exception".into() } else { s }
}

/// Call a core's `f` (`retro_run`, `retro_reset`, …); `Err` with what it
/// threw.
///
/// # Safety
///
/// `f` is a loaded core's entry point, which may be called now.
pub unsafe fn call(f: unsafe extern "C" fn()) -> Result<(), String> {
    let mut why = [0; ROOM];
    // SAFETY: as promised; `why` is ROOM long.
    match unsafe { chanoma_guard_void(f, why.as_mut_ptr(), ROOM) } {
        1 => Ok(()),
        _ => Err(said(&why)),
    }
}

/// `retro_load_game`: whether it loaded, or what it threw.
///
/// # Safety
///
/// As [`call`]; `game` and what it points to outlive the loaded game.
pub unsafe fn load(
    f: unsafe extern "C" fn(*const retro_game_info) -> bool,
    game: &retro_game_info,
) -> Result<bool, String> {
    let mut why = [0; ROOM];
    let mut ok = false;
    // SAFETY: as promised.
    match unsafe { chanoma_guard_load(f, game, &mut ok, why.as_mut_ptr(), ROOM) } {
        1 => Ok(ok),
        _ => Err(said(&why)),
    }
}

/// `retro_serialize` into `data`, or `retro_unserialize` from it (whose
/// pointer is a const one, the same to call): whether it did, or what it
/// threw.
///
/// # Safety
///
/// As [`call`]; `data` is `size` bytes the call may read or write.
pub unsafe fn state(
    f: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    data: *mut c_void,
    size: usize,
) -> Result<bool, String> {
    let mut why = [0; ROOM];
    let mut ok = false;
    // SAFETY: as promised.
    match unsafe { chanoma_guard_state(f, data, size, &mut ok, why.as_mut_ptr(), ROOM) } {
        1 => Ok(ok),
        _ => Err(said(&why)),
    }
}
