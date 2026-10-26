//! What typical interactions cost, from the event to the frame drawn:
//! handling it (the program's update, view, layout and the diff), then
//! recording and rasterizing the damage at scale 2, in a 1440 × 900
//! point window (2880 × 1800 pixels). Each rectangle of damage is drawn
//! offscreen by itself; pixels a scroll moves are the render thread's to
//! move (not counted here: see `SIDESTEP_FRAME_STATS` on a live window).
//!
//! `cargo test --release -p aizome-kit --test perf -- --ignored --nocapture`

use std::rc::Rc;
use std::time::{Duration, Instant};

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::{EditorAction, EditorContent};

const SCALE: f64 = 2.0;
const RUNS: usize = 300;

struct Bench {
    toggles: Vec<bool>,
    field: String,
    content: EditorContent,
    rows: Rc<Vec<String>>,
    selected: Vec<usize>,
    mode: Mode,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Settings,
    Editor,
    List,
}

#[derive(Clone, Debug)]
enum Msg {
    Toggle(usize, bool),
    Field(String),
    Edit(EditorAction),
    Select(Vec<usize>),
    Nothing,
}

impl Program for Bench {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Bench", "Bench", "Benchmark")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Bench").size(1440.0, 900.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Toggle(i, b) => self.toggles[i] = b,
            Msg::Field(s) => self.field = s,
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Select(s) => self.selected = s,
            Msg::Nothing => {}
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        match self.mode {
            Mode::Settings => self.settings(),
            Mode::Editor => column![
                aizome_kit::toolbar(vec![
                    dropdown(["Markdown"], 0).on_select(|_| Msg::Nothing).compact().into(),
                    spacer().into(),
                    aizome_kit::status_text("Ln 48, Col 12")
                ]),
                text_editor(&self.content).on_action(Msg::Edit).line_numbers(true).id("editor")
            ]
            .into(),
            Mode::List => {
                let rows = self.rows.clone();
                virtual_list(rows.len(), 24.0, move |i| {
                    aizome_kit::row![
                        icon(Icon::File).size(14.0),
                        text(rows[i].clone()),
                        spacer(),
                        text("4.2 KB").mono().role(Role::Text3)
                    ]
                    .spacing(8.0)
                    .padding([0.0, 10.0])
                })
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .id("list")
                .into()
            }
        }
    }
}

impl Bench {
    fn new(mode: Mode) -> Bench {
        let doc: String = (0..2000)
            .map(|i| format!("Line {i}: operations stay in points; each window renders at its output's scale.\n"))
            .collect();
        Bench {
            toggles: vec![false; 24],
            field: String::new(),
            content: EditorContent::new(&doc),
            rows: Rc::new((0..10_000).map(|i| format!("file-{i:05}.txt")).collect()),
            selected: Vec::new(),
            mode,
        }
    }

    fn settings(&self) -> Element<Msg> {
        let mut side = vec![aizome_kit::caption("System")];
        for (i, n) in [
            "Network",
            "Bluetooth",
            "Sound",
            "Displays",
            "Appearance",
            "Keyboard",
            "Mouse",
            "Printers",
            "Users",
            "Power",
        ]
        .iter()
        .enumerate()
        {
            side.push(aizome_kit::sidebar_item(Icon::Settings, *n, i == 0).on_press(Msg::Nothing).into());
        }
        let mut sections = Vec::new();
        for s in 0..4 {
            let mut rows = Vec::new();
            for r in 0..6 {
                let i = s * 6 + r;
                rows.push(
                    aizome_kit::form_row(
                        format!("Setting {i}"),
                        toggle(self.toggles[i]).on_toggle(move |b| Msg::Toggle(i, b)).id(format!("t{i}")),
                    )
                    .description("What it does, in a few words.")
                    .into(),
                );
            }
            sections.push(aizome_kit::form_section(format!("Section {s}"), rows));
        }
        let content = scroll(
            aizome_kit::widgets::layout::column(
                std::iter::once(text_field(&self.field).placeholder("Name").on_change(Msg::Field).id("field").into())
                    .chain(std::iter::once(
                        aizome_kit::row![
                            button("Apply").primary().on_press(Msg::Nothing).id("b1"),
                            button("Revert").on_press(Msg::Nothing).id("b2")
                        ]
                        .spacing(8.0)
                        .into(),
                    ))
                    .chain(sections)
                    .collect(),
            )
            .spacing(16.0)
            .padding(20.0),
        );
        aizome_kit::sidebar_layout(aizome_kit::widgets::layout::column(side), content).into()
    }
}

