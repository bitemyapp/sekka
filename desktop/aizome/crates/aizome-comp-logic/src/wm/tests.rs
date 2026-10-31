use super::*;

const INSETS: Insets = Insets::new(25, 1, 1, 1);
const USABLE: Rect = Rect::new(0, 24, 1440, 876);

fn model() -> Model {
    Model::new((1..=4).map(|n| (n.to_string(), None)))
}

fn add(m: &mut Model, app: &str) -> WinId {
    let id = m.insert(app, app, None, Rect::new(100, 100, 400, 300), None);
    m.focus(Some(id));
    id
}

#[test]
fn new_windows_stack_on_top_and_take_focus_when_asked() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    assert_eq!(m.stacking(), &[a, b]);
    assert_eq!(m.focused(), Some(b));
    assert_eq!(m.mru(), &[b, a]);
    m.focus(Some(a));
    assert_eq!(m.stacking(), &[b, a]);
    assert_eq!(m.mru(), &[a, b]);
}

#[test]
fn closing_focuses_the_most_recent_window() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    m.focus(Some(a));
    // MRU is now a, c, b.
    assert_eq!(m.remove(a), Refocus::To(Some(c)));
    m.focus(Some(c));
    assert_eq!(m.remove(b), Refocus::Same);
    assert_eq!(m.remove(c), Refocus::To(None));
    assert_eq!(m.len(), 0);
}

#[test]
fn minimise_and_hide_move_focus_on() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let b2 = add(&mut m, "b");
    assert_eq!(m.minimise(b2), Refocus::To(Some(b)));
    m.focus(Some(b));
    assert!(!m.is_shown(b2));
    assert_eq!(m.hide_app("b"), Refocus::To(Some(a)));
    m.focus(Some(a));
    assert!(!m.is_shown(b));
    m.show_app("b");
    assert!(m.is_shown(b));
    assert!(!m.is_shown(b2), "still minimised");
    assert!(m.reveal(b2).is_empty());
    assert!(m.is_shown(b2));
    assert_eq!(m.hide_others("a"), Refocus::Same);
    assert!(!m.is_shown(b) && m.is_shown(a));
}

#[test]
fn lowering_sends_to_the_back_and_focuses_the_top() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    assert_eq!(m.lower(b), Refocus::To(Some(a)));
    assert_eq!(m.stacking(), &[b, a]);
}

#[test]
fn dialogs_ride_above_their_parent() {
    let mut m = model();
    let p = add(&mut m, "p");
    let d = m.insert("p", "Save?", None, Rect::new(0, 0, 200, 100), Some(p));
    let o = add(&mut m, "o");
    assert_eq!(m.stacking(), &[p, d, o]);
    m.focus(Some(p));
    assert_eq!(m.stacking(), &[o, p, d]);
}

#[test]
fn placement_centres_then_cascades() {
    let mut m = model();
    let r1 = m.place((400, 300), USABLE, &[], INSETS, None);
    // Frame 402 x 326 centred in 1440 x 876 from y 24.
    assert_eq!(r1, Rect::new((1440 - 402) / 2 + 1, 24 + (876 - 326) / 2 + 25, 400, 300));
    let a = m.insert("a", "a", None, r1, None);
    m.focus(Some(a));
    let r2 = m.place((400, 300), USABLE, &[], INSETS, None);
    assert_eq!((r2.x, r2.y), (r1.x + CASCADE_STEP, r1.y + CASCADE_STEP));
}

#[test]
fn placement_avoids_the_deskbar() {
    let m = model();
    let deskbar = Rect::new(1440 - 232, 24, 232, 600);
    let r = m.place((400, 300), USABLE, &[deskbar], INSETS, None);
    assert!(r.right() < deskbar.x, "{r:?} overlaps the Deskbar");
    // Centred in what is left.
    let area_w = 1440 - 232;
    assert_eq!(r.x, (area_w - 402) / 2 + 1);
}

#[test]
fn filling_leaves_the_deskbar_clear() {
    let deskbar = Rect::new(1440 - 232, 24, 232, 600);
    assert_eq!(beside(USABLE, Some(deskbar)), Rect::new(0, 24, 1440 - 232, 876));
    assert_eq!(beside(USABLE, None), USABLE);
    // On another output, or floating in the middle, it takes nothing.
    assert_eq!(beside(USABLE, Some(Rect::new(1440, 24, 232, 600))), USABLE);
    assert_eq!(beside(USABLE, Some(Rect::new(600, 300, 232, 200))), USABLE);
    let mut m = model();
    let a = add(&mut m, "a");
    let filled = m.set_fill(a, None, beside(USABLE, Some(deskbar)), INSETS).unwrap();
    assert!(filled.right() + INSETS.right <= deskbar.x, "{filled:?} runs under the Deskbar");
}

