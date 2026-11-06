//! The model server: one llama-server in router mode.
//!
//! The router has no model of its own (`llama-server` without `-m`); it reads
//! presets from `models.ini`, one section per installed catalog model, and
//! starts a child llama-server for the model a request names in its `model`
//! field (the catalog id). `--models-max 1` keeps one model loaded; the
//! children inherit the router's arguments, including `--sleep-idle-seconds`,
//! after which a child unloads its weights and KV cache and wakes on the next
//! request.
//!
//! The user unit `tasuke-llm.service` runs `tasuke server exec`, which writes
//! the presets for the current hardware and then becomes llama-server
//! ([`Server::router_args`]); `tasuke server start --foreground` runs the
//! same command as a child of the CLI.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Entry};
use crate::config::{Config, Paths};
use crate::fit::{self, Fit, FitOptions, KvCacheType};
use crate::hardware::{GpuVendor, Hardware};
use crate::store::{Installed, Store};
use crate::units::{ContextTokens, ModelId, Port};

pub const UNIT: &str = "tasuke-llm.service";
/// How long router children may take to stop before they're killed.
pub const STOP_TIMEOUT_SECS: u32 = 10;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("systemctl --user {verb} {UNIT} failed: {detail}")]
    Systemctl { verb: &'static str, detail: String },
    #[error("{url}: {detail}")]
    Http { url: String, detail: String },
    #[error("no installed model to serve: download one first (tasuke download ID)")]
    NothingInstalled,
}

/// One preset: an installed model with its placement.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Preset {
    pub id: ModelId,
    pub fit: Fit,
    /// INI keys (llama-server's long argument names) and values, in order.
    pub args: Vec<(String, String)>,
}

/// The presets file's contents.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Presets {
    pub global: Vec<(String, String)>,
    pub models: Vec<Preset>,
    /// Installed models left out, with the reason ("too large for this machine").
    pub skipped: Vec<(ModelId, String)>,
}

impl Presets {
    /// Presets for every installed catalog model that fits `hw`. The context
    /// is the configured one for the configured model, else the entry's
    /// default, shrunk to fit.
    pub fn build(catalog: &Catalog, store: &Store, hw: &Hardware, config: &Config) -> Presets {
        let kv = KvCacheType::default();
        let global = vec![
            ("jinja".to_owned(), "true".to_owned()),
            ("flash-attn".to_owned(), "on".to_owned()),
            ("parallel".to_owned(), "1".to_owned()),
            ("cache-type-k".to_owned(), kv.llama_name().to_owned()),
            ("cache-type-v".to_owned(), kv.llama_name().to_owned()),
            // Our placement is the plan: llama.cpp shouldn't second-guess it.
            ("fit".to_owned(), "off".to_owned()),
            ("stop-timeout".to_owned(), STOP_TIMEOUT_SECS.to_string()),
        ];
        let mut models = Vec::new();
        let mut skipped = Vec::new();
        for entry in &catalog.models {
            let Some(installed) = store.installed(entry) else { continue };
            let context = (config.model.id.as_ref() == Some(&entry.id)).then_some(config.model.context).flatten();
            let fit = fit::fit(entry, hw, FitOptions { context, kv, ..FitOptions::default() });
            if !fit.fits() {
                skipped.push((entry.id.clone(), fit.describe(hw)));
                continue;
            }
            models.push(Preset { id: entry.id.clone(), args: preset_args(entry, &installed, &fit, hw), fit });
        }
        Presets { global, models, skipped }
    }

    pub fn to_ini(&self) -> String {
        let mut s = String::new();
        s.push_str("; Written by tasuke-models from the catalog and the downloaded models.\n");
        s.push_str("; It is rewritten whenever the server starts: change config.toml instead.\n");
        s.push_str("version = 1\n\n[*]\n");
        for (k, v) in &self.global {
            let _ = writeln!(s, "{k} = {v}");
        }
        for p in &self.models {
            let _ = writeln!(s, "\n[{}]", p.id);
            for (k, v) in &p.args {
                let _ = writeln!(s, "{k} = {v}");
            }
        }
        s
    }
}

