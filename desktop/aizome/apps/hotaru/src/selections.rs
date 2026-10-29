//! The primary selection, and programs reading and writing the clipboard
//! themselves (OSC 52).
//!
//! Both run on a worker of their own beside the user's copy and paste
//! (`clipboard`), so neither waits behind the other or replaces its job. A
//! program's write is decoded and prepared on its PTY worker. A read is
//! captured here before anyone is asked, so the answer is what the
//! clipboard held when the program asked, byte for byte. Each request
//! holds a [`Ticket`] against its pane's bounds until it is answered or
//! dropped, so a flood of requests is refused rather than queued.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use aizome_kit::{ClipboardRead, MsgSender, PreparedClipboard};
use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::app::Msg;
use crate::clipboard::CopySource;

/// Requests a pane may have waiting at once, and the bytes they may hold.
const MAX_PENDING: usize = 16;
const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
/// The most a read answers with: 8 MiB once encoded, as much as a write
/// may carry.
pub const READ_LIMIT: usize = 6 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const PREVIEW_CHARS: usize = 120;
const QUEUE: usize = 16;

/// Ghostty's decoder: padding optional, nonzero bits past the end accepted.
const DECODER: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Which selection a request names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Clipboard,
    Primary,
}

impl Target {
    /// `p` and `s` name the primary selection; `c` and anything else the
    /// clipboard, as Ghostty routes writes. Reads follow the same rule, where
    /// Ghostty reads the clipboard for all three.
    pub fn of(kind: u8) -> Self {
        if matches!(kind, b'p' | b's') { Self::Primary } else { Self::Clipboard }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Clipboard => "the clipboard",
            Self::Primary => "the primary selection",
        }
    }
}

/// A pane's requests in waiting, counted against its bounds.
#[derive(Default)]
pub struct Bounds {
    count: AtomicUsize,
    bytes: AtomicUsize,
}

/// One request's share of its pane's bounds, given back when it goes.
pub struct Ticket {
    bounds: Arc<Bounds>,
    bytes: usize,
}

