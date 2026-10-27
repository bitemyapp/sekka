//! Desktop notifications, per the freedesktop.org Desktop Notifications
//! Specification (version 1.2, with the `ActivationToken` signal).
//!
//! The shell is the notification server; apps are its clients:
//!
//! - [`server`]: [`NotificationServer`] owns `org.freedesktop.Notifications`
//!   on the session bus and serves the specification's interface at
//!   `/org/freedesktop/Notifications`. It keeps the open notifications
//!   (ids, replacement, expiry timers) and hands each
//!   [`NotificationEvent`] to the shell's callback; the shell shows them
//!   and calls back when the person dismisses one or invokes an action.
//!   Banners, the notification list, sounds and do-not-disturb are the
//!   shell's business.
//! - [`client`]: for apps, [`notify`] sends a [`Notification`] (built with
//!   [`Notification::new`] and its builder methods) and
//!   [`NotificationClient`] also reports what happens to the ones it sent:
//!   actions invoked (with their activation tokens) and closings.
//!
//! The same [`Notification`] type is what apps send and what the server
//! hands the shell, with the hints parsed into [`Hints`].

pub mod client;
mod hints;
pub mod server;

pub use client::{
    ClientEvent, NotificationClient, ServerInformation, capabilities_on, close, close_on, notify, notify_on,
    server_information_on,
};
pub use hints::{Hints, ImageData};
pub use server::{NotificationEvent, NotificationServer, NotificationsState, ServerConfig};

use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use zbus::zvariant::Value;

/// The server's bus name.
pub const BUS_NAME: &str = "org.freedesktop.Notifications";
/// Its object path.
pub const PATH: &str = "/org/freedesktop/Notifications";
/// Its interface.
pub const INTERFACE: &str = "org.freedesktop.Notifications";
/// The action key the specification reserves for activating the
/// notification itself (usually a click on it).
pub const DEFAULT_ACTION: &str = "default";

/// How urgent a notification is (the `urgency` hint).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Urgency {
    /// "Joe signed on", "automatic software update finished".
    Low = 0,
    #[default]
    Normal = 1,
    /// "Your computer is on fire": shown through do-not-disturb, and by
    /// default never expiring.
    Critical = 2,
}

impl Urgency {
    /// From the hint's value; out-of-range values read as normal.
    pub fn from_level(level: i64) -> Urgency {
        match level {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// Why a notification closed (`NotificationClosed`'s reason).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum CloseReason {
    /// Its time ran out.
    Expired = 1,
    /// The person dismissed it.
    Dismissed = 2,
    /// The app closed it (`CloseNotification`).
    Closed = 3,
    /// Anything else (the server went away, say).
    Undefined = 4,
}

impl CloseReason {
    /// From the signal's value; unknown values are [`Undefined`](CloseReason::Undefined).
    pub fn from_code(code: u32) -> CloseReason {
        match code {
            1 => CloseReason::Expired,
            2 => CloseReason::Dismissed,
            3 => CloseReason::Closed,
            _ => CloseReason::Undefined,
        }
    }

    /// The signal's value.
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// How long a notification stays (`expire_timeout`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Timeout {
    /// As long as the server's settings say (-1).
    #[default]
    Default,
    /// Until it is dismissed or closed (0).
    Never,
    /// This long after it is shown.
    After(Duration),
}

impl Timeout {
    /// From `expire_timeout` in milliseconds: -1 (or any negative value)
    /// is the default, 0 never.
    pub fn from_millis(millis: i32) -> Timeout {
        match millis {
            ..0 => Timeout::Default,
            0 => Timeout::Never,
            ms => Timeout::After(Duration::from_millis(ms.unsigned_abs().into())),
        }
    }

    /// As `expire_timeout`: at least 1 ms and at most `i32::MAX` for
    /// [`After`](Timeout::After).
    pub fn as_millis(self) -> i32 {
        match self {
            Timeout::Default => -1,
            Timeout::Never => 0,
            Timeout::After(d) => i32::try_from(d.as_millis()).unwrap_or(i32::MAX).max(1),
        }
    }
}

/// An action button (or, with the [`DEFAULT_ACTION`] key, a click on the
/// notification).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Action {
    /// What the app gets back in `ActionInvoked`; an icon name when the
    /// `action-icons` hint is set.
    pub key: String,
    /// The text to show.
    pub label: String,
}

/// A notification: what an app sends, and what the server hands the shell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notification {
    /// Set by the server: the id `Notify` returned (never 0).
    pub id: u32,
    /// For sending: the id of a notification to replace in place (0 for a
    /// new one). On the server: what the app asked to replace.
    pub replaces_id: u32,
    /// The sending app's name, may be empty.
    pub app_name: String,
    /// The app's icon: an icon name or a `file://` URI, may be empty (see
    /// [`icon`](Notification::icon)).
    pub app_icon: String,
    /// One line about what happened.
    pub summary: String,
    /// More text, possibly with the markup the server advertises (see
    /// [`strip_markup`]).
    pub body: String,
    /// Buttons, and the click action.
    pub actions: Vec<Action>,
    /// Everything else: urgency, category, images, sounds…
    pub hints: Hints,
    /// How long it stays (`expire_timeout`).
    pub timeout: Timeout,
    /// Set by the server: the sender's unique bus name.
    pub sender: Option<String>,
    /// Set by the server: when it arrived (or was last replaced).
    pub received: Option<SystemTime>,
}

