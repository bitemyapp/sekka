//! The desktop through Aizome: windows, screenshots, the controls kit apps
//! report (`AIZOME_KIT_INSPECT`), and synthetic input (`aizomectl input`).
//!
//! **Set-of-marks.** Models point badly (Gemma 4 31B clicked ~160 px off in
//! the evaluations) but read well. So each control a kit app reports gets a
//! numbered box drawn on the screenshot and a line in a list, and the model
//! clicks `[7]` rather than a coordinate. Raw x/y stays for what reports
//! nothing (browsers, games), mapped through the model's measured
//! convention ([`crate::Coords`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use crate::event::{MouseButton, ScreenPoint};
use crate::exec;
use crate::ids::MarkId;
use crate::options::Options;

/// What models are shown at most, in pixels across: bigger screenshots
/// are scaled down by a whole factor (sharp text, and Gemma's and Qwen's
/// encoders resize anyway).
const MAX_WIDTH: u32 = 1600;
/// The most numbered controls on one screenshot.
const MAX_MARKS: usize = 80;

/// A rectangle in the global layout, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    pub(crate) fn center(&self) -> ScreenPoint {
        ScreenPoint { x: self.x + self.w / 2.0, y: self.y + self.h / 2.0 }
    }
}

/// A numbered control.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    pub id: MarkId,
    pub role: String,
    pub label: String,
    pub state: String,
    pub area: Area,
    /// The window it belongs to (or the shell surface).
    pub place: String,
}

/// How the last screenshot's pixels relate to the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    /// The image's size, in pixels.
    pub width: u32,
    pub height: u32,
    /// The layout point at the image's top left, and points per pixel.
    pub origin_x: f64,
    pub origin_y: f64,
    pub points_per_pixel: f64,
}

impl Frame {
    pub(crate) fn to_points(&self, px: f64, py: f64) -> ScreenPoint {
        let px = px.clamp(0.0, f64::from(self.width.saturating_sub(1)));
        let py = py.clamp(0.0, f64::from(self.height.saturating_sub(1)));
        ScreenPoint { x: self.origin_x + px * self.points_per_pixel, y: self.origin_y + py * self.points_per_pixel }
    }

    fn to_pixels(&self, a: &Area) -> (f64, f64, f64, f64) {
        let k = 1.0 / self.points_per_pixel;
        ((a.x - self.origin_x) * k, (a.y - self.origin_y) * k, a.w * k, a.h * k)
    }
}

/// What the model is shown after looking.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observation {
    pub text: String,
    pub image: Option<PathBuf>,
}

/// The desktop during one turn: the last frame and marks.
pub(crate) struct Desktop<'a> {
    options: &'a Options,
    cancel: &'a AtomicBool,
    shots: PathBuf,
    shot_count: u32,
    pub frame: Option<Frame>,
    pub marks: Vec<Mark>,
}

