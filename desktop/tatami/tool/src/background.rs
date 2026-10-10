// SPDX-License-Identifier: MIT OR Apache-2.0
//! The desktop background, chosen from Tatami's wallpapers as Omarchy 4's
//! background switcher does (Style › Background, Super+Ctrl+Space). The
//! global image remains a symlink for compatibility with the lock screen.
//! Appearance and per-output overrides are saved separately, then passed to
//! swaybg as arguments (never a shell command or a resized image).
use crate::{CONFIG, hypr, menu, util};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

const MODES: [(&str, &str); 6] = [
    ("fill", "Fill — crop edges, keep proportions"),
    ("fit", "Fit — show the whole image, keep proportions"),
    ("stretch", "Stretch — fill the screen, change proportions"),
    ("center", "Center — original size, crop if larger"),
    ("tile", "Tile — repeat at original size"),
    ("solid_color", "Solid color — hide the image"),
];

fn defaults() -> Value {
    json!({"mode": "fill", "color": "#1a1b26", "outputs": {}})
}

fn settings_file() -> PathBuf {
    util::state_dir().join("background.json")
}

fn color(value: &str) -> Option<String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    (value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("#{}", value.to_ascii_lowercase()))
}

fn valid_output(value: &str) -> bool {
    !value.is_empty() && value != "*" && hypr::safe_name(value)
}

fn validate_style(value: &Value, output: bool) -> util::Result<()> {
    let object = value
        .as_object()
        .ok_or("Expected wallpaper settings object")?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "mode" => value
                .as_str()
                .is_some_and(|v| MODES.iter().any(|(mode, _)| *mode == v)),
            "color" => value.as_str().and_then(color).is_some(),
            "image" if output => value.as_str().is_some_and(|v| Path::new(v).is_absolute()),
            "outputs" if !output => continue,
            _ => false,
        };
        if !valid {
            return Err(format!("Invalid wallpaper setting: {key}={value}"));
        }
    }
    Ok(())
}

fn parse_settings(text: &str) -> util::Result<Value> {
    let mut value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    validate_style(&value, false)?;
    if value.get("outputs").is_none() {
        value["outputs"] = json!({});
    }
    for (name, style) in value["outputs"]
        .as_object()
        .ok_or("Expected outputs object")?
    {
        if !valid_output(name) {
            return Err(format!("Invalid output name: {name}"));
        }
        validate_style(style, true)?;
    }
    Ok(value)
}

fn load(file: &Path) -> util::Result<Value> {
    match fs::read_to_string(file) {
        Ok(text) => parse_settings(&text).map_err(|error| format!("{}: {error}", file.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
        Err(error) => Err(error.to_string()),
    }
}

fn save(file: &Path, settings: &Value) -> util::Result<()> {
    let temporary = file.with_extension("json.new");
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&temporary, text + "\n")
        .and_then(|()| fs::rename(&temporary, file))
        .map_err(|error| error.to_string())
}

fn effective(settings: &Value, output: Option<&str>, image: &Path) -> Value {
    let override_ = output
        .map(|name| &settings["outputs"][name])
        .unwrap_or(&Value::Null);
    let fallback = defaults();
    let mut result = json!({"image": image});
    for key in ["mode", "color"] {
        result[key] = override_
            .get(key)
            .or_else(|| settings.get(key))
            .unwrap_or(&fallback[key])
            .clone();
    }
    if let Some(image) = override_.get("image") {
        result["image"] = image.clone();
    }
    result
}

/// One process owns all outputs, including hot-plugged ones. The wildcard
/// supplies defaults, followed by complete, inherited per-output settings.
fn swaybg_args(settings: &Value, image: &Path) -> Vec<OsString> {
    let mut args = Vec::new();
    let mut append = |name: &str, style: Value| {
        let selected = Path::new(style["image"].as_str().unwrap());
        // A removable or deleted per-output image falls back without losing
        // its saved choice; the next restart retries that file.
        let selected = if selected.is_file() { selected } else { image };
        args.extend([
            "-o".into(),
            name.into(),
            "-m".into(),
            style["mode"].as_str().unwrap().into(),
            "-c".into(),
            style["color"].as_str().unwrap().into(),
        ]);
        if style["mode"] != "solid_color" {
            args.extend(["-i".into(), selected.as_os_str().to_owned()]);
        }
    };
    append("*", effective(settings, None, image));
    for name in settings["outputs"].as_object().unwrap().keys() {
        append(name, effective(settings, Some(name), image));
    }
    args
}

fn restart() {
    if !util::run(
        "systemctl",
        &["--user", "restart", "--no-block", "tatami-swaybg.service"],
    ) {
        eprintln!("tatami: wallpaper saved; it will apply when the Tatami session starts");
    }
}

/// The shipped default, a symlink the NixOS module writes.
fn default() -> PathBuf {
    PathBuf::from(format!("{CONFIG}/background"))
}

fn chosen() -> PathBuf {
    util::state_dir().join("background")
}

/// Point the choice at `target`, replacing any previous one in one step.
fn link(target: &std::path::Path) -> std::io::Result<()> {
    let link = chosen();
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir)?;
    }
    let staged = link.with_extension("new");
    let _ = fs::remove_file(&staged);
    symlink(target, &staged)?;
    fs::rename(&staged, &link)
}

