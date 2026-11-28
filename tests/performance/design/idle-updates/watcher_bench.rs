//! Settings-save to watcher callback latency, with the production 1 s interval.
use std::time::{Duration, Instant};
use std::sync::mpsc;

fn main() {
    let n: usize = std::env::var("WATCH_BENCH_N").unwrap_or("60".into()).parse().unwrap();
    let dir = std::env::temp_dir().join(format!("aizome-watch-bench-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.toml");
    std::fs::write(&path, "0").unwrap();
    let (tx, rx) = mpsc::channel();
    let w = aizome_config::Watcher::spawn_for(path.clone(), Duration::from_secs(1), move || {
        let _ = tx.send(Instant::now());
    });
    // The old watcher establishes its baseline inside its new thread.
    std::thread::sleep(Duration::from_millis(100));
    let mut random = 332u64;
    let mut samples = Vec::new();
    let mut delays = Vec::new();
    for i in 0..n {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ms = 1 + (random >> 32) % 999;
        std::thread::sleep(Duration::from_millis(ms));
        while rx.try_recv().is_ok() {}
        let tmp = dir.join("replacement");
        std::fs::write(&tmp, format!("{}", i + 1)).unwrap();
        let t = Instant::now();
        std::fs::rename(&tmp, &path).unwrap();
        let notified = rx.recv_timeout(Duration::from_secs(3)).expect("settings callback");
        samples.push(notified.duration_since(t).as_nanos() as u64);
        delays.push(ms);
    }
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    println!("{}", serde_json::json!({"samples_ns":samples,"delay_ms":delays}));
}
