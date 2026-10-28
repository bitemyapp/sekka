//! The PDFium worker: `aizome-shiori --pdfium-worker`, one process for each
//! open document. It reads requests on stdin and answers on stdout
//! (`protocol`). PDFium is a large C++ library reading files from
//! anywhere; a document that makes it abort (it stops on purpose when its
//! checks fail) or loop takes this process down, and the app starts
//! another and carries on without that page.
//!
//! One thread calls PDFium; another reads requests, so new ones replace
//! what's waiting while a page draws.

use std::collections::VecDeque;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use pdfium_render::prelude::*;

use crate::protocol::{
    DocInfo, Job, Link, OpenError, Outline, PageRect, PageText, Reply, Request, Target, TileKey, read_frame,
    write_frame,
};

/// How many pages stay loaded (parsed) between requests: those that show,
/// typically, drawn a tile at a time.
const LOADED_PAGES: usize = 8;

/// The most memory the worker may map: a page that asks for more fails
/// (and the app shows it couldn't be drawn) rather than filling the
/// computer's.
const ADDRESS_SPACE: u64 = 8 << 30;

/// The environment variable naming the PDFium library to load, in place of
/// the one found beside the program or on the library path.
pub const LIBRARY_VAR: &str = "AIZOME_PDFIUM_LIBRARY";

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
fn confine() {
    // SAFETY: plain system calls with constant arguments.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0);
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0);
        let limit = libc::rlimit { rlim_cur: ADDRESS_SPACE, rlim_max: ADDRESS_SPACE };
        libc::setrlimit(libc::RLIMIT_AS, &limit);
        // Its parent may already be gone.
        if libc::getppid() == 1 {
            libc::_exit(0);
        }
    }
}

/// Bind to PDFium: `$AIZOME_PDFIUM_LIBRARY`, else `libpdfium.so` through
/// the program's run path (the package's) or the library path (the
/// development shell's). It draws with its own rasterizer (AGG; nixpkgs
/// builds it without Skia) and finds the fonts documents name but don't
/// carry in fontconfig's folders.
pub fn bind() -> Result<Pdfium, String> {
    let bindings = match std::env::var_os(LIBRARY_VAR).filter(|v| !v.is_empty()) {
        Some(path) => Pdfium::bind_to_library(PathBuf::from(path)),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|e| format!("{e:?}"))?;
    let dirs = font_dirs();
    let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let config = PdfiumLibraryConfig::new()
        .set_renderer_anti_grain_geometry()
        .set_font_backend_freetype()
        .set_user_font_paths(&dirs)
        .map_err(|e| format!("{e:?}"))?;
    Ok(Pdfium::new_with_config(bindings, config))
}

