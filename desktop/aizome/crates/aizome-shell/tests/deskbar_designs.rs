//! Pictures of the Deskbar's sizes and its move and resize controls, over
//! a wallpaper, cropped to it: for choosing between designs, not checked.
//!
//! `DESKBAR_DESIGNS=<folder> DESKBAR_WALLPAPER=<image> cargo test -p
//! aizome-shell --test deskbar_designs -- --ignored`

mod common;

use aizome_config::DeskbarSide;
use aizome_kit::prelude::*;
use aizome_shell::deskbar::DeskbarMsg;
use aizome_shell::shell::{Msg, Shell};
use common::*;
use std::path::PathBuf;

type H = aizome_kit::testing::Harness<Shell>;

fn place(h: &mut H, side: DeskbarSide, width: f64) {
    let mut d = h.program().settings.deskbar_settings();
    d.side = side;
    d.width = width;
    d.collapsed = false;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);
}

fn scene_with(day: bool, side: DeskbarSide, width: f64) -> H {
    let mut h = scene(day);
    with_mail_and_shelf(&mut h);
    {
        let s = h.program_mut();
        if let Some(w) = std::env::var_os("DESKBAR_WALLPAPER") {
            let w = w.to_string_lossy().into_owned();
            s.settings.appearance.wallpaper_day = Some(w.clone());
            s.settings.appearance.wallpaper_night = Some(w);
        }
        s.theme_day = day;
        s.load_wallpapers();
        s.banner = None;
        s.banner_queue.clear();
    }
    place(&mut h, side, width);
    h.send(Msg::Nothing);
    h
}

/// The Deskbar's top left on the screen.
fn deskbar_at(h: &H) -> (WindowKey, Point, Size) {
    let db = window_titled(h, "Deskbar");
    let m = h.program().deskbar_model();
    let size = aizome_shell::deskbar::size(&m);
    let x = match m.side {
        DeskbarSide::Left => 0.0,
        DeskbarSide::Right => W - size.width,
    };
    (db, Point::new(x, 24.0), size)
}

/// The screen: wallpaper, bar, Deskbar, and a popup of the Deskbar's.
fn screen(h: &mut H, popup: Option<(u32, Rect)>) -> (u32, u32, Vec<u8>) {
    let desk = window_titled(h, "Desktop");
    let bar = window_titled(h, "Menu bar");
    let (db, at, size) = deskbar_at(h);
    h.resize(db, size);
    let (w, hh, mut px) = compose(h, &[(desk, Point::ZERO), (bar, Point::ZERO), (db, at)], Size::new(W, H), 2.0);
    if let Some((menu, r)) = popup {
        let img = h.render_popup(menu, 2.0);
        over(&mut px, w, &img, Point::new(at.x + r.x0, at.y + r.y0));
    }
    (w, hh, px)
}

/// Lay a popup's picture (premultiplied) over `px` at `at` (points, 2×).
fn over(px: &mut [u8], width: u32, img: &aizome_kit::Image, at: Point) {
    let (w, hh) = img.pixel_size();
    let src = img.to_premultiplied_rgba8().expect("pixels");
    let (ox, oy) = ((at.x * 2.0) as u32, (at.y * 2.0) as u32);
    for y in 0..hh {
        for x in 0..w {
            let (dx, dy) = (ox + x, oy + y);
            if dx >= width || (dy * width + dx) as usize * 4 >= px.len() {
                continue;
            }
            let s = ((y * w + x) * 4) as usize;
            let d = ((dy * width + dx) * 4) as usize;
            let a = src[s + 3] as u32;
            for c in 0..3 {
                px[d + c] = (src[s + c] as u32 + px[d + c] as u32 * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

/// Save the part of the screen `r` (points) shows.
fn save(name: &str, (w, _, px): &(u32, u32, Vec<u8>), r: Rect) {
    let dir = PathBuf::from(std::env::var_os("DESKBAR_DESIGNS").unwrap_or_else(|| shots().into_os_string()));
    std::fs::create_dir_all(&dir).unwrap();
    let (x0, y0) = ((r.x0 * 2.0) as u32, (r.y0 * 2.0) as u32);
    let (cw, ch) = (((r.x1 - r.x0) * 2.0) as u32, ((r.y1 - r.y0) * 2.0) as u32);
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..y0 + ch {
        let s = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&px[s..s + (cw * 4) as usize]);
    }
    aizome_kit::testing::write_png(&dir.join(name), cw, ch, &out).unwrap();
}

/// The crop around the Deskbar: `extra` points beside it, and the bar.
fn around(h: &H, extra: f64) -> Rect {
    let (_, at, size) = deskbar_at(h);
    let bottom = (at.y + size.height + 24.0).min(H);
    match h.program().deskbar_model().side {
        DeskbarSide::Left => Rect::new(0.0, 0.0, size.width + extra, bottom),
        DeskbarSide::Right => Rect::new(W - size.width - extra, 0.0, W, bottom),
    }
}

#[test]
#[ignore]
fn pictures_of_the_deskbar_designs() {
    for day in [false, true] {
        let t = if day { "day" } else { "night" };
        for (name, width) in
            [("1-full-300", 300.0), ("2-standard-232", 232.0), ("3-slim-150", 150.0), ("4-rail-48", 48.0)]
        {
            let mut h = scene_with(day, DeskbarSide::Left, width);
            let shot = screen(&mut h, None);
            save(&format!("{name}-left-{t}.png"), &shot, around(&h, 260.0));
        }
        let mut h = scene_with(day, DeskbarSide::Right, 48.0);
        let shot = screen(&mut h, None);
        save(&format!("5-rail-48-right-{t}.png"), &shot, around(&h, 260.0));
    }

    // The controls, at night: the grip under the pointer, the edge under it,
    // the grip's menu.
    let mut h = scene_with(false, DeskbarSide::Left, 232.0);
    let (db, _, size) = deskbar_at(&h);
    h.resize(db, size);
    h.move_to(db, Point::new(116.0, 5.0));
    let shot = screen(&mut h, None);
    save("6-control-grip-hover.png", &shot, around(&h, 200.0));
    h.move_to(db, Point::new(232.0 - 2.0, 120.0));
    let shot = screen(&mut h, None);
    save("7-control-edge-hover.png", &shot, around(&h, 200.0));
    h.move_to(db, Point::new(116.0, 5.0));
    h.send(Msg::Deskbar(DeskbarMsg::PlaceMenu(Point::new(116.0, 8.0))));
    let popup = h.popups(db).first().copied();
    assert!(popup.is_some(), "the grip's menu opened");
    let shot = screen(&mut h, popup);
    save("8-control-menu.png", &shot, around(&h, 260.0));

    // Dragging the edge in: Standard, then past Slim to the Rail.
    let mut h = scene_with(false, DeskbarSide::Left, 232.0);
    let (db, _, size) = deskbar_at(&h);
    h.resize(db, size);
    h.send(Msg::Deskbar(DeskbarMsg::Edge(aizome_shell::widgets::drawn::DragPhase::Start(Point::new(230.0, 100.0)))));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(aizome_shell::widgets::drawn::DragPhase::End(Point::new(70.0, 100.0)))));
    assert_eq!(h.program().deskbar_model().width, aizome_shell::deskbar::RAIL_WIDTH, "snapped to the Rail");
    // And the grip let go on the right half moves it there.
    h.send(Msg::Deskbar(DeskbarMsg::Grip(aizome_shell::widgets::drawn::DragPhase::End(Point::new(1000.0, 5.0)))));
    assert_eq!(h.program().deskbar_model().side, DeskbarSide::Right);
}
