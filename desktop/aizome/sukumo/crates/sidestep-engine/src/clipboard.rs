//! The system clipboard (the Wayland selection), between the main thread,
//! where `NSPasteboard` lives, and the render thread, which owns the
//! wl_data_device.
//!
//! Writing is asynchronous and never waits: the main thread keeps what it
//! wrote and hands the render thread a copy to offer, and the render thread
//! answers other clients' requests for it from its event loop.
//!
//! Reading can't be synchronous on Wayland: the data comes through a pipe
//! from whichever client owns the selection. So once the program has used
//! the general pasteboard, the render thread reads ahead: whenever another
//! client offers a selection (compositors offer the current one again each
//! time a window of ours gets the keyboard), it reads the text, which is
//! what programs read nearly always, up to [`READ_AHEAD_LIMIT`], and keeps
//! it here. `stringForType:` then answers at once. If the text is still on
//! its way, the main thread waits for it until [`READ_TIMEOUT`] after the
//! selection arrived, and no longer: a client that never answers costs one
//! wait, not one per call. Other types, and text too long to read ahead,
//! are read when asked for, waiting for as long as data keeps coming and
//! until [`READ_TIMEOUT`] passes without any: a large image that streams in
//! is read whole, and a client that never answers costs [`READ_TIMEOUT`],
//! once. Every request also has an absolute [`TRANSFER_TIMEOUT`] cap, even
//! while bytes continue arriving. Native UI callers use [`TextRead`] on a
//! worker with a potentially shorter deadline and an explicit size bound.
//!
//! A counter shared by both threads plays `changeCount`: the main thread
//! bumps it when the program clears the pasteboard, and the render thread
//! when another client's selection brings something new. An offer that
//! only repeats the last one (same types, same text), as compositors send
//! on a change of focus, isn't news: what was read of the one before, and
//! the pasteboard items made of it, stay good. A write of ours is current
//! while the counter hasn't moved since.
//!
//! What another client offers is read once per change: answers are kept
//! until the next one, so a program reading a list of files item by item
//! (or twice) makes one read, and a type that didn't come in time isn't
//! waited for again. The drag pasteboard is served the same way from the
//! drag and drop offer under the pointer ([`Source::Drag`]): each drag that
//! enters a window is a change, its URL list comes read with it, and the
//! rest of its data is read only when the program asks for it, from that
//! drag's own offer (none once the drag is over).
//!
//! Data the program promised (`declareTypes:owner:`, item data providers)
//! is offered by type; when another client asks for it, the render thread
//! asks the main thread, which asks the owner (see
//! `pasteboard::provide_for_render`) and hands the bytes back.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::protocol::ToRender;

/// How long the main thread waits for the selection's data before giving
/// up: for a read it asked for, since the read began or data last came; for
/// the text read ahead, since the selection arrived.
pub const READ_TIMEOUT: Duration = Duration::from_millis(200);

/// Text (or a drag's URL list) longer than this isn't read ahead; it's
/// read if a program asks.
pub const READ_AHEAD_LIMIT: usize = 1 << 20;

/// The most a read takes: a longer selection reads as nothing.
pub const READ_LIMIT: usize = 256 << 20;

/// A transfer cannot keep a render-loop pipe alive forever by trickling bytes.
/// Worker callers may impose a shorter absolute deadline.
pub const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

/// Selection identity changes immediately on a new offer or a local write.
/// The pasteboard generation alone changes only after read-ahead completes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardIdentity {
    offer: u64,
    local_write: u64,
}

/// A small, Send snapshot. `read` belongs on a worker; snapshot creation and
/// cancellation never wait for a clipboard peer or copy the selected text.
#[derive(Clone)]
pub struct TextRead {
    source: Source,
    identity: ClipboardIdentity,
    cancelled: Arc<AtomicBool>,
}

impl TextRead {
    pub fn identity(&self) -> ClipboardIdentity {
        self.identity
    }

    pub fn is_current(&self) -> bool {
        !self.cancelled.load(Ordering::Relaxed) && shared_for(self.source).identity() == self.identity
    }

    pub fn cancel(&self) {
        shared_for(self.source).cancel_read(&self.cancelled);
    }

    pub fn read(&self, timeout: Duration, maximum: usize) -> Option<Arc<str>> {
        let deadline = Instant::now().checked_add(timeout.min(TRANSFER_TIMEOUT))?;
        shared_for(self.source).read_text_snapshot(
            self.identity,
            self.cancelled.clone(),
            deadline,
            maximum.min(READ_LIMIT),
        )
    }

    /// The text as offered, byte for byte, whether or not it is UTF-8 (a
    /// terminal program reading the clipboard wants it unchanged). Belongs
    /// on a worker, as [`read`](Self::read) does.
    pub fn read_bytes(&self, timeout: Duration, maximum: usize) -> Option<Arc<[u8]>> {
        let deadline = Instant::now().checked_add(timeout.min(TRANSFER_TIMEOUT))?;
        shared_for(self.source).read_bytes_snapshot(
            self.identity,
            None,
            self.cancelled.clone(),
            deadline,
            maximum.min(READ_LIMIT),
        )
    }

