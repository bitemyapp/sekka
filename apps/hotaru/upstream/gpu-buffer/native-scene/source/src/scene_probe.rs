mod egl;
mod scene;
use hotaru_gpu_transport_prototype::{Limits, spawn};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: scene-worker-probe LIBEGL FIXTURE_DIR".into());
    }
    let lib = args[1].clone();
    let dir = std::path::Path::new(&args[2]);
    let mut paths = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "scene"))
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() || paths.len() > 64 {
        return Err("fixture count".into());
    }
    let (tx, done) = std::sync::mpsc::channel();
    let mut owner = spawn(Limits::default(), move || {
        egl::SceneDriver::new(&lib, "/dev/dri/renderD128", Some(tx))
    })
    .map_err(|e| e.to_string())?;
    let window = owner.open().map_err(|e| format!("open {e:?}"))?;
    for path in paths {
        let scene = Arc::new(scene::Scene::load(&path)?);
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(15);
            owner
                .submit(
                    window,
                    scene.clone(),
                    scene.bytes(),
                    scene.width() as usize * scene.height() as usize * 4 + 65536,
                )
                .map_err(|e| format!("submit {e:?}"))?;
            loop {
                if let Some(import) = owner.poll_ready() {
                    drop(import);
                    break;
                }
                if owner.stats().failed || Instant::now() > deadline {
                    return Err("scene worker failed/deadline".into());
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    owner.connection_lost();
    let deadline = Instant::now() + Duration::from_secs(5);
    while owner.stats().leases != 0 {
        if Instant::now() > deadline {
            return Err("lease teardown timeout".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
    if !done
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?
    {
        return Err("context cleanup failed".into());
    }
    println!(
        "{}",
        serde_json::json!({"event":"scene_probe_complete","exact_pixel_gate":true})
    );
    Ok(())
}
