//! Geometry is planned once; execution only visits validated pixel rows.
//!
//! Edges use half-open intervals and round to nearest, ties away from zero,
//! on the pixel grid of an output at the screenshot's scale. Between that
//! output's edges the grid has its capture's pixels, even where its rounded
//! logical size would round to one more or less: past its far edges, the
//! grid counts on from where the capture ends. Adjacent outputs therefore
//! share an edge, and a crop of that output keeps whole native pixels up to
//! its far edges. A thin crop below 1× whose edges would round together
//! rounds that axis from its own corner instead. Native copies round their
//! translation once. Resampling selects the source pixel under each
//! destination pixel's centre, extending edge pixels where rounded logical
//! bounds reach beyond the actual capture. Native copies use the same
//! explicit edge extension only when their bounds need it.

use std::marker::PhantomData;

use smithay::utils::{Point, Rectangle, Size};

use super::pixels::{CapturePixels, Image, RgbaImage, ScreenshotPixels, Straight};
use crate::Rect;
use crate::backend::Capture;

#[derive(Debug)]
pub(super) enum DesktopPoints {}
pub(super) type LayoutRect = Rectangle<i64, DesktopPoints>;

pub(super) fn layout_rect(rect: Rect) -> Result<LayoutRect, String> {
    if rect.is_empty() {
        return Err("the screenshot rectangle must be nonempty".into());
    }
    // Widen before any edge/offset arithmetic (negative desktop origins are
    // valid). Leave the shared, serialized desktop Rect unchanged.
    Ok(Rectangle::new((i64::from(rect.x), i64::from(rect.y)).into(), (i64::from(rect.w), i64::from(rect.h)).into()))
}

#[derive(Clone, Copy)]
struct PixelsPerPoint<S> {
    value: f64,
    space: PhantomData<S>,
}

impl<S> PixelsPerPoint<S> {
    fn new(value: f64) -> Result<Self, String> {
        if !value.is_finite() || value <= 0.0 {
            return Err("a screenshot scale must be finite and positive".into());
        }
        Ok(Self { value, space: PhantomData })
    }

    fn edge(&self, point: i64) -> Result<i64, String> {
        let pixel = (point as f64 * self.value).round();
        if !pixel.is_finite() || pixel < i64::MIN as f64 || pixel >= i64::MAX as f64 {
            return Err("screenshot pixel coordinate is out of range".into());
        }
        Ok(pixel as i64)
    }
}

pub(super) struct CapturedOutput {
    bounds: LayoutRect,
    scale: PixelsPerPoint<CapturePixels>,
    image: RgbaImage<CapturePixels, Straight>,
}

impl CapturedOutput {
    pub(super) fn new(bounds: LayoutRect, scale: f64, capture: Capture) -> Result<Self, String> {
        if bounds.is_empty() {
            return Err("the captured output must have nonempty logical bounds".into());
        }
        Ok(Self {
            bounds,
            scale: PixelsPerPoint::new(scale)?,
            image: RgbaImage::from_capture(capture)?.into_straight(),
        })
    }
}

/// The screenshot's pixel grid: an output's, at the screenshot's scale,
/// shifted so the crop's corner is pixel (0, 0). An axis that would
/// collapse to zero pixels instead rounds from the crop's corner.
#[derive(Clone, Copy)]
struct Grid {
    /// The output's corner, or the crop's on a collapsed axis.
    origin: Point<i64, DesktopPoints>,
    /// Where the output's far corner rounds to, and where its capture
    /// ends, from its corner. Past the first, the grid counts on from the
    /// second. They're equal on a collapsed axis.
    far: Point<i64, ScreenshotPixels>,
    end: Point<i64, ScreenshotPixels>,
    /// The crop's corner, from the grid's origin.
    corner: Point<i64, ScreenshotPixels>,
    scale: PixelsPerPoint<ScreenshotPixels>,
}

impl Grid {
    /// The image edge a desktop point falls on.
    fn pixel(&self, point: Point<i64, DesktopPoints>) -> Result<Point<i64, ScreenshotPixels>, String> {
        let at = point - self.origin;
        Ok((self.edge(at.x, Axis::X)?, self.edge(at.y, Axis::Y)?).into())
    }