    /// The types the selection offers (this program's own internal ones
    /// left out), while it is still the one this read was taken of: what a
    /// terminal program asking for the clipboard's types is told.
    pub fn mimes(&self) -> Option<Vec<String>> {
        shared_for(self.source).mimes_snapshot(self.identity, &self.cancelled)
    }

    /// The selection as `mime`, byte for byte, if it offers that type.
    /// Belongs on a worker, as [`read`](Self::read) does.
    pub fn read_type(&self, mime: &str, timeout: Duration, maximum: usize) -> Option<Arc<[u8]>> {
        let deadline = Instant::now().checked_add(timeout.min(TRANSFER_TIMEOUT))?;
        shared_for(self.source).read_bytes_snapshot(
            self.identity,
            Some(mime),
            self.cancelled.clone(),
            deadline,
            maximum.min(READ_LIMIT),
        )
    }
}

/// Types' data by MIME type, as offered to other clients. `None` is data
/// the program promised: the main thread makes it when a client asks.
pub struct Contents {
    pub items: Vec<(String, Option<Arc<[u8]>>)>,
}

impl Contents {
    /// The data offered as `mime`: `Some(None)` if it's promised.
    pub fn data(&self, mime: &str) -> Option<Option<Arc<[u8]>>> {
        self.items.iter().find(|(m, _)| m == mime).map(|(_, d)| d.clone())
    }
}

/// Which offer of another client's a pasteboard reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The selection: the general pasteboard.
    Selection,
    /// The drag and drop offer under the pointer: the drag pasteboard.
    Drag,
    /// What was last selected, which a middle click pastes: the primary
    /// selection, where the compositor offers one.
    Primary,
}

/// The MIME types that mark this process's own selections and drags.
pub const OWNER_PREFIX: &str = "application/x-sidestep-owner";
pub const URI_LIST: &str = "text/uri-list";
pub const GNOME_FILES: &str = "x-special/gnome-copied-files";
pub const MOZ_URL: &str = "text/x-moz-url";

/// The MIME types text is offered and read as, best first.
pub const TEXT_MIMES: &[&str] = &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"];

/// Where another program's URLs are, among the MIME types it offers: the
/// best of them, if any.
pub fn url_mime(mimes: &[String]) -> Option<&'static str> {
    [URI_LIST, GNOME_FILES, MOZ_URL].into_iter().find(|m| mimes.iter().any(|o| o == m))
}

/// The MIME type that marks a selection as ours, so the render thread
/// doesn't read back what this process offers.
pub fn owner_mime() -> &'static str {
    static MIME: OnceLock<String> = OnceLock::new();
    MIME.get_or_init(|| format!("{};pid={}", OWNER_PREFIX, std::process::id()))
}

/// The best text type among `mimes`.
pub fn text_mime(mimes: &[String]) -> Option<String> {
    TEXT_MIMES.iter().find(|m| mimes.iter().any(|o| o == *m)).map(|m| m.to_string())
}

/// Where the text of another client's selection stands.
enum Text {
    /// Not read: nobody had asked when it came, or it's too long to read
    /// ahead.
    Unread,
    /// On its way, for readers to wait for until `deadline`. `before` is
    /// the last selection's text when the types are the same, to tell
    /// whether this one is news.
    Reading { offer: u64, deadline: Instant, before: Option<Option<Arc<str>>> },
    /// Read: `None` if there's none, or it couldn't be read.
    Read(Option<Arc<str>>),
}

/// What reading the text ahead came to.
pub enum ReadAhead {
    Text(Option<String>),
    TooLong,
    Failed,
}

/// What the render thread knows of a selection another client owns.
struct Foreign {
    /// Counts other clients' selections; a read ahead answers for one.
    offer: u64,
    local_write: u64,
    /// For the drag pasteboard, the render thread's name for the drag, which
    /// reads are served from.
    drag: u64,
    mimes: Vec<String>,
    text: Text,
    /// The main thread's reads under way, by token. Answers for tokens not
    /// here (their reader gave up) go.
    reads: HashMap<u64, Reading>,
    /// What reads of the contents of change `cached_for` brought, by MIME
    /// type (`None`: nothing came, or not in time).
    cache: Vec<(String, Option<Arc<[u8]>>)>,
    cached_for: isize,
}

/// A read under way.
struct Reading {
    /// When it began, or data last came for it.
    active: Instant,
    /// The answer, once it comes: the data, or `None` if there's none.
    answer: Option<Option<Vec<u8>>>,
    bounds: ReadBounds,
}

#[derive(Clone)]
struct ReadBounds {
    identity: ClipboardIdentity,
    deadline: Instant,
    maximum: usize,
    cancelled: Option<Arc<AtomicBool>>,
}

impl ReadBounds {
    fn valid(&self, identity: ClipboardIdentity) -> bool {
        self.identity == identity
            && Instant::now() < self.deadline
            && !self.cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
}

impl Foreign {
    fn new() -> Self {
        // Until the compositor tells us of a selection, there is no text.
        Foreign {
            offer: 0,
            local_write: 0,
            drag: 0,
            mimes: Vec::new(),
            text: Text::Read(None),
            reads: HashMap::new(),
            cache: Vec::new(),
            cached_for: 0,
        }
    }

