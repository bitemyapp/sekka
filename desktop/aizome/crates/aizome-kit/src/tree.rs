//! A window's widgets, mounted from its elements: an arena of nodes with
//! their key paths, layout, absolute rectangles, clips and paint hashes;
//! the diff between two of them; painting through damage; hit testing and
//! the focus chain.

use std::hash::Hasher;
use std::sync::Arc;

use sidestep_ui::Canvas;

use crate::element::{Common, RawElement};
use crate::hash::{Fx, FxHashMap, PaintHasher, hash_of};
use crate::layout::{Constraints, Length, Point, Rect, Size, Vec2};
use crate::widget::{Env, Focusable, PaintCx, PopoverContent, StateMap, Vacant, Widget};

/// A node's identity: the hash of its key path (each ancestor's key, or
/// its index and kind among unkeyed siblings). It's the same from one
/// view to the next while the element is "the same" one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

impl NodeId {
    pub(crate) const ROOT: NodeId = NodeId(0x9e37_79b9_7f4a_7c15);

    pub(crate) fn keyed(self, key: u64, kind: std::any::TypeId) -> NodeId {
        NodeId(hash_of(&(self.0, 1u8, key, kind)))
    }

    pub(crate) fn indexed(self, index: usize, kind: &str, widget_type: std::any::TypeId) -> NodeId {
        NodeId(hash_of(&(self.0, 2u8, index, kind, widget_type)))
    }

    /// An id under this one for something that isn't an element (a
    /// popup's root).
    pub fn child(self, name: &str) -> NodeId {
        NodeId(hash_of(&(self.0, 3u8, name)))
    }
}

pub(crate) struct Node {
    pub id: NodeId,
    pub widget: Box<dyn Widget>,
    pub common: Common,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Its top left in its parent's coordinates.
    pub local: Point,
    pub size: Size,
    /// Where it is in the window.
    pub rect: Rect,
    /// What its ancestors let show, in the window.
    pub clip: Rect,
    /// Where it and its children draw, clipped: what painting skips by.
    pub bounds: Rect,
    /// Where it and its children surely draw, clipped (their rectangles
    /// and overflow, not the room kept for a focus ring).
    pub covers: Rect,
    /// Its own drawing's hash.
    pub hash: u64,
    pub dead: bool,
    /// The last constraints it was laid out with, and the size it took.
    pub cache: Option<(Constraints, Size)>,
}

impl Node {
    /// What its own drawing covers (for damage): its rectangle and what it
    /// draws outside it, within the clip.
    pub fn own_rect(&self) -> Rect {
        let reach = self.widget.overflow().max(if self.widget.focusable() != Focusable::No { 3.0 } else { 0.0 });
        self.rect.inflate(reach, reach).intersect(self.clip)
    }
}

/// A popover found while mounting: its node and its content.
pub(crate) struct Popover {
    pub node: usize,
    pub content: PopoverContent,
}

#[derive(Default)]
pub(crate) struct Tree {
    pub nodes: Vec<Node>,
    pub by_id: FxHashMap<NodeId, usize>,
    pub names: FxHashMap<Arc<str>, NodeId>,
    pub popovers: Vec<Popover>,
    pub root: Option<usize>,
    /// Ticks widgets asked for while laid out (animations starting).
    pub ticks: Vec<(NodeId, std::time::Duration)>,
    pub widget_timers: Vec<(NodeId, u32, Option<std::time::Duration>)>,
    /// What widgets found changed while laid out (their own coordinates):
    /// an editor's lines.
    pub local_damage: Vec<(usize, Rect)>,
    /// Content widgets scrolled while laid out.
    pub scrolls: Vec<Scrolled>,
}

/// A node as last drawn, kept from one build to the next for the diff:
/// what its drawing covered (clipped), its hash, and its rectangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Drawn {
    pub clip: Rect,
    pub hash: u64,
    pub rect: Rect,
}

pub(crate) type Snapshot = FxHashMap<NodeId, Drawn>;