/// Before the background unit starts: a missing choice, or one whose image
/// is gone after an update, becomes the default.
pub fn ensure() {
    if fs::metadata(chosen()).is_err() {
        let _ = link(&default());
    }
}

/// The image in use, for the lock screen.
pub fn path() -> PathBuf {
    if fs::metadata(chosen()).is_ok() {
        chosen()
    } else {
        default()
    }
}

fn change(action: &str, value: Option<&str>, output: Option<&str>) -> util::Result<()> {
    let dir = util::state_dir();
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    // Serialize concurrent menu/CLI edits, including the legacy image symlink.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("background.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let file = settings_file();
    // An explicit full reset is also the recovery path for a malformed file.
    let mut settings = if action == "reset" && output.is_none() {
        defaults()
    } else {
        load(&file)?
    };
    if action == "reset" {
        if let Some(output) = output {
            settings["outputs"].as_object_mut().unwrap().remove(output);
        }
    } else {
        let value = value.ok_or("Missing wallpaper setting")?;
        let (key, value) = match action {
            "mode" if MODES.iter().any(|(mode, _)| *mode == value) => ("mode", value.to_owned()),
            "color" => (
                "color",
                color(value).ok_or("Color must be #RRGGBB or RRGGBB")?,
            ),
            "set" => {
                let image = Path::new(value);
                if !image.is_absolute() || !image.is_file() || fs::File::open(image).is_err() {
                    return Err(
                        "Wallpaper must be an absolute path to a readable image file".into(),
                    );
                }
                if output.is_none() {
                    link(image).map_err(|error| error.to_string())?;
                    restart();
                    return Ok(());
                }
                ("image", value.to_owned())
            }
            _ => {
                return Err("Mode must be fill, fit, stretch, center, tile, or solid_color".into());
            }
        };
        if let Some(output) = output {
            if settings["outputs"].get(output).is_none() {
                settings["outputs"][output] = json!({});
            }
            settings["outputs"][output][key] = value.into();
        } else {
            settings[key] = value.into();
        }
    }
    save(&file, &settings)?;
    restart();
    Ok(())
}

pub fn command(args: &[String]) -> util::Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            pick();
            return Ok(());
        }
        ["settings"] => {
            configure();
            return Ok(());
        }
        ["ensure"] => {
            ensure();
            return Ok(());
        }
        ["path"] => {
            println!("{}", path().display());
            return Ok(());
        }
        ["status"] => {
            let mut settings = load(&settings_file())?;
            settings["effective_default"] = effective(&settings, None, &path());
            println!("{}", serde_json::to_string_pretty(&settings).unwrap());
            return Ok(());
        }
        ["run", program] => {
            ensure();
            let settings = load(&settings_file()).unwrap_or_else(|error| {
                eprintln!("tatami: {error}; using default wallpaper appearance");
                defaults()
            });
            let error = Command::new(program)
                .args(swaybg_args(&settings, &path()))
                .exec();
            return Err(format!("Cannot start wallpaper renderer: {error}"));
        }
        _ => {}
    }
    let (args, output) = if args.len() >= 3 && args[args.len() - 2] == "--output" {
        let name = args[args.len() - 1];
        if !valid_output(name) {
            return Err("Use a display connector name such as DP-1 or eDP-1".into());
        }
        (&args[..args.len() - 2], Some(name))
    } else {
        (args.as_slice(), None)
    };
    match args {
        ["reset"] => change("reset", None, output),
        [action @ ("set" | "mode" | "color"), value] => change(action, Some(value), output),
        _ => Err("Use background settings, or background <set IMAGE|mode MODE|color HEX|reset> [--output NAME]".into()),
    }
}

fn prompt(text: &str) -> Option<String> {
    menu::walker_filter(
        &[
            "--dmenu",
            "--inputonly",
            "--theme",
            menu::PANEL_THEME,
            "-p",
            text,
        ],
        b"",
    )
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
}

fn choose_image() -> Option<String> {
    let catalog = fs::read_to_string("/run/current-system/sw/share/tatami/wallpapers.json")
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(json!([]));
    let entries = catalog.as_array()?;
    let mut labels: Vec<String> = entries
        .iter()
        .map(|entry| entry["name"].as_str().unwrap_or("Wallpaper").into())
        .collect();
    labels.push("Choose another image by file path…".into());
    let index = menu::panel_index("Wallpaper", &labels, None)?;
    if index == entries.len() {
        prompt("Image file (absolute path)…")
    } else {
        Some(format!(
            "/run/current-system/sw/share/backgrounds/tatami/{}",
            entries[index]["file"].as_str()?
        ))
    }
}