impl<'a> Desktop<'a> {
    pub(crate) fn new(options: &'a Options, cancel: &'a AtomicBool, shots: PathBuf) -> Desktop<'a> {
        Desktop { options, cancel, shots, shot_count: 0, frame: None, marks: Vec::new() }
    }

    fn aizomectl(&self, args: &[&str]) -> Result<String, String> {
        let mut argv = self.options.aizomectl.clone();
        argv.extend(args.iter().map(|s| (*s).to_owned()));
        let run = exec::run(&argv, &[], None, Duration::from_secs(30), self.cancel).map_err(|e| e.to_string())?;
        if run.exit_code == Some(0) {
            Ok(run.output)
        } else {
            Err(run.output.trim().chars().take(400).collect())
        }
    }

    fn json(&self, what: &str) -> Option<Value> {
        let out = self.aizomectl(&["--json", what]).ok()?;
        serde_json::from_str(out.trim()).ok()
    }

    fn windows(&self) -> Vec<Value> {
        self.json("windows").and_then(|v| v.get("windows").cloned()).and_then(|w| w.as_array().cloned()).unwrap_or_default()
    }

    /// The display screenshots show: the primary one (else the first).
    fn screen(&self) -> Option<(String, Area)> {
        let v = self.json("outputs")?;
        let outputs = v.get("outputs")?.as_array()?;
        let pick = outputs
            .iter()
            .filter(|o| o.get("enabled").and_then(Value::as_bool).unwrap_or(true) && o.get("mirror").is_none_or(Value::is_null))
            .max_by_key(|o| o.get("primary").and_then(Value::as_bool).unwrap_or(false))?;
        let l = pick.get("logical")?;
        let f = |k: &str| l.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let name = pick.get("name")?.as_str()?.to_owned();
        Some((name, Area { x: f("x"), y: f("y"), w: f("w"), h: f("h") }))
    }

    /// The windows and the shell, with each reported control numbered.
    pub(crate) fn collect_marks(&mut self) -> String {
        let wins = self.windows();
        let docs = self.inspect_docs();
        let screen = self.screen().map(|s| s.1).unwrap_or(Area { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 });
        let mut lines = Vec::new();
        let mut marks: Vec<Mark> = Vec::new();
        // The focused window first: it is what the person is looking at.
        let mut order: Vec<&Value> = wins.iter().collect();
        order.sort_by_key(|w| !w.get("focused").and_then(Value::as_bool).unwrap_or(false));
        let titled = |pid: Option<i64>| wins.iter().filter(|w| w.get("pid").and_then(Value::as_i64) == pid).count();
        for w in order {
            let g = w.get("geometry").cloned().unwrap_or(Value::Null);
            let gf = |k: &str| g.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let title = w.get("title").and_then(Value::as_str).unwrap_or("");
            let app = w.get("app_id").and_then(Value::as_str).unwrap_or("");
            let flags: Vec<&str> = ["focused", "minimised", "hidden"]
                .into_iter()
                .filter(|k| w.get(*k).and_then(Value::as_bool).unwrap_or(false))
                .collect();
            lines.push(format!(
                "window \"{title}\" ({app}) at {:.0},{:.0} size {:.0}×{:.0}{}{}",
                gf("x"),
                gf("y"),
                gf("w"),
                gf("h"),
                if flags.is_empty() { "" } else { " " },
                flags.join(" ")
            ));
            if flags.iter().any(|f| *f != "focused") {
                continue;
            }
            let pid = w.get("pid").and_then(Value::as_i64);
            for d in &docs {
                if d.get("pid").and_then(Value::as_i64) != pid {
                    continue;
                }
                let kind = d.get("kind").and_then(Value::as_str).unwrap_or("window");
                if !matches!(kind, "window" | "dialog" | "utility") {
                    continue;
                }
                if titled(pid) > 1 && d.get("title").and_then(Value::as_str) != Some(title) {
                    continue;
                }
                add_marks(d, gf("x"), gf("y"), title, &mut marks, &mut lines);
            }
        }
        // The shell's surfaces are placed by their anchors (docs/shell.md).
        for d in &docs {
            if d.get("app_id").and_then(Value::as_str) != Some("org.aizome.Shell")
                || d.get("kind").and_then(Value::as_str) != Some("surface")
            {
                continue;
            }
            let size = d.get("size").and_then(Value::as_array);
            let sw = size.and_then(|s| s.first()).and_then(Value::as_f64).unwrap_or(0.0);
            let sh = size.and_then(|s| s.get(1)).and_then(Value::as_f64).unwrap_or(0.0);
            let title = d.get("title").and_then(Value::as_str).unwrap_or("");
            let (ox, oy) = match title {
                "Menu bar" => (screen.x, screen.y),
                "Deskbar" => (screen.x + screen.w - sw, screen.y + 24.0),
                "Palette" if d.get("has_keyboard").and_then(Value::as_bool).unwrap_or(false) => {
                    (screen.x + (screen.w - sw) / 2.0, screen.y + 24.0 + (screen.h - 24.0 - sh) / 2.0)
                }
                _ => continue,
            };
            lines.push(format!("shell: {title}"));
            add_marks(d, ox, oy, title, &mut marks, &mut lines);
        }
        if docs.iter().any(|d| matches!(d.get("kind").and_then(Value::as_str), Some("menu" | "popover"))) {
            lines.push("(a menu or popover is open; its items aren't numbered: use the keyboard or a screenshot)".into());
        }
        self.marks = marks;
        if lines.is_empty() { "(no windows are open)".into() } else { lines.join("\n") }
    }

    fn inspect_docs(&self) -> Vec<Value> {
        let Some(dir) = &self.options.inspect_dir else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
            .collect()
    }

    /// Take a screenshot, scale it for the model, number the controls on it.
    pub(crate) fn screenshot(&mut self, marks: bool) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.shots).map_err(|e| format!("{}: {e}", self.shots.display()))?;
        self.shot_count += 1;
        // The compositor writes it, as the person: somewhere they can write.
        let tmp = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let raw = tmp.join(format!("tasuke-shot-{}-{}.png", std::process::id(), self.shot_count));
        let (output, area) = self.screen().unwrap_or((String::new(), Area { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 }));
        let raw_s = raw.to_string_lossy().into_owned();
        let mut args = vec!["screenshot"];
        if !output.is_empty() {
            args.extend(["--output", output.as_str()]);
        }
        args.push(&raw_s);
        self.aizomectl(&args)?;
        let mut img = Image::read(&raw)?;
        let _ = std::fs::remove_file(&raw);
        let factor = img.width.div_ceil(MAX_WIDTH).max(1);
        if factor > 1 {
            img = img.shrink(factor);
        }
        let frame = Frame {
            width: img.width,
            height: img.height,
            origin_x: area.x,
            origin_y: area.y,
            points_per_pixel: if img.width > 0 { area.w / f64::from(img.width) } else { 1.0 },
        };
        if marks {
            for m in &self.marks {
                let (x, y, w, h) = frame.to_pixels(&m.area);
                img.draw_mark(m.id.0, x, y, w, h);
            }
        }
        let path = self.shots.join(format!("screen-{:03}.png", self.shot_count));
        img.write(&path)?;
        self.frame = Some(frame);
        Ok(path)
    }

    /// Look: the list of windows and numbered controls, and a screenshot
    /// for models that see.
    pub(crate) fn observe(&mut self) -> Observation {
        let list = self.collect_marks();
        let mut text = String::new();
        let mut image = None;
        if self.options.vision {
            match self.screenshot(self.options.set_of_marks) {
                Ok(p) => image = Some(p),
                Err(e) => text.push_str(&format!("(the screenshot failed: {e})\n")),
            }
        }
        if self.options.set_of_marks || !self.options.vision {
            text.push_str("On screen (numbered controls: click_element / type_into take the number):\n");
            text.push_str(&list);
        }
        if let Some(f) = &self.frame
            && self.options.vision
        {
            text.push_str(&format!("\nThe screenshot is {}×{} pixels.", f.width, f.height));
        }
        Observation { text, image }
    }

    pub(crate) fn mark(&self, id: MarkId) -> Option<&Mark> {
        self.marks.iter().find(|m| m.id == id)
    }

    /// A point the model gave, in layout points.
    pub(crate) fn point(&self, x: f64, y: f64) -> ScreenPoint {
        match self.frame {
            Some(f) => {
                let (px, py) = self.options.coords.to_image(x, y, f64::from(f.width), f64::from(f.height));
                f.to_points(px, py)
            }
            // No screenshot yet: take the numbers as points.
            None => ScreenPoint { x, y },
        }
    }

    pub(crate) fn click(&self, at: ScreenPoint, button: MouseButton, count: u32) -> Result<(), String> {
        self.aizomectl(&["input", "motion", &fmt(at.x), &fmt(at.y)])?;
        for _ in 0..count.clamp(1, 3) {
            self.aizomectl(&["input", "click", button.as_str()])?;
        }
        Ok(())
    }

    pub(crate) fn drag(&self, from: ScreenPoint, to: ScreenPoint) -> Result<(), String> {
        self.aizomectl(&["input", "motion", &fmt(from.x), &fmt(from.y)])?;
        self.aizomectl(&["input", "button", "left", "down"])?;
        for i in 1..=8 {
            let t = f64::from(i) / 8.0;
            let x = from.x + (to.x - from.x) * t;
            let y = from.y + (to.y - from.y) * t;
            self.aizomectl(&["input", "motion", &fmt(x), &fmt(y)])?;
            std::thread::sleep(Duration::from_millis(30));
        }
        self.aizomectl(&["input", "button", "left", "up"]).map(|_| ())
    }

    pub(crate) fn scroll(&self, at: ScreenPoint, lines: f64) -> Result<(), String> {
        self.aizomectl(&["input", "motion", &fmt(at.x), &fmt(at.y)])?;
        self.aizomectl(&["input", "scroll", "0", &format!("{:.1}", lines * 15.0)]).map(|_| ())
    }

    /// Type text: line by line, Return between lines. Aizome types US key
    /// positions, so only ASCII.
    pub(crate) fn type_text(&self, text: &str) -> Result<(), String> {
        if let Some(c) = text.chars().find(|c| !c.is_ascii() || (c.is_ascii_control() && *c != '\n' && *c != '\t')) {
            return Err(format!(
                "{c:?} can't be typed (only ASCII); for other text, ask the person or paste it from the clipboard"
            ));
        }
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.aizomectl(&["input", "key", "return"])?;
            }
            if !line.is_empty() {
                self.aizomectl(&["input", "type", line])?;
            }
        }
        Ok(())
    }

    pub(crate) fn key(&self, keys: &str) -> Result<String, String> {
        let keys = normalize_keys(keys).ok_or_else(|| format!("{keys:?} is not a key name"))?;
        let verb = if keys.contains('+') { "chord" } else { "key" };
        self.aizomectl(&["input", verb, &keys])?;
        Ok(keys)
    }

    /// Open an app by its menu name (from its desktop entry) or command.
    pub(crate) fn open_app(&self, name: &str) -> Result<String, String> {
        let entries = desktop_entries();
        let want = name.trim().to_lowercase();
        let found = entries
            .iter()
            .find(|e| e.name.to_lowercase() == want || e.id.to_lowercase() == want)
            .or_else(|| entries.iter().find(|e| e.name.to_lowercase().starts_with(&want)))
            .or_else(|| entries.iter().find(|e| e.name.to_lowercase().contains(&want) || e.keywords.contains(&want)));
        let Some(entry) = found else {
            let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            return Err(format!("no app is called {name:?}; the apps are: {}", names.join(", ")));
        };
        let mut args = vec!["spawn", "--"];
        args.extend(entry.exec.iter().map(String::as_str));
        self.aizomectl(&args)?;
        Ok(entry.name.clone())
    }
}