/// The folders fonts are in. PDFium looks for the fonts a document names
/// but doesn't carry (those it has no stand-in for: Chinese, Japanese and
/// Korean, as a rule) under `/usr/share/fonts`; NixOS keeps them in the
/// store, where fontconfig, which the rest of the desktop asks, knows to
/// look. PDFium searches each folder's subfolders, the first time a
/// document needs it.
pub fn font_dirs() -> Vec<String> {
    let mut dirs = fontconfig_dirs().unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data =
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let more = [
        data.map(|d| d.join("fonts")),
        home.map(|h| h.join(".fonts")),
        Some("/usr/share/fonts".into()),
        Some("/usr/local/share/fonts".into()),
    ];
    for d in more.into_iter().flatten() {
        let d = d.to_string_lossy().into_owned();
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs.retain(|d| Path::new(d).is_dir());
    dirs
}

/// fontconfig's configured font folders, through the `libfontconfig` on
/// the program's run path.
fn fontconfig_dirs() -> Option<Vec<String>> {
    use std::ffi::{CStr, c_char, c_void};
    type Config = unsafe extern "C" fn() -> *mut c_void;
    type Dirs = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type Next = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type Done = unsafe extern "C" fn(*mut c_void);
    // SAFETY: fontconfig's documented C API, called as it says: a config
    // loaded (without its fonts), its directories listed and the list and
    // config freed; the strings are copied before the list goes.
    unsafe {
        let lib = libloading::Library::new("libfontconfig.so.1").ok()?;
        let load: libloading::Symbol<Config> = lib.get(b"FcInitLoadConfig\0").ok()?;
        let dirs: libloading::Symbol<Dirs> = lib.get(b"FcConfigGetFontDirs\0").ok()?;
        let next: libloading::Symbol<Next> = lib.get(b"FcStrListNext\0").ok()?;
        let done: libloading::Symbol<Done> = lib.get(b"FcStrListDone\0").ok()?;
        let destroy: libloading::Symbol<Done> = lib.get(b"FcConfigDestroy\0").ok()?;
        let config = load();
        if config.is_null() {
            return None;
        }
        let list = dirs(config);
        let mut out = Vec::new();
        if !list.is_null() {
            loop {
                let s = next(list);
                if s.is_null() {
                    break;
                }
                out.push(CStr::from_ptr(s).to_string_lossy().into_owned());
            }
            done(list);
        }
        destroy(config);
        Some(out)
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
    let pdfium: &'static Pdfium = match bind() {
        Ok(p) => Box::leak(Box::new(p)),
        Err(e) => {
            let _ = write_frame(&mut out, &Reply::OpenFailed(OpenError::Library(e)), None);
            return 1;
        }
    };
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
                let _ = write_frame(&mut out, &Reply::Working(Job::Open), None);
                doc = None;
                match Doc::open(pdfium, &path, password.as_deref()) {
                    Ok((d, info)) => {
                        doc = Some(d);
                        write_frame(&mut out, &Reply::Opened(Box::new(info)), None)
                    }
                    Err(e) => write_frame(&mut out, &Reply::OpenFailed(e), None),
                }
            }
            Next::Job(job) => {
                let Some(d) = doc.as_mut() else { continue };
                let _ = write_frame(&mut out, &Reply::Working(job.clone()), None);
                match job {
                    Job::Draw(key) => match d.draw(&key) {
                        Some(px) => write_frame(&mut out, &Reply::Drawn(key), Some(&px)),
                        None => write_frame(&mut out, &Reply::DrawFailed(key), None),
                    },
                    Job::Text(page) => {
                        let text = d.text(page).unwrap_or_default();
                        write_frame(&mut out, &Reply::Text { page, text }, None)
                    }
                    Job::Links(page) => {
                        let links = d.links(page);
                        write_frame(&mut out, &Reply::Links { page, links }, None)
                    }
                    Job::Open | Job::Find(_) => Ok(()),
                }
            }
            Next::Find { serial, query, case, page } => {
                let Some(d) = doc.as_mut() else { continue };
                let _ = write_frame(&mut out, &Reply::Working(Job::Find(page)), None);
                let hits = d.find(page, &query, case);
                write_frame(&mut out, &Reply::Found { serial, page, hits }, None)
            }
        };
        if sent.is_err() {
            // The app went.
            return 0;
        }
    }
}

/// Where a page's points go as it shows: PDF's space (y up, from the
/// page's box's corner) turned by the page's rotation, y down from the
/// top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The box the page shows (left, bottom, right, top).
    pub bbox: (f32, f32, f32, f32),
    /// Quarter turns clockwise.
    pub turns: u8,
}

impl Frame {
    /// A point of the page's PDF space, as it shows.
    pub fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (l, b, r, t) = self.bbox;
        let (w, h) = (r - l, t - b);
        let (u, v) = (x - l, t - y);
        match self.turns % 4 {
            0 => (u, v),
            1 => (h - v, u),
            2 => (w - u, h - v),
            _ => (v, w - u),
        }
    }

    /// A rectangle of the page's PDF space (left, bottom, right, top), as
    /// it shows.
    pub fn rect(&self, l: f32, b: f32, r: f32, t: f32) -> PageRect {
        let (x0, y0) = self.point(l, t);
        let (x1, y1) = self.point(r, b);
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
    }
}

fn pdf_rect(frame: &Frame, r: &PdfRect) -> PageRect {
    frame.rect(r.left().value, r.bottom().value, r.right().value, r.top().value)
}

