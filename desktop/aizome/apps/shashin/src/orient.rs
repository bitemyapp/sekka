//! Turning a JPEG without decoding it: its EXIF orientation tag says how
//! to show it, so a quarter turn is a new value of that tag, written in
//! place (or added), and not a pixel changes.

use image::metadata::Orientation;

/// The EXIF orientation that shows a picture as `from` does, then turned
/// `turns` quarter turns clockwise.
pub fn compose(from: u8, turns: u8) -> u8 {
    // Found by doing it to a small picture whose every pixel differs: the
    // eight orientations are the eight ways to lay a rectangle down.
    let probe = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(3, 2, |x, y| {
        image::Rgba([x as u8, y as u8, 0, 255])
    }));
    let shown = |o: u8| {
        let mut p = probe.clone();
        p.apply_orientation(Orientation::from_exif(o).unwrap_or(Orientation::NoTransforms));
        p
    };
    let mut want = shown(from.clamp(1, 8));
    for _ in 0..turns % 4 {
        want = want.rotate90();
    }
    (1..=8).find(|o| shown(*o).as_bytes() == want.as_bytes() && shown(*o).width() == want.width()).unwrap_or(1)
}

/// What went wrong writing a JPEG's orientation.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NotJpeg,
    /// The EXIF block would outgrow its segment (64 KiB).
    TooBig,
    /// The EXIF block isn't one we can read.
    Malformed,
}

const TAG: u16 = 0x0112;

/// `jpeg` with its orientation set to `value`: the tag rewritten where it
/// is, added to the first IFD where it isn't, or an EXIF block of its own
/// added where there's none.
pub fn set_jpeg_orientation(jpeg: &[u8], value: u8) -> Result<Vec<u8>, Refusal> {
    if jpeg.len() < 4 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Err(Refusal::NotJpeg);
    }
    // Find the APP1 "Exif" segment among the markers before the scan.
    let mut at = 2;
    let mut insert_at = 2;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xff {
        let marker = jpeg[at + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        // Fill bytes and markers without a length.
        if marker == 0xff || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            at += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if len < 2 || at + 2 + len > jpeg.len() {
            return Err(Refusal::Malformed);
        }
        if marker == 0xe1 && jpeg.get(at + 4..at + 10) == Some(b"Exif\0\0") {
            let tiff = &jpeg[at + 10..at + 2 + len];
            let new_tiff = set_tiff_orientation(tiff, value)?;
            let new_len = new_tiff.len() + 8;
            if new_len > 0xffff {
                return Err(Refusal::TooBig);
            }
            let mut out = Vec::with_capacity(jpeg.len() + new_len);
            out.extend_from_slice(&jpeg[..at]);
            out.extend_from_slice(&[0xff, 0xe1]);
            out.extend_from_slice(&(new_len as u16).to_be_bytes());
            out.extend_from_slice(b"Exif\0\0");
            out.extend_from_slice(&new_tiff);
            out.extend_from_slice(&jpeg[at + 2 + len..]);
            return Ok(out);
        }
        // A new EXIF block goes after JFIF's APP0, as the standard wants.
        if marker == 0xe0 {
            insert_at = at + 2 + len;
        }
        at += 2 + len;
    }
    let tiff = minimal_tiff(value);
    let seg_len = (tiff.len() + 8) as u16;
    let mut out = Vec::with_capacity(jpeg.len() + tiff.len() + 10);
    out.extend_from_slice(&jpeg[..insert_at]);
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&seg_len.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(&tiff);
    out.extend_from_slice(&jpeg[insert_at..]);
    Ok(out)
}

/// A TIFF structure with one IFD holding only the orientation.
fn minimal_tiff(value: u8) -> Vec<u8> {
    let mut t = b"MM\0\x2a\0\0\0\x08".to_vec();
    t.extend_from_slice(&1u16.to_be_bytes());
    t.extend_from_slice(&TAG.to_be_bytes());
    t.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&u16::from(value).to_be_bytes());
    t.extend_from_slice(&[0, 0]);
    t.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    t
}

struct Endian(bool);

impl Endian {
    fn u16(&self, b: &[u8], at: usize) -> Option<u16> {
        let v = [*b.get(at)?, *b.get(at + 1)?];
        Some(if self.0 { u16::from_le_bytes(v) } else { u16::from_be_bytes(v) })
    }

    fn u32(&self, b: &[u8], at: usize) -> Option<u32> {
        let v = [*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?];
        Some(if self.0 { u32::from_le_bytes(v) } else { u32::from_be_bytes(v) })
    }

