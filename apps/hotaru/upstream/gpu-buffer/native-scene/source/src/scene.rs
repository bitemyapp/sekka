#![forbid(unsafe_code)]
use std::{io::Read, path::Path, sync::Arc};
pub struct Asset {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub data: Arc<[u8]>,
}
pub struct Scene {
    width: u32,
    height: u32,
    commands: Vec<[u32; 10]>,
    assets: Vec<Asset>,
    tiles: Vec<[u32; 2]>,
    indices: Vec<u32>,
    expected: Vec<u8>,
    name: String,
    // Prevent sibling diagnostic modules from constructing unvalidated scenes.
    // Published only behind an Arc; renderer never receives mutable access.
    _validated: (),
}
fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > maximum {
        return Err("file byte limit".into());
    }
    Ok(bytes)
}
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("length overflow")?;
        let value = self.data.get(self.at..end).ok_or("truncated scene")?;
        self.at = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
}
impl Scene {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn commands(&self) -> &[[u32; 10]] {
        &self.commands
    }
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }
    pub fn tiles(&self) -> &[[u32; 2]] {
        &self.tiles
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn expected(&self) -> &[u8] {
        &self.expected
    }
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = read_bounded(path, 64 << 20)?;
        let mut r = Reader {
            data: &bytes,
            at: 0,
        };
        if r.bytes(8)? != b"HTSCN001" {
            return Err("scene version".into());
        }
        let width = r.u32()?;
        let height = r.u32()?;
        let nc = r.u32()? as usize;
        let na = r.u32()? as usize;
        let nt = r.u32()? as usize;
        let ni = r.u32()? as usize;
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || nc > 131072
            || na > 131072
            || ni > 4 << 20
            || nt != width.div_ceil(16) as usize * height.div_ceil(16) as usize
        {
            return Err("scene dimensions/count limit".into());
        }
        let mut commands = Vec::with_capacity(nc);
        for _ in 0..nc {
            let mut c = [0; 10];
            for v in &mut c {
                *v = r.u32()?;
            }
            commands.push(c);
        }
        let mut assets = Vec::with_capacity(na);
        let mut asset_bytes = 0usize;
        for _ in 0..na {
            let id = r.u64()?;
            let w = r.u32()?;
            let h = r.u32()?;
            let format = r.u32()?;
            let len = r.u32()? as usize;
            let stride = match format {
                0 => 1usize,
                1 => 4,
                2 => 16,
                _ => return Err("asset format".into()),
            };
            if w == 0
                || h == 0
                || len > 1 << 20
                || len
                    != (w as usize)
                        .checked_mul(h as usize)
                        .and_then(|n| n.checked_mul(stride))
                        .ok_or("asset overflow")?
            {
                return Err("asset size".into());
            }
            asset_bytes = asset_bytes.checked_add(len).ok_or("asset total overflow")?;
            if asset_bytes > 32 << 20 {
                return Err("asset total limit".into());
            }
            let data: Arc<[u8]> = r.bytes(len)?.into();
            if format == 2
                && data.chunks_exact(4).any(|v| {
                    let f = f32::from_le_bytes(v.try_into().unwrap());
                    !f.is_finite() || !(0.0..=1.0).contains(&f)
                })
            {
                return Err("float sample range".into());
            }
            assets.push(Asset {
                id,
                width: w,
                height: h,
                format,
                data,
            });
        }
        let mut tiles = Vec::with_capacity(nt);
        for _ in 0..nt {
            let a = r.u32()?;
            let b = r.u32()?;
            if (a as usize).checked_add(b as usize).is_none_or(|n| n > ni) {
                return Err("tile range".into());
            }
            tiles.push([a, b]);
        }
        let mut indices = Vec::with_capacity(ni);
        for _ in 0..ni {
            let i = r.u32()?;
            if i as usize >= nc {
                return Err("command index".into());
            }
            indices.push(i);
        }
        if r.at != bytes.len() {
            return Err("trailing scene data".into());
        }
        for c in &commands {
            if c[0] >= c[2]
                || c[1] >= c[3]
                || c[2] > width
                || c[3] > height
                || c[4] > 5
                || c[6] > 255
            {
                return Err("command geometry/kind".into());
            }
            if matches!(c[4], 2..=5) {
                let a = assets.get(c[7] as usize).ok_or("command asset")?;
                let format = match c[4] {
                    2 | 4 => 0,
                    3 => 1,
                    5 => 2,
                    _ => unreachable!(),
                };
                let x = c[8] as i32 as i64;
                let y = c[9] as i32 as i64;
                if a.format != format
                    || (c[0] as i64) < x
                    || (c[1] as i64) < y
                    || (c[2] as i64) > x + a.width as i64
                    || (c[3] as i64) > y + a.height as i64
                {
                    return Err("asset command bounds".into());
                }
            } else if c[7] != u32::MAX {
                return Err("solid command asset".into());
            }
        }
        // Tile lists must contain exactly the relevant operations in original
        // order, not merely in-range arbitrary indices from a malformed fixture.
        let cols = width.div_ceil(16);
        let mut expected = vec![Vec::<u32>::new(); nt];
        let mut expanded = 0usize;
        for (i, c) in commands.iter().enumerate() {
            let cells = (((c[2] - 1) / 16 - c[0] / 16 + 1) as usize)
                .checked_mul(((c[3] - 1) / 16 - c[1] / 16 + 1) as usize)
                .ok_or("tile expansion")?;
            expanded = expanded
                .checked_add(cells)
                .ok_or("tile expansion overflow")?;
            if expanded > 4 << 20 {
                return Err("tile expansion limit".into());
            }
            for y in c[1] / 16..=(c[3] - 1) / 16 {
                for x in c[0] / 16..=(c[2] - 1) / 16 {
                    expected[(y * cols + x) as usize].push(i as u32);
                }
            }
        }
        for (i, tile) in tiles.iter().enumerate() {
            let actual = &indices[tile[0] as usize..(tile[0] + tile[1]) as usize];
            if actual != expected[i] {
                return Err("tile command order/coverage".into());
            }
        }
        let reference = path.with_extension("rgba");
        let len = width as usize * height as usize * 4;
        if len > 64 << 20 {
            return Err("reference byte limit".into());
        }
        let expected = read_bounded(&reference, len)?;
        if expected.len() != len {
            return Err("reference dimensions".into());
        }
        Ok(Self {
            width,
            height,
            commands,
            assets,
            tiles,
            indices,
            expected,
            name: path.file_stem().unwrap().to_string_lossy().into_owned(),
            _validated: (),
        })
    }
    pub fn bytes(&self) -> usize {
        self.expected.len()
            + self.commands.len() * 40
            + self.tiles.len() * 8
            + self.indices.len() * 4
            + self.assets.iter().map(|a| a.data.len() + 64).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let name = format!(
                "hotaru-gpu-scene-reader-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn blank() -> Vec<u8> {
        let mut b = b"HTSCN001".to_vec();
        for n in [1u32, 1, 0, 0, 1, 0, 0, 0] {
            b.extend(n.to_le_bytes());
        }
        b
    }
    #[test]
    fn reference_must_have_exact_read_length() {
        let temp = Temp::new();
        let path = temp.0.join("case.scene");
        let reference = path.with_extension("rgba");
        std::fs::write(&path, blank()).unwrap();
        std::fs::write(&reference, [0u8; 4]).unwrap();
        assert!(Scene::load(&path).is_ok());
        for data in [vec![], vec![0; 3], vec![0; 5]] {
            std::fs::write(&reference, data).unwrap();
            assert!(Scene::load(&path).is_err());
        }
    }
    #[test]
    fn actual_stream_read_is_bounded_and_trailing_bytes_are_rejected() {
        let temp = Temp::new();
        let path = temp.0.join("case.scene");
        std::fs::write(&path, [0u8; 9]).unwrap();
        assert!(read_bounded(&path, 8).is_err());
        std::fs::write(&path, [0u8; 8]).unwrap();
        assert_eq!(read_bounded(&path, 8).unwrap().len(), 8);
        let mut b = blank();
        b.push(0);
        std::fs::write(&path, b).unwrap();
        assert!(Scene::load(&path).is_err());
    }
    #[test]
    fn dimensions_and_tile_ranges_are_checked_before_reference_read() {
        let temp = Temp::new();
        let path = temp.0.join("case.scene");
        let mut b = blank();
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, b).unwrap();
        assert!(Scene::load(&path).is_err());
        let mut b = blank();
        b[36..40].copy_from_slice(&1u32.to_le_bytes());
        std::fs::write(&path, b).unwrap();
        assert!(Scene::load(&path).is_err());
    }
}