/// Content a widget moved while laid out (a scroll): what was in `rect`
/// (its coordinates) is now `delta` away, and `fixed` (bars, a header:
/// what stays put over the content) wants drawing again.
pub(crate) struct Scrolled {
    pub node: usize,
    pub rect: Rect,
    pub delta: Vec2,
    pub fixed: Vec<Rect>,
}

/// A region of the window whose pixels move: `node`'s content in `rect`
/// (window coordinates), by `delta`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Moved {
    pub node: usize,
    pub rect: Rect,
    pub delta: Vec2,
}

impl Tree {
    /// A tree of `root`, under the id `base`.
    pub fn new(root: RawElement, base: NodeId) -> Tree {
        let mut t = Tree::default();
        let r = t.mount_under(root, None, 0, base);
        t.root = Some(r);
        t
    }

    pub fn root(&self) -> usize {
        self.root.expect("a root")
    }

    pub fn mount(&mut self, el: RawElement, parent: Option<usize>, index: usize) -> usize {
        let base = parent.map_or(NodeId::ROOT, |p| self.nodes[p].id);
        self.mount_under(el, parent, index, base)
    }

    fn mount_under(&mut self, el: RawElement, parent: Option<usize>, index: usize, base: NodeId) -> usize {
        let RawElement { mut widget, common, children } = el;
        let mut id = match common.key {
            // A key preserves state across moves only for the same widget
            // type. Replacing its concrete type ends timers and state together.
            Some(k) => base.keyed(k, widget.as_ref().type_id()),
            None => base.indexed(index, widget.name(), widget.as_ref().type_id()),
        };
        if self.by_id.contains_key(&id) {
            // Siblings with the same key: tell them apart by place.
            id = id.indexed(index, "duplicate", widget.as_ref().type_id());
        }
        let idx = self.nodes.len();
        if let Some(name) = &common.name {
            self.names.insert(name.clone(), id);
        }
        let popover = widget.take_popover();
        self.nodes.push(Node {
            id,
            widget,
            common,
            parent,
            children: Vec::new(),
            local: Point::ZERO,
            size: Size::ZERO,
            rect: Rect::ZERO,
            clip: Rect::ZERO,
            bounds: Rect::ZERO,
            covers: Rect::ZERO,
            hash: 0,
            dead: false,
            cache: None,
        });
        self.by_id.insert(id, idx);
        if let Some(content) = popover {
            self.popovers.push(Popover { node: idx, content });
        }
        let kids: Vec<usize> =
            children.into_iter().enumerate().map(|(i, c)| self.mount_under(c, Some(idx), i, id)).collect();
        self.nodes[idx].children = kids;
        idx
    }

    /// Forget the layout of `idx` and what's under it.
    pub fn uncache(&mut self, idx: usize) {
        self.nodes[idx].cache = None;
        for k in self.nodes[idx].children.clone() {
            self.uncache(k);
        }
    }

    /// Forget the layout of `idx` and of everything around it up to the
    /// root, so the next layout places it anew (and what it moved).
    pub fn uncache_up(&mut self, mut idx: usize) {
        loop {
            self.nodes[idx].cache = None;
            match self.nodes[idx].parent {
                Some(p) => idx = p,
                None => return,
            }
        }
    }

    /// Drop the dead nodes, renumbering the live ones.
    pub fn compact(&mut self) {
        let live = self.nodes.iter().filter(|n| !n.dead).count();
        if live == self.nodes.len() {
            return;
        }
        let mut map = vec![usize::MAX; self.nodes.len()];
        let mut next = 0;
        for (i, n) in self.nodes.iter().enumerate() {
            if !n.dead {
                map[i] = next;
                next += 1;
            }
        }
        let old = std::mem::take(&mut self.nodes);
        for mut n in old.into_iter().filter(|n| !n.dead) {
            n.parent = n.parent.map(|p| map[p]);
            n.children = n.children.iter().map(|c| map[*c]).filter(|c| *c != usize::MAX).collect();
            self.nodes.push(n);
        }
        self.by_id = self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        self.root = self.root.map(|r| map[r]);
        self.popovers.retain_mut(|p| {
            p.node = map[p.node];
            p.node != usize::MAX
        });
    }

