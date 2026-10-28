use aizome_profile::{Capture, Message, Mode, Options, Target, identity, record};
use std::sync::atomic::{AtomicBool, Ordering};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.first().is_some_and(|s| s == "--json");
    if json {
        args.remove(0);
    }
    if args.len() < 3 || !matches!(args[0].as_str(), "timeline" | "cpu") {
        eprintln!(
            "Usage: aizome-profile timeline|cpu 10|30|60 PID [PID…]\nStop with Ctrl-C. Captures stay in $XDG_STATE_HOME/aizome/profiles."
        );
        std::process::exit(2);
    }
    let result = (|| -> Result<(), String> {
        let seconds = args[1].parse().map_err(|_| "invalid duration")?;
        let targets = args[2..]
            .iter()
            .map(|p| {
                if let Some((pid, start)) = p.split_once(':') {
                    Ok(Target {
                        pid: pid.parse::<u32>().map_err(|e| e.to_string())?,
                        start_ticks: start.parse::<u64>().map_err(|e| e.to_string())?,
                    })
                } else {
                    p.parse::<u32>().map_err(|e| e.to_string()).and_then(|p| identity(p).map_err(|e| e.to_string()))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        unsafe {
            libc::signal(libc::SIGINT, signal as *const () as usize);
            libc::signal(libc::SIGTERM, signal as *const () as usize);
        }
        let capture = Capture::new();
        let stop = capture.cancellation();
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let d = done.clone();
        let watcher = std::thread::spawn(move || {
            while !d.load(Ordering::Relaxed) {
                if STOP.load(Ordering::Relaxed) {
                    stop.store(true, Ordering::Release);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        let result = record(
            Options { targets, seconds, mode: if args[0] == "cpu" { Mode::Cpu } else { Mode::Timeline } },
            capture.cancellation(),
            |message| {
                if json {
                    println!("{}", serde_json::to_string(&Message::Progress { message }).unwrap());
                } else {
                    eprintln!("{message}");
                }
            },
        );
        done.store(true, Ordering::Relaxed);
        let _ = watcher.join();
        if json {
            println!("{}", serde_json::to_string(&Message::Finished { result: result.clone() }).unwrap());
        }
        let path = result.map_err(|e| e.to_string())?.path;
        if !json {
            println!("{}", path.display());
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
