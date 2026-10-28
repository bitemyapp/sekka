use hotaru_core::{Rgb, Terminal, TerminalColors};

fn configured() -> TerminalColors {
    let mut colors = TerminalColors {
        foreground: Rgb(30, 31, 32),
        background: Rgb(40, 41, 42),
        ..TerminalColors::default()
    };
    colors.palette[1] = Rgb(10, 11, 12);
    colors.palette[2] = Rgb(20, 21, 22);
    colors
}

#[test]
fn color_queries_report_the_configured_render_colors() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.set_default_colors(configured());
    terminal.feed(b"\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07");
    assert_eq!(
        terminal.take_responses(),
        b"\x1b]4;1;rgb:0a0a/0b0b/0c0c\x07\x1b]10;rgb:1e1e/1f1f/2020\x07\x1b]11;rgb:2828/2929/2a2a\x07"
    );
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.palette[1], Rgb(10, 11, 12));
    assert_eq!(snapshot.default_foreground, Rgb(30, 31, 32));
    assert_eq!(snapshot.default_background, Rgb(40, 41, 42));
}

#[test]
fn equal_rgb_overrides_survive_theme_changes_and_resets_reveal_new_defaults() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.set_default_colors(configured());
    terminal.feed(b"\x1b]4;1;#0a0b0c\x07\x1b]10;#1e1f20\x07\x1b]11;#040506\x07");
    let before = terminal.snapshot();
    let mut next = TerminalColors {
        foreground: Rgb(120, 121, 122),
        background: Rgb(130, 131, 132),
        ..configured()
    };
    next.palette[1] = Rgb(100, 101, 102);
    next.palette[2] = Rgb(110, 111, 112);
    terminal.set_default_colors(next.clone());
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.palette[1], Rgb(10, 11, 12));
    assert_eq!(snapshot.palette[2], Rgb(110, 111, 112));
    assert_eq!(snapshot.default_foreground, Rgb(30, 31, 32));
    assert_eq!(snapshot.default_background, Rgb(4, 5, 6));
    assert_eq!(before.palette[2], Rgb(20, 21, 22));

    terminal.feed(b"\x1b]104;1\x07\x1b]110\x07\x1b]111\x07");
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.palette, next.palette);
    assert_eq!(snapshot.default_foreground, next.foreground);
    assert_eq!(snapshot.default_background, next.background);

    terminal.set_default_colors(configured());
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.palette, configured().palette);
    assert_eq!(snapshot.default_foreground, configured().foreground);
    assert_eq!(snapshot.default_background, configured().background);
}

#[test]
fn ris_restores_configured_palette_and_preserves_dynamic_colors() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.set_default_colors(configured());
    terminal.feed(b"\x1b]4;1;#ff0000\x07\x1b]10;#010203\x07\x1b]11;#040506\x07\x1bc");
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.palette, configured().palette);
    assert_eq!(snapshot.default_foreground, Rgb(1, 2, 3));
    assert_eq!(snapshot.default_background, Rgb(4, 5, 6));
    terminal.feed(b"\x1b]110\x07\x1b]111\x07");
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.default_foreground, configured().foreground);
    assert_eq!(snapshot.default_background, configured().background);
}

#[test]
fn reset_all_palette_entries_clears_each_override() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.set_default_colors(configured());
    terminal.feed(b"\x1b]4;1;#ff0000;2;#ff0000\x07\x1b]104\x07");
    terminal.set_default_colors(TerminalColors::default());
    assert_eq!(
        terminal.snapshot().palette,
        TerminalColors::default().palette
    );
}
