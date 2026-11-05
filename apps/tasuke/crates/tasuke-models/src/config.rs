//! Settings (`~/.config/tasuke/config.toml`) and where Tasuke keeps things
//! (XDG base directories).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::recommend::Preferences;
use crate::units::{ContextTokens, ModelId, Port};

/// Where things live. [`Paths::from_env`] follows the XDG base directory
/// specification; tests build one under a temporary directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paths {
    /// `$XDG_CONFIG_HOME/tasuke`
    pub config_dir: PathBuf,
    /// `$XDG_DATA_HOME/tasuke`
    pub data_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR/tasuke` (falls back to the data directory).
    pub runtime_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
        let xdg = |var: &str, fallback: &str| {
            std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(fallback))
        };
        let data_dir = xdg("XDG_DATA_HOME", ".local/share").join("tasuke");
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("tasuke"))
            .unwrap_or_else(|| data_dir.join("run"));
        Paths { config_dir: xdg("XDG_CONFIG_HOME", ".config").join("tasuke"), data_dir, runtime_dir }
    }

    /// Everything under one directory (tests, `--root`).
    pub fn under(root: &Path) -> Paths {
        Paths { config_dir: root.join("config"), data_dir: root.join("data"), runtime_dir: root.join("run") }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// `~/.local/share/tasuke/models`
    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    /// The router's presets, `~/.local/share/tasuke/models.ini`.
    pub fn presets_file(&self) -> PathBuf {
        self.data_dir.join("models.ini")
    }

    /// `$XDG_RUNTIME_DIR/tasuke/api-key`, written by the unit's ExecStartPre.
    pub fn api_key_file(&self) -> PathBuf {
        self.runtime_dir.join("api-key")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }
}

/// `config.toml`. Unknown tables and keys are kept when it is saved again,
/// so the window, the agent and the CLI can each own their part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub model: ModelConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub recommend: Preferences,
    #[serde(flatten)]
    pub other: toml::Table,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// The model the assistant uses (`tasuke use ID`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ModelId>,
    /// Override the catalog's default context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextTokens>,
    #[serde(flatten)]
    pub other: toml::Table,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: Port,
    /// Unload the model after this long without requests (0: never).
    #[serde(default = "default_idle_minutes")]
    pub idle_minutes: u32,
    /// The llama-server to run in `--foreground` mode (the unit has its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llama_server: Option<PathBuf>,
    #[serde(flatten)]
    pub other: toml::Table,
}

pub const DEFAULT_PORT: Port = Port(8737);
pub const DEFAULT_IDLE_MINUTES: u32 = 10;

fn default_port() -> Port {
    DEFAULT_PORT
}

fn default_idle_minutes() -> u32 {
    DEFAULT_IDLE_MINUTES
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig { port: DEFAULT_PORT, idle_minutes: DEFAULT_IDLE_MINUTES, llama_server: None, other: toml::Table::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "yes")]
    pub act: bool,
    #[serde(default)]
    pub desktop_control: bool,
    #[serde(flatten)]
    pub other: toml::Table,
}

fn yes() -> bool {
    true
}

impl Default for AgentConfig {
    fn default() -> AgentConfig {
        AgentConfig { act: true, desktop_control: false, other: toml::Table::new() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path}: {source}")]
    Parse { path: PathBuf, source: toml::de::Error },
}

impl Config {
    /// `config.toml` under `paths`, or the defaults when there is none.
    pub fn load(paths: &Paths) -> Result<Config, ConfigError> {
        Config::load_from(&paths.config_file())
    }

    pub fn load_from(path: &Path) -> Result<Config, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| ConfigError::Parse { path: path.to_owned(), source }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(source) => Err(ConfigError::Io { path: path.to_owned(), source }),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<(), ConfigError> {
        self.save_to(&paths.config_file())
    }

    /// Atomically (a temporary file renamed over the old one).
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let io_err = |source| ConfigError::Io { path: path.to_owned(), source };
        let text = toml::to_string_pretty(self).expect("config serializes");
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).map_err(io_err)?;
        fs::rename(&tmp, path).map_err(io_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_round_trip_keep_unknown_keys() {
        let dir = tempdir("config");
        let path = dir.join("config.toml");
        fs::write(
            &path,
            "[model]\nid = \"gemma-4-31b\"\nfuture = 1\n[agent]\nact = false\n[window]\nwidth = 900\n\
             [recommend]\nexclude_lineage = [\"Alibaba\"]\n",
        )
        .unwrap();
        let mut c = Config::load_from(&path).unwrap();
        assert_eq!(c.model.id, Some(ModelId::from("gemma-4-31b")));
        assert_eq!(c.server.port, DEFAULT_PORT);
        assert!(!c.agent.act);
        assert_eq!(c.recommend.exclude_lineage, ["Alibaba"]);
        c.model.context = Some(ContextTokens::k(64));
        c.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("future = 1"), "{text}");
        assert!(text.contains("width = 900"), "{text}");
        let again = Config::load_from(&path).unwrap();
        assert_eq!(again, c);
    }

    #[test]
    fn missing_file_is_defaults() {
        let c = Config::load_from(Path::new("/nonexistent/tasuke/config.toml")).unwrap();
        assert_eq!(c, Config::default());
        assert!(c.agent.act);
    }

    pub(crate) fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tasuke-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
