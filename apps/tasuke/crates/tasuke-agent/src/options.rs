//! How a turn runs: the model server, the modes, the limits, and where
//! Tasuke keeps its files.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::ids::{StepBudget, Tokens};
use crate::skills::Skills;

/// The model server's port when the user unit runs it (DESIGN.md).
pub const DEFAULT_PORT: u16 = 8737;

/// How a vision model gives screen points (models.toml's `coords`): each
/// answers in its own convention whatever the prompt says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Coords {
    /// Pixels of the screenshot it was shown.
    #[default]
    Pixel,
    /// 0–1000 across each axis (Qwen-VL, Holo).
    Norm1000,
    /// 0–1 across each axis (Nemotron Bolt).
    Norm1,
    /// Measured per axis (`grounding.py`): the screenshot's pixel is
    /// `scale · value + offset`. The model is asked for pixels. Gemma 4 12B,
    /// for one, answers x in a 0–1000 frame and y in pixels.
    Affine { x: Axis, y: Axis },
}

/// One axis of an affine coordinate mapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub scale: f64,
    pub offset: f64,
}

impl Coords {
    /// `pixel`, `norm1000`, `norm1`, or `affine:XSCALE,XOFFSET,YSCALE,YOFFSET`.
    pub fn parse(s: &str) -> Option<Coords> {
        match s.trim() {
            "pixel" | "pixels" => Some(Coords::Pixel),
            "norm1000" => Some(Coords::Norm1000),
            "norm1" => Some(Coords::Norm1),
            other => {
                let v: Vec<f64> =
                    other.strip_prefix("affine:")?.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
                let [xs, xo, ys, yo] = v[..] else { return None };
                Some(Coords::Affine { x: Axis { scale: xs, offset: xo }, y: Axis { scale: ys, offset: yo } })
            }
        }
    }

    /// The point a model gave, in pixels of the image it was shown (`w`×`h`).
    pub fn to_image(self, x: f64, y: f64, w: f64, h: f64) -> (f64, f64) {
        match self {
            Coords::Pixel => (x, y),
            Coords::Norm1000 => (x * w / 1000.0, y * h / 1000.0),
            Coords::Norm1 => (x * w, y * h),
            Coords::Affine { x: ax, y: ay } => (ax.scale * x + ax.offset, ay.scale * y + ay.offset),
        }
    }

    /// What the tools' schemas tell the model about x and y.
    pub fn describe(self, w: u32, h: u32) -> String {
        match self {
            Coords::Norm1000 => {
                "x and y from 0 to 1000 across the screenshot's width and height (0,0 top left, 1000,1000 bottom right)"
                    .into()
            }
            Coords::Norm1 => {
                "x and y from 0 to 1 across the screenshot's width and height (0,0 top left, 1,1 bottom right)".into()
            }
            Coords::Pixel | Coords::Affine { .. } => {
                format!("x and y in pixels of the latest screenshot, 0,0 top left; it is {w}×{h}")
            }
        }
    }
}

impl<'de> Deserialize<'de> for Coords {
    /// models.toml's forms: `coords = "norm1000"` or `coords = { x = [a, b], y = [c, d] }`.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Coords, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Name(String),
            Affine { x: [f64; 2], y: [f64; 2] },
        }
        match Form::deserialize(d)? {
            Form::Name(n) => Coords::parse(&n).ok_or_else(|| serde::de::Error::custom(format!("unknown coords {n:?}"))),
            Form::Affine { x, y } => Ok(Coords::Affine {
                x: Axis { scale: x[0], offset: x[1] },
                y: Axis { scale: y[0], offset: y[1] },
            }),
        }
    }
}

/// `~/.local/share/tasuke` (or `$XDG_DATA_HOME/tasuke`): conversations and memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDir(PathBuf);

impl DataDir {
    pub fn new(path: impl Into<PathBuf>) -> DataDir {
        DataDir(path.into())
    }

