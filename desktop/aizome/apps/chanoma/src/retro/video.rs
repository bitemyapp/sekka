//! A core's frames, to the compositor: each is converted to XRGB8888 and
//! enlarged by a whole number with its pixels kept square-edged (nearest
//! neighbour) into shared memory, then shown through the window's video
//! sink, which the compositor scales the rest of the way. Pre-enlarging
//! keeps a 256-pixel picture crisp on a television: the compositor's own
//! smooth scaling then only has a small step left.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aizome_kit::sidestep_ui::video::{ShmBuffer, VideoBuffer, VideoFrame, VideoSink};

use super::ffi;

/// The tallest picture enlarged here, in pixels: past this the compositor
/// scales (copying more costs more than it shows).
const MAX_LINES: u32 = 1300;
const MAX_FACTOR: u32 = 6;
const SLOTS: usize = 3;

/// The whole-number enlargement for a picture `height` pixels tall shown
/// `target` pixels tall: the smallest that reaches it, within limits.
pub fn factor(height: u32, target: u32) -> u32 {
    if height == 0 {
        return 1;
    }
    let reach = target.div_ceil(height).max(1);
    let cap = (MAX_LINES / height).max(1);
    reach.min(cap).min(MAX_FACTOR)
}

/// 16-bit formats' colours as XRGB8888, all 65,536 of them.
fn table(format: std::ffi::c_uint) -> Box<[u32]> {
    let five = |v: u32| (v << 3) | (v >> 2);
    let six = |v: u32| (v << 2) | (v >> 4);
    (0..=0xffffu32)
        .map(|p| {
            let (r, g, b) = if format == ffi::RETRO_PIXEL_FORMAT_RGB565 {
                (five((p >> 11) & 0x1f), six((p >> 5) & 0x3f), five(p & 0x1f))
            } else {
                (five((p >> 10) & 0x1f), five((p >> 5) & 0x1f), five(p & 0x1f))
            };
            (r << 16) | (g << 8) | b
        })
        .collect()
}

struct Pool {
    map: *mut u8,
    size: usize,
    width: u32,
    height: u32,
    slots: Vec<(Arc<VideoBuffer>, Arc<AtomicBool>)>,
}

// SAFETY: written by the session's thread alone, into slots the compositor
// has released.
unsafe impl Send for Pool {}

impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: mapped by `Pool::new`, this size.
        unsafe { libc::munmap(self.map.cast(), self.size) };
    }
}

impl Pool {
    fn new(width: u32, height: u32) -> Option<Pool> {
        let frame = width as usize * height as usize * 4;
        let size = frame * SLOTS;
        // SAFETY: a new anonymous file, sized and mapped here.
        unsafe {
            let fd = libc::memfd_create(c"chanoma-game".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 {
                return None;
            }
            let owned = <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd);
            if libc::ftruncate(fd, size as libc::off_t) != 0 {
                return None;
            }
            let map =
                libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if map == libc::MAP_FAILED {
                return None;
            }
            let mut slots = Vec::with_capacity(SLOTS);
            for i in 0..SLOTS {
                let dup = std::os::fd::AsFd::as_fd(&owned).try_clone_to_owned().ok()?;
                let buffer = Arc::new(VideoBuffer::Shm(ShmBuffer {
                    fd: dup,
                    size,
                    offset: (frame * i) as u32,
                    width,
                    height,
                    stride: width * 4,
                    format: 1,
                }));
                slots.push((buffer, Arc::new(AtomicBool::new(false))));
            }
            Some(Pool { map: map.cast(), size, width, height, slots })
        }
    }

    fn take(&self) -> Option<(usize, Arc<VideoBuffer>, Arc<AtomicBool>)> {
        self.slots.iter().enumerate().find_map(|(i, (b, busy))| {
            busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .ok()
                .map(|_| (i, b.clone(), busy.clone()))
        })
    }

    fn slot(&mut self, i: usize) -> &mut [u32] {
        let px = self.width as usize * self.height as usize;
        // SAFETY: slot `i` is `px` pixels at its offset, inside the map,
        // and marked busy for this writer.
        unsafe { std::slice::from_raw_parts_mut(self.map.add(px * 4 * i).cast::<u32>(), px) }
    }
}

