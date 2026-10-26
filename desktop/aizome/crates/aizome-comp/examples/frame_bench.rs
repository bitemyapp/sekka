//! Frame times under load, for docs/performance.md.
//!
//! Starts a headless compositor (the `aizome` binary next to this
//! example), opens sidestep-ui windows (examples/sidestep_client) and runs
//! scenarios, reading the compositor's own per-frame CPU (and GPU) times
//! over IPC:
//!
//! ```sh
//! cargo build --release -p aizome-comp --bin aizome --examples
//! target/release/examples/frame_bench --renderer gles --output 2880x1800@2
//! ```

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aizome_ipc::{Client, DeskSel, Request, Response, Stats, Synthetic};

struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn main() {
    let mut renderer = "gles".to_string();
    let mut output = "2880x1800@2".to_string();
    let mut only: Option<String> = None;
    let mut log: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--renderer" => renderer = args.next().unwrap(),
            "--output" => output = args.next().unwrap(),
            "--only" => only = args.next(),
            "--log" => log = args.next(),
            _ => panic!("usage: frame_bench [--renderer gles|pixman] [--output WxH@S] [--only NAME] [--log FILTER]"),
        }
    }
    let exe = std::env::current_exe().unwrap();
    let examples = exe.parent().unwrap().to_path_buf();
    let target = examples.parent().unwrap().to_path_buf();
    let aizome = target.join("aizome");
    let client_bin = examples.join("sidestep_client");
    let dir = PathBuf::from(format!("/tmp/aizome-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config/aizome")).unwrap();
    std::fs::write(dir.join("config/aizome/settings.toml"), "").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = format!("azbench-{}", std::process::id());
    let comp = Command::new(&aizome)
        .args([
            "--backend",
            "headless",
            "--socket",
            &socket,
            "--no-session",
            "--no-shell",
            "--no-xwayland",
            "--renderer",
            &renderer,
            "--headless-output",
            &output,
        ])
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("AIZOME_LOG", log.as_deref().unwrap_or("warn"))
        .stdout(Stdio::null())
        // `--log FILTER` shows the compositor's log at that level.
        .stderr(if log.is_some() { Stdio::inherit() } else { Stdio::null() })
        .spawn()
        .expect("start aizome");
    let mut kill = Kill(vec![comp]);
    let ipc = dir.join(format!("aizome.{socket}.sock"));
    let deadline = Instant::now() + Duration::from_secs(20);
    while Client::connect_to(&ipc).is_err() {
        assert!(Instant::now() < deadline, "the compositor didn't start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let req = |r: Request| -> Response { Client::connect_to(&ipc).unwrap().request(&r).unwrap() };
    let spawn = |mode: &str| -> Child {
        Command::new(&client_bin)
            .arg(mode)
            .env("WAYLAND_DISPLAY", &socket)
            .env("XDG_RUNTIME_DIR", &dir)
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the client")
    };
    let windows = |n: usize| {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Response::Windows { windows } = req(Request::Windows)
                && windows.len() >= n
            {
                return windows;
            }
            assert!(Instant::now() < deadline, "windows didn't open");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let stats = |reset: bool| -> Stats {
        match req(Request::Stats { reset }) {
            Response::Stats { stats } => stats,
            r => panic!("{r:?}"),
        }
    };
    let report = |name: &str, s: &Stats| {
        let o = &s.outputs[0];
        let ms = |us: f64| us / 1000.0;
        let gpu = o.gpu_us.map(|g| format!("  gpu p50 {:.3} p99 {:.3}", ms(g.p50), ms(g.p99))).unwrap_or_default();
        println!(
            "{name:<28} frames {:>4} skipped {:>3}  cpu p50 {:.3} p95 {:.3} p99 {:.3} max {:.3} mean {:.3} ms{gpu}",
            o.frames_rendered,
            o.frames_skipped,
            ms(o.cpu_us.p50),
            ms(o.cpu_us.p95),
            ms(o.cpu_us.p99),
            ms(o.cpu_us.max),
            ms(o.cpu_us.mean)
        );
    };
    println!("renderer {renderer}, output {output}");
    let keys = |k: &str| {
        let r = req(Request::Synthesize { events: vec![Synthetic::Chord { keys: k.into() }] });
        assert!(matches!(r, Response::Ok), "{r:?}");
    };
    // `--only NAME` runs the scenarios whose names contain NAME (and the
    // windows they need).
    let want = |name: &str| only.as_deref().is_none_or(|o| name.contains(o));

    // Ctrl+Alt+Del straight after start, before the overlay's labels are
    // warmed: its first frame from cold.
    if want("recovery: open, cold") {
        stats(true);
        keys("ctrl+alt+delete");
        std::thread::sleep(Duration::from_millis(300));
        report("recovery: open, cold", &stats(true));
        keys("escape");
        std::thread::sleep(Duration::from_millis(300));
    }

    // Three ordinary windows.
    for _ in 0..3 {
        kill.0.push(spawn("toplevel"));
    }
    let ws = windows(3);
    std::thread::sleep(Duration::from_millis(1500));

    // Idle: nothing changes, nothing is drawn.
    if want("idle (2s)") {
        stats(true);
        std::thread::sleep(Duration::from_secs(2));
        report("idle (2s)", &stats(true));
    }

    // Moving a window: composition only.
    let id = ws[0].id;
    let (x0, y0) = (ws[0].geometry.x, ws[0].geometry.y);
    if want("move a window (240 steps)") {
        stats(true);
        for i in 0..240 {
            let dx = ((i as f64 / 240.0) * std::f64::consts::TAU).sin() * 300.0;
            req(Request::Move { id: Some(id), x: x0 + dx as i32, y: y0 + (i % 60) });
            std::thread::sleep(Duration::from_millis(16));
        }
        report("move a window (240 steps)", &stats(true));
    }

    // A client drawing every frame.
    if want("animating client (3s)") {
        kill.0.push(spawn("animate"));
        windows(4);
        std::thread::sleep(Duration::from_millis(500));
        stats(true);
        std::thread::sleep(Duration::from_secs(3));
        report("animating client (3s)", &stats(true));
        // Stop it before the next scenarios.
        if let Some(mut c) = kill.0.pop() {
            let _ = c.kill();
            let _ = c.wait();
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // Switching Desks: full-output damage while sliding.
    if want("desk switches (12, sliding)") {
        stats(true);
        for i in 0..12 {
            req(Request::SetDesk { desk: DeskSel::Index(if i % 2 == 0 { 2 } else { 1 }) });
            std::thread::sleep(Duration::from_millis(200));
        }
        report("desk switches (12, sliding)", &stats(true));
    }

    // The overview: thumbnails of every window, full damage.
    if want("overview open/close (8)") {
        stats(true);
        for _ in 0..8 {
            req(Request::Overview { state: Some(true) });
            std::thread::sleep(Duration::from_millis(150));
            req(Request::Overview { state: Some(false) });
            std::thread::sleep(Duration::from_millis(150));
        }
        report("overview open/close (8)", &stats(true));
    }

    let later = [
        "10 windows, one moving",
        "recovery: open/close (8)",
        "recovery: list updating (5s)",
        "recovery: keys moving (20)",
        "frost: window moving under (Full)",
        "frost: client drawing under (Full)",
        "full-screen client (3s)",
        "recovery over it (3s)",
    ];
    if !later.iter().any(|n| want(n)) {
        drop(kill);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    // Many windows, one moving.
    for _ in 0..7 {
        kill.0.push(spawn("toplevel"));
    }
    windows(10);
    std::thread::sleep(Duration::from_millis(1500));
    req(Request::Focus { id });
    std::thread::sleep(Duration::from_millis(200));
    if want("10 windows, one moving") {
        stats(true);
        for i in 0..120 {
            req(Request::Move { id: Some(id), x: x0 + (i * 3) % 400, y: y0 });
            std::thread::sleep(Duration::from_millis(16));
        }
        report("10 windows, one moving", &stats(true));
    }

    // The recovery overlay over ten windows: opening and closing, the
    // list updating (CPU and memory each second, pings), and moving
    // through it with the keys.
    if want("recovery: open/close (8)") || want("recovery: list updating (5s)") || want("recovery: keys moving (20)") {
        std::thread::sleep(Duration::from_millis(2500));
        stats(true);
        for _ in 0..8 {
            keys("ctrl+alt+delete");
            std::thread::sleep(Duration::from_millis(150));
            keys("escape");
            std::thread::sleep(Duration::from_millis(150));
        }
        report("recovery: open/close (8)", &stats(true));
        keys("ctrl+alt+delete");
        std::thread::sleep(Duration::from_millis(1500));
        stats(true);
        std::thread::sleep(Duration::from_secs(5));
        report("recovery: list updating (5s)", &stats(true));
        for _ in 0..20 {
            keys("down");
            std::thread::sleep(Duration::from_millis(50));
        }
        report("recovery: keys moving (20)", &stats(true));
        keys("escape");
        std::thread::sleep(Duration::from_millis(300));
    }

    // The Full effects tier: a frosted menu (90% alpha, as the shell draws
    // them) with a window moving under it, then a client drawing every
    // frame under it. Under GLES what's behind it is blurred, again
    // whenever it changes.
    if want("frost: window moving under (Full)") || want("frost: client drawing under (Full)") {
        let settings = dir.join("config/aizome/settings.toml");
        std::fs::write(&settings, "[appearance]\neffects = \"full\"\n").unwrap();
        req(Request::ReloadConfig);
        kill.0.push(spawn("frost"));
        std::thread::sleep(Duration::from_millis(1000));
        req(Request::Focus { id });
        std::thread::sleep(Duration::from_millis(200));
        stats(true);
        for i in 0..240 {
            // Back and forth across the menu (200–500 × 120–520 pt).
            let x = 60 + ((i as f64 / 240.0 * std::f64::consts::TAU).sin() * 220.0) as i32 + 220;
            req(Request::Move { id: Some(id), x, y: 220 });
            std::thread::sleep(Duration::from_millis(16));
        }
        report("frost: window moving under (Full)", &stats(true));
        kill.0.push(spawn("animate"));
        let ws = windows(11);
        let anim = ws.iter().find(|w| w.title == "Animating").map(|w| w.id).unwrap();
        req(Request::Move { id: Some(anim), x: 260, y: 160 });
        std::thread::sleep(Duration::from_millis(800));
        stats(true);
        std::thread::sleep(Duration::from_secs(3));
        report("frost: client drawing under (Full)", &stats(true));
        for _ in 0..2 {
            if let Some(mut c) = kill.0.pop() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        std::fs::write(&settings, "").unwrap();
        req(Request::ReloadConfig);
        std::thread::sleep(Duration::from_millis(500));
    }

    // A full-screen client drawing every frame: the whole output damaged.
    if want("full-screen client (3s)") || want("recovery over it (3s)") {
        kill.0.push(spawn("animate"));
        let ws = windows(11);
        let anim = ws.iter().find(|w| w.title == "Animating").map(|w| w.id).unwrap();
        req(Request::Fullscreen { id: Some(anim), state: Some(true) });
        std::thread::sleep(Duration::from_millis(800));
        stats(true);
        std::thread::sleep(Duration::from_secs(3));
        report("full-screen client (3s)", &stats(true));

        // Ctrl+Alt+Del over it: the overlay composited over every frame.
        keys("ctrl+alt+delete");
        std::thread::sleep(Duration::from_millis(500));
        stats(true);
        std::thread::sleep(Duration::from_secs(3));
        report("recovery over it (3s)", &stats(true));
        keys("escape");
    }

    drop(kill);
    let _ = std::fs::remove_dir_all(&dir);
}
