mod egl;
use hotaru_gpu_transport_prototype::{Limits, spawn};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
fn main() -> Result<(), String> {
    let lib = std::env::args()
        .nth(1)
        .ok_or("explicit libEGL path required")?;
    let (done_tx, done) = std::sync::mpsc::channel();
    let mut owner = spawn(
        Limits {
            bytes: 1 << 20,
            ..Limits::default()
        },
        move || egl::EglDriver::new(&lib, "/dev/dri/renderD128", Some(done_tx)),
    )
    .map_err(|e| e.to_string())?;
    let window = owner.open().map_err(|e| format!("open: {e:?}"))?;
    owner
        .submit(window, Arc::new(egl::Quadrants), 1, 64 * 1024)
        .map_err(|e| format!("submit: {e:?}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let import = loop {
        if let Some(import) = owner.poll_ready() {
            break import;
        }
        if owner.stats().failed || Instant::now() >= deadline {
            return Err("worker failed/deadline".into());
        }
        thread::sleep(Duration::from_millis(1));
    };
    let d = import.descriptor();
    println!(
        "{}",
        serde_json::json!({"event":"rust_export_ready","width":d.width,"height":d.height,"stride":d.stride,"offset":d.offset,"fourcc":d.fourcc,"modifier":d.modifier,"event_thread":format!("{:?}",thread::current().id())})
    );
    drop(import);
    owner.connection_lost();
    while owner.stats().leases != 0 {
        if Instant::now() >= deadline {
            return Err("teardown deadline".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
    // Diagnostic waits for cleanup evidence; production owner never joins.
    if !done
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?
    {
        return Err("EGL cleanup failed".into());
    }
    println!("{}", serde_json::json!({"event":"rust_probe_complete"}));
    Ok(())
}
