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
    assert!(!m.reveal(b2));
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
    assert_eq!(m.switch_desk(d2), Some(Refocus::To(Some(b))));
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
