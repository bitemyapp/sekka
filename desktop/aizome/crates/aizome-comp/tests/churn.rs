//! Many windows coming and going while the window manager is driven hard:
//! stacks, Desks, fill, the overview, the switcher and drags, in a fixed
//! pseudo-random order. The compositor must stay up and consistent.

mod common;

use aizome_ipc::{DeskSel, Request, Response, Synthetic};
use common::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

#[test]
fn the_window_manager_survives_churn() {
    let c = Compositor::start(Options { settings: "", ..Default::default() });
    let seed = std::env::var("AIZOME_CHURN_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x9e3779b97f4a7c15u64);
    let mut rng = Rng(seed);
    let mut clients: Vec<TestClient> = Vec::new();
    for i in 0..300 {
        let windows = c.windows();
        let ids: Vec<u64> = windows.iter().map(|w| w.id).collect();
        let pick = |rng: &mut Rng| ids.get(rng.below(ids.len() as u64) as usize).copied();
        let req = match rng.below(16) {
            0 | 1 if clients.len() < 12 => {
                let n = i % 5;
                clients.push(
                    c.client(
                        Spec::new(&format!("churn.app{n}"), 0x203040 + (i as u32 * 7919) % 0xbfbfbf)
                            .size(200 + (i % 7) * 40, 150 + (i % 5) * 30),
                    ),
                );
                continue;
            }
            2 if !clients.is_empty() => {
                let k = rng.below(clients.len() as u64) as usize;
                drop(clients.remove(k));
                continue;
            }
            3 => match (pick(&mut rng), pick(&mut rng)) {
                (Some(a), Some(b)) => Request::Stack { a, b },
                _ => continue,
            },
            4 => Request::Unstack { id: pick(&mut rng) },
            5 => Request::SetDesk { desk: DeskSel::Index(1 + rng.below(4) as u32), output: None },
            6 => match pick(&mut rng) {
                Some(id) => Request::MoveToDesk {
                    id: Some(id),
                    desk: DeskSel::Index(1 + rng.below(4) as u32),
                    follow: rng.below(2) == 0,
                },
                None => continue,
            },
            7 => Request::Fill { id: pick(&mut rng), state: None },
            8 => Request::Overview { state: None },
            9 => Request::Minimise { id: pick(&mut rng) },
            10 => match pick(&mut rng) {
                Some(id) => Request::Focus { id },
                None => continue,
            },
            11 => Request::Synthesize {
                events: vec![
                    Synthetic::PointerMotion { x: rng.below(1280) as f64, y: rng.below(800) as f64 },
                    Synthetic::PointerButton { button: "left".into(), pressed: true },
                    Synthetic::PointerMotion { x: rng.below(1280) as f64, y: rng.below(800) as f64 },
                    Synthetic::PointerButton { button: "left".into(), pressed: false },
                ],
            },
            12 => Request::Synthesize { events: vec![Synthetic::Chord { keys: "super+tab".into() }] },
            13 => Request::Split {
                id: pick(&mut rng),
                side: if rng.below(2) == 0 { aizome_ipc::Side::Left } else { aizome_ipc::Side::Right },
            },
            14 => Request::Synthesize {
                events: vec![
                    Synthetic::PointerMotion { x: rng.below(1280) as f64, y: rng.below(800) as f64 },
                    Synthetic::Click { button: "right".into() },
                    Synthetic::Chord { keys: "escape".into() },
                ],
            },
            _ => Request::HideApp {
                app: aizome_ipc::AppSel { app_id: Some(format!("churn.app{}", rng.below(5))), pid: None },
            },
        };
        // Errors (a stale id, an unstacked window) are fine; the compositor
        // going away isn't.
        let _ = c.request(req);
        if i % 50 == 0 {
            c.screenshot();
        }
    }
    // Still answering, and consistent: grouped windows share their frame.
    let windows = c.windows();
    for w in &windows {
        if let Some(g) = w.group {
            for o in windows.iter().filter(|o| o.group == Some(g)) {
                assert_eq!(o.geometry, w.geometry, "a group shares its frame");
            }
        }
    }
    assert!(matches!(c.request(Request::Version), Response::Version { .. }));
    drop(clients);
    c.wait_windows("all closed", |ws| ws.is_empty());
}
