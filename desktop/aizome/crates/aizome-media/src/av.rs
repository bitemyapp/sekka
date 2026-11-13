//! FFmpeg, as the engine uses it: owned contexts that free themselves,
//! and the few conversions between its time bases and seconds. Everything
//! unsafe about FFmpeg stays in this module and in `video`'s frame
//! export; each type here is used by one thread at a time.

use std::ffi::{CStr, CString, c_int};
use std::ptr::{self, NonNull};

// Infallible owning constructors follow Rust allocation failure behavior.
fn allocated<T>(p: *mut T) -> NonNull<T> {
    NonNull::new(p).unwrap_or_else(|| std::alloc::handle_alloc_error(std::alloc::Layout::new::<T>()))
}

use ff::{AVCodecContext, AVFormatContext, AVFrame, AVMediaType, AVPacket, AVRational, AVStream};
pub use ffmpeg_sys_next as ff;

/// An FFmpeg error code, with its message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error(pub c_int);

impl Error {
    pub fn is_eof(self) -> bool {
        self.0 == ff::AVERROR_EOF
    }

    pub fn is_again(self) -> bool {
        self.0 == ff::AVERROR(libc::EAGAIN)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0 as libc::c_char; 128];
        // SAFETY: the buffer is as long as said, and av_strerror ends it.
        unsafe { ff::av_strerror(self.0, buf.as_mut_ptr(), buf.len()) };
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        write!(f, "{}", s.to_string_lossy())
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self} ({})", self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A return code: negative is an error.
pub fn check(code: c_int) -> Result<c_int> {
    if code < 0 { Err(Error(code)) } else { Ok(code) }
}

/// Seconds of `ts` in `tb`; none for FFmpeg's "no time".
pub fn seconds(ts: i64, tb: AVRational) -> Option<f64> {
    (ts != ff::AV_NOPTS_VALUE && tb.den != 0).then(|| ts as f64 * f64::from(tb.num) / f64::from(tb.den))
}

/// `secs` in `tb`, rounded down.
pub fn in_base(secs: f64, tb: AVRational) -> i64 {
    if tb.num == 0 {
        return 0;
    }
    (secs * f64::from(tb.den) / f64::from(tb.num)).floor() as i64
}

pub fn ratio(r: AVRational) -> Option<f64> {
    (r.num != 0 && r.den != 0).then(|| f64::from(r.num) / f64::from(r.den))
}

/// A C string from FFmpeg, or none.
///
/// # Safety
/// `p` is null or a nul-terminated string.
pub unsafe fn text(p: *const libc::c_char) -> Option<String> {
    // SAFETY: as the caller promises.
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

/// Route FFmpeg diagnostics through Rust's logger. In particular, H.264
/// recovery points after seeking an open GOP can report missing references
/// without failing a decode. Keep these library diagnostics available at
/// debug level; operation failures still travel through Result/Event::Failed.
pub fn quiet() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // SAFETY: installs a process-lifetime, thread-safe C callback.
        unsafe { aizome_ffmpeg_install_log() };
    });
}

unsafe extern "C" {
    fn aizome_ffmpeg_install_log();
}

fn diagnostic_level(level: c_int) -> log::Level {
    if level <= ff::AV_LOG_FATAL {
        log::Level::Error
    } else if level <= ff::AV_LOG_INFO {
        log::Level::Debug
    } else {
        log::Level::Trace
    }
}

#[unsafe(no_mangle)]
extern "C" fn aizome_ffmpeg_log_enabled(level: c_int) -> c_int {
    // No Rust panic may cross the decoder's C callback boundary.
    std::panic::catch_unwind(|| log::log_enabled!(target: "aizome_media::ffmpeg", diagnostic_level(level)) as c_int)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn aizome_ffmpeg_log(level: c_int, message: *const libc::c_char, length: usize) {
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: the C bridge lends its local buffer for exactly length bytes.
        let bytes = unsafe { std::slice::from_raw_parts(message.cast::<u8>(), length) };
        let text = String::from_utf8_lossy(bytes);
        log::log!(target: "aizome_media::ffmpeg", diagnostic_level(level), "{}", text.trim_end());
    });
}

/// A channel layout, freed when dropped.
pub struct ChannelLayout(ff::AVChannelLayout);

