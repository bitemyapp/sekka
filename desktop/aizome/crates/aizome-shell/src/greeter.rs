//! Publish saved display scales for the next login screen. One worker keeps
//! writes ordered and D-Bus waits off the shell's event loop.

use aizome_config::greeter::DisplayScales;
use std::sync::mpsc::{Sender, channel};

pub fn start() -> Sender<DisplayScales> {
    let (tx, rx) = channel::<DisplayScales>();
    std::thread::Builder::new()
        .name("login display scales".into())
        .spawn(move || {
            while let Ok(mut scales) = rx.recv() {
                for newer in rx.try_iter() {
                    scales = newer;
                }
                let Ok(text) = scales.to_toml() else { continue };
                let result =
                    aizome_sys::Bus::system().and_then(|bus| aizome_sys::users::publish_display_scales(&bus, &text));
                if let Err(e) = result {
                    log::warn!("saving display scales for the login screen: {e}");
                }
            }
        })
        .ok();
    tx
}