/// An open document, and the pages it has loaded (dropped first: they
/// belong to it).
struct Doc {
    loaded: Vec<(u32, PdfPage<'static>)>,
    doc: PdfDocument<'static>,
}

impl Doc {
    fn open(pdfium: &'static Pdfium, path: &Path, password: Option<&str>) -> Result<(Doc, DocInfo), OpenError> {
        let doc = pdfium.load_pdf_from_file(path, password).map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                OpenError::Password { tried: password.is_some() }
            }
            PdfiumError::IoError(e) => OpenError::Io(e.to_string()),
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) => {
                OpenError::Io("The file couldn't be read.".into())
            }
            e => OpenError::Format(format!("{e:?}")),
        })?;
        let pages = doc.pages();
        let sizes = (0..pages.len())
            .map(|i| match pages.page_size(i) {
                Ok(r) => (r.width().value, r.height().value),
                Err(_) => (612.0, 792.0),
            })
            .collect();
        let meta = doc
            .metadata()
            .iter()
            .map(|t| (format!("{:?}", t.tag_type()), t.value().trim().to_owned()))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let mut info = DocInfo {
            pages: sizes,
            meta,
            version: version_name(doc.version()),
            outline: Vec::new(),
            encrypted: !matches!(
                doc.permissions().security_handler_revision(),
                Ok(PdfSecurityHandlerRevision::Unprotected)
            ),
            forms: doc.form().is_some(),
            can_copy: doc.permissions().can_extract_text_and_graphics().unwrap_or(true),
        };
        let mut d = Doc { loaded: Vec::new(), doc };
        info.outline = d.outline();
        Ok((d, info))
    }

    fn outline(&mut self) -> Vec<Outline> {
        // Deep or circular outlines (damaged files have them) stop here.
        const MOST: usize = 20_000;
        let mut count = 0usize;
        fn walk(d: &Doc, b: Option<PdfBookmark>, depth: usize, count: &mut usize) -> Vec<Outline> {
            let mut out = Vec::new();
            let mut node = b;
            while let Some(n) = node {
                *count += 1;
                if *count > MOST || depth > 32 {
                    break;
                }
                let children = if depth < 32 { walk(d, n.first_child(), depth + 1, count) } else { Vec::new() };
                let target = n
                    .destination()
                    .and_then(|dest| d.place(&dest))
                    .or_else(|| n.action().and_then(|a| d.action_target(&a)));
                out.push(Outline { title: n.title().unwrap_or_default().trim().to_owned(), target, children });
                node = n.next_sibling();
            }
            out
        }
        let root = self.doc.bookmarks().root();
        walk(self, root, 0, &mut count)
    }

    /// Where a destination goes: its page, and how far down (points as it
    /// shows) when it says.
    fn place(&self, dest: &PdfDestination) -> Option<Target> {
        let page = dest.page_index().ok()? as u32;
        let y = match dest.view_settings() {
            Ok(PdfDestinationViewSettings::SpecificCoordinatesAndZoom(x, y, _)) => {
                y.map(|y| (x.map_or(0.0, |x| x.value), y.value))
            }
            Ok(PdfDestinationViewSettings::FitPageHorizontallyToWindow(Some(y))) => Some((0.0, y.value)),
            _ => None,
        }
        .and_then(|(x, y)| {
            let frame = self.frame_of(page)?;
            Some(frame.point(x, y).1.max(0.0))
        });
        Some(Target::Page { page, y })
    }

    fn action_target(&self, a: &PdfAction) -> Option<Target> {
        match a {
            PdfAction::Uri(u) => u.uri().ok().map(Target::Uri),
            PdfAction::LocalDestination(l) => l.destination().ok().and_then(|d| self.place(&d)),
            _ => None,
        }
    }

    /// The page's frame, loading it if need be.
    fn frame_of(&self, page: u32) -> Option<Frame> {
        let loaded = self.loaded.iter().find(|(p, _)| *p == page).map(|(_, pg)| frame(pg));
        loaded.or_else(|| self.doc.pages().get(page as PdfPageIndex).ok().map(|pg| frame(&pg)))
    }

    /// Page `n`, loaded (or kept loaded) for what comes next.
    fn page(&mut self, n: u32) -> Option<&PdfPage<'static>> {
        if let Some(i) = self.loaded.iter().position(|(p, _)| *p == n) {
            let p = self.loaded.remove(i);
            self.loaded.push(p);
        } else {
            let page = self.doc.pages().get(n as PdfPageIndex).ok()?;
            if self.loaded.len() >= LOADED_PAGES {
                self.loaded.remove(0);
            }
            self.loaded.push((n, page));
        }
        self.loaded.last().map(|(_, p)| p)
    }

    /// Page `n` if it's loaded.
    fn loaded(&self, n: u32) -> Option<&PdfPage<'static>> {
        self.loaded.iter().find(|(p, _)| *p == n).map(|(_, p)| p)
    }

    fn draw(&mut self, key: &TileKey) -> Option<Vec<u8>> {
        let page = self.page(key.page)?;
        if key.w == 0 || key.h == 0 || key.size.0 == 0 || key.size.1 == 0 {
            return None;
        }
        let mut bitmap = PdfBitmap::empty(key.w as Pixels, key.h as Pixels, PdfBitmapFormat::BGRA).ok()?;
        // RGBA out (PDFium's byte order reversed), white under the page,
        // its forms' fields and annotations drawn.
        let config = PdfRenderConfig::new()
            .set_fixed_size(key.size.0 as Pixels, key.size.1 as Pixels)
            .set_origin(-(key.at.0 as Pixels), -(key.at.1 as Pixels))
            .set_clear_color(PdfColor::WHITE)
            .render_annotations(true)
            .render_form_data(true)
            .set_reverse_byte_order(true);
        page.render_into_bitmap_with_config(&mut bitmap, &config).ok()?;
        let px = bitmap.as_raw_bytes();
        (px.len() == key.bytes()).then_some(px)
    }

    fn text(&mut self, n: u32) -> Option<PageText> {
        let page = self.page(n)?;
        let frame = frame(page);
        let text = page.text().ok()?;
        let mut out = PageText::default();
        for c in text.chars().iter() {
            let generated = c.is_generated().unwrap_or(false);
            let ch = c.unicode_char().unwrap_or('\u{fffd}');
            if ch == '\r' {
                continue;
            }
            let rect = if generated { None } else { c.loose_bounds().ok().map(|r| pdf_rect(&frame, &r)) };
            out.chars.push(ch);
            out.boxes.push(rect.unwrap_or([0.0; 4]));
        }
        Some(out)
    }

    fn links(&mut self, n: u32) -> Vec<Link> {
        if self.page(n).is_none() {
            return Vec::new();
        }
        let Some(page) = self.loaded(n) else { return Vec::new() };
        let frame = frame(page);
        let mut found = Vec::new();
        for link in page.links().iter() {
            let Ok(rect) = link.rect() else { continue };
            let target = link.destination().map(|d| (Some(d), None)).unwrap_or_else(|| (None, link.action()));
            found.push((pdf_rect(&frame, &rect), target));
        }
        let mut out = Vec::new();
        for (rect, (dest, action)) in found {
            let target = dest.and_then(|d| self.place(&d)).or_else(|| action.and_then(|a| self.action_target(&a)));
            if let Some(target) = target {
                out.push(Link { rect, target });
            }
        }
        out
    }

    fn find(&mut self, n: u32, query: &str, case: bool) -> Vec<Vec<PageRect>> {
        let Some(page) = self.page(n) else { return Vec::new() };
        let frame = frame(page);
        let Ok(text) = page.text() else { return Vec::new() };
        let Ok(search) = text.search(query, &PdfSearchOptions::new().match_case(case)) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        for segments in search.iter(PdfSearchDirection::SearchForward) {
            let rects: Vec<PageRect> = segments.iter().map(|s| pdf_rect(&frame, &s.bounds())).collect();
            if !rects.is_empty() {
                hits.push(rects);
            }
            // A page of a million hits isn't searched to the end.
            if hits.len() >= 5000 {
                break;
            }
        }
        hits
    }
}