impl ChannelLayout {
    pub fn as_ptr(&self) -> *const ff::AVChannelLayout {
        &self.0
    }
    /// The layout `n` channels are mixed to and played in: FFmpeg's usual
    /// one for that many (2.1 for three, 4.0 for four, 5.1(back) for six).
    /// The resampler makes it, and the sound server is told it, from here.
    pub fn mixed(n: i32) -> ChannelLayout {
        // SAFETY: a zeroed layout, which FFmpeg fills.
        unsafe {
            let mut l: ff::AVChannelLayout = std::mem::zeroed();
            ff::av_channel_layout_default(&mut l, n);
            ChannelLayout(l)
        }
    }

    /// Where channel `i` goes (`AV_CHAN_NONE` past the last).
    pub fn channel(&self, i: u32) -> ff::AVChannel {
        // SAFETY: a layout FFmpeg made.
        unsafe { ff::av_channel_layout_channel_from_index(&self.0, i) }
    }
}

impl Drop for ChannelLayout {
    fn drop(&mut self) {
        // SAFETY: a layout FFmpeg made, freed once.
        unsafe { ff::av_channel_layout_uninit(&mut self.0) };
    }
}

/// An open input: a file, or a URL FFmpeg reads.
pub struct Input {
    ctx: NonNull<AVFormatContext>,
}

// SAFETY: an input is used by one thread at a time (the demuxer's).
unsafe impl Send for Input {}

impl Input {
    pub fn open(url: &str) -> Result<Input> {
        let c = CString::new(url).map_err(|_| Error(ff::AVERROR(libc::EINVAL)))?;
        let mut ctx: *mut AVFormatContext = ptr::null_mut();
        // SAFETY: on failure FFmpeg frees what it made and leaves ctx null.
        check(unsafe { ff::avformat_open_input(&mut ctx, c.as_ptr(), ptr::null(), ptr::null_mut()) })?;
        let input = Input { ctx: allocated(ctx) };
        // SAFETY: ctx is open.
        check(unsafe { ff::avformat_find_stream_info(ctx, ptr::null_mut()) })?;
        Ok(input)
    }

    /// Borrowed pointer: never free it or keep it past this owner. Mutations
    /// must preserve FFmpeg invariants and respect outstanding Rust borrows.
    pub fn as_ptr(&self) -> *mut AVFormatContext {
        self.ctx.as_ptr()
    }

    pub fn streams(&self) -> impl ExactSizeIterator<Item = Stream<'_>> {
        // SAFETY: the open input owns nb_streams initialized stream pointers.
        let streams = unsafe {
            let c = &*self.as_ptr();
            if c.streams.is_null() { &[] } else { std::slice::from_raw_parts(c.streams, c.nb_streams as usize) }
        };
        streams.iter().map(|s| Stream(unsafe { &**s }))
    }

    pub fn stream(&self, i: usize) -> Option<Stream<'_>> {
        self.streams().nth(i)
    }

    /// The next packet, or none at the end.
    pub fn read(&mut self, pkt: &mut Packet) -> Result<bool> {
        pkt.unref();
        // SAFETY: both are valid.
        match unsafe { ff::av_read_frame(self.as_ptr(), pkt.as_ptr()) } {
            0 => Ok(true),
            e if e == ff::AVERROR_EOF => Ok(false),
            e => Err(Error(e)),
        }
    }

    /// Go to the latest keyframe at or before `secs` (any stream's, in
    /// the format's own way), or to `stream`'s.
    pub fn seek(&mut self, secs: f64, stream: Option<usize>) -> Result<()> {
        // SAFETY: valid context; a stream index FFmpeg checks.
        unsafe {
            match stream.and_then(|i| self.stream(i).map(|s| (i, s.time_base))) {
                Some((i, tb)) => {
                    let ts = in_base(secs, tb);
                    check(ff::av_seek_frame(self.as_ptr(), i as c_int, ts, ff::AVSEEK_FLAG_BACKWARD))?;
                }
                None => {
                    let ts = (secs * f64::from(ff::AV_TIME_BASE)) as i64;
                    check(ff::avformat_seek_file(self.as_ptr(), -1, i64::MIN, ts, ts, 0))?;
                }
            }
        }
        Ok(())
    }

    /// Seconds long, if it says.
    pub fn duration(&self) -> Option<f64> {
        // SAFETY: valid context.
        let d = unsafe { (*self.as_ptr()).duration };
        (d != ff::AV_NOPTS_VALUE && d > 0).then(|| d as f64 / f64::from(ff::AV_TIME_BASE))
    }

    /// Where its timestamps start, in seconds.
    pub fn start(&self) -> f64 {
        // SAFETY: valid context.
        let s = unsafe { (*self.as_ptr()).start_time };
        if s == ff::AV_NOPTS_VALUE { 0.0 } else { s as f64 / f64::from(ff::AV_TIME_BASE) }
    }

    /// Whether it can seek: a file, or a stream FFmpeg can seek in.
    pub fn seekable(&self) -> bool {
        // SAFETY: valid context; pb may be null (formats with no file).
        unsafe {
            let c = &*self.as_ptr();
            !c.pb.is_null() && ((*c.pb).seekable & 1) != 0
        }
    }

    pub fn metadata(&self) -> Vec<(String, String)> {
        // SAFETY: valid context.
        unsafe { dictionary((*self.as_ptr()).metadata) }
    }

    pub fn format_name(&self) -> String {
        // SAFETY: an input has its format.
        unsafe { text((*(*self.as_ptr()).iformat).long_name).or_else(|| text((*(*self.as_ptr()).iformat).name)) }
            .unwrap_or_default()
    }

    pub fn bit_rate(&self) -> i64 {
        // SAFETY: valid context.
        unsafe { (*self.as_ptr()).bit_rate }
    }

    /// Chapters: start and end seconds, and title.
    pub fn chapters(&self) -> Vec<(f64, f64, Option<String>)> {
        // SAFETY: nb_chapters valid pointers.
        unsafe {
            let c = &*self.as_ptr();
            if c.chapters.is_null() {
                return Vec::new();
            }
            std::slice::from_raw_parts(c.chapters, c.nb_chapters as usize)
                .iter()
                .map(|&ch| {
                    let ch = &*ch;
                    let start = seconds(ch.start, ch.time_base).unwrap_or(0.0);
                    let end = seconds(ch.end, ch.time_base).unwrap_or(start);
                    let title = dictionary(ch.metadata)
                        .into_iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
                        .map(|(_, v)| v);
                    (start, end, title)
                })
                .collect()
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: opened by avformat_open_input; nulls it.
        unsafe { ff::avformat_close_input(&mut self.as_ptr()) };
    }
}

