//! `aizome-panel-refresh on|off|apply`: Panel Self Refresh, as root
//! (`aizome_sys::panel_refresh`). The battery menu runs it through pkexec
//! to turn it on or off; the boot service runs `apply` to put back the
//! choice kept.

use aizome_sys::panel_refresh::{CHOICE, STATUS, helper};
use std::path::Path;

fn main() {
    let on = match std::env::args().nth(1).as_deref() {
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some("apply") => None,
        _ => {
            eprintln!("usage: aizome-panel-refresh on|off|apply");
            std::process::exit(2);
        }
    };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("aizome-panel-refresh: run as root (through pkexec)");
        std::process::exit(1);
    }
    let debugfs = Path::new("/sys/kernel/debug");
    if on.is_none() {
        // At boot Intel's driver may still be coming up: its files come a
        // moment after it binds. Nothing to wait for without it.
        for _ in 0..100 {
            if helper::find(debugfs).is_some() || !helper::intel_display() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    match helper::run(debugfs, Path::new(CHOICE), Path::new(STATUS), on) {
        Ok(s) if s.available => println!("Panel Self Refresh {}", if s.on { "on" } else { "off" }),
        Ok(_) => println!("this panel can't refresh itself"),
        Err(e) => {
            eprintln!("aizome-panel-refresh: {e}");
            std::process::exit(1);
        }
    }
}