    /// The user's: `$TASUKE_DATA_DIR`, else `$XDG_DATA_HOME/tasuke`, else `~/.local/share/tasuke`.
    pub fn user() -> DataDir {
        if let Some(d) = std::env::var_os("TASUKE_DATA_DIR").filter(|v| !v.is_empty()) {
            return DataDir(PathBuf::from(d));
        }
        if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            return DataDir(PathBuf::from(d).join("tasuke"));
        }
        DataDir(home_dir().join(".local/share/tasuke"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn sessions(&self) -> PathBuf {
        self.0.join("sessions")
    }

    pub fn memory_file(&self) -> PathBuf {
        self.0.join("memory.toml")
    }
}

pub(crate) fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(h);
    }
    // SAFETY: getpwuid returns a pointer into static storage or null; it is
    // read at once, on this thread.
    unsafe {
        let pw = libc::getpwuid(libc::geteuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
            return PathBuf::from(std::ffi::OsStr::from_encoded_bytes_unchecked(dir.to_bytes()));
        }
    }
    PathBuf::from("/")
}

/// Everything a turn needs to know. [`Options::new`] has DESIGN.md's
/// defaults; [`Options::from_config`] reads the user's settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// When off, Tasuke only inspects and explains: state-changing tools
    /// are not offered to the model at all.
    pub act: bool,
    /// The desktop tools (screenshot, clicks, keys): off until the person turns it on.
    pub desktop_control: bool,
    /// The OpenAI-compatible endpoint, ending in `/v1`.
    pub base_url: String,
    /// The model id the server knows (router mode loads it on demand).
    pub model: String,
    pub api_key: Option<String>,
    pub step_budget: StepBudget,
    /// A wall-clock limit for the turn (the eval's safety net); none by default.
    pub time_limit: Option<Duration>,
    /// Whether the model takes images (offers `screenshot`).
    pub vision: bool,
    pub coords: Coords,
    /// Set-of-marks: when kit apps say where their controls are, number
    /// them on the screenshot, list them, and offer `click_element` and
    /// `type_into` (x/y clicks stay for windows without that information).
    pub set_of_marks: bool,
    /// The context size to assume when the server's `/props` doesn't say.
    pub context_fallback: Tokens,
    /// The most tokens one reply may have.
    pub max_output_tokens: Tokens,
    /// Extra fields for every chat request: sampling, `chat_template_kwargs`.
    pub request_params: serde_json::Map<String, serde_json::Value>,
    /// Send the model's reasoning back with its tool calls (gpt-oss's harmony format wants it).
    pub keep_reasoning: bool,
    pub skills: Skills,
    pub data_dir: DataDir,
    /// `tasuke-helper`, run through pkexec for `run_as_root`.
    pub helper: PathBuf,
    /// How to run `aizomectl` (a prefix: as root in the eval VM it is `as-alice aizomectl`).
    pub aizomectl: Vec<String>,
    /// Where kit apps write what their windows show (`AIZOME_KIT_INSPECT`), if anywhere.
    pub inspect_dir: Option<PathBuf>,
    /// A line added to the environment block (the eval says whose machine it is).
    pub environment_note: Option<String>,
    /// How long one model request may take before it is abandoned.
    pub request_timeout: Duration,
}

impl Options {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Options {
        Options {
            act: true,
            desktop_control: false,
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            step_budget: StepBudget::default(),
            time_limit: None,
            vision: false,
            coords: Coords::Pixel,
            set_of_marks: false,
            context_fallback: Tokens(32768),
            max_output_tokens: Tokens(8192),
            request_params: serde_json::Map::new(),
            keep_reasoning: false,
            skills: Skills::default(),
            data_dir: DataDir::user(),
            helper: default_helper(),
            aizomectl: vec!["aizomectl".into()],
            inspect_dir: std::env::var_os("AIZOME_KIT_INSPECT").filter(|v| !v.is_empty()).map(PathBuf::from),
            environment_note: None,
            request_timeout: Duration::from_secs(900),
        }
    }

