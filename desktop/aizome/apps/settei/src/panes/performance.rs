//! Display & Performance: which renderer the compositor runs (GPU or
//! software) and which it should, the effects tier, and frame times, live,
//! against Aizome's 4 ms budget: the compositor's per output (`aizomectl
//! stats`) and this window's own, with each display's refresh rate.

use std::time::Duration;

use aizome_ipc::{OutputInfo, Percentiles, Request, Response, Stats};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::text::TextSpec;
use aizome_kit::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};
use aizome_kit::{FontSpec, TimerHandle, column, draw, row};
use aizome_theme::Effects;

use crate::extra;
use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

/// The frame budget, in microseconds.
pub const BUDGET_US: f64 = 4000.0;

/// This window's frames, from the kit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Own {
    pub frames: usize,
    pub view_p50: f64,
    pub view_p95: f64,
    pub paint_p50: f64,
    pub paint_p95: f64,
}

#[derive(Default)]
pub struct Performance {
    stats: Option<Result<Stats, String>>,
    outputs: Vec<OutputInfo>,
    own: Own,
    tick: Option<TimerHandle>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tick,
    Stats(Box<Result<Stats, String>>),
    Outputs(Vec<OutputInfo>),
    Renderer(usize),
    Effects(usize),
    Reset,
    OpenDisplays,
}

pub const RENDERERS: [(&str, &str); 3] = [("auto", "Automatic"), ("gpu", "GPU"), ("software", "Software")];

fn sample_stats() -> Stats {
    let p = |p50: f64, p95: f64| Percentiles { p50, p95, p99: p95 * 1.3, max: p95 * 2.1, mean: (p50 + p95) / 2.0 };
    Stats {
        renderer: "gles".into(),
        backend: "udev".into(),
        outputs: vec![
            aizome_ipc::OutputStats {
                name: "eDP-1".into(),
                frames_rendered: 48_211,
                frames_skipped: 3_904,
                cpu_us: p(610.0, 1_480.0),
                gpu_us: Some(p(820.0, 1_900.0)),
                samples: 4096,
            },
            aizome_ipc::OutputStats {
                name: "DP-2".into(),
                frames_rendered: 12_007,
                frames_skipped: 811,
                cpu_us: p(940.0, 2_610.0),
                gpu_us: None,
                samples: 4096,
            },
        ],
        ..Stats::default()
    }
}

fn sample_outputs() -> Vec<OutputInfo> {
    let o = |name: &str, w, h, hz: i32, make: &str, model: &str| OutputInfo {
        name: name.into(),
        make: make.into(),
        model: model.into(),
        enabled: true,
        current_mode: Some(aizome_ipc::Mode { width: w, height: h, refresh: hz, preferred: true }),
        scale: 2.0,
        powered: true,
        ..OutputInfo::default()
    };
    vec![o("eDP-1", 2880, 1800, 120_000, "BOE", "NE135A1M-NY1"), o("DP-2", 3840, 2160, 60_000, "Dell", "U2723QE")]
}

/// "1.2 ms".
pub fn ms(us: f64) -> String {
    if us < 1000.0 { format!("{:.2} ms", us / 1000.0) } else { format!("{:.1} ms", us / 1000.0) }
}

impl Performance {
    fn poll(&mut self, cx: &mut PaneCx<Msg>) {
        let s = cx.kit().stats();
        let p = |x: &aizome_kit::stats::Samples, q: f64| f64::from(x.percentile(q));
        self.own = Own {
            frames: s.paint.len(),
            view_p50: p(&s.view_layout, 50.0),
            view_p95: p(&s.view_layout, 95.0),
            paint_p50: p(&s.paint, 50.0),
            paint_p95: p(&s.paint, 95.0),
        };
        if cx.sample() {
            self.stats = Some(Ok(sample_stats()));
            self.outputs = sample_outputs();
            return;
        }
        cx.compositor(Request::Stats { reset: false }, |r| match r {
            Ok(Response::Stats { stats }) => Msg::Stats(Box::new(Ok(stats))),
            Ok(Response::Error { message }) => Msg::Stats(Box::new(Err(message))),
            Ok(_) => Msg::Stats(Box::new(Err("an answer of another kind".into()))),
            Err(e) => Msg::Stats(Box::new(Err(e.to_string()))),
        });
        cx.compositor(Request::Outputs, |r| match r {
            Ok(Response::Outputs { outputs }) => Msg::Outputs(outputs),
            _ => Msg::Outputs(Vec::new()),
        });
    }
}

