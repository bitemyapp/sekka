//! What Utsushi's window shows: the picture (or a song's cover) over a bar
//! of controls; in full screen, the controls over the picture until the
//! pointer rests; subtitles in the letterbox where they fit; the info
//! sheet; and the menus.

use std::path::PathBuf;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Canvas, Key, NamedKey, PointerButton};
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{Event, FocusRing};
use aizome_kit::{column, over_video, row, video_surface};
use aizome_media::{CueBody, FramePath};

use crate::app::{Msg, Repeat, Status, Utsushi, VIDEO, display_name};
use crate::widgets::{clock, subtitle_place, timeline};

/// The control bar's height.
pub const BAR: f64 = 44.0;

pub fn window(app: &Utsushi) -> Element<Msg> {
    let stage = stage(app);
    if app.fullscreen && app.has_video() {
        return stage;
    }
    column![stage, bar(app, false)].into()
}

/// The picture, the cover, or what to do; what's over them; and the
/// keys and clicks.
fn stage(app: &Utsushi) -> Element<Msg> {
    let mut layers: Vec<Element<Msg>> = Vec::new();
    match (&app.info, app.status) {
        (_, Status::Failed) => layers.push(failed(app)),
        (Some(info), _) if info.video.is_some() => {
            let v = info.video.as_ref().expect("video");
            layers.push(
                video_surface(VIDEO)
                    .picture(v.display.0, v.display.1)
                    .inhibit_idle(app.playing() && !app.hidden)
                    .key("video")
                    .into(),
            );
            if let Some(subs) = subtitles(app) {
                layers.push(subs.key("subtitles"));
            }
        }
        (Some(_), _) => layers.push(music(app)),
        (None, Status::Loading) => layers.push(container(text("")).fill(Tone::Sunk).width(Fill).height(Fill).into()),
        (None, _) => layers.push(empty(app)),
    }
    if let Some(at) = app.resumed.filter(|_| app.info.is_some()) {
        layers.push(resumed(at).key("resumed"));
    }
    // Over everything but the full-screen controls: what takes keys and
    // clicks on the picture.
    let input = canvas(app.fullscreen, |_c: &mut Canvas, _b: Rect, _s: &bool, _t: &Theme| {})
        .on_event(stage_event)
        .focusable()
        .focus_ring(FocusRing::Never)
        .id("stage")
        .key("input")
        .label("Picture");
    layers.push(input.into());
    if app.fullscreen && app.controls && app.has_video() {
        layers.push(
            container(
                mouse_area(over_video(container(bar(app, true)).floating().max_width(900.0)))
                    .on_hover(Msg::OverControls),
            )
            .padding([0.0, 24.0, 24.0, 24.0])
            .align_x(Align::Center)
            .align_y(Align::End)
            .width(Fill)
            .height(Fill)
            .key("controls")
            .into(),
        );
    }
    mouse_area(aizome_kit::widgets::layout::stack(layers).width(Fill).height(Fill))
        .on_drop(|d| Msg::Open(d.urls.iter().filter_map(|u| file_path(u)).collect()))
        .key("stage-area")
        .into()
}

fn stage_event(ev: &Event, _size: Size, fullscreen: &bool) -> Option<Msg> {
    match ev {
        Event::PointerDown { button: PointerButton::Left, clicks, .. } => Some(Msg::StageClick(*clicks)),
        Event::PointerMove { .. } if *fullscreen => Some(Msg::Pointer),
        Event::Scroll { delta, mods, .. } if !mods.super_key() => {
            (delta.y.abs() > 0.5).then(|| Msg::VolumeBy(-(delta.y / aizome_kit::LINE) as f32 * 0.05))
        }
        Event::Key(k) if k.pressed => keys(k),
        _ => None,
    }
}

