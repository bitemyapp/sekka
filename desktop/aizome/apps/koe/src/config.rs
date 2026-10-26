use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Dictation is on: the switch in Koe's window. The session's service
    /// starts only while it is (`aizome-koe enabled` is its condition).
    pub enabled: bool,
    pub engine: Engine,
    pub audio: Audio,
    pub output: Output,
    pub tray: Tray,
    pub replacements: Vec<Replacement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tray {
    pub enabled: bool,
}
impl Default for Tray {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    #[default]
    Local,
    Http,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Engine {
    pub backend: Backend,
    pub model: PathBuf,
    pub language: String,
    pub translate: bool,
    pub threads: usize,
    pub gpu: bool,
    pub prompt: String,
    pub http: Http,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            backend: Backend::Local,
            model: data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("models/ggml-base.bin"),
            language: "auto".into(),
            translate: false,
            threads: 4,
            // whisper.cpp falls back to the processor where no GPU can start.
            gpu: GPU_COMPILED,
            prompt: String::new(),
            http: Http::default(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Http {
    pub endpoint: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub timeout_seconds: u64,
}
impl Default for Http {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8080/inference".into(),
            model: "whisper-1".into(),
            api_key_env: None,
            timeout_seconds: 120,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub target: Option<String>,
    pub max_seconds: u64,
    pub min_ms: u64,
    pub silence_seconds: f32,
    pub silence_threshold: f32,
}
impl Default for Audio {
    fn default() -> Self {
        Self { target: None, max_seconds: 300, min_ms: 250, silence_seconds: 0.0, silence_threshold: 0.008 }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[default]
    Paste,
    Clipboard,
    None,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Desktop {
    #[default]
    Auto,
    Aizome,
    Wtype,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub mode: OutputMode,
    pub desktop: Desktop,
    pub notify: bool,
    pub append_space: bool,
    pub paste_delay_ms: u64,
    pub require_same_window: bool,
    /// Override auto selection for custom terminal bindings or app IDs.
    pub paste_shortcut: PasteShortcut,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PasteShortcut {
    #[default]
    Auto,
    CtrlV,
    CtrlShiftV,
    SuperV,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            mode: OutputMode::Paste,
            desktop: Desktop::Auto,
            notify: true,
            append_space: false,
            paste_delay_ms: 150,
            require_same_window: true,
            paste_shortcut: PasteShortcut::Auto,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

/// Whether recognition can use a GPU in this build (the package's Vulkan).
pub const GPU_COMPILED: bool = cfg!(any(feature = "vulkan", feature = "cuda"));

fn xdg(var: &str, suffix: &str) -> Result<PathBuf> {
    if let Some(value) = env::var_os(var).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(value);
        ensure!(p.is_absolute(), "{var} must be an absolute path");
        return Ok(p);
    }
    Ok(PathBuf::from(env::var_os("HOME").context("HOME is unset")?).join(suffix))
}
pub fn config_path() -> Result<PathBuf> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join("aizome/koe.toml"))
}
pub fn data_dir() -> Result<PathBuf> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?.join("aizome/koe"))
}
pub fn runtime_dir() -> Result<PathBuf> {
    let path = PathBuf::from(
        env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is unset; run Koe in your desktop user session")?,
    );
    ensure!(path.is_absolute(), "XDG_RUNTIME_DIR must be absolute");
    Ok(path.join("aizome-koe"))
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut config: Self = match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("invalid configuration: {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        if let Ok(p) = config.engine.model.strip_prefix("~") {
            config.engine.model = PathBuf::from(env::var_os("HOME").context("HOME is unset")?).join(p);
        } else if config.engine.model.is_relative() {
            config.engine.model = path.parent().unwrap_or(Path::new(".")).join(&config.engine.model);
        }
        config.validate()?;
        Ok(config)
    }
    /// Write the whole configuration, readable only by its owner, through a
    /// temporary file so the service never reads half of it.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        self.validate()?;
        let dir = path.parent().context("configuration has no parent")?;
        std::fs::create_dir_all(dir)?;
        let tmp = path.with_extension("toml.tmp");
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path).with_context(|| format!("cannot save {}", path.display()))?;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=128).contains(&self.engine.threads), "engine.threads must be 1..128");
        ensure!((1..=3600).contains(&self.audio.max_seconds), "audio.max_seconds must be 1..3600");
        ensure!(
            self.audio.min_ms >= 100 && self.audio.min_ms < self.audio.max_seconds * 1000,
            "audio.min_ms must be >=100 and shorter than max_seconds"
        );
        ensure!(
            self.audio.silence_seconds.is_finite() && (0.0..=120.0).contains(&self.audio.silence_seconds),
            "audio.silence_seconds must be 0..120"
        );
        ensure!(
            self.audio.silence_threshold.is_finite() && (0.0..1.0).contains(&self.audio.silence_threshold),
            "audio.silence_threshold must be 0..1"
        );
        ensure!(self.output.paste_delay_ms <= 5000, "output.paste_delay_ms must be <=5000");
        ensure!(
            self.engine.prompt.len() <= 8192 && !self.engine.prompt.contains('\0'),
            "engine.prompt is too long or contains NUL"
        );
        ensure!(
            self.engine.language == "auto"
                || (self.engine.language.len() <= 8
                    && self.engine.language.chars().all(|c| c.is_ascii_lowercase() || c == '-')),
            "engine.language must be auto or a language code"
        );
        ensure!(self.replacements.len() <= 256, "at most 256 replacements are supported");
        for r in &self.replacements {
            ensure!(
                !r.from.trim().is_empty() && r.from.len() <= 512 && r.to.len() <= 4096,
                "invalid replacement length"
            );
        }
        if self.engine.backend == Backend::Http {
            let url = reqwest::Url::parse(&self.engine.http.endpoint).context("invalid HTTP endpoint")?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            ensure!(
                url.scheme() == "https" || (url.scheme() == "http" && local),
                "HTTP transcription requires HTTPS except on loopback"
            );
            ensure!(
                url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "use api_key_env for credentials; endpoint cannot contain credentials, query, or fragment"
            );
            ensure!((1..=3600).contains(&self.engine.http.timeout_seconds), "HTTP timeout must be 1..3600 seconds");
            if self.engine.http.api_key_env.as_ref().is_some_and(|s| s.is_empty()) {
                bail!("api_key_env cannot be empty");
            }
        }
        Ok(())
    }
}
