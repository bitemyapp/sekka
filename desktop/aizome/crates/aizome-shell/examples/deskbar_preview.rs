//! The Deskbar in a window of its own, with sample apps, windows,
//! notifications and Shelf items: to try its sizes and its move and resize
//! controls without restarting the shell (and the apps it started).
//!
//! `cargo run --release -p aizome-shell --example deskbar_preview`
//!
//! The window stands for the screen: drag the grip to the other half to
//! move the Deskbar, drag its inner edge to resize it, right-click the grip
//! (or use the Deskbar menu) for sides and sizes. Nothing is saved.

use aizome_config::DeskbarSide;
use aizome_kit::prelude::*;
use aizome_kit::{Icon, Menu, MenuBar, MenuItem};
use aizome_shell::deskbar::{
    self, AppRow, DeskbarModel, DeskbarMsg, NoteCard, RAIL_WIDTH, SHADOW, ShelfRow, WIDTH, WindowRow, clamp_width,
};
use aizome_shell::look::Hue;
use aizome_shell::widgets::drawn::{DragPhase, drawn};
use aizome_shell::wm::WindowNote;

const SCREEN: (f64, f64) = (1100.0, 720.0);

#[derive(Clone, Debug)]
enum Msg {
    Deskbar(DeskbarMsg),
    Side(DeskbarSide),
    Width(f64),
}

struct Preview {
    window: Option<WindowKey>,
    model: DeskbarModel,
    status: String,
}

fn window(id: u64, title: &str, focused: bool, note: WindowNote) -> WindowRow {
    WindowRow { id, title: title.into(), focused, note, urgent: false }
}

fn app(id: &str, name: &str, glyph: Icon, hue: Hue, windows: Vec<WindowRow>) -> AppRow {
    let focused = windows.iter().any(|w| w.focused);
    AppRow {
        app_id: id.into(),
        name: name.into(),
        glyph,
        hue,
        image: None,
        folded: windows.len() >= 3 && !focused,
        focused,
        unread: 0,
        cards: vec![],
        windows,
    }
}

fn sample() -> DeskbarModel {
    let mut mail =
        app("thunderbird", "Thunderbird", Icon::Mail, Hue::Amber, vec![window(9, "Inbox", false, WindowNote::None)]);
    mail.unread = 3;
    mail.cards = vec![NoteCard {
        id: 12,
        summary: "Re: tiles at fractional scale".into(),
        body: "“Half a pixel off rather than blurred…”".into(),
        buttons: vec![],
    }];
    let shelf = |label: &str, glyph, hue, dim| ShelfRow {
        label: label.into(),
        glyph,
        hue,
        dim,
        uri: None,
        drag_text: label.into(),
    };
    DeskbarModel {
        apps: vec![
            app(
                "org.aizome.Sumi",
                "Editor",
                Icon::Code,
                Hue::Violet,
                vec![
                    window(1, "architecture.md", true, WindowNote::None),
                    window(2, "roadmap.md", false, WindowNote::Hidden),
                ],
            ),
            app("foot", "Foot", Icon::Terminal, Hue::Green, vec![window(3, "cargo test", false, WindowNote::Stack)]),
            app(
                "org.aizome.Hako",
                "Hako",
                Icon::Folder,
                Hue::Blue,
                vec![
                    window(4, "sidestep", false, WindowNote::Stack),
                    window(5, "Downloads", false, WindowNote::Desk(1)),
                ],
            ),
            app(
                "firefox",
                "Firefox",
                Icon::Globe,
                Hue::Cyan,
                vec![
                    window(6, "docs.rs", false, WindowNote::None),
                    window(7, "GitHub", false, WindowNote::None),
                    window(8, "Sekka", false, WindowNote::None),
                ],
            ),
            mail,
        ],
        shelf: vec![
            shelf("wallpaper-draft.png", Icon::FileImage, Hue::Violet, false),
            shelf("“damage is per layer, not…”", Icon::TextAlignStart, Hue::Green, true),
            shelf("release-notes/", Icon::Folder, Hue::Blue, false),
        ],
        hide_shelf: false,
        max_height: Some(SCREEN.1),
        side: DeskbarSide::Left,
        width: WIDTH,
        moving: deskbar::Moving::No,
    }
}

