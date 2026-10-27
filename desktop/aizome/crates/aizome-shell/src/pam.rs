//! Checking the user's password: PAM through a small FFI to libpam (the
//! service `aizome` that the NixOS module defines), behind a trait so the
//! lock screen can be tested with a stand-in. Authentication blocks, so it
//! runs on a thread.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

/// Checks a password for a user.
pub trait Authenticator: Send + Sync + 'static {
    /// `Ok` when `password` is `user`'s; else what PAM said.
    fn authenticate(&self, user: &str, password: &str) -> Result<(), String>;
}

/// PAM, with a service name (`aizome`).
#[derive(Clone, Debug)]
pub struct Pam {
    pub service: String,
}

impl Pam {
    pub fn new(service: impl Into<String>) -> Pam {
        Pam { service: service.into() }
    }
}

/// A stand-in that knows one password (tests and `--demo`).
#[derive(Clone, Debug)]
pub struct Fixed(pub String);

impl Authenticator for Fixed {
    fn authenticate(&self, _user: &str, password: &str) -> Result<(), String> {
        if password == self.0 { Ok(()) } else { Err("Authentication failure".into()) }
    }
}

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_REFRESH_CRED: c_int = 0x0010;

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, pamh: *mut *mut PamHandle)
    -> c_int;
    fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_setcred(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut PamHandle, errnum: c_int) -> *const c_char;
}

/// What the conversation answers with, and what PAM said.
struct Conversation {
    user: CString,
    password: CString,
    said: Vec<String>,
}

extern "C" fn converse(
    n: c_int,
    msgs: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if n <= 0 || msgs.is_null() || resp.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: PAM hands back the pointer given to pam_start.
    let conv = unsafe { &mut *(data as *mut Conversation) };
    // SAFETY: calloc'd for PAM, which frees it (and each answer).
    let answers = unsafe { libc::calloc(n as usize, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    if answers.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..n as usize {
        // SAFETY: `msgs` has `n` entries (Linux-PAM's layout: an array of
        // pointers).
        let m = unsafe { &**msgs.add(i) };
        let text = if m.msg.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(m.msg) }.to_string_lossy().into_owned()
        };
        let reply: Option<&CStr> = match m.msg_style {
            PAM_PROMPT_ECHO_OFF => Some(&conv.password),
            PAM_PROMPT_ECHO_ON => Some(&conv.user),
            PAM_ERROR_MSG | PAM_TEXT_INFO => {
                if !text.is_empty() {
                    conv.said.push(text);
                }
                None
            }
            _ => None,
        };
        if let Some(r) = reply {
            // SAFETY: strdup'd for PAM to free.
            let s = unsafe { libc::strdup(r.as_ptr()) };
            if s.is_null() {
                for j in 0..i {
                    unsafe {
                        let a = &mut *answers.add(j);
                        if !a.resp.is_null() {
                            libc::free(a.resp as *mut c_void);
                        }
                    }
                }
                unsafe { libc::free(answers as *mut c_void) };
                return PAM_BUF_ERR;
            }
            unsafe { (*answers.add(i)).resp = s };
        }
    }
    unsafe { *resp = answers };
    PAM_SUCCESS
}

impl Authenticator for Pam {
    fn authenticate(&self, user: &str, password: &str) -> Result<(), String> {
        let service = CString::new(self.service.as_str()).map_err(|e| e.to_string())?;
        let mut conv = Box::new(Conversation {
            user: CString::new(user).map_err(|e| e.to_string())?,
            password: CString::new(password).map_err(|_| "The password can't hold a NUL".to_string())?,
            said: Vec::new(),
        });
        let pam_conv = PamConv { conv: converse, appdata_ptr: &mut *conv as *mut Conversation as *mut c_void };
        let mut handle: *mut PamHandle = ptr::null_mut();
        // SAFETY: the strings and the conversation outlive the handle,
        // which pam_end releases on every path below.
        unsafe {
            let r = pam_start(service.as_ptr(), conv.user.as_ptr(), &pam_conv, &mut handle);
            if r != PAM_SUCCESS || handle.is_null() {
                return Err(format!("PAM couldn't start the {} service", self.service));
            }
            let mut r = pam_authenticate(handle, 0);
            if r == PAM_SUCCESS {
                r = pam_acct_mgmt(handle, 0);
            }
            if r == PAM_SUCCESS {
                // Renew Kerberos tickets and the like; failing here isn't a
                // reason to stay locked.
                let _ = pam_setcred(handle, PAM_REFRESH_CRED);
            }
            let err = if r == PAM_SUCCESS {
                None
            } else {
                let s = pam_strerror(handle, r);
                Some(if s.is_null() {
                    "Authentication failure".to_owned()
                } else {
                    CStr::from_ptr(s).to_string_lossy().into_owned()
                })
            };
            pam_end(handle, r);
            // Wipe the password's bytes now PAM is done with them.
            let bytes = conv.password.as_ptr() as *mut u8;
            ptr::write_bytes(bytes, 0, conv.password.as_bytes().len());
            match err {
                None => Ok(()),
                Some(e) => Err(conv.said.last().cloned().unwrap_or(e)),
            }
        }
    }
}

/// The user this shell runs for: login name and full name.
pub fn current_user() -> (String, String) {
    // SAFETY: getpwuid_r writes into the buffers given.
    unsafe {
        let uid = libc::getuid();
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as c_char; 4096];
        let mut out: *mut libc::passwd = ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
            let name = CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned();
            let gecos = if pw.pw_gecos.is_null() {
                String::new()
            } else {
                CStr::from_ptr(pw.pw_gecos).to_string_lossy().into_owned()
            };
            let full = gecos.split(',').next().unwrap_or("").trim().to_owned();
            return (name.clone(), if full.is_empty() { name } else { full });
        }
    }
    let name = std::env::var("USER").unwrap_or_else(|_| "user".into());
    (name.clone(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_knows_its_password() {
        let a = Fixed("hunter2".into());
        assert!(a.authenticate("u", "hunter2").is_ok());
        assert_eq!(a.authenticate("u", "nope"), Err("Authentication failure".into()));
    }

    #[test]
    fn pam_refuses_a_service_that_isnt_there() {
        // No such service file: PAM falls back to `other`, which denies (or
        // fails to start); either way it isn't a yes.
        let r = Pam::new("aizome-no-such-service-test").authenticate("nobody-at-all", "x");
        assert!(r.is_err());
    }

    #[test]
    fn the_current_user_has_a_name() {
        let (login, full) = current_user();
        assert!(!login.is_empty() && !full.is_empty());
    }
}