/// llama-server's arguments for `entry` placed as `fit`, as INI keys.
pub fn preset_args(entry: &Entry, installed: &Installed, fit: &Fit, hw: &Hardware) -> Vec<(String, String)> {
    let mut args: Vec<(String, String)> = Vec::new();
    let mut set = |k: &str, v: String| {
        if let Some(slot) = args.iter_mut().find(|(key, _)| key == k) {
            slot.1 = v;
        } else {
            args.push((k.to_owned(), v));
        }
    };
    set("model", installed.weights().display().to_string());
    if let Some(mmproj) = installed.mmproj() {
        set("mmproj", mmproj.display().to_string());
    }
    if let Some(ctx) = fit.context() {
        set("ctx-size", ctx.0.to_string());
    }
    match *fit {
        Fit::Gpu { .. } | Fit::Unified { .. } => set("n-gpu-layers", "999".into()),
        Fit::GpuWithExpertOffload { cpu_expert_layers, .. } => {
            set("n-gpu-layers", "999".into());
            set("n-cpu-moe", cpu_expert_layers.0.to_string());
            set("threads", hw.cpu.llama_threads().to_string());
        }
        Fit::Cpu { .. } => {
            set("n-gpu-layers", "0".into());
            set("device", "none".into());
            set("no-mmproj-offload", "true".into());
            set("threads", hw.cpu.llama_threads().to_string());
        }
        Fit::TooLarge { .. } => {}
    }
    if let Some(device) = fit.gpu().and_then(|g| llama_device_name(hw, g)) {
        set("device", device);
    }
    for (k, v) in cli_args_to_ini(&entry.args) {
        set(&k, v);
    }
    if matches!(fit, Fit::GpuWithExpertOffload { .. }) {
        for (k, v) in cli_args_to_ini(&entry.offload_args) {
            set(&k, v);
        }
    }
    // The fit planned room for the draft; a model downloaded before its draft
    // was catalogued runs without one.
    if let (Fit::Gpu { draft: true, .. }, Some(draft), Some(path)) = (fit, &entry.draft, installed.draft()) {
        set("spec-draft-model", path.display().to_string());
        set("spec-draft-ngl", "999".into());
        for (k, v) in cli_args_to_ini(&draft.args) {
            set(&k, v);
        }
    }
    for (field, value) in &entry.sampling {
        if let Some(key) = sampling_key(field) {
            set(key, toml_scalar(value));
        }
    }
    if !entry.template_kwargs.is_empty() {
        let json: serde_json::Map<String, serde_json::Value> = entry
            .template_kwargs
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(serde_json::Value::Null)))
            .collect();
        set("chat-template-kwargs", serde_json::Value::Object(json).to_string());
    }
    args
}

/// The llama.cpp device for a GPU, when there's more than one to choose from:
/// CUDA numbers NVIDIA GPUs in PCI order (the unit sets
/// `CUDA_DEVICE_ORDER=PCI_BUS_ID`), as nvidia-smi does.
fn llama_device_name(hw: &Hardware, gpu: crate::hardware::GpuIndex) -> Option<String> {
    let g = hw.gpu(gpu)?;
    let nvidia = hw.gpus.iter().filter(|g| g.vendor == GpuVendor::Nvidia).count();
    match g.vendor {
        GpuVendor::Nvidia if nvidia > 1 => g.device_index.map(|i| format!("CUDA{i}")),
        _ => None,
    }
}

/// Request sampling fields as llama-server's argument names.
fn sampling_key(field: &str) -> Option<&'static str> {
    Some(match field {
        "temperature" => "temp",
        "top_p" => "top-p",
        "top_k" => "top-k",
        "min_p" => "min-p",
        "presence_penalty" => "presence-penalty",
        "frequency_penalty" => "frequency-penalty",
        "repeat_penalty" => "repeat-penalty",
        _ => return None,
    })
}

fn toml_scalar(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// `["--spec-type", "draft-mtp", "-ctk", "q8_0", "--no-mmap"]` as INI pairs
/// (`spec-type = draft-mtp`, `ctk = q8_0`, `no-mmap = true`).
pub fn cli_args_to_ini(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let Some(key) = a.strip_prefix("--").or_else(|| a.strip_prefix('-')) else {
            i += 1;
            continue;
        };
        if let Some((k, v)) = key.split_once('=') {
            out.push((k.to_owned(), v.to_owned()));
            i += 1;
            continue;
        }
        let value = args.get(i + 1).filter(|v| !v.starts_with('-') || v.parse::<f64>().is_ok());
        match value {
            Some(v) => {
                out.push((key.to_owned(), v.clone()));
                i += 2;
            }
            None => {
                out.push((key.to_owned(), "true".to_owned()));
                i += 1;
            }
        }
    }
    out
}