struct Release(Arc<AtomicBool>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Where the session's frames go.
pub struct Screen {
    sink: Option<VideoSink>,
    pool: Option<Pool>,
    format: std::ffi::c_uint,
    lut: Option<(std::ffi::c_uint, Box<[u32]>)>,
    /// The enlargement.
    pub factor: u32,
    /// The largest picture the core may give, in its pixels.
    max: (u32, u32),
    /// The last frame at the core's size, XRGB8888: for a picture of where
    /// a game was left.
    pub last: Vec<u32>,
    pub last_size: (u32, u32),
    pub frames: u64,
    pub dropped: u64,
}

impl Screen {
    pub fn new(sink: Option<VideoSink>) -> Screen {
        Screen {
            sink,
            pool: None,
            format: ffi::RETRO_PIXEL_FORMAT_0RGB1555,
            lut: None,
            factor: 1,
            max: (0, 0),
            last: Vec::new(),
            last_size: (0, 0),
            frames: 0,
            dropped: 0,
        }
    }

    pub fn set_format(&mut self, format: std::ffi::c_uint) {
        self.format = format;
    }

    /// The core's largest picture, and the enlargement: buffers for them.
    pub fn configure(&mut self, max_width: u32, max_height: u32, factor: u32) {
        let factor = factor.max(1);
        if (max_width, max_height) == self.max && factor == self.factor && self.pool.is_some() {
            return;
        }
        self.max = (max_width, max_height);
        self.factor = factor;
        self.pool = if self.sink.is_some() { Pool::new(max_width * factor, max_height * factor) } else { None };
    }

    /// A frame from the core: `pitch` bytes a line.
    ///
    /// # Safety
    ///
    /// `data` is `height` lines of `pitch` bytes in the screen's format, as
    /// libretro's video callback gives them (or null, the last again).
    pub unsafe fn frame(&mut self, data: *const u8, width: u32, height: u32, pitch: usize) {
        if data.is_null() || width == 0 || height == 0 {
            // The same picture again: nothing new to show.
            return;
        }
        self.frames += 1;
        let k = self.factor;
        if width > self.max.0 || height > self.max.1 || (self.pool.is_none() && self.sink.is_some()) {
            self.configure(width.max(self.max.0), height.max(self.max.1), k);
        }
        let format = self.format;
        if format != ffi::RETRO_PIXEL_FORMAT_XRGB8888 && self.lut.as_ref().is_none_or(|(f, _)| *f != format) {
            self.lut = Some((format, table(format)));
        }
        // The frame at its own size first (also kept as `last`).
        self.last.resize(width as usize * height as usize, 0);
        self.last_size = (width, height);
        for y in 0..height as usize {
            // SAFETY: line `y` of the frame, `pitch` bytes in, as the
            // caller promises.
            let line = unsafe { data.add(y * pitch) };
            let out = &mut self.last[y * width as usize..(y + 1) * width as usize];
            if format == ffi::RETRO_PIXEL_FORMAT_XRGB8888 {
                // SAFETY: `width` pixels of four bytes on this line.
                let src = unsafe { std::slice::from_raw_parts(line.cast::<u32>(), width as usize) };
                out.copy_from_slice(src);
            } else {
                let lut = &self.lut.as_ref().expect("made above").1;
                // SAFETY: `width` pixels of two bytes on this line.
                let src = unsafe { std::slice::from_raw_parts(line.cast::<u16>(), width as usize) };
                for (o, p) in out.iter_mut().zip(src) {
                    *o = lut[usize::from(*p)];
                }
            }
        }
        let Some(pool) = self.pool.as_mut() else { return };
        let Some((i, buffer, busy)) = pool.take() else {
            // The compositor holds every buffer: this frame goes.
            self.dropped += 1;
            return;
        };
        let stride = pool.width as usize;
        let (w, h) = (width as usize, height as usize);
        let k = k as usize;
        let dst = pool.slot(i);
        for y in 0..h {
            let row = &self.last[y * w..(y + 1) * w];
            let first = y * k * stride;
            {
                let out = &mut dst[first..first + w * k];
                for (x, p) in row.iter().enumerate() {
                    out[x * k..(x + 1) * k].fill(*p);
                }
            }
            for r in 1..k {
                dst.copy_within(first..first + w * k, first + r * stride);
            }
        }
        let mut frame = VideoFrame::new(buffer);
        frame.crop = [0.0, 0.0, (w * k) as f32, (h * k) as f32];
        frame.keep = Some(Box::new(Release(busy)));
        if let Some(sink) = &self.sink {
            sink.show(frame);
        }
    }

    /// Show nothing (the game ended).
    pub fn clear(&self) {
        if let Some(s) = &self.sink {
            s.clear();
        }
    }

    /// The last frame as RGBA, for a picture.
    pub fn last_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (w, h) = self.last_size;
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgba = Vec::with_capacity(self.last.len() * 4);
        for p in &self.last {
            rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
        }
        Some((w, h, rgba))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enlargement_reaches_the_screen_within_limits() {
        // A Super Famicom's 224 lines on a 1080-line television: five.
        assert_eq!(factor(224, 1080), 5);
        // On a 4K one it would be ten; the compositor does the rest.
        assert_eq!(factor(224, 2160), 5);
        assert_eq!(factor(144, 2160), 6);
        assert_eq!(factor(480, 1080), 2);
        assert_eq!(factor(1080, 720), 1);
        assert_eq!(factor(0, 1080), 1);
    }

    #[test]
    fn sixteen_bit_colours_reach_full_range() {
        let rgb565 = table(ffi::RETRO_PIXEL_FORMAT_RGB565);
        assert_eq!(rgb565[0xffff], 0x00ff_ffff);
        assert_eq!(rgb565[0xf800], 0x00ff_0000);
        assert_eq!(rgb565[0x07e0], 0x0000_ff00);
        assert_eq!(rgb565[0x001f], 0x0000_00ff);
        let rgb555 = table(ffi::RETRO_PIXEL_FORMAT_0RGB1555);
        assert_eq!(rgb555[0x7fff], 0x00ff_ffff);
        assert_eq!(rgb555[0x7c00], 0x00ff_0000);
    }

    #[test]
    fn frames_are_kept_at_their_own_size() {
        let mut s = Screen::new(None);
        s.set_format(ffi::RETRO_PIXEL_FORMAT_RGB565);
        let px: [u16; 4] = [0xf800, 0x07e0, 0x001f, 0xffff];
        // SAFETY: two lines of two pixels, four bytes a line.
        unsafe { s.frame(px.as_ptr().cast(), 2, 2, 4) };
        assert_eq!(s.last, vec![0xff0000, 0x00ff00, 0x0000ff, 0xffffff]);
        let (w, h, rgba) = s.last_rgba().unwrap();
        assert_eq!((w, h, &rgba[..4]), (2, 2, &[255, 0, 0, 255][..]));
    }
}