    fn identity(&self) -> ClipboardIdentity {
        ClipboardIdentity { offer: self.offer, local_write: self.local_write }
    }

    /// The cache, emptied first if it's for another change than `generation`.
    fn cache_for(&mut self, generation: isize) -> &mut Vec<(String, Option<Arc<[u8]>>)> {
        if self.cached_for != generation {
            self.cache.clear();
            self.cached_for = generation;
        }
        &mut self.cache
    }
}

pub struct Shared {
    source: Source,
    /// `changeCount`.
    generation: AtomicIsize,
    /// The program has read the general pasteboard: reading ahead is worth
    /// it from now on.
    used: AtomicBool,
    foreign: Mutex<Foreign>,
    arrived: Condvar,
    next_token: AtomicU64,
}

/// The selection's state.
pub fn shared() -> &'static Shared {
    shared_for(Source::Selection)
}

pub fn shared_for(source: Source) -> &'static Shared {
    static SELECTION: OnceLock<Shared> = OnceLock::new();
    static DRAG: OnceLock<Shared> = OnceLock::new();
    static PRIMARY: OnceLock<Shared> = OnceLock::new();
    let cell = match source {
        Source::Selection => &SELECTION,
        Source::Drag => &DRAG,
        Source::Primary => &PRIMARY,
    };
    cell.get_or_init(|| Shared::new(source))
}

impl Shared {
    fn new(source: Source) -> Self {
        Shared {
            source,
            generation: AtomicIsize::new(1),
            used: AtomicBool::new(false),
            foreign: Mutex::new(Foreign::new()),
            arrived: Condvar::new(),
            next_token: AtomicU64::new(1),
        }
    }

    pub fn change_count(&self) -> isize {
        self.used.store(true, Ordering::Relaxed);
        self.current()
    }

    /// The change count, without taking it as a sign the program reads the
    /// pasteboard.
    fn current(&self) -> isize {
        self.generation.load(Ordering::Acquire)
    }

    /// The program cleared the pasteboard: a new generation, its own.
    pub fn bump(&self) -> isize {
        self.bump_identity().0
    }

    /// Stamp a local write atomically with the offer identity it replaces.
    pub fn bump_identity(&self) -> (isize, ClipboardIdentity) {
        let mut f = self.lock();
        f.local_write = f.local_write.wrapping_add(1);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let identity = f.identity();
        drop(f);
        self.arrived.notify_all();
        (generation, identity)
    }

    pub fn identity(&self) -> ClipboardIdentity {
        self.lock().identity()
    }