/// systemd's view of the unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitState {
    /// `loaded`, `not-found`…
    pub load: String,
    /// `active`, `inactive`, `failed`, `activating`…
    pub active: String,
    pub sub: String,
}

impl UnitState {
    pub fn installed(&self) -> bool {
        self.load == "loaded"
    }

    pub fn running(&self) -> bool {
        self.active == "active" || self.active == "activating" || self.active == "reloading"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Loaded,
    Sleeping,
    Downloading,
    #[serde(untagged)]
    Other(String),
}

impl ModelStatus {
    fn parse(s: &str) -> ModelStatus {
        match s {
            "unloaded" => ModelStatus::Unloaded,
            "loading" => ModelStatus::Loading,
            "loaded" => ModelStatus::Loaded,
            "sleeping" => ModelStatus::Sleeping,
            "downloading" => ModelStatus::Downloading,
            other => ModelStatus::Other(other.to_owned()),
        }
    }
}

/// A model the router lists (`GET /models`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServedModel {
    pub id: String,
    pub status: ModelStatus,
    /// Its last load failed.
    #[serde(default)]
    pub failed: bool,
}

/// What `GET /props?model=…` says about a loaded model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Props {
    pub n_ctx: Option<ContextTokens>,
    pub is_sleeping: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Health {
    /// Serving.
    Ok,
    /// Up, still starting.
    Loading,
    /// Not answering: the reason.
    Down { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub url: String,
    pub unit: Option<UnitState>,
    pub health: Health,
    pub models: Vec<ServedModel>,
}

/// An HTTP client for the router, with the API key.
#[derive(Clone)]
pub struct Client {
    base: String,
    api_key: Option<String>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base).finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(base: impl Into<String>, api_key: Option<String>) -> Client {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(2)))
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        Client { base: base.into().trim_end_matches('/').to_owned(), api_key, agent }
    }

    /// `http://127.0.0.1:PORT`, without `/v1`.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// The OpenAI-compatible base, `…/v1`.
    pub fn openai_base(&self) -> String {
        format!("{}/v1", self.base)
    }

    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    fn get(&self, path: &str) -> Result<(u16, String), ServerError> {
        let url = format!("{}{path}", self.base);
        let mut req = self.agent.get(&url);
        if let Some(k) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        let mut resp = req.call().map_err(|e| ServerError::Http { url: url.clone(), detail: e.to_string() })?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().map_err(|e| ServerError::Http { url, detail: e.to_string() })?;
        Ok((status, body))
    }

    fn get_json(&self, path: &str) -> Result<serde_json::Value, ServerError> {
        let (status, body) = self.get(path)?;
        if status != 200 {
            return Err(ServerError::Http {
                url: format!("{}{path}", self.base),
                detail: format!("HTTP {status}: {body}"),
            });
        }
        serde_json::from_str(&body)
            .map_err(|e| ServerError::Http { url: format!("{}{path}", self.base), detail: e.to_string() })
    }

    pub fn health(&self) -> Health {
        match self.get("/health") {
            Ok((200, _)) => Health::Ok,
            Ok((503, _)) => Health::Loading,
            Ok((status, body)) => Health::Down { reason: format!("HTTP {status}: {}", body.trim()) },
            Err(e) => Health::Down { reason: e.to_string() },
        }
    }

    pub fn models(&self) -> Result<Vec<ServedModel>, ServerError> {
        let v = self.get_json("/models")?;
        Ok(parse_models(&v))
    }

    /// Doesn't load or wake the model.
    pub fn props(&self, model: &ModelId) -> Result<Props, ServerError> {
        let v = self.get_json(&format!("/props?model={}&autoload=false", model))?;
        Ok(Props {
            n_ctx: v
                .pointer("/default_generation_settings/n_ctx")
                .or_else(|| v.get("n_ctx"))
                .and_then(|n| n.as_u64())
                .map(|n| ContextTokens(n as u32)),
            is_sleeping: v.get("is_sleeping").and_then(|b| b.as_bool()).unwrap_or(false),
            model_path: v.get("model_path").and_then(|s| s.as_str()).map(str::to_owned),
        })
    }
}

