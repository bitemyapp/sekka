//! Refresh requests must compose across a batch without hiding kit work.

use std::cell::RefCell;

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;

#[derive(Default)]
struct App {
    windows: Vec<WindowKey>,
    values: [usize; 2],
    views: RefCell<Vec<WindowKey>>,
    sheet: Option<WindowKey>,
    menu_name: bool,
}

#[derive(Clone)]
enum Msg {
    One(usize),
    Both,
    Unchanged,
    Open,
    Sheet,
    EditSheet,
    CloseSheet,
    Hide,
    Show,
    RenameMenu,
}

impl Program for App {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.RefreshTest", "Refresh", "Test")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        for title in ["First", "Second"] {
            self.windows.push(cx.open_window(WindowSpec::new(title).size(320.0, 240.0)));
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::One(i) => {
                self.values[i] += 1;
                cx.refresh_window(self.windows[i]);
            }
            Msg::Both => self.values.iter_mut().for_each(|n| *n += 1),
            Msg::Unchanged => cx.skip_view_refresh(),
            Msg::Open => {
                self.windows.push(cx.open_window(WindowSpec::new("Third")));
                cx.skip_view_refresh();
            }
            Msg::Sheet => {
                self.sheet = Some(cx.show_sheet(self.windows[0]));
                cx.skip_view_refresh();
            }
            Msg::EditSheet => {
                self.values[0] += 1;
                cx.refresh_window(self.sheet.unwrap());
            }
            Msg::CloseSheet => {
                cx.close_window(self.sheet.take().unwrap());
                cx.skip_view_refresh();
            }
            Msg::Hide => {
                cx.hide_window(self.windows[0]);
                cx.skip_view_refresh();
            }
            Msg::Show => {
                cx.show_window(self.windows[0]);
                cx.skip_view_refresh();
            }
            Msg::RenameMenu => {
                self.menu_name = !self.menu_name;
                cx.skip_view_refresh();
            }
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        self.views.borrow_mut().push(window);
        let i = self.windows.iter().position(|w| *w == window).unwrap_or(0).min(1);
        text(format!("Value {}", self.values[i])).into()
    }

    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info()).with_file_menu(Menu::new(if self.menu_name { "Document" } else { "File" }))
    }
}

fn clear(h: &mut Harness<App>) {
    h.program().views.borrow_mut().clear();
    h.reset_stats();
    for window in h.program().windows.clone() {
        h.take_damage(window);
    }
}

#[test]
fn a_local_update_views_only_its_window_and_a_noop_views_none() {
    let mut h = Harness::new(App::default());
    let [first, second] = h.program().windows[..] else { panic!() };
    clear(&mut h);
    h.send(Msg::One(0));
    assert_eq!(*h.program().views.borrow(), [first]);
    assert!(h.texts(first).contains(&"Value 1".into()));
    assert!(h.texts(second).contains(&"Value 0".into()));
    assert!(h.take_damage(second).is_empty());

    clear(&mut h);
    h.send(Msg::Unchanged);
    assert!(h.program().views.borrow().is_empty());
    assert!(h.stats().view_layout.is_empty());
}

#[test]
fn batches_union_requests_and_an_unannotated_update_still_refreshes_all() {
    let mut h = Harness::new(App::default());
    let windows = h.program().windows.clone();
    for messages in [
        vec![Msg::One(0), Msg::One(1), Msg::Unchanged],
        vec![Msg::Both, Msg::Unchanged],
        vec![Msg::Unchanged, Msg::Both],
        vec![Msg::One(0), Msg::Both, Msg::Unchanged],
    ] {
        clear(&mut h);
        h.send_all(messages);
        assert_eq!(*h.program().views.borrow(), windows);
        assert_eq!(h.stats().view_layout.len(), 2);
    }
}

#[test]
fn opening_and_closing_sheets_preserves_lifecycle_refreshes() {
    let mut h = Harness::new(App::default());
    let first = h.program().windows[0];
    clear(&mut h);
    h.send(Msg::Sheet);
    let sheet = h.program().sheet.unwrap();
    assert_eq!(*h.program().views.borrow(), [first, sheet]);

    clear(&mut h);
    h.send(Msg::EditSheet);
    assert_eq!(*h.program().views.borrow(), [first, sheet]);
    assert!(h.texts(first).contains(&"Value 1".into()));

    clear(&mut h);
    h.send(Msg::CloseSheet);
    assert_eq!(*h.program().views.borrow(), [first]);

    clear(&mut h);
    h.send(Msg::Open);
    assert_eq!(*h.program().views.borrow(), [h.program().windows[2]]);
}

#[test]
fn hidden_windows_refresh_when_shown_after_a_targeted_update() {
    let mut h = Harness::new(App::default());
    let first = h.program().windows[0];
    h.send(Msg::Hide);
    clear(&mut h);
    h.send(Msg::One(0));
    assert!(h.program().views.borrow().is_empty());
    h.send(Msg::Show);
    assert_eq!(*h.program().views.borrow(), [first]);
    assert!(h.texts(first).contains(&"Value 1".into()));
}

#[test]
fn in_window_menu_titles_stay_current_even_when_content_did_not_change() {
    let mut h = Harness::with(App::default(), Default::default(), true);
    // A new title must refresh the strip in both windows.
    clear(&mut h);
    h.send(Msg::RenameMenu);
    assert_eq!(*h.program().views.borrow(), h.program().windows);
}