fn fmt(v: f64) -> String {
    format!("{v:.1}")
}

const MARKED_ROLES_EXCLUDED: &[&str] =
    &["window", "label", "statictext", "group", "genericcontainer", "pane", "scrollview", "unknown", "paragraph"];

fn add_marks(d: &Value, ox: f64, oy: f64, place: &str, marks: &mut Vec<Mark>, lines: &mut Vec<String>) {
    let Some(elements) = d.get("elements").and_then(Value::as_array) else { return };
    for e in elements {
        if marks.len() >= MAX_MARKS {
            lines.push("  … (more controls than can be numbered)".into());
            return;
        }
        let shown = e.get("shown").or_else(|| e.get("rect")).and_then(Value::as_array);
        let Some([x, y, w, h]) = shown.and_then(|r| {
            let v: Vec<f64> = r.iter().filter_map(Value::as_f64).collect();
            <[f64; 4]>::try_from(v).ok()
        }) else {
            continue;
        };
        if w < 2.0 || h < 2.0 {
            continue;
        }
        let role = e.get("role").and_then(Value::as_str).unwrap_or("");
        let label = e
            .get("label")
            .or_else(|| e.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| (!role.is_empty()).then(|| e.get("name").and_then(Value::as_str).unwrap_or("").replace(['-', '_'], " ")))
            .unwrap_or_default();
        let label: String = label.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
        let mut state = Vec::new();
        for k in ["checked", "selected", "focused", "disabled", "expanded"] {
            if e.get(k).and_then(Value::as_bool).unwrap_or(false) {
                state.push(k.to_owned());
            }
        }
        if let Some(v) = e.get("value").and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_f64().map(|f| f.to_string()))) {
            state.push(format!("value \"{}\"", v.chars().take(40).collect::<String>()));
        }
        let area = Area { x: ox + x, y: oy + y, w, h };
        let markable = !role.is_empty() && !MARKED_ROLES_EXCLUDED.contains(&role) && !label.is_empty();
        if markable {
            let id = MarkId(marks.len() as u32 + 1);
            lines.push(format!("  [{id}] {role} \"{label}\"{}{}", if state.is_empty() { "" } else { " " }, state.join(" ")));
            marks.push(Mark { id, role: role.into(), label: label.clone(), state: state.join(" "), area, place: place.into() });
        } else if !label.is_empty() && role.is_empty() {
            lines.push(format!("  text \"{label}\""));
        }
        // A segmented control's segments are clickable on their own.
        if let Some(parts) = e.get("parts").and_then(Value::as_array) {
            for p in parts {
                let name = p.get("name").and_then(Value::as_str).unwrap_or("");
                let Some(r) = p.get("rect").and_then(Value::as_array) else { continue };
                let v: Vec<f64> = r.iter().filter_map(Value::as_f64).collect();
                let Ok([px, py, pw, ph]) = <[f64; 4]>::try_from(v) else { continue };
                if name.is_empty() || marks.len() >= MAX_MARKS {
                    continue;
                }
                let id = MarkId(marks.len() as u32 + 1);
                lines.push(format!("  [{id}] segment \"{name}\" of \"{label}\""));
                marks.push(Mark {
                    id,
                    role: "segment".into(),
                    label: name.into(),
                    state: String::new(),
                    area: Area { x: ox + px, y: oy + py, w: pw, h: ph },
                    place: place.into(),
                });
            }
        }
    }
}

