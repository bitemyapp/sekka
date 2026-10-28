//! Stable pane identity is independent of split positions and tab selection.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Right,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaneTree {
    Leaf(u64),
    Split { id: u64, axis: Axis, first: Box<PaneTree>, second: Box<PaneTree> },
}

impl PaneTree {
    pub fn leaves(&self) -> Vec<u64> {
        let mut leaves = Vec::new();
        self.visit(&mut |id| leaves.push(id));
        leaves
    }

    fn visit(&self, f: &mut impl FnMut(u64)) {
        match self {
            Self::Leaf(id) => f(*id),
            Self::Split { first, second, .. } => {
                first.visit(f);
                second.visit(f);
            }
        }
    }

    pub fn split(&mut self, target: u64, pane: u64, split: u64, axis: Axis) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    id: split,
                    axis,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(pane)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split(target, pane, split, axis) || second.split(target, pane, split, axis)
            }
        }
    }

    /// Removing a leaf collapses its parent, preserving the sibling subtree.
    pub fn remove(self, target: u64) -> Option<Self> {
        match self {
            Self::Leaf(id) => (id != target).then_some(Self::Leaf(id)),
            Self::Split { id, axis, first, second } => match (first.remove(target), second.remove(target)) {
                (Some(first), Some(second)) => {
                    Some(Self::Split { id, axis, first: Box::new(first), second: Box::new(second) })
                }
                (Some(tree), None) | (None, Some(tree)) => Some(tree),
                (None, None) => None,
            },
        }
    }

    pub fn adjacent(&self, current: u64, previous: bool) -> Option<u64> {
        let leaves = self.leaves();
        let index = leaves.iter().position(|id| *id == current)?;
        Some(leaves[if previous { (index + leaves.len() - 1) % leaves.len() } else { (index + 1) % leaves.len() }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_nested_pane_preserves_sibling_identity_and_order() {
        let mut tree = PaneTree::Leaf(1);
        assert!(tree.split(1, 2, 10, Axis::Right));
        assert!(tree.split(2, 3, 11, Axis::Down));
        assert_eq!(tree.leaves(), [1, 2, 3]);
        let tree = tree.remove(2).unwrap();
        assert_eq!(tree.leaves(), [1, 3]);
        assert_eq!(tree.adjacent(1, true), Some(3));
        assert_eq!(tree.adjacent(3, false), Some(1));
        assert_eq!(tree.remove(1), Some(PaneTree::Leaf(3)));
    }
}
