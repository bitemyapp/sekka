// SPDX-License-Identifier: MIT OR Apache-2.0
//! Aizome's display settings, for the displays Tatami has none of its own
//! for: a display turned to portrait, scaled or arranged in Aizome's
//! Settings comes up the same way in Tatami.
//!
//! Aizome keeps them in $XDG_CONFIG_HOME/aizome/settings.toml as
//! `[[outputs]]` entries, each matching a display by make, model and serial
//! from its EDID, or by connector (aizome-config's `OutputConfig::matches`).
//! Hyprland names makes otherwise ("Red Hat, Inc." for the EDID's "RHT"),
//! so displays are recognized here as Aizome recognizes them, from the
//! kernel's copy of each connector's EDID, and their rules name the
//! connector.
//!
//! The rules go to $XDG_RUNTIME_DIR/tatami/aizome-displays.lua, written as
//! the session starts, before Hyprland does, and again before each reload
//! and when displays come or go. The configuration loads that file before
//! Tatami's own display settings (displays.rs), and of the rules naming a
//! display the last loaded wins: once a display is set up in the Displays
//! window or with Super+/, Tatami's settings are the ones it keeps.
use crate::{hypr, window};
use std::{fs, path::Path, path::PathBuf};

const FILE: &str = "aizome-displays.lua";

/// A connected display: its connector and what its EDID says it is.
#[derive(Clone, Debug, PartialEq)]
struct Display {
    connector: String,
    make: String,
    model: String,
    serial: String,
}

/// The manufacturer's PNP id, the monitor name and the serial, as Aizome
/// reads them (parse_edid in Aizome's backend/udev.rs): the name descriptor,
/// else the product code in hex; the serial descriptor, else the serial
/// number in decimal.
fn parse_edid(e: &[u8]) -> Option<(String, String, String)> {
    if e.len() < 128 || e[0..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return None;
    }
    let id = u16::from_be_bytes([e[8], e[9]]);
    let letter = |v: u16| (b'A' + ((v & 0x1f) as u8).saturating_sub(1)) as char;
    let make: String = [letter(id >> 10), letter(id >> 5), letter(id)]
        .iter()
        .collect();
    let product = u16::from_le_bytes([e[10], e[11]]);
    let serial_number = u32::from_le_bytes([e[12], e[13], e[14], e[15]]);
    let mut model = format!("0x{product:04X}");
    let mut serial = if serial_number != 0 {
        serial_number.to_string()
    } else {
        String::new()
    };
    for d in 0..4 {
        let b = &e[54 + d * 18..72 + d * 18];
        if b[0] == 0 && b[1] == 0 {
            let text: String = b[5..18]
                .iter()
                .take_while(|c| **c != b'\n')
                .map(|c| *c as char)
                .collect::<String>()
                .trim()
                .to_string();
            match b[3] {
                0xfc if !text.is_empty() => model = text,
                0xff if !text.is_empty() => serial = text,
                _ => {}
            }
        }
    }
    Some((make, model, serial))
}

/// The connected displays (/sys/class/drm/card0-DP-1/status and edid), by
/// connector: what Hyprland finds when it starts. One the EDID does not
/// describe is "Unknown", as in Aizome.
fn connected(drm: &Path) -> Vec<Display> {
    let mut displays: Vec<Display> = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            fs::read_to_string(entry.path().join("status"))
                .is_ok_and(|status| status.trim() == "connected")
        })
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (card, connector) = name.split_once('-')?;
            let card = card.strip_prefix("card")?;
            if card.is_empty() || !card.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let (make, model, serial) = fs::read(entry.path().join("edid"))
                .ok()
                .and_then(|edid| parse_edid(&edid))
                .unwrap_or_else(|| ("Unknown".into(), "Unknown".into(), String::new()));
            Some(Display {
                connector: connector.to_owned(),
                make,
                model,
                serial,
            })
        })
        .filter(|display| hypr::safe_name(&display.connector))
        .collect();
    displays.sort_by(|a, b| a.connector.cmp(&b.connector));
    displays.dedup_by(|a, b| a.connector == b.connector);
    displays
}