/// Run `act` `RUNS` times, each followed by drawing its damage: the
/// times, in microseconds, sorted.
fn measure(h: &mut Harness<Bench>, w: WindowKey, mut act: impl FnMut(&mut Harness<Bench>, usize)) -> Vec<f64> {
    use aizome_kit::sidestep_ui::offscreen;
    let mut times = Vec::with_capacity(RUNS);
    let (mut handling, mut drawing, mut area) = (0.0, 0.0, 0.0);
    h.take_damage(w);
    h.platform.scrolls.clear();
    // Sidestep's own measure of the renders, as of a window's frames.
    offscreen::set_frame_stats(Some(Duration::from_millis(100)));
    let window = Rect::new(0.0, 0.0, 1440.0, 900.0);
    for i in 0..RUNS {
        let started = Instant::now();
        act(h, i);
        let handled = started.elapsed().as_secs_f64() * 1e6;
        // The damage as a window keeps it: each rectangle not inside another.
        let mut damage: Vec<Rect> = Vec::new();
        for r in h.take_damage(w).into_iter().map(|r| r.intersect(window)).filter(|r| r.area() > 0.0) {
            if !damage.iter().any(|d| d.contains_rect(r)) {
                damage.retain(|d| !r.contains_rect(*d));
                damage.push(r);
            }
        }
        h.platform.scrolls.clear();
        for r in damage {
            area += r.area();
            let _ = h.render_rect(w, r, SCALE);
        }
        let total = started.elapsed().as_secs_f64() * 1e6;
        handling += handled;
        drawing += total - handled;
        times.push(total);
    }
    let renders = offscreen::frame_stats()
        .map(|s| {
            format!(
                "; Sidestep: record {}/{} µs, rasterize {}/{} µs (p50/p95)",
                s.record.p50.as_micros(),
                s.record.p95.as_micros(),
                s.rasterize.p50.as_micros(),
                s.rasterize.p95.as_micros()
            )
        })
        .unwrap_or_default();
    offscreen::set_frame_stats(None);
    eprintln!(
        "    handling {:.0} µs, drawing {:.0} µs, damage {:.0} pt² a frame (means){renders}",
        handling / RUNS as f64,
        drawing / RUNS as f64,
        area / RUNS as f64
    );
    times.sort_by(f64::total_cmp);
    times
}

fn report(name: &str, t: &[f64]) {
    let p = |q: f64| t[((t.len() - 1) as f64 * q).round() as usize] / 1000.0;
    println!("| {name} | {:.2} | {:.2} | {:.2} |", p(0.5), p(0.95), t[t.len() - 1] / 1000.0);
}

#[test]
#[ignore = "a measurement for docs/kit.md, run in a release build"]
fn interactions_stay_within_four_milliseconds() {
    aizome_kit::fonts::install();
    println!("| Interaction | p50 ms | p95 ms | max ms |");
    println!("|---|---|---|---|");

    let mut h = Harness::new(Bench::new(Mode::Settings));
    let w = h.window();
    h.focus_window(w, true);
    let (b1, b2) = (h.find_id(w, "b1").unwrap().center(), h.find_id(w, "b2").unwrap().center());
    let hover = measure(&mut h, w, |h, i| h.move_to(w, if i % 2 == 0 { b1 } else { b2 }));
    report("Hover a button (settings pane, ~200 widgets)", &hover);

    let t0 = h.find_id(w, "t0").unwrap().center();
    let toggle = measure(&mut h, w, |h, _| h.click(w, t0));
    report("Toggle a switch (update, view, layout, diff)", &toggle);

    let field = h.find_id(w, "field").unwrap().center();
    h.click(w, field);
    let typing = measure(&mut h, w, |h, i| {
        if i % 40 == 39 {
            h.command(w, 'a');
            h.named(w, NamedKey::Backspace, Modifiers::NONE);
        } else {
            h.type_text(w, "a");
        }
    });
    report("Type a character in a text field", &typing);

    let mut h = Harness::new(Bench::new(Mode::Editor));
    let w = h.window();
    h.focus_window(w, true);
    let e = h.find_id(w, "editor").unwrap();
    h.click(w, Point::new(e.x0 + 200.0, e.y0 + 200.0));
    let editing = measure(&mut h, w, |h, _| h.type_text(w, "x"));
    report("Type a character in a 2,000-line editor", &editing);

    let mut h = Harness::new(Bench::new(Mode::List));
    let w = h.window();
    let scrolling = measure(&mut h, w, |h, i| {
        let d = if (i / 50) % 2 == 0 { 40.0 } else { -40.0 };
        h.scroll(w, Point::new(700.0, 450.0), Vec2::new(0.0, d));
    });
    report("Scroll a 10,000-row list one notch (rows moved, the strip drawn)", &scrolling);

    h.focus_window(w, true);
    h.click(w, Point::new(300.0, 12.0));
    let keys = measure(&mut h, w, |h, i| {
        h.named(w, if (i / 20) % 2 == 0 { NamedKey::ArrowDown } else { NamedKey::ArrowUp }, Modifiers::NONE);
    });
    report("Move a 10,000-row list's selection by a row", &keys);

    // What the offscreen renderer adds that a window's render thread
    // doesn't: a fresh, cleared buffer for the whole damage and a copy of
    // it out.
    let mut overhead = Vec::new();
    for _ in 0..50 {
        let s = Instant::now();
        let img = aizome_kit::sidestep_ui::offscreen::render(
            Size::new(1440.0, 900.0),
            SCALE,
            aizome_kit::sidestep_ui::Appearance::DARK,
            |c| {
                c.fill_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK);
            },
        );
        drop(img);
        overhead.push(s.elapsed().as_secs_f64() * 1e6);
    }
    overhead.sort_by(f64::total_cmp);
    report("(Offscreen buffer for a whole 2880 × 1800 frame: allocation and copy only)", &overhead);
    for (name, t) in [
        ("hover", &hover),
        ("toggle", &toggle),
        ("typing", &typing),
        ("editing", &editing),
        ("scrolling", &scrolling),
        ("keys", &keys),
    ] {
        let p50 = t[t.len() / 2] / 1000.0;
        assert!(p50 < 4.0, "{name}: p50 {p50:.2} ms");
    }
    let _ = Duration::ZERO;
}