pub(crate) fn parse_models(v: &serde_json::Value) -> Vec<ServedModel> {
    let empty = Vec::new();
    v.get("data")
        .and_then(|d| d.as_array())
        .unwrap_or(&empty)
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_owned();
            let status =
                m.pointer("/status/value").and_then(|s| s.as_str()).map_or(ModelStatus::Unloaded, ModelStatus::parse);
            let failed = m.pointer("/status/failed").and_then(|b| b.as_bool()).unwrap_or(false);
            Some(ServedModel { id, status, failed })
        })
        .collect()
}

/// Tasuke's model server: its presets, its unit, its HTTP API.
#[derive(Clone, Debug)]
pub struct Server {
    paths: Paths,
    port: Port,
    idle_minutes: u32,
}

impl Server {
    pub fn new(paths: &Paths, config: &Config) -> Server {
        Server { paths: paths.clone(), port: config.server.port, idle_minutes: config.server.idle_minutes }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The router's API key, if the unit (or [`Server::make_api_key`]) wrote one.
    pub fn api_key(&self) -> Option<String> {
        let text = fs::read_to_string(self.paths.api_key_file()).ok()?;
        let key = text.lines().find(|l| !l.trim().is_empty() && !l.starts_with('#'))?.trim().to_owned();
        Some(key)
    }

    pub fn client(&self) -> Client {
        Client::new(self.url(), self.api_key())
    }

    /// Write `models.ini` for the installed models; returns what it wrote.
    pub fn write_presets(
        &self,
        catalog: &Catalog,
        store: &Store,
        hw: &Hardware,
        config: &Config,
    ) -> Result<Presets, ServerError> {
        let presets = Presets::build(catalog, store, hw, config);
        let path = self.paths.presets_file();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("ini.tmp");
        fs::write(&tmp, presets.to_ini())?;
        fs::rename(&tmp, &path)?;
        Ok(presets)
    }

    /// A fresh random API key in `$XDG_RUNTIME_DIR/tasuke/api-key`, mode 0600
    /// (the unit's ExecStartPre). Keeps an existing key, so clients that read
    /// it keep working across restarts.
    pub fn make_api_key(&self) -> Result<PathBuf, ServerError> {
        let path = self.paths.api_key_file();
        if self.api_key().is_some() {
            return Ok(path);
        }
        let dir = path.parent().expect("a file in a directory");
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let mut bytes = [0u8; 32];
        fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let tmp = path.with_extension("tmp");
        let _ = fs::remove_file(&tmp);
        {
            use std::io::Write as _;
            let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
            writeln!(f, "{key}")?;
        }
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// The router's arguments (after the llama-server program name).
    pub fn router_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "--models-preset".into(),
            self.paths.presets_file().display().to_string(),
            "--models-max".into(),
            "1".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string(),
            "--api-key-file".into(),
            self.paths.api_key_file().display().to_string(),
            "--no-webui".into(),
        ];
        if self.idle_minutes > 0 {
            args.push("--sleep-idle-seconds".into());
            args.push((u64::from(self.idle_minutes) * 60).to_string());
        }
        args
    }

    /// llama-server as the router, ready to spawn or exec.
    pub fn router_command(&self, llama_server: &Path) -> Command {
        let mut cmd = Command::new(llama_server);
        cmd.args(self.router_args()).env("CUDA_DEVICE_ORDER", "PCI_BUS_ID").stdin(Stdio::null());
        cmd
    }