/// A loaded page's frame.
fn frame(page: &PdfPage) -> Frame {
    let b = page
        .boundaries()
        .bounding()
        .map(|b| b.bounds)
        .unwrap_or_else(|_| PdfRect::new(PdfPoints::ZERO, PdfPoints::ZERO, page.height(), page.width()));
    let turns = match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 1,
        Ok(PdfPageRenderRotation::Degrees180) => 2,
        Ok(PdfPageRenderRotation::Degrees270) => 3,
        _ => 0,
    };
    Frame { bbox: (b.left().value, b.bottom().value, b.right().value, b.top().value), turns }
}

fn version_name(v: PdfDocumentVersion) -> String {
    match v {
        PdfDocumentVersion::Pdf1_0 => "1.0",
        PdfDocumentVersion::Pdf1_1 => "1.1",
        PdfDocumentVersion::Pdf1_2 => "1.2",
        PdfDocumentVersion::Pdf1_3 => "1.3",
        PdfDocumentVersion::Pdf1_4 => "1.4",
        PdfDocumentVersion::Pdf1_5 => "1.5",
        PdfDocumentVersion::Pdf1_6 => "1.6",
        PdfDocumentVersion::Pdf1_7 => "1.7",
        PdfDocumentVersion::Pdf2_0 => "2.0",
        _ => "",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_turn_points_as_the_page_shows() {
        // A 200 × 100 box whose corner is at (10, 20).
        let f = |turns| Frame { bbox: (10.0, 20.0, 210.0, 120.0), turns };
        // PDF's top left is the shown top left, unturned.
        assert_eq!(f(0).point(10.0, 120.0), (0.0, 0.0));
        assert_eq!(f(0).point(210.0, 20.0), (200.0, 100.0));
        // A quarter turn clockwise: the top left goes to the top right of
        // a 100 × 200 page, the top right to the bottom right.
        assert_eq!(f(1).point(10.0, 120.0), (100.0, 0.0));
        assert_eq!(f(1).point(210.0, 120.0), (100.0, 200.0));
        assert_eq!(f(2).point(10.0, 120.0), (200.0, 100.0));
        assert_eq!(f(3).point(10.0, 120.0), (0.0, 200.0));
        assert_eq!(f(1).rect(10.0, 20.0, 60.0, 120.0), [0.0, 0.0, 100.0, 50.0]);
    }

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
