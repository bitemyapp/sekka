//! Soroban headlessly: the keypads by pointer and keyboard, the menus'
//! ❖C and ❖V, the modes' window sizes, the bit toggles, the tape, golden
//! images of each mode in Night and Day, and what a key press costs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_kit::Size;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_soroban::app::{Msg, Soroban, window_size};
use aizome_soroban::bits;
use aizome_soroban::entry::{Key, Mode, Num};

fn harness() -> Harness<Soroban> {
    Harness::new(Soroban::new(None))
}

fn harness_in(theme: ThemeChoice) -> Harness<Soroban> {
    Harness::with(Soroban::new(None), Appearance { theme, ..Appearance::default() }, false)
}

fn click_key(h: &mut Harness<Soroban>, key: Key) {
    let w = h.window();
    let r = h.find_id(w, &format!("key-{key:?}")).unwrap_or_else(|| panic!("{key:?} on the keypad"));
    h.click(w, r.center());
}

fn display(h: &Harness<Soroban>) -> (String, String) {
    let s = h.program().calc.shown();
    (s.main, s.sub)
}

#[test]
fn the_keypad_answers_the_pointer() {
    let mut h = harness();
    for k in [Key::Digit(1), Key::Digit(2), Key::Mul, Key::Digit(3)] {
        click_key(&mut h, k);
    }
    assert_eq!(display(&h), ("12 × 3".into(), "= 36".into()));
    click_key(&mut h, Key::Equals);
    assert_eq!(display(&h), ("36".into(), "12 × 3 =".into()));
    assert_eq!(h.program().calc.tape.len(), 1);
    click_key(&mut h, Key::Negate);
    assert_eq!(display(&h).0, "−36");
    click_key(&mut h, Key::Clear);
    assert_eq!(display(&h).0, "0");
}

#[test]
fn typing_is_enough() {
    let mut h = harness();
    let w = h.window();
    h.type_text(w, "2^10/4");
    assert_eq!(display(&h), ("2^10 ÷ 4".into(), "= 256".into()));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(display(&h).0, "256");
    h.type_text(w, "+1\n");
    assert_eq!(display(&h).0, "257");
    h.named(w, NamedKey::Backspace, Modifiers::NONE);
    assert_eq!(display(&h).0, "25");
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(display(&h).0, "0");
    // A typed key shows its key pressed a moment.
    h.type_text(w, "7");
    let r = h.find_id(w, "key-Digit(7)").expect("7");
    h.take_damage(w);
    h.advance(Duration::from_millis(200));
    let damage = h.take_damage(w);
    assert!(damage.iter().any(|d| d.intersect(r).area() > 0.0), "the flash ends: {damage:?}");
    // Tab walks to the keys (past the toolbar); Space presses the one
    // focused, and typing still types.
    for _ in 0..4 {
        if h.focused_name(w).is_some_and(|n| n.starts_with("key-")) {
            break;
        }
        h.named(w, NamedKey::Tab, Modifiers::NONE);
    }
    assert_eq!(h.focused_name(w).as_deref(), Some("key-Clear"));
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("key-Back"));
    h.type_text(w, "42 ");
    assert_eq!(display(&h).0, "74");
    // Clicking another key leaves the focus be; Return is still =.
    click_key(&mut h, Key::Digit(9));
    assert_eq!(display(&h).0, "749");
    h.type_text(w, "+1\n");
    assert_eq!(display(&h).0, "750");
    assert_eq!(h.focused_name(w).as_deref(), Some("key-Back"));
}

#[test]
fn copy_and_paste_go_through_the_menus() {
    let mut h = harness();
    let w = h.window();
    h.type_text(w, "1/8\n");
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("0.125"));
    h.platform.clipboard = Some("1\u{2009}000 × 3".into());
    h.command(w, 'v');
    assert_eq!(display(&h), ("1000 × 3".into(), "= 3\u{2009}000".into()));
    h.command(w, 'z');
    assert_eq!(display(&h).0, "0.125");
    // Big answers copy without their thin spaces.
    h.type_text(w, "1234567*1000\n");
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some("1234567000"));
}

#[test]
fn each_mode_has_its_own_fixed_size() {
    let mut h = harness();
    let w = h.window();
    assert_eq!(h.size(w), window_size(Mode::Basic, true));
    h.command(w, '2');
    assert_eq!(h.program().calc.mode, Mode::Scientific);
    let win = *h.platform.requested_sizes.keys().next().expect("a size asked for");
    assert_eq!(h.platform.requested_sizes[&win], window_size(Mode::Scientific, true));
    h.command(w, 't');
    assert!(!h.program().show_tape);
    assert_eq!(h.platform.requested_sizes[&win], window_size(Mode::Scientific, false));
    h.command(w, '3');
    assert_eq!(h.platform.requested_sizes[&win], window_size(Mode::Programmer, false));
    for m in Mode::ALL {
        for tape in [false, true] {
            let s = window_size(m, tape);
            assert!(s.width <= 820.0 && s.height <= 720.0, "{m:?} {tape}: {s:?}");
        }
    }
}