    fn systemctl(&self, verb: &'static str) -> Result<(), ServerError> {
        let out = Command::new("systemctl").args(["--user", verb, UNIT]).stdin(Stdio::null()).output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(ServerError::Systemctl { verb, detail: String::from_utf8_lossy(&out.stderr).trim().to_owned() })
        }
    }

    pub fn start(&self) -> Result<(), ServerError> {
        self.systemctl("start")
    }

    pub fn stop(&self) -> Result<(), ServerError> {
        self.systemctl("stop")
    }

    /// Restart (to apply new presets: a different model, context or download).
    pub fn restart(&self) -> Result<(), ServerError> {
        self.systemctl("restart")
    }

    /// `None` when there's no systemd user manager to ask.
    pub fn unit_state(&self) -> Option<UnitState> {
        let out = Command::new("systemctl")
            .args(["--user", "show", "-p", "LoadState,ActiveState,SubState", UNIT])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let props: BTreeMap<&str, &str> = text.lines().filter_map(|l| l.split_once('=')).collect();
        Some(UnitState {
            load: props.get("LoadState")?.to_string(),
            active: props.get("ActiveState")?.to_string(),
            sub: props.get("SubState").unwrap_or(&"").to_string(),
        })
    }

    pub fn status(&self) -> ServerStatus {
        let client = self.client();
        let health = client.health();
        let models = if health == Health::Ok { client.models().unwrap_or_default() } else { Vec::new() };
        ServerStatus { url: client.openai_base(), unit: self.unit_state(), health, models }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_args_become_ini_keys() {
        let args: Vec<String> = [
            "--spec-type",
            "draft-mtp",
            "--spec-draft-n-max",
            "2",
            "-ctk",
            "q8_0",
            "--no-mmap",
            "--temp",
            "-0.5",
            "--x=y",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            cli_args_to_ini(&args),
            [
                ("spec-type", "draft-mtp"),
                ("spec-draft-n-max", "2"),
                ("ctk", "q8_0"),
                ("no-mmap", "true"),
                ("temp", "-0.5"),
                ("x", "y")
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
    }

    #[test]
    fn the_draft_runs_when_planned_and_downloaded() {
        use crate::hardware::{Cpu, Gpu, GpuMemory, GpuVendor, Ram};
        use crate::units::{Bytes, BytesPerSecond};
        let catalog = Catalog::load_default();
        let entry = catalog.get(&ModelId::new("gemma-4-26b-a4b")).unwrap();
        let hw = Hardware {
            gpus: vec![Gpu {
                vendor: GpuVendor::Nvidia,
                name: "NVIDIA GeForce RTX 5090".into(),
                memory: GpuMemory::Dedicated { total: Bytes::gib(32), free: Bytes::gib(32) },
                drives_display: true,
                integrated: false,
                bandwidth: Some(BytesPerSecond::gb_per_s(1792.0)),
                pci: None,
                device_index: Some(0),
                compute_capability: None,
            }],
            ram: Ram { total: Bytes::gib(64), available: Bytes::gib(32) },
            cpu: Cpu { model: "AMD Ryzen".into(), physical_cores: 8, logical_cores: 16, ..Cpu::default() },
            notes: Vec::new(),
        };
        let root = std::env::temp_dir().join(format!("tasuke-draft-{}", std::process::id()));
        let store = Store::new(&root);
        fs::create_dir_all(store.model_dir(&entry.id)).unwrap();
        // Sparse stand-ins at the catalog's sizes, all but the draft.
        for f in entry.files.iter().filter(|f| f.kind != crate::FileKind::Draft) {
            fs::File::create(store.file_path(&entry.id, f)).unwrap().set_len(f.size.0).unwrap();
        }
        let fit = fit::fit(entry, &hw, FitOptions::default());
        assert!(matches!(fit, Fit::Gpu { draft: true, .. }), "{fit:?}");
        // Downloaded before its draft was catalogued: it runs without one.
        let installed = store.installed(entry).expect("installed without its draft");
        assert!(installed.draft().is_none());
        assert!(matches!(store.state(entry), crate::store::InstallState::Installed { .. }));
        assert!(!store.complete(entry), "a download still has the draft to fetch");
        let args = preset_args(entry, &installed, &fit, &hw);
        assert!(!args.iter().any(|(k, _)| k.starts_with("spec-")), "{args:?}");
        // With the draft downloaded too, it drafts.
        let draft = entry.draft_file().unwrap();
        fs::File::create(store.file_path(&entry.id, draft)).unwrap().set_len(draft.size.0).unwrap();
        assert!(store.complete(entry));
        let installed = store.installed(entry).unwrap();
        let args = preset_args(entry, &installed, &fit, &hw);
        let get = |k: &str| args.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("spec-draft-model"), installed.draft().and_then(|p| p.to_str()));
        assert_eq!(get("spec-type"), Some("draft-mtp"));
        assert_eq!(get("spec-draft-n-max"), Some("2"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn models_list_parses_router_statuses() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"data":[{"id":"a","status":{"value":"loaded"}},{"id":"b","status":{"value":"unloaded","failed":true,"exit_code":1}},
                {"id":"c","status":{"value":"sleeping"}},{"id":"d"}]}"#,
        )
        .unwrap();
        let m = parse_models(&v);
        assert_eq!(m[0].status, ModelStatus::Loaded);
        assert!(m[1].failed);
        assert_eq!(m[2].status, ModelStatus::Sleeping);
        assert_eq!(m[3].status, ModelStatus::Unloaded);
    }
}
