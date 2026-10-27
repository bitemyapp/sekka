//! Thumbnails under pixman (the overview's and the switcher's windows).
//!
//! A scene's [`SceneItem::Thumb`] scales a window's buffers into a box as
//! it composites. On the GPU that's free; pixman filters every pixel on
//! the CPU, through its slowest path (a transformed, filtered source in
//! another channel order, often with an alpha), and again each frame the
//! box is redrawn: three thumbnails cost about 1.5 ms of each frame of the
//! overview fading in at 2880 × 1800.
//!
//! [`ThumbCache`] keeps each thumbnail as a picture at its size instead,
//! drawn through its own damage tracker, so a window's thumbnail is
//! scaled again only where the window drew, and the frame copies the
//! picture one to one. It's the same pixels: the same scaling, at the
//! same place within the box. A see-through thumbnail (the overview's
//! filter dims the windows it leaves out) is the picture at that alpha, as
//! GLES draws it; scaled straight onto the frame, pixman drew an opaque
//! window's buffer at a quarter's alpha far too pale.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree};
use smithay::backend::renderer::element::utils::{
    CropRenderElement, Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, Offscreen};
use smithay::reexports::pixman::Image;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::utils::{Buffer, Physical, Point, Rectangle, Size, Transform};

use crate::scene::{Scene, SceneItem};

/// How long a thumbnail nobody drew is kept.
const KEEP: Duration = Duration::from_secs(10);

struct Entry {
    k: f64,
    scale: f64,
    image: Image<'static, 'static>,
    tracker: OutputDamageTracker,
    buffer: MemoryRenderBuffer,
    used: Instant,
}

type Key = (ObjectId, i32, i32);

/// Thumbnails drawn once and kept, per window surface and size.
pub struct ThumbCache {
    format: Fourcc,
    entries: HashMap<Key, Entry>,
}

impl ThumbCache {
    /// Pictures in `format`: the output's own channel order, so the frame
    /// copies them without converting.
    pub fn new(format: Fourcc) -> ThumbCache {
        ThumbCache { format, entries: HashMap::new() }
    }

    /// Replaces a scene's thumbnails by their pictures (at integer scales,
    /// where a picture maps pixel to pixel; others keep scaling as they
    /// composite).
    pub fn bake(&mut self, renderer: &mut PixmanRenderer, scene: &mut Scene, scale: f64) {
        let now = Instant::now();
        if scale.fract() == 0.0 {
            for item in scene.iter_mut() {
                if let SceneItem::Thumb { .. } = item
                    && let Some(baked) = self.picture(renderer, item, scale, now)
                {
                    *item = baked;
                }
            }
        }
        self.entries.retain(|_, e| now.duration_since(e.used) < KEEP);
    }

    fn picture(
        &mut self,
        renderer: &mut PixmanRenderer,
        item: &SceneItem,
        scale: f64,
        now: Instant,
    ) -> Option<SceneItem> {
        let SceneItem::Thumb { surface, surface_loc, alpha, k, target, crop } = item else { return None };
        let size = crop.size;
        if size.w <= 0 || size.h <= 0 {
            return None;
        }
        let key = (surface.id(), size.w, size.h);
        if self.entries.get(&key).is_some_and(|e| e.k != *k || e.scale != scale) {
            self.entries.remove(&key);
        }
        if !self.entries.contains_key(&key) {
            // Whole points, as a memory buffer's size is: the picture fills
            // its top left and the rest stays clear.
            let s = scale as i32;
            let buffer_size: Size<i32, Buffer> = ((size.w + s - 1) / s * s, (size.h + s - 1) / s * s).into();
            let image = renderer.create_buffer(self.format, buffer_size).ok()?;
            let buffer = MemoryRenderBuffer::new(self.format, buffer_size, s, Transform::Normal, None);
            let tracker = OutputDamageTracker::new((buffer_size.w, buffer_size.h), scale, Transform::Normal);
            self.entries.insert(key.clone(), Entry { k: *k, scale, image, tracker, buffer, used: now });
        }
        let entry = self.entries.get_mut(&key)?;
        entry.used = now;

        // The thumbnail as the frame would draw it, moved so its box starts
        // at the picture's origin (whole pixels, so the scaling samples
        // the same places).
        let elems: Vec<WaylandSurfaceRenderElement<PixmanRenderer>> =
            render_elements_from_surface_tree(renderer, surface, *surface_loc, scale, 1.0, Kind::Unspecified);
        let at: Point<i32, Physical> = *target - crop.loc;
        let bounds: Rectangle<i32, Physical> = Rectangle::from_size(size);
        let elements: Vec<_> = elems
            .into_iter()
            .filter_map(|e| {
                let e = RescaleRenderElement::from_element(e, Point::from((0, 0)), *k);
                let e = RelocateRenderElement::from_element(e, at, Relocate::Relative);
                CropRenderElement::from_element(e, scale, bounds)
            })
            .collect();
        let damage: Vec<Rectangle<i32, Physical>> = {
            let mut fb = renderer.bind(&mut entry.image).ok()?;
            let res = entry.tracker.render_output(renderer, &mut fb, 1, &elements, [0.0; 4]).ok()?;
            res.damage.cloned().unwrap_or_default()
        };
        if !damage.is_empty() {
            copy_damage(&entry.image, &mut entry.buffer, &damage);
        }
        Some(SceneItem::Memory {
            buffer: entry.buffer.clone(),
            loc: crop.loc.to_f64(),
            alpha: Some(*alpha),
            src: None,
            size: None,
            kind: Kind::Unspecified,
        })
    }
}

/// Copies the damaged rectangles of a picture into the buffer the frame
/// draws, and tells the buffer which they were.
fn copy_damage(image: &Image<'static, 'static>, buffer: &mut MemoryRenderBuffer, damage: &[Rectangle<i32, Physical>]) {
    let (w, h, stride) = (image.width(), image.height(), image.stride());
    // Safety: the image owns `stride × height` bytes of pixels, and nothing
    // writes them while this reads.
    let src = unsafe { std::slice::from_raw_parts(image.data() as *const u8, stride * h) };
    let mut ctx = buffer.render();
    let _ = ctx.draw(|dst: &mut [u8]| {
        let row = w * 4;
        let mut out = Vec::with_capacity(damage.len());
        for d in damage {
            let Some(d) = d.intersection(Rectangle::from_size((w as i32, h as i32).into())) else { continue };
            let (x0, x1) = (d.loc.x as usize * 4, (d.loc.x + d.size.w) as usize * 4);
            for y in d.loc.y as usize..(d.loc.y + d.size.h) as usize {
                dst[y * row + x0..y * row + x1].copy_from_slice(&src[y * stride + x0..y * stride + x1]);
            }
            out.push(Rectangle::<i32, Buffer>::new((d.loc.x, d.loc.y).into(), (d.size.w, d.size.h).into()));
        }
        Ok::<_, ()>(out)
    });
}
