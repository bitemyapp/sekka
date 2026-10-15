//! Kakehashi (架け橋, a bridge): writes Sekka, or any disk image, to a USB
//! drive, then reads every byte back. It lists Sekka's nightly builds and
//! downloads the one you pick, or takes an image you have.
//!
//! The window runs as you. Writing a drive needs administrator rights, so
//! one step, kakehashi-helper, runs through the system's own prompt; the
//! window watches its progress.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod elevate;
#[cfg(target_os = "macos")]
mod staged_image;
mod theme;
mod view;
mod when;

use gpui_kit::{AppContext as _, TitlebarOptions, WindowBounds, WindowOptions, px, size};

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(760.), px(780.)), cx)),
            titlebar: Some(TitlebarOptions { title: Some("Kakehashi".into()), ..Default::default() }),
            app_id: Some("computer.sekka.Kakehashi".into()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| view::Kakehashi::new(window, cx)))
            .expect("could not open Kakehashi's window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