/// A dictionary's entries, in order.
///
/// # Safety
/// `d` is null or a valid dictionary.
pub unsafe fn dictionary(d: *const ff::AVDictionary) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut e: *mut ff::AVDictionaryEntry = ptr::null_mut();
    let empty = c"";
    loop {
        // SAFETY: iterating with AV_DICT_IGNORE_SUFFIX (2) and the last entry.
        e = unsafe { ff::av_dict_get(d, empty.as_ptr(), e, 2) };
        if e.is_null() {
            break;
        }
        // SAFETY: entries have a key and a value.
        let (k, v) = unsafe { (text((*e).key), text((*e).value)) };
        if let (Some(k), Some(v)) = (k, v) {
            out.push((k, v));
        }
    }
    out
}

/// A stream borrowed from a live input, never a caller-constructed C struct.
#[derive(Clone, Copy)]
pub struct Stream<'a>(&'a AVStream);
impl std::ops::Deref for Stream<'_> {
    type Target = AVStream;
    fn deref(&self) -> &AVStream {
        self.0
    }
}

/// A stream's kind.
pub fn media_type(s: Stream<'_>) -> AVMediaType {
    // SAFETY: streams have parameters.
    unsafe { (*s.codecpar).codec_type }
}

/// A packet, reused for each read.
/// ```compile_fail
/// use aizome_media::av::Packet;
/// let packet = Packet::new();
/// let duplicate = Packet(packet.0);
/// ```
pub struct Packet(NonNull<AVPacket>);

// SAFETY: a packet's data is reference-counted; one owner at a time.
unsafe impl Send for Packet {}

impl Packet {
    /// Borrowed pointer: never free it or keep it past this owner. Mutations
    /// must preserve FFmpeg invariants and respect outstanding Rust borrows.
    pub fn as_ptr(&self) -> *mut AVPacket {
        self.0.as_ptr()
    }
    pub fn new() -> Packet {
        // SAFETY: allocation, checked before constructing the owner.
        Packet(allocated(unsafe { ff::av_packet_alloc() }))
    }

    /// A new reference to the same data.
    pub fn share(&self) -> Packet {
        let p = Packet::new();
        // SAFETY: both valid.
        check(unsafe { ff::av_packet_ref(p.as_ptr(), self.as_ptr()) }).expect("reference packet");
        p
    }

    pub fn unref(&mut self) {
        // SAFETY: valid packet.
        unsafe { ff::av_packet_unref(self.as_ptr()) };
    }