#[test]
fn placement_keeps_the_tab_on_screen() {
    let m = model();
    let r = m.place((1400, 2000), USABLE, &[], INSETS, None);
    assert!(r.y - INSETS.top >= USABLE.y);
    assert!(r.x - INSETS.left >= USABLE.x);
    assert!(r.right() + INSETS.right <= USABLE.right());
    assert!(r.bottom() + INSETS.bottom <= USABLE.bottom());
}

#[test]
fn crowded_cascades_and_dialogs_stay_inside_the_work_area() {
    for usable in [USABLE, Rect::new(-1280, -300, 1280, 800), Rect::new(1440, 24, 800, 576)] {
        for left in [false, true] {
            let deskbar = Rect::new(if left { usable.x } else { usable.right() - 232 }, usable.y, 232, 360);
            let area = beside(usable, Some(deskbar));
            for size in [(400, 300), (area.w - 2, area.h - 26), (2000, 2000)] {
                let mut m = model();
                for n in 0..70 {
                    let r = m.place(size, usable, &[deskbar], INSETS, None);
                    assert_eq!(r.outset(INSETS).intersection(area), Some(r.outset(INSETS)), "{r:?} in {area:?}");
                    let id = m.insert("a", &n.to_string(), None, r, None);
                    m.focus(Some(id));
                }
                let p = m.insert("parent", "parent", None, deskbar, None);
                let r = m.place(size, usable, &[deskbar], INSETS, Some(p));
                assert_eq!(r.outset(INSETS).intersection(area), Some(r.outset(INSETS)), "dialog {r:?}");
            }
        }
    }
}

#[test]
fn fitting_remembered_geometry_preserves_it_when_it_fits() {
    let r = Rect::new(200, 200, 600, 400);
    assert_eq!(fit_into(r, USABLE, INSETS), r);
    let area = beside(USABLE, Some(Rect::new(1208, 24, 232, 500)));
    assert_eq!(fit_into(Rect::new(1200, 800, 1800, 1200), area, INSETS), area.inset(INSETS));
}

#[test]
fn dialogs_centre_on_their_parent() {
    let mut m = model();
    let p = m.insert("p", "p", None, Rect::new(200, 200, 600, 400), None);
    let r = m.place((200, 100), USABLE, &[], INSETS, Some(p));
    assert_eq!(r.center(), (500, 400));
}

#[test]
fn fill_toggles_and_restores() {
    let mut m = model();
    let a = add(&mut m, "a");
    let before = m.get(a).unwrap().rect;
    let filled = m.set_fill(a, None, USABLE, INSETS).unwrap();
    assert_eq!(filled, USABLE.inset(INSETS));
    assert!(m.get(a).unwrap().fill());
    let back = m.set_fill(a, None, USABLE, INSETS).unwrap();
    assert_eq!(back, before);
    assert_eq!(m.get(a).unwrap().placement, Placement::Free);
}

#[test]
fn split_takes_half_and_again_restores() {
    let mut m = model();
    let a = add(&mut m, "a");
    let before = m.get(a).unwrap().rect;
    let left = m.set_split(a, Side::Left, USABLE, INSETS).unwrap();
    assert_eq!(left, Rect::new(0, 24, 720, 876).inset(INSETS));
    let right = m.set_split(a, Side::Right, USABLE, INSETS).unwrap();
    assert_eq!(right, Rect::new(720, 24, 720, 876).inset(INSETS));
    // From split to fill and back to free restores the original free rect.
    m.set_fill(a, Some(true), USABLE, INSETS);
    assert_eq!(m.set_fill(a, Some(false), USABLE, INSETS).unwrap(), before);
    m.set_split(a, Side::Left, USABLE, INSETS);
    assert_eq!(m.set_split(a, Side::Left, USABLE, INSETS).unwrap(), before);
}

#[test]
fn fullscreen_covers_the_output_and_restores() {
    let mut m = model();
    let a = add(&mut m, "a");
    let before = m.get(a).unwrap().rect;
    let out = Rect::new(0, 0, 1440, 900);
    assert_eq!(m.set_fullscreen(a, None, out), Some(out));
    assert!(m.get(a).unwrap().fullscreen());
    assert_eq!(m.set_fullscreen(a, None, out), Some(before));
}

#[test]
fn stacking_shares_geometry_and_shows_one() {
    let mut m = model();
    let a = add(&mut m, "term");
    let b = m.insert("files", "files", None, Rect::new(600, 500, 300, 200), None);
    m.focus(Some(b));
    let changed = m.stack(a, b).unwrap();
    assert_eq!(changed, vec![b]);
    let g = m.group_of(a).unwrap().clone();
    assert_eq!(g.members, vec![a, b]);
    assert_eq!(g.active, b);
    assert_eq!(m.get(b).unwrap().rect, m.get(a).unwrap().rect);
    assert!(m.is_shown(b) && !m.is_shown(a));
    assert_eq!(m.focused(), Some(b));
    // Focusing a member shows it.
    m.focus(Some(a));
    assert!(m.is_shown(a) && !m.is_shown(b));
    // Moving the group moves both.
    m.set_rect(a, Rect::new(10, 40, 500, 400));
    assert_eq!(m.get(b).unwrap().rect, Rect::new(10, 40, 500, 400));
}