    /// Take a node and its children out.
    pub fn kill(&mut self, idx: usize) {
        let n = &mut self.nodes[idx];
        n.dead = true;
        if self.by_id.get(&n.id) == Some(&idx) {
            self.by_id.remove(&n.id);
        }
        for k in std::mem::take(&mut self.nodes[idx].children) {
            self.kill(k);
        }
    }

    /// Lay `idx` out within `bc`, as narrowed by its own lengths.
    pub fn layout(&mut self, idx: usize, bc: Constraints, env: &mut Env) -> Size {
        let bc = narrow(&self.nodes[idx].common, bc);
        if let Some((c, s)) = self.nodes[idx].cache
            && c == bc
        {
            return s;
        }
        let mut widget = std::mem::replace(&mut self.nodes[idx].widget, Box::new(Vacant));
        let size = {
            let mut cx = crate::widget::LayoutCx { tree: self, node: idx, env };
            widget.layout(&mut cx, bc)
        };
        let node = &mut self.nodes[idx];
        node.widget = widget;
        let size = bc.constrain(node.common.clamp(size));
        let size = Size::new(size.width.max(0.0), size.height.max(0.0));
        node.size = size;
        node.cache = Some((bc, size));
        size
    }

    /// Lay the whole tree out in a window of `size`, then place, clip and
    /// hash every node.
    pub fn layout_window(&mut self, size: Size, env: &mut Env) {
        let root = self.root();
        self.layout(root, Constraints::tight(size), env);
        self.nodes[root].local = Point::ZERO;
        let window = Rect::from_origin_size(Point::ZERO, size);
        self.finish(root, Point::ZERO, window, env.theme.generation);
    }

    /// The content widgets scrolled since `before` was taken, split into
    /// regions whose pixels can move (window coordinates) and damage: the
    /// regions that can't move (the widget moved, something else draws
    /// over them, a scroll inside a scroll; all, unless `can_move`), and
    /// what stays put over each.
    pub fn take_scrolls(&mut self, before: &Snapshot, can_move: bool) -> (Vec<Moved>, Vec<Rect>) {
        let scrolls = std::mem::take(&mut self.scrolls);
        let mut moved: Vec<Moved> = Vec::new();
        let mut damage = Vec::new();
        for s in scrolls {
            let Some(n) = self.nodes.get(s.node).filter(|n| !n.dead) else { continue };
            let origin = n.rect.origin().to_vec2();
            for f in &s.fixed {
                push_rect(&mut damage, (*f + origin).intersect(n.clip));
            }
            let rect = (s.rect + origin).intersect(n.rect).intersect(n.clip);
            if rect.area() <= 0.0 || s.delta == Vec2::ZERO {
                continue;
            }
            let same_place = before.get(&n.id).is_some_and(|d| d.rect == n.rect && d.clip == n.own_rect());
            let nested = moved.iter().any(|m| self.is_ancestor(m.node, s.node) || self.is_ancestor(s.node, m.node));
            if !can_move || !same_place || nested || self.covered(s.node, rect) {
                push_rect(&mut damage, rect);
                continue;
            }
            match moved.iter_mut().find(|m| m.node == s.node && m.rect == rect) {
                Some(m) => m.delta += s.delta,
                None => moved.push(Moved { node: s.node, rect, delta: s.delta }),
            }
        }
        (moved, damage)
    }

    /// Whether `a` is an ancestor of `b`.
    pub(crate) fn is_ancestor(&self, a: usize, b: usize) -> bool {
        let mut at = self.nodes[b].parent;
        while let Some(p) = at {
            if p == a {
                return true;
            }
            at = self.nodes[p].parent;
        }
        false
    }

