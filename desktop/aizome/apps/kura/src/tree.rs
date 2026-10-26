//! The scanned tree, kept compact: one flat arena of fixed-size nodes, the
//! names in one byte buffer, and each folder's children side by side in
//! the arena (sorted largest first once the scan ends), so a folder is a
//! range rather than a list of its own.
//!
//! Every child comes after its parent in the arena, so sizes add up in
//! one backward pass and nothing points forward that isn't a child range.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// A node's index in the arena. The root is `NodeId(0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    File,
    Dir,
    Symlink,
    /// Sockets, pipes, devices.
    Other,
}

/// Something about a node besides its kind.
pub mod flags {
    /// Reading it (or its folder) failed: it may hold more than it shows.
    pub const ERROR: u8 = 1;
    /// A folder on another filesystem, not entered.
    pub const MOUNT: u8 = 2;
    /// A hard link to a file already counted: its size counts as zero.
    pub const HARDLINK: u8 = 4;
    /// A folder the scan was told to skip.
    pub const SKIPPED: u8 = 8;
    /// Gone since the scan (moved to the Trash): its size no longer counts.
    pub const REMOVED: u8 = 16;
}

/// One file or folder. 48 bytes.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    name_at: u32,
    name_len: u16,
    pub kind: Kind,
    pub flags: u8,
    pub parent: u32,
    /// The children: `first..first + count`.
    pub first: u32,
    pub count: u32,
    /// Allocated bytes (`st_blocks` × 512), the subtree's for a folder.
    pub alloc: u64,
    /// Apparent bytes (`st_size`), the subtree's for a folder.
    pub apparent: u64,
    /// Files and folders inside (0 for a file).
    pub items: u32,
    /// Modified, as Unix time: for a folder, the newest anything inside it.
    pub mtime: i64,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// The arena. Build it with [`Tree::new`] and [`Tree::push_children`] (the
/// scanner does), then [`Tree::finish`].
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    names: Vec<u8>,
    /// Where the root is, as scanned.
    root_path: PathBuf,
}

/// What one entry of a folder brings to [`Tree::push_children`].
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: Vec<u8>,
    pub kind: Kind,
    pub flags: u8,
    pub alloc: u64,
    pub apparent: u64,
    pub mtime: i64,
}

impl Tree {
    /// A tree of just its root (a folder, or a file if the scan was of one).
    pub fn new(root_path: PathBuf, kind: Kind, alloc: u64, apparent: u64, mtime: i64) -> Tree {
        let name = root_path.as_os_str().as_bytes().to_vec();
        let mut t = Tree { nodes: Vec::with_capacity(1 << 14), names: Vec::with_capacity(1 << 18), root_path };
        t.names.extend_from_slice(&name);
        t.nodes.push(Node {
            name_at: 0,
            name_len: name.len().min(u16::MAX as usize) as u16,
            kind,
            flags: 0,
            parent: 0,
            first: 0,
            count: 0,
            alloc,
            apparent,
            items: 0,
            mtime,
        });
        t
    }

    /// Give `parent` its children, all at once: the id of the first (the
    /// rest follow it).
    pub fn push_children(&mut self, parent: NodeId, entries: &[Entry]) -> u32 {
        let first = self.nodes.len() as u32;
        for e in entries {
            let at = self.names.len() as u32;
            let len = e.name.len().min(u16::MAX as usize);
            self.names.extend_from_slice(&e.name[..len]);
            self.nodes.push(Node {
                name_at: at,
                name_len: len as u16,
                kind: e.kind,
                flags: e.flags,
                parent: parent.0,
                first: 0,
                count: 0,
                alloc: e.alloc,
                apparent: e.apparent,
                items: 0,
                mtime: e.mtime,
            });
        }
        let p = &mut self.nodes[parent.index()];
        p.first = first;
        p.count = entries.len() as u32;
        first
    }

    /// Mark a node (a folder that couldn't be read, say).
    pub fn add_flag(&mut self, id: NodeId, flag: u8) {
        self.nodes[id.index()].flags |= flag;
    }