impl Preview {
    fn place(&mut self, side: Option<DeskbarSide>, width: Option<f64>) {
        if let Some(s) = side {
            self.model.side = s;
        }
        if let Some(w) = width {
            self.model.width = clamp_width(w);
        }
        let size = match deskbar::density(self.model.width) {
            deskbar::Density::Rail => "Icons Only",
            deskbar::Density::Slim => "Slim",
            deskbar::Density::Full => "Full",
        };
        let side = if self.model.side == DeskbarSide::Left { "left" } else { "right" };
        self.status = format!("{size}, {} pt, at the {side}", self.model.width);
    }

    /// The Deskbar's top left in the window (the screen).
    fn origin(&self) -> f64 {
        match self.model.side {
            DeskbarSide::Left => 0.0,
            DeskbarSide::Right => SCREEN.0 - self.model.width - SHADOW,
        }
    }

    fn place_menu(&self) -> Menu<Msg> {
        let (side, w) = (self.model.side, self.model.width);
        Menu::new("Deskbar")
            .item(MenuItem::new("At the Left", Msg::Side(DeskbarSide::Left)).checked(side == DeskbarSide::Left))
            .item(MenuItem::new("At the Right", Msg::Side(DeskbarSide::Right)).checked(side == DeskbarSide::Right))
            .separator()
            .item(MenuItem::new("Icons Only", Msg::Width(RAIL_WIDTH)).checked(w == RAIL_WIDTH))
            .item(MenuItem::new("Slim", Msg::Width(150.0)).checked(w == 150.0))
            .item(MenuItem::new("Standard", Msg::Width(WIDTH)).checked(w == WIDTH))
            .item(MenuItem::new("Wide", Msg::Width(300.0)).checked(w == 300.0))
    }
}

impl Program for Preview {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.DeskbarPreview", "Deskbar Preview", "Deskbar Preview").icon(Icon::Folder)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.place(None, None);
        self.window =
            Some(cx.open_window(WindowSpec::new("Deskbar Preview").size(SCREEN.0, SCREEN.1).resizable(false)));
    }

    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info()).with_menu(self.place_menu())
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Side(s) => self.place(Some(s), None),
            Msg::Width(w) => self.place(None, Some(w)),
            // Points here are the window's: the screen's.
            Msg::Deskbar(DeskbarMsg::Grip(DragPhase::End(p))) => {
                let side = if p.x < SCREEN.0 / 2.0 { DeskbarSide::Left } else { DeskbarSide::Right };
                self.place(Some(side), None);
            }
            Msg::Deskbar(DeskbarMsg::Edge(DragPhase::Move(p) | DragPhase::End(p))) => {
                let w = match self.model.side {
                    DeskbarSide::Left => p.x,
                    DeskbarSide::Right => SCREEN.0 - p.x,
                };
                self.place(None, Some(w));
            }
            Msg::Deskbar(DeskbarMsg::PlaceMenu(at)) => {
                if let Some(w) = self.window {
                    cx.show_context_menu(w, Point::new(self.origin() + at.x, at.y), self.place_menu());
                }
            }
            Msg::Deskbar(DeskbarMsg::Fold(id)) => {
                if let Some(a) = self.model.apps.iter_mut().find(|a| a.app_id == id) {
                    a.folded = !a.folded;
                }
            }
            Msg::Deskbar(DeskbarMsg::FocusWindow(id)) => {
                for a in &mut self.model.apps {
                    for w in &mut a.windows {
                        w.focused = w.id == id;
                    }
                    a.focused = a.windows.iter().any(|w| w.focused);
                }
            }
            Msg::Deskbar(other) => self.status = format!("{other:?}"),
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        let status = self.status.clone();
        // The screen: a plain ground and what the Deskbar is doing.
        let ground = drawn(SCREEN.0, SCREEN.1, ("ground", status.clone()), move |cx| {
            let t = cx.theme();
            let b = cx.bounds();
            cx.canvas.fill_rect(b, aizome_shell::look::mix(t.sunk(), t.focus(), 0.08));
            let s = aizome_shell::look::caption(t);
            cx.draw_text_in(&status, &s, Rect::new(0.0, b.y1 - 40.0, b.x1, b.y1 - 16.0), Align::Center);
        });
        let bar = deskbar::view(&self.model).map(Msg::Deskbar);
        aizome_kit::stack(vec![
            ground.into(),
            aizome_kit::container(bar).padding([0.0, 0.0, 0.0, self.origin()]).into(),
        ])
        .width(SCREEN.0)
        .height(SCREEN.1)
        .into()
    }
}

fn main() {
    aizome_kit::run(Preview { window: None, model: sample(), status: String::new() });
}
