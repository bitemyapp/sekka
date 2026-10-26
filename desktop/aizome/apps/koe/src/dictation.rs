//! The window's look: the toolbar's pages, and the Dictation page, which
//! is everything setting dictation up takes: a speech model, the switch,
//! how it recognises and types, and its shortcuts.
use crate::{
    app::{Koe, Msg, Page, RECOMMENDED, language_options, silence_options},
    config::{Backend, GPU_COMPILED, OutputMode},
    models::{self, Installed},
    protocol::Phase,
    service,
};
use aizome_kit::{column, prelude::*};

const PRIVACY: &str = "Koe listens only from when you start dictating until you finish. What it hears \
     stays in memory on this computer, and is saved only when you save a teaching example.";

pub fn window(app: &Koe) -> Element<Msg> {
    let body = match app.page {
        Page::Dictation => dictation(app),
        Page::Vocabulary => app.vocab.view(),
    };
    column![toolbar(app), body].width(Fill).height(Fill).into()
}

fn toolbar(app: &Koe) -> Element<Msg> {
    let names: Vec<&str> = Page::ALL.iter().map(|p| p.name()).collect();
    let index = Page::ALL.iter().position(|p| *p == app.page);
    let switch = segmented(names, index).on_select(|i| Msg::Page(Page::ALL[i.min(Page::ALL.len() - 1)])).label("Page");
    aizome_kit::toolbar(vec![spacer().into(), switch.into(), spacer().into()])
}

fn dictation(app: &Koe) -> Element<Msg> {
    let mut parts: Vec<Element<Msg>> = vec![header(app)];
    if let Some((message, error)) = &app.message {
        parts.push(
            text(message.as_str()).role(if *error { Role::Danger } else { Role::Text2 }).wrap().id("message").into(),
        );
    }
    parts.extend([
        model_section(app),
        recognition_section(app),
        typing_section(app),
        shortcut_section(app),
        text(PRIVACY).role(Role::Text3).wrap().into(),
    ]);
    scroll(
        container(column(parts).spacing(20.0).max_width(660.0))
            .padding([20.0, 24.0])
            .width(Fill)
            .align_x(Align::Center),
    )
    .height(Fill)
    .into()
}

/// The first shortcut for starting and stopping, as it's typed: "⌥❖D".
fn shortcut(app: &Koe) -> Option<String> {
    app.shortcuts.0.first().map(|c| service::keycaps(c).concat())
}