    /// The user's settings (`~/.config/tasuke/config.toml`), the router's
    /// API key file, the installed skills.
    pub fn from_config() -> Options {
        let config = Config::load();
        let port = config.server.port.unwrap_or(DEFAULT_PORT);
        let mut o = Options::new(format!("http://127.0.0.1:{port}/v1"), config.model.id.clone().unwrap_or_default());
        o.api_key = read_api_key();
        if let Some(c) = config.model.context {
            o.context_fallback = Tokens(c);
        }
        if let Some(act) = config.agent.act {
            o.act = act;
        }
        if let Some(d) = config.agent.desktop_control {
            o.desktop_control = d;
        }
        o.skills = Skills::discover();
        o
    }
}

/// Where `tasuke-helper` is: `$TASUKE_HELPER`, the path the package was
/// built with, or beside this executable.
fn default_helper() -> PathBuf {
    if let Some(p) = std::env::var_os("TASUKE_HELPER").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("TASUKE_HELPER_PATH") {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("tasuke-helper")))
        .unwrap_or_else(|| PathBuf::from("/run/current-system/sw/bin/tasuke-helper"))
}

/// The parts of `~/.config/tasuke/config.toml` the agent reads. Unknown
/// keys are ignored; a broken file reads as empty.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: ModelConfig,
    pub server: ServerConfig,
    pub agent: AgentConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub id: Option<String>,
    pub context: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub idle_minutes: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub act: Option<bool>,
    pub desktop_control: Option<bool>,
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"));
        base.join("tasuke/config.toml")
    }

    pub fn load() -> Config {
        Config::load_from(&Config::path())
    }

    pub fn load_from(path: &Path) -> Config {
        std::fs::read_to_string(path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }
}

/// The router's key, `$XDG_RUNTIME_DIR/tasuke/api-key`, if it is there.
pub fn read_api_key() -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    let key = std::fs::read_to_string(PathBuf::from(dir).join("tasuke/api-key")).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_what_it_knows_and_ignores_the_rest() {
        let dir = std::env::temp_dir().join(format!("tasuke-config-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(
            &p,
            "[model]\nid = \"gemma-4-31b\"\ncontext = 49152\n[server]\nport = 9000\nother = 1\n[agent]\nact = false\n",
        )
        .unwrap();
        let c = Config::load_from(&p);
        assert_eq!(c.model.id.as_deref(), Some("gemma-4-31b"));
        assert_eq!(c.model.context, Some(49152));
        assert_eq!(c.server.port, Some(9000));
        assert_eq!(c.agent.act, Some(false));
        std::fs::write(&p, "[model\nbroken").unwrap();
        assert!(Config::load_from(&p).model.id.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn coords_parse() {
        assert_eq!(Coords::parse("norm1000"), Some(Coords::Norm1000));
        assert_eq!(Coords::parse("norm1"), Some(Coords::Norm1));
        assert_eq!(Coords::parse("pixel"), Some(Coords::Pixel));
        assert_eq!(Coords::parse("inches"), None);
        assert_eq!(Coords::parse("affine:1,2,3"), None);
        let gemma = Coords::parse("affine:1.45,8,0.99,-32").unwrap();
        let (x, y) = gemma.to_image(100.0, 100.0, 1440.0, 900.0);
        assert!((x - 153.0).abs() < 1e-9 && (y - 67.0).abs() < 1e-9);
        assert_eq!(Coords::Norm1000.to_image(500.0, 500.0, 1440.0, 900.0), (720.0, 450.0));
        assert_eq!(Coords::Norm1.to_image(0.5, 0.25, 1440.0, 900.0), (720.0, 225.0));
    }

    #[test]
    fn coords_read_from_toml_names_and_tables() {
        #[derive(Deserialize)]
        struct M {
            coords: Coords,
        }
        let m: M = toml::from_str("coords = \"norm1\"").unwrap();
        assert_eq!(m.coords, Coords::Norm1);
        let m: M = toml::from_str("coords = { x = [1.45, 8.0], y = [0.99, -32.0] }").unwrap();
        assert_eq!(
            m.coords,
            Coords::Affine { x: Axis { scale: 1.45, offset: 8.0 }, y: Axis { scale: 0.99, offset: -32.0 } }
        );
        assert!(toml::from_str::<M>("coords = \"feet\"").is_err());
    }
}
