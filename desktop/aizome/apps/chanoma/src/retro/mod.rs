//! Games, played through libretro cores: the emulators of RetroArch and
//! others, loaded as libraries and driven here, so there is no other
//! program's menu for a kid to get lost in.
//!
//! Cores that draw in software are what Chanoma runs (most 8-, 16- and
//! 32-bit systems'); those that need OpenGL or Vulkan (Nintendo 64,
//! Dreamcast and later) are left out.

pub mod audio;
pub mod ffi;
pub mod host;
pub mod session;
pub mod video;

pub use session::{CoreSource, Event, Launch, Session};

/// Rumble from a game: player, strong and weak motors (0–65535).
pub type Rumble = Box<dyn Fn(usize, u16, u16) + Send>;