#[test]
fn unstacking_leaves_the_rest() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    m.stack(a, b);
    m.stack(a, c);
    assert_eq!(m.group_of(a).unwrap().members, vec![a, b, c]);
    assert_eq!(m.group_of(a).unwrap().active, c);
    assert!(m.unstack(c));
    assert!(m.get(c).unwrap().group.is_none());
    // The neighbour shows.
    assert_eq!(m.group_of(a).unwrap().active, b);
    assert!(m.unstack(b));
    // A group of one dissolves.
    assert!(m.get(a).unwrap().group.is_none());
    assert_eq!(m.groups().count(), 0);
    assert!(!m.unstack(a));
}

#[test]
fn neighbour_tabs_wrap_round_the_group() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    assert_eq!(m.neighbour_tab(a, true), None, "a window on its own has no tabs");
    m.stack(a, b);
    m.stack(a, c);
    assert_eq!(m.neighbour_tab(a, true), Some(b));
    assert_eq!(m.neighbour_tab(c, true), Some(a));
    assert_eq!(m.neighbour_tab(a, false), Some(c));
    assert_eq!(m.neighbour_tab(b, false), Some(a));
    // Tab order, not the order they joined in.
    m.reorder_tab(c, 0);
    assert_eq!(m.neighbour_tab(c, true), Some(a));
    assert_eq!(m.neighbour_tab(c, false), Some(b));
}

#[test]
fn a_moved_tab_stops_at_the_ends_and_keeps_showing() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    assert!(!m.move_tab(a, true), "a window on its own has no tabs");
    m.stack(a, b);
    m.stack(a, c);
    assert!(m.move_tab(c, false));
    assert_eq!(m.group_of(a).unwrap().members, vec![a, c, b]);
    assert!(m.move_tab(c, false));
    assert_eq!(m.group_of(a).unwrap().members, vec![c, a, b]);
    assert!(!m.move_tab(c, false), "the first stays first");
    assert!(m.move_tab(a, true));
    assert_eq!(m.group_of(a).unwrap().members, vec![c, b, a]);
    assert!(!m.move_tab(a, true), "the last stays last");
    assert_eq!(m.group_of(a).unwrap().active, c, "moving a tab doesn't change which shows");
}

#[test]
fn stacking_a_group_into_another_merges_them() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    let d = add(&mut m, "d");
    m.stack(a, b);
    m.stack(c, d);
    m.stack(a, d);
    assert_eq!(m.group_of(a).unwrap().members, vec![a, b, c, d]);
    assert_eq!(m.group_of(a).unwrap().active, d);
    assert_eq!(m.groups().count(), 1);
}

#[test]
fn a_torn_out_tab_goes_back_before_any_other() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    m.stack(a, b);
    m.stack(a, c);
    // The showing tab, torn out (⌥-drag) and dropped before the first.
    assert!(m.unstack(c));
    let shown = m.group_of(a).unwrap().active;
    m.stack_at(shown, c, Some(0));
    assert_eq!(m.group_of(a).unwrap().members, vec![c, a, b]);
    assert_eq!(m.group_of(a).unwrap().active, c);
    // Between the two others.
    assert!(m.unstack(c));
    m.stack_at(a, c, Some(1));
    assert_eq!(m.group_of(a).unwrap().members, vec![a, c, b]);
    // Past the end is the end.
    assert!(m.unstack(c));
    m.stack_at(a, c, Some(9));
    assert_eq!(m.group_of(a).unwrap().members, vec![a, b, c]);
}

#[test]
fn stacking_before_a_tab_brings_the_whole_group() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    let d = add(&mut m, "d");
    // Before a window on its own.
    m.stack_at(a, b, Some(0));
    assert_eq!(m.group_of(a).unwrap().members, vec![b, a]);
    m.stack(c, d);
    m.stack_at(a, d, Some(1));
    assert_eq!(m.group_of(a).unwrap().members, vec![b, c, d, a]);
    assert_eq!(m.group_of(a).unwrap().active, d);
    assert_eq!(m.groups().count(), 1);
}

#[test]
fn closing_a_shown_member_shows_a_neighbour() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    m.stack(a, b);
    m.focus(Some(b));
    assert_eq!(m.remove(b), Refocus::To(Some(a)));
    assert!(m.is_shown(a));
}

#[test]
fn window_behind_skips_the_group() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    let c = add(&mut m, "c");
    assert_eq!(m.window_behind(c), Some(b));
    m.stack(c, b);
    assert_eq!(m.window_behind(b), Some(a));
}