impl Pane for Performance {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "performance"
    }

    fn title(&self) -> &'static str {
        "Performance"
    }

    fn icon(&self) -> Icon {
        Icon::Gauge
    }

    fn tile(&self) -> Tile {
        Tile::Kohaku
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["display & performance", "speed", "fps", "frames", "gpu", "smooth"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("renderer", "Renderer").keywords(&[
                "gpu",
                "software rendering",
                "pixman",
                "gles",
                "opengl",
            ]),
            SettingEntry::new("effects", "Effects").keywords(&["shadows", "frost", "animation", "austere", "lean"]),
            SettingEntry::new("frames", "Frame times").keywords(&[
                "stats",
                "budget",
                "lag",
                "smoothness",
                "aizomectl stats",
            ]),
            SettingEntry::new("refresh", "Refresh rate").keywords(&["hz", "fps"]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        self.poll(cx);
        if !cx.sample() && self.tick.is_none() {
            self.tick = Some(cx.every(Duration::from_secs(1), Msg::Tick));
        }
    }

    fn hidden(&mut self, cx: &mut PaneCx<Msg>) {
        if let Some(t) = self.tick.take() {
            cx.cancel(t);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Tick => self.poll(cx),
            Msg::Stats(s) => self.stats = Some(*s),
            Msg::Outputs(o) => self.outputs = o,
            Msg::Renderer(i) => {
                cx.change(|s| extra::set(extra::section(s, "performance"), "renderer", RENDERERS[i.min(2)].0))
            }
            Msg::Effects(i) => {
                cx.change(|s| s.appearance.effects = [Effects::Full, Effects::Lean, Effects::Austere][i.min(2)])
            }
            Msg::OpenDisplays => cx.show_pane("displays", None),
            Msg::Reset => {
                cx.compositor(Request::Stats { reset: true }, |_| Msg::Tick);
            }
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = vcx.settings();
        let chosen = extra::get_str(extra::section_ref(s, "performance"), "renderer").unwrap_or("auto");
        let chosen_i = RENDERERS.iter().position(|(k, _)| *k == chosen).unwrap_or(0);
        let (running, about, glyph) = match &self.stats {
            Some(Ok(st)) if st.renderer == "gles" => {
                ("Rendering on the GPU", "OpenGL ES, through smithay's GlesRenderer.", Icon::Microchip)
            }
            Some(Ok(st)) if st.renderer == "pixman" => (
                "Rendering in software",
                "pixman, on the processor: there's no usable GPU, or it was chosen.",
                Icon::Cpu,
            ),
            Some(Ok(st)) => (st.renderer.as_str(), "", Icon::Cpu),
            Some(Err(_)) => (
                "Renderer unknown",
                "The compositor isn't answering: is this an Aizome session?",
                Icon::CircleQuestionMark,
            ),
            None => ("…", "", Icon::Gauge),
        };
        let backend = match &self.stats {
            Some(Ok(st)) => match st.backend.as_str() {
                "udev" => "On this computer's displays.",
                "winit" => "In a window of another desktop.",
                "headless" => "With no display (headless).",
                _ => "",
            },
            _ => "",
        };
        let hero = row![
            container(icon(glyph).size(26.0).role(Role::Focus))
                .center()
                .width(48.0)
                .height(48.0)
                .fill(Tone::Sunk)
                .border(Edge::Line2),
            column![
                text(running.to_owned()).size(17.0).weight(650.0),
                text(format!("{about} {backend}").trim().to_owned()).size(12.0).role(Role::Text3).wrap(),
            ]
            .spacing(3.0)
            .width(Fill),
        ]
        .spacing(14.0)
        .align(Align::Center);

        let mut frames: Vec<form::Row<Msg>> = Vec::new();
        match &self.stats {
            Some(Ok(st)) => {
                for o in &st.outputs {
                    let refresh = self
                        .outputs
                        .iter()
                        .find(|x| x.name == o.name)
                        .and_then(|x| x.current_mode.as_ref())
                        .map(|m| f64::from(m.refresh) / 1000.0);
                    let detail = format!(
                        "{} frames drawn, {} found nothing to draw{}",
                        o.frames_rendered,
                        o.frames_skipped,
                        refresh.map_or(String::new(), |r| format!(" · {r:.0} Hz"))
                    );
                    let mut r = form::whole(
                        container(
                            column![
                                row![text(o.name.clone()).mono().weight(600.0), spacer(), verdict(o.cpu_us.p95)]
                                    .align(Align::Center),
                                meter_el(o.cpu_us.p50, o.cpu_us.p95, o.cpu_us.max),
                                text(format!(
                                    "CPU p50 {} · p95 {} · p99 {}{}",
                                    ms(o.cpu_us.p50),
                                    ms(o.cpu_us.p95),
                                    ms(o.cpu_us.p99),
                                    o.gpu_us.map_or(String::new(), |g| format!(" · GPU p95 {}", ms(g.p95)))
                                ))
                                .mono()
                                .size(11.0)
                                .role(Role::Text2),
                                text(detail).size(11.0).role(Role::Text3),
                            ]
                            .spacing(6.0),
                        )
                        .padding([10.0, form::INSET])
                        .width(Fill),
                    );
                    if frames.is_empty() {
                        r = r.entry("frames");
                    }
                    frames.push(r);
                }
            }
            Some(Err(e)) => frames.push(
                form::whole(
                    container(
                        text(format!("No frame times from the compositor: {e}")).size(12.0).role(Role::Text3).wrap(),
                    )
                    .padding([12.0, form::INSET]),
                )
                .entry("frames"),
            ),
            None => frames.push(
                form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET]))
                    .entry("frames"),
            ),
        }
        let own = self.own;
        let own_p95 = own.view_p95 + own.paint_p95;
        frames.push(form::whole(
            container(
                column![
                    row![text("This window").weight(600.0), spacer(), verdict(own_p95)].align(Align::Center),
                    meter_el(own.view_p50 + own.paint_p50, own_p95, own_p95),
                    text(format!(
                        "view and layout p95 {} · drawing p95 {} · {} frames",
                        ms(own.view_p95),
                        ms(own.paint_p95),
                        own.frames
                    ))
                    .mono()
                    .size(11.0)
                    .role(Role::Text2),
                ]
                .spacing(6.0),
            )
            .padding([10.0, form::INSET])
            .width(Fill),
        ));

        let mut displays: Vec<form::Row<Msg>> = self
            .outputs
            .iter()
            .filter_map(|o| {
                let m = o.current_mode.as_ref()?;
                let name = if o.make.is_empty() { o.name.clone() } else { format!("{} {}", o.make, o.model) };
                Some(
                    form::value_mono(
                        name,
                        format!("{} × {} at {:.0} Hz", m.width, m.height, f64::from(m.refresh) / 1000.0),
                    )
                    .detail(o.name.clone())
                    .compact(),
                )
            })
            .collect();
        if displays.is_empty() {
            displays.push(form::whole(
                container(text("The compositor didn't say.").role(Role::Text3)).padding([10.0, form::INSET]),
            ));
        }
        let first = displays.remove(0);
        displays.insert(0, first.entry("refresh"));
        displays.push(form::row("Resolution and refresh rate", button("Displays…").on_press(Msg::OpenDisplays)));
        let effects_i = match s.appearance.effects {
            Effects::Full => 0,
            Effects::Lean => 1,
            Effects::Austere => 2,
        };
        form::page(vec![
            vcx.section(
                "Renderer",
                vec![
                    form::whole(container(hero).padding([14.0, form::INSET]).width(Fill)),
                    form::row("Use", form::segments(&RENDERERS.map(|r| r.1), chosen_i, Msg::Renderer))
                        .entry("renderer")
                        .help("Automatic takes the GPU when there's one that works. Software always works, and spends the processor. From the next login."),
                    form::row("Effects", form::segments(&["Full", "Lean", "Austere"], effects_i, Msg::Effects))
                        .entry("effects")
                        .help(super::appearance::effects_text(s.appearance.effects).1),
                ],
            ),
            vcx.section("Frame Times", frames),
            vcx.section("Displays", displays),
            row![spacer(), button("Start Counting Again").enabled(!vcx.sample()).on_press(Msg::Reset)].width(Fill).into(),
        ])
    }
}

