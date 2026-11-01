//! Being asked to stop (systemd stopping the session's unit, logging out,
//! shutting down, Ctrl+C) puts a game away where it was, and a film's
//! place, as leaving does: the signals are blocked in every thread from
//! the start, and one thread waits for them and tells the program.

use aizome_kit::MsgSender;

const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

fn set() -> libc::sigset_t {
    // SAFETY: a zeroed sigset_t emptied and filled by libc.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in SIGNALS {
            libc::sigaddset(&mut set, s);
        }
        set
    }
}

/// Block the signals in this thread and those it starts: call before
/// any other thread starts.
pub fn block() {
    let set = set();
    // SAFETY: changing this thread's own mask.
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
}

/// Wait for the signals on a thread of its own; each sends `msg`.
pub fn forward<Msg: Clone + Send + 'static>(tx: MsgSender<Msg>, msg: Msg) {
    let _ = std::thread::Builder::new().name("chanoma-signals".into()).spawn(move || {
        let set = set();
        loop {
            let mut sig = 0;
            // SAFETY: waiting for signals blocked in every thread.
            if unsafe { libc::sigwait(&set, &mut sig) } == 0 && !tx.send(msg.clone()) {
                return;
            }
        }
    });
}