    pub fn text_read(&self) -> TextRead {
        self.used.store(true, Ordering::Relaxed);
        TextRead { source: self.source, identity: self.identity(), cancelled: Arc::new(AtomicBool::new(false)) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Foreign> {
        self.foreign.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cancel_read(&self, cancelled: &AtomicBool) {
        // Synchronize with the waiter's check-and-wait sequence. A bare atomic
        // store plus notify can lose the notification just before Condvar wait.
        let guard = self.lock();
        cancelled.store(true, Ordering::Relaxed);
        drop(guard);
        self.arrived.notify_all();
    }

    // The render thread's side.

    /// Another client offers a selection with `mimes`, or cleared it (no
    /// types). Returns the offer whose text to read ahead, if it should be.
    pub fn foreign_offered(&self, mimes: Vec<String>, has_text: bool) -> Option<u64> {
        let mut f = self.lock();
        f.offer += 1;
        let read_ahead = has_text && self.used.load(Ordering::Relaxed);
        if read_ahead {
            // The generation moves once the text shows whether this is news.
            let before = match &f.text {
                _ if f.mimes != mimes => None,
                Text::Read(text) => Some(text.clone()),
                // Offered again before its text came: compare with what
                // was there before that.
                Text::Reading { before, .. } => before.clone(),
                Text::Unread => None,
            };
            f.text = Text::Reading { offer: f.offer, deadline: Instant::now() + READ_TIMEOUT, before };
        } else {
            let news = !(mimes.is_empty() && f.mimes.is_empty());
            f.text = if has_text { Text::Unread } else { Text::Read(None) };
            if news {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
        }
        f.mimes = mimes;
        let offer = f.offer;
        drop(f);
        self.arrived.notify_all();
        read_ahead.then_some(offer)
    }

    /// Drag `drag` entered one of our windows offering `mimes`, and its URL
    /// list, if it has one, as read with it: always a change, as each drag
    /// is new. Returns the new change count.
    pub fn drag_offered(&self, drag: u64, mimes: Vec<String>, urls: Option<(String, Arc<[u8]>)>) -> isize {
        let mut f = self.lock();
        f.offer += 1;
        f.drag = drag;
        f.text = Text::Unread;
        f.mimes = mimes;
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let cache = f.cache_for(generation);
        cache.extend(urls.map(|(mime, data)| (mime, Some(data))));
        drop(f);
        self.arrived.notify_all();
        generation
    }

    /// The text of `offer` was read ahead, or couldn't be.
    pub fn text_arrived(&self, offer: u64, outcome: ReadAhead) {
        // The potentially large allocation is outside the shared-state lock.
        let (text, failed) = match outcome {
            ReadAhead::Text(text) => (Some(text.map(Arc::<str>::from)), false),
            ReadAhead::TooLong => (None, false),
            ReadAhead::Failed => (None, true),
        };
        let mut f = self.lock();
        let Text::Reading { offer: reading, before, .. } = &mut f.text else { return };
        if *reading != offer {
            return;
        }
        let before = before.take();
        let (text, news) = match (text, failed) {
            (Some(text), _) => {
                let news = before.as_ref() != Some(&text);
                (Text::Read(text), news)
            }
            (None, false) => (Text::Unread, true),
            (None, true) => (Text::Read(None), true),
        };
        f.text = text;
        if news {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        drop(f);
        self.arrived.notify_all();
    }

    /// Data is coming for the main thread's read `token`: it waits on.
    pub fn read_progressed(&self, token: u64) {
        if let Some(reading) = self.lock().reads.get_mut(&token) {
            reading.active = Instant::now();
        }
    }

    /// The answer to the main thread's read `token`.
    pub fn read_arrived(&self, token: u64, data: Option<Vec<u8>>) {
        let mut f = self.lock();
        let identity = f.identity();
        if let Some(reading) = f.reads.get_mut(&token) {
            reading.answer =
                Some(data.filter(|bytes| reading.bounds.valid(identity) && bytes.len() <= reading.bounds.maximum));
        }
        drop(f);
        self.arrived.notify_all();
    }

    /// The render thread validates the token before opening a foreign pipe and
    /// while its timer runs, so a cancelled worker cannot leave a stalled fd.
    pub fn read_options(&self, token: u64) -> Option<(usize, Instant)> {
        let f = self.lock();
        let bounds = &f.reads.get(&token)?.bounds;
        bounds.valid(f.identity()).then_some((bounds.maximum, bounds.deadline))
    }

    // The main thread's side.

    /// Offer `contents` as the selection, or clear it.
    pub fn offer(&self, contents: Option<Contents>) {
        self.offer_prepared(contents.map(Arc::new));
    }

    pub fn offer_prepared(&self, contents: Option<Arc<Contents>>) {
        match self.source {
            Source::Selection => crate::backend::send_if_running(ToRender::SetSelection { contents }),
            Source::Primary => crate::backend::send_if_running(ToRender::SetPrimarySelection { contents }),
            // A drag offers what its source holds.
            Source::Drag => {}
        }
    }

    /// The MIME types the foreign selection offers.
    pub fn foreign_mimes(&self) -> Vec<String> {
        self.used.store(true, Ordering::Relaxed);
        self.lock().mimes.clone()
    }

    /// The foreign selection's text: read ahead, or on its way until its
    /// deadline, or read now (see [`read_foreign`](Self::read_foreign)).
    pub fn foreign_text(&self) -> Option<String> {
        self.used.store(true, Ordering::Relaxed);
        let mut f = self.lock();
        loop {
            match &f.text {
                Text::Read(text) => return text.as_ref().map(|text| text.to_string()),
                Text::Reading { deadline, .. } => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    f = self.arrived.wait_timeout(f, left).unwrap_or_else(|e| e.into_inner()).0;
                }
                Text::Unread => {
                    let (offer, mime) = (f.offer, text_mime(&f.mimes)?);
                    drop(f);
                    let text = self.read_foreign(&mime).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                    // Kept for the selection it was read from, even if the
                    // read failed: asking again would only wait again.
                    let mut f = self.lock();
                    if f.offer == offer && matches!(f.text, Text::Unread) {
                        f.text = Text::Read(text.as_deref().map(Arc::from));
                    }
                    return text;
                }
            }
        }
    }

    /// Read the foreign selection as `mime` (see
    /// [`read_offer`](Self::read_offer)).
    pub fn read_foreign(&self, mime: &str) -> Option<Vec<u8>> {
        self.read_offer(mime, None).map(|data| data.to_vec())
    }

    /// Whether change `generation` is still current: data read now is its.
    pub fn is_current(&self, generation: isize) -> bool {
        self.current() == generation
    }

    /// Read `mime` for the contents of change `from` (none: the current
    /// ones), or nothing if the pasteboard has changed since. Waits while
    /// data keeps coming, and gives up after [`READ_TIMEOUT`] without any.
    /// The absolute [`TRANSFER_TIMEOUT`] still applies while data arrives.
    /// Each type is read once per change: what came, or that nothing came
    /// in time, is kept, so a client that never answers costs one wait.
    pub fn read_offer(&self, mime: &str, from: Option<isize>) -> Option<Arc<[u8]>> {
        let bounds = ReadBounds {
            identity: self.identity(),
            deadline: Instant::now() + TRANSFER_TIMEOUT,
            maximum: READ_LIMIT,
            cancelled: None,
        };
        self.read_offer_bounded(mime, from, bounds)
    }

    fn read_text_snapshot(
        &self,
        identity: ClipboardIdentity,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        maximum: usize,
    ) -> Option<Arc<str>> {
        let bounds = ReadBounds { identity, deadline, maximum, cancelled: Some(cancelled) };
        let mut f = self.lock();
        loop {
            if !bounds.valid(f.identity()) {
                return None;
            }
            match &f.text {
                Text::Read(text) => return text.clone().filter(|text| text.len() <= maximum),
                Text::Reading { .. } => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    f = self.arrived.wait_timeout(f, left).unwrap_or_else(|e| e.into_inner()).0;
                }
                Text::Unread => {
                    let mime = text_mime(&f.mimes)?;
                    drop(f);
                    let data = self.read_offer_bounded(&mime, None, bounds.clone())?;
                    let text: Arc<str> = String::from_utf8_lossy(&data).into_owned().into();
                    // A replacement-character expansion also obeys the limit.
                    if text.len() > maximum || !bounds.valid(self.identity()) {
                        return None;
                    }
                    return Some(text);
                }
            }
        }
    }

    fn mimes_snapshot(&self, identity: ClipboardIdentity, cancelled: &AtomicBool) -> Option<Vec<String>> {
        self.used.store(true, Ordering::Relaxed);
        let f = self.lock();
        if f.identity() != identity || cancelled.load(Ordering::Relaxed) {
            return None;
        }
        Some(f.mimes.iter().filter(|mime| !mime.starts_with(OWNER_PREFIX)).cloned().collect())
    }

    /// The bytes of `mime`, or of the text when none is named.
    fn read_bytes_snapshot(
        &self,
        identity: ClipboardIdentity,
        mime: Option<&str>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        maximum: usize,
    ) -> Option<Arc<[u8]>> {
        let bounds = ReadBounds { identity, deadline, maximum, cancelled: Some(cancelled) };
        let mime = {
            let mut f = self.lock();
            loop {
                if !bounds.valid(f.identity()) {
                    return None;
                }
                // Until a new offer's read ahead shows whether it's news, the
                // change count, and the cache it keys, are still the last
                // offer's: its bytes mustn't answer for this one.
                if !matches!(f.text, Text::Reading { .. }) {
                    break match mime {
                        Some(mime) => f.mimes.iter().find(|offered| *offered == mime)?.clone(),
                        None => text_mime(&f.mimes)?,
                    };
                }
                let left = deadline.saturating_duration_since(Instant::now());
                f = self.arrived.wait_timeout(f, left).unwrap_or_else(|e| e.into_inner()).0;
            }
        };
        // Read ahead text was made UTF-8; these come from the pipe as sent.
        self.read_offer_bounded(&mime, None, bounds)
    }

    fn read_offer_bounded(&self, mime: &str, from: Option<isize>, bounds: ReadBounds) -> Option<Arc<[u8]>> {
        self.used.store(true, Ordering::Relaxed);
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let (generation, drag) = {
            let mut f = self.lock();
            let generation = self.current();
            if from.is_some_and(|g| g != generation) || !bounds.valid(f.identity()) {
                return None;
            }
            if let Some((_, data)) = f.cache_for(generation).iter().find(|(m, _)| m == mime) {
                return data.clone().filter(|bytes| bytes.len() <= bounds.maximum);
            }
            f.reads.insert(token, Reading { active: Instant::now(), answer: None, bounds: bounds.clone() });
            (generation, f.drag)
        };
        let source = self.source;
        crate::backend::send_if_running(ToRender::ReadSelection { mime: mime.to_owned(), token, source, drag });
        let mut f = self.lock();
        loop {
            if !bounds.valid(f.identity()) {
                f.reads.remove(&token);
                return None;
            }
            let reading = f.reads.get(&token)?;
            if reading.answer.is_some() {
                let data = f.reads.remove(&token).and_then(|r| r.answer).flatten();
                drop(f);
                // Moving a Vec into an Arc may copy many MiB. The UI's small
                // snapshot must not wait on that allocation under this lock.
                let data: Option<Arc<[u8]>> = data.map(Into::into);
                let mut f = self.lock();
                if !bounds.valid(f.identity()) {
                    return None;
                }
                if data.is_some() || bounds.cancelled.is_none() {
                    self.remember(&mut f, generation, mime, data.clone());
                }
                return data;
            }
            let left = (reading.active + READ_TIMEOUT).min(bounds.deadline).saturating_duration_since(Instant::now());
            if left.is_zero() {
                // A late answer has nowhere to go, and asking again would
                // only wait again.
                f.reads.remove(&token);
                if bounds.cancelled.is_none() {
                    self.remember(&mut f, generation, mime, None);
                }
                return None;
            }
            f = self.arrived.wait_timeout(f, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Keep what a read of change `generation` brought, if that's still the
    /// current one.
    fn remember(&self, f: &mut Foreign, generation: isize, mime: &str, data: Option<Arc<[u8]>>) {
        if !self.is_current(generation) {
            return;
        }
        let cache = f.cache_for(generation);
        if !cache.iter().any(|(m, _)| m == mime) {
            cache.push((mime.to_owned(), data));
        }
    }
}

/// The URLs in data of `mime` (see `url_mime`).
pub fn parse_urls(mime: &str, data: &[u8]) -> Vec<String> {
    match mime {
        URI_LIST => parse_uri_list(&String::from_utf8_lossy(data)),
        GNOME_FILES => {
            // "copy" or "cut", then a URI per line.
            let text = String::from_utf8_lossy(data);
            text.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
        }
        MOZ_URL => {
            // UTF-16 (as Firefox writes it) or UTF-8: the URL, then its title.
            let text = decode_moz(data);
            text.lines().next().map(str::trim).filter(|l| !l.is_empty()).map(|l| vec![l.to_owned()]).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// A `text/uri-list`: one URI a line, lines ending in CRLF (or LF), `#`
/// starting a comment line.
pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned).collect()
}

fn decode_moz(data: &[u8]) -> String {
    let utf16 = data.len() >= 2 && data.len().is_multiple_of(2) && (data.starts_with(&[0xff, 0xfe]) || data[1] == 0);
    if !utf16 {
        return String::from_utf8_lossy(data).into_owned();
    }
    let units = data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
    char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).filter(|c| *c != '\u{feff}').collect()
}

/// Whether a URI names a file, to read as `public.file-url` rather than
/// `public.url`.
pub fn is_file_uri(uri: &str) -> bool {
    uri.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:"))
}

/// The path a `file:` URL names, with its escapes decoded, for
/// `NSFilenamesPboardType`.
pub fn file_path(uri: &str) -> Option<String> {
    let rest = uri.get(5..).filter(|_| is_file_uri(uri))?;
    // file:///path, or file://localhost/path.
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => {
            let slash = authority_and_path.find('/')?;
            let host = &authority_and_path[..slash];
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
                return None;
            }
            &authority_and_path[slash..]
        }
        None => rest,
    };
    let path = path.split(['?', '#']).next().unwrap_or(path);
    percent_decode(path)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Shared {
        let s = Shared::new(Source::Selection);
        s.used.store(true, Ordering::Relaxed);
        s
    }

    fn text() -> Vec<String> {
        vec!["text/plain;charset=utf-8".to_string()]
    }

    #[test]
    fn snapshot_identity_changes_before_read_ahead_finishes() {
        let s = fresh();
        let first = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(first, ReadAhead::Text(Some("first".into())));
        let generation = s.change_count();
        let identity = s.identity();
        s.foreign_offered(text(), true).unwrap();
        assert_eq!(s.change_count(), generation, "read-ahead has not classified the new text yet");
        assert_ne!(s.identity(), identity, "an old paste must already be stale");
        let before_write = s.identity();
        s.bump();
        assert_ne!(s.identity(), before_write, "local copy also invalidates pending paste");
    }

    #[test]
    fn worker_read_has_absolute_deadline_even_while_bytes_trickle() {
        let s = Arc::new(fresh());
        let offer = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(offer, ReadAhead::TooLong);
        let token = s.next_token.load(Ordering::Relaxed);
        let producer = {
            let s = s.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(1);
                while !s.lock().reads.contains_key(&token) {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                for _ in 0..20 {
                    s.read_progressed(token);
                    std::thread::sleep(Duration::from_millis(5));
                }
                s.read_arrived(token, Some(b"too late".to_vec()));
            })
        };
        let start = Instant::now();
        let answer = s.read_text_snapshot(
            s.identity(),
            Arc::new(AtomicBool::new(false)),
            start + Duration::from_millis(40),
            1024,
        );
        assert!(answer.is_none());
        assert!(start.elapsed() < Duration::from_millis(200));
        assert!(s.read_options(token).is_none(), "native pipe must see its token cancelled");
        producer.join().unwrap();
    }

    #[test]
    fn worker_cancellation_wakes_a_read_ahead_waiter() {
        let s = Arc::new(fresh());
        s.foreign_offered(text(), true).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let other = {
            let s = s.clone();
            let cancelled = cancelled.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                s.cancel_read(&cancelled);
            })
        };
        let start = Instant::now();
        assert!(s.read_text_snapshot(s.identity(), cancelled, start + Duration::from_secs(5), 1024).is_none());
        assert!(start.elapsed() < Duration::from_millis(200));
        other.join().unwrap();
    }

    #[test]
    fn replaced_offer_cannot_deliver_old_worker_result() {
        let s = Arc::new(fresh());
        let offer = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(offer, ReadAhead::TooLong);
        let identity = s.identity();
        let token = s.next_token.load(Ordering::Relaxed);
        let producer = {
            let s = s.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(1);
                while !s.lock().reads.contains_key(&token) {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                s.foreign_offered(text(), true);
                s.read_arrived(token, Some(b"obsolete".to_vec()));
            })
        };
        assert!(
            s.read_text_snapshot(
                identity,
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                1024
            )
            .is_none()
        );
        producer.join().unwrap();
        assert!(s.read_options(token).is_none());
    }