impl Notification {
    /// A notification saying `summary`.
    pub fn new(summary: impl Into<String>) -> Notification {
        Notification { summary: summary.into(), ..Notification::default() }
    }

    /// The body text.
    pub fn body(mut self, body: impl Into<String>) -> Notification {
        self.body = body.into();
        self
    }

    /// The app's icon: an icon name or a `file://` URI.
    pub fn icon(mut self, icon: impl Into<String>) -> Notification {
        self.app_icon = icon.into();
        self
    }

    /// The sending app's name.
    pub fn app_name(mut self, name: impl Into<String>) -> Notification {
        self.app_name = name.into();
        self
    }

    /// The `urgency` hint.
    pub fn urgency(mut self, urgency: Urgency) -> Notification {
        self.hints.urgency = urgency;
        self
    }

    /// The `category` hint (`email.arrived`, `im.received`…).
    pub fn category(mut self, category: impl Into<String>) -> Notification {
        self.hints.category = Some(category.into());
        self
    }

    /// The `desktop-entry` hint: the app's desktop file name without
    /// `.desktop`.
    pub fn desktop_entry(mut self, entry: impl Into<String>) -> Notification {
        self.hints.desktop_entry = Some(entry.into());
        self
    }

    /// The `image-path` hint: an icon name or a `file://` URI.
    pub fn image_path(mut self, image: impl Into<String>) -> Notification {
        self.hints.image_path = Some(image.into());
        self
    }

    /// The `image-data` hint: pixels.
    pub fn image(mut self, image: ImageData) -> Notification {
        self.hints.image_data = Some(image);
        self
    }

    /// The `sound-name` hint.
    pub fn sound_name(mut self, sound: impl Into<String>) -> Notification {
        self.hints.sound_name = Some(sound.into());
        self
    }

    /// The `transient` hint: don't keep it in the history.
    pub fn transient(mut self, transient: bool) -> Notification {
        self.hints.transient = transient;
        self
    }

    /// The `resident` hint: keep it after an action is invoked.
    pub fn resident(mut self, resident: bool) -> Notification {
        self.hints.resident = resident;
        self
    }

    /// Add an action button.
    pub fn action(mut self, key: impl Into<String>, label: impl Into<String>) -> Notification {
        self.actions.push(Action { key: key.into(), label: label.into() });
        self
    }

    /// Add the action invoked by a click on the notification.
    pub fn default_action(self, label: impl Into<String>) -> Notification {
        self.action(DEFAULT_ACTION, label)
    }

