// SPDX-License-Identifier: GPL-3.0-or-later
//! The new partitions as a bar, each as wide as its bytes (with a floor, so
//! a 1 GiB boot partition still shows beside a terabyte), and a legend.
use crate::app::{Genkan, Msg};
use crate::format::size;
use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::kurbo::RoundedRect;
use calamares_nixos::{Filesystem, Firmware, disk::Layout};
use std::hash::{Hash, Hasher};

/// One partition, coloured as Tansu colours partitions by purpose.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub name: String,
    pub bytes: u64,
    pub role: Role,
}

/// The partitions `layout` makes, in disk order.
pub fn segments(layout: &Layout, firmware: Firmware, filesystem: Filesystem) -> Vec<Segment> {
    let mib = 1024 * 1024;
    let mut segments = vec![
        if firmware == Firmware::Uefi {
            Segment {
                name: "EFI system (FAT32)".into(),
                bytes: (layout.boot.1 - layout.boot.0) * mib,
                role: Role::Warning,
            }
        } else {
            Segment {
                name: "BIOS boot".into(),
                bytes: (layout.boot.1 - layout.boot.0) * mib,
                role: Role::Text3,
            }
        },
        Segment {
            name: format!(
                "NixOS ({}){}",
                filesystem.name(),
                if layout.encrypted { " · LUKS2" } else { "" }
            ),
            bytes: layout.root_bytes(),
            role: Role::Accent,
        },
    ];
    if let Some((start, end)) = layout.bios_boot {
        segments.insert(
            1,
            Segment {
                name: "Unencrypted /boot (ext4)".into(),
                bytes: (end - start) * mib,
                role: Role::Warning,
            },
        );
    }
    if let Some(bytes) = layout.swap_bytes() {
        segments.push(Segment {
            name: if layout.encrypted {
                "Swap + zswap · LUKS2"
            } else {
                "Swap + zswap"
            }
            .into(),
            bytes,
            role: Role::Violet,
        });
    }
    segments
}

/// Widths for `bytes` filling `width` with `gap` between them: tiny boot
/// partitions stay visible, the rest are proportional.
pub fn widths(bytes: &[u64], width: f64, gap: f64) -> Vec<f64> {
    let total: u64 = bytes.iter().sum();
    if total == 0 || width <= 0.0 {
        return vec![0.0; bytes.len()];
    }
    let minimum = (width * 0.035).max(10.0);
    let available = (width - gap * (bytes.len() as f64 - 1.0)).max(0.0);
    let raw: Vec<f64> = bytes
        .iter()
        .map(|b| available * *b as f64 / total as f64)
        .collect();
    let small = raw.iter().filter(|w| **w < minimum).count() as f64 * minimum;
    let large: f64 = raw.iter().filter(|w| **w >= minimum).sum();
    let scale = if large > 0.0 {
        ((available - small) / large).max(0.0)
    } else {
        1.0
    };
    raw.into_iter()
        .map(|w| if w < minimum { minimum } else { w * scale })
        .collect()
}

#[derive(Clone)]
struct Bar(Vec<Segment>);

impl Hash for Bar {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for segment in &self.0 {
            segment.bytes.hash(state);
            std::mem::discriminant(&segment.role).hash(state);
        }
    }
}

const HEIGHT: f64 = 26.0;

pub fn view(app: &Genkan) -> Element<Msg> {
    let layout = match app.layout() {
        Ok(layout) => layout,
        Err(message) => {
            return container(text(message).role(Role::Text3).wrap())
                .padding(12.0)
                .width(Fill)
                .into();
        }
    };
    let segments = segments(&layout, app.firmware, app.filesystem);
    let bar = canvas(Bar(segments.clone()), |canvas, bounds, bar, theme| {
        let bytes: Vec<u64> = bar.0.iter().map(|s| s.bytes).collect();
        let gap = 3.0;
        let mut x = bounds.x0;
        for (segment, width) in bar.0.iter().zip(widths(&bytes, bounds.width(), gap)) {
            let shape = RoundedRect::new(x, bounds.y0, x + width, bounds.y1, 3.0);
            canvas.fill(&shape, theme.role(segment.role));
            x += width + gap;
        }
    })
    .height(HEIGHT)
    .width(Fill)
    .label("New partitions");
    // One paragraph, so a narrow window wraps it rather than cutting it off.
    let mut spans = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::new("     "));
        }
        spans.push(Span::new("● ").role(segment.role));
        spans.push(Span::new(format!(
            "{} · {}",
            segment.name,
            size(segment.bytes)
        )));
    }
    let legend = rich_text(spans).wrap().width(Fill);
    container(column![bar, legend].spacing(10.0).width(Fill))
        .padding(12.0)
        .width(Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_partitions_keep_a_visible_width_and_all_fit() {
        let gib = 1024u64.pow(3);
        let widths = widths(&[gib, 900 * gib, 32 * gib], 600.0, 3.0);
        assert!(widths[0] >= 21.0 - 1e-9);
        assert!((widths.iter().sum::<f64>() + 6.0 - 600.0).abs() < 1e-6);
        assert!(widths[1] > widths[2]);
    }
}