/// `ctrl+A`, `Super + Space`, `enter` → aizomectl's names.
pub(crate) fn normalize_keys(keys: &str) -> Option<String> {
    let alias = |k: &str| -> String {
        match k {
            "enter" => "return",
            "esc" => "escape",
            "cmd" | "command" | "meta" | "win" | "windows" | "logo" | "mod4" => "super",
            "control" => "ctrl",
            "option" => "alt",
            "del" => "delete",
            "pgup" => "pageup",
            "pgdn" => "pagedown",
            other => other,
        }
        .to_owned()
    };
    let joined: Vec<String> =
        keys.trim().to_lowercase().split('+').map(|k| alias(k.trim())).filter(|k| !k.is_empty()).collect();
    let ok = !joined.is_empty()
        && joined.iter().all(|k| k.chars().all(|c| c.is_ascii_alphanumeric() || "-=[];',./\\`".contains(c)));
    ok.then(|| joined.join("+"))
}

struct DesktopEntry {
    id: String,
    name: String,
    keywords: String,
    exec: Vec<String>,
}

fn desktop_entries() -> Vec<DesktopEntry> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::options::home_dir().join(".local/share"));
    dirs.push(home);
    let data = std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/run/current-system/sw/share".into());
    dirs.extend(std::env::split_paths(&data));
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    dirs.push(PathBuf::from("/etc/profiles/per-user").join(std::env::var("USER").unwrap_or_default()).join("share"));
    let mut out: Vec<DesktopEntry> = Vec::new();
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(d.join("applications")) else { continue };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let id = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            if out.iter().any(|o| o.id == id) {
                continue;
            }
            if let Some(entry) = parse_entry(&p, id) {
                out.push(entry);
            }
        }
    }
    out
}

