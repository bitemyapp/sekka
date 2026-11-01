//! What the engine plays, told by a file's name, and where to carry on a
//! film left part-way: one answer for every app that plays through it,
//! so Utsushi and Chanoma find the same files and resume them alike.

use std::path::Path;

/// Video files' extensions, lowercase.
pub const VIDEO_EXTENSIONS: &[&str] =
    &["mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "mpg", "mpeg", "ts", "m2ts", "mts", "ogv", "flv", "3gp"];

/// Audio files' extensions, lowercase.
pub const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wav", "aiff", "aif", "wv", "ape", "wma", "mka", "alac"];

fn extension(path: &Path) -> Option<String> {
    path.extension()?.to_str().map(str::to_ascii_lowercase)
}

/// Whether `path` is a video, by its extension.
pub fn is_video(path: &Path) -> bool {
    extension(path).is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.as_str()))
}

/// Whether `path` is audio, by its extension.
pub fn is_audio(path: &Path) -> bool {
    extension(path).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

/// Whether the engine plays `path`, by its extension.
pub fn is_playable(path: &Path) -> bool {
    is_video(path) || is_audio(path)
}

/// Shorter than this, a film starts from the beginning again.
const SHORT: f64 = 5.0 * 60.0;
/// Left sooner than this, it starts again too.
const BEGUN: f64 = 30.0;

/// Whether a film left at `position` of `duration` seconds was watched
/// to its end: within half a minute of it, or of a long film's last
/// twentieth (its credits). Of unknown length, never.
pub fn finished(position: f64, duration: f64) -> bool {
    duration > 0.0 && position >= duration - 30.0_f64.max(duration * 0.05)
}

/// Where to carry on a film left at `position` of `duration` seconds:
/// there, unless it's short, was barely begun, or was finished.
pub fn resume_point(position: f64, duration: f64) -> Option<f64> {
    (duration >= SHORT && position >= BEGUN && !finished(position, duration)).then_some(position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_told_by_their_extension_whatever_its_case() {
        assert!(is_video(Path::new("/a/B.MKV")) && is_video(Path::new("show/01.m2ts")));
        assert!(is_audio(Path::new("song.flac")) && !is_video(Path::new("song.flac")));
        assert!(is_playable(Path::new("x.Opus")));
        assert!(!is_playable(Path::new("notes.txt")) && !is_playable(Path::new("folder")));
    }

    #[test]
    fn a_film_carries_on_unless_short_barely_begun_or_finished() {
        let hour = 3600.0;
        assert_eq!(resume_point(600.0, hour), Some(600.0));
        assert_eq!(resume_point(20.0, hour), None, "barely begun");
        assert_eq!(resume_point(hour - 100.0, hour), None, "in the credits (the last twentieth)");
        assert_eq!(resume_point(120.0, 4.0 * 60.0), None, "short");
        assert_eq!(resume_point(120.0, 0.0), None, "of unknown length");
        assert!(finished(hour - 100.0, hour) && !finished(hour - 400.0, hour));
        assert!(finished(400.0, 420.0) && !finished(380.0, 420.0), "half a minute from a short one's end");
        assert!(!finished(400.0, 0.0));
    }
}
