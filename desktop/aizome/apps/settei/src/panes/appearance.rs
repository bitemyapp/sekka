//! Appearance: Night, Day, Auto and High Contrast with live previews; the
//! focus colour; the effects tier; the size; wallpapers for Night and Day;
//! reduced motion. `[appearance]`, applied by the compositor, the shell and
//! every app at once.

use std::path::{Path, PathBuf};

use aizome_config::{ThemeChoice, UiSize};
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_theme::{Effects, FocusColor, Palette, ThemeName};

use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::preview::{self, Look};
use crate::wallpaper;
use crate::widgets::{PressStyle, picture, press, swatch};

/// A preview's size.
const PREVIEW: (f64, f64) = (132.0, 84.0);

#[derive(Default)]
pub struct Appearance {
    /// The wallpapers found, with their thumbnails as they come; `None`
    /// until looked for.
    wallpapers: Option<Vec<(PathBuf, Option<Image>)>>,
    /// Which theme's wallpaper the grid chooses.
    day_tab: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Theme(ThemeChoice),
    FollowSunset(bool),
    Focus(FocusColor),
    Effects(Effects),
    Size(UiSize),
    ReducedMotion(bool),
    WallpaperTab(bool),
    Wallpaper(Option<String>),
    Choose,
    Chosen(Option<Vec<PathBuf>>),
    Found(Vec<PathBuf>),
    Thumb(PathBuf, Option<Image>),
    Sample(Vec<(PathBuf, Image)>),
}

pub const FOCUS_COLORS: [(FocusColor, &str); 4] =
    [(FocusColor::Amber, "Amber"), (FocusColor::Cyan, "Cyan"), (FocusColor::Rose, "Rose"), (FocusColor::Lime, "Lime")];

/// What each tier spends, as the pane says it.
pub fn effects_text(e: Effects) -> (&'static str, &'static str) {
    match e {
        Effects::Full => {
            ("Full", "Translucent, frosted menus, panels and Deskbar, and slower, softer motion (120 ms).")
        }
        Effects::Lean => ("Lean", "Hard shadows and quick motion (80 ms), no frost. The default."),
        Effects::Austere => ("Austere", "No shadows, no motion, nothing see-through: the least to draw."),
    }
}

/// The name a focus colour goes by.
/// What Auto does today when it follows sunset and sunrise: "Day from
/// sunrise (08:15) to sunset (18:57) in Paris, Night after."
pub fn sun_words(s: &aizome_config::Settings) -> String {
    sun_words_at(s, aizome_config::LocalNow::now())
}

pub fn sun_words_at(s: &aizome_config::Settings, now: aizome_config::LocalNow) -> String {
    use aizome_config::{Span, hm};
    match s.auto_day(now) {
        (Span::Between { start, end }, Some(place)) => {
            format!("Day from sunrise ({}) to sunset ({}) {}, Night after.", hm(start), hm(end), place.words())
        }
        (Span::AllDay, Some(place)) => format!("The sun doesn't set {} today: Day all day.", place.words()),
        (Span::Never, Some(place)) => format!("The sun doesn't rise {} today: Night all day.", place.words()),
        _ => "Where you are isn't known (set a time zone in Date & Time), so Day from 07:00 to 19:00.".into(),
    }
}

pub fn focus_name(f: FocusColor) -> &'static str {
    match f {
        FocusColor::Amber => "Kohaku amber",
        FocusColor::Cyan => "Cyan",
        FocusColor::Rose => "Rose",
        FocusColor::Lime => "Lime",
    }
}

impl Appearance {
    /// The thumbnail of the wallpaper at `path`, once it's made, and an id.
    fn thumb_for(&self, path: Option<&Path>) -> Option<(Image, u64)> {
        let path = path?;
        let list = self.wallpapers.as_ref()?;
        let (_, thumb) = list.iter().find(|(p, _)| p == path)?;
        thumb.clone().map(|t| (t, aizome_kit::hash::hash_of(path)))
    }

