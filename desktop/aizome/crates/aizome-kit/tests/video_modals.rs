//! Native video must not cover the kit's modal sheets or their dimming layer.

use std::path::PathBuf;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Player {
    main: Option<WindowKey>,
    sheet: Option<WindowKey>,
    opened: Vec<Option<Vec<PathBuf>>>,
    saved: Vec<Option<PathBuf>>,
}

#[derive(Clone)]
enum Msg {
    Sheet,
    CloseSheet,
    Alert,
    Open,
    Save,
    Opened(Option<Vec<PathBuf>>),
    Saved(Option<PathBuf>),
}

impl Program for Player {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.VideoModalTest", "Video modal test", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.main = Some(cx.open_window(WindowSpec::new("Video").size(720.0, 520.0).video()));
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        let main = self.main.unwrap();
        match msg {
            Msg::Sheet => self.sheet = Some(cx.show_sheet(main)),
            Msg::CloseSheet => cx.close_window(self.sheet.take().unwrap()),
            Msg::Alert => {
                cx.alert(main, Alert::new("Pause playback?", "This must cover the video.").cancel_button("Cancel"))
            }
            Msg::Open => cx.open_panel(main, FileDialog::new().directory(env!("CARGO_TARGET_TMPDIR")), Msg::Opened),
            Msg::Save => cx.save_panel(
                main,
                FileDialog::new().directory(env!("CARGO_TARGET_TMPDIR")).file_name("video-frame.png"),
                Msg::Saved,
            ),
            Msg::Opened(paths) => self.opened.push(paths),
            Msg::Saved(path) => self.saved.push(path),
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.sheet {
            return aizome_kit::column![
                text("Media Info"),
                text("H.264, 640 by 360"),
                button("Done").default().on_press(Msg::CloseSheet),
            ]
            .padding(16.0)
            .width(400.0)
            .into();
        }
        aizome_kit::video_surface(7).picture(640.0, 360.0).into()
    }
}

fn player() -> (Harness<Player>, WindowKey) {
    let mut h = Harness::new(Player::default());
    let w = h.window();
    h.focus_window(w, true);
    assert_video_below(&h, false);
    (h, w)
}

fn assert_video_below(h: &Harness<Player>, expected: bool) {
    assert_eq!(h.platform.videos.len(), 1, "the video stays placed while a modal opens or closes");
    let video = h.platform.videos.values().next().unwrap();
    assert_eq!(video.below, expected, "modal pixels must cover video; unobstructed video can use its own plane");
}

#[test]
fn sheet_and_nested_alert_cover_video_until_the_last_modal_closes() {
    let (mut h, w) = player();
    h.send(Msg::Sheet);
    assert!(h.texts(w).iter().any(|s| s == "Media Info"));
    assert_video_below(&h, true);
    h.send(Msg::Alert);
    assert_video_below(&h, true);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().sheet.is_some());
    assert_video_below(&h, true);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().sheet.is_none());
    assert_video_below(&h, false);

    h.send(Msg::Alert);
    assert_video_below(&h, true);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_video_below(&h, false);
}

#[test]
fn open_and_save_panels_cover_video_and_restore_it_on_cancel_or_accept() {
    let (mut h, w) = player();
    h.send(Msg::Open);
    assert_video_below(&h, true);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().opened, [None]);
    assert_video_below(&h, false);

    h.send(Msg::Save);
    assert_video_below(&h, true);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().saved, [None]);
    assert_video_below(&h, false);

    h.send(Msg::Save);
    assert_video_below(&h, true);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().saved[1], Some(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("video-frame.png")));
    assert_video_below(&h, false);
}