    /// Add up sizes, counts and newest times from the leaves to the root,
    /// then sort every folder's children largest first.
    pub fn finish(&mut self) {
        for n in self.nodes.iter_mut() {
            if n.is_dir() {
                n.items = 0;
            }
        }
        for i in (1..self.nodes.len()).rev() {
            let c = self.nodes[i];
            let p = &mut self.nodes[c.parent as usize];
            p.alloc += c.alloc;
            p.apparent += c.apparent;
            p.items += 1 + c.items;
            p.mtime = p.mtime.max(c.mtime);
        }
        self.sort_children();
        // Growth leaves up to half of each buffer spare.
        self.nodes.shrink_to_fit();
        self.names.shrink_to_fit();
    }

    /// Each folder's children largest first (then by name), keeping every
    /// grandchild's parent pointing at where its parent moved to.
    fn sort_children(&mut self) {
        let mut order: Vec<u32> = Vec::new();
        let mut moved: Vec<Node> = Vec::new();
        for i in 0..self.nodes.len() {
            let Node { first, count, .. } = self.nodes[i];
            if count < 2 {
                continue;
            }
            let range = first..first + count;
            order.clear();
            order.extend(range.clone());
            let nodes = &self.nodes;
            let names = &self.names;
            let name = |n: &Node| &names[n.name_at as usize..n.name_at as usize + n.name_len as usize];
            order.sort_by(|&a, &b| {
                let (x, y) = (&nodes[a as usize], &nodes[b as usize]);
                y.alloc.cmp(&x.alloc).then(y.apparent.cmp(&x.apparent)).then_with(|| name(x).cmp(name(y)))
            });
            if order.iter().copied().eq(range.clone()) {
                continue;
            }
            moved.clear();
            moved.extend(order.iter().map(|&j| self.nodes[j as usize]));
            for (k, n) in moved.iter().enumerate() {
                let at = first + k as u32;
                self.nodes[at as usize] = *n;
                for c in n.first..n.first + n.count {
                    self.nodes[c as usize].parent = at;
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        (id != NodeId::ROOT).then(|| NodeId(self.nodes[id.index()].parent))
    }

    /// Its children, largest first.
    pub fn children(&self, id: NodeId) -> impl DoubleEndedIterator<Item = NodeId> + ExactSizeIterator + Clone {
        let n = &self.nodes[id.index()];
        (n.first..n.first + n.count).map(NodeId)
    }

    /// Its name's bytes (the whole path for the root).
    pub fn name_bytes(&self, id: NodeId) -> &[u8] {
        let n = &self.nodes[id.index()];
        &self.names[n.name_at as usize..n.name_at as usize + n.name_len as usize]
    }

    pub fn name_os(&self, id: NodeId) -> &OsStr {
        OsStr::from_bytes(self.name_bytes(id))
    }

    /// Its name for people (invalid UTF-8 replaced).
    pub fn name(&self, id: NodeId) -> Cow<'_, str> {
        String::from_utf8_lossy(self.name_bytes(id))
    }

    /// Its ancestors from the root down to it, both included.
    pub fn lineage(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut at = id;
        while let Some(p) = self.parent(at) {
            out.push(p);
            at = p;
        }
        out.reverse();
        out
    }

    /// How far below the root it is (the root is 0).
    pub fn depth(&self, id: NodeId) -> usize {
        let mut d = 0;
        let mut at = id;
        while let Some(p) = self.parent(at) {
            d += 1;
            at = p;
        }
        d
    }

    /// Its full path.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut p = self.root_path.clone();
        for n in self.lineage(id).into_iter().skip(1) {
            p.push(self.name_os(n));
        }
        p
    }

    /// The node at `path`, if the scan saw it.
    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let rest = path.strip_prefix(&self.root_path).ok()?;
        let mut at = NodeId::ROOT;
        for part in rest.components() {
            let want = part.as_os_str().as_bytes();
            at = self.children(at).find(|&c| self.name_bytes(c) == want)?;
        }
        Some(at)
    }

    /// The child of `dir` named `name`.
    pub fn child_named(&self, dir: NodeId, name: &str) -> Option<NodeId> {
        self.children(dir).find(|&c| self.name_bytes(c) == name.as_bytes())
    }

