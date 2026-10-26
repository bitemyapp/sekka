//! The window manager's answers to IPC requests (the core answers the
//! rest: outputs, screenshots, settings, stats, reloads).

use aizome_comp_core::aizome_ipc::{AppSel, DeskSel, Request, Response};

use crate::prelude::*;
use crate::wm::{DeskId, Side};

pub trait RequestsExt {
    /// A request for the window manager; `None` when it's not one.
    fn wm_request(&mut self, req: &Request) -> Option<Response>;
}

impl RequestsExt for State {
    fn wm_request(&mut self, req: &Request) -> Option<Response> {
        match wm_request(self, req.clone()) {
            Ok(Some(r)) => Some(r),
            Ok(None) => None,
            Err(e) => Some(Response::Error { message: e }),
        }
    }
}

fn window_arg(st: &State, id: Option<WinId>) -> Result<WinId, String> {
    let wm = &st.lg.r().wm;
    match id {
        Some(id) if wm.contains(id) => Ok(id),
        Some(id) => Err(format!("no window {id}")),
        None => wm.focused().ok_or_else(|| "no window has focus".to_string()),
    }
}

fn desk_arg(st: &State, d: &DeskSel) -> Result<DeskId, String> {
    let wm = &st.lg.r().wm;
    match d {
        DeskSel::Index(i) => wm.desk_at(*i).ok_or_else(|| format!("no Desk {i}")),
        DeskSel::Name(n) => wm.desk_named(n).ok_or_else(|| format!("no Desk named {n}")),
    }
}

fn app_arg(st: &State, app: &AppSel) -> Result<String, String> {
    let wm = &st.lg.r().wm;
    if let Some(a) = &app.app_id {
        return Ok(a.clone());
    }
    if let Some(pid) = app.pid {
        return wm
            .windows()
            .find(|w| w.pid == Some(pid))
            .map(|w| w.app_id.clone())
            .ok_or_else(|| format!("no window of process {pid}"));
    }
    let id = wm.focused().ok_or("no window has focus")?;
    Ok(wm.get(id).map(|w| w.app_id.clone()).unwrap_or_default())
}

