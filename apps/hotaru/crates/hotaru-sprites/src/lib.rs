//! Terminal sprites, adapted from Ghostty commit
//! 6ba144158577d033ccb6ed4b574e4013957c9161. See LICENSE.ghostty.
//! The pure raster has no font, graphics API, or terminal-state dependencies.
mod curves;
mod lines;
mod powerline;
mod vector;

/// Final physical-pixel metrics. Perform font/scale rounding before calling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub cell_width: u32,
    pub cell_height: u32,
    pub box_thickness: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidMetrics,
    SizeLimit,
    Allocation,
}

/// Half-open coordinates within the padded mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Unpremultiplied alpha coverage, row-major with stride `width`.
/// Origins are relative to the cell's top-left, including Ghostty's padding.
/// The mask may extend outside the cell when the requested stroke is thick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub alpha: Vec<u8>,
    pub ink: Option<Bounds>,
}

pub const MAX_MASK_BYTES: usize = 4 * 1024 * 1024;

pub const fn supports(cp: u32) -> bool {
    matches!(cp, 0x2500..=0x259f | 0xe0b0..=0xe0bf | 0xe0d2 | 0xe0d4)
}

/// Check metrics without allocating. Native integrations may select ordinary
/// font shaping when these raster limits cannot represent a configured face.
pub fn validate_metrics(m: Metrics) -> Result<(), Error> {
    dimensions(m).map(|_| ())
}
fn dimensions(m: Metrics) -> Result<(u32, u32, usize), Error> {
    if m.cell_width == 0
        || m.cell_height == 0
        || m.cell_width > 4096
        || m.cell_height > 4096
        || m.box_thickness == 0
        || m.box_thickness > m.cell_width.min(m.cell_height)
    {
        return Err(Error::InvalidMetrics);
    }
    let px = m.cell_width / 4;
    let py = m.cell_height / 4;
    let width = m.cell_width + 2 * px;
    let height = m.cell_height + 2 * py;
    let len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(Error::SizeLimit)?;
    if len > MAX_MASK_BYTES {
        return Err(Error::SizeLimit);
    }
    Ok((width, height, len))
}

/// Render one supported scalar. No output or substitution is returned on error.
pub fn rasterize(cp: u32, m: Metrics) -> Result<Raster, Error> {
    if !supports(cp) {
        return Err(Error::Unsupported);
    }
    let (width, height, len) = dimensions(m)?;
    let mut alpha = Vec::new();
    alpha
        .try_reserve_exact(len)
        .map_err(|_| Error::Allocation)?;
    alpha.resize(len, 0);
    let mut result = Raster {
        width,
        height,
        origin_x: -((m.cell_width / 4) as i32),
        origin_y: -((m.cell_height / 4) as i32),
        alpha,
        ink: None,
    };
    if cp >= 0xe000 {
        powerline::draw(&mut result, m, cp)?;
    } else if cp >= 0x2580 {
        blocks(&mut result, m, cp);
    } else if (0x256d..=0x2573).contains(&cp) {
        curves::draw(&mut result, m, cp)?;
    } else if let Some(lines) = lines::definition(cp) {
        lines::draw(&mut result, m, lines);
    } else {
        dashed(&mut result, m, cp);
    }
    result.ink = result.find_ink();
    Ok(result)
}

impl Raster {
    /// Equivalent to upstream Canvas.box, including reversed endpoints and clip.
    fn rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, alpha: u8) {
        let px = -self.origin_x as u32;
        let py = -self.origin_y as u32;
        let left = (x0.min(x1) + px).min(self.width);
        let right = (x0.max(x1) + px).min(self.width);
        let top = (y0.min(y1) + py).min(self.height);
        let bottom = (y0.max(y1) + py).min(self.height);
        for y in top..bottom {
            let start = (y * self.width + left) as usize;
            self.alpha[start..start + (right - left) as usize].fill(alpha);
        }
    }

    fn find_ink(&self) -> Option<Bounds> {
        let mut x0 = self.width;
        let mut x1 = 0;
        let mut y0 = self.height;
        let mut y1 = 0;
        for (y, row) in self.alpha.chunks_exact(self.width as usize).enumerate() {
            for (x, alpha) in row.iter().enumerate() {
                if *alpha != 0 {
                    x0 = x0.min(x as u32);
                    x1 = x1.max(x as u32 + 1);
                    y0 = y0.min(y as u32);
                    y1 = y1.max(y as u32 + 1);
                }
            }
        }
        (x0 < x1 && y0 < y1).then_some(Bounds {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0),
            height: y1.saturating_sub(y0),
        })
    }
}

