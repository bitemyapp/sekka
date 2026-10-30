//! The primary selection, and programs reading and writing the clipboard
//! themselves: OSC 52's text, and Kitty's clipboard protocol (OSC 5522),
//! which names types, so programs can copy and paste pictures and rich text.
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
use hotaru_core::kitty_clipboard::{self, Location, Status};

/// Requests a pane may have waiting at once, and the bytes they may hold;
/// one Kitty write may hold as much.
const MAX_PENDING: usize = 16;
pub const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
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
    fn at(location: Location) -> Self {
        match location {
            Location::Primary => Self::Primary,
            Location::Standard => Self::Clipboard,
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
    /// The types a Kitty request reads or writes, to name in a question.
    pub types: Vec<String>,
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
        Self { text, bytes: bytes.len(), types: Vec::new() }
    }
    /// The types, as a question names them: a few, then how many more.
    pub fn named_types(&self) -> String {
        const SHOWN: usize = 4;
        let mut named = self.types.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
        if self.types.len() > SHOWN {
            named.push_str(&format!(" and {} more", self.types.len() - SHOWN));
        }
        named.chars().filter(|c| !c.is_control()).take(200).collect()
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
    /// A Kitty clipboard protocol read: types by name, the listing, or both.
    KittyRead(Arc<kitty_clipboard::Read>),
    /// A committed Kitty write, its types prepared to offer as sent.
    KittyWrite {
        write: Arc<kitty_clipboard::Write>,
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

impl Request {
    /// A Kitty request's answer with `status` alone; none for OSC 52's.
    pub fn status(&self, status: Status) -> Option<Arc<[u8]>> {
        let mut answer = Vec::new();
        match &self.kind {
            Kind::KittyRead(read) => read.status(status, &mut answer),
            Kind::KittyWrite { write, .. } => write.status(status, &mut answer),
            Kind::Read | Kind::Write { .. } => return None,
        }
        Some(answer.into())
    }
    /// What a refused request is answered with, if anything: OSC 52's
    /// reads an empty answer, Kitty's EPERM.
    pub fn refusal(&self) -> Option<Arc<[u8]>> {
        match self.kind {
            Kind::Read => Some(reply(self.tag, b"")),
            Kind::Write { .. } => None,
            _ => self.status(Status::Eperm),
        }
    }
    /// What a request nobody can take now is answered with: Kitty's hear
    /// EBUSY, to try again.
    pub fn busy(&self) -> Option<Arc<[u8]>> {
        match self.kind {
            Kind::Read | Kind::Write { .. } => self.refusal(),
            _ => self.status(Status::Ebusy),
        }
    }
}

/// A Kitty clipboard request on the PTY worker, a write's types prepared to
/// offer there. Past the pane's bounds, or a write of no type this desktop
/// can offer, it comes to the program's answer instead.
pub fn kitty(pane: u64, request: kitty_clipboard::Request, bounds: &Arc<Bounds>) -> Result<Request, Arc<[u8]>> {
    let mut answer = Vec::new();
    match request {
        kitty_clipboard::Request::Read(read) => {
            let Some(ticket) = Ticket::take(bounds, 0) else {
                read.status(Status::Ebusy, &mut answer);
                return Err(answer.into());
            };
            let target = Target::at(read.loc);
            Ok(Request { pane, tag: 0, target, kind: Kind::KittyRead(Arc::from(read)), _ticket: Arc::new(ticket) })
        }
        kitty_clipboard::Request::Write(write) => {
            let Some(ticket) = Ticket::take(bounds, write.size()) else {
                write.status(Status::Ebusy, &mut answer);
                return Err(answer.into());
            };
            // Names the desktop can't carry go; an empty write clears.
            let offered: Vec<_> = write
                .contents
                .iter()
                .filter(|(mime, _)| mime.len() <= 255 && mime.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
                .cloned()
                .collect();
            let data = if write.contents.is_empty() {
                Ok(PreparedClipboard::from_text_bytes(Arc::from(&[][..])))
            } else {
                PreparedClipboard::from_representations(offered)
            };
            let Ok(data) = data else {
                write.status(Status::Einval, &mut answer);
                return Err(answer.into());
            };
            let text = data.text_bytes().map(|bytes| Preview::of(&bytes).text).unwrap_or_default();
            let types = write.contents.iter().map(|(mime, _)| mime.clone()).collect();
            let preview = Preview { text, bytes: write.size(), types };
            let target = Target::at(write.loc);
            let kind = Kind::KittyWrite { write: Arc::from(write), data, preview };
            Ok(Request { pane, tag: 0, target, kind, _ticket: Arc::new(ticket) })
        }
    }
}

/// What a Kitty read will be answered with, captured as OSC 52's is: the
/// types asked for that the selection offers (a text type answering for
/// any other, as Ghostty serves them), at most [`READ_LIMIT`] together, and
/// the listing if it was asked for. The preview counts what it discloses.
fn capture_kitty(request: &kitty_clipboard::Read, read: &ClipboardRead) -> (Arc<[u8]>, Preview) {
    let deadline = std::time::Instant::now() + READ_TIMEOUT;
    let types = read.types();
    let mut served: Vec<(String, Arc<[u8]>)> = Vec::new();
    let mut left = READ_LIMIT;
    for mime in &request.mimes {
        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
        let data = if types.contains(mime) {
            read.read_type(mime, timeout, left)
        } else if kitty_clipboard::is_text_mime(mime) && types.iter().any(|t| kitty_clipboard::is_text_mime(t)) {
            read.read_bytes(timeout, left)
        } else {
            None
        };
        if let Some(data) = data.filter(|data| !data.is_empty()) {
            left -= data.len();
            served.push((mime.clone(), data));
        }
    }
    let available: Vec<String> =
        if request.list { types.into_iter().take(kitty_clipboard::MAX_LISTING_MIMES).collect() } else { Vec::new() };
    let mut answer = Vec::new();
    request.success(&available, &served, &mut answer);
    let text = served
        .iter()
        .find(|(mime, _)| kitty_clipboard::is_text_mime(mime))
        .map(|(_, data)| Preview::of(data).text)
        .unwrap_or_default();
    let preview = Preview {
        text,
        bytes: served.iter().map(|(_, data)| data.len()).sum(),
        types: served.into_iter().map(|(mime, _)| mime).collect(),
    };
    (answer.into(), preview)
}

/// The answer a request chose, under its question's token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Refuse,
    Allow,
    /// Allow, and the program's later requests with its password, until the
    /// terminal resets (Kitty's protocol only).
    Remember,
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
                if let Kind::KittyRead(kitty) = &request.kind {
                    let (answer, preview) = capture_kitty(kitty, &read);
                    tx.send(Msg::ClipboardCaptured(request, Some(answer), preview))
                } else {
                    // Nothing came (empty, unreadable, too large or too slow):
                    // there's no answer to ask about.
                    let bytes = read.read_bytes(READ_TIMEOUT, READ_LIMIT).filter(|bytes| !bytes.is_empty());
                    let preview = bytes.as_deref().map(Preview::of).unwrap_or_default();
                    let answer = bytes.map(|bytes| reply(request.tag, &bytes));
                    tx.send(Msg::ClipboardCaptured(request, answer, preview))
                }
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

    fn kitty_requests(input: &[u8]) -> Vec<kitty_clipboard::Request> {
        let mut terminal = hotaru_core::Terminal::new(10, 2, 0);
        terminal.feed(input);
        let events = terminal.take_events().into_iter();
        events
            .filter_map(|event| if let hotaru_core::TerminalEvent::KittyClipboard(r) = event { Some(r) } else { None })
            .collect()
    }

    #[test]
    fn kitty_writes_offer_what_was_sent_and_past_bounds_or_with_nothing_to_offer_are_answered() {
        let bounds = Arc::new(Bounds::default());
        // "hi" as text/plain, aliased as UTF8_STRING; PNG bytes under a name
        // that isn't ASCII ("text/é"), which the desktop can't carry.
        let input = concat!(
            "\x1b]5522;type=write:id=w\x1b\\",
            "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\",
            "\x1b]5522;type=wdata:mime=dGV4dC/DqQ==;iVBORw==\x1b\\",
            "\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;VVRGOF9TVFJJTkc=\x1b\\",
            "\x1b]5522;type=wdata\x1b\\",
        );
        let [request] = <[_; 1]>::try_from(kitty_requests(input.as_bytes())).unwrap();
        let Ok(Request { target, kind: Kind::KittyWrite { data, preview, .. }, .. }) = kitty(1, request, &bounds)
        else {
            panic!("a write");
        };
        assert_eq!((target, data.text(), data.data("UTF8_STRING")), (Target::Clipboard, Some("hi"), Some(&b"hi"[..])));
        assert_eq!(data.data("text/é"), None);
        assert_eq!((preview.text.as_str(), preview.bytes), ("hi", 6), "aliases count once");
        assert_eq!(preview.types, ["text/plain", "text/é", "UTF8_STRING"]);

        let only_unofferable = "\x1b]5522;type=write:id=x\x1b\\\x1b]5522;type=wdata:mime=dGV4dC/DqQ==;aGk=\x1b\\\x1b]5522;type=wdata\x1b\\";
        let [request] = <[_; 1]>::try_from(kitty_requests(only_unofferable.as_bytes())).unwrap();
        assert_eq!(&*kitty(1, request, &bounds).err().unwrap(), b"\x1b]5522;type=write:status=EINVAL:id=x\x1b\\");

        let clear = "\x1b]5522;type=write:loc=primary\x07\x1b]5522;type=wdata\x07";
        let [request] = <[_; 1]>::try_from(kitty_requests(clear.as_bytes())).unwrap();
        let Ok(cleared) = kitty(1, request, &bounds) else { panic!("an empty write clears") };
        assert_eq!(cleared.target, Target::Primary);
        assert_eq!(cleared.status(Status::Done).as_deref(), Some(&b"\x1b]5522;type=write:status=DONE\x07"[..]));
        drop(cleared);

        let read = "\x1b]5522;type=read:id=r;dGV4dC9wbGFpbg==\x1b\\".repeat(MAX_PENDING + 1);
        let mut held = Vec::new();
        let mut answers = Vec::new();
        for request in kitty_requests(read.as_bytes()) {
            match kitty(1, request, &bounds) {
                Ok(request) => held.push(request),
                Err(answer) => answers.push(answer),
            }
        }
        assert_eq!(held.len(), MAX_PENDING);
        assert_eq!(answers, [Arc::from(&b"\x1b]5522;type=read:status=EBUSY:id=r\x1b\\"[..])]);
        assert_eq!(held[0].refusal().as_deref(), Some(&b"\x1b]5522;type=read:status=EPERM:id=r\x1b\\"[..]));
    }

    #[test]
    fn kitty_reads_capture_named_types_text_under_any_text_name_and_the_listing() {
        let html: Arc<[u8]> = Arc::from(&b"<b>hi</b>"[..]);
        let data = PreparedClipboard::new(Some("hi".into()), vec![("text/html".into(), html)]).unwrap();
        let read = ClipboardRead::from_prepared(&data);
        // "text/plain text/html ." and an unknown type.
        let input = "\x1b]5522;type=read:id=r;dGV4dC9wbGFpbiB0ZXh0L2h0bWwgLg==\x1b\\";
        let [kitty_clipboard::Request::Read(request)] = <[_; 1]>::try_from(kitty_requests(input.as_bytes())).unwrap()
        else {
            panic!("a read");
        };
        let (answer, preview) = capture_kitty(&request, &read);
        let answer = String::from_utf8(answer.to_vec()).unwrap();
        assert!(
            answer
                .starts_with("\x1b]5522;type=read:status=OK:id=r\x1b\\\x1b]5522;type=read:status=DATA:id=r:mime=Lg==;")
        );
        assert!(answer.contains(":mime=dGV4dC9wbGFpbg==;aGk=\x1b\\"));
        assert!(answer.contains(":mime=dGV4dC9odG1s;PGI+aGk8L2I+\x1b\\"));
        assert!(answer.ends_with("\x1b]5522;type=read:status=DONE:id=r\x1b\\"));
        assert_eq!((preview.text.as_str(), preview.bytes, preview.types.len()), ("hi", 11, 2));
        // A text name the clipboard doesn't list still gets its text.
        let mut request = (*request).clone();
        request.mimes = vec!["STRING".into(), "image/png".into()];
        request.list = false;
        let only_html = ClipboardRead::from_prepared(
            &PreparedClipboard::new(None, vec![("text/html".into(), Arc::from(&b"x"[..]))]).unwrap(),
        );
        let (_, preview) = capture_kitty(&request, &only_html);
        assert_eq!(preview.bytes, 0, "no text there, and no picture");
        let (answer, preview) = capture_kitty(&request, &ClipboardRead::from_text(Some(Arc::from("t"))));
        assert_eq!(preview.types, ["STRING"]);
        assert!(String::from_utf8(answer.to_vec()).unwrap().contains(":mime=U1RSSU5H;dA==\x1b\\"));
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
        assert_eq!(Preview { bytes: 2400, ..Preview::default() }.size(), "2.4 KB");
        assert_eq!(Preview { bytes: 1, ..Preview::default() }.size(), "1 byte");
        let types = (0..6).map(|i| format!("x/{i}")).collect();
        assert_eq!(Preview { types, ..Preview::default() }.named_types(), "x/0, x/1, x/2, x/3 and 2 more");
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