#[test]
fn desks_show_their_own_windows() {
    let mut m = model();
    let d1 = m.desks()[0].id;
    let d2 = m.desks()[1].id;
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    assert_eq!(m.move_to_desk(b, d2), Refocus::To(Some(a)));
    m.focus(Some(a));
    assert!(!m.is_shown(b));
    assert_eq!(m.switch_desk(d2).map(|(r, _)| r), Some(Refocus::To(Some(b))));
    m.focus(Some(b));
    assert!(m.is_shown(b) && !m.is_shown(a));
    assert_eq!(m.switch_desk(d2), None);
    // Sticky windows show everywhere.
    m.set_sticky(a, true);
    assert!(m.is_shown(a));
    m.switch_desk(d1);
    assert!(m.is_shown(a));
    assert_eq!(m.desk_window_count(d2), 1);
}

#[test]
fn desks_can_be_added_removed_renamed_and_moved() {
    let mut m = model();
    let d1 = m.desks()[0].id;
    let d2 = m.desks()[1].id;
    let mail = m.add_desk("Mail".into(), Some("~/Mail".into()), Some(1));
    assert_eq!(m.desks()[0].name, "Mail");
    assert_eq!(m.desk_index(mail), Some(1));
    assert_eq!(m.desk_index(d1), Some(2));
    assert!(m.rename_desk(d2, "Build".into()));
    assert_eq!(m.desk_named("Build"), Some(d2));
    assert!(m.move_desk(mail, 5));
    assert_eq!(m.desks().last().unwrap().id, mail);
    // Removing a Desk moves its windows to the one before it.
    m.switch_desk(d2);
    let a = add(&mut m, "a");
    assert_eq!(m.remove_desk(d2), Some(Refocus::To(Some(a))));
    assert_eq!(m.get(a).unwrap().desk, d1);
    assert_eq!(m.current_desk(), d1);
    while m.desks().len() > 1 {
        let id = m.desks()[0].id;
        m.remove_desk(id);
    }
    assert_eq!(m.remove_desk(m.desks()[0].id), None, "the last Desk stays");
}

#[test]
fn neighbour_desks_wrap() {
    let mut m = model();
    let ids: Vec<_> = m.desks().iter().map(|d| d.id).collect();
    assert_eq!(m.neighbour_desk(true), ids[1]);
    assert_eq!(m.neighbour_desk(false), ids[3]);
    m.switch_desk(ids[3]);
    assert_eq!(m.neighbour_desk(true), ids[0]);
}

#[test]
fn moving_a_stacked_window_moves_its_group() {
    let mut m = model();
    let d2 = m.desks()[1].id;
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    m.stack(a, b);
    m.move_to_desk(a, d2);
    assert_eq!(m.get(b).unwrap().desk, d2);
}

#[test]
fn switcher_lists_apps_by_recent_use() {
    let mut m = model();
    let a1 = add(&mut m, "a");
    let b1 = add(&mut m, "b");
    let a2 = add(&mut m, "a");
    let c1 = add(&mut m, "c");
    m.minimise(c1);
    m.focus(Some(b1));
    let apps = m.apps_mru();
    let names: Vec<&str> = apps.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(names, ["b", "c", "a"]);
    assert_eq!(apps[2].1, vec![a2, a1]);
}

#[test]
fn floating_and_fullscreen_windows_stay_on_top() {
    let mut m = model();
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    m.get_mut(a).unwrap().float = true;
    assert_eq!(m.shown_stacking(), vec![b, a]);
    m.set_fullscreen(b, Some(true), Rect::new(0, 0, 100, 100));
    m.focus(Some(b));
    assert_eq!(m.shown_stacking(), vec![a, b]);
}

#[test]
fn desks_sync_from_settings() {
    let mut m = model();
    let a = add(&mut m, "a");
    let d4 = m.desks()[3].id;
    m.move_to_desk(a, d4);
    assert!(m.sync_desks(&[("Mail".into(), None), ("Build".into(), Some("~/work".into()))]));
    assert_eq!(m.desks().len(), 2);
    assert_eq!(m.desks()[1].folder.as_deref(), Some("~/work"));
    assert_eq!(m.get(a).unwrap().desk, m.desks()[1].id);
}

