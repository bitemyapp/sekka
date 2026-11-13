//! What preparing a wallpaper holds at its most: the bytes allocated and
//! not yet freed, counted by this test binary's allocator on the thread
//! that prepares it (where the file is read, decoded and scaled).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};

thread_local! {
    /// The bytes this thread holds beyond those it held when the count
    /// began, and the most they came to.
    static HELD: Cell<(isize, isize)> = const { Cell::new((0, 0)) };
}

struct Counting;

fn count(change: isize) {
    let _ = HELD.try_with(|held| {
        let (now, most) = held.get();
        held.set((now + change, most.max(now + change)));
    });
}

// SAFETY: every call goes to `System` unchanged, and the count is a
// constant-initialized thread-local `Cell`, which allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(-(layout.size() as isize));
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size as isize - layout.size() as isize);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What `f` gives, and the most bytes it held at once.
fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
    HELD.with(|held| held.set((0, 0)));
    let out = f();
    (out, HELD.with(|held| held.get().1) as usize)
}

/// A color JPEG file `width × height`, smooth, so the file itself is small.
fn photo(width: u32, height: u32) -> PathBuf {
    use image::ImageEncoder;
    let shade = |at: u32, of: u32| (at * 255 / of) as u8;
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [shade(x, width), shade(y, height), shade(x + y, width + height)]))
        .collect();
    let mut file = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new(&mut file);
    encoder.write_image(&rgb, width, height, image::ExtendedColorType::Rgb8).unwrap();
    let path = std::env::temp_dir().join(format!("aizome-wallpaper-memory-{}.jpg", std::process::id()));
    std::fs::write(&path, file).unwrap();
    path
}

/// A photo is megapixels and an output far fewer: scaling one to the
/// other takes the photo's pixels as decoded, three bytes each, but never
/// an RGBA copy of them beside those, four bytes more.
#[test]
fn a_photo_is_scaled_without_an_rgba_copy_of_it() {
    let (w, h) = (2400, 1600);
    let path = photo(w, h);
    let (wall, most) = peak(|| aizome_face::wallpaper::prepare(&path, 640, 400));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(wall.map(|rgba| rgba.len()), Some(640 * 400 * 4));
    let (rgb, rgba) = ((w * h * 3) as usize, (w * h * 4) as usize);
    let mb = |bytes: usize| bytes as f64 / 1e6;
    assert!(
        most < rgba,
        "a {w} × {h} photo ({:.1} MB as RGB, {:.1} MB as RGBA) held {:.1} MB at its most",
        mb(rgb),
        mb(rgba),
        mb(most)
    );
}

/// What a wallpaper of one's own holds at its most, and takes, for
/// outputs of three sizes:
/// `AIZOME_WALLPAPER=photo.jpg cargo test --release -p aizome-face --test wallpaper_memory -- --ignored --nocapture`
#[test]
#[ignore]
fn what_a_wallpaper_named_in_the_environment_holds() {
    let path = std::env::var_os("AIZOME_WALLPAPER").expect("AIZOME_WALLPAPER names a picture");
    for (w, h) in [(1920, 1080), (2880, 1800), (3840, 2160)] {
        let started = std::time::Instant::now();
        let (wall, most) = peak(|| aizome_face::wallpaper::prepare(Path::new(&path), w, h));
        let took = started.elapsed();
        assert!(wall.is_some(), "it doesn't read");
        println!("for {w} × {h}: {:.1} MB held at its most, {:.0} ms", most as f64 / 1e6, took.as_secs_f64() * 1e3);
    }
}
