// SPDX-License-Identifier: GPL-3.0-or-later
//! libxpc, declared by hand for the dozen calls Sekka uses.
use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};

use block2::{Block, RcBlock};
use zeroize::Zeroizing;

use crate::{MESSAGE_LIMIT, PASSWORD_KEY, Peer, REPLY_TIMEOUT, REQUEST_KEY, RESPONSE_KEY, XpcError};

#[allow(non_camel_case_types)]
type xpc_object_t = *mut c_void;
#[allow(non_camel_case_types)]
type xpc_type_t = *const c_void;
#[allow(non_camel_case_types)]
type dispatch_queue_t = *mut c_void;

const XPC_CONNECTION_MACH_SERVICE_LISTENER: u64 = 1 << 0;
const XPC_CONNECTION_MACH_SERVICE_PRIVILEGED: u64 = 1 << 1;

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

unsafe extern "C" {
    static _xpc_type_connection: Opaque;
    static _xpc_type_dictionary: Opaque;
    static _xpc_type_error: Opaque;

    fn xpc_connection_create_mach_service(
        name: *const c_char,
        targetq: dispatch_queue_t,
        flags: u64,
    ) -> xpc_object_t;
    fn xpc_connection_set_event_handler(connection: xpc_object_t, handler: &Block<dyn Fn(xpc_object_t)>);
    fn xpc_connection_set_peer_code_signing_requirement(
        connection: xpc_object_t,
        requirement: *const c_char,
    ) -> c_int;
    fn xpc_connection_resume(connection: xpc_object_t);
    fn xpc_connection_cancel(connection: xpc_object_t);
    fn xpc_connection_get_euid(connection: xpc_object_t) -> libc::uid_t;
    fn xpc_connection_get_pid(connection: xpc_object_t) -> libc::pid_t;
    fn xpc_connection_send_message(connection: xpc_object_t, message: xpc_object_t);
    fn xpc_connection_send_message_with_reply(
        connection: xpc_object_t,
        message: xpc_object_t,
        replyq: dispatch_queue_t,
        handler: &Block<dyn Fn(xpc_object_t)>,
    );
    fn xpc_dictionary_create(
        keys: *const *const c_char,
        values: *const xpc_object_t,
        count: usize,
    ) -> xpc_object_t;
    fn xpc_dictionary_create_reply(original: xpc_object_t) -> xpc_object_t;
    fn xpc_dictionary_get_remote_connection(dictionary: xpc_object_t) -> xpc_object_t;
    fn xpc_dictionary_set_data(dictionary: xpc_object_t, key: *const c_char, bytes: *const c_void, length: usize);
    fn xpc_dictionary_get_data(dictionary: xpc_object_t, key: *const c_char, length: *mut usize) -> *const c_void;
    fn xpc_get_type(object: xpc_object_t) -> xpc_type_t;
    fn xpc_retain(object: xpc_object_t) -> xpc_object_t;
    fn xpc_release(object: xpc_object_t);
    fn dispatch_main() -> !;
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> dispatch_queue_t;
}

fn type_of(object: xpc_object_t) -> xpc_type_t {
    // SAFETY: `object` is a live XPC object handed to us by libxpc.
    unsafe { xpc_get_type(object) }
}
fn is(object: xpc_object_t, kind: &Opaque) -> bool {
    type_of(object) == kind as *const Opaque as xpc_type_t
}

fn key(name: &str) -> CString {
    CString::new(name).expect("keys have no NUL")
}

