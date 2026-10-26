//! What assistive technology is told: each element's semantic node
//! (role, label, value, state, actions) in a tree that follows the
//! elements'. The kit builds it from the widgets on demand; wiring it to
//! AccessKit (and through it AT-SPI) is to come.

use crate::layout::Rect;
use crate::tree::{NodeId, Tree};
use crate::widget::Semantics;

/// A node of the semantic tree: an element that has something to say,
/// with the nodes under it.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNode {
    pub id: NodeId,
    pub semantics: Semantics,
    /// Where it is in its window.
    pub rect: Rect,
    pub children: Vec<SemanticNode>,
}

/// The semantics of `idx`: the element's own label over the widget's,
/// and its tooltip naming it when it has no label or shows no text (an
/// icon button is "Move up", not its glyph's name).
pub(crate) fn of(tree: &Tree, idx: usize) -> Option<Semantics> {
    let n = &tree.nodes[idx];
    let mut s = n.widget.semantics()?;
    if let Some(l) = &n.common.label {
        s.label = Some(l.to_string());
    } else if let Some(t) = &n.common.tooltip
        && (s.label.is_none() || n.widget.text().is_none())
    {
        s.label = Some(t.to_string());
    }
    Some(s)
}

/// The window's semantic tree: elements with nothing to say are left out
/// and their children taken up.
pub(crate) fn semantic_tree(tree: &Tree) -> Vec<SemanticNode> {
    fn walk(tree: &Tree, idx: usize, out: &mut Vec<SemanticNode>) {
        let n = &tree.nodes[idx];
        if n.dead {
            return;
        }
        let mut kids = Vec::new();
        for &k in &n.children {
            walk(tree, k, &mut kids);
        }
        match of(tree, idx) {
            Some(semantics) => out.push(SemanticNode { id: n.id, semantics, rect: n.rect, children: kids }),
            None => out.extend(kids),
        }
    }
    let mut out = Vec::new();
    if let Some(r) = tree.root {
        walk(tree, r, &mut out);
    }
    out
}

/// Every node with semantics, in tree order.
pub(crate) fn nodes(tree: &Tree) -> Vec<(Semantics, Rect)> {
    let mut out = Vec::new();
    fn flatten(nodes: &[SemanticNode], out: &mut Vec<(Semantics, Rect)>) {
        for n in nodes {
            out.push((n.semantics.clone(), n.rect));
            flatten(&n.children, out);
        }
    }
    flatten(&semantic_tree(tree), &mut out);
    out
}