/// One display's settings in Aizome: those Tatami follows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Followed {
    connector: String,
    /// "WIDTHxHEIGHT" or "WIDTHxHEIGHT@HZ".
    mode: Option<String>,
    scale: Option<f64>,
    position: Option<(i64, i64)>,
    /// wl_output transform, 0-7.
    transform: Option<i64>,
    /// The connector of the display this one shows.
    pub mirror: Option<String>,
}

/// An `[[outputs]]` entry: which displays it is for and their settings.
struct Entry {
    name: Option<String>,
    make: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    enabled: Option<bool>,
    settings: Followed,
}

/// Aizome's names for wl_output transforms, as the numbers Hyprland takes.
fn transform(name: &str) -> Option<i64> {
    let names = [
        "normal",
        "90",
        "180",
        "270",
        "flipped",
        "flipped-90",
        "flipped-180",
        "flipped-270",
    ];
    names
        .iter()
        .position(|n| *n == name)
        .map(|index| index as i64)
}

/// "WxH" or "WxH@R" as Aizome reads it (aizome-config's parse_mode): spaces
/// and an "Hz" allowed, either case of x.
fn mode(text: &str) -> Option<String> {
    let (size, refresh) = match text.split_once('@') {
        Some((size, refresh)) => (size, Some(refresh)),
        None => (text, None),
    };
    let (width, height) = size.trim().split_once(['x', 'X'])?;
    let width = width.trim().parse::<u32>().ok().filter(|w| *w > 0)?;
    let height = height.trim().parse::<u32>().ok().filter(|h| *h > 0)?;
    match refresh {
        None => Some(format!("{width}x{height}")),
        Some(refresh) => {
            let refresh = refresh.trim().trim_end_matches("Hz").trim();
            let hz = refresh.parse::<f64>().ok()?;
            (hz > 0.0 && hz < 1000.0).then(|| format!("{width}x{height}@{hz}"))
        }
    }
}

/// The `[[outputs]]` of Aizome's settings, in order. A value Aizome would
/// not take is left out: that setting keeps Tatami's default.
fn entries(text: &str) -> Vec<Entry> {
    let Ok(table) = text.parse::<toml::Table>() else {
        return vec![];
    };
    let Some(outputs) = table.get("outputs").and_then(toml::Value::as_array) else {
        return vec![];
    };
    outputs
        .iter()
        .filter_map(toml::Value::as_table)
        .map(|entry| {
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            };
            let position = entry
                .get("position")
                .and_then(toml::Value::as_array)
                .and_then(|xy| match xy.as_slice() {
                    [x, y] => Some((x.as_integer()?, y.as_integer()?)),
                    _ => None,
                });
            let scale = entry.get("scale").and_then(|scale| {
                scale
                    .as_float()
                    .or_else(|| scale.as_integer().map(|n| n as f64))
            });
            Entry {
                make: text("make"),
                model: text("model"),
                serial: text("serial"),
                enabled: entry.get("enabled").and_then(toml::Value::as_bool),
                settings: Followed {
                    connector: String::new(),
                    mode: text("mode").as_deref().and_then(mode),
                    scale: scale.filter(|scale| (0.25..=10.0).contains(scale)),
                    position,
                    transform: text("transform").as_deref().and_then(transform),
                    mirror: text("mirror").filter(|mirror| hypr::safe_name(mirror)),
                },
                name: text("name"),
            }
        })
        .collect()
}

impl Entry {
    /// Whether the entry is for this display, as Aizome decides: its
    /// connector, or the make, model and serial it gives (one left out
    /// matches any).
    fn matches(&self, display: &Display) -> bool {
        let identified = self.make.is_some() || self.model.is_some() || self.serial.is_some();
        if let Some(name) = &self.name {
            if *name == display.connector {
                return true;
            }
            if !identified {
                return false;
            }
        }
        let same = |want: &Option<String>, have: &str| want.as_deref().is_none_or(|w| w == have);
        identified
            && same(&self.make, &display.make)
            && same(&self.model, &display.model)
            && same(&self.serial, &display.serial)
    }
}

/// What Aizome has for each connected display: the first entry matching
/// it, as in Aizome. A display Aizome keeps off is placed beside the
/// others, since Aizome may have arranged others where it was.
fn followed(settings: &str, displays: &[Display]) -> Vec<Followed> {
    let entries = entries(settings);
    displays
        .iter()
        .filter_map(|display| {
            let entry = entries.iter().find(|entry| entry.matches(display))?;
            let mut followed = Followed {
                connector: display.connector.clone(),
                ..entry.settings.clone()
            };
            if entry.enabled == Some(false) {
                followed.position = None;
            }
            if followed.mirror.as_deref() == Some(display.connector.as_str()) {
                followed.mirror = None;
            }
            Some(followed)
        })
        .collect()
}