    #[test]
    fn worker_read_limit_does_not_poison_the_shared_cache() {
        let s = Arc::new(fresh());
        let offer = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(offer, ReadAhead::TooLong);
        let identity = s.identity();
        let producer = answer_next(&s, b"larger than limit");
        assert!(
            s.read_text_snapshot(
                identity,
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
                2
            )
            .is_none()
        );
        producer.join().unwrap();
        let producer = answer_next(&s, b"larger than limit");
        let answer = s.read_text_snapshot(
            identity,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            1024,
        );
        producer.join().unwrap();
        assert_eq!(answer.as_deref(), Some("larger than limit"));
    }

    #[test]
    fn a_repeated_offer_isnt_news() {
        let s = fresh();
        let offer = s.foreign_offered(text(), true).expect("read ahead");
        s.text_arrived(offer, ReadAhead::Text(Some("hi".into())));
        let count = s.change_count();
        assert_eq!(count, 2);
        // The compositor offers the same selection again, on focus.
        let again = s.foreign_offered(text(), true).expect("read ahead");
        s.text_arrived(again, ReadAhead::Text(Some("hi".into())));
        assert_eq!(s.change_count(), count);
        assert_eq!(s.foreign_text().as_deref(), Some("hi"));
        // New text is.
        let other = s.foreign_offered(text(), true).expect("read ahead");
        s.text_arrived(other, ReadAhead::Text(Some("ho".into())));
        assert_eq!(s.change_count(), count + 1);
    }