/// What dictation is doing, and the switch.
fn header(app: &Koe) -> Element<Msg> {
    let on = app.config.enabled;
    let key = shortcut(app);
    let mut lines: Vec<Element<Msg>> = vec![];
    let mut title = |s: &str| lines.push(text(s).size(18.0).semibold().into());
    let mut more: Vec<Element<Msg>> = vec![];
    let detail = |s: String, role: Role| -> Element<Msg> { text(s).role(role).wrap().id("state").into() };
    if let Some(error) = &app.config_error {
        title("Koe's settings can't be read");
        more.push(detail(error.clone(), Role::Danger));
        more.push(button("Use Usual Settings…").on_press(Msg::AskReset).into());
        if app.status.is_some() {
            more.push(
                button("Stop It")
                    .on_press(Msg::Enable(false))
                    .enabled(app.switching.is_none())
                    .id("stop-service")
                    .into(),
            );
        }
    } else if let Some(turning_on) = app.switching.or(app.started.and(app.status.is_none().then_some(true))) {
        title(if turning_on { "Turning dictation on…" } else { "Turning dictation off…" });
        more.push(progress_indeterminate().into());
    } else if let Some(d) = app.download.as_ref().filter(|d| d.setting_up()) {
        title("Setting up dictation");
        more.push(detail(
            format!(
                "Downloading {} ({}). Dictation turns on when it's done.",
                d.model.title(),
                models::size_text(d.model.bytes)
            ),
            Role::Text2,
        ));
        more.push(
            row![
                progress(d.done as f64 / d.model.bytes as f64),
                button("Cancel").compact().on_press(Msg::CancelDownload)
            ]
            .spacing(10.0)
            .into(),
        );
    } else if !on && app.status.is_some() {
        // Turned off where the service couldn't be stopped (or by editing
        // koe.toml): the shortcut still works it.
        title("Dictation is off");
        more.push(detail("But its service is still running.".into(), Role::Warning));
        more.push(button("Stop It").on_press(Msg::Enable(false)).id("stop-service").into());
    } else if !on {
        title("Dictation is off");
        if app.ready_to_recognise() {
            more.push(detail(
                "Turn it on to dictate into any app. It starts again each time you log in.".into(),
                Role::Text2,
            ));
        } else {
            let which = app
                .find(RECOMMENDED)
                .map(|m| format!("{}, {}", m.title(), models::size_text(m.bytes)))
                .unwrap_or_default();
            more.push(detail(
                "Koe types what you say into the app you're using. It recognises speech on this \
                 computer, with a speech model it downloads once you ask."
                    .into(),
                Role::Text2,
            ));
            more.push(
                row![
                    button("Download a Model and Turn On")
                        .primary()
                        .icon(Icon::Download)
                        .on_press(Msg::SetUp)
                        .enabled(app.download.is_none())
                        .id("set-up"),
                    text(which).role(Role::Text3)
                ]
                .spacing(10.0)
                .into(),
            );
        }
    } else {
        match &app.status {
            None => {
                title("Dictation is on");
                more.push(detail("But its service isn't running.".into(), Role::Warning));
                more.push(button("Start It").on_press(Msg::Enable(true)).into());
            }
            Some(s) if s.teaching => {
                title("Teaching");
                more.push(detail(
                    "An example is being recorded or reviewed on the Vocabulary page.".into(),
                    Role::Text2,
                ));
            }
            Some(s) => match s.phase {
                Phase::Idle => {
                    title("Ready");
                    more.push(detail(
                        match &key {
                            Some(k) => format!(
                                "Press {k} in any app, speak, then press {k} again. Your words are typed where you were."
                            ),
                            None => "Give dictation a shortcut below, or start it from Koe in the menu bar.".into(),
                        },
                        Role::Text2,
                    ));
                    let place = if s.gpu { "on the graphics card" } else { "on the processor" };
                    let line = match s.transcription_ms {
                        Some(ms) => {
                            Some(format!("The last dictation took {:.2} s to recognise, {place}.", ms as f64 / 1000.0))
                        }
                        None if s.model_loaded => Some(format!("Recognition runs {place}.")),
                        None => None,
                    };
                    if let Some(line) = line {
                        more.push(text(line).role(Role::Text3).id("where").into());
                    }
                }
                Phase::Recording => {
                    title("Listening");
                    more.push(detail(
                        match &key {
                            Some(k) => format!("{:.1} s · press {k} again to finish", s.elapsed_ms as f64 / 1000.0),
                            None => format!("{:.1} s", s.elapsed_ms as f64 / 1000.0),
                        },
                        Role::Text2,
                    ));
                    // RMS is small: a fifth of full scale fills the bar.
                    more.push(progress(f64::from(s.level * 5.0)).label("Microphone level").into());
                }
                Phase::Paused => {
                    title("Paused");
                    more.push(detail(
                        "Your microphone is off. Resume or stop from Koe in the menu bar.".into(),
                        Role::Text2,
                    ));
                }
                Phase::Finishing | Phase::Transcribing => {
                    title("Transcribing");
                    more.push(detail("Your microphone is off.".into(), Role::Text2));
                    more.push(progress_indeterminate().into());
                }
                Phase::Delivering => {
                    title("Typing");
                    more.push(detail("Putting your words into the app.".into(), Role::Text2));
                }
                Phase::Error => {
                    title("Dictation needs attention");
                    more.push(detail(s.error.clone().unwrap_or_else(|| "Something went wrong.".into()), Role::Danger));
                    more.push(button("Dismiss").on_press(Msg::Dismiss).into());
                }
            },
        }
    }
    lines.extend(more);
    let switch = toggle(on)
        .on_toggle(Msg::Enable)
        .enabled(
            app.switching.is_none()
                && app.config_error.is_none()
                && app.download.is_none()
                && (on || app.ready_to_recognise()),
        )
        .label("Dictation")
        .id("dictation-switch");
    container(
        row![
            icon(if on { Icon::Mic } else { Icon::MicOff }).size(28.0).role(if on {
                Role::Accent
            } else {
                Role::Text3
            }),
            column(lines).spacing(6.0).width(Fill),
            switch
        ]
        .spacing(14.0)
        .align(Align::Start),
    )
    .padding(16.0)
    .fill(Tone::Chrome)
    .border(Edge::Line)
    .width(Fill)
    .into()
}