impl Followed {
    /// The monitor rule. What Aizome leaves unset is as the configuration's
    /// rule for every monitor has it.
    pub fn rule(&self) -> String {
        let mode = self.mode.as_deref().unwrap_or("preferred");
        let size = mode
            .split('@')
            .next()
            .and_then(|size| size.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<i64>().ok()?, h.parse::<i64>().ok()?)));
        let scale = match (self.scale, size) {
            // Hyprland takes only scales giving whole logical pixels, and
            // warns as it rounds another.
            (Some(scale), Some((w, h))) => format!("{}", window::clean_scale(scale, w, h)),
            (Some(scale), None) => format!("{scale}"),
            (None, _) => "\"auto\"".to_owned(),
        };
        let position = self
            .position
            .map(|(x, y)| format!("{x}x{y}"))
            .unwrap_or_else(|| "auto".to_owned());
        format!(
            "hl.monitor({{ output = {}, mode = \"{mode}\", position = \"{position}\", scale = {scale}, transform = {}, disabled = false, mirror = {} }})",
            window::lua_string(&self.connector),
            self.transform.unwrap_or(0),
            window::lua_string(self.mirror.as_deref().unwrap_or("")),
        )
    }
}

/// Aizome's settings file: $XDG_CONFIG_HOME/aizome/settings.toml, an unset
/// or empty variable meaning ~/.config.
fn settings_file() -> PathBuf {
    let config = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|dir| !dir.is_empty())
        .unwrap_or_else(|| format!("{}/.config", crate::util::home()));
    Path::new(&config).join("aizome/settings.toml")
}

/// What Aizome has for the displays connected now.
fn now() -> Vec<Followed> {
    let settings = fs::read_to_string(settings_file()).unwrap_or_default();
    followed(&settings, &connected(Path::new("/sys/class/drm")))
}

/// What Aizome has for one connected display.
pub fn for_connector(connector: &str) -> Option<Followed> {
    now()
        .into_iter()
        .find(|followed| followed.connector == connector)
}

/// The file: a rule for each display Aizome has settings for.
fn lua(followed: &[Followed]) -> String {
    let mut lua = String::from(
        "-- Aizome's settings for the connected displays, written by the tatami\n\
         -- helper (tool/src/aizome_displays.rs). Tatami's own display settings\n\
         -- load after these and win.\n",
    );
    for display in followed {
        lua.push_str(&display.rule());
        lua.push('\n');
    }
    lua
}