    /// One axis of [`Self::pixel`], `at` points from the origin.
    fn edge(&self, at: i64, axis: Axis) -> Result<i64, String> {
        let (far, end) = (axis.component(self.far), axis.component(self.end));
        let edge = self.scale.edge(at)?;
        let edge = if edge < far { Some(edge) } else { (edge - far).checked_add(end) };
        edge.and_then(|edge| edge.checked_sub(axis.component(self.corner)))
            .ok_or_else(|| "screenshot pixel coordinate is out of range".into())
    }

    /// Where image pixel `d`'s centre falls, in points from the origin.
    fn centre(&self, d: u32, axis: Axis) -> f64 {
        let (far, end) = (axis.component(self.far), axis.component(self.end));
        let pixel = i64::from(d) + axis.component(self.corner);
        let pixel = if pixel < end { pixel } else { pixel - end + far };
        (pixel as f64 + 0.5) / self.scale.value
    }
}

pub(super) struct Screenshot {
    grid: Grid,
    image: Image,
}

impl Screenshot {
    /// A crop of the desktop on the pixel grid of the output at `grid`,
    /// whose capture is `pixels` big, at `scale`.
    pub(super) fn new(
        crop: LayoutRect,
        grid: LayoutRect,
        pixels: Size<u32, CapturePixels>,
        scale: f64,
    ) -> Result<Self, String> {
        let scale = PixelsPerPoint::new(scale)?;
        let far = Point::<i64, ScreenshotPixels>::from((scale.edge(grid.size.w)?, scale.edge(grid.size.h)?));
        // Only a capture the output's points were rounded from (as
        // `output_geometry` rounds a mode) moves the grid. Its far edge is
        // then within half a point of where the capture ends, so the grid
        // stays in order.
        let capture_end = |far: i64, pixels: u32, points: i64| {
            if (f64::from(pixels) / scale.value).round() == points as f64 { i64::from(pixels) } else { far }
        };
        let end = Point::from((capture_end(far.x, pixels.w, grid.size.w), capture_end(far.y, pixels.h, grid.size.h)));
        let mut grid = Grid { origin: grid.loc, far, end, corner: (0, 0).into(), scale };
        grid.corner = grid.pixel(crop.loc)?;
        let end = grid.pixel(crop.loc + crop.size)?;
        // Below 1×, a nonempty crop can have both edges round to the
        // same pixel. Round that axis from the crop instead, so its one
        // pixel is also covered by the output, including at the far edge.
        if end.x == 0 {
            grid.origin.x = crop.loc.x;
            (grid.far.x, grid.end.x, grid.corner.x) = (0, 0, 0);
        }
        if end.y == 0 {
            grid.origin.y = crop.loc.y;
            (grid.far.y, grid.end.y, grid.corner.y) = (0, 0, 0);
        }
        let end = grid.pixel(crop.loc + crop.size)?;
        let width = u32::try_from(end.x).map_err(|_| "screenshot width is out of range")?;
        let height = u32::try_from(end.y).map_err(|_| "screenshot height is out of range")?;
        Ok(Self { grid, image: Image::transparent((width, height).into())? })
    }

    pub(super) fn into_image(self) -> Image {
        self.image
    }