fn model_section(app: &Koe) -> Element<Msg> {
    let in_use = app.model_in_use();
    let mut rows: Vec<Element<Msg>> = vec![];
    let local = app.config.engine.backend == Backend::Local;
    if local && in_use.is_none() {
        // A model of one's own, named in koe.toml.
        let path = &app.config.engine.model;
        rows.push(
            form_row("Your model", text(if path.is_file() { "In use" } else { "Missing" }).role(Role::Text2))
                .description(path.display().to_string())
                .into(),
        );
    }
    for model in app.catalog.iter().cloned() {
        let name = model.name.clone();
        let using = in_use.as_ref().is_some_and(|m| m.name == name);
        let state = app.installed.get(&name).copied().unwrap_or(Installed::Missing);
        let idle = app.download.is_none();
        let control: Element<Msg> = match app.download.as_ref().filter(|d| d.model.name == name) {
            Some(d) => {
                let fraction = d.done as f64 / model.bytes as f64;
                row![
                    progress(fraction).width(110.0).label(format!("Downloading {}", model.title())),
                    text(format!("{:.0}%", fraction * 100.0)).tabular().role(Role::Text2),
                    button("Cancel").compact().on_press(Msg::CancelDownload)
                ]
                .spacing(8.0)
                .into()
            }
            None => match state {
                Installed::Present if using => row![
                    icon(Icon::Check).role(Role::Success),
                    text("In use").role(Role::Success),
                    button("Remove…")
                        .compact()
                        .on_press(Msg::AskRemove(name.clone()))
                        .enabled(!app.config.enabled)
                        .tooltip("Turn dictation off, or use another model, to remove this one")
                ]
                .spacing(6.0)
                .into(),
                Installed::Present => row![
                    button("Use").compact().on_press(Msg::UseModel(name.clone())).enabled(local),
                    button("Remove…").compact().on_press(Msg::AskRemove(name.clone()))
                ]
                .spacing(6.0)
                .into(),
                Installed::Damaged => row![
                    text("Incomplete").role(Role::Warning),
                    button("Download Again").compact().on_press(Msg::Download(name.clone())).enabled(idle)
                ]
                .spacing(6.0)
                .into(),
                Installed::Missing => button("Download")
                    .compact()
                    .icon(Icon::Download)
                    .on_press(Msg::Download(name.clone()))
                    .enabled(idle)
                    .id(format!("download:{name}"))
                    .into(),
            },
        };
        rows.push(form_row(model.title(), control).description(model.description()).into());
    }
    let mut footer = "Nothing is downloaded until you ask.".to_owned();
    // Room matters only when it's short.
    if let Some(free) = app.free.filter(|f| *f < 2_000_000_000) {
        footer.push_str(&format!(" Only {} is free on this disk.", models::size_text(free)));
    }
    if !local {
        footer = format!(
            "Koe sends what you say to {}, as koe.toml says, so these aren't used.",
            app.config.engine.http.endpoint
        );
    }
    column![
        form_section("Speech Model", rows),
        row![
            text(footer).role(Role::Text3).size(11.0).wrap().width(Fill),
            button("Show in Files")
                .compact()
                .on_press(Msg::ShowModels)
                .enabled(app.paths.desktop && app.paths.models.is_dir())
        ]
        .spacing(8.0)
    ]
    .spacing(6.0)
    .into()
}

fn recognition_section(app: &Koe) -> Element<Msg> {
    let engine = &app.config.engine;
    let english_only = app.model_in_use().is_some_and(|m| m.name.ends_with(".en"));
    let languages = language_options(&engine.language);
    let selected = if english_only {
        languages.iter().position(|(c, _)| c == "en")
    } else {
        languages.iter().position(|(c, _)| *c == engine.language)
    };
    let mut rows: Vec<Element<Msg>> = vec![
        form_row(
            "Language",
            dropdown(languages.iter().map(|(_, n)| n.clone()), selected)
                .on_select(Msg::Language)
                .enabled(!english_only)
                .label("Language"),
        )
        .description(if english_only {
            "This model knows only English."
        } else {
            "Choosing yours saves Koe working it out each time."
        })
        .into(),
        form_row(
            "Translate into English",
            toggle(engine.translate).on_toggle(Msg::Translate).enabled(!english_only).label("Translate into English"),
        )
        .description("Whatever language you speak, Koe types English.")
        .into(),
    ];
    if GPU_COMPILED {
        // Whether the service found one, once it has loaded the model.
        let missing = engine.gpu && app.status.as_ref().is_some_and(|s| s.model_loaded && !s.gpu);
        rows.push(
            form_row("Use the graphics card", toggle(engine.gpu).on_toggle(Msg::Gpu).label("Use the graphics card"))
                .description(if missing {
                    "Koe couldn't start a graphics card here, so it uses the processor."
                } else {
                    "Much faster on most computers. Where the card can't be used, Koe uses the processor."
                })
                .into(),
        );
    }
    rows.push(
        form_row(
            "Microphone",
            button("Sound Settings…").compact().on_press(Msg::OpenSettings("sound")).enabled(app.paths.desktop),
        )
        .description(match &app.config.audio.target {
            Some(target) => format!("{target}, as koe.toml says."),
            None => "The input chosen in Settings ▸ Sound.".into(),
        })
        .into(),
    );
    form_section("Recognition", rows)
}