    pub fn get(&self) -> &AVPacket {
        // SAFETY: valid packet.
        unsafe { self.0.as_ref() }
    }

    pub fn is_key(&self) -> bool {
        self.get().flags & ff::AV_PKT_FLAG_KEY != 0
    }

    pub fn bytes(&self) -> &[u8] {
        let p = self.get();
        if p.data.is_null() || p.size <= 0 {
            &[]
        } else {
            // SAFETY: size bytes at data.
            unsafe { std::slice::from_raw_parts(p.data, p.size as usize) }
        }
    }
}

impl Default for Packet {
    fn default() -> Packet {
        Packet::new()
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: allocated by av_packet_alloc.
        unsafe { ff::av_packet_free(&mut self.as_ptr()) };
    }
}

/// A frame.
pub struct Frame(NonNull<AVFrame>);

// SAFETY: frames are reference-counted and used by one owner at a time;
// a hardware frame's surface is FFmpeg's to synchronize.
unsafe impl Send for Frame {}

impl Frame {
    /// Borrowed pointer: never free it or keep it past this owner. Mutations
    /// must preserve FFmpeg invariants and respect outstanding Rust borrows.
    pub fn as_ptr(&self) -> *mut AVFrame {
        self.0.as_ptr()
    }
    pub fn new() -> Frame {
        // SAFETY: allocation, checked before constructing the owner.
        Frame(allocated(unsafe { ff::av_frame_alloc() }))
    }

    /// Another reference to the same pictures (or samples).
    pub fn share(&self) -> Frame {
        // SAFETY: valid frame.
        Frame(allocated(unsafe { ff::av_frame_clone(self.as_ptr()) }))
    }

    pub fn get(&self) -> &AVFrame {
        // SAFETY: valid frame.
        unsafe { self.0.as_ref() }
    }

    /// # Safety
    /// Preserve FFmpeg ownership and buffer invariants of every modified field.
    pub(crate) unsafe fn get_mut(&mut self) -> &mut AVFrame {
        // SAFETY: valid frame, ours.
        unsafe { self.0.as_mut() }
    }

    pub fn unref(&mut self) {
        // SAFETY: valid frame.
        unsafe { ff::av_frame_unref(self.as_ptr()) };
    }

    /// Its time in seconds in `tb`: the decoder's best guess.
    pub fn time(&self, tb: AVRational) -> Option<f64> {
        let f = self.get();
        let ts = if f.best_effort_timestamp != ff::AV_NOPTS_VALUE { f.best_effort_timestamp } else { f.pts };
        seconds(ts, tb)
    }
}

impl Default for Frame {
    fn default() -> Frame {
        Frame::new()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: allocated by av_frame_alloc or av_frame_clone.
        unsafe { ff::av_frame_free(&mut self.as_ptr()) };
    }
}

/// A decoder for one stream.
pub struct Decoder {
    ctx: NonNull<AVCodecContext>,
    /// The stream's time base, which its frames' times are in.
    pub time_base: AVRational,
}

