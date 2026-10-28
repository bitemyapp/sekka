//! Lossless expectations extracted from the pinned upstream PNGs, not this port.
use hotaru_sprites::{Metrics, rasterize};

#[test]
fn every_supported_sprite_matches_four_committed_upstream_atlases() {
    let bytes = include_bytes!("fixtures/ghostty.rle");
    assert_eq!(&bytes[..4], b"HSP1");
    let mut at = 4;
    let mut cases = 0;
    while at < bytes.len() {
        let cp = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let width = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap());
        let height = u16::from_le_bytes(bytes[at + 6..at + 8].try_into().unwrap());
        let thickness = u16::from_le_bytes(bytes[at + 8..at + 10].try_into().unwrap());
        let length = u32::from_le_bytes(bytes[at + 10..at + 14].try_into().unwrap()) as usize;
        at += 14;
        let mut expected = Vec::new();
        let encoded = &bytes[at..at + length];
        assert!(length.is_multiple_of(3));
        for run in encoded.chunks_exact(3) {
            let n = u16::from_le_bytes([run[0], run[1]]) as usize;
            assert_ne!(n, 0);
            expected.extend(std::iter::repeat_n(run[2], n));
        }
        at += length;
        let actual = rasterize(
            cp,
            Metrics {
                cell_width: u32::from(width),
                cell_height: u32::from(height),
                box_thickness: u32::from(thickness),
            },
        )
        .unwrap();
        assert_eq!(
            actual.alpha, expected,
            "U+{cp:04X} {width}x{height}+{thickness}"
        );
        cases += 1;
    }
    assert_eq!(cases, 612);
    assert_eq!(at, bytes.len());
}