#[test]
fn scientific_keys_work() {
    let mut h = harness();
    let w = h.window();
    h.send(Msg::SetMode(Mode::Scientific));
    h.resize(w, window_size(Mode::Scientific, true));
    for k in [Key::Sin, Key::Digit(3), Key::Digit(0), Key::Close, Key::Add, Key::Pi, Key::Equals] {
        click_key(&mut h, k);
    }
    let ans = h.program().calc.ans;
    assert!((ans - (0.5 + std::f64::consts::PI)).abs() < 1e-12, "{ans}");
    click_key(&mut h, Key::Angle);
    click_key(&mut h, Key::Second);
    for k in [Key::Clear, Key::Cos, Key::Digit(0), Key::Equals] {
        click_key(&mut h, k);
    }
    assert_eq!(h.program().calc.ans, std::f64::consts::FRAC_PI_2);
    click_key(&mut h, Key::MemAdd);
    assert_eq!(h.program().calc.memory, Some(std::f64::consts::FRAC_PI_2));
}

#[test]
fn programmer_bits_flip_and_radixes_switch() {
    let mut h = harness();
    let w = h.window();
    h.send(Msg::SetMode(Mode::Programmer));
    h.resize(w, window_size(Mode::Programmer, true));
    h.type_text(w, "5\n");
    // Bit 4's cell, inside the grid.
    let grid = h.find_id(w, "bits").expect("the bit toggles");
    let c = bits::cell(4);
    h.click(w, grid.origin() + c.center().to_vec2());
    assert_eq!(h.program().calc.int_ans, 21);
    h.click_text(w, "HEX");
    assert_eq!(h.program().calc.radix, 16);
    h.type_text(w, "ff and f0\n");
    assert_eq!(display(&h).0, "F0");
    h.send(Msg::Word(8));
    h.send(Msg::Signed(true));
    h.click_text(w, "DEC");
    assert_eq!(display(&h).0, "−16");
    // Bits past the word don't flip.
    h.click(w, grid.origin() + bits::cell(40).center().to_vec2());
    assert_eq!(h.program().calc.int_ans, 0xF0);
}

#[test]
fn the_tape_recalls_copies_and_saves() {
    let mut h = harness();
    let w = h.window();
    h.type_text(w, "6*7\n100-1\n");
    assert_eq!(h.program().calc.tape.len(), 2);
    let list = h.find_id(w, "tape-list").expect("the tape");
    // The first entry, at the top of the tape.
    h.click(w, list.origin() + aizome_kit::Vec2::new(list.width() / 2.0, 20.0));
    h.type_text(w, "+1\n");
    assert_eq!(display(&h).0, "43");
    h.send(Msg::CopyTape);
    assert_eq!(h.platform.clipboard.as_deref(), Some("6 × 7 = 42\n100 − 1 = 99\n42 + 1 = 43\n"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("soroban-tape.txt");
    h.send(Msg::SaveTapeTo(Some(out.clone())));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "6 × 7 = 42\n100 − 1 = 99\n42 + 1 = 43\n");
    h.command(w, 'k');
    assert!(h.program().calc.tape.is_empty());
}

#[test]
fn state_survives_a_restart() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("soroban-state");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("state.json");
    {
        let mut h = Harness::new(Soroban::new(Some(path.clone())));
        let w = h.window();
        h.type_text(w, "3*3\n");
        h.send(Msg::SetMode(Mode::Programmer));
        h.send(Msg::Radix(16));
        h.advance(Duration::from_secs(1));
    }
    let s = Soroban::new(Some(path));
    assert_eq!(s.calc.mode, Mode::Programmer);
    assert_eq!(s.calc.radix, 16);
    assert_eq!(s.calc.tape.len(), 1);
    assert_eq!(s.calc.tape[0].value, Num::Real(9.0));
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

/// Look at a mode: a golden at 1× (compared), and a 2× picture to read.
fn pictures(h: &mut Harness<Soroban>, name: &str) {
    let w = h.window();
    // Typed keys' flashes over.
    h.advance(Duration::from_millis(300));
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}@2x.png")), 2.0);
    if !h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)");
        return;
    }
    assert_golden(h, w, golden(name), 1.0);
}

