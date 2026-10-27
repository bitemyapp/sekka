//! Notifications: Do Not Disturb (now, or every day between two times),
//! how long a notification stays in the menu bar, previews on the lock
//! screen, and each app's: allowed, shown in the bar, with sound.
//! `[notifications]`, which the shell follows.

use aizome_config::NotificationSettings;
use aizome_kit::prelude::*;
use aizome_kit::row;
use aizome_sys::apps::AppIndex;

use crate::extra;
use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};

#[derive(Default)]
pub struct Notifications {
    /// Apps that send notifications, as (app id, name).
    apps: Option<Vec<(String, String)>>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Apps(Vec<(String, String)>),
    Dnd(bool),
    Scheduled(bool),
    From(usize),
    Until(usize),
    Seconds(usize),
    Previews(bool),
    Allow(String, bool),
    Banners(String, bool),
    Sounds(String, bool),
}

/// Every half hour of the day, "00:00" to "23:30".
fn half_hours() -> Vec<String> {
    (0..48).map(|i| format!("{:02}:{:02}", i / 2, (i % 2) * 30)).collect()
}

fn hh_index(s: Option<&str>, default: usize) -> usize {
    s.and_then(aizome_config::shell::hhmm).map_or(default, |m| ((m / 30) as usize).min(47))
}

pub const SECONDS: [u32; 5] = [3, 5, 8, 12, 20];

/// Apps that say they send notifications, and Aizome's own.
pub fn senders(index: &AppIndex) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = index
        .apps()
        .into_iter()
        .filter(|a| a.id.starts_with("org.aizome.") || a.get_bool("X-GNOME-UsesNotifications").unwrap_or(false))
        .map(|a| (a.app_id().to_owned(), a.name.clone()))
        .collect();
    v.sort_by_key(|(_, n)| n.to_lowercase());
    v.dedup();
    v
}

fn sample_apps() -> Vec<(String, String)> {
    [
        ("org.mozilla.firefox", "Firefox"),
        ("org.aizome.Koyomi", "Koyomi"),
        ("org.signal.Signal", "Signal"),
        ("org.mozilla.Thunderbird", "Thunderbird"),
        ("org.aizome.Tokei", "Tokei"),
        ("org.aizome.Yukimi", "Yukimi"),
    ]
    .iter()
    .map(|(a, n)| ((*a).to_owned(), (*n).to_owned()))
    .collect()
}

fn toggle_in(list: &mut Vec<String>, id: &str, present: bool) {
    list.retain(|x| x != id);
    if present {
        list.push(id.to_owned());
    }
}

