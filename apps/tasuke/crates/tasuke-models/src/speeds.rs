//! Speeds measured on this computer, model by model (`tasuke bench`, or
//! Measure on Compare Models), kept in `~/.local/share/tasuke/speeds.toml`
//! and shown beside the estimates. A local model's speed is this machine's;
//! a cloud model's is the service's, as seen from here.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Paths;
use crate::fit::Fit;
use crate::hardware::Hardware;

/// One measurement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// Output tokens a second once the answer is flowing.
    pub decode: f64,
    /// From sending to the first token, the model's loading excluded.
    pub first_token_ms: u64,
    /// Prompt tokens a second, when the server reports it (llama.cpp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<f64>,
    /// The tokens it wrote, thinking included.
    pub tokens: u64,
    /// "2026-11-06".
    pub date: String,
    /// What it ran on: "NVIDIA GeForce RTX 5090", or "Anthropic".
    pub on: String,
}

/// Every measurement, by model: a catalog id, or "anthropic/claude-…".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Speeds {
    #[serde(flatten)]
    pub by_model: BTreeMap<String, Measured>,
}

impl Speeds {
    pub fn path(paths: &Paths) -> PathBuf {
        paths.data_dir.join("speeds.toml")
    }

    /// The file's measurements; none when it's missing or unreadable.
    pub fn load(path: &Path) -> Speeds {
        std::fs::read_to_string(path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    pub fn get(&self, model: &str) -> Option<&Measured> {
        self.by_model.get(model)
    }

    /// Record a measurement, replacing the model's last one.
    pub fn record(&mut self, model: &str, m: Measured) {
        self.by_model.insert(model.to_owned(), m);
    }
}

/// Where a local model runs, as a measurement records it: the GPU's name,
/// or "the processor".
pub fn place(fit: &Fit, hw: &Hardware) -> String {
    let gpu = match *fit {
        Fit::Gpu { gpu, .. } | Fit::GpuWithExpertOffload { gpu, .. } | Fit::Unified { gpu, .. } => hw.gpu(gpu),
        Fit::Cpu { .. } | Fit::TooLarge { .. } => None,
    };
    gpu.map_or_else(|| "the processor".to_owned(), |g| g.name.clone())
}

/// Today's date, UTC: "2026-11-06".
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil(secs / 86_400)
}

/// The date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: u64) -> String {
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurements_round_trip_by_model() {
        let dir = std::env::temp_dir().join(format!("tasuke-speeds-{}", std::process::id()));
        let path = dir.join("speeds.toml");
        let mut s = Speeds::load(&path);
        assert!(s.by_model.is_empty(), "a missing file reads as none");
        let m = Measured {
            decode: 116.4,
            first_token_ms: 180,
            prefill: Some(3400.0),
            tokens: 256,
            date: "2026-11-06".into(),
            on: "NVIDIA GeForce RTX 5090".into(),
        };
        s.record("gemma-4-31b", m.clone());
        s.record("anthropic/claude-sonnet-5-5", Measured { prefill: None, on: "Anthropic".into(), ..m.clone() });
        s.save(&path).expect("saved");
        let back = Speeds::load(&path);
        assert_eq!(back, s);
        assert_eq!(back.get("gemma-4-31b"), Some(&m));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn days_become_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(11_016), "2000-02-29");
        assert_eq!(civil(20_763), "2026-11-06");
    }
}
