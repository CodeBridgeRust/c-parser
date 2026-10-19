//! Arena-based AST storage.
//!
//! This is one of the key engineering innovations of the project: all AST
//! nodes are stored in a single flat `Vec<Node>`, linked through 32-bit
//! `NodeId` indices.  This provides O(1) insertion, spatial locality, stable
//! indices and simple traversal/serialization, and it avoids the
//! pointer/lifetime plumbing of `Box`/`Rc`-based trees.
//!
//! ## Overflow guard
//!
//! `NodeId = u32` supports up to ~4.29 billion nodes — far beyond any
//! realistic C code base.  If the arena approaches this limit (i.e.
//! `u32::MAX` nodes), [`Arena::push`] returns a fatal error **before**
//! overflow, preventing panics.

use crate::ast::{Node, NodeId};

/// A growable arena of AST nodes.
///
/// Wraps `Vec<Node>` with a bounds-checked [`push`](Arena::push) that guards
/// against `NodeId` overflow.
#[derive(Debug, Clone)]
pub struct Arena {
    nodes: Vec<Node>,
}

impl Arena {
    /// Create a new, empty arena with the given capacity hint.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(cap),
        }
    }

    /// Create an empty arena.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
        }
    }

    /// Number of nodes currently in the arena.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Return `true` if the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Push a node, returning its `NodeId`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the arena already contains `u32::MAX` nodes.  This
    /// is a fatal condition — the caller should propagate the error.
    pub fn push(&mut self, node: Node) -> Result<NodeId, ArenaError> {
        let id = self
            .nodes
            .len()
            .try_into()
            .map_err(|_| ArenaError::Overflow)?;
        self.nodes.push(node);
        Ok(id)
    }

    /// Return a reference to the node at `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds.
    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    /// Return a mutable reference to the node at `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds.
    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    /// Return a slice of all nodes in the arena.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Consume the arena and return the underlying `Vec<Node>`.
    pub fn into_nodes(self) -> Vec<Node> {
        self.nodes
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ArenaError
// ---------------------------------------------------------------------------

/// Fatal arena error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    /// The arena has reached `NodeId::MAX` nodes — cannot allocate more.
    Overflow,
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::Overflow => write!(
                f,
                "arena overflow: exceeded maximum of {} nodes",
                u32::MAX
            ),
        }
    }
}

impl std::error::Error for ArenaError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{NodeKind, Span};

    fn dummy_node() -> Node {
        Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 1 }, vec![])
    }

    #[test]
    fn push_returns_sequential_ids() {
        let mut arena = Arena::new();
        let id0 = arena.push(dummy_node()).unwrap();
        let id1 = arena.push(dummy_node()).unwrap();
        let id2 = arena.push(dummy_node()).unwrap();
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(arena.len(), 3);
    }

    #[test]
    fn get_retrieves_pushed_node() {
        let mut arena = Arena::new();
        let node = Node::structural(
            NodeKind::IfStatement,
            Span { start: 10, end: 20 },
            vec![1, 2],
        );
        let id = arena.push(node.clone()).unwrap();
        assert_eq!(arena.get(id), &node);
    }

    #[test]
    fn get_mut_allows_mutation() {
        let mut arena = Arena::new();
        let id = arena.push(dummy_node()).unwrap();
        arena.get_mut(id).span.end = 42;
        assert_eq!(arena.get(id).span.end, 42);
    }

    #[test]
    fn into_nodes_returns_vec() {
        let mut arena = Arena::new();
        arena.push(dummy_node()).unwrap();
        let nodes = arena.into_nodes();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn with_capacity_preallocates() {
        let arena = Arena::with_capacity(100);
        assert_eq!(arena.len(), 0);
        assert!(arena.nodes.capacity() >= 100);
    }

    #[test]
    fn default_creates_empty_arena() {
        let arena = Arena::default();
        assert!(arena.is_empty());
        assert_eq!(arena.len(), 0);
    }

    #[test]
    fn nodes_returns_all() {
        let mut arena = Arena::new();
        arena.push(dummy_node()).unwrap();
        arena.push(dummy_node()).unwrap();
        let nodes = arena.nodes();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    #[should_panic]
    fn get_out_of_bounds_panics() {
        let arena = Arena::new();
        arena.get(0); // empty arena — must panic
    }

    #[test]
    fn arena_error_is_std_error() {
        let e = ArenaError::Overflow;
        let _: &dyn std::error::Error = &e;
    }

    #[test]
    fn arena_error_partial_eq() {
        assert_eq!(ArenaError::Overflow, ArenaError::Overflow);
    }

    #[test]
    fn arena_error_clone() {
        let e = ArenaError::Overflow;
        assert_eq!(e.clone(), ArenaError::Overflow);
    }
}