fn parse_entry(path: &Path, id: String) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_main = false;
    let (mut name, mut exec, mut keywords, mut hidden) = (None, None, String::new(), false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        match line.split_once('=') {
            Some(("Name", v)) => name = Some(v.trim().to_owned()),
            Some(("Exec", v)) => exec = Some(v.trim().to_owned()),
            Some(("Keywords", v)) => keywords = v.to_lowercase(),
            Some(("NoDisplay" | "Hidden", v)) if v.trim() == "true" => hidden = true,
            _ => {}
        }
    }
    if hidden {
        return None;
    }
    let exec: Vec<String> = exec?
        .split_whitespace()
        .filter(|a| !(a.len() == 2 && a.starts_with('%')))
        .map(|a| a.trim_matches('"').to_owned())
        .collect();
    let name = name?;
    (!exec.is_empty()).then(|| DesktopEntry { id, name, keywords, exec })
}

/// An RGBA image, for scaling screenshots and drawing marks.
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    fn read(path: &Path) -> Result<Image, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("the screenshot is too big")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return Err("an indexed screenshot".into()),
        };
        Ok(Image { width: info.width, height: info.height, rgba })
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&self.rgba).map_err(|e| e.to_string())
    }

    /// Average each `k`×`k` block into one pixel.
    fn shrink(&self, k: u32) -> Image {
        let (w, h) = (self.width / k, self.height / k);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for dy in 0..k {
                    let row = ((y * k + dy) * self.width + x * k) as usize * 4;
                    for dx in 0..k as usize {
                        for (c, s) in sum.iter_mut().enumerate() {
                            *s += u32::from(self.rgba[row + dx * 4 + c]);
                        }
                    }
                }
                let o = ((y * w + x) * 4) as usize;
                for c in 0..4 {
                    rgba[o + c] = (sum[c] / (k * k)) as u8;
                }
            }
        }
        Image { width: w, height: h, rgba }
    }

    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let o = ((y as u32 * self.width + x as u32) * 4) as usize;
        self.rgba[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
    }

    fn fill(&mut self, x: i64, y: i64, w: i64, h: i64, c: [u8; 3]) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.put(xx, yy, c);
            }
        }
    }

    /// A box around the control and its number in a tag at the top left.
    fn draw_mark(&mut self, n: u32, x: f64, y: f64, w: f64, h: f64) {
        const COLORS: [[u8; 3]; 6] =
            [[230, 25, 75], [0, 130, 200], [245, 130, 48], [60, 160, 60], [145, 30, 180], [0, 128, 128]];
        let c = COLORS[(n as usize - 1) % COLORS.len()];
        let (x, y, w, h) = (x.round() as i64, y.round() as i64, w.round() as i64, h.round() as i64);
        for t in 0..2 {
            self.fill(x - t, y - t, w + 2 * t, 1, c);
            self.fill(x - t, y + h - 1 + t, w + 2 * t, 1, c);
            self.fill(x - t, y - t, 1, h + 2 * t, c);
            self.fill(x + w - 1 + t, y - t, 1, h + 2 * t, c);
        }
        let digits: Vec<u32> = n.to_string().chars().filter_map(|d| d.to_digit(10)).collect();
        let scale = 2;
        let tag_w = digits.len() as i64 * (3 * scale + scale) + scale;
        let tag_h = 5 * scale + 2 * scale;
        // Inside the box's top left when it fits, else just above it.
        let ty = if h >= tag_h + 4 { y } else { (y - tag_h).max(0) };
        self.fill(x, ty, tag_w, tag_h, c);
        for (i, d) in digits.iter().enumerate() {
            let gx = x + scale + i as i64 * (3 * scale + scale);
            for (row, bits) in DIGITS[*d as usize].iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) != 0 {
                        self.fill(gx + col * scale, ty + scale + row as i64 * scale, scale, scale, [255, 255, 255]);
                    }
                }
            }
        }
    }
}

