//! Local synchronized lyrics. LRC's positive offset advances the words;
//! the final line ends at the media duration (or after five seconds if unknown).
//! Metadata and malformed timestamps are ignored, never interpreted as lyrics.

use std::io::Read;
use std::path::{Path, PathBuf};

use aizome_media::{Cue, CueBody};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_CUES: usize = 20_000;

pub fn beside(media: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (media.parent(), media.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let prefix = format!("{stem}.");
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("lrc"))
                && path.file_name().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with(&prefix))
                && path.is_file()
        })
        .take(64)
        .collect();
    paths.sort();
    paths
}

pub fn load(path: &Path, duration: f64) -> Result<Vec<Cue>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open lyrics: {e}"))?;
    let mut text = String::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|e| format!("Cannot read lyrics as UTF-8: {e}"))?;
    parse(&text, duration)
}

pub fn parse(text: &str, duration: f64) -> Result<Vec<Cue>, String> {
    if text.len() > MAX_BYTES {
        return Err("Lyrics exceed the 1 MiB limit. Choose a smaller LRC file.".into());
    }
    let mut offset = 0.0;
    let mut lines = Vec::new();
    let mut expanded_bytes = 0usize;
    for line in text.trim_start_matches('\u{feff}').lines() {
        if line.len() > 4096 {
            continue;
        }
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some((tag, after)) = tag.split_once(']') else { break };
            if let Some(value) = tag.strip_prefix("offset:") {
                if let Ok(ms) = value.trim().parse::<i64>() {
                    offset = ms as f64 / 1000.0;
                }
            } else if let Some(time) = timestamp(tag) {
                times.push(time);
            }
            rest = after.trim_start();
        }
        if lines.len() + times.len() > MAX_CUES {
            return Err("Lyrics contain too many timestamps (limit: 20,000).".into());
        }
        for time in times {
            expanded_bytes += rest.len();
            if expanded_bytes > 4 * MAX_BYTES {
                return Err("Expanded lyrics exceed the 4 MiB limit. Choose a smaller LRC file.".into());
            }
            lines.push((time, rest.trim().to_owned()));
        }
    }
    for (time, _) in &mut lines {
        *time -= offset;
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cues = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = lines[i].0;
        let next = lines[i..].partition_point(|(time, _)| *time == start) + i;
        let end = lines
            .get(next)
            .map_or_else(|| if duration.is_finite() && duration > 0.0 { duration } else { start + 5.0 }, |line| line.0);
        let end = if duration.is_finite() && duration > 0.0 { end.min(duration) } else { end };
        for (_, words) in &mut lines[i..next] {
            // An empty timestamp ends the preceding line without drawing a box.
            if !words.is_empty() && end > start.max(0.0) {
                cues.push(Cue {
                    id: cues.len() as u64 + 1,
                    start: start.max(0.0),
                    end,
                    body: CueBody::Text { text: std::mem::take(words), italic: false, top: false },
                });
            }
        }
        i = next;
    }
    if cues.is_empty() {
        return Err("No timed lyrics found. Choose a UTF-8 LRC file with [mm:ss.xx] timestamps.".into());
    }
    Ok(cues)
}

fn timestamp(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let minutes = minutes.parse::<u32>().ok()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if whole.len() != 2
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 3
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let seconds = whole.parse::<u32>().ok()?;
    if seconds >= 60 {
        return None;
    }
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        fraction.parse::<u32>().ok()? as f64 / 10_f64.powi(fraction.len() as i32)
    };
    Some(f64::from(minutes) * 60.0 + f64::from(seconds) + fraction)
}