    /// Whether anything painted after `idx` and its children (a later
    /// sibling of it or of an ancestor: an overlay, a sheet) draws in `r`.
    fn covered(&self, idx: usize, r: Rect) -> bool {
        let mut child = idx;
        while let Some(p) = self.nodes[child].parent {
            let kids = &self.nodes[p].children;
            let after = kids.iter().position(|k| *k == child).map_or(kids.len(), |i| i + 1);
            if kids[after..].iter().any(|&k| !self.nodes[k].dead && self.nodes[k].covers.intersect(r).area() > 0.0) {
                return true;
            }
            child = p;
        }
        false
    }

    /// Lay out content that picks its own size (a popup's), up to `max`.
    pub fn layout_free(&mut self, max: Size, env: &mut Env) -> Size {
        let root = self.root();
        let size = self.layout(root, Constraints::loose(max), env);
        let window = Rect::from_origin_size(Point::ZERO, size);
        self.finish(root, Point::ZERO, window, env.theme.generation);
        size
    }

    /// Absolute rectangles, clips, hashes and paint bounds, from `idx`
    /// down.
    fn finish(&mut self, idx: usize, origin: Point, clip: Rect, generation: u64) -> (Rect, Rect) {
        let node = &mut self.nodes[idx];
        let at = origin + node.local.to_vec2();
        node.rect = Rect::from_origin_size(at, node.size);
        node.clip = clip;
        let mut fx = Fx::default();
        fx.write_u64(node.size.width.to_bits());
        fx.write_u64(node.size.height.to_bits());
        fx.write_u64(generation);
        node.widget.hash(&mut PaintHasher(&mut fx));
        node.hash = fx.finish();
        let inner = if node.widget.clips() { clip.intersect(node.rect) } else { clip };
        let mut bounds = node.own_rect();
        let reach = node.widget.overflow();
        let mut covers = node.rect.inflate(reach, reach).intersect(clip);
        let join = |a: Rect, b: Rect| {
            if b.area() <= 0.0 {
                a
            } else if a.area() <= 0.0 {
                b
            } else {
                a.union(b)
            }
        };
        for k in node.children.clone() {
            let (b, c) = self.finish(k, at, inner, generation);
            bounds = join(bounds, b);
            covers = join(covers, c);
        }
        self.nodes[idx].bounds = bounds;
        self.nodes[idx].covers = covers;
        (bounds, covers)
    }

    /// Every live node's place and drawing.
    pub fn snapshot(&self) -> Snapshot {
        let mut s = Snapshot::with_capacity_and_hasher(self.nodes.len(), Default::default());
        for n in self.nodes.iter().filter(|n| !n.dead) {
            s.insert(n.id, Drawn { clip: n.own_rect(), hash: n.hash, rect: n.rect });
        }
        s
    }

    /// What changed since `before` (the rectangles of nodes that moved,
    /// resized, drew differently, came or went), where the pixels of `moved` regions
    /// have moved: what moved with them needs nothing, and what changed
    /// there is drawn where it is now and where its old pixels went. The
    /// focused node (`focus`), whose ring reaches outside, is drawn again
    /// if it moved.
    pub fn diff_moved(&self, before: &Snapshot, moved: &[Moved], focus: Option<NodeId>) -> Vec<Rect> {
        let mut damage = Vec::new();
        // Which region each node's pixels were in.
        let mut region: Vec<Option<usize>> = Vec::new();
        if !moved.is_empty() {
            region = vec![None; self.nodes.len()];
            for (m, mv) in moved.iter().enumerate() {
                let mut stack: Vec<usize> = self.nodes[mv.node].children.clone();
                while let Some(i) = stack.pop() {
                    region[i] = Some(m);
                    stack.extend(self.nodes[i].children.iter().copied());
                }
            }
        }
        let mut seen = 0usize;
        for (i, n) in self.nodes.iter().enumerate().filter(|(_, n)| !n.dead) {
            let now = n.own_rect();
            let Some(was) = before.get(&n.id) else {
                push_rect(&mut damage, now);
                continue;
            };
            seen += 1;
            match region.get(i).copied().flatten().map(|m| moved[m]) {
                Some(m) => {
                    let along = was.hash == n.hash && n.rect == was.rect + m.delta && Some(n.id) != focus;
                    if !along {
                        push_rect(&mut damage, m.carried(was.clip));
                        push_rect(&mut damage, now);
                    }
                    // Pixels outside the region didn't move.
                    for r in outside(was.clip, m.rect).into_iter().chain(outside(now, m.rect)) {
                        push_rect(&mut damage, r);
                    }
                }
                None => {
                    if was.clip != now || was.hash != n.hash {
                        push_rect(&mut damage, was.clip);
                        push_rect(&mut damage, now);
                    }
                }
            }
        }
        if seen < before.len() {
            for (id, was) in before {
                if !self.by_id.get(id).is_some_and(|&i| !self.nodes[i].dead) {
                    match moved.iter().find(|m| m.rect.intersect(was.clip).area() > 0.0) {
                        Some(m) => {
                            push_rect(&mut damage, m.carried(was.clip));
                            for r in outside(was.clip, m.rect) {
                                push_rect(&mut damage, r);
                            }
                        }
                        None => push_rect(&mut damage, was.clip),
                    }
                }
            }
        }
        damage
    }