    #[test]
    fn a_client_that_never_answers_costs_one_wait() {
        let s = fresh();
        s.foreign_offered(text(), true).expect("read ahead");
        let start = Instant::now();
        assert_eq!(s.foreign_text(), None);
        assert!(start.elapsed() >= READ_TIMEOUT / 2);
        // The deadline has passed: no more waiting.
        let again = Instant::now();
        assert_eq!(s.foreign_text(), None);
        assert!(again.elapsed() < READ_TIMEOUT / 4);
        // An answer for an older offer is ignored.
        s.text_arrived(0, ReadAhead::Text(Some("late".into())));
        assert_eq!(s.foreign_text(), None);
    }

    #[test]
    fn each_drag_is_a_change() {
        let s = Shared::new(Source::Drag);
        let count = s.change_count();
        s.drag_offered(1, text(), None);
        let urls: Arc<[u8]> = Arc::from(&b"file:///tmp/a\r\n"[..]);
        let generation = s.drag_offered(2, text(), Some(("text/uri-list".into(), urls.clone())));
        assert_eq!(s.change_count(), count + 2);
        assert_eq!(generation, count + 2);
        assert_eq!(s.foreign_mimes(), text());
        // The URL list came with the drag: reading it doesn't wait.
        let start = Instant::now();
        assert_eq!(s.read_offer("text/uri-list", Some(generation)), Some(urls));
        assert!(start.elapsed() < READ_TIMEOUT / 4);
        // Nor does reading an earlier drag's data: there's none.
        assert_eq!(s.read_offer("text/uri-list", Some(generation - 1)), None);
        assert!(start.elapsed() < READ_TIMEOUT / 4);
    }

