//! The greeter's own compositor: Aizome's, without its shell, Xwayland or
//! session plumbing, started as the greeter's child so that the two live
//! and die together. The greeter starts it (greetd runs only the greeter),
//! waits for its Wayland socket, and on the way out stops it and waits,
//! so the display is free before greetd starts the session. If the greeter
//! dies, the kernel stops the compositor too (`PR_SET_PDEATHSIG`); if the
//! compositor dies, the greeter loses its display and exits, and greetd
//! starts both again.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub struct Compositor {
    child: Child,
    pub socket: String,
}

impl Compositor {
    /// Start `argv` (the configuration's `compositor`) on a socket of its
    /// own, with `settings` for its settings file, and wait until it
    /// listens.
    pub fn start(argv: &[String], settings: Option<&PathBuf>) -> Result<Compositor, String> {
        let (program, args) = argv.split_first().ok_or("no compositor configured")?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).ok_or("XDG_RUNTIME_DIR isn't set")?;
        let socket = format!("aizome-greeter-{}", std::process::id());
        let mut cmd = Command::new(program);
        cmd.args(args).arg("--socket").arg(&socket);
        if let Some(s) = settings {
            cmd.arg("--config").arg(s);
        }
        // Nothing of a session the greeter might have been started from.
        cmd.env_remove("WAYLAND_DISPLAY").env_remove("DISPLAY").env_remove("AIZOME_SOCKET");
        // Safety: prctl in the child before exec, async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // The greeter may already be gone (a race with the fork).
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGTERM);
                }
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
        let mut c = Compositor { child, socket };
        let path = runtime.join(&c.socket);
        let until = Instant::now() + Duration::from_secs(20);
        while !path.exists() {
            if let Ok(Some(status)) = c.child.try_wait() {
                return Err(format!("{program} exited ({status}) before it was ready"));
            }
            if Instant::now() > until {
                c.stop();
                return Err(format!("{program} wasn't ready after 20 s"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(c)
    }

    /// Stop it and wait (SIGTERM, then SIGKILL after 3 s).
    pub fn stop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        // Safety: kill on our own child's pid.
        unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop();
    }
}
