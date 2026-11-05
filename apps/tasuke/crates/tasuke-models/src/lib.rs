//! Tasuke's models: what the machine has, which catalog models fit it and
//! which to recommend, downloading them, and serving them with llama.cpp.
//!
//! Everything here is blocking and thread-based (no async runtime): a window
//! calls it from a worker thread, and long work (a download) runs on its own
//! thread and reports over a channel. See apps/tasuke/DESIGN.md ("Models").
//!
//! ```no_run
//! use tasuke_models::{Catalog, Hardware, Role, recommend, Preferences};
//! let hw = Hardware::detect();
//! let catalog = Catalog::load_default();
//! for r in recommend(&hw, &catalog, Role::Assistant, &Preferences::default()) {
//!     println!("{}: {}", r.id, r.why);
//! }
//! ```

pub mod catalog;
pub mod config;
pub mod download;
pub mod fit;
pub mod hardware;
pub mod recommend;
pub mod server;
pub mod store;
pub mod units;

pub use catalog::{Architecture, Catalog, Coords, Entry, FileKind, ModelFile, Role, Scores, Tally};
pub use config::{Config, Paths};
pub use download::{Download, DownloadError, DownloadEvent, DownloadHandle, DownloadOptions, Progress};
pub use fit::{Fit, FitOptions, KvCacheType, MemoryBasis, ModelFit};
pub use hardware::{Cpu, Gpu, GpuIndex, GpuMemory, GpuVendor, Hardware, Probe, Ram};
pub use recommend::{Preferences, Recommendation, recommend};
pub use server::{Client, Health, ModelStatus, Presets, ServedModel, Server, ServerError, ServerStatus, UnitState};
pub use store::{InstallState, Installed, Store};
pub use units::{Bytes, BytesPerSecond, BytesPerToken, ContextTokens, ModelId, Port, TokensPerSecond};
