// Adapted from Ghostty sprite/draw/box.zig at the pinned commit; MIT license.
use crate::{Metrics, Raster};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Style {
    None,
    Light,
    Heavy,
    Double,
}
include!("lines_map.rs");

pub(super) fn draw(c: &mut Raster, m: Metrics, [up, right, down, left]: [Style; 4]) {
    use Style::*;
    let light = m.box_thickness;
    let heavy = light * 2;
    let hlt = m.cell_height.saturating_sub(light) / 2;
    let hlb = hlt + light;
    let hht = m.cell_height.saturating_sub(heavy) / 2;
    let hhb = hht + heavy;
    let hdt = hlt.saturating_sub(light);
    let hdb = hlb + light;
    let vll = m.cell_width.saturating_sub(light) / 2;
    let vlr = vll + light;
    let vhl = m.cell_width.saturating_sub(heavy) / 2;
    let vhr = vhl + heavy;
    let vdl = vll.saturating_sub(light);
    let vdr = vlr + light;

    let up_bottom = if left == Heavy || right == Heavy {
        hhb
    } else if left != right || down == up {
        if left == Double || right == Double {
            hdb
        } else {
            hlb
        }
    } else if left == None && right == None {
        hlb
    } else {
        hlt
    };
    let down_top = if left == Heavy || right == Heavy {
        hht
    } else if left != right || up == down {
        if left == Double || right == Double {
            hdt
        } else {
            hlt
        }
    } else if left == None && right == None {
        hlt
    } else {
        hlb
    };
    let left_right = if up == Heavy || down == Heavy {
        vhr
    } else if up != down || left == right {
        if up == Double || down == Double {
            vdr
        } else {
            vlr
        }
    } else if up == None && down == None {
        vlr
    } else {
        vll
    };
    let right_left = if up == Heavy || down == Heavy {
        vhl
    } else if up != down || right == left {
        if up == Double || down == Double {
            vdl
        } else {
            vll
        }
    } else if up == None && down == None {
        vll
    } else {
        vlr
    };

    match up {
        None => {}
        Light => c.rect(vll, 0, vlr, up_bottom, 255),
        Heavy => c.rect(vhl, 0, vhr, up_bottom, 255),
        Double => {
            c.rect(
                vdl,
                0,
                vll,
                if left == Double { hlt } else { up_bottom },
                255,
            );
            c.rect(
                vlr,
                0,
                vdr,
                if right == Double { hlt } else { up_bottom },
                255,
            );
        }
    }
    match right {
        None => {}
        Light => c.rect(right_left, hlt, m.cell_width, hlb, 255),
        Heavy => c.rect(right_left, hht, m.cell_width, hhb, 255),
        Double => {
            c.rect(
                if up == Double { vlr } else { right_left },
                hdt,
                m.cell_width,
                hlt,
                255,
            );
            c.rect(
                if down == Double { vlr } else { right_left },
                hlb,
                m.cell_width,
                hdb,
                255,
            );
        }
    }
    match down {
        None => {}
        Light => c.rect(vll, down_top, vlr, m.cell_height, 255),
        Heavy => c.rect(vhl, down_top, vhr, m.cell_height, 255),
        Double => {
            c.rect(
                vdl,
                if left == Double { hlb } else { down_top },
                vll,
                m.cell_height,
                255,
            );
            c.rect(
                vlr,
                if right == Double { hlb } else { down_top },
                vdr,
                m.cell_height,
                255,
            );
        }
    }
    match left {
        None => {}
        Light => c.rect(0, hlt, left_right, hlb, 255),
        Heavy => c.rect(0, hht, left_right, hhb, 255),
        Double => {
            c.rect(
                0,
                hdt,
                if up == Double { vll } else { left_right },
                hlt,
                255,
            );
            c.rect(
                0,
                hlb,
                if down == Double { vll } else { left_right },
                hdb,
                255,
            );
        }
    }
}
