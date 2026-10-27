//! A notification's `hints` (the specification's "Hints" section), parsed
//! leniently into [`Hints`], and written back into an `a{sv}` for sending.

use super::Urgency;
use crate::bus::{unwrap_variant, value_bool, value_bytes, value_i64, value_str};
use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

/// The hints of a notification, typed. A hint of the wrong type reads as
/// absent; hints the specification doesn't define are kept in `other`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    /// `urgency` (a byte: 0 low, 1 normal, 2 critical).
    pub urgency: Urgency,
    /// `category`: what kind of notification it is (`email.arrived`,
    /// `im.received`, `device.added`…), for the do-not-disturb rules.
    pub category: Option<String>,
    /// `desktop-entry`: the sending app's desktop file name, without
    /// `.desktop` (stripped when an app adds it).
    pub desktop_entry: Option<String>,
    /// `image-data` (or the 1.1 name `image_data`): raw pixels.
    pub image_data: Option<ImageData>,
    /// `image-path` (or the 1.1 name `image_path`): an icon name or a
    /// `file://` URI.
    pub image_path: Option<String>,
    /// `icon_data`: raw pixels, from senders older than the specification's
    /// 1.1; shown after the app icon.
    pub icon_data: Option<ImageData>,
    /// `sound-file`: a sound file to play.
    pub sound_file: Option<String>,
    /// `sound-name`: a sound theme name (`message-new-instant`).
    pub sound_name: Option<String>,
    /// `suppress-sound`: play nothing (the app plays its own).
    pub suppress_sound: bool,
    /// `transient`: show it, but don't keep it in the history.
    pub transient: bool,
    /// `resident`: keep it after an action is invoked, until the person or
    /// the app removes it.
    pub resident: bool,
    /// `x` and `y`: a point on the screen the notification is about.
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// `action-icons`: the action keys are icon names.
    pub action_icons: bool,
    /// Every other hint (vendor ones such as `sender-pid` or `x-kde-…`),
    /// as sent. Values carrying file descriptors are dropped.
    pub other: HashMap<String, OwnedValue>,
}

impl Hints {
    /// Parse the hints a `Notify` call carries. Where the specification
    /// renamed a hint, the current name wins over the old one.
    pub(crate) fn from_dbus(mut hints: HashMap<String, OwnedValue>) -> Hints {
        let mut parsed = Hints::default();
        // The older names first, so the current ones overwrite them.
        for key in ["image_data", "image-data", "image_path", "image-path"] {
            if let Some(value) = hints.remove(key) {
                parsed.set(key, value);
            }
        }
        for (key, value) in hints {
            parsed.set(&key, value);
        }
        parsed
    }

    /// Set the hint `key`: into its typed field if the specification
    /// defines it, else into `other`.
    pub fn set(&mut self, key: &str, value: OwnedValue) {
        let text = || value_str(&value).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let int = || value_i64(&value).and_then(|n| i32::try_from(n).ok());
        let flag = || value_bool(&value).unwrap_or(false);
        match key {
            "urgency" => self.urgency = value_i64(&value).map(Urgency::from_level).unwrap_or_default(),
            "category" => self.category = text(),
            "desktop-entry" => {
                self.desktop_entry = text().map(|d| d.strip_suffix(".desktop").map(str::to_owned).unwrap_or(d))
            }
            "image-data" | "image_data" => self.image_data = ImageData::from_value(&value),
            "image-path" | "image_path" => self.image_path = text(),
            "icon_data" => self.icon_data = ImageData::from_value(&value),
            "sound-file" => self.sound_file = text(),
            "sound-name" => self.sound_name = text(),
            "suppress-sound" => self.suppress_sound = flag(),
            "transient" => self.transient = flag(),
            "resident" => self.resident = flag(),
            "x" => self.x = int(),
            "y" => self.y = int(),
            "action-icons" => self.action_icons = flag(),
            _ if has_fds(&value) => {}
            _ => {
                self.other.insert(key.to_owned(), value);
            }
        }
    }

    /// The point on the screen the notification is about, when both `x`
    /// and `y` are given.
    pub fn position(&self) -> Option<(i32, i32)> {
        Some((self.x?, self.y?))
    }

