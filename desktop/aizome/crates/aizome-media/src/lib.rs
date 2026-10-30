//! Aizome's playback engine: video and audio files (and streams FFmpeg
//! reads) played with as little work as the machine can do them with.
//!
//! - **Video** decodes on the GPU through VA-API where the codec allows,
//!   and each frame goes to the compositor as the decoder left it (a
//!   dmabuf, through the window's video sink: `sidestep_ui::video`), to
//!   be scanned out on a plane of its own where nothing covers it.
//!   Software decoding converts each frame once into shared memory.
//! - **Out of sight**, nothing is decoded: frames the compositor didn't
//!   show, or the window saying so, stop the video while audio plays on;
//!   back in sight, it catches up from the keyframe before.
//! - **Audio** goes to PipeWire in long graph cycles (music, a few a
//!   second), from a ring decoded ahead in bursts; pausing and seeking
//!   stay immediate whatever the cycle.
//! - **One clock**: what the speaker plays now, from PipeWire's timing,
//!   which the video follows.
//!
//! [`Player`] is the interface: commands in, [`Event`]s out, the position
//! read from its clock whenever it's wanted.

pub mod audio;
pub mod av;
pub mod clock;
pub mod pipewire;
pub mod player;
pub mod subs;
pub mod video;

pub use pipewire::StreamKind;
pub use player::{
    AudioChoice, Chapter, Event, FramePath, MediaInfo, Options, Pipeline, PlayState, Player, Stats, Track, VideoInfo,
    language_name,
};
pub use subs::{Cue, CueBody};
pub use video::Picture;
