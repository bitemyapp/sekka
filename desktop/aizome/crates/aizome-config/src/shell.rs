//! The shell's sections, typed: `[menubar]`, `[deskbar]`, `[notifications]`,
//! `[lock]`, `[palette]`, `[desktop]`, `[power]`, `[sound]` and `[recording]`.
//!
//! [`Settings`](crate::Settings) keeps these sections as plain tables (so
//! a load and save never loses a key this version doesn't know); the
//! accessors here read them into the types below, missing keys taking
//! their defaults and a key of the wrong type its default alone, and
//! [`Settings::set_shell_section`](crate::Settings::set_shell_section)
//! writes one back over its table, keeping the keys it doesn't know.

use serde::{Deserialize, Serialize};

/// How the menu bar's clock reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClockFormat {
    /// 21:42.
    #[default]
    #[serde(rename = "24h")]
    H24,
    /// 9:42 PM.
    #[serde(rename = "12h")]
    H12,
}

/// The systray's widgets, left to right.
pub const SYSTRAY_WIDGETS: [&str; 7] = ["cpu", "memory", "network", "sound", "bluetooth", "display", "battery"];

/// `[menubar]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MenubarSettings {
    pub clock: ClockFormat,
    /// The clock shows seconds (it then ticks every second).
    pub clock_seconds: bool,
    /// The weekday and date before the time ("Tue 13").
    pub clock_date: bool,
    /// The systray widgets that show, from [`SYSTRAY_WIDGETS`] (a widget
    /// whose hardware is absent hides anyway).
    pub widgets: Vec<String>,
    /// The battery's percentage beside its glyph.
    pub battery_percent: bool,
}

impl Default for MenubarSettings {
    fn default() -> Self {
        MenubarSettings {
            clock: ClockFormat::H24,
            clock_seconds: false,
            clock_date: false,
            widgets: SYSTRAY_WIDGETS.iter().map(|s| s.to_string()).collect(),
            battery_percent: true,
        }
    }
}

impl MenubarSettings {
    pub fn shows(&self, widget: &str) -> bool {
        self.widgets.iter().any(|w| w == widget)
    }
}

/// `[deskbar]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskbarSettings {
    /// Folded into the bar's `N apps ⌄` control.
    pub collapsed: bool,
    /// Reveal from the bar's apps control; hide after the pointer leaves.
    pub auto_hide: bool,
    /// App ids kept in the Deskbar while not running ("Keep in Deskbar").
    pub pinned: Vec<String>,
    /// An explicitly arranged order, independent of whether apps are pinned.
    pub order: Vec<String>,
    /// The Shelf under the apps.
    pub show_shelf: bool,
    /// The screen edge it stands against, under the bar.
    pub side: DeskbarSide,
    /// Its width in points: 200 to 400 shows everything, narrower is Slim,
    /// and the narrowest just icons (the shell keeps it in range).
    pub width: f64,
}

impl Default for DeskbarSettings {
    fn default() -> Self {
        DeskbarSettings {
            collapsed: false,
            auto_hide: false,
            pinned: Vec::new(),
            order: Vec::new(),
            show_shelf: true,
            side: DeskbarSide::Right,
            width: aizome_theme::metrics::DESKBAR_WIDTH,
        }
    }
}

/// `[deskbar] side`: the edge the Deskbar hangs from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeskbarSide {
    Left,
    /// At the top right, as in C2-Desktop (the default).
    #[default]
    Right,
}

/// The Deskbar's named widths, the same in its grip's menu and in Settei.
pub const DESKBAR_SIZES: [(&str, f64); 5] = [
    ("Icons Only", 48.0),
    ("Slim", 150.0),
    ("Standard", aizome_theme::metrics::DESKBAR_WIDTH),
    ("Wide", 300.0),
    ("Wider", 360.0),
];

/// `[desktop]`: the desktop's icons.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    /// The home folder's icon.
    pub show_home: bool,
    /// Disks and mounted images.
    pub show_disks: bool,
    pub show_trash: bool,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        DesktopSettings { show_home: true, show_disks: true, show_trash: true }
    }
}