#[test]
fn a_parked_desk_sleeps_until_it_is_visited() {
    let mut m = model();
    let d1 = m.desks()[0].id;
    let d2 = m.desks()[1].id;
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    m.move_to_desk(b, d2);
    // The current Desk isn't parked; another is, once.
    assert!(!m.set_parked(d1, Some(100)));
    assert!(m.set_parked(d2, Some(100)));
    assert!(!m.set_parked(d2, Some(200)), "already parked");
    assert_eq!(m.desk(d2).unwrap().parked, Some(100));
    assert!(m.is_parked_window(b) && !m.is_parked_window(a));
    // A sticky window shows everywhere, so isn't parked with it.
    m.set_sticky(b, true);
    assert!(!m.is_parked_window(b));
    m.set_sticky(b, false);
    m.move_to_desk(b, d2);
    assert!(m.is_parked_window(b));
    assert_eq!(m.desk_windows(d2), vec![b]);
    // Going there wakes it, and the Desk left says when.
    m.switch_desk(d2);
    m.mark_left(d1, 300);
    assert_eq!(m.desk(d2).unwrap().parked, None);
    assert!(!m.is_parked_window(b));
    assert_eq!(m.desk(d1).unwrap().left, Some(300));
    // Unparking what isn't parked changes nothing.
    assert!(!m.set_parked(d1, None));
}

#[test]
fn a_desk_counts_the_windows_asking_for_attention() {
    let mut m = model();
    let d2 = m.desks()[1].id;
    let a = add(&mut m, "a");
    let b = add(&mut m, "b");
    m.move_to_desk(a, d2);
    m.move_to_desk(b, d2);
    m.get_mut(a).unwrap().urgent = true;
    assert_eq!(m.desk_urgent_count(d2), 1);
}

// ---- displays with Desks of their own -----------------------------------

const LAPTOP: Rect = Rect::new(0, 0, 1440, 900);
const MONITOR: Rect = Rect::new(1440, 0, 2560, 1440);

/// A display with a 24pt menu bar.
fn geom(output: &str, rect: Rect) -> ScreenGeom {
    let usable = Rect::new(rect.x, rect.y + 24, rect.w, rect.h - 24);
    ScreenGeom { output: output.into(), rect, usable, area: usable }
}

fn usable(rect: Rect) -> Rect {
    geom("", rect).usable
}

fn both(m: &mut Model, primary: &str) -> Vec<DeskMove> {
    m.sync_screens(&[geom("eDP-1", LAPTOP), geom("DP-1", MONITOR)], Some(primary))
}

/// A window at `rect`, laid out on the display it's on.
fn put(m: &mut Model, app: &str, rect: Rect) -> WinId {
    let id = m.insert(app, app, None, rect, None);
    m.focus(Some(id));
    let frame = m.screen_at(rect).unwrap().usable;
    m.set_frame(id, frame);
    id
}

fn ssd() -> Insets {
    crate::decor::insets(true)
}

#[test]
fn the_main_display_shows_the_current_desk_and_the_others_the_next() {
    let mut m = model();
    let (d1, d2) = (m.desks()[0].id, m.desks()[1].id);
    let moves = both(&mut m, "DP-1");
    assert_eq!(m.desk_on("DP-1"), Some(d1));
    assert_eq!(m.desk_on("eDP-1"), Some(d2));
    assert_eq!(m.current_desk(), d1);
    assert_eq!(m.focused_screen().unwrap().output, "DP-1");
    assert_eq!(moves.len(), 2);
    assert!(moves.iter().all(|mv| mv.from.is_none()));
    // Nothing changed, nothing moves.
    assert!(both(&mut m, "DP-1").is_empty());
}

#[test]
fn switching_desks_changes_only_the_display_with_the_focus() {
    let mut m = model();
    let (d1, d2, d3) = (m.desks()[0].id, m.desks()[1].id, m.desks()[2].id);
    both(&mut m, "DP-1");
    assert_eq!(m.focus_screen("eDP-1"), Some(Refocus::To(None)));
    let l = put(&mut m, "laptop", Rect::new(100, 100, 400, 300));
    assert_eq!(m.get(l).unwrap().desk, d2);
    assert_eq!(m.focus_screen("DP-1"), Some(Refocus::To(None)), "the monitor's Desk is empty");
    let a = put(&mut m, "monitor", Rect::new(1600, 100, 800, 600));
    assert_eq!(m.get(a).unwrap().desk, d1);
    assert!(m.is_shown(a) && m.is_shown(l), "both displays show their Desks");

    let (r, moves) = m.switch_desk(d3).unwrap();
    assert_eq!(moves, vec![DeskMove { output: "DP-1".into(), from: Some(d1), to: d3 }]);
    assert_eq!(r, Refocus::To(None), "focus stays on its display, which shows an empty Desk");
    assert!(!m.is_shown(a) && m.is_shown(l));
    assert_eq!(m.desk_on("eDP-1"), Some(d2));
    // Focusing the laptop's window takes the focus there.
    m.focus(Some(l));
    assert_eq!(m.current_desk(), d2);
    assert_eq!(m.focused_screen().unwrap().output, "eDP-1");
}

