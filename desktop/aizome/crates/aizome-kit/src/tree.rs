//! A window's widgets, mounted from its elements: an arena of nodes with
//! their key paths, layout, absolute rectangles, clips and paint hashes;
//! the diff between two of them; painting through damage; hit testing and
//! the focus chain.

use std::collections::hash_map::Entry;
use std::hash::Hasher;
use std::sync::Arc;

use sidestep_ui::Canvas;

use crate::element::{Common, RawElement};
use crate::hash::{Fx, FxHashMap, PaintHasher, hash_of};
use crate::layout::{Constraints, Length, Point, Rect, Size, Vec2};
use crate::lazy::{AtMount, Lazy, Lengths, Same, take_after};
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
    /// Mounted, given a new widget or laid out since its place, clip and
    /// hash were last worked out. Layout runs from the root down, so the
    /// nodes above an unsettled one are unsettled too. (Read only in a tree
    /// that keeps nodes: see [`Tree::kept`].)
    unsettled: bool,
    /// Whether a sheet is at or under it: where input stays is found by
    /// reaching every one.
    modal_below: bool,
    lazy: LazyNode,
    /// The view that mounted it or last kept it.
    viewed: View,
    /// The last constraints it was laid out with, and the size it took.
    pub cache: Option<(Constraints, Size)>,
}

/// Whether a node is a lazy element's, with what the element built under
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LazyNode {
    No,
    /// A lazy element given nothing of its own but perhaps a key: no name,
    /// tooltip, label, length or mapping of its messages.
    Plain,
    /// A lazy element with settings of its own.
    Given,
    /// A lazy element to be built again at the next view whatever its
    /// value: something under it asked for a rebuild.
    Spoiled,
}

impl LazyNode {
    /// What a lazy element with the settings `c` is, before it takes any
    /// length after its content.
    fn of(c: &Common) -> LazyNode {
        let plain = c.name.is_none()
            && c.mapper.is_none()
            && c.tooltip.is_none()
            && c.label.is_none()
            && c.width == Length::Shrink
            && c.height == Length::Shrink
            && c.min_width.is_none()
            && c.max_width.is_none()
            && c.min_height.is_none()
            && c.max_height.is_none();
        if plain { LazyNode::Plain } else { LazyNode::Given }
    }

    /// Whether what's under the node may be kept at the next view.
    fn keeps(self) -> bool {
        matches!(self, LazyNode::Plain | LazyNode::Given)
    }
}

/// Which of a tree's views a node is from: the first mounts it, each one
/// after is a number on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct View(u32);

impl View {
    /// The view after this one, while there are numbers left.
    fn next(self) -> Option<View> {
        self.0.checked_add(1).map(View)
    }
}

impl Node {
    /// What its own drawing covers (for damage): its rectangle and what it
    /// draws outside it, within the clip.
    pub fn own_rect(&self) -> Rect {
        let reach = self.widget.overflow().max(if self.widget.focusable() != Focusable::No { 3.0 } else { 0.0 });
        self.rect.inflate(reach, reach).intersect(self.clip)
    }

    /// How far past what clips this node its focus ring may be drawn.
    const RING_PAST_CLIP: f64 = 3.0;

    /// What clips the focus ring when it's drawn around this node: what
    /// clips the node, and a little more, so that a field flush with its
    /// scroll view's edge shows its ring whole.
    pub fn ring_clip(&self) -> Rect {
        self.clip.inflate(Node::RING_PAST_CLIP, Node::RING_PAST_CLIP)
    }