/// The keys the picture answers (the menus show them; the kit's menus
/// match only those with ❖ or ⌃).
fn keys(k: &aizome_kit::sidestep_ui::KeyEvent) -> Option<Msg> {
    let m = k.modifiers;
    if m.super_key() || m.control() {
        return None;
    }
    let small = m.shift();
    match &k.key {
        Key::Named(NamedKey::ArrowLeft) => Some(Msg::SeekBy(if small { -1.0 } else if m.alt() { -60.0 } else { -5.0 })),
        Key::Named(NamedKey::ArrowRight) => Some(Msg::SeekBy(if small { 1.0 } else if m.alt() { 60.0 } else { 5.0 })),
        Key::Named(NamedKey::ArrowUp) => Some(Msg::VolumeBy(0.05)),
        Key::Named(NamedKey::ArrowDown) => Some(Msg::VolumeBy(-0.05)),
        Key::Named(NamedKey::Home) => Some(Msg::SeekTo(0.0)),
        Key::Named(NamedKey::PageUp) => Some(Msg::Chapter(-1)),
        Key::Named(NamedKey::PageDown) => Some(Msg::Chapter(1)),
        Key::Named(NamedKey::Escape) => Some(Msg::Escape),
        Key::Character(s) => match s.to_lowercase().as_str() {
            " " | "k" => Some(Msg::Toggle),
            "j" => Some(Msg::SeekBy(-10.0)),
            "l" => Some(Msg::SeekBy(10.0)),
            "f" => Some(Msg::ToggleFullscreen),
            "m" => Some(Msg::Mute),
            "c" => Some(Msg::CycleSubtitles),
            "s" => Some(Msg::SavePicture),
            "z" => Some(Msg::SubtitleDelay(Some(-0.1))),
            "x" => Some(Msg::SubtitleDelay(Some(0.1))),
            "," | "<" => Some(Msg::Step(false)),
            "." | ">" => Some(Msg::Step(true)),
            "[" => Some(Msg::SpeedBy(-1)),
            "]" => Some(Msg::SpeedBy(1)),
            "n" if small => Some(Msg::Next),
            "p" if small => Some(Msg::Previous),
            d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => {
                Some(Msg::SeekFraction(f64::from(d.parse::<u8>().unwrap_or(0)) / 10.0))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The cues showing, in a box over the picture's bottom, or in the
/// letterbox under it where they fit.
fn subtitles(app: &Utsushi) -> Option<Element<Msg>> {
    let cues = app.active_cues();
    if cues.is_empty() {
        return None;
    }
    let picture = app.info.as_ref().and_then(|i| i.video.as_ref()).map(|v| v.display);
    let fitted = aizome_kit::video::fit(app.stage, picture.map(|(w, h)| Size::new(w, h)));
    let size = (fitted.height() * 0.042).clamp(15.0, 42.0).round();
    let mut lines: Vec<Element<Msg>> = Vec::new();
    let mut top = false;
    let mut pictures: Vec<Element<Msg>> = Vec::new();
    for c in cues {
        match &c.body {
            CueBody::Text { text: words, italic, top: t } => {
                top |= *t;
                for line in words.lines() {
                    let t = text(line.to_owned()).color(Color::WHITE).size(size).weight(560.0);
                    lines.push(if *italic { t.italic() } else { t }.into());
                }
            }
            CueBody::Picture { rgba, w, h, x, y, frame } => {
                if let Some(img) = Image::from_rgba(*w, *h, rgba) {
                    pictures.push(picture_cue(c.id, img, (*x, *y), *frame, picture));
                }
            }
            CueBody::Clear => {}
        }
    }
    if !pictures.is_empty() {
        return Some(over_video(aizome_kit::widgets::layout::stack(pictures).width(Fill).height(Fill)));
    }
    let lines = container(aizome_kit::widgets::layout::column(lines).spacing(2.0).align(Align::Center))
        .padding([4.0, 10.0])
        .key("cue-box");
    let boxed = crate::widgets::scrim(lines.into());
    Some(subtitle_place(picture, top, over_video(boxed)))
}

/// A picture subtitle, placed on the video's frame.
fn picture_cue(id: u64, img: Image, at: (i32, i32), frame: (u32, u32), picture: Option<(f64, f64)>) -> Element<Msg> {
    // Drawn again when the cue changes: its id is what the canvas hashes.
    canvas(id, move |c: &mut Canvas, b: Rect, _id: &u64, _t: &Theme| {
        let ((x, y), (fw, fh)) = (&at, &frame);
        if *fw == 0 || *fh == 0 {
            return;
        }
        let pic = aizome_kit::video::fit(b.size(), picture.map(|(w, h)| Size::new(w, h)));
        let s = pic.width() / f64::from(*fw);
        let (iw, ih) = img.pixel_size();
        let dst = Rect::from_origin_size(
            Point::new(b.x0 + pic.x0 + f64::from(*x) * s, b.y0 + pic.y0 + f64::from(*y) * s),
            Size::new(f64::from(iw) * s, f64::from(ih) * s),
        );
        c.draw_image(&img, dst);
    })
    .width(Fill)
    .height(Fill)
    .into()
}

fn resumed(at: f64) -> Element<Msg> {
    let note = container(
        row![
            text(format!("Resumed at {}", clock(at))).role(Role::Text2),
            button("Start Over").compact().on_press(Msg::StartOver),
            icon_button(Icon::X).plain().compact().on_press(Msg::DismissResumed).tooltip("Dismiss"),
        ]
        .spacing(8.0),
    )
    .padding([4.0, 10.0])
    .floating();
    container(over_video(note)).padding(16.0).align_x(Align::Start).align_y(Align::Start).width(Fill).height(Fill).into()
}

/// A song: its cover (or a tile in the dye), what it is and who made it;
/// beside it, the queue when there's more than one.
fn music(app: &Utsushi) -> Element<Msg> {
    let info = app.info.as_ref().expect("info");
    let art: Element<Msg> = match &app.cover {
        Some(img) => container(image(img.clone()).fit(Fit::Cover).width(Fill).height(Fill)).border(Edge::Line).into(),
        None => container(icon(Icon::Music).size(64.0).role(Role::Faint))
            .center()
            .fill(Tone::Chrome)
            .border(Edge::Line)
            .width(Fill)
            .height(Fill)
            .into(),
    };
    // The title, the line under it and the space between: about 72pt.
    let art = crate::widgets::square(280.0, 72.0, art);
    let mut by = Vec::new();
    if let Some(a) = &info.artist {
        by.push(a.clone());
    }
    if let Some(a) = &info.album {
        by.push(a.clone());
    }
    let mut about = by.join(" — ");
    if let Some(y) = &info.year {
        about = if about.is_empty() { y.clone() } else { format!("{about} · {y}") };
    }
    let now = column![
        art,
        space(14.0),
        text(info.title.clone()).size(22.0).semibold().max_width(560.0),
        text(about).role(Role::Text2).max_width(560.0),
    ]
    .align(Align::Center)
    .spacing(4.0)
    .padding(24.0);
    let now = container(now).center().width(Fill).height(Fill).fill(Tone::Sunk);
    if app.queue.len() < 2 {
        return now.into();
    }
    let names: std::rc::Rc<Vec<String>> = std::rc::Rc::new(app.queue.iter().map(|p| display_name(p)).collect());
    let current = app.current;
    let list = virtual_list(names.len(), 28.0, move |i| {
        let n = names[i].clone();
        row![
            text(format!("{}", i + 1)).mono().tabular().role(Role::Text3).width(28.0),
            text(n).role(if Some(i) == current { Role::Accent } else { Role::Text }),
        ]
        .spacing(6.0)
        .padding([0.0, 10.0])
    })
    .selected(current)
    .on_activate(Msg::PlayItem);
    split(now, container(list).fill(Tone::Chrome)).ratio(0.62).into()
}

fn empty(app: &Utsushi) -> Element<Msg> {
    let mut parts: Vec<Element<Msg>> = vec![
        icon(Icon::Film).size(48.0).role(Role::Faint).into(),
        text("Open a video or some music").size(17.0).semibold().role(Role::Text2).into(),
        text("Or drop files and folders here.").role(Role::Text3).into(),
        space(4.0),
        button("Open…").primary().on_press(Msg::OpenPanel).into(),
    ];
    let recent: Vec<Element<Msg>> = app
        .store
        .recent
        .iter()
        .take(6)
        .enumerate()
        .map(|(i, r)| {
            let left = app.store.resume_at(&r.path).map(|p| format!("  ·  {} in", clock(p))).unwrap_or_default();
            let length = r.duration.map(clock).unwrap_or_default();
            button(format!("{}   {length}{left}", r.title))
                .plain()
                .icon(if r.video { Icon::Film } else { Icon::Music })
                .on_press(Msg::OpenRecent(i))
                .into()
        })
        .collect();
    if !recent.is_empty() {
        parts.push(space(18.0));
        parts.push(caption("Recent"));
        parts.extend(recent);
    }
    container(aizome_kit::widgets::layout::column(parts).spacing(8.0).align(Align::Center))
        .center()
        .fill(Tone::Sunk)
        .width(Fill)
        .height(Fill)
        .into()
}

fn failed(app: &Utsushi) -> Element<Msg> {
    let name = app.current_path().map(display_name).unwrap_or_default();
    let mut actions: Vec<Element<Msg>> = vec![button("Open…").on_press(Msg::OpenPanel).into()];
    if app.can_next() {
        actions.push(button("Play the Next").primary().on_press(Msg::Next).into());
    }
    container(
        column![
            icon(Icon::CircleAlert).size(40.0).role(Role::Faint),
            text(format!("Utsushi can't play “{name}”")).size(15.0).semibold().role(Role::Text2),
            text(app.error.clone().unwrap_or_default()).role(Role::Text3).wrap().center(),
            space(4.0),
            aizome_kit::widgets::layout::row(actions).spacing(8.0),
        ]
        .spacing(8.0)
        .align(Align::Center)
        .padding(24.0),
    )
    .center()
    .fill(Tone::Sunk)
    .width(Fill)
    .height(Fill)
    .into()
}

fn tool(icon_: Icon, tip: &str, msg: Msg, enabled: bool) -> Element<Msg> {
    icon_button(icon_).toolbar().on_press(msg).enabled(enabled).tooltip(tip).into()
}

/// The controls: previous, play, next; the time, the timeline, what's
/// left; volume; tracks and speed; full screen. `floating`: over the
/// picture in full screen.
fn bar(app: &Utsushi, floating: bool) -> Element<Msg> {
    let have = app.info.is_some();
    let d = app.duration();
    let pos = app.shown_pos();
    let many = app.queue.len() > 1;
    let chapters = app.chapters();
    let mut items: Vec<Element<Msg>> = Vec::new();
    if many || !chapters.is_empty() {
        let (msg, tip) = if many { (Msg::Previous, "Previous  ⇧P") } else { (Msg::Chapter(-1), "Previous Chapter  ⇞") };
        items.push(tool(Icon::SkipBack, tip, msg, have));
    }
    let (play_icon, play_tip) = if app.playing() { (Icon::Pause, "Pause  Space") } else { (Icon::Play, "Play  Space") };
    items.push(
        icon_button(play_icon)
            .toolbar()
            .on_press(Msg::Toggle)
            .enabled(have || app.status == Status::Empty)
            .tooltip(play_tip)
            .key("play")
            .into(),
    );
    if many || !chapters.is_empty() {
        let (msg, tip) = if many { (Msg::Next, "Next  ⇧N") } else { (Msg::Chapter(1), "Next Chapter  ⇟") };
        items.push(tool(Icon::SkipForward, tip, msg, have && (app.can_next() || !chapters.is_empty())));
    }
    let mono = FontSpec { tabular: true, ..FontSpec::mono(12.0) };
    items.push(space(4.0));
    items.push(crate::widgets::digits(&clock(pos), mono, Role::Text2).key("elapsed"));
    items.push(timeline(pos, d, chapters, have && app.info.as_ref().is_some_and(|i| i.seekable)));
    let right = if app.show_remaining && d > 0.0 { format!("−{}", clock((d - pos).max(0.0))) } else { clock(d) };
    items.push(
        button(right)
            .plain()
            .compact()
            .mono()
            .on_press(Msg::ToggleRemaining)
            .tooltip(if app.show_remaining { "Show the length" } else { "Show what's left" })
            .key("right-time")
            .into(),
    );
    let vol_icon = if app.muted || app.volume <= 0.0 {
        Icon::VolumeX
    } else if app.volume < 0.34 {
        Icon::Volume
    } else if app.volume < 0.67 {
        Icon::Volume1
    } else {
        Icon::Volume2
    };
    items.push(tool(vol_icon, if app.muted { "Unmute  M" } else { "Mute  M" }, Msg::Mute, true));
    items.push(
        slider(if app.muted { 0.0 } else { f64::from(app.volume) }, 0.0..=1.0)
            .on_change(|v| Msg::Volume(v as f32))
            .width(84.0)
            .key("volume")
            .label("Volume")
            .into(),
    );
    if (app.speed - 1.0).abs() > 1e-3 {
        items.push(
            button(format!("{}×", trim(app.speed)))
                .toolbar()
                .mono()
                .on_press(Msg::SpeedMenu)
                .id("speed")
                .tooltip("Speed  [ ]")
                .into(),
        );
    }
    let tracks = app.info.as_ref().is_some_and(|i| !i.subtitles.is_empty() || i.audio.len() > 1 || i.video.is_some());
    if tracks {
        items.push(
            icon_button(Icon::Captions)
                .toolbar()
                .selected(app.sub_track.is_some())
                .on_press(Msg::TracksMenu)
                .id("tracks")
                .tooltip("Subtitles and Audio")
                .into(),
        );
    }
    if app.has_video() {
        let (i, tip) =
            if app.fullscreen { (Icon::Minimize, "Exit Full Screen  F") } else { (Icon::Maximize, "Full Screen  F") };
        items.push(tool(i, tip, Msg::ToggleFullscreen, true));
    }
    let row = aizome_kit::widgets::layout::row(items).spacing(6.0).align(Align::Center).padding([0.0, 10.0]);
    let c = container(row).height(BAR).width(Fill).align_y(Align::Center);
    if floating { c.fill(Tone::Chrome).into() } else { c.fill(Tone::Chrome).edges(Edge::BevelLo, Sides::TOP).into() }
}

fn trim(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Subtitles and audio tracks, as a menu by the button.
pub fn tracks_menu(app: &Utsushi) -> Menu<Msg> {
    let mut m = Menu::new("Tracks");
    let Some(info) = &app.info else { return m };
    m = m.header("Subtitles").item(MenuItem::new("Off", Msg::Subtitles(None)).checked(app.sub_track.is_none()));
    for (i, t) in info.subtitles.iter().enumerate() {
        m = m.item(MenuItem::new(t.label(i + 1), Msg::Subtitles(Some(i))).checked(app.sub_track == Some(i)));
    }
    m = m.item(MenuItem::new("Add Subtitle File…", Msg::AddSubtitles));
    if !info.audio.is_empty() {
        m = m.separator().header("Audio");
        for (i, t) in info.audio.iter().enumerate() {
            m = m.item(MenuItem::new(t.label(i + 1), Msg::Audio(Some(i))).checked(app.audio_track == Some(i)));
        }
    }
    m.separator().submenu(speed_menu(app))
}

pub fn speed_menu(app: &Utsushi) -> Menu<Msg> {
    let mut m = Menu::new("Speed");
    for s in [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
        let label = if s == 1.0 { "Normal".to_owned() } else { format!("{}×", trim(s)) };
        m = m.item(MenuItem::new(label, Msg::Speed(s)).checked((app.speed - s).abs() < 1e-3));
    }
    m
}

pub fn menus(app: &Utsushi) -> MenuBar<Msg> {
    let info = MenuBar::<Msg>::standard(&aizome_kit::AppInfo::new(crate::app::APP_ID, "Utsushi", "Media Player"));
    let have = app.info.is_some();
    let video = app.has_video();
    let seekable = app.info.as_ref().is_some_and(|i| i.seekable);
    let key = Shortcut::key;
    let mut recent = Menu::new("Open Recent");
    for (i, r) in app.store.recent.iter().take(12).enumerate() {
        recent = recent.item(MenuItem::new(r.title.clone(), Msg::OpenRecent(i)));
    }
    recent = recent.separator().item(MenuItem::new("Clear Menu", Msg::ClearRecent).enabled(!app.store.recent.is_empty()));
    let file = Menu::new("File")
        .item(MenuItem::new("Open…", Msg::OpenPanel).shortcut(Shortcut::cmd('O')))
        .submenu(recent)
        .separator()
        .item(MenuItem::new("Media Info", Msg::ShowInfo).shortcut(Shortcut::cmd('I')).enabled(have))
        .item(MenuItem::new("Show in Hako", Msg::ShowInHako).shortcut(Shortcut::cmd('R').alt()).enabled(app.current_path().is_some()))
        .separator()
        .item(MenuItem::new("Close", Msg::Close).shortcut(Shortcut::cmd('W')).enabled(have || app.status == Status::Failed));
    let mut repeat = Menu::new("Repeat");
    for (label, r) in [("Off", Repeat::Off), ("This Item", Repeat::One), ("All", Repeat::All)] {
        repeat = repeat.item(MenuItem::new(label, Msg::Repeat(r)).checked(app.repeat == r));
    }
    let playback = Menu::new("Playback")
        .item(
            MenuItem::new(if app.playing() { "Pause" } else { "Play" }, Msg::Toggle)
                .shortcut(key(ShortcutKey::Space))
                .enabled(have),
        )
        .separator()
        .item(MenuItem::new("Back 5 Seconds", Msg::SeekBy(-5.0)).shortcut(key(ShortcutKey::Left)).enabled(seekable))
        .item(MenuItem::new("Forward 5 Seconds", Msg::SeekBy(5.0)).shortcut(key(ShortcutKey::Right)).enabled(seekable))
        .item(MenuItem::new("Back 10 Seconds", Msg::SeekBy(-10.0)).shortcut(key(ShortcutKey::Char('J'))).enabled(seekable))
        .item(MenuItem::new("Forward 10 Seconds", Msg::SeekBy(10.0)).shortcut(key(ShortcutKey::Char('L'))).enabled(seekable))
        .item(MenuItem::new("Previous Chapter", Msg::Chapter(-1)).shortcut(key(ShortcutKey::PageUp)).enabled(!app.chapters().is_empty()))
        .item(MenuItem::new("Next Chapter", Msg::Chapter(1)).shortcut(key(ShortcutKey::PageDown)).enabled(!app.chapters().is_empty()))
        .item(MenuItem::new("Previous Frame", Msg::Step(false)).shortcut(key(ShortcutKey::Char(','))).enabled(video && seekable))
        .item(MenuItem::new("Next Frame", Msg::Step(true)).shortcut(key(ShortcutKey::Char('.'))).enabled(video))
        .separator()
        .item(MenuItem::new("Previous Item", Msg::Previous).shortcut(key(ShortcutKey::Char('P')).shift()).enabled(app.can_previous()))
        .item(MenuItem::new("Next Item", Msg::Next).shortcut(key(ShortcutKey::Char('N')).shift()).enabled(app.can_next()))
        .separator()
        .item(MenuItem::new("Slower", Msg::SpeedBy(-1)).shortcut(key(ShortcutKey::Char('['))).enabled(have))
        .item(MenuItem::new("Faster", Msg::SpeedBy(1)).shortcut(key(ShortcutKey::Char(']'))).enabled(have))
        .submenu(speed_menu(app))
        .submenu(repeat);
    let mut subs = Menu::new("Subtitles")
        .item(MenuItem::new("Off", Msg::Subtitles(None)).checked(app.sub_track.is_none()).enabled(have));
    let mut audio = Menu::new("Audio")
        .item(MenuItem::new("Louder", Msg::VolumeBy(0.05)).shortcut(key(ShortcutKey::Up)))
        .item(MenuItem::new("Quieter", Msg::VolumeBy(-0.05)).shortcut(key(ShortcutKey::Down)))
        .item(MenuItem::new("Mute", Msg::Mute).shortcut(key(ShortcutKey::Char('M'))).checked(app.muted));
    if let Some(info) = &app.info {
        for (i, t) in info.subtitles.iter().enumerate() {
            subs = subs.item(MenuItem::new(t.label(i + 1), Msg::Subtitles(Some(i))).checked(app.sub_track == Some(i)));
        }
        if !info.audio.is_empty() {
            audio = audio.separator();
            for (i, t) in info.audio.iter().enumerate() {
                audio = audio.item(MenuItem::new(t.label(i + 1), Msg::Audio(Some(i))).checked(app.audio_track == Some(i)));
            }
        }
    }
    subs = subs
        .separator()
        .item(MenuItem::new("Next Subtitles", Msg::CycleSubtitles).shortcut(key(ShortcutKey::Char('C'))).enabled(have))
        .item(MenuItem::new("Show Earlier", Msg::SubtitleDelay(Some(-0.1))).shortcut(key(ShortcutKey::Char('Z'))).enabled(have))
        .item(MenuItem::new("Show Later", Msg::SubtitleDelay(Some(0.1))).shortcut(key(ShortcutKey::Char('X'))).enabled(have))
        .item(
            MenuItem::new(
                if app.sub_delay == 0.0 { "Original Timing".to_owned() } else { format!("Original Timing (now {:+.1} s)", app.sub_delay) },
                Msg::SubtitleDelay(None),
            )
            .enabled(app.sub_delay != 0.0),
        )
        .item(MenuItem::new("Add Subtitle File…", Msg::AddSubtitles).enabled(video));
    let view = Menu::new("View").item(
        MenuItem::new(if app.fullscreen { "Exit Full Screen" } else { "Enter Full Screen" }, Msg::ToggleFullscreen)
            .shortcut(key(ShortcutKey::Char('F')))
            .enabled(video),
    );
    let edit = Menu::new("Edit")
        .item(MenuItem::new("Copy Picture", Msg::CopyPicture).shortcut(Shortcut::cmd('C')).enabled(video))
        .item(
            MenuItem::new("Save Picture to Pictures", Msg::SavePicture)
                .shortcut(Shortcut::cmd('S').shift())
                .enabled(video),
        );
    info.with_file_menu(file).replace("Edit", edit).insert(2, playback).insert(3, audio).insert(4, subs).insert(5, view)
}

/// The info sheet: what the media is, and how it's being played.
pub fn info_sheet(app: &Utsushi) -> Element<Msg> {
    let Some(info) = &app.info else { return text("").into() };
    let row_ = |label: &str, value: String| -> Element<Msg> {
        form_row(label, text(value).role(Role::Text2).selectable().max_lines(2)).into()
    };
    let mut file = Vec::new();
    if let Some(p) = &info.path {
        file.push(row_("Name", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        file.push(row_("Where", p.parent().map(|d| d.display().to_string()).unwrap_or_default()));
    } else {
        file.push(row_("Address", info.url.clone()));
    }
    if let Some(s) = info.size {
        file.push(row_("Size", size_text(s)));
    }
    file.push(row_("Format", info.format.clone()));
    if let Some(d) = info.duration {
        file.push(row_("Length", clock(d)));
    }
    let mut sections = vec![
        text(info.title.clone()).size(17.0).semibold().max_lines(2).into(),
        form_section("File", file),
    ];
    if let Some(v) = &info.video {
        let mut rows = vec![
            row_("Codec", format!("{} · {}-bit", v.codec.to_uppercase(), v.bit_depth)),
            row_("Size", format!("{} × {}", v.width, v.height)),
        ];
        if let Some(f) = v.fps {
            rows.push(row_("Frame rate", format!("{} fps", trim((f * 1000.0).round() / 1000.0))));
        }
        if v.hdr {
            rows.push(row_("Range", "HDR, shown as standard range".into()));
        }
        sections.push(form_section("Video", rows));
    }
    if let Some(t) = app.audio_track.and_then(|i| info.audio.get(i)) {
        sections.push(
            form_section(
                "Audio",
                vec![row_("Track", t.label(app.audio_track.unwrap_or(0) + 1)), row_("Codec", t.codec.to_uppercase())],
            ),
        );
    }
    // How it's being played, which is what costs the battery.
    let stats = app.player.as_ref().map(aizome_media::Player::stats);
    let mut how = Vec::new();
    if info.video.is_some() {
        let decoding = match (app.pipeline.video_decoder.as_deref(), stats) {
            (Some(c), Some(s)) if s.path != FramePath::None => {
                format!("{c} {}", if s.gpu { "on the GPU (VA-API)" } else { "in software" })
            }
            (Some(c), _) => c.to_owned(),
            _ => "—".into(),
        };
        how.push(row_("Video decoding", decoding));
        if let Some(s) = stats {
            let path = match s.path {
                FramePath::Dmabuf => "As decoded: the compositor shows the GPU's frames",
                FramePath::Software => "Converted once into shared memory",
                FramePath::Downloaded => "Copied from the GPU (the compositor can't take its frames)",
                FramePath::None => "—",
            };
            how.push(row_("Frames to the screen", path.into()));
            how.push(row_(
                "Frames",
                format!("{} shown, {} dropped{}", s.shown, s.dropped, s.refresh_hz.map(|h| format!(" · display at {h:.0} Hz")).unwrap_or_default()),
            ));
            how.push(row_(
                "Out of sight",
                if s.suspended {
                    "Not decoding video now: the window is out of sight".into()
                } else if s.suspensions > 0 {
                    format!("Video decoding stopped {} time{} while out of sight", s.suspensions, if s.suspensions == 1 { "" } else { "s" })
                } else {
                    "Video stops decoding while the window is out of sight".into()
                },
            ));
        }
    }
    if let Some(a) = &app.pipeline.audio_decoder {
        how.push(row_("Audio decoding", a.clone()));
    }
    how.push(row_("Sound", app.pipeline.audio_output.clone()));
    sections.push(form_section("Playing", how));
    sections.push(row![spacer(), button("Done").default().on_press(Msg::CloseSheet)].into());
    container(scroll(aizome_kit::widgets::layout::column(sections).spacing(14.0).padding(18.0)))
        .width(520.0)
        .max_height(620.0)
        .into()
}

fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    }
}

/// The path of a `file://` URL.
pub fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_and_sizes_read_cleanly() {
        assert_eq!(trim(1.5), "1.5");
        assert_eq!(trim(2.0), "2");
        assert_eq!(trim(0.75), "0.75");
        assert_eq!(size_text(1_536_000), "1.5 MB");
        assert_eq!(size_text(4_200_000_000), "4.20 GB");
    }

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(file_path("file:///home/me/A%20Film.mkv"), Some(PathBuf::from("/home/me/A Film.mkv")));
        assert_eq!(file_path("https://x"), None);
    }
}
