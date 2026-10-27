//! greetd's IPC: JSON messages, each after its length (a native-endian
//! u32), over the Unix socket at `$GREETD_SOCK`. The greeter asks greetd to
//! open a PAM conversation for a user, answers its questions, and asks it
//! to start the session; greetd does the rest as root. See greetd-ipc(7).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

/// What the greeter asks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Begin logging `username` in: greetd opens a PAM conversation.
    CreateSession { username: String },
    /// The answer to the last `AuthMessage` (`None` for an info or error
    /// one, which only needs acknowledging).
    PostAuthMessageResponse { response: Option<String> },
    /// Logged in: start `cmd` with `env` once the greeter has exited.
    StartSession { cmd: Vec<String>, env: Vec<String> },
    /// Give up on this login (another user was picked).
    CancelSession,
}

/// What greetd answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error { error_type: ErrorType, description: String },
    AuthMessage { auth_message_type: AuthMessageType, auth_message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    /// The answers didn't authenticate (a wrong password).
    AuthError,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessageType {
    /// A question whose answer shows as it's typed (a user name, a code).
    Visible,
    /// A question whose answer doesn't show (a password).
    Secret,
    /// Something to tell the person; acknowledge it.
    Info,
    /// Something wrong to tell the person; acknowledge it.
    Error,
}

/// Write one message: its length, then its JSON.
pub fn write_msg<T: Serialize>(w: &mut impl Write, msg: &T) -> std::io::Result<()> {
    let mut body = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let len = u32::try_from(body.len()).map_err(std::io::Error::other)?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&len.to_ne_bytes());
    frame.extend_from_slice(&body);
    let result = w.write_all(&frame);
    // A response to a secret question is in there: don't leave it behind.
    zero(&mut frame);
    zero(&mut body);
    result
}

/// Read one message.
pub fn read_msg<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> std::io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_ne_bytes(len) as usize;
    // greetd's messages are small; anything this big is not one of them.
    if len > 1 << 20 {
        return Err(std::io::Error::other(format!("a {len}-byte message from greetd")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(std::io::Error::other)
}

/// Overwrite bytes that held a secret before they're freed.
pub fn zero(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // Volatile, so the writes aren't elided as dead stores.
        // Safety: `b` is a valid, aligned, exclusive reference.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Zero a string's bytes, then empty it.
pub fn zero_string(s: &mut String) {
    // Safety: zeros are valid UTF-8, and the string is emptied after.
    zero(unsafe { s.as_bytes_mut() });
    s.clear();
}

/// A connection to greetd.
pub struct Greetd {
    stream: UnixStream,
}

impl Greetd {
    /// Connect to `$GREETD_SOCK`.
    pub fn connect() -> std::io::Result<Greetd> {
        let path = std::env::var_os("GREETD_SOCK")
            .ok_or_else(|| std::io::Error::other("GREETD_SOCK isn't set: not started by greetd"))?;
        Ok(Greetd { stream: UnixStream::connect(path)? })
    }

    pub fn from_stream(stream: UnixStream) -> Greetd {
        Greetd { stream }
    }

    /// Ask, and wait for the answer.
    pub fn ask(&mut self, request: &Request) -> std::io::Result<Response> {
        write_msg(&mut self.stream, request)?;
        read_msg(&mut self.stream)
    }
}

/// A stand-in for greetd (`aizome-greeter --demo`, to look at it inside
/// another session): anyone, with the password `aizome`.
#[derive(Default)]
pub struct Demo;

impl Demo {
    pub fn ask(&mut self, request: &Request) -> std::io::Result<Response> {
        Ok(match request {
            Request::CreateSession { .. } => {
                Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: "Password: ".into() }
            }
            Request::PostAuthMessageResponse { response: Some(p) } if p == "aizome" => Response::Success,
            Request::PostAuthMessageResponse { response: Some(_) } => {
                Response::Error { error_type: ErrorType::AuthError, description: "pam_authenticate: AUTH_ERR".into() }
            }
            Request::PostAuthMessageResponse { response: None }
            | Request::StartSession { .. }
            | Request::CancelSession => Response::Success,
        })
    }

    pub fn into_fn(mut self) -> impl FnMut(&Request) -> std::io::Result<Response> {
        move |r| self.ask(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_greetds_json() {
        let j = |r: &Request| serde_json::to_value(r).unwrap();
        assert_eq!(
            j(&Request::CreateSession { username: "alice".into() }),
            serde_json::json!({"type": "create_session", "username": "alice"})
        );
        assert_eq!(
            j(&Request::PostAuthMessageResponse { response: Some("pw".into()) }),
            serde_json::json!({"type": "post_auth_message_response", "response": "pw"})
        );
        assert_eq!(
            j(&Request::PostAuthMessageResponse { response: None }),
            serde_json::json!({"type": "post_auth_message_response", "response": null})
        );
        assert_eq!(
            j(&Request::StartSession { cmd: vec!["aizome-session".into()], env: vec!["A=b".into()] }),
            serde_json::json!({"type": "start_session", "cmd": ["aizome-session"], "env": ["A=b"]})
        );
        assert_eq!(j(&Request::CancelSession), serde_json::json!({"type": "cancel_session"}));
        let r = |s: &str| serde_json::from_str::<Response>(s).unwrap();
        assert_eq!(r(r#"{"type":"success"}"#), Response::Success);
        assert_eq!(
            r(r#"{"type":"error","error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"}"#),
            Response::Error { error_type: ErrorType::AuthError, description: "pam_authenticate: AUTH_ERR".into() }
        );
        assert_eq!(
            r(r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#),
            Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: "Password: ".into() }
        );
    }

    #[test]
    fn a_message_is_its_length_then_its_json() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &Request::CancelSession).unwrap();
        let body = br#"{"type":"cancel_session"}"#;
        assert_eq!(&buf[..4], &(body.len() as u32).to_ne_bytes());
        assert_eq!(&buf[4..], body);
        let back: Request = read_msg(&mut &buf[..]).unwrap();
        assert_eq!(back, Request::CancelSession);
        // Too long to be greetd's.
        let mut huge = (2u32 << 20).to_ne_bytes().to_vec();
        huge.extend_from_slice(b"{}");
        assert!(read_msg::<Response>(&mut &huge[..]).is_err());
    }

    #[test]
    fn a_secret_is_zeroed() {
        let mut s = String::from("hunter2");
        let ptr = s.as_ptr();
        let cap = s.capacity();
        zero_string(&mut s);
        assert!(s.is_empty());
        // Safety: the buffer is still the string's, allocated, `cap` long.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, cap.min(7)) };
        assert!(bytes.iter().all(|b| *b == 0));
    }
}
