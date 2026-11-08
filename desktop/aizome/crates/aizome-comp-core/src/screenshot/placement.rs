//! Geometry is planned once; execution only visits validated pixel rows.
//!
//! Edges use half-open intervals and round to nearest, ties away from zero,
//! relative to the crop's origin. Adjacent outputs therefore share an edge.
//! Native copies round their translation once. Resampling selects the source
//! pixel under each destination pixel's centre, extending edge pixels where
//! rounded logical bounds reach beyond the actual capture. Native copies use
//! the same explicit edge extension only when their bounds need it.

use std::marker::PhantomData;

use smithay::utils::{Point, Rectangle};

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

pub(super) struct Screenshot {
    crop: LayoutRect,
    scale: PixelsPerPoint<ScreenshotPixels>,
    image: Image,
}

impl Screenshot {
    pub(super) fn new(crop: LayoutRect, scale: f64) -> Result<Self, String> {
        let scale = PixelsPerPoint::new(scale)?;
        let width = u32::try_from(scale.edge(crop.size.w)?).map_err(|_| "screenshot width is out of range")?;
        let height = u32::try_from(scale.edge(crop.size.h)?).map_err(|_| "screenshot height is out of range")?;
        Ok(Self { crop, scale, image: Image::transparent((width, height).into())? })
    }

    pub(super) fn into_image(self) -> Image {
        self.image
    }

    /// The job borrows exactly the images whose dimensions were validated.
    pub(super) fn plan<'s, 'd>(&'d mut self, source: &'s CapturedOutput) -> Result<Blit<'s, 'd>, String> {
        let offset = source.bounds.loc - self.crop.loc;
        let origin = Point::<i64, ScreenshotPixels>::from((self.scale.edge(offset.x)?, self.scale.edge(offset.y)?));
        let end = Point::<i64, ScreenshotPixels>::from((
            self.scale.edge(offset.x + source.bounds.size.w)?,
            self.scale.edge(offset.y + source.bounds.size.h)?,
        ));
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
            let native = source.scale.value == self.scale.value;
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
                    Sampling::Nearest { offset, source_scale: source.scale, target_scale: self.scale }
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
        offset: Point<i64, DesktopPoints>,
        source_scale: PixelsPerPoint<CapturePixels>,
        target_scale: PixelsPerPoint<ScreenshotPixels>,
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
                    Self::Nearest { offset, source_scale, target_scale } => {
                        let at = axis.component(*offset);
                        let point = (d as f64 + 0.5) / target_scale.value - at as f64;
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
