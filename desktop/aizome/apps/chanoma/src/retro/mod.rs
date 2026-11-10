//! Games, played through libretro cores: the emulators of RetroArch and
//! others, loaded as libraries and driven here, so there is no other
//! program's menu for a kid to get lost in.
//!
//! Cores that draw in software (most 8-, 16- and 32-bit systems') hand
//! their frames over as pixels ([`video`]); those that draw with OpenGL
//! (the Dreamcast's, the PlayStation 2's) get a context of Chanoma's, and
//! their frames go to the compositor from the GPU ([`gl`]). Vulkan isn't
//! offered. [`bios`]: the consoles' BIOS images a grown-up keeps by
//! their games. [`guard`]: a core's C++ exceptions caught, so one let out
//! ends its game rather than Chanoma.

pub mod audio;
pub mod bios;
pub mod ffi;
pub mod gl;
pub mod guard;
pub mod host;
pub mod session;
pub mod video;

pub use session::{CoreSource, Event, Launch, Session};

/// Rumble from a game: player, strong and weak motors (0–65535).
pub type Rumble = Box<dyn Fn(usize, u16, u16) + Send>;