    /// The hints as an `a{sv}`, for `Notify`. Flags that are off and the
    /// normal urgency are left out, as absent hints mean the same.
    pub(crate) fn to_dbus(&self) -> HashMap<String, Value<'_>> {
        let mut hints: HashMap<String, Value<'_>> =
            self.other.iter().filter_map(|(key, value)| Some((key.clone(), value.try_clone().ok()?.into()))).collect();
        let mut put = |key: &str, value: Value<'static>| {
            hints.insert(key.to_owned(), value);
        };
        if self.urgency != Urgency::Normal {
            put("urgency", Value::U8(self.urgency as u8));
        }
        let texts = [
            ("category", &self.category),
            ("desktop-entry", &self.desktop_entry),
            ("image-path", &self.image_path),
            ("sound-file", &self.sound_file),
            ("sound-name", &self.sound_name),
        ];
        for (key, text) in texts {
            if let Some(text) = text {
                put(key, Value::from(text.clone()));
            }
        }
        if let Some(image) = &self.image_data {
            put("image-data", image.to_value());
        }
        if let Some(image) = &self.icon_data {
            put("icon_data", image.to_value());
        }
        let flags = [
            ("suppress-sound", self.suppress_sound),
            ("transient", self.transient),
            ("resident", self.resident),
            ("action-icons", self.action_icons),
        ];
        for (key, on) in flags {
            if on {
                put(key, Value::Bool(true));
            }
        }
        if let Some(x) = self.x {
            put("x", Value::I32(x));
        }
        if let Some(y) = self.y {
            put("y", Value::I32(y));
        }
        hints
    }
}

/// Whether a value holds a file descriptor anywhere (cloning those can
/// fail, and a hint has no use for one).
fn has_fds(value: &Value<'_>) -> bool {
    match value {
        Value::Fd(_) => true,
        Value::Value(inner) => has_fds(inner),
        Value::Array(array) => array.iter().any(has_fds),
        Value::Structure(structure) => structure.fields().iter().any(has_fds),
        Value::Dict(dict) => dict.iter().any(|(k, v)| has_fds(k) || has_fds(v)),
        _ => false,
    }
}

/// Raw pixels, the `(iiibiiay)` structure of `image-data` and `icon_data`
/// (gdk-pixbuf's layout): rows of `width` pixels, `rowstride` bytes apart,
/// RGB or RGBA with 8 bits per sample.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageData {
    /// Size in pixels.
    pub width: i32,
    pub height: i32,
    /// Bytes from the start of one row to the next.
    pub rowstride: i32,
    /// Whether each pixel has a fourth, alpha byte.
    pub has_alpha: bool,
    /// Always 8 in practice.
    pub bits_per_sample: i32,
    /// 4 with alpha, 3 without.
    pub channels: i32,
    /// The pixels, row by row, in RGB(A) byte order.
    pub data: Vec<u8>,
}

impl ImageData {
    /// Read the structure; `None` unless it has seven fields of usable
    /// types (integers of any width are taken).
    pub(crate) fn from_value(value: &Value<'_>) -> Option<ImageData> {
        let Value::Structure(structure) = unwrap_variant(value) else { return None };
        let [width, height, rowstride, has_alpha, bits, channels, data] = structure.fields() else {
            return None;
        };
        let int = |v: &Value<'_>| value_i64(v).and_then(|n| i32::try_from(n).ok());
        Some(ImageData {
            width: int(width)?,
            height: int(height)?,
            rowstride: int(rowstride)?,
            has_alpha: value_bool(has_alpha)?,
            bits_per_sample: int(bits)?,
            channels: int(channels)?,
            data: value_bytes(data),
        })
    }