// SAFETY: used by one thread at a time.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Borrowed pointer: never free it or keep it past this owner. Mutations
    /// must preserve FFmpeg invariants and respect outstanding Rust borrows.
    pub fn as_ptr(&self) -> *mut AVCodecContext {
        self.ctx.as_ptr()
    }
    /// A decoder for `stream`; `setup` adjusts the context before it
    /// opens (threads, a hardware device).
    pub fn open(stream: Stream<'_>, threads: i32) -> Result<Decoder> {
        // SAFETY: only the scalar thread count is changed.
        unsafe { Decoder::open_with(stream, false, |ctx| ctx.thread_count = threads) }
    }

    /// As `open`, with a decoder that can use a VA-API device if `vaapi`
    /// and the codec has one (AV1's is FFmpeg's own, where libdav1d, which
    /// can't, comes first).
    /// # Safety
    /// `setup` must preserve the context's ownership and pointer invariants.
    pub(crate) unsafe fn open_with(
        stream: Stream<'_>,
        vaapi: bool,
        setup: impl FnOnce(&mut AVCodecContext),
    ) -> Result<Decoder> {
        // SAFETY: the stream's parameters are valid; on each failure the
        // context is freed by the Decoder's drop.
        unsafe {
            let par = &*stream.codecpar;
            let codec = match vaapi.then(|| vaapi_decoder(par.codec_id)).filter(|c| !c.is_null()) {
                Some(c) => c,
                None => ff::avcodec_find_decoder(par.codec_id),
            };
            if codec.is_null() {
                return Err(Error(ff::AVERROR_DECODER_NOT_FOUND));
            }
            let ctx = allocated(ff::avcodec_alloc_context3(codec));
            let dec = Decoder { ctx, time_base: stream.time_base };
            check(ff::avcodec_parameters_to_context(ctx.as_ptr(), stream.codecpar))?;
            (*ctx.as_ptr()).pkt_timebase = stream.time_base;
            setup(&mut *ctx.as_ptr());
            check(ff::avcodec_open2(ctx.as_ptr(), codec, ptr::null_mut()))?;
            Ok(dec)
        }
    }

    pub fn get(&self) -> &AVCodecContext {
        // SAFETY: valid context.
        unsafe { &*self.as_ptr() }
    }

    /// Give it a packet, or none to drain it. `Err` again: take frames
    /// first.
    pub fn send(&mut self, pkt: Option<&Packet>) -> Result<()> {
        // SAFETY: valid context; null drains.
        check(unsafe { ff::avcodec_send_packet(self.as_ptr(), pkt.map_or(ptr::null(), |p| p.as_ptr())) }).map(|_| ())
    }

    /// Take a decoded frame: none when it needs more input (or is
    /// drained).
    pub fn receive(&mut self, frame: &mut Frame) -> Result<bool> {
        // SAFETY: valid context and frame.
        match unsafe { ff::avcodec_receive_frame(self.as_ptr(), frame.as_ptr()) } {
            0 => Ok(true),
            e if e == ff::AVERROR_EOF || e == ff::AVERROR(libc::EAGAIN) => Ok(false),
            e => Err(Error(e)),
        }
    }

    /// Forget what it was decoding (after a seek).
    pub fn flush(&mut self) {
        // SAFETY: valid, open context.
        unsafe { ff::avcodec_flush_buffers(self.as_ptr()) };
    }

    pub fn codec_name(&self) -> String {
        // SAFETY: an open context has its codec.
        unsafe { text((*(*self.as_ptr()).codec).name) }.unwrap_or_default()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: allocated by avcodec_alloc_context3; nulls it.
        unsafe { ff::avcodec_free_context(&mut self.as_ptr()) };
    }
}

/// A decoder of `id` that can decode on a VA-API device, if FFmpeg has one.
fn vaapi_decoder(id: ff::AVCodecID) -> *const ff::AVCodec {
    let mut opaque = ptr::null_mut();
    // SAFETY: iterating FFmpeg's static codec list and their static
    // hardware configurations.
    unsafe {
        loop {
            let c = ff::av_codec_iterate(&mut opaque);
            if c.is_null() {
                return c;
            }
            if ff::av_codec_is_decoder(c) == 0 || (*c).id != id {
                continue;
            }
            for i in 0.. {
                let cfg = ff::avcodec_get_hw_config(c, i);
                if cfg.is_null() {
                    break;
                }
                if (*cfg).device_type == ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI
                    && (*cfg).methods & ff::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32 != 0
                {
                    return c;
                }
            }
        }
    }
}

/// A codec's short name.
pub fn codec_name(id: ff::AVCodecID) -> String {
    // SAFETY: always a static string.
    unsafe { text(ff::avcodec_get_name(id)) }.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_convert_both_ways() {
        let tb = AVRational { num: 1, den: 90_000 };
        assert_eq!(seconds(180_000, tb), Some(2.0));
        assert_eq!(seconds(ff::AV_NOPTS_VALUE, tb), None);
        assert_eq!(in_base(2.0, tb), 180_000);
        assert_eq!(in_base(1.0 / 3.0, AVRational { num: 1, den: 3 }), 1);
    }

    #[test]
    fn errors_say_what_they_are() {
        let e = Error(ff::AVERROR_EOF);
        assert!(e.is_eof());
        assert!(e.to_string().to_lowercase().contains("end of file"));
        assert!(Error(ff::AVERROR(libc::EAGAIN)).is_again());
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn shared_packet_retains_data_after_original_is_dropped() {
        let packet = Packet::new();
        // SAFETY: packet is a live empty owner; FFmpeg allocates its buffer.
        check(unsafe { ff::av_new_packet(packet.as_ptr(), 3) }).unwrap();
        // SAFETY: the packet now owns three writable bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(b"abc".as_ptr(), packet.get().data, 3);
        }
        let shared = packet.share();
        assert_ne!(packet.as_ptr(), shared.as_ptr());
        drop(packet);
        assert_eq!(shared.bytes(), b"abc");
    }
}
