//! First run: what this computer has, the model that will run well on it,
//! its download (with how long, and what's happening), then hello.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::{column, row};

use super::{bar, models::machine, models::tag};
use crate::app::{FirstRun, Msg, Tasuke};
use crate::models::{self, FitBar, size, time_at};

const WIDE: f64 = 640.0;

fn frame(parts: Vec<Element<Msg>>) -> Element<Msg> {
    let brand = row![
        container(icon(Icon::MessageSquare).size(18.0).role(Role::Accent))
            .padding(5.0)
            .fill(Tone::Chrome)
            .border(Edge::Line),
        column![
            text("Assistant").size(15.0).weight(700.0),
            text("助 Tasuke · on this computer").size(11.0).role(Role::Text3)
        ]
        .spacing(1.0),
    ]
    .spacing(10.0);
    let content = column_of(std::iter::once(brand.into()).chain(parts).collect()).spacing(18.0).max_width(WIDE);
    container(scroll(container(content).align_x(Align::Center).padding([40.0, 32.0]).width(Fill)))
        .fill(Tone::Surface)
        .width(Fill)
        .height(Fill)
        .into()
}

fn para(words: &str) -> Element<Msg> {
    text(words.to_owned()).size(13.5).role(Role::Text2).wrap().line_height(1.4).into()
}

fn point(glyph: Icon, words: String) -> Element<Msg> {
    row![
        super::lead(icon(glyph).size(15.0).role(Role::Text3)),
        text(words).size(13.0).role(Role::Text2).wrap().line_height(1.4)
    ]
    .spacing(10.0)
    .align(Align::Start)
    .into()
}

pub fn view(app: &Tasuke, step: &FirstRun) -> Element<Msg> {
    let hw = app.models.hardware.as_ref();
    let choice = app.first_choice();
    let entry = choice.as_ref().and_then(|id| app.models.catalog.get(id));
    match (step, hw) {
        (FirstRun::Looking, _) | (_, None) => frame(vec![
            text("Hello.").size(28.0).weight(700.0).into(),
            para("Looking at this computer, to see which models will run well on it…"),
            progress_indeterminate().width(260.0).into(),
        ]),
        (FirstRun::Recommend, Some(hw)) => {
            let mut parts: Vec<Element<Msg>> = vec![
                text("Here's what will run well on this computer").size(24.0).weight(700.0).into(),
                para(
                    "Tasuke is an assistant that runs entirely on this computer. It needs a model, downloaded once. \
                     This is the one Sekka recommends for your hardware.",
                ),
                machine(app, hw),
            ];
            match (entry, choice.as_ref()) {
                (Some(e), Some(_)) => {
                    let rec = models::first_choice(hw, &app.models.catalog, &app.models.prefs).expect("the choice");
                    let fb = FitBar::of(e, &rec.fit, hw);
                    let score = match rec.score {
                        Some(t) => format!("Fixed {} of {} faults in Sekka's evaluations.", t.passed, t.total),
                        None => "Not yet in Sekka's evaluations: it's the best that fits until a measured one does."
                            .to_owned(),
                    };
                    let card = container(
                        column![
                            row![text(e.name.clone()).size(17.0).weight(700.0), tag("Recommended", Role::Accent)]
                                .spacing(8.0),
                            text(format!("By {} · {} licence · {}", e.maker, e.license, e.origin.join(", ")))
                                .size(12.0)
                                .role(Role::Text3),
                            bar::verdict(&fb),
                            bar::bar(&fb, 12.0),
                            bar::legend(&fb),
                            text(score).size(12.5).role(Role::Text2).wrap().line_height(1.4),
                        ]
                        .spacing(8.0)
                        .width(Fill),
                    )
                    .padding([14.0, 16.0])
                    .fill(Tone::Chrome)
                    .border(Edge::Line2)
                    .width(Fill);
                    parts.push(card.into());
                    let total = e.download_size();
                    parts.push(
                        row![
                            button(format!("Download {} · {}", e.name, size(total)))
                                .icon(Icon::Download)
                                .primary()
                                .on_press(Msg::FirstRunDownload),
                            button("Choose Another…").on_press(Msg::FirstRunSkip),
                        ]
                        .spacing(8.0)
                        .into(),
                    );
                    parts.push(
                        text(format!(
                            "It's {}: {}. You can use the computer as usual while it downloads.",
                            size(total),
                            time_at(total, 100.0)
                        ))
                        .size(12.0)
                        .role(Role::Text3)
                        .wrap()
                        .line_height(1.4)
                        .into(),
                    );
                }
                _ => parts.push(para(
                    "None of the catalog's models fits this computer's memory well enough to be useful. \
                     Models lists them all with what each would need.",
                )),
            }
            frame(parts)
        }
        (FirstRun::Downloading, Some(_)) => {
            let Some(d) = &app.models.download else { return frame(vec![para("Starting the download…")]) };
            let name = app.models.catalog.get(&d.id).map(|e| e.name.clone()).unwrap_or_else(|| d.id.0.clone());
            let p = &d.progress;
            let control: Element<Msg> = if p.paused {
                button("Resume").icon(Icon::Play).on_press(Msg::Resume).into()
            } else {
                button("Pause").icon(Icon::Pause).on_press(Msg::Pause).into()
            };
            let sleeps = match app.models.idle_minutes {
                0 => "It loads when you send something and stays loaded until the model server stops.".to_owned(),
                n => format!(
                    "It loads when you send something and sleeps after {} unused, so games and other programs get the graphics card back.",
                    if n == 1 { "a minute".to_owned() } else { format!("{n} minutes") }
                ),
            };
            frame(vec![
                text(format!("Downloading {name}")).size(24.0).weight(700.0).into(),
                container(
                    column![
                        progress(p.fraction()).width(Fill),
                        row![text(p.words()).size(12.5).role(Role::Text2).width(Fill), control, button("Cancel").on_press(Msg::CancelDownload)].spacing(8.0),
                    ]
                    .spacing(10.0)
                    .width(Fill),
                )
                .padding([14.0, 16.0])
                .fill(Tone::Chrome)
                .border(Edge::Line)
                .width(Fill)
                .into(),
                aizome_kit::caption("While it downloads"),
                column![
                    point(Icon::Download, format!("{name} comes from Hugging Face, and every byte is checked against the fingerprint Sekka recorded for it, so you get exactly the model that was tested.")),
                    point(Icon::Lock, "It runs here, with llama.cpp. Nothing you ask leaves this computer.".into()),
                    point(Icon::Moon, sleeps),
                    point(Icon::Pause, "Pause or close the window whenever you like: the download picks up where it stopped.".into()),
                ]
                .spacing(12.0)
                .into(),
            ])
        }
        (FirstRun::Ready, Some(_)) => {
            let name = app
                .models
                .in_use
                .as_ref()
                .and_then(|id| app.models.catalog.get(id))
                .map(|e| e.name.clone())
                .unwrap_or_default();
            frame(vec![
                text("Ready.").size(28.0).weight(700.0).into(),
                para(&format!("{name} is downloaded and checked. Ask it anything, or tell it what's wrong.")),
                button("Say Hello").icon(Icon::MessageSquare).primary().on_press(Msg::FirstRunDone).into(),
            ])
        }
    }
}