    /// What the focus ring covers when it's drawn around this node (and a
    /// point more). Part of that is outside what clips the node, and so
    /// outside [`own_rect`](Self::own_rect), which is what the comparison
    /// of one layout with the next draws again: the ring's place is
    /// followed apart from that, by the window.
    pub fn ring_rect(&self) -> Rect {
        self.rect.inflate(3.0, 3.0).intersect(self.ring_clip())
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
    /// The topmost modal node (a sheet), as of the last layout: the last
    /// one painted. Nodes come and go only while laid out.
    modal: Option<usize>,
    /// How many of `nodes` are dead, counted as they die: a pass that lays
    /// the tree out again asks, and mustn't read every node to learn it.
    dead: usize,
    /// Whether the tree keeps nodes from one view to the next: it was
    /// mounted with lazy elements in it. Then a view is brought in by
    /// [`rebuild`](Self::rebuild), and each pass works out and compares
    /// what changed rather than every node.
    kept: bool,
    /// How many live nodes are lazy elements'.
    lazies: usize,
    view: View,
    /// The nodes whose place, clip and hash were worked out since the last
    /// comparison with what was drawn (in a tree that keeps nodes).
    settled: Vec<usize>,
    /// The nodes that died since then.
    gone: Vec<NodeId>,
    /// Whether any of those is still gone: its widgets' state can go.
    lost: bool,
    /// Whether a node may be a video's place or drawn over videos (known
    /// only in a tree that keeps nodes, of what came since it was mounted).
    video: bool,
    /// Mounted, and not yet compared with what was drawn: the first
    /// comparison is of every node, as what was drawn is of another tree.
    anew: bool,
    /// Lazy elements built and kept, for whoever counts.
    counts: LazyCounts,
    /// Sheets found so far in the pass that's working places out.
    modals: u32,
}

/// How often lazy elements were built, and how often kept instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LazyCounts {
    /// Times a lazy element's function was called.
    pub built: u64,
    /// Times one was found as it was, and its subtree kept.
    pub kept: u64,
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
    /// A tree of `root`, under the id `base`, with room for `nodes` nodes
    /// to begin with: as many as the tree it takes over from had, since
    /// most views change little from one build to the next.
    pub fn new(root: RawElement, base: NodeId, nodes: usize) -> Tree {
        let mut t = Tree::default();
        t.nodes.reserve(nodes);
        t.by_id.reserve(nodes);
        let r = t.mount_under(root, None, 0, base);
        t.root = Some(r);
        t.kept = t.lazies > 0;
        t.anew = true;
        // Nothing has looked for videos among its nodes yet.
        t.video = true;
        t
    }

    pub fn root(&self) -> usize {
        self.root.expect("a root")
    }

    /// Whether the tree keeps nodes from one view to the next, and so
    /// takes its next view through [`rebuild`](Self::rebuild).
    pub fn kept(&self) -> bool {
        self.kept
    }

    /// Have every pass work out and compare every node, as a tree without
    /// lazy elements does: for a tree that's mounted anew at each view (a
    /// popup's).
    pub fn keep_nothing(&mut self) {
        self.kept = false;
    }

    /// Whether its next view can be brought in with [`rebuild`](Self::rebuild):
    /// it keeps nodes, it has lazy elements for that to be worth it, and
    /// its views haven't run out of numbers (after four thousand million a
    /// tree is mounted anew, as one without lazy elements is each time).
    pub fn takes_views(&self) -> bool {
        self.kept && self.lazies > 0 && self.view.next().is_some()
    }

    /// The lazy elements built and kept since this was last asked.
    pub fn take_lazy_counts(&mut self) -> LazyCounts {
        std::mem::take(&mut self.counts)
    }

    /// Whether a node may be a video's place or drawn over videos. (A tree
    /// that doesn't keep nodes can't say, and answers yes.)
    pub fn may_have_video(&self) -> bool {
        self.video || !self.kept
    }

    /// What a look at every node found: whether any is a video's place or
    /// drawn over videos.
    pub fn found_video(&mut self, any: bool) {
        self.video = any;
    }

    pub fn mount(&mut self, el: RawElement, parent: Option<usize>, index: usize) -> usize {
        let base = parent.map_or(NodeId::ROOT, |p| self.nodes[p].id);
        let first = self.nodes.len();
        let idx = self.mount_under(el, parent, index, base);
        if self.kept && !self.video {
            self.video = self.nodes[first..].iter().any(|n| is_video(n.widget.as_ref()));
        }
        idx
    }