/// Style › Background settings: defaults or one output, then the appearance.
fn configure() {
    menu::after_menu();
    let settings = match load(&settings_file()) {
        Ok(settings) => settings,
        Err(error) => {
            util::notify(
                "",
                "Wallpaper settings could not be read",
                &format!("{error}. Run tatami background reset to recover."),
            );
            return;
        }
    };
    let mut outputs: Vec<String> = settings["outputs"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    if let Ok(monitors) = hypr::json_args(&["monitors", "all"]) {
        for monitor in monitors.as_array().into_iter().flatten() {
            if let Some(name) = monitor["name"].as_str().filter(|name| valid_output(name)) {
                outputs.push(name.into());
            }
        }
    }
    outputs.sort();
    outputs.dedup();
    let mut labels = vec!["All displays — defaults".into()];
    labels.extend(
        outputs
            .iter()
            .map(|name| format!("{name} — display override")),
    );
    let Some(index) = menu::panel_index("Wallpaper settings", &labels, None) else {
        return;
    };
    let output = index.checked_sub(1).map(|i| outputs[i].as_str());
    loop {
        let Ok(settings) = load(&settings_file()) else {
            return;
        };
        let current = effective(&settings, output, &path());
        let labels = vec![
            format!("Scaling: {}", current["mode"].as_str().unwrap()),
            format!("Background color: {}", current["color"].as_str().unwrap()),
            "Choose wallpaper…".into(),
            if output.is_some() {
                "Use defaults for this display".into()
            } else {
                "Reset all display appearance settings".into()
            },
        ];
        let Some(action) =
            menu::panel_index(output.unwrap_or("All displays — defaults"), &labels, None)
        else {
            return;
        };
        let selection = match action {
            0 => {
                let labels = MODES
                    .iter()
                    .map(|(_, label)| (*label).to_owned())
                    .collect::<Vec<_>>();
                let current = MODES.iter().position(|(mode, _)| current["mode"] == *mode);
                menu::panel_index("Scaling mode", &labels, current)
                    .map(|i| ("mode", Some(MODES[i].0.to_owned())))
            }
            1 => prompt("Background color (#RRGGBB)…").map(|v| ("color", Some(v))),
            2 => choose_image().map(|v| ("set", Some(v))),
            _ => Some(("reset", None)),
        };
        if let Some((action, value)) = selection
            && let Err(error) = change(action, value.as_deref(), output)
        {
            util::notify("", "Wallpaper settings were not applied", &error);
        }
    }
}

/// Style › Background and Super+Ctrl+Space: the wallpapers by name, with the
/// selected one previewed large (the tatami-backgrounds menu the module
/// generates).
pub fn pick() {
    crate::menu::after_menu();
    crate::menu::walker(&[
        "-m",
        "menus:tatami-backgrounds",
        "-p",
        "Background\u{2026}",
        "--minwidth",
        "330",
        "--maxwidth",
        "330",
        "--maxheight",
        "390",
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_invalid_or_unknown_values() {
        for value in [
            json!({"mode": "zoom"}),
            json!({"color": "red"}),
            json!({"color": "#aabbccdd"}),
            json!({"outputs": []}),
            json!({"outputs": {"*": {"mode": "fit"}}}),
            json!({"outputs": {"DP-1": {"image": "relative.jpg"}}}),
            json!({"outputs": {"DP-1": {"mode": 3}}}),
            json!({"typo": "fit"}),
        ] {
            assert!(parse_settings(&value.to_string()).is_err(), "{value}");
        }
        assert_eq!(color("A1b2C3"), Some("#a1b2c3".into()));
    }

    #[test]
    fn output_overrides_inherit_individual_properties() {
        let settings = parse_settings(
            r##"{
            "mode": "fit", "color": "#112233",
            "outputs": {"DP-1": {"mode": "center"}, "eDP-1": {"color": "#445566"}}
        }"##,
        )
        .unwrap();
        let image = Path::new("/wallpaper.jpg");
        assert_eq!(
            effective(&settings, Some("DP-1"), image),
            json!({"mode": "center", "color": "#112233", "image": image})
        );
        assert_eq!(
            effective(&settings, Some("eDP-1"), image),
            json!({"mode": "fit", "color": "#445566", "image": image})
        );
        assert_eq!(
            effective(&settings, Some("HDMI-A-1"), image),
            effective(&settings, None, image)
        );
    }

    #[test]
    fn every_mode_has_an_explicit_renderer_mode_and_color() {
        for (mode, _) in MODES {
            let settings = parse_settings(&json!({"mode": mode}).to_string()).unwrap();
            let args = swaybg_args(&settings, Path::new("/image with spaces.jpg"));
            assert_eq!(
                args[..6],
                ["-o", "*", "-m", mode, "-c", "#1a1b26"].map(OsString::from)
            );
            if mode == "solid_color" {
                assert_eq!(args.len(), 6);
            } else {
                assert_eq!(
                    args[6..],
                    ["-i", "/image with spaces.jpg"].map(OsString::from)
                );
            }
        }
    }
}