/// `[notifications]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// File new notifications without showing them in the bar.
    pub do_not_disturb: bool,
    /// Do Not Disturb every day from this time ("22:00") …
    pub quiet_from: Option<String>,
    /// … until this one ("07:00").
    pub quiet_until: Option<String>,
    /// How long a notification shows in the bar, in seconds.
    pub banner_seconds: u32,
    /// Apps (by app id or name) whose notifications are filed silently.
    pub muted_apps: Vec<String>,
    /// The lock screen shows notifications' text (else only whose they are).
    pub previews_when_locked: bool,
    /// Apps (by app id, desktop entry or name) whose notifications are dropped.
    pub blocked_apps: Vec<String>,
    /// Apps whose notifications make no sound.
    pub silent_apps: Vec<String>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        NotificationSettings {
            do_not_disturb: false,
            quiet_from: None,
            quiet_until: None,
            banner_seconds: 5,
            muted_apps: Vec::new(),
            previews_when_locked: false,
            blocked_apps: Vec::new(),
            silent_apps: Vec::new(),
        }
    }
}

/// Whether `list` names an app, by any of `names` (its app id, desktop
/// entry, name), ignoring case and a `.desktop` ending.
pub fn names_app(list: &[String], names: &[&str]) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(".desktop").to_ascii_lowercase();
    list.iter().any(|l| {
        let l = norm(l);
        !l.is_empty() && names.iter().any(|n| norm(n) == l)
    })
}

impl NotificationSettings {
    /// Whether Do Not Disturb holds at `minutes` past local midnight: set by
    /// hand, or inside the quiet hours (which may wrap past midnight).
    pub fn quiet_at(&self, minutes: u32) -> bool {
        if self.do_not_disturb {
            return true;
        }
        let (Some(from), Some(until)) =
            (self.quiet_from.as_deref().and_then(hhmm), self.quiet_until.as_deref().and_then(hhmm))
        else {
            return false;
        };
        if from <= until { (from..until).contains(&minutes) } else { minutes >= from || minutes < until }
    }
}

/// Minutes past midnight of `"HH:MM"`.
pub fn hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// `[lock]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LockSettings {
    /// Lock before the machine sleeps.
    pub before_sleep: bool,
    /// The user's picture on the lock screen (else their initial).
    pub show_picture: bool,
    /// The PAM service the lock screen authenticates with.
    pub pam_service: String,
    /// The time and date on the lock screen.
    pub show_clock: bool,
    /// Lock when the compositor turns the monitors off for idleness.
    pub after_monitors_off: bool,
}

impl Default for LockSettings {
    fn default() -> Self {
        LockSettings {
            before_sleep: true,
            show_picture: true,
            pam_service: "aizome".into(),
            show_clock: true,
            after_monitors_off: true,
        }
    }
}

/// The palette's scopes, by the names `[palette] scopes` uses.
pub const PALETTE_SCOPES: [&str; 6] = ["commands", "windows", "files", "apps", "settings", "actions"];

/// `[palette]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaletteSettings {
    /// Folders the Files scope indexes ("~" is the home folder).
    pub roots: Vec<String>,
    /// The file index is rebuilt in the background when older than this.
    pub reindex_minutes: u32,
    /// Most rows shown per section under All.
    pub per_section: u32,
    /// What the palette finds, from [`PALETTE_SCOPES`].
    pub scopes: Vec<String>,
    /// Folders the file index skips ("~" is the home folder).
    pub exclude: Vec<String>,
}

impl Default for PaletteSettings {
    fn default() -> Self {
        PaletteSettings {
            roots: vec!["~".into()],
            reindex_minutes: 10,
            per_section: 4,
            scopes: PALETTE_SCOPES.iter().map(|s| s.to_string()).collect(),
            exclude: Vec::new(),
        }
    }
}