fn blocks(c: &mut Raster, m: Metrics, cp: u32) {
    let w = m.cell_width;
    let h = m.cell_height;
    // All fractions in this block are exact binary eighths. Integer round-half-up
    // matches Zig @round for positive dimensions without platform FP differences.
    let eighth = |size, numerator| (size * numerator + 4) / 8;
    match cp {
        0x2580 => c.rect(0, 0, w, eighth(h, 4), 255),
        0x2581..=0x2587 => c.rect(0, h - eighth(h, cp - 0x2580), w, h, 255),
        0x2588 => c.rect(0, 0, w, h, 255),
        0x2589..=0x258f => c.rect(0, 0, eighth(w, 0x2590 - cp), h, 255),
        0x2590 => c.rect(w - eighth(w, 4), 0, w, h, 255),
        0x2591..=0x2593 => c.rect(0, 0, w, h, ((cp - 0x2590) * 64) as u8),
        0x2594 => c.rect(0, 0, w, eighth(h, 1), 255),
        0x2595 => c.rect(w - eighth(w, 1), 0, w, h, 255),
        0x2596..=0x259f => {
            // TL, TR, BL, BR; complementary odd halves overlap one pixel.
            let mask = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14][(cp - 0x2596) as usize];
            if mask & 1 != 0 {
                c.rect(0, 0, w.div_ceil(2), h.div_ceil(2), 255);
            }
            if mask & 2 != 0 {
                c.rect(w / 2, 0, w, h.div_ceil(2), 255);
            }
            if mask & 4 != 0 {
                c.rect(0, h / 2, w.div_ceil(2), h, 255);
            }
            if mask & 8 != 0 {
                c.rect(w / 2, h / 2, w, h, 255);
            }
        }
        _ => unreachable!("range checked before allocation"),
    }
}

fn dashed(c: &mut Raster, m: Metrics, cp: u32) {
    let (count, vertical, heavy, gap) = match cp {
        0x2504..=0x250b => (
            if cp < 0x2508 { 3 } else { 4 },
            cp & 2 != 0,
            cp & 1 != 0,
            m.box_thickness.max(4),
        ),
        0x254c..=0x254f => (
            2,
            cp & 2 != 0,
            cp & 1 != 0,
            if cp == 0x254c {
                m.box_thickness
            } else {
                m.box_thickness * 2
            },
        ),
        _ => unreachable!("every supported box scalar is mapped"),
    };
    let size = if vertical {
        m.cell_height
    } else {
        m.cell_width
    };
    let across = if vertical {
        m.cell_width
    } else {
        m.cell_height
    };
    let thick = if heavy {
        m.box_thickness * 2
    } else {
        m.box_thickness
    };
    if size < count * 2 {
        // Upstream deliberately falls back to LIGHT, even for a heavy dash.
        let start = across.saturating_sub(m.box_thickness) / 2;
        if vertical {
            c.rect(start, 0, start + m.box_thickness, size, 255);
        } else {
            c.rect(0, start, size, start + m.box_thickness, 255);
        }
        return;
    }
    let gap = gap.min(size / (2 * count));
    let available = size - count * gap;
    let span = available / count;
    let extra = available % count;
    let cross = across.saturating_sub(thick) / 2;
    let mut start = if vertical { 0 } else { gap / 2 };
    for i in 0..count {
        let end = start + span + u32::from(i < extra);
        if vertical {
            c.rect(cross, start, cross + thick, end, 255);
        } else {
            c.rect(start, cross, end, cross + thick, 255);
        }
        start = end + gap;
    }
}