    /// Whether `id` is `ancestor` or inside it.
    pub fn is_within(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut at = id;
        loop {
            if at == ancestor {
                return true;
            }
            match self.parent(at) {
                Some(p) => at = p,
                None => return false,
            }
        }
    }

    /// Take `id` out of the counts (it went to the Trash): its folders
    /// above lose its size and items, and it's marked [`flags::REMOVED`].
    /// Nodes keep their places (and ids), so folders whose sizes changed
    /// may no longer be in order among their siblings: sort when showing.
    pub fn remove(&mut self, id: NodeId) {
        if id == NodeId::ROOT || self.node(id).has(flags::REMOVED) {
            return;
        }
        let n = self.nodes[id.index()];
        let mut at = id;
        while let Some(p) = self.parent(at) {
            let pn = &mut self.nodes[p.index()];
            pn.alloc = pn.alloc.saturating_sub(n.alloc);
            pn.apparent = pn.apparent.saturating_sub(n.apparent);
            pn.items = pn.items.saturating_sub(1 + n.items);
            at = p;
        }
        let m = &mut self.nodes[id.index()];
        m.flags |= flags::REMOVED;
        m.alloc = 0;
        m.apparent = 0;
    }

    /// Put a fresh scan of folder `at` in place of what the tree held there
    /// (after something inside it changed): `sub`'s root is `at` scanned
    /// again. The new nodes go at the end of the arena (children still
    /// after their parents); the old ones are marked removed and left
    /// unreachable; the folders above take the difference in size.
    pub fn graft(&mut self, at: NodeId, sub: &Tree) {
        let old = self.nodes[at.index()];
        // The old children go (their own descendants with them).
        for c in old.first..old.first + old.count {
            self.nodes[c as usize].flags |= flags::REMOVED;
        }
        let new = sub.nodes[0];
        let offset = self.nodes.len() as u32 - 1;
        let name_offset = self.names.len() as u32;
        // The root's name (its whole path) comes first in the names; the rest follow.
        let name_base = u32::from(new.name_len);
        self.names.extend_from_slice(&sub.names[name_base as usize..]);
        for n in &sub.nodes[1..] {
            let mut m = *n;
            m.name_at = n.name_at - name_base + name_offset;
            m.parent = if n.parent == 0 { at.0 } else { n.parent + offset };
            if m.count > 0 {
                m.first = n.first + offset;
            }
            self.nodes.push(m);
        }
        // The difference, up through the folders above.
        let mut up = at;
        while let Some(p) = self.parent(up) {
            let pn = &mut self.nodes[p.index()];
            pn.alloc = (pn.alloc + new.alloc).saturating_sub(old.alloc);
            pn.apparent = (pn.apparent + new.apparent).saturating_sub(old.apparent);
            pn.items = (pn.items + new.items).saturating_sub(old.items);
            pn.mtime = pn.mtime.max(new.mtime);
            up = p;
        }
        let a = &mut self.nodes[at.index()];
        a.first = if new.count > 0 { new.first + offset } else { 0 };
        a.count = new.count;
        a.alloc = new.alloc;
        a.apparent = new.apparent;
        a.items = new.items;
        a.mtime = new.mtime;
        a.flags = new.flags;
    }

    /// Which nodes are gone: removed, or inside something removed.
    pub fn gone(&self) -> Vec<bool> {
        let mut v = vec![false; self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate().skip(1) {
            v[i] = v[n.parent as usize] || n.has(flags::REMOVED);
        }
        v
    }

    /// `dir`'s children still there, largest first (whatever removals did
    /// to the order).
    pub fn sorted_children(&self, dir: NodeId) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = self.children(dir).filter(|&c| !self.node(c).has(flags::REMOVED)).collect();
        // Already sorted unless something was removed: a stable sort of a
        // sorted list costs one pass.
        v.sort_by_key(|n| std::cmp::Reverse(self.node(*n).alloc));
        v
    }