#[test]
fn a_desk_another_display_shows_swaps_and_scales_there_and_back_exactly() {
    let mut m = model();
    let (d1, d2) = (m.desks()[0].id, m.desks()[1].id);
    both(&mut m, "DP-1");
    m.focus_screen("eDP-1");
    let l = put(&mut m, "laptop", Rect::new(100, 100, 400, 300));
    m.set_fill(l, Some(true), usable(LAPTOP), ssd());
    m.focus_screen("DP-1");
    let r0 = Rect::new(1440 + 1, 24 + 25, 1280, 700);
    let a = put(&mut m, "monitor", r0);
    m.take_relaid();

    // The laptop's Desk comes to the monitor (where the focus is), and the
    // monitor's goes to the laptop.
    let (_, moves) = m.switch_desk(d2).unwrap();
    assert_eq!(moves.len(), 2);
    assert_eq!((m.desk_on("DP-1"), m.desk_on("eDP-1")), (Some(d2), Some(d1)));
    // The monitor's window, flush with the top left, is flush there on
    // the laptop, at the laptop's proportions (1440/2560 wide, 876/1416
    // tall, frames included).
    let w = m.get(a).unwrap().rect;
    let i = ssd();
    assert_eq!((w.x - i.left, w.y - i.top), (0, 24), "{w:?}");
    let fw = |r: Rect| r.w + i.left + i.right;
    let fh = |r: Rect| r.h + i.top + i.bottom;
    assert_eq!(fw(w), (fw(r0) as f64 * 1440.0 / 2560.0).round() as i32);
    assert_eq!(fh(w), (fh(r0) as f64 * 876.0 / 1416.0).round() as i32);
    // The laptop's filled window fills the monitor.
    assert_eq!(m.get(l).unwrap().rect, usable(MONITOR).inset(i));
    let mut relaid = m.take_relaid();
    relaid.sort();
    assert_eq!(relaid, vec![l, a]);

    // And back: both exactly where they were.
    m.switch_desk(d1).unwrap();
    assert_eq!((m.desk_on("DP-1"), m.desk_on("eDP-1")), (Some(d1), Some(d2)));
    assert_eq!(m.get(a).unwrap().rect, r0);
    assert_eq!(m.get(l).unwrap().rect, usable(LAPTOP).inset(i));
    assert!(m.get(a).unwrap().home.is_none());
    // Unfilled on the laptop it goes back to its own place.
    m.set_fill(l, Some(false), usable(LAPTOP), i);
    assert_eq!(m.get(l).unwrap().rect, Rect::new(100, 100, 400, 300));
}

#[test]
fn moving_a_window_on_another_display_forgets_where_it_was() {
    let mut m = model();
    let d2 = m.desks()[1].id;
    both(&mut m, "DP-1");
    let r0 = Rect::new(1700, 200, 900, 600);
    let a = put(&mut m, "a", r0);
    m.switch_desk(d2);
    m.focus_screen("eDP-1");
    let on_laptop = m.get(a).unwrap().rect;
    assert!(LAPTOP.contains(on_laptop.x as f64, on_laptop.y as f64));
    m.set_rect(a, Rect::new(50, 60, 500, 400));
    m.focus_screen("DP-1");
    m.switch_desk(m.desk_on("eDP-1").unwrap());
    let back = m.get(a).unwrap().rect;
    assert_ne!(back, r0, "moved since, it scales from its new place");
    assert_eq!(back, scale_frame(Rect::new(50, 60, 500, 400), usable(LAPTOP), usable(MONITOR), ssd()));
}

#[test]
fn a_display_that_goes_takes_its_desk_along_and_brings_it_back() {
    let mut m = model();
    let (d1, d2) = (m.desks()[0].id, m.desks()[1].id);
    both(&mut m, "DP-1");
    m.focus_screen("eDP-1");
    let r1 = Rect::new(100, 100, 400, 300);
    let l = put(&mut m, "laptop", r1);
    m.focus_screen("DP-1");
    let r0 = Rect::new(2000, 300, 1000, 800);
    let a = put(&mut m, "monitor", r0);

    // Unplugged with the focus on it: its Desk comes to the laptop, and
    // the laptop's hides, its window where it was.
    let moves = m.sync_screens(&[geom("eDP-1", LAPTOP)], Some("eDP-1"));
    assert_eq!(moves, vec![DeskMove { output: "eDP-1".into(), from: Some(d2), to: d1 }]);
    assert_eq!(m.current_desk(), d1);
    assert!(m.is_shown(a) && !m.is_shown(l));
    assert!(LAPTOP.contains(m.get(a).unwrap().rect.x as f64, m.get(a).unwrap().rect.y as f64));
    assert_eq!(m.get(l).unwrap().rect, r1);

    // Plugged back in: every Desk where it was, every window exactly.
    let moves = both(&mut m, "DP-1");
    assert_eq!(moves.len(), 2, "{moves:?}");
    assert_eq!((m.desk_on("DP-1"), m.desk_on("eDP-1")), (Some(d1), Some(d2)));
    assert_eq!(m.get(a).unwrap().rect, r0);
    assert_eq!(m.get(l).unwrap().rect, r1);
    assert!(m.is_shown(a) && m.is_shown(l));
}