    /// A thread answering the next read `s` asks for with `data`.
    fn answer_next(s: &std::sync::Arc<Shared>, data: &'static [u8]) -> std::thread::JoinHandle<()> {
        let token = s.next_token.load(Ordering::Relaxed);
        let s = s.clone();
        std::thread::spawn(move || {
            while !s.lock().reads.contains_key(&token) {
                std::thread::yield_now();
            }
            s.read_arrived(token, Some(data.to_vec()));
        })
    }

    #[test]
    fn byte_reads_never_answer_a_new_offer_with_the_last_ones_bytes() {
        let s = std::sync::Arc::new(fresh());
        let offer = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(offer, ReadAhead::Text(Some("old".into())));
        let feeder = answer_next(&s, b"old");
        let read = |s: &Shared, wait| {
            s.read_bytes_snapshot(s.identity(), None, Arc::new(AtomicBool::new(false)), Instant::now() + wait, 16)
        };
        assert_eq!(read(&s, Duration::from_secs(1)).as_deref(), Some(&b"old"[..]));
        feeder.join().unwrap();
        // A new owner whose text never comes: the read waits out its own
        // deadline rather than answering from the last owner's cache.
        s.foreign_offered(text(), true).unwrap();
        let start = Instant::now();
        assert!(read(&s, Duration::from_millis(60)).is_none());
        assert!(start.elapsed() >= Duration::from_millis(60));
        // Once its text comes, the new owner's bytes are read.
        let newer = s.foreign_offered(text(), true).unwrap();
        s.text_arrived(newer, ReadAhead::Text(Some("new".into())));
        let feeder = answer_next(&s, b"new");
        assert_eq!(read(&s, Duration::from_secs(1)).as_deref(), Some(&b"new"[..]));
        feeder.join().unwrap();
    }