    /// The job borrows exactly the images whose dimensions were validated.
    pub(super) fn plan<'s, 'd>(&'d mut self, source: &'s CapturedOutput) -> Result<Blit<'s, 'd>, String> {
        let origin = self.grid.pixel(source.bounds.loc)?;
        let end = self.grid.pixel(source.bounds.loc + source.bounds.size)?;
        let size = self.image.pixels().size();
        let start = Point::<u32, ScreenshotPixels>::from((
            origin.x.clamp(0, size.w as i64) as u32,
            origin.y.clamp(0, size.h as i64) as u32,
        ));
        let end = Point::<u32, ScreenshotPixels>::from((
            end.x.clamp(0, size.w as i64) as u32,
            end.y.clamp(0, size.h as i64) as u32,
        ));
        let destination = Rectangle::from_extremities(start, end);
        let placement = if destination.is_empty() {
            Placement::Empty
        } else {
            let native = source.scale.value == self.grid.scale.value;
            let source_size = source.image.size();
            // Integer translation only. No inference from sampled endpoints.
            let sx = (start.x as i128 - origin.x as i128) as u64;
            let sy = (start.y as i128 - origin.y as i128) as u64;
            if native
                && sx + destination.size.w as u64 <= source_size.w as u64
                && sy + destination.size.h as u64 <= source_size.h as u64
            {
                Placement::ExactCopy(CopyRegion {
                    source: Rectangle::new(
                        (sx as u32, sy as u32).into(),
                        (destination.size.w, destination.size.h).into(),
                    ),
                    destination: start,
                })
            } else {
                let mode = if native {
                    Sampling::ExtendNativeEdges { origin }
                } else {
                    Sampling::Nearest {
                        grid: self.grid,
                        offset: source.bounds.loc - self.grid.origin,
                        source_scale: source.scale,
                    }
                };
                Placement::Resample(SampleRegion {
                    destination,
                    columns: mode.axis(start.x, destination.size.w, source_size.w, Axis::X),
                    rows: mode.axis(start.y, destination.size.h, source_size.h, Axis::Y),
                })
            }
        };
        Ok(Blit { source: &source.image, destination: self.image.pixels_mut(), placement })
    }
}

enum Placement {
    Empty,
    ExactCopy(CopyRegion),
    Resample(SampleRegion),
}

struct CopyRegion {
    source: Rectangle<u32, CapturePixels>,
    destination: Point<u32, ScreenshotPixels>,
}

struct SampleRegion {
    destination: Rectangle<u32, ScreenshotPixels>,
    // Source indices, constructed against the nonempty capture's actual size.
    columns: Vec<u32>,
    rows: Vec<u32>,
}

enum Sampling {
    ExtendNativeEdges {
        origin: Point<i64, ScreenshotPixels>,
    },
    Nearest {
        grid: Grid,
        /// The source's corner, from the grid's.
        offset: Point<i64, DesktopPoints>,
        source_scale: PixelsPerPoint<CapturePixels>,
    },
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn component<S>(self, point: Point<i64, S>) -> i64 {
        match self {
            Self::X => point.x,
            Self::Y => point.y,
        }
    }
}

impl Sampling {
    fn axis(&self, start: u32, count: u32, source_len: u32, axis: Axis) -> Vec<u32> {
        (start..start + count)
            .map(|d| {
                let source = match self {
                    Self::ExtendNativeEdges { origin } => {
                        (d as i128 - i128::from(axis.component(*origin))).min(i128::from(source_len - 1)) as u32
                    }
                    Self::Nearest { grid, offset, source_scale } => {
                        let point = grid.centre(d, axis) - axis.component(*offset) as f64;
                        (point * source_scale.value) as u32
                    }
                };
                source.min(source_len - 1)
            })
            .collect()
    }
}

pub(super) struct Blit<'s, 'd> {
    source: &'s RgbaImage<CapturePixels, Straight>,
    destination: &'d mut RgbaImage<ScreenshotPixels, Straight>,
    placement: Placement,
}

impl Blit<'_, '_> {
    pub(super) fn execute(self) {
        match self.placement {
            Placement::Empty => (),
            Placement::ExactCopy(region) => {
                for y in 0..region.source.size.h {
                    let src = self
                        .source
                        .row_segment((region.source.loc.x, region.source.loc.y + y).into(), region.source.size.w);
                    let dst = self
                        .destination
                        .row_segment_mut((region.destination.x, region.destination.y + y).into(), region.source.size.w);
                    dst.copy_from_slice(src);
                }
            }
            Placement::Resample(region) => {
                for (y, sy) in region.rows.into_iter().enumerate() {
                    let src = self.source.row(sy);
                    let dst = self.destination.row_segment_mut(
                        (region.destination.loc.x, region.destination.loc.y + y as u32).into(),
                        region.destination.size.w,
                    );
                    for (pixel, sx) in dst.iter_mut().zip(&region.columns) {
                        *pixel = src[*sx as usize];
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
