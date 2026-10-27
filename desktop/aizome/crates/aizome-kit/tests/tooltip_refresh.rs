//! Tooltip reconciliation with a stationary pointer, using real rendered pixels.
use std::time::Duration;

use aizome_kit::platform::OpenKind;
use aizome_kit::prelude::*;
use aizome_kit::testing::{Harness, write_png};

struct Probe {
    active: bool,
    show_tip: bool,
    ancestor: Option<&'static str>,
}
impl Default for Probe {
    fn default() -> Self {
        Self { active: false, show_tip: true, ancestor: None }
    }
}
#[derive(Clone)]
enum Msg {
    Changed,
    Remove,
    Add,
    Ancestor,
    Unchanged,
}
impl Program for Probe {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.TooltipProbe", "Tooltip Probe", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Tooltip Probe").size(400.0, 300.0));
    }
    fn update(&mut self, msg: Msg, _: &mut Cx<Msg>) {
        match msg {
            Msg::Changed => self.active = true,
            Msg::Remove => self.show_tip = false,
            Msg::Add => self.show_tip = true,
            Msg::Ancestor => self.ancestor = Some("Current ancestor guidance"),
            Msg::Unchanged => {}
        }
    }
    fn view(&self, _: WindowKey) -> Element<Msg> {
        let mut b = button(if self.active { "Swap Off" } else { "Swap On" })
            .on_press(Msg::Unchanged)
            .id("swap")
            .width(120.0)
            .height(30.0);
        if self.show_tip {
            b = b.tooltip(if self.active { "Stop using it as swap" } else { "Use it as swap" });
        }
        let mut body = container(b).padding(16.0);
        if let Some(tip) = self.ancestor {
            body = body.tooltip(tip);
        }
        body.into()
    }
}
fn popup_ids(h: &Harness<Probe>) -> Vec<u32> {
    h.platform
        .windows
        .iter()
        .filter_map(|(id, w)| matches!(w.spec.kind, OpenKind::Popup { .. }).then_some(*id))
        .collect()
}
fn picture(h: &mut Harness<Probe>, name: &str) -> ((u32, u32), Vec<u8>) {
    let ids = popup_ids(h);
    assert_eq!(ids.len(), 1, "exactly one tooltip");
    let image = h.render_popup(ids[0], 1.0);
    let size = image.pixel_size();
    let rgba = image.to_rgba8().unwrap();
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.png"));
    write_png(&path, size.0, size.1, &rgba).unwrap();
    (size, rgba)
}
fn hovering(p: Probe) -> Harness<Probe> {
    let mut h = Harness::new(p);
    let w = h.window();
    h.move_to(w, h.find_id(w, "swap").unwrap().center());
    h
}
fn shown(p: Probe) -> Harness<Probe> {
    let mut h = hovering(p);
    h.advance(Duration::from_millis(650));
    h
}
#[test]
fn shown_tooltip_tracks_async_state_change_with_stationary_pointer() {
    let mut h = shown(Probe::default());
    let w = h.window();
    let before = picture(&mut h, "tooltip-shown-before");
    h.send(Msg::Changed);
    assert!(h.texts(w).iter().any(|t| t == "Swap Off"));
    let after = picture(&mut h, "tooltip-shown-after");
    let expected = picture(&mut shown(Probe { active: true, ..Default::default() }), "tooltip-expected-current");
    h.move_to(w, Point::new(390.0, 290.0));
    h.move_to(w, h.find_id(w, "swap").unwrap().center());
    h.advance(Duration::from_millis(650));
    let rehover = picture(&mut h, "tooltip-rehover-control");
    assert!(rehover == expected, "moving away and back obtains current tooltip");
    println!(
        "old {:?}, stationary {:?}, expected {:?}; pixels unchanged={}",
        before.0,
        after.0,
        expected.0,
        before == after
    );
    assert!(after == expected, "stationary tooltip must match current state immediately, as rehover does");
}
#[test]
fn pending_text_change_preserves_original_deadline_and_uses_current_text() {
    let mut h = hovering(Probe::default());
    h.advance(Duration::from_millis(300));
    h.send(Msg::Changed);
    h.advance(Duration::from_millis(349));
    assert!(popup_ids(&h).is_empty());
    h.advance(Duration::from_millis(1));
    let actual = picture(&mut h, "tooltip-pending-current");
    let expected = picture(&mut shown(Probe { active: true, ..Default::default() }), "tooltip-pending-expected");
    assert!(actual == expected, "pending tooltip uses current text at original deadline");
}
#[test]
fn unchanged_rebuilds_and_pointer_moves_do_not_restart_or_reopen_tooltips() {
    let mut h = hovering(Probe::default());
    let w = h.window();
    let at = h.find_id(w, "swap").unwrap().center();
    h.advance(Duration::from_millis(300));
    h.send(Msg::Unchanged);
    h.move_to(w, at + Vec2::new(1.0, 0.0));
    h.advance(Duration::from_millis(350));
    let ids = popup_ids(&h);
    assert_eq!(ids.len(), 1);
    h.send(Msg::Unchanged);
    h.move_to(w, at);
    h.advance(Duration::from_secs(2));
    assert_eq!(popup_ids(&h), ids, "unchanged shown tooltip retains its window");
    h.click(w, at);
    assert!(popup_ids(&h).is_empty());
    h.send(Msg::Unchanged);
    h.move_to(w, at + Vec2::new(1.0, 0.0));
    h.advance(Duration::from_secs(2));
    assert!(popup_ids(&h).is_empty(), "click dismissal survives unchanged rebuild/movement");
}
#[test]
fn removed_shown_tooltip_closes_without_rehover() {
    let mut h = shown(Probe::default());
    assert_eq!(popup_ids(&h).len(), 1);
    h.send(Msg::Remove);
    assert!(popup_ids(&h).is_empty());
    h.advance(Duration::from_secs(2));
    assert!(popup_ids(&h).is_empty());
}
#[test]
fn removed_pending_tooltip_cannot_appear_later() {
    let mut h = hovering(Probe::default());
    h.advance(Duration::from_millis(300));
    h.send(Msg::Remove);
    h.advance(Duration::from_secs(2));
    assert!(popup_ids(&h).is_empty());
}
#[test]
fn added_tooltip_starts_delay_without_pointer_movement() {
    let mut h = hovering(Probe { show_tip: false, ..Default::default() });
    h.advance(Duration::from_secs(2));
    assert!(popup_ids(&h).is_empty());
    h.send(Msg::Add);
    h.advance(Duration::from_millis(649));
    assert!(popup_ids(&h).is_empty());
    h.advance(Duration::from_millis(1));
    assert_eq!(popup_ids(&h).len(), 1);
}
#[test]
fn ancestor_text_and_nearest_source_changes_refresh_same_hot_child() {
    let mut h = shown(Probe { show_tip: false, ancestor: Some("Old ancestor guidance"), ..Default::default() });
    h.send(Msg::Ancestor);
    let actual = picture(&mut h, "tooltip-ancestor-after");
    let expected = picture(
        &mut shown(Probe { show_tip: false, ancestor: Some("Current ancestor guidance"), ..Default::default() }),
        "tooltip-ancestor-expected",
    );
    assert!(actual == expected, "ancestor text changes even though hot child is unchanged");
    h.send(Msg::Add);
    let child = picture(&mut h, "tooltip-nearest-child");
    let expected_child = picture(&mut shown(Probe::default()), "tooltip-nearest-expected");
    assert!(child == expected_child, "new nearer source replaces ancestor tooltip");
    h.send(Msg::Remove);
    assert!(picture(&mut h, "tooltip-ancestor-restored") == expected, "removed nearer source reveals ancestor");
}
#[test]
fn pending_source_change_restarts_delay_and_moving_off_closes_it() {
    let mut h = hovering(Probe { ancestor: Some("Old ancestor guidance"), ..Default::default() });
    let w = h.window();
    h.advance(Duration::from_millis(300));
    h.send(Msg::Remove);
    h.advance(Duration::from_millis(649));
    assert!(popup_ids(&h).is_empty());
    h.advance(Duration::from_millis(1));
    assert_eq!(popup_ids(&h).len(), 1);
    h.move_to(w, Point::new(-10.0, -10.0));
    assert!(popup_ids(&h).is_empty());
    h.advance(Duration::from_secs(2));
    assert!(popup_ids(&h).is_empty());
}