    #[test]
    fn type_reads_name_offered_types_and_wait_out_a_new_offers_read_ahead() {
        let s = std::sync::Arc::new(fresh());
        let owner = format!("{OWNER_PREFIX}-1");
        let mimes = vec!["text/plain;charset=utf-8".to_string(), "image/png".to_string(), owner];
        let offer = s.foreign_offered(mimes.clone(), true).unwrap();
        s.text_arrived(offer, ReadAhead::Text(Some("hi".into())));
        let read = TextRead { source: Source::Selection, identity: s.identity(), cancelled: Arc::default() };
        let snapshot = |s: &Shared, mime, wait| {
            s.read_bytes_snapshot(
                read.identity,
                Some(mime),
                Arc::new(AtomicBool::new(false)),
                Instant::now() + wait,
                16,
            )
        };
        assert_eq!(
            s.mimes_snapshot(read.identity, &AtomicBool::new(false)).unwrap(),
            ["text/plain;charset=utf-8", "image/png"],
            "without the internal owner type"
        );
        let feeder = answer_next(&s, b"\x89PNG");
        assert_eq!(snapshot(&s, "image/png", Duration::from_secs(1)).as_deref(), Some(&b"\x89PNG"[..]));
        feeder.join().unwrap();
        let start = Instant::now();
        assert_eq!(snapshot(&s, "text/html", Duration::from_secs(1)), None, "not offered");
        assert!(start.elapsed() < READ_TIMEOUT / 4);
        // Another owner: the old read is stale, its types and data with it.
        let newer = s.foreign_offered(mimes, true).unwrap();
        assert_eq!(s.mimes_snapshot(read.identity, &AtomicBool::new(false)), None);
        assert_eq!(snapshot(&s, "image/png", Duration::from_millis(20)), None);
        s.text_arrived(newer, ReadAhead::Text(Some("ho".into())));
    }

    #[test]
    fn a_repeated_offer_keeps_what_was_read() {
        let s = std::sync::Arc::new(fresh());
        let mimes = vec!["text/plain;charset=utf-8".to_string(), "text/html".to_string()];
        let offer = s.foreign_offered(mimes.clone(), true).expect("read ahead");
        s.text_arrived(offer, ReadAhead::Text(Some("hi".into())));
        // Items made now belong to this change.
        let generation = s.change_count();
        let feeder = answer_next(&s, b"<b>hi</b>");
        assert_eq!(s.read_offer("text/html", Some(generation)).as_deref(), Some(&b"<b>hi</b>"[..]));
        feeder.join().unwrap();
        // The compositor offers the same selection again, as when a window of
        // ours gets the keyboard: the items stay good, and what was read of
        // it is read again at once.
        let again = s.foreign_offered(mimes.clone(), true).expect("read ahead");
        s.text_arrived(again, ReadAhead::Text(Some("hi".into())));
        assert!(s.is_current(generation));
        let start = Instant::now();
        assert_eq!(s.read_offer("text/html", Some(generation)).as_deref(), Some(&b"<b>hi</b>"[..]));
        assert!(start.elapsed() < READ_TIMEOUT / 4);
        assert_eq!(s.foreign_text().as_deref(), Some("hi"));
        // New contents are a new change: the old items read nothing.
        let other = s.foreign_offered(mimes, true).expect("read ahead");
        s.text_arrived(other, ReadAhead::Text(Some("ho".into())));
        assert!(!s.is_current(generation));
        assert_eq!(s.read_offer("text/html", Some(generation)), None);
    }

    #[test]
    fn a_type_that_never_comes_costs_one_wait() {
        let s = fresh();
        let start = Instant::now();
        assert_eq!(s.read_offer("image/png", None), None);
        assert!(start.elapsed() >= READ_TIMEOUT);
        // Remembered for this change: no second wait.
        let again = Instant::now();
        assert_eq!(s.read_offer("image/png", None), None);
        assert!(again.elapsed() < READ_TIMEOUT / 4);
        // A new change asks again.
        s.foreign_offered(vec!["image/png".into()], false);
        let fresh_start = Instant::now();
        assert_eq!(s.read_offer("image/png", None), None);
        assert!(fresh_start.elapsed() >= READ_TIMEOUT);
    }

    #[test]
    fn late_answers_to_abandoned_reads_go() {
        let s = fresh();
        s.read_arrived(42, Some(vec![1, 2, 3]));
        s.read_progressed(42);
        assert!(s.lock().reads.is_empty());
    }

    #[test]
    fn reads_wait_while_data_comes() {
        let s = std::sync::Arc::new(fresh());
        // The first token read_offer takes.
        let token = s.next_token.load(Ordering::Relaxed);
        let feeder = {
            let s = s.clone();
            std::thread::spawn(move || {
                // Data comes for three timeouts, a little at a time.
                let until = Instant::now() + READ_TIMEOUT * 3;
                while Instant::now() < until {
                    std::thread::sleep(READ_TIMEOUT / 4);
                    s.read_progressed(token);
                }
                s.read_arrived(token, Some(b"all of it".to_vec()));
            })
        };
        let start = Instant::now();
        assert_eq!(s.read_offer("image/png", None).as_deref(), Some(&b"all of it"[..]));
        assert!(start.elapsed() >= READ_TIMEOUT * 3);
        feeder.join().unwrap();
        // Read once per change.
        assert_eq!(s.read_offer("image/png", None).as_deref(), Some(&b"all of it"[..]));
        // Without data, it gives up.
        let start = Instant::now();
        assert_eq!(s.read_offer("text/html", None), None);
        assert!(start.elapsed() >= READ_TIMEOUT && start.elapsed() < READ_TIMEOUT * 3);
    }
}