/// Write the rules for the displays connected now. Whether they changed:
/// none, with no file, is no change.
pub fn write() -> bool {
    let Some(dir) = window::session_dir() else {
        return false;
    };
    let path = dir.join(FILE);
    let rules = lua(&now());
    if fs::read_to_string(&path).unwrap_or_else(|_| lua(&[])) == rules {
        return false;
    }
    let partial = path.with_extension("partial");
    fs::create_dir_all(&dir).is_ok()
        && fs::write(&partial, rules).is_ok()
        && fs::rename(&partial, &path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An EDID as QEMU's virtio GPU makes one: vendor RHT, a monitor name,
    /// and a serial number only when given.
    fn edid(name: &str, serial: u32, serial_text: Option<&str>) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[0..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        let id: u16 = ((b'R' - b'A' + 1) as u16) << 10
            | ((b'H' - b'A' + 1) as u16) << 5
            | (b'T' - b'A' + 1) as u16;
        e[8..10].copy_from_slice(&id.to_be_bytes());
        e[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
        e[12..16].copy_from_slice(&serial.to_le_bytes());
        let mut descriptor = |d: usize, tag: u8, text: &str| {
            let at = 54 + d * 18;
            e[at..at + 5].copy_from_slice(&[0, 0, 0, tag, 0]);
            let mut field = [b' '; 13];
            field[..text.len()].copy_from_slice(text.as_bytes());
            if text.len() < 13 {
                field[text.len()] = b'\n';
            }
            e[at + 5..at + 18].copy_from_slice(&field);
        };
        if !name.is_empty() {
            descriptor(0, 0xfc, name);
        }
        if let Some(text) = serial_text {
            descriptor(1, 0xff, text);
        }
        e
    }

    fn display(connector: &str, make: &str, model: &str, serial: &str) -> Display {
        Display {
            connector: connector.into(),
            make: make.into(),
            model: model.into(),
            serial: serial.into(),
        }
    }

    #[test]
    fn displays_are_named_as_aizome_names_them() {
        let names = |e: &[u8]| parse_edid(e).unwrap();
        assert_eq!(
            names(&edid("Tall", 0, None)),
            ("RHT".into(), "Tall".into(), String::new())
        );
        // Without a name descriptor, the product code; the serial number in
        // decimal, unless a serial descriptor says otherwise.
        assert_eq!(
            names(&edid("", 123456, None)),
            ("RHT".into(), "0x1234".into(), "123456".into())
        );
        assert_eq!(
            names(&edid("Tall", 123456, Some("SN0042"))),
            ("RHT".into(), "Tall".into(), "SN0042".into())
        );
        assert!(parse_edid(&[0; 10]).is_none());
        assert!(parse_edid(&[0; 128]).is_none());
    }

    #[test]
    fn connected_displays_are_read_from_the_kernel() {
        let dir = std::env::temp_dir().join(format!("tatami-aizome-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (entry, status, edid) in [
            ("card1-DP-1", "connected", Some(edid("Tall", 0, None))),
            ("card1-HDMI-A-1", "connected", None),
            ("card1-DP-2", "disconnected", Some(edid("Wide", 0, None))),
            ("card1", "", None),
            ("renderD128", "", None),
        ] {
            fs::create_dir_all(dir.join(entry)).unwrap();
            if !status.is_empty() {
                fs::write(dir.join(entry).join("status"), format!("{status}\n")).unwrap();
            }
            if let Some(edid) = edid {
                fs::write(dir.join(entry).join("edid"), edid).unwrap();
            }
        }
        assert_eq!(
            connected(&dir),
            vec![
                display("DP-1", "RHT", "Tall", ""),
                display("HDMI-A-1", "Unknown", "Unknown", ""),
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// What Settings writes for two displays: the second turned to
    /// portrait, scaled, and left of the first, which is the main display.
    const SETTINGS: &str = r#"
[appearance]
cursor_size = 32

[[outputs]]
make = "RHT"
model = "Wide"
mode = "1280x800@75"
scale = 1.0
position = [640, 0]
transform = "normal"
enabled = true
primary = true

[[outputs]]
make = "RHT"
model = "Tall"
mode = "1280x800@75"
scale = 1.25
position = [0, 0]
transform = "90"
enabled = true
"#;

    #[test]
    fn displays_follow_their_settings_in_aizome() {
        let displays = [
            display("Virtual-1", "RHT", "Wide", ""),
            display("Virtual-2", "RHT", "Tall", ""),
            display("DP-3", "DEL", "U2720Q", "ABC"),
        ];
        let rules: Vec<String> = followed(SETTINGS, &displays)
            .iter()
            .map(Followed::rule)
            .collect();
        assert_eq!(
            rules,
            [
                "hl.monitor({ output = \"Virtual-1\", mode = \"1280x800@75\", position = \"640x0\", scale = 1, transform = 0, disabled = false, mirror = \"\" })",
                "hl.monitor({ output = \"Virtual-2\", mode = \"1280x800@75\", position = \"0x0\", scale = 1.25, transform = 1, disabled = false, mirror = \"\" })",
            ]
        );
    }

    #[test]
    fn entries_match_displays_as_in_aizome() {
        let tall = display("DP-2", "RHT", "Tall", "7");
        let entry = |toml: &str| entries(&format!("[[outputs]]\n{toml}\n")).remove(0);
        // By connector.
        assert!(entry("name = \"DP-2\"").matches(&tall));
        assert!(!entry("name = \"DP-1\"").matches(&tall));
        // By what is given of make, model and serial.
        assert!(entry("make = \"RHT\"\nmodel = \"Tall\"").matches(&tall));
        assert!(entry("make = \"RHT\"\nmodel = \"Tall\"\nserial = \"7\"").matches(&tall));
        assert!(!entry("make = \"RHT\"\nmodel = \"Tall\"\nserial = \"8\"").matches(&tall));
        assert!(!entry("make = \"Red Hat, Inc.\"\nmodel = \"Tall\"").matches(&tall));
        // A connector and a description: either.
        assert!(entry("name = \"DP-1\"\nmodel = \"Tall\"").matches(&tall));
        // Nothing to match by.
        assert!(!entry("transform = \"90\"").matches(&tall));
        // The first entry that matches.
        let settings = "[[outputs]]\nmodel = \"Tall\"\ntransform = \"270\"\n\
                        [[outputs]]\nname = \"DP-2\"\ntransform = \"90\"\n";
        assert_eq!(followed(settings, &[tall])[0].transform, Some(3));
    }

    #[test]
    fn what_aizome_leaves_unset_or_cannot_take_keeps_tatamis_defaults() {
        let tall = [display("DP-2", "RHT", "Tall", "")];
        let rule = |toml: &str| {
            followed(&format!("[[outputs]]\nmodel = \"Tall\"\n{toml}\n"), &tall)
                .first()
                .map(Followed::rule)
        };
        assert_eq!(
            rule("transform = \"flipped-270\"").unwrap(),
            "hl.monitor({ output = \"DP-2\", mode = \"preferred\", position = \"auto\", scale = \"auto\", transform = 7, disabled = false, mirror = \"\" })"
        );
        assert_eq!(
            rule("mode = \"2560 X 1440 @ 59.951Hz\"\nscale = 2\nposition = [-1440, 120]\ntransform = \"sideways\"").unwrap(),
            "hl.monitor({ output = \"DP-2\", mode = \"2560x1440@59.951\", position = \"-1440x120\", scale = 2, transform = 0, disabled = false, mirror = \"\" })"
        );
        assert!(
            rule("mode = \"1920x1080\"")
                .unwrap()
                .contains("mode = \"1920x1080\"")
        );
        for bad in [
            "mode = \"wide\"",
            "mode = \"0x1080@60\"",
            "mode = \"1920x1080@0\"",
            "scale = 0",
            "position = [1]",
            "position = \"0x0\"",
            "transform = 1",
            "mirror = \"\\\"; os.exit()\"",
        ] {
            let rule = rule(bad).unwrap();
            assert!(
                rule.contains("mode = \"preferred\", position = \"auto\", scale = \"auto\", transform = 0, disabled = false, mirror = \"\""),
                "{bad}: {rule}"
            );
        }
        // Not Aizome's settings at all.
        assert!(followed("outputs = [", &tall).is_empty());
        assert!(followed("", &tall).is_empty());
    }

    #[test]
    fn scales_hyprland_would_round_are_rounded_here() {
        let tall = [display("DP-2", "RHT", "Tall", "")];
        // 2560x1440 at 1.5 is 1706.67 points wide; 1.6 gives 1600x900.
        let rule = followed(
            "[[outputs]]\nmodel = \"Tall\"\nmode = \"2560x1440@60\"\nscale = 1.5\n",
            &tall,
        )[0]
        .rule();
        assert!(rule.contains("scale = 1.6,"), "{rule}");
    }

    #[test]
    fn mirrors_and_displays_aizome_keeps_off() {
        let displays = [
            display("DP-1", "RHT", "Wide", ""),
            display("DP-2", "RHT", "Tall", ""),
        ];
        let settings = "[[outputs]]\nmodel = \"Wide\"\nposition = [0, 0]\nmirror = \"DP-2\"\n\
                        [[outputs]]\nmodel = \"Tall\"\nposition = [1280, 0]\nenabled = false\ntransform = \"90\"\n";
        let rules: Vec<String> = followed(settings, &displays)
            .iter()
            .map(Followed::rule)
            .collect();
        assert!(rules[0].contains("position = \"0x0\"") && rules[0].contains("mirror = \"DP-2\""));
        // Turned on in Tatami, where it was may be another's: beside them.
        assert!(rules[1].contains("position = \"auto\"") && rules[1].contains("transform = 1"));
        // A display cannot mirror itself.
        let own = followed(
            "[[outputs]]\nmodel = \"Wide\"\nmirror = \"DP-1\"\n",
            &displays,
        );
        assert_eq!(own[0].mirror, None);
    }
}
