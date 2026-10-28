//! Shiori's worker over a folder of PDFs, as the app drives it: each file
//! opened, its first pages drawn at 1.5× (the size a 900-point window
//! fits), their text and links read, and its first pages searched. One
//! line a file: what happened, and how long it took.
//!
//! `cargo run --release -p aizome-shiori --example corpus -- DIR [JOBS]`
//! (build `aizome-shiori` first: the workers run its program).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aizome_shiori::engine::{Engine, Event};
use aizome_shiori::protocol::{Reply, Request, TileKey};

fn one(exe: &Path, path: PathBuf) -> String {
    let start = Instant::now();
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let e = Engine::start(exe.to_path_buf(), path, None, move |ev| {
        let _ = tx.lock().map(|t| t.send(ev));
    });
    let mut out: Vec<String> = Vec::new();
    let mut waiting = 0usize;
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let Ok(ev) = rx.recv_timeout(left) else {
            out.push(format!("unanswered({waiting})"));
            break;
        };
        match ev {
            Event::Reply(Reply::Opened(info)) => {
                if waiting > 0 {
                    // A worker started again after a failure: the rest is
                    // still coming.
                    continue;
                }
                let pages = info.pages.len().min(3);
                let keys: Vec<TileKey> = (0..pages)
                    .map(|i| {
                        let (w, h) = info.pages[i];
                        let z = 1.5 * 868.0 / f64::from(w.max(1.0));
                        let size =
                            (((f64::from(w) * z) as u32).clamp(1, 4096), ((f64::from(h) * z) as u32).clamp(1, 4096));
                        TileKey { page: i as u32, size, at: (0, 0), w: size.0.min(1024), h: size.1.min(1024) }
                    })
                    .collect();
                waiting = keys.len() + 2 * pages + 1;
                e.send(Request::Draw(keys));
                for i in 0..pages as u32 {
                    e.send(Request::Text(i));
                    e.send(Request::Links(i));
                }
                let n = info.pages.len().min(10) as u32;
                e.send(Request::Find { serial: 1, query: "the".into(), case: false, pages: (0..n).collect() });
                out.push(format!("{} pages", info.pages.len()));
            }
            Event::Reply(Reply::OpenFailed(err)) => {
                out.push(format!("open-failed {err:?}"));
                break;
            }
            Event::Drawn(..) | Event::Reply(Reply::Text { .. } | Reply::Links { .. }) => waiting -= 1,
            Event::Reply(Reply::DrawFailed(k)) => {
                out.push(format!("draw-failed p{}", k.page));
                waiting -= 1;
            }
            Event::Reply(Reply::Found { page, .. }) => {
                // The search's last page answers for it.
                if page + 1 == 10 || waiting == 1 {
                    waiting = waiting.saturating_sub(1);
                }
            }
            Event::Failed { job, why, gave_up } => {
                out.push(format!("{why:?} on {job:?}{}", if gave_up { " (gave up)" } else { "" }));
                waiting = waiting.saturating_sub(1);
                if gave_up {
                    break;
                }
            }
            Event::Reply(_) => {}
        }
        if waiting == 0
            && !out.is_empty()
            && !out[0].starts_with("unanswered")
            && out.iter().any(|o| o.ends_with("pages"))
        {
            break;
        }
    }
    format!("{:.0} ms\t{}", start.elapsed().as_secs_f64() * 1000.0, out.join("; "))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("a folder of PDFs"));
    let jobs: usize = args.next().and_then(|j| j.parse().ok()).unwrap_or(4);
    let exe = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("aizome-shiori");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")))
        .collect();
    files.sort();
    let files = std::sync::Arc::new(std::sync::Mutex::new(files.into_iter()));
    let threads: Vec<_> = (0..jobs)
        .map(|_| {
            let (files, exe) = (files.clone(), exe.clone());
            std::thread::spawn(move || {
                loop {
                    let Some(f) = files.lock().unwrap().next() else { return };
                    let name = f.file_name().unwrap().to_string_lossy().into_owned();
                    println!("{name}\t{}", one(&exe, f));
                }
            })
        })
        .collect();
    for t in threads {
        let _ = t.join();
    }
}
