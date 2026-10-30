//! The worker: `aizome-shiori --worker`, one process for each open
//! document. It reads requests on stdin and answers on stdout (`protocol`).
//! hayro draws the pages, and reads their text, links and the outline with
//! its parser. Its own code forbids `unsafe`, but the SIMD decoding Shiori
//! turns on (`Cargo.toml`) runs some on the document's bytes, and any
//! document can still panic it or keep it busy for seconds on a page: a
//! panic costs that job (the tile isn't drawn, the page has no text), and a
//! page taking too long, or a worker dying, has the app start another and
//! carry on without that page.
//!
//! One thread interprets and draws; another reads requests, so new ones
//! replace what's waiting while a page draws.

use std::collections::VecDeque;
use std::io::{self, BufReader, BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings};
use hayro_syntax::{DecryptionError, LoadPdfError, Pdf, PdfVersion};

use crate::document::{Structure, metadata, shown};
use crate::protocol::{
    DocInfo, Job, Link, OpenError, PageRect, PageText, Reply, Request, TileKey, read_frame, write_frame,
};

/// The most memory the worker may map: a page that asks for more fails
/// (and the app shows it couldn't be drawn) rather than filling the
/// computer's.
const ADDRESS_SPACE: u64 = 8 << 30;

/// How much the pipe the drawings go back through holds.
const PIPE: libc::c_int = 1 << 20;

/// How many pages' text is kept between requests: those that show, and
/// those either side, typically, which text, links and find ask about.
const KEPT_TEXTS: usize = 16;

#[derive(Default)]
struct Queue {
    open: Option<(PathBuf, Option<String>)>,
    draws: VecDeque<TileKey>,
    asks: VecDeque<Job>,
    find: Option<Find>,
    closed: bool,
}

struct Find {
    serial: u64,
    query: String,
    case: bool,
    pages: VecDeque<u32>,
}

impl Queue {
    fn next(&mut self) -> Option<Next> {
        if let Some((path, password)) = self.open.take() {
            return Some(Next::Open(path, password));
        }
        if let Some(k) = self.draws.pop_front() {
            return Some(Next::Job(Job::Draw(k)));
        }
        if let Some(j) = self.asks.pop_front() {
            return Some(Next::Job(j));
        }
        if let Some(f) = &mut self.find {
            match f.pages.pop_front() {
                Some(p) => return Some(Next::Find { serial: f.serial, query: f.query.clone(), case: f.case, page: p }),
                None => self.find = None,
            }
        }
        None
    }

    fn take(&mut self, r: Request) {
        match r {
            Request::Open { path, password } => self.open = Some((path, password)),
            Request::Draw(keys) => self.draws = keys.into(),
            Request::Text(p) => {
                if !self.asks.contains(&Job::Text(p)) {
                    self.asks.push_back(Job::Text(p));
                }
            }
            Request::Links(p) => {
                if !self.asks.contains(&Job::Links(p)) {
                    self.asks.push_back(Job::Links(p));
                }
            }
            Request::Find { serial, query, case, pages } => {
                self.find = Some(Find { serial, query, case, pages: pages.into() })
            }
            Request::StopFind => self.find = None,
        }
    }
}

enum Next {
    Open(PathBuf, Option<String>),
    Job(Job),
    Find { serial: u64, query: String, case: bool, page: u32 },
}

/// Limit what the worker can do to the rest of the computer: it goes when
/// the app does, can't gain privileges, and can't take all the memory.
/// And widen the pipe its drawings go back through.
fn confine() {
    // SAFETY: plain system calls with constant arguments.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0);
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0);
        let limit = libc::rlimit { rlim_cur: ADDRESS_SPACE, rlim_max: ADDRESS_SPACE };
        libc::setrlimit(libc::RLIMIT_AS, &limit);
        // A tile is megabytes: a larger pipe takes it in fewer turns (and the
        // default stays if the system won't).
        libc::fcntl(1, libc::F_SETPIPE_SZ, PIPE);
        // Its parent may already be gone.
        if libc::getppid() == 1 {
            libc::_exit(0);
        }
    }
}