    fn put16(&self, v: u16) -> [u8; 2] {
        if self.0 { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    fn put32(&self, v: u32) -> [u8; 4] {
        if self.0 { v.to_le_bytes() } else { v.to_be_bytes() }
    }
}

/// A TIFF structure (an EXIF block) with its orientation set.
fn set_tiff_orientation(tiff: &[u8], value: u8) -> Result<Vec<u8>, Refusal> {
    let e = match tiff.get(..4) {
        Some(b"II\x2a\0") => Endian(true),
        Some(b"MM\0\x2a") => Endian(false),
        _ => return Err(Refusal::Malformed),
    };
    let ifd = e.u32(tiff, 4).ok_or(Refusal::Malformed)? as usize;
    let count = usize::from(e.u16(tiff, ifd).ok_or(Refusal::Malformed)?);
    let entries = ifd + 2;
    if entries + count * 12 + 4 > tiff.len() {
        return Err(Refusal::Malformed);
    }
    for i in 0..count {
        let at = entries + i * 12;
        if e.u16(tiff, at) == Some(TAG) {
            // A SHORT with one value sits in the entry's first two bytes.
            let mut out = tiff.to_vec();
            out[at + 2..at + 4].copy_from_slice(&e.put16(3));
            out[at + 4..at + 8].copy_from_slice(&e.put32(1));
            out[at + 8..at + 10].copy_from_slice(&e.put16(u16::from(value)));
            out[at + 10..at + 12].copy_from_slice(&[0, 0]);
            return Ok(out);
        }
    }
    // No tag: a copy of the first IFD with it added goes at the end, and
    // the header points there. Every other offset stays good, since
    // nothing before the end moves.
    let mut out = tiff.to_vec();
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let new_ifd = out.len();
    let mut list: Vec<[u8; 12]> = (0..count)
        .map(|i| {
            let at = entries + i * 12;
            let mut entry = [0u8; 12];
            entry.copy_from_slice(&tiff[at..at + 12]);
            entry
        })
        .collect();
    let mut ours = [0u8; 12];
    ours[0..2].copy_from_slice(&e.put16(TAG));
    ours[2..4].copy_from_slice(&e.put16(3));
    ours[4..8].copy_from_slice(&e.put32(1));
    ours[8..10].copy_from_slice(&e.put16(u16::from(value)));
    list.push(ours);
    // Entries in tag order, as TIFF wants.
    list.sort_by_key(|entry| e.u16(entry, 0).unwrap_or(0));
    out.extend_from_slice(&e.put16(list.len() as u16));
    for entry in &list {
        out.extend_from_slice(entry);
    }
    let next = e.u32(tiff, entries + count * 12).ok_or(Refusal::Malformed)?;
    out.extend_from_slice(&e.put32(next));
    out[4..8].copy_from_slice(&e.put32(new_ifd as u32));
    Ok(out)
}

/// A JPEG's EXIF orientation (1 if it says none).
pub fn jpeg_orientation(jpeg: &[u8]) -> u8 {
    let mut decoder = match image::ImageReader::new(std::io::Cursor::new(jpeg))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
    {
        Some(d) => d,
        None => return 1,
    };
    use image::ImageDecoder;
    decoder.orientation().map(|o| o.to_exif()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 30) as u8, (y * 30) as u8, 90]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn composes_turns() {
        assert_eq!(compose(1, 0), 1);
        assert_eq!(compose(1, 1), 6);
        assert_eq!(compose(1, 2), 3);
        assert_eq!(compose(1, 3), 8);
        assert_eq!(compose(6, 1), 3);
        assert_eq!(compose(8, 1), 1);
        // Mirrored ones stay mirrored.
        assert!([2, 4, 5, 7].contains(&compose(2, 1)));
    }

    #[test]
    fn adds_an_exif_block_and_rewrites_it() {
        let plain = jpeg(8, 4);
        assert_eq!(jpeg_orientation(&plain), 1);
        let turned = set_jpeg_orientation(&plain, 6).unwrap();
        assert_eq!(jpeg_orientation(&turned), 6);
        // The scan is untouched.
        assert!(turned.ends_with(&plain[plain.len() - 100..]));
        let again = set_jpeg_orientation(&turned, 3).unwrap();
        assert_eq!(jpeg_orientation(&again), 3);
        assert_eq!(again.len(), turned.len(), "rewritten in place");
        let decoded = image::load_from_memory(&turned).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (8, 4), "pixels as stored");
    }

    #[test]
    fn adds_the_tag_to_an_exif_block_without_one() {
        // An EXIF block with one entry (Make), little-endian, and no
        // orientation.
        let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x010fu16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&3u32.to_le_bytes());
        tiff.extend_from_slice(b"Ab\0\0");
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let plain = jpeg(4, 4);
        let mut with = plain[..2].to_vec();
        with.extend_from_slice(&[0xff, 0xe1]);
        with.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        with.extend_from_slice(b"Exif\0\0");
        with.extend_from_slice(&tiff);
        with.extend_from_slice(&plain[2..]);
        assert_eq!(jpeg_orientation(&with), 1);
        let turned = set_jpeg_orientation(&with, 8).unwrap();
        assert_eq!(jpeg_orientation(&turned), 8);
        let exif = exif::Reader::new().read_from_container(&mut std::io::Cursor::new(&turned)).unwrap();
        let make = exif.get_field(exif::Tag::Make, exif::In::PRIMARY).unwrap();
        assert_eq!(make.display_value().to_string(), "\"Ab\"");
    }

    #[test]
    fn refuses_what_isnt_a_jpeg() {
        assert_eq!(set_jpeg_orientation(b"\x89PNG....", 6), Err(Refusal::NotJpeg));
    }
}