    fn theme_card(&self, vcx: &ViewCx, choice: ThemeChoice, label: &str) -> Element<Msg> {
        let a = &vcx.settings().appearance;
        let look = match choice {
            ThemeChoice::Night => Look::Theme(ThemeName::Night),
            ThemeChoice::Day => Look::Theme(ThemeName::Day),
            ThemeChoice::HighContrast => Look::Theme(ThemeName::HighContrast),
            ThemeChoice::Auto => Look::Auto,
        };
        let selected = a.theme == choice;
        // High Contrast keeps to its plain desktop, for legibility.
        let walls = if choice == ThemeChoice::HighContrast {
            [None, None]
        } else {
            [self.thumb_for(a.wallpaper_path(false).as_deref()), self.thumb_for(a.wallpaper_path(true).as_deref())]
        };
        let pic = press(preview::with_wallpapers(look, a.focus, a.effects, PREVIEW.0, PREVIEW.1, walls))
            .style(PressStyle::Card)
            .selected(selected)
            .padding([3.0, 3.0])
            .on_press(Msg::Theme(choice))
            .named(label.to_owned());
        let caption = text(label.to_owned())
            .size(12.0)
            .weight(if selected { 650.0 } else { 400.0 })
            .role(if selected { Role::Text } else { Role::Text2 });
        let caption = press(caption).style(PressStyle::Plain).padding([2.0, 6.0]).on_press(Msg::Theme(choice));
        column![pic, caption].spacing(6.0).align(Align::Center).into()
    }

    fn wallpaper_grid(&self, vcx: &ViewCx) -> Element<Msg> {
        let a = &vcx.settings().appearance;
        let current = if self.day_tab { a.wallpaper_day.clone() } else { a.wallpaper_night.clone() };
        let (tw, th) = (wallpaper::THUMB.width, wallpaper::THUMB.height);
        let mut tiles: Vec<Element<Msg>> = Vec::new();
        // The explicit blank choice uses the desktop's own colour.
        let theme = if self.day_tab { ThemeName::Day } else { ThemeName::Night };
        let p = Palette::new(theme, a.focus);
        let plain = || {
            container(crate::widgets::mark::<Msg>(6.0))
                .center()
                .width(tw)
                .height(th)
                .fill(Tone::Color(crate::widgets::c(p.desktop)))
        };
        let default = aizome_config::Appearance::system_wallpaper(self.day_tab);
        let default_content = if let Some(path) = default.as_deref() {
            picture(self.thumb_for(Some(path)).map(|(image, _)| image), aizome_kit::hash::hash_of(path), tw, th)
        } else {
            plain().into()
        };
        tiles.push(self.tile(default_content, "System default", current.is_none(), Msg::Wallpaper(None)));
        tiles.push(self.tile(
            plain().into(),
            "No picture",
            current.as_deref() == Some(""),
            Msg::Wallpaper(Some(String::new())),
        ));
        if let Some(list) = &self.wallpapers {
            for (path, thumb) in list {
                let s = path.display().to_string();
                let id = aizome_kit::hash::hash_of(&s);
                let sel = current.as_deref() == Some(s.as_str());
                tiles.push(self.tile(
                    picture(thumb.clone(), id, tw, th),
                    &wallpaper::name(path),
                    sel,
                    Msg::Wallpaper(Some(s)),
                ));
            }
            // A picture chosen from elsewhere.
            if let Some(c) = &current
                && !c.is_empty()
                && !list.iter().any(|(p, _)| p.display().to_string() == *c)
            {
                let path = PathBuf::from(c);
                let id = aizome_kit::hash::hash_of(c);
                tiles.push(self.tile(
                    picture(None, id, tw, th),
                    &wallpaper::name(&path),
                    true,
                    Msg::Wallpaper(Some(c.clone())),
                ));
            }
        }
        let choose = container(
            column![icon(Icon::FolderOpen).size(20.0).role(Role::Text3), text("Choose…").size(12.0).role(Role::Text3)]
                .spacing(6.0)
                .align(Align::Center),
        )
        .center()
        .width(tw)
        .height(th)
        .fill(Tone::Sunk);
        tiles.push(self.tile(choose.into(), "From a file", false, Msg::Choose));
        let mut rows = column![].spacing(12.0);
        let per_row = 4;
        let mut it = tiles.into_iter().peekable();
        while it.peek().is_some() {
            let mut r = row![].spacing(12.0);
            for _ in 0..per_row {
                if let Some(t) = it.next() {
                    r = r.push(t);
                }
            }
            rows = rows.push(r);
        }
        if self.wallpapers.is_none() {
            rows = rows.push(
                row![progress_indeterminate().width(120.0), text("Looking for pictures…").size(11.0).role(Role::Text3)]
                    .spacing(8.0),
            );
        }
        container(rows).padding([12.0, 12.0]).into()
    }