#[test]
fn goldens() {
    for (theme, tname) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut h = harness_in(theme);
        let w = h.window();
        h.type_text(w, "1234*5\n4/7\n");
        h.type_text(w, "12.5+(3");
        pictures(&mut h, &format!("basic-{tname}"));

        h.send(Msg::SetMode(Mode::Scientific));
        h.resize(w, window_size(Mode::Scientific, true));
        h.send(Msg::Key(Key::Second));
        h.send(Msg::Key(Key::MemAdd));
        pictures(&mut h, &format!("scientific-{tname}"));

        h.send(Msg::SetMode(Mode::Programmer));
        h.resize(w, window_size(Mode::Programmer, true));
        h.send(Msg::Radix(16));
        h.send(Msg::Word(32));
        h.type_text(w, "dead and ff00\n");
        pictures(&mut h, &format!("programmer-{tname}"));

        h.send(Msg::ToggleTape);
        h.resize(w, window_size(Mode::Programmer, false));
        h.send(Msg::SetMode(Mode::Basic));
        h.resize(w, window_size(Mode::Basic, false));
        pictures(&mut h, &format!("basic-no-tape-{tname}"));
    }
}

/// A key press, from the key to its pixels: the program's update, the
/// view, layout and diff, and rasterising what changed. Prints the
/// percentiles (for docs/performance.md) and holds them to the 4 ms
/// budget in an optimised build.
#[test]
fn a_key_press_fits_the_frame_budget() {
    for mode in [Mode::Basic, Mode::Scientific, Mode::Programmer] {
        let mut h = harness();
        let w = h.window();
        h.send(Msg::SetMode(mode));
        h.resize(w, window_size(mode, true));
        h.type_text(w, "1+2\n");
        let _ = h.render(w, 1.0);
        let mut frames = Vec::new();
        let text = "12345*678-9/3\n";
        for round in 0..20 {
            for c in text.chars() {
                h.take_damage(w);
                let start = Instant::now();
                match c {
                    '\n' => h.named(w, NamedKey::Enter, Modifiers::NONE),
                    c => h.type_text(w, &c.to_string()),
                }
                let damage = h.take_damage(w);
                let mut bounds: Option<aizome_kit::Rect> = None;
                for d in damage {
                    bounds = Some(bounds.map_or(d, |b| b.union(d)));
                }
                if let Some(b) = bounds {
                    let _ = h.render_rect(w, b, 2.0);
                }
                if round > 1 {
                    frames.push(start.elapsed());
                }
            }
            h.advance(Duration::from_millis(200));
        }
        frames.sort();
        let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
        eprintln!(
            "soroban {mode:?}: key press p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
            p(0.5),
            p(0.95),
            frames.last().unwrap(),
            frames.len()
        );
        if !cfg!(debug_assertions) {
            assert!(p(0.95) < Duration::from_millis(4), "{mode:?} p95 {:?}", p(0.95));
        }
    }
}

#[test]
fn window_sizes_hold_what_they_show() {
    // The panel's parts add up to the window: nothing is cut off.
    for m in Mode::ALL {
        let s = window_size(m, false);
        let pads = aizome_soroban::keypad::size(m);
        assert!(s.width >= pads.width + 24.0);
        assert_eq!(s, Size::new(s.width.round(), s.height.round()));
    }
}

#[test]
fn the_save_sheet_names_the_file_and_asks_before_replacing() {
    let mut h = harness();
    let w = h.window();
    h.type_text(w, "2+2\n");
    h.command(w, 's');
    assert!(h.program().save.is_some(), "❖S opens the sheet");
    assert_eq!(h.focused_name(w).as_deref(), Some("save-name"));
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("soroban-save");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    h.program_mut().save.as_mut().unwrap().folders = vec![("Here".into(), dir.clone())];
    // Typing goes to the name, not the calculator.
    h.send(Msg::SaveName(String::new()));
    h.type_text(w, "sums.txt");
    assert_eq!(h.program().save.as_ref().map(|s| s.name.as_str()), Some("sums.txt"));
    assert_eq!(display(&h).0, "4");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().save.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("sums.txt")).unwrap(), "2 + 2 = 4\n");
    // Again: the file is there, so the first Save asks.
    h.command(w, 's');
    h.program_mut().save.as_mut().unwrap().folders = vec![("Here".into(), dir.clone())];
    h.send(Msg::SaveName("sums.txt".into()));
    h.type_text(w, "\n");
    assert!(h.program().save.as_ref().is_some_and(|s| s.replacing));
    assert!(h.texts(w).iter().any(|t| t == "Replace"));
    h.type_text(w, "\n");
    assert!(h.program().save.is_none());
    // Escape cancels.
    h.command(w, 's');
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().save.is_none());
    assert_eq!(display(&h).0, "4", "Escape closed the sheet, not cleared the calculator");
}
