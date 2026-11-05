//! Checking the user's password: PAM through a small FFI to libpam (the
//! service `aizome` that the NixOS module defines), behind a trait so the
//! lock screen can be tested with a stand-in. Authentication blocks, so it
//! runs on a thread.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

/// Checks a password for a user.
pub trait Authenticator: Send + Sync + 'static {
    /// What PAM answered about `user`'s `password`.
    fn authenticate(&self, user: &str, password: &str) -> Reply;
}

/// What PAM answered, as it answered: the lock screen tells the person
/// from this ([`Reply::verdict`]), not from a note its caller picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The password is the user's, and the account may be used now.
    Accepted,
    /// PAM refused at `step` with `code`. `said` is what its modules told
    /// the conversation (errors and information, in order), and `reason`
    /// is `pam_strerror`'s text for the code.
    Refused { step: Step, code: Code, said: Vec<String>, reason: String },
    /// PAM couldn't be asked: its service didn't start, or the password
    /// can't be passed to it.
    Unasked(String),
}

/// Which of PAM's checks refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// `pam_authenticate`: the password, and modules such as faillock.
    Authenticate,
    /// `pam_acct_mgmt`: whether the account may be used now.
    Account,
}

/// A PAM result code (Linux-PAM's `_pam_types.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code(pub c_int);

impl Code {
    pub const PERM_DENIED: Code = Code(6);
    pub const AUTH_ERR: Code = Code(7);
    pub const CRED_INSUFFICIENT: Code = Code(8);
    pub const AUTHINFO_UNAVAIL: Code = Code(9);
    pub const USER_UNKNOWN: Code = Code(10);
    pub const MAXTRIES: Code = Code(11);
    pub const NEW_AUTHTOK_REQD: Code = Code(12);
    pub const ACCT_EXPIRED: Code = Code(13);
}

/// What to tell the person about a password PAM checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Accepted,
    /// The password is wrong: authentication refused it, and no module
    /// said anything more.
    WrongPassword,
    /// Refused for another reason (a lockout, an expired password or
    /// account, a module or service that failed): what to show.
    Refused(Refusal),
}

/// Why PAM refused other than for a wrong password, in words to show:
/// what its modules said, else what the code means. Only
/// [`Reply::verdict`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    text: String,
}