/// 3×5 digits, a row per entry, high bit left.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_normalize() {
        assert_eq!(normalize_keys("Ctrl + A").as_deref(), Some("ctrl+a"));
        assert_eq!(normalize_keys("cmd+space").as_deref(), Some("super+space"));
        assert_eq!(normalize_keys("Enter").as_deref(), Some("return"));
        assert_eq!(normalize_keys("rm -rf ~; x"), None);
        assert_eq!(normalize_keys(""), None);
    }

    #[test]
    fn marks_number_controls_and_segments_in_layout_points() {
        let doc = json!({"pid": 7, "kind": "window", "title": "Settings", "elements": [
            {"rect": [0, 0, 400, 30], "shown": [0, 0, 400, 30], "text": "Appearance"},
            {"rect": [10, 40, 100, 60], "shown": [10, 40, 100, 60], "role": "radiobutton", "label": "Day", "selected": false},
            {"rect": [10, 120, 200, 30], "shown": [10, 120, 200, 30], "role": "button", "label": "Reset", "disabled": true},
            {"rect": [0, 0, 1, 1], "shown": [0, 0, 0, 0], "role": "button", "label": "hidden"},
            {"rect": [10, 200, 300, 24], "shown": [10, 200, 300, 24], "role": "group", "label": "Size",
             "parts": [{"name": "Small", "rect": [10, 200, 100, 24]}, {"name": "Large", "rect": [110, 200, 100, 24]}]}
        ]});
        let mut marks = Vec::new();
        let mut lines = Vec::new();
        add_marks(&doc, 100.0, 50.0, "Settings", &mut marks, &mut lines);
        assert_eq!(marks.len(), 4, "{lines:?}");
        assert_eq!(marks[0].label, "Day");
        assert_eq!(marks[0].area.center(), ScreenPoint { x: 160.0, y: 120.0 });
        assert_eq!(marks[1].state, "disabled");
        assert_eq!(marks[2].role, "segment");
        assert_eq!(marks[3].label, "Large");
        assert!(lines.iter().any(|l| l == "  text \"Appearance\""));
        assert!(lines.iter().any(|l| l == "  [1] radiobutton \"Day\""));
    }

    #[test]
    fn frames_map_model_points_to_layout_points() {
        let f = Frame { width: 1440, height: 900, origin_x: 0.0, origin_y: 0.0, points_per_pixel: 2.0 };
        assert_eq!(f.to_points(100.0, 50.0), ScreenPoint { x: 200.0, y: 100.0 });
        assert_eq!(f.to_points(-5.0, 5000.0), ScreenPoint { x: 0.0, y: 1798.0 });
    }

    #[test]
    fn shrinking_averages_and_marks_draw_inside() {
        let mut img = Image { width: 4, height: 2, rgba: [[0u8, 0, 0, 255], [255, 255, 255, 255]].repeat(4).concat() };
        let half = img.shrink(2);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.rgba[0], 127);
        img = Image { width: 100, height: 60, rgba: vec![0; 100 * 60 * 4] };
        img.draw_mark(13, 10.0, 10.0, 50.0, 30.0);
        let px = |x: u32, y: u32| img.rgba[((y * 100 + x) * 4) as usize];
        assert_eq!(px(10, 10), 230, "the tag's colour at the box's corner");
        assert_eq!(px(59, 39), 230, "the box's bottom right");
        assert_eq!(px(30, 30), 0, "the inside is left alone");
    }

    #[test]
    fn desktop_entries_parse() {
        let dir = std::env::temp_dir().join(format!("tasuke-entries-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("org.aizome.Soroban.desktop");
        std::fs::write(&p, "[Desktop Entry]\nName=Calculator\nExec=aizome-soroban %U\nKeywords=math;sum;\n[Desktop Action x]\nName=Other\n").unwrap();
        let e = parse_entry(&p, "org.aizome.Soroban".into()).unwrap();
        assert_eq!(e.name, "Calculator");
        assert_eq!(e.exec, vec!["aizome-soroban"]);
        assert!(e.keywords.contains("math"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