#[test]
fn a_display_without_the_focus_that_goes_hides_its_desk() {
    let mut m = model();
    let (d1, d2) = (m.desks()[0].id, m.desks()[1].id);
    both(&mut m, "DP-1");
    m.focus_screen("eDP-1");
    let l = put(&mut m, "laptop", Rect::new(100, 100, 400, 300));
    m.focus_screen("DP-1");
    m.sync_screens(&[geom("DP-1", MONITOR)], Some("DP-1"));
    assert_eq!(m.desk_on("DP-1"), Some(d1));
    assert!(!m.is_shown(l));
    assert_eq!(m.get(l).unwrap().rect, Rect::new(100, 100, 400, 300));
    // Going to it brings it to the monitor, scaled.
    m.switch_desk(d2);
    assert!(m.is_shown(l));
    assert!(MONITOR.contains(m.get(l).unwrap().rect.x as f64, m.get(l).unwrap().rect.y as f64));
    // The laptop back: the monitor keeps what it was asked to show, and the
    // laptop takes a hidden Desk.
    both(&mut m, "DP-1");
    assert_eq!((m.desk_on("DP-1"), m.desk_on("eDP-1")), (Some(d2), Some(d1)));
    // Swapped back, the window is on the laptop exactly where it was.
    m.switch_desk(d1);
    assert_eq!(m.desk_on("eDP-1"), Some(d2));
    assert_eq!(m.get(l).unwrap().rect, Rect::new(100, 100, 400, 300), "back on the laptop: {:?}", m.screens());
}

#[test]
fn stepping_through_desks_passes_those_other_displays_show() {
    let mut m = model();
    let ids: Vec<DeskId> = m.desks().iter().map(|d| d.id).collect();
    both(&mut m, "DP-1");
    // The monitor shows 1 and has the focus; the laptop shows 2.
    assert_eq!(m.neighbour_desk(true), ids[2]);
    assert_eq!(m.neighbour_desk(false), ids[3]);
    m.focus_screen("eDP-1");
    assert_eq!(m.neighbour_desk(true), ids[2]);
    assert_eq!(m.neighbour_desk(false), ids[3]);
}

#[test]
fn a_window_dragged_to_another_display_joins_its_desk() {
    let mut m = model();
    let (d1, d2) = (m.desks()[0].id, m.desks()[1].id);
    both(&mut m, "DP-1");
    let a = put(&mut m, "a", Rect::new(1600, 100, 400, 300));
    let d = m.insert("a", "Save?", None, Rect::new(1700, 150, 200, 100), Some(a));
    assert_eq!(m.get(a).unwrap().desk, d1);
    assert!(!m.adopt_into_screen(a), "still on its own display");
    m.set_rect(a, Rect::new(200, 100, 400, 300));
    assert!(m.adopt_into_screen(a));
    assert_eq!(m.get(a).unwrap().desk, d2);
    assert_eq!(m.get(d).unwrap().desk, d2, "its dialog goes along");
    assert_eq!(m.get(a).unwrap().frame, Some(usable(LAPTOP)));
    assert_eq!(m.current_desk(), d2, "the focus goes with it");
}

#[test]
fn moving_a_window_to_a_desk_another_display_shows_puts_it_there() {
    let mut m = model();
    let d2 = m.desks()[1].id;
    both(&mut m, "DP-1");
    let a = put(&mut m, "a", Rect::new(1600, 100, 800, 600));
    assert_eq!(m.move_to_desk(a, d2), Refocus::To(None));
    assert!(m.is_shown(a));
    let r = m.get(a).unwrap().rect;
    assert!(LAPTOP.contains(r.x as f64, r.y as f64), "{r:?}");
    assert_eq!(m.take_relaid(), vec![a]);
}

#[test]
fn sticky_windows_show_on_their_own_display() {
    let mut m = model();
    let (d1, d2, d3) = (m.desks()[0].id, m.desks()[1].id, m.desks()[2].id);
    both(&mut m, "DP-1");
    let s = put(&mut m, "s", Rect::new(100, 100, 400, 300));
    m.set_sticky(s, true);
    m.focus_screen("eDP-1");
    m.switch_desk(d3);
    assert!(m.is_shown(s));
    assert_eq!(m.desk_shown_with(s), Some(d3));
    m.focus_screen("DP-1");
    assert_eq!(m.current_desk(), d1);
    assert_eq!(m.next_focus(None), None, "the laptop's sticky window isn't the monitor's to focus");
    m.focus(Some(s));
    assert_eq!(m.current_desk(), d3);
    // Unstuck, it stays on the laptop's Desk.
    m.set_sticky(s, false);
    assert_eq!(m.get(s).unwrap().desk, d3);
    assert_ne!(d2, d3);
}

