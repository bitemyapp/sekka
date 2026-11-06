//! Which monitor a connector's EDID describes: the maker's PNP id, the
//! product code and serial, the name, and, from a DisplayID extension, the
//! container ID the monitor shares with its USB functions and the tiled
//! display it is a tile of.
//!
//! Make, model and serial are worked out exactly as the compositor works
//! them out for its outputs (`aizome-comp-core`'s `parse_edid`), so they
//! match what Settings writes under `[[outputs]]`.

/// What an EDID says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edid {
    /// The PNP id: `APP`, `BNQ`, `DEL`.
    pub make: String,
    /// The product code.
    pub product: u16,
    /// The binary serial number (0 when there is none).
    pub serial_number: u32,
    /// The monitor name descriptor (`StudioDisplay`), else the product
    /// code (`0xAE3A`): the compositor's model.
    pub model: String,
    /// The serial number descriptor, else the binary serial, else empty:
    /// the compositor's serial.
    pub serial: String,
    /// DisplayID's container ID: the same 16 bytes as the monitor's USB
    /// devices' Container ID capability, where it has any.
    pub container_id: Option<[u8; 16]>,
    /// The tiled display this connector shows a part of.
    pub tile_group: Option<TileGroup>,
}

/// A tiled display, as DisplayID's tiled display topology block names it:
/// every tile of one monitor carries the same.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileGroup {
    pub make: String,
    pub product: u16,
    pub serial: u32,
}

fn pnp(id: u16) -> String {
    let letter = |v: u16| (b'A' + ((v & 0x1f) as u8).saturating_sub(1)) as char;
    [letter(id >> 10), letter(id >> 5), letter(id)].iter().collect()
}

/// Parse an EDID (the base block and any extensions).
pub fn parse(e: &[u8]) -> Option<Edid> {
    if e.len() < 128 || e[0..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return None;
    }
    let make = pnp(u16::from_be_bytes([e[8], e[9]]));
    let product = u16::from_le_bytes([e[10], e[11]]);
    let serial_number = u32::from_le_bytes([e[12], e[13], e[14], e[15]]);
    let mut model = format!("0x{product:04X}");
    let mut serial = if serial_number != 0 { serial_number.to_string() } else { String::new() };
    for d in 0..4 {
        let b = &e[54 + d * 18..72 + d * 18];
        if b[0] == 0 && b[1] == 0 {
            let text: String =
                b[5..18].iter().take_while(|c| **c != b'\n').map(|c| *c as char).collect::<String>().trim().to_string();
            match b[3] {
                0xfc if !text.is_empty() => model = text,
                0xff if !text.is_empty() => serial = text,
                _ => {}
            }
        }
    }
    let mut edid = Edid { make, product, serial_number, model, serial, container_id: None, tile_group: None };
    for block in e[128..].as_chunks::<128>().0 {
        if block[0] == 0x70 {
            display_id(block, &mut edid);
        }
    }
    Some(edid)
}

/// The DisplayID section of an EDID extension block: its data blocks
/// start at the fifth byte (after the version, the section's length, the
/// product type and the extension count).
fn display_id(block: &[u8], edid: &mut Edid) {
    let end = (5 + block[2] as usize).min(block.len() - 1);
    let mut at = 5;
    while at + 3 <= end {
        let (tag, len) = (block[at], block[at + 2] as usize);
        if at + 3 + len > end {
            return;
        }
        let payload = &block[at + 3..at + 3 + len];
        match tag {
            // ContainerID (DisplayID 2.0's tag; Apple puts it in 1.2
            // sections too).
            0x29 if len == 16 => edid.container_id = payload.try_into().ok(),
            // Tiled display topology: 0x12 in DisplayID 1.3, 0x28 in 2.0.
            0x12 | 0x28 if len >= 22 => {
                let make: String = payload[13..16].iter().map(|c| *c as char).collect();
                edid.tile_group = Some(TileGroup {
                    make,
                    product: u16::from_le_bytes([payload[16], payload[17]]),
                    serial: u32::from_le_bytes([payload[18], payload[19], payload[20], payload[21]]),
                });
            }
            _ => {}
        }
        at += 3 + len;
    }
}