    /// Any hint; those the specification defines go to their typed field
    /// in [`Hints`].
    pub fn hint<'v>(mut self, key: &str, value: impl Into<Value<'v>>) -> Notification {
        self.hints.set(key, crate::bus::owned(value));
        self
    }

    /// How long it stays (`expire_timeout`).
    pub fn timeout(mut self, timeout: Timeout) -> Notification {
        self.timeout = timeout;
        self
    }

    /// Replace the notification with this id in place.
    pub fn replaces(mut self, id: u32) -> Notification {
        self.replaces_id = id;
        self
    }

    /// The action a click on the notification invokes (the
    /// [`DEFAULT_ACTION`] key), if the app gave one.
    pub fn click_action(&self) -> Option<&Action> {
        self.action_for(DEFAULT_ACTION)
    }

    /// The action with this key.
    pub fn action_for(&self, key: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.key == key)
    }

    /// The actions to show as buttons: all but the default one.
    pub fn buttons(&self) -> impl Iterator<Item = &Action> {
        self.actions.iter().filter(|a| a.key != DEFAULT_ACTION)
    }

    /// The app's icon, from `app_icon`.
    pub fn icon_image(&self) -> Option<Image<'_>> {
        Image::parse(&self.app_icon)
    }

    /// The notification's own image, for a layout that shows it beside the
    /// app's icon: `image-data`, else `image-path`, else `icon_data`.
    pub fn image_beside_icon(&self) -> Option<Image<'_>> {
        let hints = &self.hints;
        hints
            .image_data
            .as_ref()
            .map(Image::Data)
            .or_else(|| hints.image_path.as_deref().and_then(Image::parse))
            .or_else(|| hints.icon_data.as_ref().map(Image::Data))
    }

    /// The one image to show, for a layout with room for one, in the
    /// specification's order: `image-data`, `image-path`, `app_icon`,
    /// `icon_data`.
    pub fn image_to_show(&self) -> Option<Image<'_>> {
        let hints = &self.hints;
        hints
            .image_data
            .as_ref()
            .map(Image::Data)
            .or_else(|| hints.image_path.as_deref().and_then(Image::parse))
            .or_else(|| self.icon_image())
            .or_else(|| hints.icon_data.as_ref().map(Image::Data))
    }

    /// The body without markup, for a plain-text line or a screen reader.
    pub fn plain_body(&self) -> String {
        strip_markup(&self.body)
    }

    /// The `actions` array of `Notify`: keys and labels in turn.
    pub(crate) fn wire_actions(&self) -> Vec<&str> {
        self.actions.iter().flat_map(|a| [a.key.as_str(), a.label.as_str()]).collect()
    }

    /// Actions from `Notify`'s array; a key without a label is dropped.
    pub(crate) fn parse_actions(actions: Vec<String>) -> Vec<Action> {
        let mut actions = actions.into_iter();
        let mut parsed = Vec::new();
        while let (Some(key), Some(label)) = (actions.next(), actions.next()) {
            parsed.push(Action { key, label });
        }
        parsed
    }
}