/// Bytes stored under `name`, if present and within the limit. The pointer
/// libxpc returns lives as long as the dictionary, so it is copied at once.
fn data(dictionary: xpc_object_t, name: &str) -> Option<Zeroizing<Vec<u8>>> {
    let k = key(name);
    let mut length = 0usize;
    // SAFETY: `dictionary` is a live dictionary and `k` a NUL-terminated key.
    let bytes = unsafe { xpc_dictionary_get_data(dictionary, k.as_ptr(), &mut length) };
    if bytes.is_null() || length > MESSAGE_LIMIT {
        return None;
    }
    // SAFETY: libxpc guarantees `length` readable bytes at `bytes`.
    let slice = unsafe { std::slice::from_raw_parts(bytes as *const u8, length) };
    Some(Zeroizing::new(slice.to_vec()))
}

fn set_data(dictionary: xpc_object_t, name: &str, bytes: &[u8]) {
    let k = key(name);
    // SAFETY: libxpc copies `bytes` into the dictionary.
    unsafe { xpc_dictionary_set_data(dictionary, k.as_ptr(), bytes.as_ptr() as *const c_void, bytes.len()) };
}

type Handler = dyn Fn(Peer, &[u8], Option<Zeroizing<Vec<u8>>>) -> Vec<u8> + Send + Sync;

/// Run the helper's listener forever. Every peer must meet `requirement`;
/// each request is answered by `handler` on libxpc's queue for that peer,
/// so one slow request doesn't hold up another peer.
pub fn listen(
    service: &str,
    requirement: &str,
    handler: impl Fn(Peer, &[u8], Option<Zeroizing<Vec<u8>>>) -> Vec<u8> + Send + Sync + 'static,
) -> ! {
    let handler: Arc<Handler> = Arc::new(handler);
    let name = CString::new(service).expect("service name");
    let requirement = CString::new(requirement).expect("requirement");
    // SAFETY: creating a listener for our own launchd service name.
    let listener = unsafe {
        xpc_connection_create_mach_service(name.as_ptr(), ptr::null_mut(), XPC_CONNECTION_MACH_SERVICE_LISTENER)
    };
    let on_peer = RcBlock::new(move |peer: xpc_object_t| {
        if !is(peer, unsafe { &_xpc_type_connection }) {
            return;
        }
        // SAFETY: `peer` is a new connection we retain for its handler.
        let peer = unsafe { xpc_retain(peer) };
        // Refuse anything not signed as Sekka Installer before it can send.
        // SAFETY: `peer` is a live connection that hasn't been resumed.
        if unsafe { xpc_connection_set_peer_code_signing_requirement(peer, requirement.as_ptr()) } != 0 {
            unsafe {
                xpc_connection_cancel(peer);
                xpc_release(peer);
            }
            return;
        }
        let handler = handler.clone();
        let connection = peer as usize;
        let on_message = RcBlock::new(move |message: xpc_object_t| {
            let peer = connection as xpc_object_t;
            if !is(message, unsafe { &_xpc_type_dictionary }) {
                if is(message, unsafe { &_xpc_type_error }) {
                    // The peer went away or failed the code-signing check.
                }
                return;
            }
            let Some(request) = data(message, REQUEST_KEY) else {
                return;
            };
            let password = data(message, PASSWORD_KEY);
            // SAFETY: `peer` is the connection this message arrived on.
            let who = Peer {
                uid: unsafe { xpc_connection_get_euid(peer) },
                pid: unsafe { xpc_connection_get_pid(peer) },
            };
            let response = handler(who, &request, password);
            // SAFETY: replying on the message's own connection.
            unsafe {
                let reply = xpc_dictionary_create_reply(message);
                if reply.is_null() {
                    return;
                }
                set_data(reply, RESPONSE_KEY, &response);
                xpc_connection_send_message(xpc_dictionary_get_remote_connection(message), reply);
                xpc_release(reply);
            }
        });
        // SAFETY: libxpc copies the block; `peer` stays retained for the
        // connection's life.
        unsafe {
            xpc_connection_set_event_handler(peer, &on_message);
            xpc_connection_resume(peer);
        }
    });
    // SAFETY: the listener is ours; libxpc copies the block.
    unsafe {
        xpc_connection_set_event_handler(listener, &on_peer);
        xpc_connection_resume(listener);
        dispatch_main()
    }
}