/// Whether a USB Container ID names the same container: compared as the
/// bytes are, and with the first three fields byte-swapped, since USB
/// tools read the capability as a little-endian GUID.
pub fn same_container(display: &[u8; 16], usb: &[u8; 16]) -> bool {
    if display == usb {
        return true;
    }
    let mut swapped = *usb;
    swapped[0..4].reverse();
    swapped[4..6].reverse();
    swapped[6..8].reverse();
    *display == swapped
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An EDID like a 2022 Studio Display's tiles (APP, product 0xAE3A,
    /// named StudioDisplay), with a DisplayID 1.2 extension carrying a
    /// container ID and the tile block, as the kernel shows them; the
    /// serial and container ID are made up.
    pub(crate) fn studio_display(tile: u8) -> Vec<u8> {
        let mut e = base(*b"APP", 0xAE3A, 0x1234_5678, "StudioDisplay", None);
        let mut ext = vec![0x70, 0x12, 0, 0, 0];
        ext.extend([0x29, 0x00, 0x10]);
        ext.extend(CONTAINER);
        ext.extend([0x12, 0x00, 0x16]);
        // Two tiles across; this one at column `tile`; 2560 x 2880.
        ext.extend([0x82, 0x10, tile << 4, 0x00, 0xff, 0x09, 0x3f, 0x0b, 0, 0, 0, 0, 0]);
        ext.extend(*b"APP");
        ext.extend(0xAE3Bu16.to_le_bytes());
        ext.extend(0x1234_5678u32.to_le_bytes());
        ext[2] = (ext.len() - 5) as u8;
        ext.resize(128, 0);
        e[126] = 1;
        e.extend(ext);
        e
    }

    pub(crate) const CONTAINER: [u8; 16] = [1, 0xe7, 0x9f, 0x68, 0x2d, 0xf4, 0x41, 0xf3, 0xba, 0x52, 9, 9, 9, 9, 9, 9];

    /// A base block: `make`, product, serial number, a name and an optional
    /// serial descriptor.
    pub(crate) fn base(make: [u8; 3], product: u16, serial: u32, name: &str, text_serial: Option<&str>) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[0..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        let id = make.iter().fold(0u16, |a, c| (a << 5) | (c - b'A' + 1) as u16);
        e[8..10].copy_from_slice(&id.to_be_bytes());
        e[10..12].copy_from_slice(&product.to_le_bytes());
        e[12..16].copy_from_slice(&serial.to_le_bytes());
        let mut descriptor = |slot: usize, tag: u8, text: &str| {
            let d = &mut e[54 + slot * 18..72 + slot * 18];
            d[3] = tag;
            let mut bytes: Vec<u8> = text.bytes().take(13).collect();
            if bytes.len() < 13 {
                bytes.push(b'\n');
            }
            bytes.resize(13, b' ');
            d[5..18].copy_from_slice(&bytes);
        };
        descriptor(1, 0xfc, name);
        if let Some(s) = text_serial {
            descriptor(2, 0xff, s);
        }
        e
    }

    #[test]
    fn make_model_and_serial_as_the_compositor_reads_them() {
        let benq = parse(&base(*b"BNQ", 32859, 0x0101_0101, "BenQ RD280U", Some("G5R0013401Q"))).unwrap();
        assert_eq!(
            (benq.make.as_str(), benq.model.as_str(), benq.serial.as_str()),
            ("BNQ", "BenQ RD280U", "G5R0013401Q")
        );
        assert_eq!((benq.container_id, benq.tile_group), (None, None));
        let studio = parse(&studio_display(0)).unwrap();
        assert_eq!(
            (studio.make.as_str(), studio.model.as_str(), studio.serial.as_str()),
            ("APP", "StudioDisplay", "305419896")
        );
        assert_eq!(studio.product, 0xAE3A);
        assert!(parse(&[0u8; 128]).is_none());
        assert!(parse(&[0u8; 12]).is_none());
    }

    #[test]
    fn display_id_gives_the_container_and_the_tile_group() {
        let left = parse(&studio_display(0)).unwrap();
        let right = parse(&studio_display(1)).unwrap();
        assert_eq!(left.container_id, Some(CONTAINER));
        let group = TileGroup { make: "APP".into(), product: 0xAE3B, serial: 0x1234_5678 };
        assert_eq!(left.tile_group.as_ref(), Some(&group));
        assert_eq!(right.tile_group, left.tile_group, "both tiles name one monitor");
    }

    #[test]
    fn a_truncated_display_id_block_is_ignored() {
        let mut e = studio_display(0);
        // The section says it runs past the block.
        e[128 + 2] = 200;
        e[128 + 5 + 2] = 120;
        let edid = parse(&e).unwrap();
        assert_eq!(edid.container_id, None);
    }

    #[test]
    fn container_ids_compare_in_either_byte_order() {
        let display = CONTAINER;
        let mut guid = CONTAINER;
        guid[0..4].reverse();
        guid[4..6].reverse();
        guid[6..8].reverse();
        assert!(same_container(&display, &display));
        assert!(same_container(&display, &guid));
        assert!(!same_container(&display, &[0; 16]));
    }
}