fn typing_section(app: &Koe) -> Element<Msg> {
    let output = &app.config.output;
    let mode = match output.mode {
        OutputMode::Paste => 0,
        OutputMode::Clipboard => 1,
        OutputMode::None => 2,
    };
    let silences = silence_options(app.config.audio.silence_seconds);
    let silence = silences.iter().position(|s| *s == app.config.audio.silence_seconds);
    let label = |s: &f32| match *s {
        0.0 => "Never".to_owned(),
        1.0 => "After 1 second".to_owned(),
        s => format!("After {s} seconds"),
    };
    form_section(
        "Typing",
        vec![
            form_row(
                "When you finish",
                dropdown(["Type it into the app", "Copy it to the clipboard", "Keep it in Koe"], mode)
                    .on_select(Msg::Output)
                    .label("When you finish"),
            )
            .description(match output.mode {
                OutputMode::Paste => {
                    "Into the app you were using. If you've switched apps since, Koe keeps your words for \
                     Paste Last Transcript in its menu."
                }
                OutputMode::Clipboard => "Paste them yourself, wherever you like.",
                OutputMode::None => "Copy Last Transcript in Koe's menu gives them to you.",
            })
            .into(),
            form_row(
                "Finish when you stop talking",
                dropdown(silences.iter().map(label), silence)
                    .on_select(Msg::Silence)
                    .label("Finish when you stop talking"),
            )
            .description("Otherwise, press the shortcut again to finish.")
            .into(),
            form_row(
                "Add a space after",
                toggle(output.append_space).on_toggle(Msg::AppendSpace).label("Add a space after"),
            )
            .description("So the next dictation doesn't run into this one.")
            .into(),
            form_row("Notifications", toggle(output.notify).on_toggle(Msg::Notify).label("Notifications"))
                .description("When Koe is listening, transcribing and done, and what went wrong. Never your words.")
                .into(),
        ],
    )
}

fn chords(chords: &[String]) -> Element<Msg> {
    let mut caps = row![].spacing(3.0).align(Align::Center);
    if chords.is_empty() {
        caps = caps.push(text("None").role(Role::Text3));
    }
    for (i, chord) in chords.iter().enumerate() {
        if i > 0 {
            caps = caps.push(text("or").size(11.0).role(Role::Text3));
        }
        for cap in service::keycaps(chord) {
            caps = caps.push(keycap(cap));
        }
    }
    caps.into()
}

fn shortcut_section(app: &Koe) -> Element<Msg> {
    let (toggle_chords, cancel_chords) = &app.shortcuts;
    let mut buttons = row![].spacing(8.0);
    // What adding them would do: chords free for actions without one.
    let mut after = app.bindings.clone();
    if service::add_shortcuts(&mut after) {
        let added: Vec<String> = after
            .iter()
            .filter(|(chord, action)| app.bindings.get(*chord) != Some(action))
            .map(|(chord, _)| service::keycaps(chord).concat())
            .collect();
        buttons = buttons
            .push(button(format!("Use {}", added.join(" and "))).on_press(Msg::AddShortcuts).id("add-shortcuts"));
    }
    buttons = buttons.push(spacer()).push(
        button("Change in Keyboard Settings…")
            .on_press(Msg::OpenSettings("keyboard.commands"))
            .enabled(app.paths.desktop),
    );
    column![
        form_section(
            "Shortcuts",
            vec![
                form_row("Start or stop dictation", chords(toggle_chords)).into(),
                form_row("Cancel dictation", chords(cancel_chords)).into(),
            ],
        ),
        buttons.width(Fill)
    ]
    .spacing(8.0)
    .into()
}