#[test]
fn removing_a_desk_a_display_shows_gives_it_another() {
    let mut m = Model::new((1..=3).map(|n| (n.to_string(), None)));
    let ids: Vec<DeskId> = m.desks().iter().map(|d| d.id).collect();
    both(&mut m, "DP-1");
    // DP-1 shows 1 and eDP-1 2; removing 2 sends its windows to 1, which
    // the monitor shows, so the laptop gets 3.
    m.focus_screen("eDP-1");
    let l = put(&mut m, "laptop", Rect::new(100, 100, 400, 300));
    m.remove_desk(ids[1]).unwrap();
    assert_eq!(m.desk_on("eDP-1"), Some(ids[2]));
    assert_eq!(m.get(l).unwrap().desk, ids[0]);
    assert!(MONITOR.contains(m.get(l).unwrap().rect.x as f64, 200.0));
    // Two displays keep two Desks.
    assert_eq!(m.remove_desk(ids[2]), None);
    assert!(!m.sync_desks(&[("1".into(), None)]) || m.desks().len() == 2);
    assert_eq!(m.desks().len(), 2);
}

#[test]
fn more_displays_than_desks_get_a_desk_each() {
    let mut m = Model::new([("1".to_string(), None)]);
    both(&mut m, "eDP-1");
    assert_eq!(m.desks().len(), 2);
    assert_ne!(m.desk_on("eDP-1"), m.desk_on("DP-1"));
}

#[test]
fn a_model_from_before_brings_the_current_desks_windows_onto_the_main_display() {
    let mut m = model();
    // Desks used to span the displays: the current Desk had windows on both.
    let a = m.insert("a", "a", None, Rect::new(100, 100, 400, 300), None);
    let b = m.insert("b", "b", None, Rect::new(1600, 100, 400, 300), None);
    both(&mut m, "DP-1");
    assert!(m.is_shown(a) && m.is_shown(b));
    let r = m.get(a).unwrap().rect;
    assert!(MONITOR.contains(r.x as f64, r.y as f64), "from the laptop to the main display: {r:?}");
    assert_eq!(m.get(b).unwrap().rect, Rect::new(1600, 100, 400, 300));
}

#[test]
fn a_full_screen_window_on_another_display_stays_in_front_there() {
    let mut m = model();
    both(&mut m, "DP-1");
    let v = put(&mut m, "video", Rect::new(1600, 100, 400, 300));
    m.set_fullscreen(v, Some(true), MONITOR);
    let o = put(&mut m, "other", Rect::new(1700, 200, 400, 300));
    m.focus_screen("eDP-1");
    put(&mut m, "laptop", Rect::new(100, 100, 400, 300));
    // The monitor's last focus was "other", over the video.
    let s = m.shown_stacking();
    assert!(s.iter().position(|w| *w == o) > s.iter().position(|w| *w == v), "{s:?}");
    m.focus(Some(v));
    m.focus_screen("eDP-1");
    let s = m.shown_stacking();
    assert_eq!(s.last(), Some(&v), "focused last on its display, it covers it: {s:?}");
}

#[test]
fn scaling_keeps_edges_and_the_same_area_gives_the_same_rect() {
    let i = ssd();
    let (from, to) = (usable(LAPTOP), usable(MONITOR));
    for r in [Rect::new(1, 49, 1203, 850), Rect::new(333, 77, 640, 480), Rect::new(-50, 500, 300, 200)] {
        assert_eq!(scale_frame(r, from, from, i), r);
        assert_eq!(scale_frame(scale_frame(r, from, to, i), to, to, i), scale_frame(r, from, to, i));
    }
    // Flush with the right and bottom edges, it stays flush.
    let r = Rect::new(800 + i.left, 400 + i.top, 640 - i.left - i.right, 500 - i.top - i.bottom);
    let s = scale_frame(r, from, to, i).outset(i);
    assert_eq!((s.right(), s.bottom()), (to.right(), to.bottom()));
}

#[test]
fn a_bar_coming_to_a_display_fits_windows_rather_than_scaling_them() {
    let mut m = model();
    let bare = |output: &str, rect: Rect| ScreenGeom { output: output.into(), rect, usable: rect, area: rect };
    m.sync_screens(&[bare("eDP-1", LAPTOP)], Some("eDP-1"));
    let r = Rect::new(440, 262, 400, 300);
    let a = put(&mut m, "a", r);
    m.set_fill(a, Some(true), LAPTOP, ssd());
    m.take_relaid();
    // The shell's bar takes 24pt: the same display, its windows unscaled
    // (the logic fits them to it), the free place to go back to as it was.
    m.sync_screens(&[geom("eDP-1", LAPTOP)], Some("eDP-1"));
    assert!(m.take_relaid().is_empty());
    assert_eq!(m.get(a).unwrap().restore, Some(r));
    assert_eq!(m.get(a).unwrap().frame, Some(usable(LAPTOP)));
}