impl Ticket {
    fn take(bounds: &Arc<Bounds>, bytes: usize) -> Option<Self> {
        // Taken first and given back past the bounds, so racing takers can't
        // both fit in the last place.
        let ticket = Self { bounds: bounds.clone(), bytes };
        let count = bounds.count.fetch_add(1, Ordering::AcqRel);
        let held = bounds.bytes.fetch_add(bytes, Ordering::AcqRel);
        (count < MAX_PENDING && held.saturating_add(bytes) <= MAX_PENDING_BYTES).then_some(ticket)
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.bounds.count.fetch_sub(1, Ordering::AcqRel);
        self.bounds.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// A few words of what a request carries, made safe to show, and its size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub bytes: usize,
}

impl Preview {
    pub fn of(bytes: &[u8]) -> Self {
        let lossy = String::from_utf8_lossy(&bytes[..bytes.len().min(PREVIEW_CHARS * 4)]);
        let mut text: String =
            lossy.chars().take(PREVIEW_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
        let shown = text.trim_end().len();
        text.truncate(shown);
        if lossy.chars().count() > PREVIEW_CHARS || bytes.len() > PREVIEW_CHARS * 4 {
            text.push('…');
        }
        Self { text, bytes: bytes.len() }
    }
    /// Its size as people read one: "12 bytes", "3.4 KB", "1.2 MB".
    pub fn size(&self) -> String {
        match self.bytes {
            1 => "1 byte".into(),
            n if n < 1000 => format!("{n} bytes"),
            n if n < 1_000_000 => format!("{:.1} KB", n as f64 / 1e3),
            n => format!("{:.1} MB", n as f64 / 1e6),
        }
    }
}

#[derive(Clone)]
pub enum Kind {
    Read,
    /// The decoded bytes, offered exactly as sent.
    Write {
        data: PreparedClipboard,
        preview: Preview,
    },
}

#[derive(Clone)]
pub struct Request {
    pub pane: u64,
    /// The selection byte the program named, which an answer repeats.
    pub tag: u8,
    pub target: Target,
    pub kind: Kind,
    _ticket: Arc<Ticket>,
}

/// A request from an OSC 52 body's selection byte and data, on the PTY
/// worker: `?` asks to read, base64 asks to write. Undecodable data, and a
/// request past the pane's bounds, come to nothing.
pub fn request(pane: u64, tag: u8, data: &[u8], bounds: &Arc<Bounds>) -> Option<Request> {
    let target = Target::of(tag);
    if data == b"?" {
        let ticket = Arc::new(Ticket::take(bounds, 0)?);
        return Some(Request { pane, tag, target, kind: Kind::Read, _ticket: ticket });
    }
    // Held before decoding, so the bytes it makes are within bounds.
    let ticket = Arc::new(Ticket::take(bounds, data.len() / 4 * 3 + 3)?);
    let bytes = DECODER.decode(data).ok()?;
    let preview = Preview::of(&bytes);
    let data = PreparedClipboard::from_text_bytes(bytes.into());
    Some(Request { pane, tag, target, kind: Kind::Write { data, preview }, _ticket: ticket })
}

/// A request being asked about, under the token its answer comes with; a
/// read's with the answer it would send.
pub struct Question {
    pub token: u64,
    pub request: Request,
    pub reply: Option<Arc<[u8]>>,
}

/// The OSC 52 answer to a read: `bytes` encoded, under the program's tag.
/// Always ST-terminated, as Ghostty answers.
pub fn reply(tag: u8, bytes: &[u8]) -> Arc<[u8]> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut reply = Vec::with_capacity(encoded.len() + 8);
    reply.extend_from_slice(b"\x1b]52;");
    reply.push(tag);
    reply.push(b';');
    reply.extend_from_slice(encoded.as_bytes());
    reply.extend_from_slice(b"\x1b\\");
    reply.into()
}

pub enum Job {
    /// Capture what a read request will be answered with.
    Read { request: Request, read: ClipboardRead },
    /// Make a finished pointer selection the primary selection, as plain text.
    Primary { token: u64, pane: u64, source: CopySource, cancel: Arc<AtomicBool> },
}

pub struct Worker {
    jobs: mpsc::SyncSender<Job>,
}

impl Worker {
    pub fn new(tx: MsgSender<Msg>) -> io::Result<Self> {
        let (jobs, receiver) = mpsc::sync_channel(QUEUE);
        std::thread::Builder::new().name("hotaru-selections".into()).spawn(move || run(receiver, tx))?;
        Ok(Self { jobs })
    }
    /// Queue `job`: false when the queue is full, and it is dropped.
    pub fn submit(&self, job: Job) -> bool {
        self.jobs.try_send(job).is_ok()
    }
}

fn run(jobs: mpsc::Receiver<Job>, tx: MsgSender<Msg>) {
    // Ends once the app drops its Worker.
    for job in jobs {
        let sent = match job {
            Job::Read { request, read } => {
                // Nothing came (empty, unreadable, too large or too slow):
                // there's no answer to ask about.
                let bytes = read.read_bytes(READ_TIMEOUT, READ_LIMIT).filter(|bytes| !bytes.is_empty());
                let preview = bytes.as_deref().map(Preview::of).unwrap_or_default();
                let answer = bytes.map(|bytes| reply(request.tag, &bytes));
                tx.send(Msg::ClipboardCaptured(request, answer, preview))
            }
            Job::Primary { token, pane, source, cancel } => {
                match crate::clipboard::prepare_copy(token, pane, source, crate::settings::CopyFormat::Plain, &cancel) {
                    Some(Msg::CopyReady(token, _, data)) => tx.send(Msg::PrimaryReady(token, data)),
                    _ => true,
                }
            }
        };
        if !sent {
            return;
        }
    }
}

/// Text to paste for dropped URLs: files as their paths, quoted for the
/// shell and separated by spaces, as Ghostty pastes them; other URLs as
/// they are.
pub fn dropped(urls: &[String]) -> String {
    let words: Vec<String> =
        urls.iter().map(|url| file_path(url).map_or_else(|| url.clone(), |path| quoted(&path))).collect();
    words.join(" ")
}

/// A local `file:` URI's path, percent-decoded.
fn file_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let path = if rest.starts_with('/') { rest } else { rest.strip_prefix("localhost")? };
    let mut bytes = Vec::with_capacity(path.len());
    let mut source = path.bytes();
    while let Some(byte) = source.next() {
        bytes.push(if byte == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|v| v as u8);
            hex(source.next()?)? * 16 + hex(source.next()?)?
        } else {
            byte
        });
    }
    // Paths with bytes that aren't text paste as the URI they came as.
    String::from_utf8(bytes).ok().filter(|path| !path.chars().any(char::is_control))
}

