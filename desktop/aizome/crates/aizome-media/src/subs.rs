//! Subtitles as cues: text (SubRip, ASS/SSA, WebVTT, MP4's tx3g), shown
//! as plain lines by the interface in its own type, or pictures (Blu-ray
//! PGS, DVD and DVB subtitles), shown over the picture where they were
//! placed. Embedded tracks are decoded as the demuxer reads them, a few
//! seconds ahead; a subtitle file beside the video is read whole when it
//! opens.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::av::{self, Decoder, Input, Packet, ff};

/// A subtitle: when it shows, and what.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub id: u64,
    pub start: f64,
    /// Until the next cue of its track, if it doesn't say (a picture
    /// subtitle ends when the next one, often empty, comes).
    pub end: f64,
    pub body: CueBody,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CueBody {
    /// Lines of text, with `\n` between them; italic if the whole cue is.
    Text { text: String, italic: bool, top: bool },
    /// A picture, RGBA (straight alpha), `w` × `h`, at `x`, `y` of a
    /// video `frame` big.
    Picture { rgba: Arc<Vec<u8>>, w: u32, h: u32, x: i32, y: i32, frame: (u32, u32) },
    /// Clears what its track shows (and ends the cue before).
    Clear,
}

static IDS: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    IDS.fetch_add(1, Ordering::Relaxed)
}

/// Text from an ASS event: its last field, without override blocks
/// (`{\i1}`), with `\N` as line breaks; and whether it's italic
/// throughout, and placed at the top (`\an7`–`\an9`).
pub fn ass_text(ass: &str) -> (String, bool, bool) {
    // "ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text"
    // in decoded events (8 commas before the text); a "Dialogue:" line
    // from a file has "Layer,Start,End,Style,..." (9).
    let body = if let Some(rest) = ass.strip_prefix("Dialogue:") {
        rest.splitn(10, ',').nth(9).unwrap_or("")
    } else {
        ass.splitn(9, ',').nth(8).unwrap_or(ass)
    };
    let mut out = String::new();
    let mut italic_all = false;
    let mut top = false;
    let mut in_tag = false;
    let mut tag = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if in_tag {
            if c == '}' {
                in_tag = false;
                if tag.contains("\\i1") && out.trim().is_empty() {
                    italic_all = true;
                }
                if ["\\an7", "\\an8", "\\an9"].iter().any(|t| tag.contains(t)) {
                    top = true;
                }
                tag.clear();
            } else {
                tag.push(c);
            }
            continue;
        }
        match c {
            '{' => in_tag = true,
            '\\' => match chars.peek() {
                Some('N') | Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('h') => {
                    chars.next();
                    out.push('\u{a0}');
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    let text = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    (text, italic_all, top)
}

/// Text from SubRip-like markup: tags (`<i>`, `<font …>`) dropped.
pub fn plain_text(s: &str) -> (String, bool) {
    let italic = s.trim_start().starts_with("<i>") && s.trim_end().ends_with("</i>");
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let text = out.replace("\r\n", "\n").lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    (text, italic)
}

/// Decode a subtitle packet into cues; `frame` is the video's size, for
/// pictures that don't say theirs.
pub fn decode(dec: &mut Decoder, pkt: &Packet, frame: (u32, u32)) -> Vec<Cue> {
    let tb = dec.time_base;
    let p = pkt.get();
    let Some(pts) = av::seconds(if p.pts != ff::AV_NOPTS_VALUE { p.pts } else { p.dts }, tb) else {
        return Vec::new();
    };
    let pkt_end = av::seconds(p.duration, tb).filter(|d| *d > 0.0).map(|d| pts + d);
    let mut sub: ff::AVSubtitle = unsafe { std::mem::zeroed() };
    let mut got = 0;
    // SAFETY: valid decoder and packet; the subtitle is freed below.
    let r = unsafe { ff::avcodec_decode_subtitle2(dec.ctx, &mut sub, &mut got, pkt.0) };
    if r < 0 || got == 0 {
        return Vec::new();
    }
    let start = pts + f64::from(sub.start_display_time) / 1000.0;
    let end = if sub.end_display_time > 0 && sub.end_display_time != u32::MAX {
        pts + f64::from(sub.end_display_time) / 1000.0
    } else {
        pkt_end.unwrap_or(f64::INFINITY)
    };
    // The decoder's frame size, for pictures.
    let (fw, fh) = {
        let c = dec.get();
        if c.width > 0 && c.height > 0 { (c.width as u32, c.height as u32) } else { frame }
    };
    let mut cues = Vec::new();
    // SAFETY: num_rects rectangles, each valid until avsubtitle_free.
    unsafe {
        if sub.num_rects == 0 {
            cues.push(Cue { id: next_id(), start, end: start, body: CueBody::Clear });
        }
        let rects = if sub.rects.is_null() { &[][..] } else { std::slice::from_raw_parts(sub.rects, sub.num_rects as usize) };
        for &r in rects {
            let r = &*r;
            let body = match r.type_ {
                ff::AVSubtitleType::SUBTITLE_ASS => {
                    let (text, italic, top) = ass_text(&av::text(r.ass).unwrap_or_default());
                    (!text.is_empty()).then_some(CueBody::Text { text, italic, top })
                }
                ff::AVSubtitleType::SUBTITLE_TEXT => {
                    let (text, italic) = plain_text(&av::text(r.text).unwrap_or_default());
                    (!text.is_empty()).then_some(CueBody::Text { text, italic, top: false })
                }
                ff::AVSubtitleType::SUBTITLE_BITMAP => picture(r).map(|(rgba, w, h)| CueBody::Picture {
                    rgba: Arc::new(rgba),
                    w,
                    h,
                    x: r.x,
                    y: r.y,
                    frame: (fw, fh),
                }),
                _ => None,
            };
            if let Some(body) = body {
                cues.push(Cue { id: next_id(), start, end, body });
            }
        }
        ff::avsubtitle_free(&mut sub);
    }
    cues
}

/// A palette picture's pixels, RGBA.
unsafe fn picture(r: &ff::AVSubtitleRect) -> Option<(Vec<u8>, u32, u32)> {
    if r.w <= 0 || r.h <= 0 || r.data[0].is_null() || r.data[1].is_null() {
        return None;
    }
    let (w, h) = (r.w as usize, r.h as usize);
    // SAFETY: the palette is 256 32-bit ARGB entries in native order; the
    // indices are h rows of linesize bytes.
    let palette = unsafe { std::slice::from_raw_parts(r.data[1] as *const u32, 256) };
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let row = unsafe { std::slice::from_raw_parts(r.data[0].add(y * r.linesize[0] as usize), w) };
        for (x, &i) in row.iter().enumerate() {
            let argb = palette[usize::from(i)];
            let o = (y * w + x) * 4;
            rgba[o] = (argb >> 16) as u8;
            rgba[o + 1] = (argb >> 8) as u8;
            rgba[o + 2] = argb as u8;
            rgba[o + 3] = (argb >> 24) as u8;
        }
    }
    Some((rgba, w as u32, h as u32))
}

/// Subtitle files beside a video: the same name (`film.srt`,
/// `film.en.srt`), in a format FFmpeg reads.
pub fn beside(video: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            matches!(ext.as_deref(), Some("srt" | "ass" | "ssa" | "vtt" | "sub"))
                && (name.starts_with(&format!("{stem}.")))
        })
        .collect();
    found.sort();
    found
}

