//! The sunburst's geometry, apart from drawing: which segments a folder's
//! rings hold, where each sits, what's under a point, and how rings move
//! when the centre goes down a level or up one.
//!
//! A segment's angles are fractions of the full turn (0 at twelve o'clock,
//! clockwise). Ring 0 is the centre disk; ring `d` holds what is `d` levels
//! below the centre.

use std::f64::consts::{FRAC_PI_2, TAU};

use aizome_kit::{Point, Size};

use crate::tree::{Kind, NodeId, Tree, flags};

/// Rings shown beyond the centre.
pub const RINGS: usize = 5;

/// Segments narrower than this share of the turn go into their parent's
/// "smaller items" segment.
pub const MIN_SPAN: f64 = 0.0025;

/// One segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    /// What it shows; `None` for the smaller items of `parent`.
    pub node: Option<NodeId>,
    pub parent: NodeId,
    /// 1 for the centre's children.
    pub depth: u8,
    /// Which of the centre's children it's under (its colour).
    pub branch: u16,
    /// Which child of that one it's under (its colour once that child is
    /// the centre); 0 in the first ring.
    pub sub: u16,
    pub x0: f64,
    pub x1: f64,
    pub is_dir: bool,
}

impl Seg {
    pub fn span(&self) -> f64 {
        self.x1 - self.x0
    }
}

/// The segments of `center`'s rings, `depth` deep, by ring then angle.
pub fn segments(tree: &Tree, center: NodeId, depth: usize) -> Vec<Seg> {
    let mut out = Vec::new();
    let total = tree.node(center).alloc.max(1) as f64;
    // Breadth first, so segments come ring by ring, each ring in angle order.
    let mut level: Vec<(NodeId, f64, f64, u16, u16)> = vec![(center, 0.0, 1.0, 0, 0)];
    for d in 1..=depth {
        let mut next = Vec::new();
        for &(parent, px0, px1, branch, sub) in &level {
            let pn = tree.node(parent);
            if !pn.is_dir() || pn.count == 0 {
                continue;
            }
            let mut at = px0;
            let mut small_from = None;
            for (i, c) in tree.sorted_children(parent).into_iter().enumerate() {
                let n = tree.node(c);
                if n.has(flags::REMOVED) || n.alloc == 0 {
                    continue;
                }
                let span = n.alloc as f64 / total;
                let i = i.min(u16::MAX as usize) as u16;
                let (b, sb) = match d {
                    1 => (i, 0),
                    2 => (branch, i),
                    _ => (branch, sub),
                };
                if span < MIN_SPAN {
                    // Largest first: the rest are smaller still.
                    small_from.get_or_insert(at);
                    at += span;
                    continue;
                }
                out.push(Seg {
                    node: Some(c),
                    parent,
                    depth: d as u8,
                    branch: b,
                    sub: sb,
                    x0: at,
                    x1: at + span,
                    is_dir: n.kind == Kind::Dir,
                });
                if n.is_dir() {
                    next.push((c, at, at + span, b, sb));
                }
                at += span;
            }
            if let Some(from) = small_from
                && at - from >= MIN_SPAN / 4.0
            {
                out.push(Seg {
                    node: None,
                    parent,
                    depth: d as u8,
                    branch,
                    sub,
                    x0: from,
                    x1: at.min(px1),
                    is_dir: false,
                });
            }
        }
        level = next;
    }
    out
}

/// Where rings go in a square of `size`: the centre point, the centre
/// disk's radius, and each ring's width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub center: Point,
    pub hole: f64,
    pub ring: f64,
    pub rings: usize,
}

impl Geometry {
    pub fn new(size: Size, rings: usize) -> Geometry {
        let r = (size.width.min(size.height) / 2.0 - 14.0).max(40.0);
        let hole = (r * 0.25).max(44.0).min(r * 0.45);
        let ring = ((r - hole) / rings as f64).floor().max(6.0);
        Geometry { center: Point::new((size.width / 2.0).round(), (size.height / 2.0).round()), hole, ring, rings }
    }

    /// The radius of boundary `q`: 0 at the very middle (`q = -1`), the
    /// centre disk's edge at 0, then a ring per unit. Continuous, so rings
    /// can move between places.
    pub fn boundary(&self, q: f64) -> f64 {
        if q <= 0.0 { self.hole * (1.0 + q).max(0.0) } else { self.hole + q * self.ring }
    }

    /// The outermost radius.
    pub fn outer(&self) -> f64 {
        self.boundary(self.rings as f64)
    }

    /// Inner and outer radius of a ring at place `p` (1 is the first ring).
    pub fn radii(&self, p: f64) -> (f64, f64) {
        (self.boundary(p - 1.0), self.boundary(p))
    }

    /// The angle (radians, kurbo's: from +x, clockwise on screen) of a
    /// fraction of the turn from twelve o'clock.
    pub fn angle(x: f64) -> f64 {
        x * TAU - FRAC_PI_2
    }