impl Pane for Notifications {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "notifications"
    }

    fn title(&self) -> &'static str {
        "Notifications"
    }

    fn icon(&self) -> Icon {
        Icon::Bell
    }

    fn tile(&self) -> Tile {
        Tile::Red
    }

    fn group(&self) -> Group {
        Group::Alerts
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["alerts", "banners", "focus", "quiet"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("dnd", "Do Not Disturb").keywords(&["quiet", "focus", "silence", "dnd"]),
            SettingEntry::new("schedule", "Quiet hours").keywords(&["schedule", "night", "do not disturb"]),
            SettingEntry::new("banner-time", "How long notifications show")
                .keywords(&["banner", "duration", "seconds"]),
            SettingEntry::new("previews", "Show previews when locked").keywords(&["lock screen", "privacy"]),
            SettingEntry::new("apps", "Notifications by app").keywords(&["per app", "mute", "sounds", "allow"]),
        ]
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if self.apps.is_some() {
            return;
        }
        if cx.sample() {
            self.apps = Some(sample_apps());
        } else {
            cx.spawn(|| Msg::Apps(senders(&AppIndex::load())));
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        let shell = |cx: &mut PaneCx<Msg>, f: Box<dyn FnOnce(&mut NotificationSettings)>| {
            cx.change(move |s| {
                let mut n = s.notification_settings();
                f(&mut n);
                s.set_shell_section("notifications", &n);
            });
        };
        match msg {
            Msg::Apps(a) => {
                // Apps already in a list stay listed even if uninstalled.
                let mut a = a;
                let n = cx.settings().notification_settings();
                for id in n.muted_apps.iter() {
                    if !a.iter().any(|(x, _)| x == id) {
                        a.push((id.clone(), id.clone()));
                    }
                }
                self.apps = Some(a);
            }
            Msg::Dnd(on) => shell(cx, Box::new(move |n| n.do_not_disturb = on)),
            Msg::Scheduled(on) => shell(
                cx,
                Box::new(move |n| {
                    if on {
                        n.quiet_from = Some("22:00".into());
                        n.quiet_until = Some("07:00".into());
                    } else {
                        n.quiet_from = None;
                        n.quiet_until = None;
                    }
                }),
            ),
            Msg::From(i) => shell(cx, Box::new(move |n| n.quiet_from = half_hours().get(i).cloned())),
            Msg::Until(i) => shell(cx, Box::new(move |n| n.quiet_until = half_hours().get(i).cloned())),
            Msg::Seconds(i) => shell(cx, Box::new(move |n| n.banner_seconds = SECONDS[i.min(SECONDS.len() - 1)])),
            Msg::Previews(on) => cx.change(|s| extra::set(&mut s.notifications, "previews_when_locked", on)),
            Msg::Allow(id, on) => cx.change(|s| {
                let mut l = extra::get_list(&s.notifications, "blocked_apps").unwrap_or_default();
                toggle_in(&mut l, &id, !on);
                extra::set_list(&mut s.notifications, "blocked_apps", &l);
            }),
            Msg::Banners(id, on) => shell(cx, Box::new(move |n| toggle_in(&mut n.muted_apps, &id, !on))),
            Msg::Sounds(id, on) => cx.change(|s| {
                let mut l = extra::get_list(&s.notifications, "silent_apps").unwrap_or_default();
                toggle_in(&mut l, &id, !on);
                extra::set_list(&mut s.notifications, "silent_apps", &l);
            }),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = vcx.settings();
        let n = s.notification_settings();
        let scheduled = n.quiet_from.is_some() && n.quiet_until.is_some();
        let hours = half_hours();
        let mut dnd = vec![
            form::row("Do Not Disturb", toggle(n.do_not_disturb).on_toggle(Msg::Dnd))
                .entry("dnd")
                .help("Notifications are filed under their app in the Deskbar without showing in the menu bar. Critical ones still show."),
            form::row("Every day at set times", toggle(scheduled).on_toggle(Msg::Scheduled)).entry("schedule"),
        ];
        if scheduled {
            let times = row![
                text("From").role(Role::Text3),
                dropdown(hours.clone(), hh_index(n.quiet_from.as_deref(), 44)).on_select(Msg::From).compact(),
                text("until").role(Role::Text3),
                dropdown(hours, hh_index(n.quiet_until.as_deref(), 14)).on_select(Msg::Until).compact(),
            ]
            .spacing(8.0)
            .align(Align::Center);
            dnd.push(form::row("Quiet hours", times).help("Past midnight is fine: 22:00 until 07:00 is the night."));
        }
        let secs_i = SECONDS.iter().position(|v| *v == n.banner_seconds).unwrap_or(1);
        let blocked = extra::get_list(&s.notifications, "blocked_apps").unwrap_or_default();
        let silent = extra::get_list(&s.notifications, "silent_apps").unwrap_or_default();
        let mut app_rows: Vec<form::Row<Msg>> = match &self.apps {
            None => vec![form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET]))],
            Some(apps) if apps.is_empty() => vec![form::whole(
                container(text("No app has said it sends notifications.").role(Role::Text3))
                    .padding([12.0, form::INSET]),
            )],
            Some(apps) => apps
                .iter()
                .map(|(id, name)| {
                    let allowed = !blocked.contains(id);
                    let banners = !n.muted_apps.contains(id);
                    let sounds = !silent.contains(id);
                    let (a, b, c) = (id.clone(), id.clone(), id.clone());
                    let controls = row![
                        checkbox("Banners", banners).enabled(allowed).on_toggle(move |on| Msg::Banners(b.clone(), on)),
                        checkbox("Sounds", sounds).enabled(allowed).on_toggle(move |on| Msg::Sounds(c.clone(), on)),
                        toggle(allowed).on_toggle(move |on| Msg::Allow(a.clone(), on)),
                    ]
                    .spacing(14.0)
                    .align(Align::Center);
                    let detail = if !allowed {
                        "Off"
                    } else if !banners {
                        "Filed quietly in the Deskbar"
                    } else {
                        "In the menu bar, then the Deskbar"
                    };
                    form::row(name.clone(), controls)
                        .leading(icon(crate::widgets::app_glyph(id)).size(16.0).role(Role::Text2))
                        .detail(detail)
                        .compact()
                })
                .collect(),
        };
        if !app_rows.is_empty() {
            let first = app_rows.remove(0);
            app_rows.insert(0, first.entry("apps"));
        }
        form::page(vec![
            vcx.section("Do Not Disturb", dnd),
            vcx.section(
                "Showing",
                vec![
                    form::row(
                        "Show in the menu bar for",
                        form::segments(&["3 s", "5 s", "8 s", "12 s", "20 s"], secs_i, Msg::Seconds),
                    )
                    .entry("banner-time")
                    .help("A notification takes the menu bar's line, then files itself under its app."),
                    form::row(
                        "Show previews when locked",
                        toggle(extra::get_bool(&s.notifications, "previews_when_locked", false))
                            .on_toggle(Msg::Previews),
                    )
                    .entry("previews")
                    .help("Off, the lock screen says only which app has news."),
                ],
            ),
            vcx.section("Apps", app_rows),
        ])
    }
}