    /// The nodes under `p`, from the root to the deepest one that takes
    /// the pointer or stops it. Later siblings are over earlier ones; one
    /// that doesn't stop the pointer there lets it through to what's under.
    pub fn path_at(&self, p: Point, within: Option<usize>) -> Vec<usize> {
        let mut path = Vec::new();
        let start = within.unwrap_or_else(|| self.root());
        if !self.nodes[start].rect.contains(p) {
            return path;
        }
        self.hit(start, p, &mut path);
        path
    }

    fn hit(&self, idx: usize, p: Point, path: &mut Vec<usize>) -> bool {
        path.push(idx);
        let n = &self.nodes[idx];
        for &k in n.children.iter().rev() {
            let c = &self.nodes[k];
            if c.dead || !c.rect.contains(p) || !c.clip.contains(p) {
                continue;
            }
            let len = path.len();
            if self.hit(k, p, path) {
                return true;
            }
            path.truncate(len);
        }
        n.widget.solid()
    }

    /// The nodes that take the focus, in order, under `within`.
    pub fn focus_chain(&self, within: Option<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        let start = within.unwrap_or_else(|| self.root());
        self.collect_focus(start, &mut out);
        out
    }

    fn collect_focus(&self, idx: usize, out: &mut Vec<usize>) {
        let n = &self.nodes[idx];
        if n.dead {
            return;
        }
        if matches!(n.widget.focusable(), Focusable::Tab | Focusable::Click)
            && n.rect.width() > 0.0
            && n.rect.height() > 0.0
        {
            out.push(idx);
        }
        for &k in &n.children {
            self.collect_focus(k, out);
        }
    }

    /// The topmost modal node (a sheet), if any.
    pub fn modal(&self) -> Option<usize> {
        let mut found = None;
        let mut stack = vec![self.root()];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if n.dead {
                continue;
            }
            if n.widget.modal() {
                found = Some(i);
            }
            // Visit in paint order: the last modal painted owns input.
            // The stack pops its last item, so push children backwards.
            stack.extend(n.children.iter().rev().copied());
        }
        found
    }

    pub fn get(&self, id: NodeId) -> Option<usize> {
        self.by_id.get(&id).copied().filter(|&i| !self.nodes[i].dead)
    }

    /// Draw what the canvas's damage needs.
    pub fn paint(&self, canvas: &mut Canvas, env: &mut PaintEnv) {
        if let Some(root) = self.root {
            self.paint_node(root, canvas, env);
        }
    }

    fn paint_node(&self, idx: usize, canvas: &mut Canvas, env: &mut PaintEnv) {
        let n = &self.nodes[idx];
        if n.dead || n.bounds.area() <= 0.0 || !canvas.needs(n.bounds) {
            return;
        }
        let focused = env.focused == Some(n.id);
        let make = |canvas: &mut Canvas, env: &mut PaintEnv, over: bool| {
            canvas.save();
            // A widget that clips its children clips its own drawing too.
            if n.widget.clips() {
                canvas.clip_rect(n.rect);
            }
            canvas.translate(n.rect.origin().to_vec2());
            let mut cx = PaintCx {
                canvas,
                theme: env.theme,
                text: env.text,
                state: crate::widget::StateView::new(env.states, n.id),
                size: n.size,
                hovered: env.hot == Some(n.id),
                pressed: env.active == Some(n.id) && env.hot == Some(n.id),
                focused,
                focus_visible: env.focus_visible,
                window_active: env.window_active,
                now: env.now,
            };
            if over {
                n.widget.paint_over(&mut cx);
            } else {
                n.widget.paint(&mut cx);
            }
            canvas.restore();
        };
        make(canvas, env, false);
        if !n.children.is_empty() {
            canvas.save();
            if n.widget.clips() {
                canvas.clip_rect(n.rect);
            }
            for &k in &n.children {
                self.paint_node(k, canvas, env);
            }
            canvas.restore();
        }
        make(canvas, env, true);
    }
}