    /// Bytes the arena takes (for the performance notes).
    pub fn heap_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>() + self.names.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, kind: Kind, alloc: u64) -> Entry {
        Entry { name: name.as_bytes().to_vec(), kind, flags: 0, alloc, apparent: alloc, mtime: alloc as i64 }
    }

    #[test]
    fn sums_and_sorts_keeping_parents() {
        let mut t = Tree::new(PathBuf::from("/r"), Kind::Dir, 4, 4, 0);
        // root: a (dir), b (file 100), c (dir)
        let first =
            t.push_children(NodeId::ROOT, &[e("a", Kind::Dir, 4), e("b", Kind::File, 100), e("c", Kind::Dir, 4)]);
        let (a, c) = (NodeId(first), NodeId(first + 2));
        t.push_children(a, &[e("a1", Kind::File, 10), e("a2", Kind::File, 1000)]);
        let cf = t.push_children(c, &[e("c1", Kind::Dir, 4)]);
        t.push_children(NodeId(cf), &[e("deep", Kind::File, 50)]);
        t.finish();

        assert_eq!(t.node(NodeId::ROOT).alloc, 4 + 4 + 100 + 4 + 10 + 1000 + 4 + 50);
        assert_eq!(t.node(NodeId::ROOT).items, 7);
        let names: Vec<_> = t.children(NodeId::ROOT).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        let a = t.child_named(NodeId::ROOT, "a").unwrap();
        let kids: Vec<_> = t.children(a).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, ["a2", "a1"]);
        for k in t.children(a) {
            assert_eq!(t.parent(k), Some(a));
        }
        let deep = t.find(Path::new("/r/c/c1/deep")).unwrap();
        assert_eq!(t.path(deep), PathBuf::from("/r/c/c1/deep"));
        assert_eq!(t.depth(deep), 3);
        assert_eq!(t.node(t.child_named(NodeId::ROOT, "c").unwrap()).mtime, 50);
    }

    #[test]
    fn grafting_a_rescan_replaces_a_folder() {
        let mut t = Tree::new(PathBuf::from("/r"), Kind::Dir, 0, 0, 0);
        let f = t.push_children(NodeId::ROOT, &[e("keep", Kind::File, 10), e("dir", Kind::Dir, 0)]);
        let dir = NodeId(f + 1);
        t.push_children(dir, &[e("old-a", Kind::File, 100), e("old-b", Kind::File, 50)]);
        t.finish();
        let dir = t.child_named(NodeId::ROOT, "dir").unwrap();
        assert_eq!(t.node(NodeId::ROOT).alloc, 160);

        // The folder scanned again: one file went, two came, one deeper.
        let mut sub = Tree::new(PathBuf::from("/r/dir"), Kind::Dir, 0, 0, 0);
        let s = sub
            .push_children(NodeId::ROOT, &[e("old-a", Kind::File, 100), e("new", Kind::Dir, 0), e("c", Kind::File, 1)]);
        sub.push_children(NodeId(s + 1), &[e("deep", Kind::File, 7)]);
        sub.finish();
        t.graft(dir, &sub);

        assert_eq!(t.node(NodeId::ROOT).alloc, 10 + 100 + 7 + 1);
        assert_eq!(t.node(NodeId::ROOT).items, 2 + 4);
        assert_eq!(t.node(dir).alloc, 108);
        let names: Vec<_> = t.sorted_children(dir).iter().map(|&c| t.name(c).into_owned()).collect();
        assert_eq!(names, ["old-a", "new", "c"]);
        let deep = t.find(Path::new("/r/dir/new/deep")).unwrap();
        assert_eq!(t.path(deep), PathBuf::from("/r/dir/new/deep"));
        assert!(t.find(Path::new("/r/dir/old-b")).is_none());
        // What was there is gone from every count.
        let gone = t.gone();
        let live: u64 =
            t.nodes().iter().enumerate().filter(|(i, n)| !gone[*i] && n.kind == Kind::File).map(|(_, n)| n.alloc).sum();
        assert_eq!(live, 118);
        for i in 1..t.len() {
            assert!(t.nodes()[i].parent < i as u32, "children come after their parents");
        }
    }

    #[test]
    fn nodes_stay_small() {
        assert!(std::mem::size_of::<Node>() <= 48);
    }
}
