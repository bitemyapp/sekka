//! The hardware and network panes over a copy of this machine's state:
//! NetworkManager, BlueZ, PipeWire and CUPS are read once and let go, and
//! nothing the panes do reaches them. Run by hand to look at real data:
//!
//! ```sh
//! SETTEI_LIVE_SNAPSHOT=1 cargo test -p aizome-settei --test live_snapshot -- --ignored
//! ```

mod common;

use aizome_kit::layout::Size;

#[test]
#[ignore = "reads this machine's services: pictures to look at, with SETTEI_LIVE_SNAPSHOT=1"]
fn panes_over_a_copy_of_this_machine() {
    if std::env::var_os("SETTEI_LIVE_SNAPSHOT").is_none() {
        eprintln!("skipping: SETTEI_LIVE_SNAPSHOT isn't set");
        return;
    }
    for pane in ["network", "bluetooth", "sound", "printers"] {
        let (mut h, _) = common::open(&format!("live-{pane}"), pane);
        let w = h.window();
        h.resize(w, Size::new(940.0, 1180.0));
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.send(settei::AppMsg::Nothing);
        }
        eprintln!("{pane}: {:?}", h.texts(w).iter().skip(22).take(30).collect::<Vec<_>>());
        h.save_png(w, common::shots_dir().join(format!("live-{pane}.png")), 2.0);
    }
}