    fn to_value(&self) -> Value<'static> {
        Value::from((
            self.width,
            self.height,
            self.rowstride,
            self.has_alpha,
            self.bits_per_sample,
            self.channels,
            self.data.clone(),
        ))
    }

    /// An image from tightly packed RGBA pixels (`width × height × 4`
    /// bytes), for sending.
    pub fn from_rgba(width: i32, height: i32, rgba: Vec<u8>) -> ImageData {
        ImageData {
            width,
            height,
            rowstride: width.saturating_mul(4),
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data: rgba,
        }
    }

    /// Whether the numbers agree with each other and with the data: a
    /// positive size, 8 bits per sample, 3 or 4 channels, rows that fit
    /// their stride, and enough bytes.
    pub fn is_valid(&self) -> bool {
        self.layout().is_some()
    }

    /// (width, height, rowstride, channels) as sizes, when valid.
    fn layout(&self) -> Option<(usize, usize, usize, usize)> {
        let width = usize::try_from(self.width).ok().filter(|&w| w > 0)?;
        let height = usize::try_from(self.height).ok().filter(|&h| h > 0)?;
        let rowstride = usize::try_from(self.rowstride).ok()?;
        let channels = usize::try_from(self.channels).ok().filter(|c| matches!(c, 3 | 4))?;
        if self.bits_per_sample != 8 {
            return None;
        }
        let row = width.checked_mul(channels)?;
        let needed = rowstride.checked_mul(height - 1)?.checked_add(row)?;
        (rowstride >= row && self.data.len() >= needed).then_some((width, height, rowstride, channels))
    }

    /// The pixels as tightly packed RGBA (opaque where there's no alpha),
    /// `width × height × 4` bytes; `None` if the image isn't valid.
    pub fn to_rgba8(&self) -> Option<Vec<u8>> {
        let (width, height, rowstride, channels) = self.layout()?;
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            let start = y * rowstride;
            let row = self.data.get(start..start + width * channels)?;
            for pixel in row.chunks_exact(channels) {
                let alpha = if channels == 4 && self.has_alpha { pixel[3] } else { 255 };
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], alpha]);
            }
        }
        Some(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;
    use zbus::zvariant::Structure;

    fn hints(entries: Vec<(&str, OwnedValue)>) -> Hints {
        Hints::from_dbus(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    fn image(w: i32, h: i32, stride: i32, alpha: bool, channels: i32, data: Vec<u8>) -> OwnedValue {
        owned((w, h, stride, alpha, 8i32, channels, data))
    }

    #[test]
    fn standard_hints_are_typed() {
        let h = hints(vec![
            ("urgency", owned(2u8)),
            ("category", owned("im.received")),
            ("desktop-entry", owned("org.gnome.Fractal.desktop")),
            ("image-path", owned("file:///tmp/a.png")),
            ("sound-name", owned("message-new-instant")),
            ("sound-file", owned("/usr/share/sounds/x.oga")),
            ("suppress-sound", owned(true)),
            ("transient", owned(true)),
            ("resident", owned(false)),
            ("action-icons", owned(true)),
            ("x", owned(100i32)),
            ("y", owned(20i32)),
            ("sender-pid", owned(4242i64)),
            ("x-vendor-thing", owned("kept")),
        ]);
        assert_eq!(h.urgency, Urgency::Critical);
        assert_eq!(h.category.as_deref(), Some("im.received"));
        assert_eq!(h.desktop_entry.as_deref(), Some("org.gnome.Fractal"));
        assert_eq!(h.image_path.as_deref(), Some("file:///tmp/a.png"));
        assert_eq!(h.sound_name.as_deref(), Some("message-new-instant"));
        assert_eq!(h.sound_file.as_deref(), Some("/usr/share/sounds/x.oga"));
        assert!(h.suppress_sound && h.transient && !h.resident && h.action_icons);
        assert_eq!(h.position(), Some((100, 20)));
        assert_eq!(h.other.len(), 2);
        assert_eq!(h.other.get("sender-pid"), Some(&owned(4242i64)));
    }

    #[test]
    fn odd_hints_read_leniently() {
        let h = hints(vec![
            ("urgency", owned(2u32)),
            ("category", owned(7i32)),
            ("transient", owned(1u8)),
            ("resident", owned("yes")),
            ("x", owned(5_000_000_000i64)),
            ("y", owned(3u16)),
            ("image_path", owned("old-name")),
            ("image-path", owned("new-name")),
        ]);
        assert_eq!(h.urgency, Urgency::Critical, "any integer type");
        assert_eq!(h.category, None, "a number isn't a category");
        assert!(h.transient && !h.resident);
        assert_eq!((h.x, h.y), (None, Some(3)));
        assert_eq!(h.position(), None);
        assert_eq!(h.image_path.as_deref(), Some("new-name"), "the current name wins");
        assert!(h.other.is_empty(), "known hints of the wrong type are dropped");

        for (level, urgency) in [(0, Urgency::Low), (1, Urgency::Normal), (2, Urgency::Critical), (9, Urgency::Normal)]
        {
            assert_eq!(hints(vec![("urgency", owned(level as u8))]).urgency, urgency);
        }
        assert_eq!(hints(vec![("urgency", owned("high"))]).urgency, Urgency::Normal);
    }

    #[test]
    fn image_data_in_all_its_names() {
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let h = hints(vec![
            ("image_data", image(1, 1, 4, true, 4, vec![9, 9, 9, 9])),
            ("image-data", image(2, 1, 8, true, 4, pixels.clone())),
            ("icon_data", image(1, 1, 3, false, 3, vec![1, 2, 3])),
        ]);
        let data = h.image_data.unwrap();
        assert_eq!((data.width, data.height, data.rowstride, data.channels), (2, 1, 8, 4));
        assert!(data.is_valid());
        assert_eq!(data.to_rgba8().unwrap(), pixels);
        let icon = h.icon_data.unwrap();
        assert_eq!(icon.to_rgba8().unwrap(), [1, 2, 3, 255]);

        let old = hints(vec![("image_data", image(1, 1, 4, true, 4, vec![9, 9, 9, 9]))]);
        assert_eq!(old.image_data.unwrap().data, [9, 9, 9, 9]);

        // Not a structure, too few fields, a wrong field type.
        let short = owned(Structure::from((1i32, 1i32)));
        let wrong = owned((1i32, 1i32, 4i32, "alpha", 8i32, 4i32, vec![0u8; 4]));
        let bad = hints(vec![("image-data", owned("pixels")), ("icon_data", short), ("image_data", wrong)]);
        assert_eq!((bad.image_data, bad.icon_data), (None, None));
    }

    #[test]
    fn invalid_images_are_refused_not_read_past() {
        let ok = ImageData::from_rgba(2, 2, vec![0; 16]);
        assert!(ok.is_valid());
        // Padded rows: the last row needs no padding.
        let padded = ImageData { rowstride: 8, channels: 3, has_alpha: false, data: vec![7; 14], ..ok.clone() };
        assert_eq!(padded.to_rgba8().unwrap(), [7, 7, 7, 255].repeat(4));
        for broken in [
            ImageData { data: vec![0; 15], ..ok.clone() },
            ImageData { width: 0, ..ok.clone() },
            ImageData { height: -2, ..ok.clone() },
            ImageData { rowstride: 4, ..ok.clone() },
            ImageData { channels: 2, ..ok.clone() },
            ImageData { bits_per_sample: 16, ..ok.clone() },
            ImageData { width: i32::MAX, height: i32::MAX, rowstride: i32::MAX, ..ok.clone() },
        ] {
            assert!(!broken.is_valid(), "{broken:?}");
            assert_eq!(broken.to_rgba8(), None);
        }
    }

    #[test]
    fn hints_survive_a_round_trip() {
        let mut h = Hints {
            urgency: Urgency::Low,
            category: Some("transfer.complete".into()),
            desktop_entry: Some("firefox".into()),
            image_data: Some(ImageData::from_rgba(1, 1, vec![1, 2, 3, 4])),
            image_path: Some("folder-download".into()),
            sound_name: Some("complete".into()),
            transient: true,
            x: Some(-4),
            y: Some(8),
            ..Hints::default()
        };
        h.set("x-aizome-progress", owned(50u32));
        let wire: HashMap<String, OwnedValue> =
            h.to_dbus().into_iter().map(|(k, v)| (k, OwnedValue::try_from(v).unwrap())).collect();
        assert!(!wire.contains_key("resident"), "flags that are off aren't sent");
        assert_eq!(Hints::from_dbus(wire), h);
    }
}