/// An image a notification refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Image<'a> {
    /// Pixels sent with it.
    Data(&'a ImageData),
    /// A file (from a `file://` URI or an absolute path).
    File(PathBuf),
    /// A name in the icon theme.
    Icon(&'a str),
}

impl<'a> Image<'a> {
    /// `app_icon` or `image-path`: a `file://` URI (the only scheme the
    /// specification allows), an absolute path, or an icon name.
    pub fn parse(reference: &'a str) -> Option<Image<'a>> {
        let reference = reference.trim();
        if reference.is_empty() {
            None
        } else if let Some(rest) = reference.strip_prefix("file://") {
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            path.starts_with('/').then(|| Image::File(percent_decode(path)))
        } else if reference.starts_with('/') {
            Some(Image::File(PathBuf::from(reference)))
        } else if reference.contains("://") {
            None
        } else {
            Some(Image::Icon(reference))
        }
    }
}

/// A URI path with its `%XX` escapes decoded (into bytes, as Linux paths
/// are).
fn percent_decode(path: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// The body's text without markup: the tags the specification allows
/// (`b`, `i`, `u`, `a`, `img`) and any others removed, entities (`&amp;`,
/// `&#233;`…) decoded. A `<` that doesn't start a tag is kept, as apps that
/// don't escape send them.
pub fn strip_markup(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(['<', '&']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let after = &tail[1..];
        if tail.starts_with('<') {
            let tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
            match after.find('>').filter(|_| tag) {
                Some(end) => rest = &after[end + 1..],
                None => {
                    out.push('<');
                    rest = after;
                }
            }
        } else {
            match after.find(';').filter(|&end| end <= 10).and_then(|end| Some((decode_entity(&after[..end])?, end))) {
                Some((c, end)) => {
                    out.push(c);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('&');
                    rest = after;
                }
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeouts_convert_both_ways() {
        assert_eq!(Timeout::from_millis(-1), Timeout::Default);
        assert_eq!(Timeout::from_millis(-500), Timeout::Default);
        assert_eq!(Timeout::from_millis(0), Timeout::Never);
        assert_eq!(Timeout::from_millis(1500), Timeout::After(Duration::from_millis(1500)));
        assert_eq!(Timeout::Default.as_millis(), -1);
        assert_eq!(Timeout::Never.as_millis(), 0);
        assert_eq!(Timeout::After(Duration::from_micros(10)).as_millis(), 1);
        assert_eq!(Timeout::After(Duration::from_secs(u64::MAX)).as_millis(), i32::MAX);
    }

    #[test]
    fn close_reasons_have_their_codes() {
        for code in 1..=4 {
            assert_eq!(CloseReason::from_code(code).code(), code);
        }
        assert_eq!(CloseReason::from_code(0), CloseReason::Undefined);
        assert_eq!(CloseReason::from_code(77), CloseReason::Undefined);
    }

    #[test]
    fn actions_come_in_pairs() {
        let actions =
            Notification::parse_actions(["default", "Open", "reply", "Reply", "dangling"].map(String::from).to_vec());
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[1], Action { key: "reply".into(), label: "Reply".into() });
        let n = Notification::new("x").default_action("Open").action("reply", "Reply");
        assert_eq!(n.wire_actions(), ["default", "Open", "reply", "Reply"]);
        assert_eq!(n.click_action().map(|a| a.label.as_str()), Some("Open"));
        assert_eq!(n.buttons().map(|a| a.key.as_str()).collect::<Vec<_>>(), ["reply"]);
    }

    #[test]
    fn images_are_found_in_the_specified_order() {
        assert_eq!(Image::parse("mail-unread"), Some(Image::Icon("mail-unread")));
        assert_eq!(Image::parse("file:///tmp/a%20b.png"), Some(Image::File("/tmp/a b.png".into())));
        assert_eq!(Image::parse("file://localhost/tmp/c.png"), Some(Image::File("/tmp/c.png".into())));
        assert_eq!(Image::parse("/tmp/d%zz.png"), Some(Image::File("/tmp/d%zz.png".into())));
        assert_eq!(Image::parse("file://%"), None);
        assert_eq!(Image::parse("https://example.com/e.png"), None);
        assert_eq!(Image::parse("  "), None);

        let mut n = Notification::new("x").icon("firefox");
        assert_eq!(n.image_to_show(), Some(Image::Icon("firefox")));
        assert_eq!(n.image_beside_icon(), None);
        n.hints.icon_data = Some(ImageData::from_rgba(1, 1, vec![0; 4]));
        assert_eq!(n.image_to_show(), Some(Image::Icon("firefox")), "icon_data comes after app_icon");
        assert!(matches!(n.image_beside_icon(), Some(Image::Data(_))));
        n = n.image_path("file:///tmp/photo.jpg");
        assert_eq!(n.image_to_show(), Some(Image::File("/tmp/photo.jpg".into())));
        n = n.image(ImageData::from_rgba(1, 1, vec![1; 4]));
        assert_eq!(n.image_to_show(), Some(Image::Data(&ImageData::from_rgba(1, 1, vec![1; 4]))));
        assert_eq!(n.icon_image(), Some(Image::Icon("firefox")));
    }

    #[test]
    fn markup_is_stripped() {
        assert_eq!(strip_markup("<b>Bold</b> and <i>it</i>"), "Bold and it");
        assert_eq!(strip_markup(r#"see <a href="https://x.org/?a=1&amp;b=2">this</a>"#), "see this");
        assert_eq!(strip_markup("Tom &amp; Jerry &lt;3 &#233;&#x1F600;"), "Tom & Jerry <3 é😀");
        assert_eq!(strip_markup("a < b && c > d"), "a < b && c > d");
        assert_eq!(strip_markup("x &bogus; y & z; <img src=\"a.png\" alt=\"pic\"/>"), "x &bogus; y & z; ");
        assert_eq!(strip_markup("unclosed <b"), "unclosed <b");
        assert_eq!(strip_markup("&#xFFFFFFFF; &#; &"), "&#xFFFFFFFF; &#; &");
        assert_eq!(strip_markup("日本 <u>語</u>"), "日本 語");
    }
}