fn verdict<Msg: 'static>(p95: f64) -> Element<Msg> {
    let (t, role) = if p95 <= 0.0 {
        ("no frames yet", Role::Text3)
    } else if p95 <= BUDGET_US {
        ("within budget", Role::Success)
    } else if p95 <= BUDGET_US * 2.0 {
        ("over budget", Role::Warning)
    } else {
        ("well over budget", Role::Danger)
    };
    row![
        icon(if role == Role::Success { Icon::CircleCheck } else { Icon::CircleAlert }).size(12.0).role(role),
        text(t).size(11.0).role(role)
    ]
    .spacing(4.0)
    .align(Align::Center)
    .into()
}

fn meter_el<Msg>(p50: f64, p95: f64, max: f64) -> Element<Msg> {
    Element::new(BudgetMeter { p50, p95, max }, Vec::new())
}

/// A bar from 0 to 8 ms: p95 filled, p50 darker within it, the worst as a
/// tick, and the 4 ms budget as a line in the focus colour.
struct BudgetMeter {
    p50: f64,
    p95: f64,
    max: f64,
}

impl Widget for BudgetMeter {
    fn name(&self) -> &'static str {
        "budget-meter"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(Size::new(bc.max.width.min(2000.0), 26.0))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let w = cx.size().width;
        let bar = Rect::new(0.0, 0.0, w, 12.0);
        draw::sunken(cx.canvas, t, bar, t.sunk(), t.line2());
        let scale = 2.0 * BUDGET_US;
        let x = |us: f64| (1.0 + (us / scale).clamp(0.0, 1.0) * (w - 2.0)).round();
        let over = self.p95 > BUDGET_US;
        let c95 = if over { t.role(Role::Warning) } else { t.role(Role::Accent) };
        cx.canvas.fill_rect(Rect::new(1.0, 1.0, x(self.p95), 11.0), c95.with_alpha(0.55));
        cx.canvas.fill_rect(Rect::new(1.0, 1.0, x(self.p50), 11.0), c95);
        if self.max > 0.0 {
            let m = x(self.max);
            cx.canvas.fill_rect(Rect::new(m - 1.0, 1.0, m, 11.0), t.text3());
        }
        let b = x(BUDGET_US);
        cx.canvas.fill_rect(Rect::new(b - 1.0, -2.0, b + 1.0, 14.0), t.focus());
        let label = TextSpec::new(FontSpec::mono(9.5), t.text3());
        cx.draw_text_in("0", &label, Rect::new(0.0, 13.0, 30.0, 26.0), Align::Start);
        cx.draw_text_in("4 ms", &label, Rect::new(b - 30.0, 13.0, b + 30.0, 26.0), Align::Center);
        cx.draw_text_in("8 ms", &label, Rect::new(w - 40.0, 13.0, w, 26.0), Align::End);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.f64(self.p50).f64(self.p95).f64(self.max);
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Meter,
            range: Some((0.0, 2.0 * BUDGET_US, self.p95)),
            ..Default::default()
        })
    }
}
