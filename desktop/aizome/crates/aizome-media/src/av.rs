//! FFmpeg, as the engine uses it: owned contexts that free themselves,
//! and the few conversions between its time bases and seconds. Everything
//! unsafe about FFmpeg stays in this module and in `video`'s frame
//! export; each type here is used by one thread at a time.

use std::ffi::{CStr, CString, c_int};
use std::ptr;

pub use ffmpeg_sys_next as ff;
use ff::{AVCodecContext, AVFormatContext, AVFrame, AVMediaType, AVPacket, AVRational, AVStream};

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

/// What FFmpeg says, to the log, at warnings and up.
pub fn quiet() {
    // SAFETY: setting a level.
    unsafe { ff::av_log_set_level(if log::log_enabled!(log::Level::Debug) { 32 } else { 16 }) };
}

/// A channel layout, freed when dropped.
pub struct ChannelLayout(pub ff::AVChannelLayout);

impl ChannelLayout {
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
    pub ctx: *mut AVFormatContext,
}

// SAFETY: an input is used by one thread at a time (the demuxer's).
unsafe impl Send for Input {}

impl Input {
    pub fn open(url: &str) -> Result<Input> {
        let c = CString::new(url).map_err(|_| Error(ff::AVERROR(libc::EINVAL)))?;
        let mut ctx: *mut AVFormatContext = ptr::null_mut();
        // SAFETY: on failure FFmpeg frees what it made and leaves ctx null.
        check(unsafe { ff::avformat_open_input(&mut ctx, c.as_ptr(), ptr::null(), ptr::null_mut()) })?;
        let input = Input { ctx };
        // SAFETY: ctx is open.
        check(unsafe { ff::avformat_find_stream_info(ctx, ptr::null_mut()) })?;
        Ok(input)
    }

    pub fn streams(&self) -> &[*mut AVStream] {
        // SAFETY: nb_streams pointers, valid while the input is.
        unsafe {
            let c = &*self.ctx;
            if c.streams.is_null() { &[] } else { std::slice::from_raw_parts(c.streams, c.nb_streams as usize) }
        }
    }

    pub fn stream(&self, i: usize) -> Option<&AVStream> {
        // SAFETY: as `streams`.
        self.streams().get(i).map(|s| unsafe { &**s })
    }