fn wm_request(st: &mut State, req: Request) -> Result<Option<Response>, String> {
    Ok(Some(match req {
        Request::Windows => {
            let ids: Vec<WinId> = st.lg.r().wm.stacking().to_vec();
            let windows = ids.iter().filter_map(|id| st.window_info(*id)).collect();
            Response::Windows { windows }
        }
        Request::Desks => {
            let l = st.lg.r();
            let desks = crate::actions::desk_infos(l);
            let current = l.wm.desk_index(l.wm.current_desk()).unwrap_or(1);
            Response::Desks { desks, current }
        }
        Request::FocusedWindow => {
            let f = st.lg.r().wm.focused();
            Response::Window { window: f.and_then(|id| st.window_info(id)) }
        }
        Request::Focus { id } => {
            let id = window_arg(st, Some(id))?;
            st.activate_window(id);
            Response::Ok
        }
        Request::Close { id } => {
            let id = window_arg(st, id)?;
            st.close_window(id);
            Response::Ok
        }
        Request::Minimise { id } => {
            let id = window_arg(st, id)?;
            st.minimise(id);
            Response::Ok
        }
        Request::Unminimise { id } => {
            let id = window_arg(st, Some(id))?;
            st.unminimise(id);
            Response::Ok
        }
        Request::HideApp { app } => {
            let app = app_arg(st, &app)?;
            st.hide_app(&app);
            Response::Ok
        }
        Request::HideOthers => {
            let app = app_arg(st, &AppSel::default())?;
            st.hide_others(&app);
            Response::Ok
        }
        Request::ShowApp { app } => {
            let app = app_arg(st, &app)?;
            st.show_app(&app);
            Response::Ok
        }
        Request::Fill { id, state } => {
            let id = window_arg(st, id)?;
            st.set_fill(id, state);
            Response::Ok
        }
        Request::Split { id, side } => {
            let id = window_arg(st, id)?;
            let side = match side {
                aizome_comp_core::aizome_ipc::Side::Left => Side::Left,
                aizome_comp_core::aizome_ipc::Side::Right => Side::Right,
            };
            st.set_split(id, side);
            Response::Ok
        }
        Request::Fullscreen { id, state } => {
            let id = window_arg(st, id)?;
            st.set_fullscreen(id, state);
            Response::Ok
        }
        Request::Float { id, state } => {
            let id = window_arg(st, id)?;
            st.set_float(id, state);
            Response::Ok
        }
        Request::Sticky { id, state } => {
            let id = window_arg(st, id)?;
            st.set_sticky(id, state);
            Response::Ok
        }
        Request::MoveToDesk { id, desk, follow } => {
            let id = window_arg(st, id)?;
            let desk = desk_arg(st, &desk)?;
            st.move_to_desk(id, desk, follow);
            Response::Ok
        }
        Request::SetDesk { desk } => {
            let desk = desk_arg(st, &desk)?;
            st.switch_desk(desk);
            Response::Ok
        }
        Request::AddDesk { name, folder, index } => {
            st.lg.m().wm.add_desk(name, folder, index);
            st.emit_desks();
            Response::Ok
        }
        Request::RenameDesk { desk, name } => {
            let desk = desk_arg(st, &desk)?;
            st.lg.m().wm.rename_desk(desk, name);
            st.emit_desks();
            Response::Ok
        }
        Request::SetDeskFolder { desk, folder } => {
            let desk = desk_arg(st, &desk)?;
            st.lg.m().wm.set_desk_folder(desk, folder);
            st.emit_desks();
            Response::Ok
        }
        Request::RemoveDesk { desk } => {
            let desk = desk_arg(st, &desk)?;
            let l = st.lg.m();
            let before = l.wm.current_desk();
            let r = l.wm.remove_desk(desk).ok_or("the last Desk stays")?;
            let changed = l.wm.current_desk() != before;
            let ids: Vec<WinId> = l.wm.windows().map(|w| w.id).collect();
            for id in ids {
                l.window_changed(id);
            }
            if changed {
                st.az.window_outputs_dirty = true;
                st.az.queue_redraw_all();
            }
            st.apply_refocus(r);
            st.emit_desks();
            Response::Ok
        }
        Request::MoveDesk { desk, index } => {
            let desk = desk_arg(st, &desk)?;
            st.lg.m().wm.move_desk(desk, index);
            st.emit_desks();
            Response::Ok
        }
        Request::Stack { a, b } => {
            let a = window_arg(st, Some(a))?;
            let b = window_arg(st, Some(b))?;
            if !st.stack(a, b) {
                return Err("those windows are already stacked".into());
            }
            Response::Ok
        }
        Request::Unstack { id } => {
            let id = window_arg(st, id)?;
            if !st.unstack(id, None) {
                return Err(format!("window {id} isn't stacked"));
            }
            Response::Ok
        }
        Request::Move { id, x, y } => {
            let id = window_arg(st, id)?;
            st.move_window(id, x, y);
            Response::Ok
        }
        Request::Resize { id, rect } => {
            let id = window_arg(st, id)?;
            st.resize_window(id, Rect::new(rect.x, rect.y, rect.w, rect.h));
            Response::Ok
        }
        Request::SetAudible { pids } => {
            let audible = with_parents(pids.iter().map(|p| *p as i32));
            let l = st.lg.m();
            if l.audible != audible {
                l.audible = audible;
                if let Some(o) = l.overlays.overview.as_ref().map(|o| o.output.clone()) {
                    st.az.queue_redraw(&o);
                }
            }
            Response::Ok
        }
        Request::Overview { state } => {
            let open = st.lg.r().overlays.overview_open();
            if state.unwrap_or(!open) != open {
                crate::overlays::toggle_overview(st);
            }
            Response::Ok
        }
        Request::Switcher { reverse, state } => {
            match state {
                Some(false) => crate::overlays::close_switcher(st, false),
                _ => crate::overlays::open_switcher(st, reverse, false),
            }
            Response::Ok
        }
        Request::Action { action } if action == "test-panic" && crate::VARIANT.is_some() => {
            panic!("the test variant panics on purpose")
        }
        Request::Action { action } => {
            let a = crate::bindings::Action::parse(&action).ok_or_else(|| format!("unknown action {action}"))?;
            st.run_action(a);
            Response::Ok
        }
        Request::PickArea { tag, prompt, targets } => {
            let report = crate::overlays::area::Report::new(tag, prompt, &targets);
            crate::overlays::start_area_pick(st, Some(report));
            Response::Ok
        }
        _ => return Ok(None),
    }))
}

/// Processes and their parents, up to init: a window belongs to the
/// process that drew it, but a browser plays sound from a child of it.
fn with_parents(pids: impl Iterator<Item = i32>) -> std::collections::HashSet<i32> {
    let mut out = std::collections::HashSet::new();
    for mut pid in pids {
        for _ in 0..16 {
            if pid <= 1 || !out.insert(pid) {
                break;
            }
            match parent_of(pid) {
                Some(p) => pid = p,
                None => break,
            }
        }
    }
    out
}

/// A process's parent, from `/proc/PID/stat` (its fourth field, after the
/// name in parentheses, which may hold spaces).
fn parent_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod audible_tests {
    #[test]
    fn a_process_brings_its_parents() {
        let me = std::process::id() as i32;
        let parent = super::parent_of(me).expect("this test has a parent");
        let set = super::with_parents([me].into_iter());
        assert!(set.contains(&me) && set.contains(&parent), "{set:?}");
        assert!(!set.contains(&1) && !set.contains(&0), "init and nothing aren't anyone's sound");
        assert!(super::with_parents([0, -3].into_iter()).is_empty());
    }
}