/// What painting a tree needs besides the canvas.
pub(crate) struct PaintEnv<'a> {
    pub theme: &'a crate::theme::Theme,
    pub text: &'a mut crate::text::TextCache,
    pub states: &'a StateMap,
    pub hot: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focused: Option<NodeId>,
    pub focus_visible: bool,
    pub window_active: bool,
    pub now: std::time::Instant,
}

/// Damage, skipping what's empty or already covered.
impl Moved {
    /// Where the pixels of `r` that were in the region are now.
    fn carried(&self, r: Rect) -> Rect {
        let inside = r.intersect(self.rect);
        if inside.area() <= 0.0 { Rect::ZERO } else { (inside + self.delta).intersect(self.rect) }
    }
}

/// The parts of `a` outside `r`: bands above and below, and beside.
fn outside(a: Rect, r: Rect) -> Vec<Rect> {
    let i = a.intersect(r);
    if i.area() <= 0.0 {
        return vec![a];
    }
    vec![
        Rect::new(a.x0, a.y0, a.x1, i.y0),
        Rect::new(a.x0, i.y1, a.x1, a.y1),
        Rect::new(a.x0, i.y0, i.x0, i.y1),
        Rect::new(i.x1, i.y0, a.x1, i.y1),
    ]
}

fn push_rect(damage: &mut Vec<Rect>, r: Rect) {
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    if damage.iter().any(|d| d.contains_rect(r)) {
        return;
    }
    damage.retain(|d| !r.contains_rect(*d));
    damage.push(r);
}

/// `bc` narrowed by an element's own width and height.
pub(crate) fn narrow(c: &Common, mut bc: Constraints) -> Constraints {
    match c.width {
        Length::Fixed(w) => bc = bc.with_width(w.max(bc.min.width)),
        Length::Fill | Length::FillPortion(_) if bc.max.width.is_finite() => bc.min.width = bc.max.width,
        _ => {}
    }
    match c.height {
        Length::Fixed(h) => bc = bc.with_height(h.max(bc.min.height)),
        Length::Fill | Length::FillPortion(_) if bc.max.height.is_finite() => bc.min.height = bc.max.height,
        _ => {}
    }
    if let Some(m) = c.max_width {
        bc.max.width = bc.max.width.min(m.max(bc.min.width));
    }
    if let Some(m) = c.max_height {
        bc.max.height = bc.max.height.min(m.max(bc.min.height));
    }
    if let Some(m) = c.min_width {
        bc.min.width = bc.min.width.max(m).min(bc.max.width);
    }
    if let Some(m) = c.min_height {
        bc.min.height = bc.min.height.max(m).min(bc.max.height);
    }
    bc
}

impl Tree {
    /// The damage widgets found while laid out, in the window.
    pub(crate) fn take_local_damage(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.local_damage)
            .into_iter()
            .filter(|(i, _)| !self.nodes[*i].dead)
            .map(|(i, r)| {
                let n = &self.nodes[i];
                (r + n.rect.origin().to_vec2()).intersect(n.clip)
            })
            .filter(|r| r.area() > 0.0)
            .collect()
    }
}
