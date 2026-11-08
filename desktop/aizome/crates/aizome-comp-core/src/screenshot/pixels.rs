//! Validated, tightly packed RGBA images. Keep the backend's byte allocation
//! for PNG encoding, but expose rows as pixels to placement code.

use std::marker::PhantomData;

use smithay::utils::{Point, Size};

use crate::backend::Capture;

#[derive(Clone, Copy, Debug)]
pub(super) enum CapturePixels {}
#[derive(Clone, Copy, Debug)]
pub(super) enum ScreenshotPixels {}
pub(super) enum Premultiplied {}
pub(super) enum Straight {}

pub(super) struct RgbaImage<Space, Alpha> {
    size: Size<u32, Space>,
    bytes: Vec<u8>,
    alpha: PhantomData<Alpha>,
}

fn byte_len<S>(size: Size<u32, S>) -> Result<usize, String> {
    if size.is_empty() {
        return Err("an image must have nonzero dimensions".into());
    }
    (size.w as usize)
        .checked_mul(size.h as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .ok_or_else(|| "image dimensions overflow the buffer size".into())
}

impl<S, A> RgbaImage<S, A> {
    fn from_bytes(size: Size<u32, S>, bytes: Vec<u8>) -> Result<Self, String> {
        let expected = byte_len(size)?;
        if bytes.len() != expected {
            return Err(format!("an RGBA image needs {expected} bytes, got {}", bytes.len()));
        }
        Ok(Self { size, bytes, alpha: PhantomData })
    }

    pub(super) fn size(&self) -> Size<u32, S> {
        self.size
    }

    pub(super) fn row(&self, y: u32) -> &[[u8; 4]] {
        &self.bytes.as_chunks::<4>().0[y as usize * self.size.w as usize..][..self.size.w as usize]
    }

    pub(super) fn row_segment(&self, at: Point<u32, S>, width: u32) -> &[[u8; 4]] {
        &self.row(at.y)[at.x as usize..][..width as usize]
    }

    pub(super) fn row_segment_mut(&mut self, at: Point<u32, S>, width: u32) -> &mut [[u8; 4]] {
        let row =
            &mut self.bytes.as_chunks_mut::<4>().0[at.y as usize * self.size.w as usize..][..self.size.w as usize];
        &mut row[at.x as usize..][..width as usize]
    }
}

impl RgbaImage<CapturePixels, Premultiplied> {
    pub(super) fn from_capture(capture: Capture) -> Result<Self, String> {
        Self::from_bytes((capture.width, capture.height).into(), capture.rgba)
    }

    pub(super) fn into_straight(mut self) -> RgbaImage<CapturePixels, Straight> {
        for pixel in self.bytes.as_chunks_mut::<4>().0 {
            let alpha = pixel[3] as u32;
            if alpha != 0 && alpha != 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        RgbaImage { size: self.size, bytes: self.bytes, alpha: PhantomData }
    }
}

/// A nonempty screenshot with validated dimensions and straight RGBA pixels.
/// Dimensions and storage cannot be changed independently.
pub struct Image(RgbaImage<ScreenshotPixels, Straight>);

impl Image {
    pub fn width(&self) -> u32 {
        self.0.size.w
    }

    pub fn height(&self) -> u32 {
        self.0.size.h
    }

    pub fn rgba(&self) -> &[u8] {
        &self.0.bytes
    }

    pub(super) fn from_capture(capture: Capture) -> Result<Self, String> {
        let source = RgbaImage::from_capture(capture)?.into_straight();
        // A single-output shot adopts the capture's pixel grid unchanged.
        Ok(Self(RgbaImage {
            size: Size::from((source.size.w, source.size.h)),
            bytes: source.bytes,
            alpha: PhantomData,
        }))
    }

    pub(super) fn transparent(size: Size<u32, ScreenshotPixels>) -> Result<Self, String> {
        let len = byte_len(size)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|e| format!("allocate screenshot: {e}"))?;
        bytes.resize(len, 0);
        Ok(Self(RgbaImage::from_bytes(size, bytes)?))
    }

    pub(super) fn pixels(&self) -> &RgbaImage<ScreenshotPixels, Straight> {
        &self.0
    }

    pub(super) fn pixels_mut(&mut self) -> &mut RgbaImage<ScreenshotPixels, Straight> {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_construction_checks_dimensions_and_storage_together() {
        for size in [(0, 1), (1, 0), (0, 0), (u32::MAX, u32::MAX)] {
            assert!(RgbaImage::<CapturePixels, Premultiplied>::from_bytes(size.into(), vec![]).is_err());
        }
        for len in [0, 7, 9, 12] {
            assert!(RgbaImage::<CapturePixels, Premultiplied>::from_bytes((2, 1).into(), vec![0; len]).is_err());
        }
        let image = RgbaImage::<CapturePixels, Premultiplied>::from_bytes((2, 1).into(), vec![0; 8]).unwrap();
        assert_eq!(image.row(0).len(), 2);
    }

    #[test]
    fn alpha_conversion_preserves_opaque_pixels_and_unpremultiplies_once() {
        let image = RgbaImage::<CapturePixels, Premultiplied>::from_bytes(
            (3, 1).into(),
            vec![12, 34, 56, 255, 64, 32, 0, 128, 0, 0, 0, 0],
        )
        .unwrap()
        .into_straight();
        assert_eq!(image.row(0), &[[12, 34, 56, 255], [128, 64, 0, 128], [0, 0, 0, 0]]);
    }
}