    fn tile(&self, content: Element<Msg>, label: &str, selected: bool, msg: Msg) -> Element<Msg> {
        let pic = press(content)
            .style(PressStyle::Card)
            .selected(selected)
            .padding([2.0, 2.0])
            .on_press(msg)
            .named(label.to_owned());
        column![
            pic,
            text(label.to_owned())
                .size(11.0)
                .role(if selected { Role::Text } else { Role::Text3 })
                .max_width(wallpaper::THUMB.width)
        ]
        .spacing(5.0)
        .into()
    }

    fn look_for_wallpapers(&mut self, cx: &mut PaneCx<Msg>) {
        if cx.sample() {
            cx.send(Msg::Sample(wallpaper::sample()));
            return;
        }
        let tx = cx.stream();
        let a = &cx.settings().appearance;
        let chosen: Vec<PathBuf> = [
            a.wallpaper_path(false),
            a.wallpaper_path(true),
            aizome_config::Appearance::system_wallpaper(false),
            aizome_config::Appearance::system_wallpaper(true),
        ]
        .into_iter()
        .flatten()
        .collect();
        let _ = std::thread::Builder::new().name("settei-wallpapers".into()).spawn(move || {
            let mut found = wallpaper::find(&wallpaper::dirs(), 48);
            // Pictures chosen from elsewhere, so the previews show them too.
            for c in chosen {
                if !found.contains(&c) && c.is_file() {
                    found.push(c);
                }
            }
            tx.send(Msg::Found(found.clone()));
            for p in found {
                let t = wallpaper::thumbnail(&p);
                if !tx.send(Msg::Thumb(p, t)) {
                    return;
                }
            }
        });
    }
}