/// The worker's whole life; its exit status.
pub fn run() -> i32 {
    confine();
    let state = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
    {
        let state = state.clone();
        let _ = std::thread::Builder::new().name("requests".into()).spawn(move || {
            let mut stdin = BufReader::new(std::io::stdin().lock());
            loop {
                let req = read_frame::<Request>(&mut stdin);
                let (lock, cv) = &*state;
                let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
                match req {
                    Ok(Some(r)) => q.take(r),
                    _ => {
                        q.closed = true;
                        cv.notify_one();
                        return;
                    }
                }
                cv.notify_one();
            }
        });
    }
    let mut out = BufWriter::with_capacity(1 << 20, std::io::stdout().lock());
    let mut doc: Option<Doc> = None;
    loop {
        let next = {
            let (lock, cv) = &*state;
            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(n) = q.next() {
                    break n;
                }
                if q.closed {
                    return 0;
                }
                q = cv.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let sent = match next {
            Next::Open(path, password) => {
                let _ = send(&mut out, Reply::Working(Job::Open), None);
                doc = None;
                match Doc::open(&path, password.as_deref()) {
                    Ok((d, info)) => {
                        doc = Some(d);
                        send(&mut out, Reply::Opened(Box::new(info)), None)
                    }
                    Err(e) => send(&mut out, Reply::OpenFailed(e), None),
                }
            }
            Next::Job(job) => {
                let Some(d) = doc.as_mut() else { continue };
                let _ = send(&mut out, Reply::Working(job.clone()), None);
                match job {
                    Job::Draw(key) => match d.guarded(|d| d.draw(&key)).flatten() {
                        Some(px) => send(&mut out, Reply::Drawn(key), Some(px.data_as_u8_slice())),
                        None => send(&mut out, Reply::DrawFailed(key), None),
                    },
                    Job::Text(page) => {
                        let text = d.guarded(|d| d.text(page)).flatten().unwrap_or_default();
                        send(&mut out, Reply::Text { page, text }, None)
                    }
                    Job::Links(page) => {
                        let links = d.guarded(|d| d.links(page)).unwrap_or_default();
                        send(&mut out, Reply::Links { page, links }, None)
                    }
                    Job::Open | Job::Find(_) => Ok(()),
                }
            }
            Next::Find { serial, query, case, page } => {
                let Some(d) = doc.as_mut() else { continue };
                let _ = send(&mut out, Reply::Working(Job::Find(page)), None);
                let hits = d.guarded(|d| d.find(page, &query, case)).unwrap_or_default();
                send(&mut out, Reply::Found { serial, page, hits }, None)
            }
        };
        if sent.is_err() {
            // The app went.
            return 0;
        }
    }
}

/// Send `reply` (and a drawing's pixels), with what JSON can't carry left
/// out of it (`Reply::finite`).
fn send(out: &mut impl Write, reply: Reply, pixels: Option<&[u8]>) -> io::Result<()> {
    write_frame(out, &reply.finite(), pixels)
}

/// An open document. The worker reads one document for its whole life, so
/// the document is kept for good (`'static`), and what hayro keeps of it
/// (parsed fonts, decoded images, glyphs drawn) borrows it.
struct Doc {
    pdf: &'static Pdf,
    cache: RenderCache<'static>,
    settings: InterpreterSettings,
    structure: Structure,
    /// The text of the pages asked about lately, newest last.
    texts: VecDeque<(u32, PageText)>,
}

impl Doc {
    fn open(path: &Path, password: Option<&str>) -> Result<(Doc, DocInfo), OpenError> {
        let data = std::fs::read(path).map_err(|e| OpenError::Io(e.to_string()))?;
        let pdf = Pdf::new_with_password(data, password.unwrap_or("")).map_err(|e| match e {
            LoadPdfError::Decryption(DecryptionError::PasswordProtected) => {
                OpenError::Password { tried: password.is_some() }
            }
            LoadPdfError::Decryption(DecryptionError::UnsupportedAlgorithm) => {
                OpenError::Format("its encryption is of a kind Shiori doesn't know".into())
            }
            LoadPdfError::Decryption(_) => OpenError::Format("its encryption is damaged".into()),
            LoadPdfError::Invalid => OpenError::Format("it's damaged, or not a PDF".into()),
        })?;
        let pdf: &'static Pdf = Box::leak(Box::new(pdf));
        let mut structure = Structure::new(pdf);
        let pages = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
        let permissions = pdf.permissions();
        let forms = structure
            .catalog()
            .and_then(|c| c.get::<hayro_syntax::object::Dict<'_>>(b"AcroForm"))
            .and_then(|f| f.get::<hayro_syntax::object::Array<'_>>(b"Fields"))
            .is_some_and(|f| f.iter::<hayro_syntax::object::Object<'_>>().next().is_some());
        let info = DocInfo {
            pages,
            meta: metadata(pdf),
            version: version_name(pdf.version()).into(),
            outline: structure.outline(),
            encrypted: permissions.is_some(),
            forms,
            can_copy: permissions.is_none_or(|p| p.can_copy()),
        };
        let settings = InterpreterSettings { font_resolver: crate::fonts::resolver(), ..Default::default() };
        let doc = Doc { pdf, cache: RenderCache::new(), settings, structure, texts: VecDeque::new() };
        Ok((doc, info))
    }

    /// `job`'s result, or `None` if it panicked; what was cached when it
    /// did is let go, as it may be half made.
    fn guarded<T>(&mut self, job: impl FnOnce(&mut Doc) -> T) -> Option<T> {
        match catch_unwind(AssertUnwindSafe(|| job(self))) {
            Ok(t) => Some(t),
            Err(_) => {
                self.cache = RenderCache::new();
                self.texts.clear();
                None
            }
        }
    }

    fn draw(&mut self, key: &TileKey) -> Option<Pixmap> {
        let page = self.pdf.pages().get(key.page as usize)?;
        let (w, h) = page.render_dimensions();
        let (tw, th) = (u16::try_from(key.w).ok()?, u16::try_from(key.h).ok()?);
        if tw == 0 || th == 0 || key.size.0 == 0 || key.size.1 == 0 {
            return None;
        }
        // The page drawn `size` large, from `at`.
        let transform = Affine::translate((-f64::from(key.at.0), -f64::from(key.at.1)))
            * Affine::scale_non_uniform(f64::from(key.size.0) / f64::from(w), f64::from(key.size.1) / f64::from(h))
            * shown(page);
        let mut ctx = RenderContext::new(tw, th);
        hayro::render_into(page, &self.cache, &self.settings, &RenderSettings::default(), &mut ctx, transform);
        ctx.flush();
        let mut pixmap = Pixmap::new(tw, th);
        // White under the page: every pixel ends opaque, so its
        // premultiplied bytes are the straight RGBA the protocol sends.
        let settings = RasterizerSettings { target_init: TargetInit::Clear(WHITE), ..Default::default() };
        ctx.render_with(&mut pixmap, &mut Resources::default(), settings);
        (pixmap.data_as_u8_slice().len() == key.bytes()).then_some(pixmap)
    }

    /// Page `n`'s text, kept for what comes next.
    fn text(&mut self, n: u32) -> Option<PageText> {
        if let Some(i) = self.texts.iter().position(|(p, _)| *p == n) {
            let kept = self.texts.remove(i)?;
            self.texts.push_back(kept);
        } else {
            let page = self.pdf.pages().get(n as usize)?;
            let text = crate::pdftext::page_text(page, self.cache.interpreter_cache(), &self.settings);
            if self.texts.len() >= KEPT_TEXTS {
                self.texts.pop_front();
            }
            self.texts.push_back((n, text));
        }
        self.texts.back().map(|(_, t)| t.clone())
    }

    fn links(&mut self, n: u32) -> Vec<Link> {
        match self.pdf.pages().get(n as usize) {
            Some(page) => self.structure.links(page, n),
            None => Vec::new(),
        }
    }

    fn find(&mut self, n: u32, query: &str, case: bool) -> Vec<Vec<PageRect>> {
        match self.text(n) {
            Some(text) => crate::pdftext::find(&text, query, case),
            None => Vec::new(),
        }
    }
}

fn version_name(v: PdfVersion) -> &'static str {
    match v {
        PdfVersion::Pdf10 => "1.0",
        PdfVersion::Pdf11 => "1.1",
        PdfVersion::Pdf12 => "1.2",
        PdfVersion::Pdf13 => "1.3",
        PdfVersion::Pdf14 => "1.4",
        PdfVersion::Pdf15 => "1.5",
        PdfVersion::Pdf16 => "1.6",
        PdfVersion::Pdf17 => "1.7",
        PdfVersion::Pdf20 => "2.0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_draws_replace_old_and_finds_come_last() {
        let k = |page| TileKey { page, size: (10, 10), at: (0, 0), w: 10, h: 10 };
        let mut q = Queue::default();
        q.take(Request::Draw(vec![k(1), k(2)]));
        q.take(Request::Find { serial: 1, query: "x".into(), case: false, pages: vec![5, 6] });
        q.take(Request::Text(4));
        q.take(Request::Text(4));
        q.take(Request::Draw(vec![k(3)]));
        assert!(matches!(q.next(), Some(Next::Job(Job::Draw(t))) if t.page == 3));
        assert!(matches!(q.next(), Some(Next::Job(Job::Text(4)))));
        assert!(matches!(q.next(), Some(Next::Find { page: 5, .. })));
        q.take(Request::StopFind);
        assert!(q.next().is_none());
    }
}
