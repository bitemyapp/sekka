//! Tasuke's agent: the LLM client, the agent loop, its tools and the
//! command policy, skills, memory and sessions. UI-independent: the window
//! (`aizome-tasuke`) and the CLI (`tasuke ask`) drive it the same way.
//!
//! ```no_run
//! use tasuke_agent::{Agent, Decision, Event, Options, Session};
//!
//! let options = Options::from_config();
//! let session = Session::create(&options.data_dir).unwrap();
//! let turn = Agent::start(&session, "my sound stopped working", options);
//! for event in turn.events() {
//!     if let Event::ToolCall { id, needs, .. } = &event {
//!         if needs.asks() {
//!             turn.decide(id, Decision::Allow);
//!         }
//!     }
//! }
//! ```
//!
//! See apps/tasuke/DESIGN.md.

mod agent;
pub mod chat;
mod context;
mod desktop;
mod event;
mod exec;
mod ids;
mod llm;
pub mod memory;
mod options;
pub mod policy;
pub mod root;
pub mod session;
pub mod skills;
mod textcalls;
mod tools;
mod turn;
mod usage;

pub use agent::{Agent, TurnHandle};
pub use event::{
    Approval, Decision, Event, FinishReason, Level, MouseButton, ScreenPoint, ToolCall, ToolOutput, ToolStatus, Usage,
};
pub use ids::{CallId, NoteId, SessionId, StepBudget, Timestamp, Tokens, TurnNumber};
pub use memory::{Memory, MemoryError, Note};
pub use options::{Config, Coords, DEFAULT_PORT, DataDir, Options, read_api_key};
pub use session::{Entry, Session};
pub use skills::{Skill, Skills};