/// The app's connection to the helper.
pub struct Client {
    connection: usize,
    /// Cancelled already, when a reply took too long.
    cancelled: AtomicBool,
}

// The connection is used from one worker thread at a time; libxpc
// connections are themselves thread-safe.
unsafe impl Send for Client {}
unsafe impl Sync for Client {}

impl Client {
    pub fn connect(service: &str, requirement: &str) -> Result<Self, XpcError> {
        let name = CString::new(service).map_err(|_| XpcError::Unavailable)?;
        let requirement = CString::new(requirement).map_err(|_| XpcError::Untrusted)?;
        // SAFETY: connecting to a privileged mach service by name.
        let connection = unsafe {
            xpc_connection_create_mach_service(name.as_ptr(), ptr::null_mut(), XPC_CONNECTION_MACH_SERVICE_PRIVILEGED)
        };
        if connection.is_null() {
            return Err(XpcError::Unavailable);
        }
        // SAFETY: the connection is new and not yet resumed.
        if unsafe { xpc_connection_set_peer_code_signing_requirement(connection, requirement.as_ptr()) } != 0 {
            unsafe { xpc_release(connection) };
            return Err(XpcError::Untrusted);
        }
        let ignore = RcBlock::new(|_event: xpc_object_t| {});
        // SAFETY: as above.
        unsafe {
            xpc_connection_set_event_handler(connection, &ignore);
            xpc_connection_resume(connection);
        }
        Ok(Self {
            connection: connection as usize,
            cancelled: AtomicBool::new(false),
        })
    }

    /// Send one request and wait for its reply, at most [`REPLY_TIMEOUT`].
    /// Blocks: call it from a worker thread, never the main thread.
    pub fn send(&self, request: &[u8], password: Option<&[u8]>) -> Result<Vec<u8>, XpcError> {
        // SAFETY: an empty dictionary we own.
        let message = unsafe { xpc_dictionary_create(ptr::null(), ptr::null(), 0) };
        set_data(message, REQUEST_KEY, request);
        if let Some(password) = password {
            set_data(message, PASSWORD_KEY, password);
        }
        let (tx, rx) = channel();
        // Called once, on a dispatch queue, with the reply or an error
        // object. The reply is libxpc's and lives only for the call, so its
        // bytes are copied out here.
        let on_reply = RcBlock::new(move |reply: xpc_object_t| {
            let result = if is(reply, unsafe { &_xpc_type_dictionary }) {
                data(reply, RESPONSE_KEY)
                    .map(|bytes| bytes.to_vec())
                    .ok_or(XpcError::BadReply)
            } else {
                Err(XpcError::Unavailable)
            };
            let _ = tx.send(result);
        });
        // SAFETY: a live connection and message; libxpc copies the block and
        // keeps what it needs of the message, which is ours to release. A
        // global queue needs no release.
        unsafe {
            xpc_connection_send_message_with_reply(
                self.connection as xpc_object_t,
                message,
                dispatch_get_global_queue(0, 0),
                &on_reply,
            );
            xpc_release(message);
        }
        match rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Disconnected) => Err(XpcError::Unavailable),
            Err(RecvTimeoutError::Timeout) => {
                // Stop waiting: cancelling hands the pending reply handler
                // an error, which nobody reads any more.
                if !self.cancelled.swap(true, Ordering::SeqCst) {
                    // SAFETY: the connection is ours and not yet cancelled.
                    unsafe { xpc_connection_cancel(self.connection as xpc_object_t) };
                }
                Err(XpcError::Timeout)
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: the connection is ours; it is cancelled once.
        unsafe {
            if !self.cancelled.swap(true, Ordering::SeqCst) {
                xpc_connection_cancel(self.connection as xpc_object_t);
            }
            xpc_release(self.connection as xpc_object_t);
        }
    }
}
