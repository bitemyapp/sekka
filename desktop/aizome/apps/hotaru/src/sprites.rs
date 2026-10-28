//! Worker-owned, bounded reuse of qualified terminal Alpha8 sprites.
use aizome_kit::sidestep_ui::{GridMask, grid};
use hotaru_sprites::{Metrics, rasterize, supports};
use std::collections::HashMap;
use std::sync::Arc;

const CACHE_BYTES: usize = 4 * 1024 * 1024;
const CACHE_ENTRIES: usize = 512;

pub struct Glyph {
    pub mask: GridMask,
    pub origin_x: i32,
    pub origin_y: i32,
    pub bytes: usize,
    pub overhang: bool,
}
#[derive(Clone)]
pub struct Instance {
    pub column: usize,
    pub width: usize,
    pub glyph: Arc<Glyph>,
}
type Key = (u32, u32, u32, u32);
#[derive(Default)]
pub struct Cache {
    entries: HashMap<Key, (Arc<Glyph>, u64)>,
    bytes: usize,
    clock: u64,
}
pub fn codepoint(row: &hotaru_core::Row, column: usize, m: grid::Metrics) -> Option<u32> {
    let cell = row.cells()[column];
    if cell.width() == 0 || !supports(cell.character() as u32) {
        return None;
    }
    if hotaru_sprites::validate_metrics(Metrics {
        cell_width: m.cell_width_px,
        cell_height: m.cell_height_px,
        box_thickness: m.box_thickness_px,
    })
    .is_err()
    {
        return None;
    }
    // Multi-scalar/variation-selector combinations stay on ordinary shaping
    // until their exact upstream sprite selection policy is separately ported.
    if row.grapheme(column).is_some_and(|s| s.chars().count() != 1) {
        return None;
    }
    Some(cell.character() as u32)
}
impl Cache {
    pub fn get(&mut self, cp: u32, m: grid::Metrics) -> Result<Arc<Glyph>, grid::Error> {
        let key = (cp, m.cell_width_px, m.cell_height_px, m.box_thickness_px);
        // Reset only ages on the practically unreachable counter wrap.
        if self.clock == u64::MAX {
            self.entries.values_mut().for_each(|(_, age)| *age = 0);
            self.clock = 0;
        }
        self.clock += 1;
        if let Some((glyph, age)) = self.entries.get_mut(&key) {
            *age = self.clock;
            return Ok(glyph.clone());
        }
        let raster = rasterize(
            cp,
            Metrics { cell_width: m.cell_width_px, cell_height: m.cell_height_px, box_thickness: m.box_thickness_px },
        )
        .map_err(|_| grid::Error::LimitExceeded)?;
        let overhang = raster.ink.is_some_and(|ink| {
            let x = i64::from(raster.origin_x) + i64::from(ink.x);
            let y = i64::from(raster.origin_y) + i64::from(ink.y);
            x < 0
                || y < 0
                || x + i64::from(ink.width) > i64::from(m.cell_width_px)
                || y + i64::from(ink.height) > i64::from(m.cell_height_px)
        });
        let mask = GridMask::new(raster.width, raster.height, raster.alpha).ok_or(grid::Error::LimitExceeded)?;
        let bytes = mask.retention_bytes() + std::mem::size_of::<Glyph>() + 64;
        let glyph = Arc::new(Glyph { mask, origin_x: raster.origin_x, origin_y: raster.origin_y, bytes, overhang });
        // A mask larger than cache admission still renders from its owned frame.
        if bytes <= CACHE_BYTES {
            while self.bytes.saturating_add(bytes) > CACHE_BYTES || self.entries.len() >= CACHE_ENTRIES {
                let Some(old) = self.entries.iter().min_by_key(|(_, (_, age))| *age).map(|(key, _)| *key) else {
                    break;
                };
                if let Some((old, _)) = self.entries.remove(&old) {
                    self.bytes -= old.bytes;
                }
            }
            self.bytes += bytes;
            self.entries.insert(key, (glyph.clone(), self.clock));
        }
        Ok(glyph)
    }
}

pub fn retention_bytes(instances: &[Instance]) -> usize {
    // Conservative: shared masks count once per instance here, so frame/row
    // admission never relies on external cache ownership or weak identities.
    instances.iter().fold(std::mem::size_of_val(instances) + 32, |n, s| n.saturating_add(s.glyph.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_key_reuse_bounded_admission_and_owned_eviction() {
        let fonts = grid::FontSet::new(grid::FontConfig::default()).unwrap();
        let mut cache = Cache::default();
        let metrics = fonts.metrics(1.0).unwrap();
        let held = cache.get(0x253c, metrics).unwrap();
        let repeat = cache.get(0x253c, metrics).unwrap();
        assert!(Arc::ptr_eq(&held, &repeat));
        let original = held.mask.alpha_bytes().to_vec();
        for i in 1..=40 {
            let mut m = metrics;
            m.cell_width_px = i * 3;
            m.cell_height_px = i * 5;
            for cp in (0x2500..=0x259f).chain(0x2800..=0x28ff).chain(0xe0b0..=0xe0bf).chain([0xe0d2, 0xe0d4]) {
                if !supports(cp) {
                    continue;
                }
                let _ = cache.get(cp, m).unwrap();
                assert!(cache.bytes <= CACHE_BYTES && cache.entries.len() <= CACHE_ENTRIES);
            }
        }
        assert_eq!(held.mask.alpha_bytes(), original);
        let after = cache.get(0x253c, metrics).unwrap();
        assert_eq!(held.mask.alpha_bytes(), after.mask.alpha_bytes());
        assert!(!Arc::ptr_eq(&held, &after));
    }
}