impl Refusal {
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Reply {
    /// What to tell the person. A refusal is a wrong password only when
    /// authentication refused it (`PAM_AUTH_ERR`) and no module said why:
    /// faillock's lockout is `PAM_AUTH_ERR` too, but faillock says so.
    pub fn verdict(&self) -> Verdict {
        let (step, code, said, reason) = match self {
            Reply::Accepted => return Verdict::Accepted,
            Reply::Unasked(why) => return Verdict::Refused(Refusal { text: why.clone() }),
            Reply::Refused { step, code, said, reason } => (*step, *code, said, reason),
        };
        if !said.is_empty() {
            return Verdict::Refused(Refusal { text: said.join(" ") });
        }
        let text = match (step, code) {
            (Step::Authenticate, Code::AUTH_ERR) => return Verdict::WrongPassword,
            (Step::Authenticate | Step::Account, Code::NEW_AUTHTOK_REQD) => {
                "Your password has expired and must be changed."
            }
            (Step::Authenticate | Step::Account, Code::ACCT_EXPIRED) => "This account has expired.",
            (Step::Authenticate | Step::Account, Code::MAXTRIES) => "Too many tries. Wait, then try again.",
            (Step::Authenticate | Step::Account, _) => reason.as_str(),
        };
        Verdict::Refused(Refusal { text: text.to_owned() })
    }
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
    fn authenticate(&self, _user: &str, password: &str) -> Reply {
        if password == self.0 {
            Reply::Accepted
        } else {
            Reply::Refused {
                step: Step::Authenticate,
                code: Code::AUTH_ERR,
                said: Vec::new(),
                reason: "Authentication failure".into(),
            }
        }
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
    fn authenticate(&self, user: &str, password: &str) -> Reply {
        let Ok(service) = CString::new(self.service.as_str()) else {
            return Reply::Unasked(format!("PAM's service name {:?} holds a NUL", self.service));
        };
        let Ok(user) = CString::new(user) else { return Reply::Unasked("The user name holds a NUL".into()) };
        let Ok(password) = CString::new(password) else {
            return Reply::Unasked("The password can't hold a NUL".into());
        };
        let mut conv = Box::new(Conversation { user, password, said: Vec::new() });
        let pam_conv = PamConv { conv: converse, appdata_ptr: &mut *conv as *mut Conversation as *mut c_void };
        let mut handle: *mut PamHandle = ptr::null_mut();
        // SAFETY: the strings and the conversation outlive the handle,
        // which pam_end releases on every path below.
        unsafe {
            let r = pam_start(service.as_ptr(), conv.user.as_ptr(), &pam_conv, &mut handle);
            if r != PAM_SUCCESS || handle.is_null() {
                return Reply::Unasked(format!("PAM couldn't start the {} service", self.service));
            }
            let mut step = Step::Authenticate;
            let mut r = pam_authenticate(handle, 0);
            if r == PAM_SUCCESS {
                step = Step::Account;
                r = pam_acct_mgmt(handle, 0);
            }
            if r == PAM_SUCCESS {
                // Renew Kerberos tickets and the like; failing here isn't a
                // reason to stay locked.
                let _ = pam_setcred(handle, PAM_REFRESH_CRED);
            }
            let reason = if r == PAM_SUCCESS {
                String::new()
            } else {
                let s = pam_strerror(handle, r);
                if s.is_null() {
                    "Authentication failure".to_owned()
                } else {
                    CStr::from_ptr(s).to_string_lossy().into_owned()
                }
            };
            pam_end(handle, r);
            // Wipe the password's bytes now PAM is done with them.
            let bytes = conv.password.as_ptr() as *mut u8;
            ptr::write_bytes(bytes, 0, conv.password.as_bytes().len());
            if r == PAM_SUCCESS {
                Reply::Accepted
            } else {
                Reply::Refused { step, code: Code(r), said: std::mem::take(&mut conv.said), reason }
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
        assert_eq!(a.authenticate("u", "hunter2"), Reply::Accepted);
        assert_eq!(a.authenticate("u", "nope").verdict(), Verdict::WrongPassword);
    }

    #[test]
    fn pam_refuses_a_service_that_isnt_there() {
        // No such service file: PAM falls back to `other`, which denies (or
        // fails to start); either way it isn't a yes.
        let r = Pam::new("aizome-no-such-service-test").authenticate("nobody-at-all", "x");
        assert_ne!(r.verdict(), Verdict::Accepted, "{r:?}");
    }

    fn refused(step: Step, code: Code, said: &[&str], reason: &str) -> Reply {
        Reply::Refused { step, code, said: said.iter().map(|s| s.to_string()).collect(), reason: reason.into() }
    }

    fn shown(r: Reply) -> String {
        match r.verdict() {
            Verdict::Refused(r) => r.text().to_owned(),
            other @ (Verdict::Accepted | Verdict::WrongPassword) => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_a_silent_authentication_failure_is_a_wrong_password() {
        let wrong = refused(Step::Authenticate, Code::AUTH_ERR, &[], "Authentication failure");
        assert_eq!(wrong.verdict(), Verdict::WrongPassword);
        // faillock refuses with PAM_AUTH_ERR too, and says why.
        let locked = refused(
            Step::Authenticate,
            Code::AUTH_ERR,
            &["The account is locked due to 3 failed logins.", "(10 minutes left to unlock)"],
            "Authentication failure",
        );
        assert_eq!(shown(locked), "The account is locked due to 3 failed logins. (10 minutes left to unlock)");
        // pam_unix's account check, with and without its message.
        let expired = "You are required to change your password immediately (password expired)";
        assert_eq!(shown(refused(Step::Account, Code::NEW_AUTHTOK_REQD, &[expired], "x")), expired);
        assert_eq!(
            shown(refused(Step::Account, Code::NEW_AUTHTOK_REQD, &[], "Authentication token is no longer valid")),
            "Your password has expired and must be changed."
        );
        assert_eq!(shown(refused(Step::Account, Code::ACCT_EXPIRED, &[], "x")), "This account has expired.");
        // PAM_AUTH_ERR from the account check isn't a wrong password.
        assert_eq!(
            shown(refused(Step::Account, Code::AUTH_ERR, &[], "Authentication failure")),
            "Authentication failure"
        );
        // Anything else: pam_strerror's words.
        let down = "Authentication service cannot retrieve authentication info";
        assert_eq!(shown(refused(Step::Authenticate, Code::AUTHINFO_UNAVAIL, &[], down)), down);
        assert_eq!(shown(Reply::Unasked("The password can't hold a NUL".into())), "The password can't hold a NUL");
    }

    #[test]
    fn the_current_user_has_a_name() {
        let (login, full) = current_user();
        assert!(!login.is_empty() && !full.is_empty());
    }
}
