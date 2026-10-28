//! Binary oracle transport: each request is four decimal integers on a line;
//! Each response has a 36-byte header (len, dimensions, signed origins, ink
//! bounds), then the full padded Alpha8 mask. Empty ink uses x=u32::MAX.
use hotaru_sprites::{Metrics, rasterize};
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<u32> = line
            .split_ascii_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(io::Error::other)?;
        let [cp, width, height, thickness] = fields.as_slice() else {
            return Err(io::Error::other("expected four fields"));
        };
        let image = rasterize(
            *cp,
            Metrics {
                cell_width: *width,
                cell_height: *height,
                box_thickness: *thickness,
            },
        )
        .map_err(|e| io::Error::other(format!("{e:?}")))?;
        out.write_all(&(image.alpha.len() as u32).to_le_bytes())?;
        out.write_all(&image.width.to_le_bytes())?;
        out.write_all(&image.height.to_le_bytes())?;
        out.write_all(&image.origin_x.to_le_bytes())?;
        out.write_all(&image.origin_y.to_le_bytes())?;
        let bounds = image
            .ink
            .map_or([u32::MAX, 0, 0, 0], |b| [b.x, b.y, b.width, b.height]);
        for value in bounds {
            out.write_all(&value.to_le_bytes())?;
        }
        out.write_all(&image.alpha)?;
    }
    out.flush()
}