    /// What's at `p`: the centre, a ring and fraction, or nothing.
    pub fn locate(&self, p: Point) -> Place {
        let v = p - self.center;
        let r = v.hypot();
        if r < self.hole {
            return Place::Center;
        }
        let ring = ((r - self.hole) / self.ring).floor() as usize + 1;
        if ring > self.rings {
            return Place::Outside;
        }
        let x = ((v.y.atan2(v.x) + FRAC_PI_2) / TAU).rem_euclid(1.0);
        Place::Ring(ring as u8, x)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Center,
    Ring(u8, f64),
    Outside,
}

/// The segment at a ring and fraction.
pub fn hit(segs: &[Seg], ring: u8, x: f64) -> Option<usize> {
    let start = segs.partition_point(|s| s.depth < ring);
    let end = segs.partition_point(|s| s.depth <= ring);
    let slice = &segs[start..end];
    let i = slice.partition_point(|s| s.x1 <= x);
    (i < slice.len() && slice[i].x0 <= x).then_some(start + i)
}

/// A move of the centre one level down (`zoom` from 0 to 1) or up (1 to 0),
/// drawn in the outer folder's segments: `inner` (the child going to or
/// from the centre) spans `c0..c1` of the outer view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub c0: f64,
    pub c1: f64,
    /// 0 shows the outer folder, 1 the inner one.
    pub s: f64,
}

impl Zoom {
    /// A fraction of the outer view as it shows at this moment.
    pub fn x(&self, x: f64) -> f64 {
        let w0 = self.s * self.c0;
        let w1 = 1.0 - self.s * (1.0 - self.c1);
        ((x - w0) / (w1 - w0).max(1e-9)).clamp(0.0, 1.0)
    }

    /// Where ring `depth` of the outer view is at this moment.
    pub fn place(&self, depth: u8) -> f64 {
        f64::from(depth) - self.s
    }
}

/// Ease in and out (a cubic), for `t` from 0 to 1.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Entry, Tree};
    use std::path::PathBuf;

    fn tree() -> Tree {
        let e = |n: &str, k: Kind, a: u64| Entry { name: n.into(), kind: k, flags: 0, alloc: a, apparent: a, mtime: 0 };
        let mut t = Tree::new(PathBuf::from("/r"), Kind::Dir, 0, 0, 0);
        let f = t.push_children(
            NodeId::ROOT,
            &[e("big", Kind::Dir, 0), e("file", Kind::File, 250), e("tiny", Kind::File, 1)],
        );
        t.push_children(NodeId(f), &[e("a", Kind::File, 500), e("b", Kind::File, 249)]);
        t.finish();
        t
    }

    #[test]
    fn rings_by_size_with_small_items_together() {
        let t = tree();
        let segs = segments(&t, NodeId::ROOT, 3);
        let depth1: Vec<_> = segs.iter().filter(|s| s.depth == 1).collect();
        assert_eq!(depth1.len(), 3, "{segs:#?}");
        assert_eq!(t.name(depth1[0].node.unwrap()), "big");
        assert!((depth1[0].span() - 0.749).abs() < 1e-9);
        // "tiny" (0.1%) is under MIN_SPAN: it shows as smaller items.
        assert!(depth1[2].node.is_none());
        let depth2: Vec<_> = segs.iter().filter(|s| s.depth == 2).collect();
        assert_eq!(depth2.len(), 2);
        assert_eq!(depth2[0].x0, 0.0);
        assert!(depth2.iter().all(|s| s.branch == 0));
        assert_eq!(hit(&segs, 2, 0.6), Some(4));
        assert_eq!(hit(&segs, 1, 0.8), Some(1));
        assert_eq!(hit(&segs, 3, 0.1), None);
    }

    #[test]
    fn geometry_locates_points() {
        let g = Geometry::new(Size::new(400.0, 300.0), RINGS);
        assert_eq!(g.locate(g.center), Place::Center);
        let r = g.hole + g.ring * 0.5;
        // Straight up is the start of the turn; straight right a quarter.
        match g.locate(Point::new(g.center.x, g.center.y - r)) {
            Place::Ring(1, x) => assert!(x < 0.01 || x > 0.99),
            p => panic!("{p:?}"),
        }
        match g.locate(Point::new(g.center.x + r, g.center.y)) {
            Place::Ring(1, x) => assert!((x - 0.25).abs() < 0.01),
            p => panic!("{p:?}"),
        }
        assert_eq!(g.locate(Point::new(0.0, 0.0)), Place::Outside);
        assert_eq!(g.boundary(-1.0), 0.0);
        assert_eq!(g.boundary(0.0), g.hole);
    }

    #[test]
    fn zooming_moves_the_child_to_the_centre() {
        let z = Zoom { c0: 0.25, c1: 0.5, s: 1.0 };
        assert_eq!(z.x(0.25), 0.0);
        assert_eq!(z.x(0.5), 1.0);
        assert_eq!(z.x(0.1), 0.0);
        assert_eq!(z.place(1), 0.0);
        let still = Zoom { s: 0.0, ..z };
        assert_eq!(still.x(0.3), 0.3);
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-9);
    }
}