impl PaletteSettings {
    /// Whether the palette finds `scope` (a name from [`PALETTE_SCOPES`]).
    pub fn finds(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

/// What the power key does, where the session handles it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerKey {
    /// Ask: sleep, restart or shut down (the shut-down dialog).
    Ask,
    Sleep,
    /// Lock the screen.
    Lock,
    /// Shut down.
    Off,
    Nothing,
}

/// `[power]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerSettings {
    /// What the power key does; unset leaves it to the system (logind's
    /// `HandlePowerKey`, the session asking where that's `ignore`).
    pub power_key: Option<PowerKey>,
}

/// `[sound]`, the shell's part (Settings' Sound and Accessibility panes
/// write it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    /// Flash the screen's edge when the alert sounds (the system bell), so
    /// it's seen as well as heard.
    pub visual_alert: bool,
    /// The alert: a name in the freedesktop sound theme (`bell`).
    pub alert_sound: String,
    /// How loud alerts and notifications' sounds are, 0–1, of the output's
    /// volume (0: silent).
    pub alert_volume: f64,
    /// Play a sound when the volume keys or the Sound panel change the
    /// volume, at the new volume.
    pub volume_feedback: bool,
}

impl Default for SoundSettings {
    fn default() -> SoundSettings {
        SoundSettings { visual_alert: false, alert_sound: "bell".into(), alert_volume: 0.7, volume_feedback: false }
    }
}

impl SoundSettings {
    /// The alert's volume, within 0–1.
    pub fn alert_level(&self) -> f64 {
        if self.alert_volume.is_finite() { self.alert_volume.clamp(0.0, 1.0) } else { 0.7 }
    }
}

/// The container a screen recording is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordingFormat {
    /// H.264 in MP4: plays anywhere, browsers and chat apps included.
    #[default]
    Mp4,
    /// H.264 in Matroska: survives a recorder that dies mid-file.
    Mkv,
}

impl RecordingFormat {
    pub fn extension(self) -> &'static str {
        match self {
            RecordingFormat::Mp4 => "mp4",
            RecordingFormat::Mkv => "mkv",
        }
    }
}

/// How hard the encoder works for the picture (constant quality).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordingQuality {
    Medium,
    High,
    #[default]
    VeryHigh,
    Ultra,
}

/// What records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recorder {
    /// gpu-screen-recorder (KMS capture, hardware encoding), else
    /// wf-recorder through the compositor's screencopy.
    #[default]
    Auto,
    GpuScreenRecorder,
    WfRecorder,
}

/// `[recording]`: screen recordings (the palette's Record Screen and
/// Record Area, ⌥Print, ❖⇧5's Capture menu).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingSettings {
    pub format: RecordingFormat,
    /// Frames per second, 1–240.
    pub framerate: u32,
    pub quality: RecordingQuality,
    /// Draw the pointer into the video.
    pub cursor: bool,
    /// Record what the computer plays (the default output's monitor)
    /// when Record Screen or Record Area doesn't say.
    pub desktop_audio: bool,
    /// Record the default input (the microphone) when not said.
    pub microphone: bool,
    /// Where recordings go; unset, `Recordings` in the Videos folder
    /// (`XDG_VIDEOS_DIR`).
    pub folder: Option<String>,
    pub recorder: Recorder,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        RecordingSettings {
            format: RecordingFormat::Mp4,
            framerate: 60,
            quality: RecordingQuality::VeryHigh,
            cursor: true,
            desktop_audio: false,
            microphone: false,
            folder: None,
            recorder: Recorder::Auto,
        }
    }
}

/// Reads a section's table as `T`, a field of the wrong type taking its
/// default rather than spoiling the rest.
pub(crate) fn read<T: Default + Serialize + for<'de> Deserialize<'de>>(table: Option<&toml::Table>) -> T {
    let Some(table) = table else { return T::default() };
    if let Ok(t) = toml::Value::Table(table.clone()).try_into::<T>() {
        return t;
    }
    // Field by field over the defaults.
    let Ok(toml::Value::Table(mut base)) = toml::Value::try_from(T::default()) else { return T::default() };
    for (k, v) in table {
        let mut trial = base.clone();
        trial.insert(k.clone(), v.clone());
        if toml::Value::Table(trial.clone()).try_into::<T>().is_ok() {
            base = trial;
        } else {
            log::warn!("settings: ignoring {k} = {v}");
        }
    }
    toml::Value::Table(base).try_into::<T>().unwrap_or_default()
}