    /// The next packet, or none at the end.
    pub fn read(&mut self, pkt: &mut Packet) -> Result<bool> {
        pkt.unref();
        // SAFETY: both are valid.
        match unsafe { ff::av_read_frame(self.ctx, pkt.0) } {
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
                    check(ff::av_seek_frame(self.ctx, i as c_int, ts, ff::AVSEEK_FLAG_BACKWARD))?;
                }
                None => {
                    let ts = (secs * f64::from(ff::AV_TIME_BASE)) as i64;
                    check(ff::avformat_seek_file(self.ctx, -1, i64::MIN, ts, ts, 0))?;
                }
            }
        }
        Ok(())
    }

    /// Seconds long, if it says.
    pub fn duration(&self) -> Option<f64> {
        // SAFETY: valid context.
        let d = unsafe { (*self.ctx).duration };
        (d != ff::AV_NOPTS_VALUE && d > 0).then(|| d as f64 / f64::from(ff::AV_TIME_BASE))
    }

    /// Where its timestamps start, in seconds.
    pub fn start(&self) -> f64 {
        // SAFETY: valid context.
        let s = unsafe { (*self.ctx).start_time };
        if s == ff::AV_NOPTS_VALUE { 0.0 } else { s as f64 / f64::from(ff::AV_TIME_BASE) }
    }

    /// Whether it can seek: a file, or a stream FFmpeg can seek in.
    pub fn seekable(&self) -> bool {
        // SAFETY: valid context; pb may be null (formats with no file).
        unsafe {
            let c = &*self.ctx;
            !c.pb.is_null() && ((*c.pb).seekable & 1) != 0
        }
    }

    pub fn metadata(&self) -> Vec<(String, String)> {
        // SAFETY: valid context.
        unsafe { dictionary((*self.ctx).metadata) }
    }

    pub fn format_name(&self) -> String {
        // SAFETY: an input has its format.
        unsafe { text((*(*self.ctx).iformat).long_name).or_else(|| text((*(*self.ctx).iformat).name)) }
            .unwrap_or_default()
    }

    pub fn bit_rate(&self) -> i64 {
        // SAFETY: valid context.
        unsafe { (*self.ctx).bit_rate }
    }

    /// Chapters: start and end seconds, and title.
    pub fn chapters(&self) -> Vec<(f64, f64, Option<String>)> {
        // SAFETY: nb_chapters valid pointers.
        unsafe {
            let c = &*self.ctx;
            if c.chapters.is_null() {
                return Vec::new();
            }
            std::slice::from_raw_parts(c.chapters, c.nb_chapters as usize)
                .iter()
                .map(|&ch| {
                    let ch = &*ch;
                    let start = seconds(ch.start, ch.time_base).unwrap_or(0.0);
                    let end = seconds(ch.end, ch.time_base).unwrap_or(start);
                    let title =
                        dictionary(ch.metadata).into_iter().find(|(k, _)| k.eq_ignore_ascii_case("title")).map(|(_, v)| v);
                    (start, end, title)
                })
                .collect()
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: opened by avformat_open_input; nulls it.
        unsafe { ff::avformat_close_input(&mut self.ctx) };
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

/// A stream's kind.
pub fn media_type(s: &AVStream) -> AVMediaType {
    // SAFETY: streams have parameters.
    unsafe { (*s.codecpar).codec_type }
}

/// A packet, reused for each read.
pub struct Packet(pub *mut AVPacket);

// SAFETY: a packet's data is reference-counted; one owner at a time.
unsafe impl Send for Packet {}

impl Packet {
    pub fn new() -> Packet {
        // SAFETY: allocation; FFmpeg aborts on failure.
        Packet(unsafe { ff::av_packet_alloc() })
    }

    /// A new reference to the same data.
    pub fn share(&self) -> Packet {
        let p = Packet::new();
        // SAFETY: both valid.
        unsafe { ff::av_packet_ref(p.0, self.0) };
        p
    }

    pub fn unref(&mut self) {
        // SAFETY: valid packet.
        unsafe { ff::av_packet_unref(self.0) };
    }

    pub fn get(&self) -> &AVPacket {
        // SAFETY: valid packet.
        unsafe { &*self.0 }
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
        unsafe { ff::av_packet_free(&mut self.0) };
    }
}

/// A frame.
pub struct Frame(pub *mut AVFrame);

// SAFETY: frames are reference-counted and used by one owner at a time;
// a hardware frame's surface is FFmpeg's to synchronize.
unsafe impl Send for Frame {}

impl Frame {
    pub fn new() -> Frame {
        // SAFETY: allocation; FFmpeg aborts on failure.
        Frame(unsafe { ff::av_frame_alloc() })
    }

    /// Another reference to the same pictures (or samples).
    pub fn share(&self) -> Frame {
        // SAFETY: valid frame.
        Frame(unsafe { ff::av_frame_clone(self.0) })
    }

    pub fn get(&self) -> &AVFrame {
        // SAFETY: valid frame.
        unsafe { &*self.0 }
    }

    pub fn get_mut(&mut self) -> &mut AVFrame {
        // SAFETY: valid frame, ours.
        unsafe { &mut *self.0 }
    }

    pub fn unref(&mut self) {
        // SAFETY: valid frame.
        unsafe { ff::av_frame_unref(self.0) };
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
        unsafe { ff::av_frame_free(&mut self.0) };
    }
}

/// A decoder for one stream.
pub struct Decoder {
    pub ctx: *mut AVCodecContext,
    /// The stream's time base, which its frames' times are in.
    pub time_base: AVRational,
}

// SAFETY: used by one thread at a time.
unsafe impl Send for Decoder {}

impl Decoder {
    /// A decoder for `stream`; `setup` adjusts the context before it
    /// opens (threads, a hardware device).
    pub fn open(stream: &AVStream, setup: impl FnOnce(&mut AVCodecContext)) -> Result<Decoder> {
        Decoder::open_with(stream, false, setup)
    }

    /// As `open`, with a decoder that can use a VA-API device if `vaapi`
    /// and the codec has one (AV1's is FFmpeg's own, where libdav1d, which
    /// can't, comes first).
    pub fn open_with(stream: &AVStream, vaapi: bool, setup: impl FnOnce(&mut AVCodecContext)) -> Result<Decoder> {
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
            let ctx = ff::avcodec_alloc_context3(codec);
            let dec = Decoder { ctx, time_base: stream.time_base };
            check(ff::avcodec_parameters_to_context(ctx, stream.codecpar))?;
            (*ctx).pkt_timebase = stream.time_base;
            setup(&mut *ctx);
            check(ff::avcodec_open2(ctx, codec, ptr::null_mut()))?;
            Ok(dec)
        }
    }

    pub fn get(&self) -> &AVCodecContext {
        // SAFETY: valid context.
        unsafe { &*self.ctx }
    }

    /// Give it a packet, or none to drain it. `Err` again: take frames
    /// first.
    pub fn send(&mut self, pkt: Option<&Packet>) -> Result<()> {
        // SAFETY: valid context; null drains.
        check(unsafe { ff::avcodec_send_packet(self.ctx, pkt.map_or(ptr::null(), |p| p.0)) }).map(|_| ())
    }

    /// Take a decoded frame: none when it needs more input (or is
    /// drained).
    pub fn receive(&mut self, frame: &mut Frame) -> Result<bool> {
        // SAFETY: valid context and frame.
        match unsafe { ff::avcodec_receive_frame(self.ctx, frame.0) } {
            0 => Ok(true),
            e if e == ff::AVERROR_EOF || e == ff::AVERROR(libc::EAGAIN) => Ok(false),
            e => Err(Error(e)),
        }
    }

    /// Forget what it was decoding (after a seek).
    pub fn flush(&mut self) {
        // SAFETY: valid, open context.
        unsafe { ff::avcodec_flush_buffers(self.ctx) };
    }

    pub fn codec_name(&self) -> String {
        // SAFETY: an open context has its codec.
        unsafe { text((*(*self.ctx).codec).name) }.unwrap_or_default()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: allocated by avcodec_alloc_context3; nulls it.
        unsafe { ff::avcodec_free_context(&mut self.ctx) };
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