/// `text` as one shell word: unchanged when it is plainly safe, otherwise in
/// single quotes.
fn quoted(text: &str) -> String {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-+=.,:/@%".contains(&b)) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_decode_leniently_route_by_tag_and_stay_within_bounds() {
        let bounds = Arc::new(Bounds::default());
        let Some(Request { target, kind: Kind::Write { data, preview }, .. }) = request(1, b'c', b"aGk", &bounds)
        else {
            panic!("unpadded base64 is a write");
        };
        assert_eq!(
            (target, data.text_bytes().as_deref(), preview.text.as_str()),
            (Target::Clipboard, Some(&b"hi"[..]), "hi")
        );
        // Bytes that aren't UTF-8 are offered as sent.
        let Some(Request { kind: Kind::Write { data, .. }, .. }) = request(1, b'c', b"YQBi/w==", &bounds) else {
            panic!("binary write");
        };
        assert_eq!(data.text_bytes().as_deref(), Some(&b"a\0b\xff"[..]));
        assert_eq!(request(1, b'p', b"?", &bounds).map(|r| r.target), Some(Target::Primary));
        assert_eq!(request(1, b's', b"?", &bounds).map(|r| r.target), Some(Target::Primary));
        assert_eq!(request(1, 0xff, b"?", &bounds).map(|r| r.target), Some(Target::Clipboard));
        assert!(request(1, b'c', b"not base64!", &bounds).is_none());
        let held: Vec<_> = (0..MAX_PENDING).map_while(|_| request(1, b'c', b"?", &bounds)).collect();
        assert_eq!(held.len(), MAX_PENDING);
        assert!(request(1, b'c', b"?", &bounds).is_none(), "a full pane refuses");
        drop(held);
        assert!(request(1, b'c', b"?", &bounds).is_some(), "answered requests free their place");
    }

    #[test]
    fn replies_carry_the_tag_and_exact_bytes() {
        assert_eq!(&*reply(b'c', b"hi"), b"\x1b]52;c;aGk=\x1b\\");
        assert_eq!(&*reply(b'p', b""), b"\x1b]52;p;\x1b\\");
        assert_eq!(&*reply(b'c', b"a\0b\xff"), b"\x1b]52;c;YQBi/w==\x1b\\");
    }

    #[test]
    fn previews_are_short_single_lines_with_people_sized_sizes() {
        let preview = Preview::of(b"line one\nline two\x1b[31m");
        assert_eq!(preview.text, "line one line two [31m");
        assert_eq!(Preview::of(&[b'x'; 500]).text.chars().count(), PREVIEW_CHARS + 1);
        assert_eq!(Preview { text: String::new(), bytes: 2400 }.size(), "2.4 KB");
        assert_eq!(Preview { text: String::new(), bytes: 1 }.size(), "1 byte");
    }

    #[test]
    fn dropped_files_paste_as_quoted_paths_and_links_as_themselves() {
        let urls = [
            "file:///home/me/notes.md",
            "file:///tmp/a%20b/it's",
            "https://example.test/x",
            "file://localhost/etc/hosts",
        ];
        let urls: Vec<String> = urls.iter().map(|s| s.to_string()).collect();
        assert_eq!(dropped(&urls), r"/home/me/notes.md '/tmp/a b/it'\''s' https://example.test/x /etc/hosts");
        assert_eq!(dropped(&["file://elsewhere/x".into()]), "file://elsewhere/x");
    }
}