/// The language a subtitle file's name gives (`film.en.srt`: "en").
pub fn file_language(path: &Path, video: &Path) -> Option<String> {
    let stem = video.file_stem()?.to_str()?;
    let name = path.file_stem()?.to_str()?;
    let lang = name.strip_prefix(stem)?.strip_prefix('.')?;
    (!lang.is_empty() && lang.len() <= 8).then(|| lang.to_owned())
}

/// Read a subtitle file's cues, in order.
pub fn read_file(path: &Path) -> Result<Vec<Cue>, String> {
    let mut input = Input::open(&path.to_string_lossy()).map_err(|e| e.to_string())?;
    let Some(index) = input.streams().iter().position(|&s| av::media_type(unsafe { &*s }) == ff::AVMediaType::AVMEDIA_TYPE_SUBTITLE)
    else {
        return Err("no subtitles in it".into());
    };
    let mut dec = Decoder::open(input.stream(index).ok_or("no stream")?, |_| {}).map_err(|e| e.to_string())?;
    let mut pkt = Packet::new();
    let mut cues = Vec::new();
    while input.read(&mut pkt).map_err(|e| e.to_string())? {
        if pkt.get().stream_index as usize == index {
            cues.extend(decode(&mut dec, &pkt, (0, 0)));
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

/// Cues in order, with open-ended ones ending at the next of the track
/// and clearing cues folded into the ends they make.
pub fn settle(cues: &mut Vec<Cue>) {
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    for i in 0..cues.len() {
        if cues[i].end.is_infinite()
            && let Some(next) = cues[i + 1..].iter().find(|c| c.start > cues[i].start)
        {
            cues[i].end = next.start;
        }
    }
    cues.retain(|c| c.body != CueBody::Clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ass_events_lose_their_fields_and_tags() {
        let (t, italic, top) = ass_text("0,0,Default,,0,0,0,,{\\i1}Hello,\\Nworld{\\i0}");
        assert_eq!(t, "Hello,\nworld");
        assert!(italic);
        assert!(!top);
        let (t, _, top) = ass_text("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\an8}Sign\\hhere");
        assert_eq!(t, "Sign\u{a0}here");
        assert!(top);
    }

    #[test]
    fn subrip_markup_goes() {
        assert_eq!(plain_text("<i>Quietly</i>"), ("Quietly".into(), true));
        assert_eq!(plain_text("<font color=\"red\">A</font>\r\nB "), ("A\nB".into(), false));
    }

    #[test]
    fn open_cues_end_at_the_next() {
        let text = |s: &str| CueBody::Text { text: s.into(), italic: false, top: false };
        let mut cues = vec![
            Cue { id: 1, start: 1.0, end: f64::INFINITY, body: text("a") },
            Cue { id: 2, start: 3.0, end: 3.0, body: CueBody::Clear },
            Cue { id: 3, start: 5.0, end: 6.0, body: text("b") },
        ];
        settle(&mut cues);
        assert_eq!(cues.len(), 2);
        assert_eq!((cues[0].start, cues[0].end), (1.0, 3.0));
    }

    #[test]
    fn languages_come_from_names() {
        let v = Path::new("/m/Film (2024).mkv");
        assert_eq!(file_language(Path::new("/m/Film (2024).en.srt"), v).as_deref(), Some("en"));
        assert_eq!(file_language(Path::new("/m/Film (2024).srt"), v), None);
    }
}
