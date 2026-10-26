//! Stickies and the compositor: Aizome's IPC keeps a sticky above other
//! windows and puts it back where it was; elsewhere a sticky is a plain
//! utility window where the compositor puts it.

use std::time::{Duration, Instant};

use aizome_ipc::{Client, Rect, Request, Response, WindowInfo};
use serde::{Deserialize, Serialize};

/// Where a sticky was, as the compositor had it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 1-based; none when it shows on every Desk.
    pub desk: Option<u32>,
}

impl Place {
    pub fn of(w: &WindowInfo) -> Place {
        Place { x: w.geometry.x, y: w.geometry.y, w: w.geometry.w, h: w.geometry.h, desk: w.desk }
    }
}

fn connect() -> Option<Client> {
    aizome_ipc::socket_path()?;
    Client::connect().ok()
}

/// Our windows with this title, as the compositor lists them.
fn ours(c: &mut Client, title: &str) -> Option<WindowInfo> {
    let pid = std::process::id() as i32;
    match c.request(&Request::Windows).ok()? {
        Response::Windows { windows } => windows.into_iter().find(|w| w.pid == Some(pid) && w.title == title),
        _ => None,
    }
}

/// Once the sticky titled `title` shows, keep it above the others and
/// (with `place`) put it where it was. Blocks until it shows (or 3 s):
/// run it on a thread. Whether the compositor was there to ask.
pub fn settle_sticky(title: &str, place: Option<Place>, float: bool) -> bool {
    let Some(mut c) = connect() else { return false };
    let until = Instant::now() + Duration::from_secs(3);
    let win = loop {
        if let Some(w) = ours(&mut c, title) {
            break w;
        }
        if Instant::now() > until {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let id = Some(win.id);
    if float {
        let _ = c.request(&Request::Float { id, state: Some(true) });
    }
    if let Some(p) = place {
        if let Some(d) = p.desk {
            let _ = c.request(&Request::MoveToDesk { id, desk: aizome_ipc::DeskSel::Index(d), follow: false });
        }
        let _ = c.request(&Request::Resize { id, rect: Rect::new(p.x, p.y, p.w.max(120), p.h.max(80)) });
    }
    true
}

/// Keep the sticky titled `title` above the others, or not.
pub fn set_float(title: &str, on: bool) -> bool {
    let Some(mut c) = connect() else { return false };
    match ours(&mut c, title) {
        Some(w) => c.request(&Request::Float { id: Some(w.id), state: Some(on) }).is_ok(),
        None => false,
    }
}

/// Where each of these stickies is now (by title), and whether it floats.
pub fn places(titles: &[String]) -> Vec<(String, Place, bool)> {
    let Some(mut c) = connect() else { return Vec::new() };
    let pid = std::process::id() as i32;
    let Ok(Response::Windows { windows }) = c.request(&Request::Windows) else { return Vec::new() };
    windows
        .iter()
        .filter(|w| w.pid == Some(pid) && titles.contains(&w.title))
        .map(|w| (w.title.clone(), Place::of(w), w.float))
        .collect()
}