    fn mount_under(&mut self, el: RawElement, parent: Option<usize>, index: usize, base: NodeId) -> usize {
        let RawElement { mut widget, mut common, mut children } = el;
        let kind = widget.as_ref().type_id();
        let mut id = match common.key {
            // A key preserves state across moves only for the same widget
            // type. Replacing its concrete type ends timers and state together.
            Some(k) => base.keyed(k, kind),
            None => base.indexed(index, widget.name(), kind),
        };
        let idx = self.nodes.len();
        // Looked for and put in at one go: this runs for every node of
        // every build.
        match self.by_id.entry(id) {
            Entry::Vacant(place) => {
                place.insert(idx);
            }
            Entry::Occupied(_) => {
                // Siblings with the same key: tell them apart by place.
                id = id.indexed(index, "duplicate", kind);
                self.by_id.insert(id, idx);
            }
        }
        let (lazy, popover) = match widget.at_mount() {
            AtMount::Plain => (LazyNode::No, None),
            AtMount::Popover(content) => (LazyNode::No, Some(content)),
            AtMount::Lazy(element) => {
                let lazy = LazyNode::of(&common);
                children = self.build_lazy(element, &mut common);
                self.lazies += 1;
                (lazy, None)
            }
        };
        if let Some(name) = &common.name {
            self.names.insert(name.clone(), id);
        }
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
            unsettled: true,
            modal_below: false,
            lazy,
            viewed: self.view,
            cache: None,
        });
        if let Some(content) = popover {
            self.popovers.push(Popover { node: idx, content });
        }
        let kids: Vec<usize> =
            children.into_iter().enumerate().map(|(i, c)| self.mount_under(c, Some(idx), i, id)).collect();
        self.nodes[idx].children = kids;
        idx
    }

    /// What a lazy element shows, built now, as its node's children, with
    /// the element sized after it. (Out of line: mounting is every build's
    /// path for every element, and nearly none of them is one of these.)
    #[inline(never)]
    fn build_lazy(&mut self, element: &mut dyn Lazy, common: &mut Common) -> Vec<RawElement> {
        let content = element.build();
        take_after(common, Lengths::of(&content.common));
        self.counts.built += 1;
        vec![content]
    }

    /// Bring the tree up to `root`, the next view of what it was mounted
    /// from, keeping what the view says is as it was.
    ///
    /// An element takes the node it was at the last view (the one with its
    /// id), with its place among the nodes, so most views change nothing
    /// about which nodes there are. A lazy element whose value is the same
    /// keeps that node and all that's under it, untouched: nothing is
    /// built, and nothing here is proportional to what's kept. Any other
    /// element gives the node its new widget and settings, to be laid out
    /// again, and what the last view had there that this one doesn't dies.
    ///
    /// For a tree that [takes views](Self::takes_views).
    pub fn rebuild(&mut self, mut root: RawElement, base: NodeId) {
        self.view = self.view.next().expect("a tree that takes views has one left");
        let before = self.root;
        let now = self.bring(&mut root, None, 0, base, before);
        if let Some(before) = before
            && before != now
        {
            self.kill(before);
        }
        self.root = Some(now);
    }

    /// The node for `el`, child `index` of `parent` (whose id is `base`),
    /// at this view: [`rebuild`](Self::rebuild)'s work for one element and
    /// what's under it. `hint` is the node that was in its place among its
    /// siblings at the last view, most often the very one: then the id map
    /// isn't asked.
    ///
    /// The element is read where it is and taken apart only if a node needs
    /// its parts: one kept is left for its owner to drop, never moved.
    fn bring(
        &mut self,
        el: &mut RawElement,
        parent: Option<usize>,
        index: usize,
        base: NodeId,
        hint: Option<usize>,
    ) -> usize {
        let kind = el.widget.as_ref().type_id();
        let mut id = match el.common.key {
            Some(k) => base.keyed(k, kind),
            None => base.indexed(index, el.widget.name(), kind),
        };
        // The node it was at the last view; one this view has already
        // given a sibling means the two have the same key.
        let mut was = match hint {
            Some(h) if self.nodes[h].id == id && !self.nodes[h].dead => Some(h),
            _ => self.get(id),
        };
        if was.is_some_and(|i| self.nodes[i].viewed == self.view) {
            id = id.indexed(index, "duplicate", kind);
            was = self.get(id).filter(|&i| self.nodes[i].viewed != self.view);
        }
        let (lazy, popover) = match el.widget.at_mount() {
            AtMount::Plain => (LazyNode::No, None),
            AtMount::Popover(content) => (LazyNode::No, Some(content)),
            AtMount::Lazy(element) => {
                if let Some(node) = was
                    && self.nodes[node].lazy.keeps()
                    && let Some(same) = element.same(self.nodes[node].widget.as_ref())
                {
                    return self.keep(node, same, &mut el.common);
                }
                let lazy = LazyNode::of(&el.common);
                el.children = self.build_lazy(element, &mut el.common);
                (lazy, None)
            }
        };
        let widget = std::mem::replace(&mut el.widget, Box::new(Vacant));
        let common = std::mem::take(&mut el.common);
        let mut children = std::mem::take(&mut el.children);
        self.video = self.video || is_video(widget.as_ref());
        let (idx, last) = match was {
            Some(idx) => {
                self.forget_name(idx);
                let n = &mut self.nodes[idx];
                if n.lazy != LazyNode::No {
                    self.lazies -= 1;
                }
                // As a node just mounted is, but for where it was drawn:
                // every node reached from the root has that worked out
                // again.
                n.widget = widget;
                n.common = common;
                n.parent = parent;
                n.local = Point::ZERO;
                n.size = Size::ZERO;
                n.unsettled = true;
                n.lazy = lazy;
                n.viewed = self.view;
                n.cache = None;
                (idx, std::mem::take(&mut n.children))
            }
            None => {
                let idx = self.nodes.len();
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
                    unsettled: true,
                    modal_below: false,
                    lazy,
                    viewed: self.view,
                    cache: None,
                });
                self.by_id.insert(id, idx);
                (idx, Vec::new())
            }
        };
        if lazy != LazyNode::No {
            self.lazies += 1;
        }
        if let Some(name) = &self.nodes[idx].common.name {
            self.names.insert(name.clone(), id);
        }
        if let Some(content) = popover {
            self.popovers.push(Popover { node: idx, content });
        }
        let mut kids = Vec::with_capacity(children.len());
        for (i, child) in children.iter_mut().enumerate() {
            kids.push(self.bring(child, Some(idx), i, id, last.get(i).copied()));
        }
        // What the last view had here and this one didn't take. (A node's
        // id comes of its parent's, so nothing under one of these was
        // taken either.)
        for k in last {
            if !self.nodes[k].dead && self.nodes[k].viewed != self.view {
                self.kill(k);
            }
        }
        self.nodes[idx].children = kids;
        idx
    }

    /// Keep `node`, a lazy element's, and all that's under it as they are:
    /// `same` says the element this view has there would build what the
    /// node has. The element's own settings are this view's (its name, its
    /// tooltip, how its messages are mapped: none of them built from its
    /// value), taken from `common` if there's anything to take.
    fn keep(&mut self, node: usize, _: Same, common: &mut Common) -> usize {
        // Most lazy elements are given nothing but a key, at one view as at
        // the last: then the node has all of it already, and neither it nor
        // what it built is read for more.
        let lazy = LazyNode::of(common);
        if !(lazy == LazyNode::Plain && self.nodes[node].lazy == LazyNode::Plain) {
            if let Some(&content) = self.nodes[node].children.first() {
                take_after(common, Lengths::of(&self.nodes[content].common));
            }
            self.forget_name(node);
            let n = &mut self.nodes[node];
            if let Some(name) = &common.name {
                self.names.insert(name.clone(), n.id);
            }
            n.common = std::mem::take(common);
            n.lazy = lazy;
        }
        // To its parent it is as an element just mounted: nowhere and of no
        // size until laid out and placed. Laid out in the room it had, it
        // has the size it had ([`layout`](Self::layout)), and nothing
        // under it is touched.
        let n = &mut self.nodes[node];
        n.local = Point::ZERO;
        n.size = Size::ZERO;
        n.viewed = self.view;
        self.counts.kept += 1;
        node
    }

    /// Take `idx`'s name out of the names, if it's the one known by it.
    fn forget_name(&mut self, idx: usize) {
        let n = &self.nodes[idx];
        if let Some(name) = &n.common.name
            && self.names.get(name) == Some(&n.id)
        {
            self.names.remove(name);
        }
    }

    /// Have the lazy elements around `idx` built again at the next view,
    /// whatever their values: a widget there asked for the window to be
    /// viewed again, which gives it a new widget, laid out anew.
    pub fn spoil(&mut self, idx: usize) {
        if !self.kept {
            return;
        }
        let mut at = Some(idx);
        while let Some(i) = at {
            let n = &mut self.nodes[i];
            if n.lazy.keeps() {
                n.lazy = LazyNode::Spoiled;
            }
            at = n.parent;
        }
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

    /// How many nodes are dead and still take room ([`compact`](Self::compact)
    /// drops them).
    pub fn dead(&self) -> usize {
        self.dead
    }

    /// Drop the dead nodes, renumbering the live ones.
    pub fn compact(&mut self) {
        if self.dead == 0 {
            return;
        }
        self.dead = 0;
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
        self.modal = self.modal.map(|m| map[m]).filter(|m| *m != usize::MAX);
        self.popovers.retain_mut(|p| {
            p.node = map[p.node];
            p.node != usize::MAX
        });
        // What a layout left for the comparison after it names nodes by
        // place too.
        self.scrolls.retain_mut(|s| {
            s.node = map[s.node];
            s.node != usize::MAX
        });
        self.local_damage.retain_mut(|(node, _)| {
            *node = map[*node];
            *node != usize::MAX
        });
        self.settled.retain_mut(|node| {
            *node = map[*node];
            *node != usize::MAX
        });
    }

    /// Take a node and its children out.
    pub fn kill(&mut self, idx: usize) {
        if !self.nodes[idx].dead {
            if self.kept {
                // What was drawn for it is looked for at the next
                // comparison, and its name mustn't outlive it.
                self.gone.push(self.nodes[idx].id);
                self.forget_name(idx);
            }
            let n = &mut self.nodes[idx];
            n.dead = true;
            self.dead += 1;
            if n.lazy != LazyNode::No {
                self.lazies -= 1;
            }
        }
        let n = &self.nodes[idx];
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
            // (A node kept from another view has no size till it's laid
            // out, as one newly mounted has none: here it has it again.)
            self.nodes[idx].size = s;
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
        node.unsettled = true;
        size
    }

    /// Lay the whole tree out in a window of `size`, then place, clip and
    /// hash every node.
    pub fn layout_window(&mut self, size: Size, env: &mut Env) {
        let root = self.root();
        self.layout(root, Constraints::tight(size), env);
        self.nodes[root].local = Point::ZERO;
        let window = Rect::from_origin_size(Point::ZERO, size);
        self.settle_from(root, window, env.theme.generation);
    }

    /// Places, clips, hashes and paint bounds from `root` down, in a
    /// window of `window`: every node's, or in a tree that keeps nodes,
    /// those of what changed.
    fn settle_from(&mut self, root: usize, window: Rect, generation: u64) {
        self.modal = None;
        self.modals = 0;
        if self.kept {
            self.finish::<true>(root, Point::ZERO, window, generation, false);
        } else {
            self.finish::<false>(root, Point::ZERO, window, generation, false);
        }
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
        self.settle_from(root, window, env.theme.generation);
        size
    }

    /// Absolute rectangles, clips, hashes and paint bounds, from `idx`
    /// down, in the order nodes are painted; and which node is modal.
    /// What it gives back: where `idx` and its children draw, and where
    /// they surely draw.
    ///
    /// In a tree that keeps nodes (`KEPT`), a node that's settled, where it
    /// was and clipped as it was, is left as it is with all that's under
    /// it: nothing there was laid out or given a new widget, so every
    /// place, clip and hash under it would come out the same. Not under a
    /// sheet's ancestors (the sheet has to be reached to be found), and not
    /// while `sweep`ing: inside content whose pixels moved, where the
    /// comparison needs every node, moved or not. The nodes worked out are
    /// listed for the comparison.
    fn finish<const KEPT: bool>(
        &mut self,
        idx: usize,
        origin: Point,
        clip: Rect,
        generation: u64,
        sweep: bool,
    ) -> (Rect, Rect) {
        let node = &mut self.nodes[idx];
        let at = origin + node.local.to_vec2();
        if KEPT {
            if !sweep
                && !node.unsettled
                && !node.modal_below
                && node.rect.origin() == at
                && node.rect.size() == node.size
                && node.clip == clip
            {
                return (node.bounds, node.covers);
            }
            // Kept from the last view, and not laid out at this one (its
            // size is as `keep` left it): its parent no longer shows it.
            // As nodes newly mounted and never laid out are, it and what's
            // under it have no size.
            if node.size == Size::ZERO && node.cache.is_some_and(|(_, s)| s != Size::ZERO) {
                self.unlay(idx);
            }
            self.settled.push(idx);
        }
        let node = &mut self.nodes[idx];
        let before = self.modals;
        node.rect = Rect::from_origin_size(at, node.size);
        node.clip = clip;
        // A settled node that only moved, in a tree that keeps nodes, draws
        // what it drew: its size and its widget are as they were when its
        // hash was last worked out.
        if !KEPT || node.unsettled {
            node.hash = paint_hash(node, generation);
        }
        let inner = if node.widget.clips() { clip.intersect(node.rect) } else { clip };
        // Its own rectangle (as `own_rect` has it) and what it surely
        // covers, asking how far it draws outside itself once.
        let reach = node.widget.overflow();
        let ring = if node.widget.focusable() != Focusable::No { 3.0 } else { 0.0 };
        let mut bounds = node.rect.inflate(reach.max(ring), reach.max(ring)).intersect(clip);
        let mut covers = node.rect.inflate(reach, reach).intersect(clip);
        if !node.dead && node.widget.modal() {
            self.modal = Some(idx);
            self.modals += 1;
        }
        let join = |a: Rect, b: Rect| {
            if b.area() <= 0.0 {
                a
            } else if a.area() <= 0.0 {
                b
            } else {
                a.union(b)
            }
        };
        // Content that scrolled in this layout: its pixels move, and what
        // didn't move with them has to be found.
        let sweep = sweep || (KEPT && self.scrolls.iter().any(|s| s.node == idx && s.delta != Vec2::ZERO));
        // By place rather than over a copy of the list: this runs for every
        // node of every build, and nothing below changes who its children
        // are.
        for i in 0..self.nodes[idx].children.len() {
            let k = self.nodes[idx].children[i];
            let (b, c) = self.finish::<KEPT>(k, at, inner, generation, sweep);
            bounds = join(bounds, b);
            covers = join(covers, c);
        }
        let node = &mut self.nodes[idx];
        node.bounds = bounds;
        node.covers = covers;
        if KEPT {
            node.unsettled = false;
            // A sheet found since this node was come to is at or under it.
            node.modal_below = self.modals != before;
        }
        (bounds, covers)
    }

    /// Leave what's under `idx` as nodes newly mounted and never laid out
    /// are, of no size and nowhere, and `idx` to be laid out anew if it's
    /// asked again.
    fn unlay(&mut self, idx: usize) {
        let n = &mut self.nodes[idx];
        n.cache = None;
        n.unsettled = true;
        for i in 0..self.nodes[idx].children.len() {
            let k = self.nodes[idx].children[i];
            let n = &mut self.nodes[k];
            n.size = Size::ZERO;
            n.local = Point::ZERO;
            self.unlay(k);
        }
    }

    /// Every live node's place and drawing, in place of what `s` held: a
    /// window's snapshot keeps its room from one build to the next.
    pub fn snapshot_into(&self, s: &mut Snapshot) {
        s.clear();
        s.reserve(self.nodes.len());
        for n in self.nodes.iter().filter(|n| !n.dead) {
            s.insert(n.id, Drawn { clip: n.own_rect(), hash: n.hash, rect: n.rect });
        }
    }

    /// What changed since `drawn` was kept (the rectangles of nodes that
    /// moved, resized, drew differently, came or went), where the pixels of
    /// `moved` regions have moved: what moved with them needs nothing, and
    /// what changed there is drawn where it is now and where its old pixels
    /// went. The focused node (`focus`), whose ring reaches outside, is
    /// drawn again if it moved.
    ///
    /// `drawn` is brought up to date as it's read: one look finds what a
    /// node was and keeps what it is, where comparing and then keeping
    /// were two for every node of every build.
    ///
    /// A tree that keeps nodes compares the ones worked out since the last
    /// comparison and looks for the ones that died since: the rest are
    /// where they were, drawing what they drew, and `drawn` already says
    /// so.
    pub fn diff_moved(&mut self, drawn: &mut Snapshot, moved: &[Moved], focus: Option<NodeId>) -> Vec<Rect> {
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
        if self.kept && !std::mem::take(&mut self.anew) {
            let mut settled = std::mem::take(&mut self.settled);
            for &i in &settled {
                if !self.nodes[i].dead {
                    self.compare(i, drawn, &region, moved, focus, &mut damage);
                }
            }
            settled.clear();
            self.settled = settled;
            self.lost = false;
            let mut gone = std::mem::take(&mut self.gone);
            for id in gone.drain(..) {
                // One that died and came back (a row built again) is among
                // the settled.
                if self.get(id).is_none() {
                    self.lost = true;
                    if let Some(was) = drawn.remove(&id) {
                        went(&was, moved, &mut damage);
                    }
                }
            }
            self.gone = gone;
            return damage;
        }
        // Every node, and every one that's gone: nothing listed is needed.
        self.settled.clear();
        self.gone.clear();
        self.lost = true;
        let kept = drawn.len();
        let mut seen = 0usize;
        for i in 0..self.nodes.len() {
            if !self.nodes[i].dead && self.compare(i, drawn, &region, moved, focus, &mut damage) {
                seen += 1;
            }
        }
        if seen < kept {
            // Some that were drawn are gone.
            drawn.retain(|id, was| {
                if self.by_id.get(id).is_some_and(|&i| !self.nodes[i].dead) {
                    return true;
                }
                went(was, moved, &mut damage);
                false
            });
        }
        damage
    }

    /// Live node `i` against what `drawn` has for it, which then has it as
    /// it is: the damage, and whether it was drawn before. (In each of the
    /// two loops over nodes that call it, not a call for every node.)
    #[inline(always)]
    fn compare(
        &self,
        i: usize,
        drawn: &mut Snapshot,
        region: &[Option<usize>],
        moved: &[Moved],
        focus: Option<NodeId>,
        damage: &mut Vec<Rect>,
    ) -> bool {
        let n = &self.nodes[i];
        let now = n.own_rect();
        let is = Drawn { clip: now, hash: n.hash, rect: n.rect };
        let was = match drawn.entry(n.id) {
            Entry::Vacant(place) => {
                place.insert(is);
                push_rect(damage, now);
                return false;
            }
            Entry::Occupied(mut place) => std::mem::replace(place.get_mut(), is),
        };
        match region.get(i).copied().flatten().map(|m| moved[m]) {
            Some(m) => {
                let along = was.hash == n.hash && n.rect == was.rect + m.delta && Some(n.id) != focus;
                if !along {
                    push_rect(damage, m.carried(was.clip));
                    push_rect(damage, now);
                }
                // Pixels outside the region didn't move.
                for r in outside(was.clip, m.rect).into_iter().chain(outside(now, m.rect)) {
                    push_rect(damage, r);
                }
            }
            None => {
                if was.clip != now || was.hash != n.hash {
                    push_rect(damage, was.clip);
                    push_rect(damage, now);
                }
            }
        }
        true
    }

    /// Drop what a layout left for a comparison that isn't to be made (the
    /// whole window is drawn again; `drawn` is written whole).
    pub fn forget_changes(&mut self) {
        self.scrolls.clear();
        self.local_damage.clear();
        self.settled.clear();
        self.gone.clear();
        self.anew = false;
    }

    /// Whether nodes may have gone at the last comparison, their widgets'
    /// state with them. (A tree that doesn't keep nodes can't say, and
    /// answers yes.)
    pub fn lost_nodes(&self) -> bool {
        self.lost || !self.kept
    }

    /// Widget `idx` has just handled an event, which may have changed what
    /// it draws. A tree that keeps nodes doesn't work every hash out again
    /// at its next layout, so this one is worked out now: what to draw
    /// again if it changed, with `drawn` told. (A node waiting to be laid
    /// out has that coming anyway.)
    pub fn touched(&mut self, idx: usize, generation: u64, drawn: &mut Snapshot) -> Option<Rect> {
        if !self.kept {
            return None;
        }
        let n = self.nodes.get_mut(idx).filter(|n| !n.dead && !n.unsettled && n.cache.is_some())?;
        let hash = paint_hash(n, generation);
        let own = n.own_rect();
        let was = drawn.get_mut(&n.id)?;
        if was.clip != own {
            // It reaches further than it did (or less far): what's around
            // it has bounds to work out again, as a layout does.
            self.uncache_up(idx);
            return None;
        }
        if hash == n.hash {
            return None;
        }
        n.hash = hash;
        was.hash = hash;
        Some(own)
    }

    /// The nodes under `p`, from the root to the deepest one that takes
    /// the pointer or stops it. Later siblings are over earlier ones; one
    /// that doesn't stop the pointer there lets it through to what's under.
    pub fn path_at(&self, p: Point, within: Option<usize>) -> Vec<usize> {
        self.path_through(p, within, false)
    }

    /// [`path_at`](Self::path_at) past what takes only drags
    /// ([`Widget::drags_only`]): the nodes under it, which clicks, other
    /// buttons, the wheel and drops reach.
    pub fn path_under(&self, p: Point, within: Option<usize>) -> Vec<usize> {
        self.path_through(p, within, true)
    }

    fn path_through(&self, p: Point, within: Option<usize>, under: bool) -> Vec<usize> {
        let mut path = Vec::new();
        let start = within.unwrap_or_else(|| self.root());
        if !self.nodes[start].rect.contains(p) {
            return path;
        }
        self.hit(start, p, &mut path, under);
        path
    }

    fn hit(&self, idx: usize, p: Point, path: &mut Vec<usize>, under: bool) -> bool {
        path.push(idx);
        let n = &self.nodes[idx];
        if !n.widget.hit_children(p - n.rect.origin().to_vec2()) {
            return n.widget.solid();
        }
        for &k in n.children.iter().rev() {
            let c = &self.nodes[k];
            if c.dead || !c.rect.contains(p) || !c.clip.contains(p) || (under && c.widget.drags_only()) {
                continue;
            }
            let len = path.len();
            if self.hit(k, p, path, under) {
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

    /// The topmost modal node (a sheet), if any: the last modal painted
    /// owns input. Found as the tree is laid out (`finish`), since every
    /// pointer event asks.
    pub fn modal(&self) -> Option<usize> {
        self.modal
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
                hovered: env.hot == Some(n.id) || env.hot_under == Some(n.id),
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

/// The hash of what `node` draws, as it is now: its size, the theme's
/// generation, and what its widget says its drawing depends on.
fn paint_hash(node: &Node, generation: u64) -> u64 {
    let mut fx = Fx::default();
    fx.write_u64(node.size.width.to_bits());
    fx.write_u64(node.size.height.to_bits());
    fx.write_u64(generation);
    node.widget.hash(&mut PaintHasher(&mut fx));
    fx.finish()
}

/// Whether `widget` is a video's place or is drawn over videos: what the
/// window's videos are placed by.
fn is_video(widget: &dyn Widget) -> bool {
    widget.video().is_some() || widget.over_video()
}

/// The damage of a node that was drawn as `was` and is gone.
fn went(was: &Drawn, moved: &[Moved], damage: &mut Vec<Rect>) {
    match moved.iter().find(|m| m.rect.intersect(was.clip).area() > 0.0) {
        Some(m) => {
            push_rect(damage, m.carried(was.clip));
            for r in outside(was.clip, m.rect) {
                push_rect(damage, r);
            }
        }
        None => push_rect(damage, was.clip),
    }
}

/// What painting a tree needs besides the canvas.
pub(crate) struct PaintEnv<'a> {
    pub theme: &'a crate::theme::Theme,
    pub text: &'a mut crate::text::TextCache,
    pub states: &'a StateMap,
    pub hot: Option<NodeId>,
    pub hot_under: Option<NodeId>,
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

/// The parts of `a` outside `r`: bands above and below, and beside, any
/// of which may be empty. (No list is made for them: this is asked twice
/// for every node of content that scrolls.)
fn outside(a: Rect, r: Rect) -> [Rect; 4] {
    let i = a.intersect(r);
    if i.area() <= 0.0 {
        return [a, Rect::ZERO, Rect::ZERO, Rect::ZERO];
    }
    [
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