/// Writes `value` over `table`, keeping the keys it doesn't have.
pub(crate) fn write<T: Serialize>(table: &mut toml::Table, value: &T) {
    // An option set to none leaves the table: TOML has no null, so it
    // isn't written, and the old value would stay (quiet hours turned off
    // came back).
    if let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(value) {
        for (k, v) in fields {
            if v.is_null() {
                table.remove(&k);
            }
        }
    }
    if let Ok(toml::Value::Table(t)) = toml::Value::try_from(value) {
        for (k, v) in t {
            table.insert(k, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    use super::*;

    #[test]
    fn an_option_set_to_none_leaves_the_section() {
        let mut s = Settings::from_toml("[notifications]\nquiet_from = \"22:00\"\nquiet_until = \"07:00\"\nkept = 1\n")
            .unwrap();
        let mut n = s.notification_settings();
        assert_eq!(n.quiet_from.as_deref(), Some("22:00"));
        n.quiet_from = None;
        n.quiet_until = None;
        s.set_shell_section("notifications", &n);
        let n = s.notification_settings();
        assert_eq!((n.quiet_from, n.quiet_until), (None, None));
        assert!(!s.notifications.contains_key("quiet_from"));
        // Keys of others stay.
        assert!(s.notifications.contains_key("kept"));
    }

    #[test]
    fn sections_read_with_defaults() {
        let s =
            Settings::from_toml("[menubar]\nclock = \"12h\"\nfuture = 1\n[deskbar]\npinned = [\"org.aizome.Hako\"]\n")
                .unwrap();
        let m = s.menubar_settings();
        assert_eq!(m.clock, ClockFormat::H12);
        assert!(m.shows("battery"));
        assert_eq!(s.deskbar_settings().pinned, ["org.aizome.Hako"]);
        assert!(!s.deskbar_settings().collapsed);
        assert_eq!(s.notification_settings().banner_seconds, 5);
        assert_eq!(s.lock_settings().pam_service, "aizome");
        assert_eq!(s.palette_settings().reindex_minutes, 10);
    }

    #[test]
    fn recording_reads_with_defaults() {
        let d = Settings::default().recording_settings();
        assert_eq!(
            (d.format, d.framerate, d.quality, d.cursor),
            (RecordingFormat::Mp4, 60, RecordingQuality::VeryHigh, true)
        );
        assert!(!d.desktop_audio && !d.microphone && d.folder.is_none());
        let s = Settings::from_toml(
            "[recording]\nformat = \"mkv\"\nframerate = 30\nquality = \"ultra\"\nmicrophone = true\nrecorder = \"wf-recorder\"\n",
        )
        .unwrap();
        let r = s.recording_settings();
        assert_eq!(r.format.extension(), "mkv");
        assert_eq!((r.framerate, r.quality, r.recorder), (30, RecordingQuality::Ultra, Recorder::WfRecorder));
        assert!(r.microphone && !r.desktop_audio);
    }

    #[test]
    fn a_bad_key_spoils_only_itself() {
        let s = Settings::from_toml("[menubar]\nclock = 7\nclock_seconds = true\n").unwrap();
        let m = s.menubar_settings();
        assert_eq!(m.clock, ClockFormat::H24);
        assert!(m.clock_seconds);
    }

    #[test]
    fn writing_a_section_keeps_unknown_keys() {
        let mut s = Settings::from_toml("[deskbar]\nsparkle = true\n").unwrap();
        let mut d = s.deskbar_settings();
        d.collapsed = true;
        s.set_shell_section("deskbar", &d);
        let again = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert!(again.deskbar_settings().collapsed);
        assert_eq!(again.deskbar.get("sparkle").and_then(|v| v.as_bool()), Some(true));
        // [palette] isn't a field of its own: it lives in the extra keys.
        let mut p = again.palette_settings();
        p.per_section = 6;
        let mut again = again;
        again.set_shell_section("palette", &p);
        let back = Settings::from_toml(&again.to_toml().unwrap()).unwrap();
        assert_eq!(back.palette_settings().per_section, 6);
    }

    #[test]
    fn the_keys_settings_writes_read_typed() {
        // As Settei writes them (apps/settei/src/extra.rs).
        let s = Settings::from_toml(
            "[notifications]\npreviews_when_locked = true\nblocked_apps = [\"firefox.desktop\"]\nsilent_apps = [\"org.aizome.Fusen\"]\n\
             [deskbar]\nshow_shelf = false\n[desktop]\nshow_disks = false\n[lock]\nshow_clock = false\nafter_monitors_off = false\n\
             [power]\npower_key = \"lock\"\n[palette]\nscopes = [\"commands\", \"apps\"]\nexclude = [\"~/Private\"]\n[sound]\nvisual_alert = true\nvolume_step = 5\nalert_sound = \"message\"\nalert_volume = 0.4\nvolume_feedback = true\n",
        )
        .unwrap();
        let n = s.notification_settings();
        assert!(n.previews_when_locked);
        assert!(names_app(&n.blocked_apps, &["Firefox", "firefox"]));
        assert!(names_app(&n.silent_apps, &["org.aizome.Fusen"]));
        assert!(!names_app(&n.silent_apps, &["org.aizome.Sumi", ""]));
        assert!(!s.deskbar_settings().show_shelf);
        let d = s.desktop_settings();
        assert!(d.show_home && !d.show_disks && d.show_trash);
        let l = s.lock_settings();
        assert!(!l.show_clock && !l.after_monitors_off);
        assert_eq!(s.power_settings().power_key, Some(PowerKey::Lock));
        let p = s.palette_settings();
        assert!(p.finds("apps") && !p.finds("files"));
        assert_eq!(p.exclude, ["~/Private"]);
        let snd = s.sound_settings();
        assert!(snd.visual_alert && snd.volume_feedback);
        assert_eq!((snd.alert_sound.as_str(), snd.alert_level()), ("message", 0.4));
        // Left out, the defaults: everything shows, the system has the key.
        let s = Settings::default();
        let snd = s.sound_settings();
        assert_eq!((snd.alert_sound.as_str(), snd.alert_level(), snd.volume_feedback), ("bell", 0.7, false));
        assert!(s.deskbar_settings().show_shelf && s.desktop_settings().show_trash && s.lock_settings().show_clock);
        assert_eq!(s.power_settings().power_key, None);
        assert_eq!(s.palette_settings().scopes.len(), PALETTE_SCOPES.len());
        // Written back, [power] and [sound] stay their own sections.
        let mut s = Settings::from_toml("[sound]\nvolume_step = 5\n").unwrap();
        s.set_shell_section("power", &PowerSettings { power_key: Some(PowerKey::Ask) });
        s.set_shell_section("sound", &SoundSettings { visual_alert: true, ..Default::default() });
        let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
        assert_eq!(back.power_settings().power_key, Some(PowerKey::Ask));
        assert!(back.sound_settings().visual_alert);
        assert_eq!(back.sound.get("volume_step").and_then(|v| v.as_integer()), Some(5));
    }

    #[test]
    fn quiet_hours_wrap_past_midnight() {
        let n = NotificationSettings {
            quiet_from: Some("22:00".into()),
            quiet_until: Some("07:00".into()),
            ..Default::default()
        };
        assert!(n.quiet_at(23 * 60));
        assert!(n.quiet_at(6 * 60 + 59));
        assert!(!n.quiet_at(12 * 60));
        let day = NotificationSettings {
            quiet_from: Some("12:00".into()),
            quiet_until: Some("13:00".into()),
            ..Default::default()
        };
        assert!(day.quiet_at(12 * 60 + 30) && !day.quiet_at(13 * 60));
        assert!(NotificationSettings { do_not_disturb: true, ..Default::default() }.quiet_at(0));
        assert_eq!(hhmm("7:05"), Some(425));
        assert_eq!(hhmm("25:00"), None);
    }
}