impl Pane for Appearance {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "appearance"
    }

    fn title(&self) -> &'static str {
        "Appearance"
    }

    fn icon(&self) -> Icon {
        Icon::Palette
    }

    fn tile(&self) -> Tile {
        Tile::Focus
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["theme", "dark mode", "light mode", "colours", "colors", "look"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("theme", "Theme").keywords(&["night", "day", "dark", "light", "auto", "high contrast"]),
            SettingEntry::new("follow-sunset", "Follow sunset and sunrise").keywords(&[
                "auto",
                "automatic",
                "schedule",
            ]),
            SettingEntry::new("focus", "Focus colour").keywords(&[
                "accent",
                "color",
                "highlight",
                "amber",
                "cyan",
                "rose",
                "lime",
            ]),
            SettingEntry::new("effects", "Effects").keywords(&[
                "shadows",
                "frost",
                "blur",
                "animation",
                "full",
                "lean",
                "austere",
            ]),
            SettingEntry::new("size", "Size").keywords(&["scale", "larger text", "zoom", "small", "large"]),
            SettingEntry::new("reduced-motion", "Reduce motion").keywords(&["animation", "accessibility"]),
            SettingEntry::new("wallpaper", "Wallpaper").keywords(&["background", "desktop picture"]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if self.wallpapers.is_none() {
            self.look_for_wallpapers(cx);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Theme(t) => cx.change(|s| {
                // High Contrast remembers the theme before it, so turning it
                // off in Accessibility goes back there.
                if t == ThemeChoice::HighContrast {
                    super::accessibility::set_high_contrast(&mut s.appearance, true);
                } else {
                    s.appearance.extra.remove("theme_before_contrast");
                    s.appearance.theme = t;
                }
            }),
            Msg::FollowSunset(on) => cx.change(|s| s.appearance.follow_sunset = on),
            Msg::Focus(f) => cx.change(|s| s.appearance.focus = f),
            Msg::Effects(e) => cx.change(|s| s.appearance.effects = e),
            Msg::Size(z) => cx.change(|s| s.appearance.size = z),
            Msg::ReducedMotion(on) => cx.change(|s| s.appearance.reduced_motion = on),
            Msg::WallpaperTab(day) => self.day_tab = day,
            Msg::Wallpaper(w) => {
                // A picture from elsewhere joins the grid, with a thumbnail.
                if let (Some(p), Some(list)) = (w.as_ref().filter(|p| !p.is_empty()), &mut self.wallpapers) {
                    let path = PathBuf::from(p);
                    if !list.iter().any(|(x, _)| *x == path) {
                        list.push((path.clone(), None));
                        if !cx.sample() {
                            cx.spawn(move || {
                                let t = wallpaper::thumbnail(&path);
                                Msg::Thumb(path, t)
                            });
                        }
                    }
                }
                let day = self.day_tab;
                cx.change(|s| {
                    if day {
                        s.appearance.wallpaper_day = w;
                    } else {
                        s.appearance.wallpaper_night = w;
                    }
                });
            }
            Msg::Choose => {
                let pictures = aizome_config::home_dir().join("Pictures");
                let dialog = FileDialog::new()
                    .title("Choose a Wallpaper")
                    .directory(pictures)
                    .filter("Pictures", &["png", "jpg", "jpeg", "webp"]);
                cx.open_panel(dialog, Msg::Chosen);
            }
            Msg::Chosen(Some(paths)) => {
                if let Some(p) = paths.into_iter().next() {
                    cx.send(Msg::Wallpaper(Some(p.display().to_string())));
                }
            }
            Msg::Chosen(None) => {}
            Msg::Found(paths) => self.wallpapers = Some(paths.into_iter().map(|p| (p, None)).collect()),
            Msg::Thumb(path, thumb) => {
                if let Some(list) = &mut self.wallpapers
                    && let Some(slot) = list.iter_mut().find(|(p, _)| *p == path)
                {
                    slot.1 = thumb;
                }
            }
            Msg::Sample(list) => self.wallpapers = Some(list.into_iter().map(|(p, i)| (p, Some(i))).collect()),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let a = &vcx.settings().appearance;
        let themes = row![
            self.theme_card(vcx, ThemeChoice::Night, "Night"),
            self.theme_card(vcx, ThemeChoice::Day, "Day"),
            self.theme_card(vcx, ThemeChoice::Auto, "Auto"),
            self.theme_card(vcx, ThemeChoice::HighContrast, "High Contrast"),
        ]
        .spacing(14.0);
        let mut theme_rows = vec![
            form::whole(container(themes).padding([14.0, 12.0, 12.0, 12.0]).width(Fill).align_x(Align::Center))
                .entry("theme"),
        ];
        let auto = a.theme == ThemeChoice::Auto;
        theme_rows.push(
            form::row("Follow sunset and sunrise", toggle(a.follow_sunset).enabled(auto).on_toggle(Msg::FollowSunset))
                .entry("follow-sunset")
                .help(match (auto, a.follow_sunset) {
                    (false, _) => "With Auto: Day while the sun is up where you are, else from 07:00 to 19:00.".into(),
                    (true, true) => sun_words(vcx.settings()),
                    (true, false) => "Day from 07:00 to 19:00, Night otherwise.".into(),
                }),
        );

        // The swatches in the theme showing now.
        let now = vcx.settings().theme_now();
        let swatches = aizome_kit::widgets::layout::row(
            FOCUS_COLORS
                .iter()
                .map(|(f, name)| {
                    let fill = aizome_theme::focus_color(now, *f);
                    press(swatch(fill, 22.0, a.focus == *f))
                        .style(PressStyle::Plain)
                        .selected(a.focus == *f)
                        .on_press(Msg::Focus(*f))
                        .named(*name)
                        .into()
                })
                .collect(),
        )
        .spacing(6.0);

        let effects = aizome_kit::widgets::layout::row(
            [Effects::Full, Effects::Lean, Effects::Austere]
                .iter()
                .map(|e| {
                    let (title, about) = effects_text(*e);
                    let sel = a.effects == *e;
                    let body = column![
                        row![
                            icon(if sel { Icon::CircleCheck } else { Icon::Circle }).size(14.0).role(if sel {
                                Role::Focus
                            } else {
                                Role::Text3
                            }),
                            text(title).weight(650.0)
                        ]
                        .spacing(6.0)
                        .align(Align::Center),
                        text(about).size(11.0).role(Role::Text3).wrap().line_height(1.25),
                    ]
                    .spacing(6.0)
                    .width(Fill);
                    press(container(body).padding([10.0, 10.0]).width(Fill).height(84.0))
                        .style(PressStyle::Card)
                        .selected(sel)
                        .on_press(Msg::Effects(*e))
                        .named(title)
                        .width(Fill)
                        .into()
                })
                .collect(),
        )
        .spacing(10.0)
        .width(Fill);

        let size_i = match a.size {
            UiSize::Small => 0,
            UiSize::Medium => 1,
            UiSize::Large => 2,
        };
        form::page(vec![
            vcx.section("Theme", theme_rows),
            vcx.section(
                "Colour",
                vec![
                    form::row("Focus colour", swatches)
                        .entry("focus")
                        .detail(focus_name(a.focus))
                        .help("The focused window's tab and rail, focus rings, and whatever is current."),
                ],
            ),
            vcx.section(
                "Effects",
                vec![form::whole(container(effects).padding([12.0, 12.0]).width(Fill)).entry("effects")],
            ),
            vcx.section(
                "Size and Motion",
                vec![
                    form::row(
                        "Size",
                        form::segments(&["Small", "Medium", "Large"], size_i, |i| {
                            Msg::Size([UiSize::Small, UiSize::Medium, UiSize::Large][i.min(2)])
                        }),
                    )
                    .entry("size")
                    .help("Text and controls grow or shrink; pictures stay sharp."),
                    form::row("Reduce motion", toggle(a.reduced_motion).on_toggle(Msg::ReducedMotion))
                        .entry("reduced-motion")
                        .help("Menus, the Deskbar and windows appear where they go instead of sliding there."),
                ],
            ),
            vcx.section(
                "Wallpaper",
                vec![
                    form::row(
                        if self.day_tab { "Wallpaper for Day" } else { "Wallpaper for Night" },
                        form::segments(&["Night", "Day"], usize::from(self.day_tab), |i| Msg::WallpaperTab(i == 1)),
                    )
                    .entry("wallpaper")
                    .help("Each theme keeps a picture of its own; Auto changes both at once."),
                    form::whole(self.wallpaper_grid(vcx)),
                ],
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use aizome_config::{LocalNow, Settings, ThemeChoice};

    #[test]
    fn auto_says_when_the_sun_rises_and_sets() {
        let mut s = Settings::default();
        s.appearance.theme = ThemeChoice::Auto;
        s.appearance.follow_sunset = true;
        s.night_light.latitude = Some(48.8667);
        s.night_light.longitude = Some(2.3333);
        let words = super::sun_words_at(&s, LocalNow::at(12, 0, 305, 60));
        assert!(words.starts_with("Day from sunrise (07:") && words.contains("to sunset (17:"), "{words}");
        assert!(words.ends_with("where you are, Night after."), "{words}");
        // Midsummer in Tromsø.
        s.night_light.latitude = Some(69.65);
        s.night_light.longitude = Some(18.96);
        let words = super::sun_words_at(&s, LocalNow::at(12, 0, 172, 120));
        assert_eq!(words, "The sun doesn't set where you are today: Day all day.");
    }
}
