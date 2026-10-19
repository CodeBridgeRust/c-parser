//! Sorted `Span -> NodeId` index for incremental updates.
//!
//! Because nodes are appended to the arena in traversal order (not guaranteed
//! sorted by `span.start`), a plain binary search over `Vec<Node>` is
//! incorrect. This module maintains a separate sorted index (`Vec<(Span,
//! NodeId)>` for the MVP) that enables honest O(log N) container lookup.
//!
//! ## Algorithm
//!
//! 1. `build()` — scan the arena once, collect `(span, id)` pairs, sort by
//!    `span.start`.
//! 2. `find_covering(range)` — binary-search for the rightmost entry whose
//!    `span.start ≤ range.start`, then scan backwards to find the entry with
//!    the tightest `span` that still fully covers `range`.  This is the
//!    *minimal container node* used for invalidation.
//!
//! ## Performance
//!
//! - Build: O(N log N) — done once after a full parse or after a subtree
//!   replacement that adds/removes many nodes.
//! - Find: O(log N + K) where K is the number of candidate covering nodes
//!   (typically very small — just the nesting chain from TranslationUnit down
//!   to the actual container).
//!
//! For MVP the index is a sorted vector.  Per the architecture doc (§7.3),
//! this can later be replaced with an interval tree without changing the
//! public API.

use crate::ast::{Node, NodeId, Span};

// ---------------------------------------------------------------------------
// SpanIndex
// ---------------------------------------------------------------------------

/// A sorted `Span → NodeId` index used to find the minimal AST node that
/// fully covers a given byte range.
///
/// Entries are sorted by `span.start` ascending, then by `span.end` descending
/// (so that for equal `start`, longer spans — which are ancestors — come
/// first).
#[derive(Debug, Clone)]
pub struct SpanIndex {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    span: Span,
    id: NodeId,
}

impl SpanIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Build (or rebuild) the index from a slice of arena nodes.
    ///
    /// Complexity: O(N log N).
    pub fn build(nodes: &[Node]) -> Self {
        let mut entries: Vec<Entry> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| Entry {
                span: node.span,
                id: i as NodeId,
            })
            .collect();

        // Sort by span.start ascending; for equal starts, longer spans first
        // (descending end) so that ancestors appear before their children
        // when scanning backwards.
        entries.sort_by(|a, b| {
            a.span
                .start
                .cmp(&b.span.start)
                .then_with(|| b.span.end.cmp(&a.span.end))
        });

        Self { entries }
    }

    /// Rebuild the index in-place from arena nodes.
    pub fn rebuild(&mut self, nodes: &[Node]) {
        *self = Self::build(nodes);
    }

    /// Return the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Find the minimal (tightest) node whose [`Span`] fully covers `range`.
    ///
    /// Returns `None` if no node covers `range` (should not happen for valid
    /// ranges — the `TranslationUnit` root always covers the whole file).
    ///
    /// Complexity: O(log N + K) where K is the number of candidate nodes
    /// whose `span.start ≤ range.start`.
    pub fn find_covering(&self, range: Span) -> Option<NodeId> {
        if self.entries.is_empty() {
            return None;
        }

        // Binary search for the rightmost entry with span.start <= range.start.
        // We use partition_point on the predicate `e.span.start <= range.start`
        // which returns the first index where the predicate is false.  So the
        // rightmost true index is partition_point - 1.
        let pivot = self
            .entries
            .partition_point(|e| e.span.start <= range.start);

        // All entries with span.start <= range.start are in entries[..pivot].
        // Scan backwards from pivot-1 down to 0 looking for covering spans.
        let mut best: Option<(usize, Entry)> = None;

        for i in (0..pivot).rev() {
            let entry = &self.entries[i];
            if entry.span.end >= range.end {
                // This node covers the range.  Keep the tightest (smallest
                // span length).
                match best {
                    None => {
                        best = Some((i, *entry));
                    }
                    Some((_, ref best_e)) => {
                        let best_len = best_e.span.end - best_e.span.start;
                        let cur_len = entry.span.end - entry.span.start;
                        if cur_len < best_len {
                            best = Some((i, *entry));
                        }
                    }
                }
            }
        }

        best.map(|(_, e)| e.id)
    }

    /// Return all entries for iteration / inspection (useful for testing).
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Consume the index and return the raw entries.
    pub fn into_entries(self) -> Vec<Entry> {
        self.entries
    }
}

impl Default for SpanIndex {
    fn default() -> Self {
        Self::new()
    }
}

// Allow external code to read Entry fields.
impl Entry {
    /// The span of the indexed node.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The NodeId of the indexed node.
    pub fn id(&self) -> NodeId {
        self.id
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Node, NodeKind};

    /// Helper: build a minimal arena with nodes that have specific spans.
    fn make_nodes(spans: &[(u32, u32)]) -> Vec<Node> {
        spans
            .iter()
            .map(|&(s, e)| {
                Node::structural(NodeKind::TranslationUnit, Span { start: s, end: e }, vec![])
            })
            .collect()
    }

    #[test]
    fn build_is_sorted_by_start() {
        let nodes = make_nodes(&[(30, 40), (0, 100), (50, 60)]);
        let idx = SpanIndex::build(&nodes);
        let starts: Vec<u32> = idx.entries().iter().map(|e| e.span.start).collect();
        assert_eq!(starts, vec![0, 30, 50]);
    }

    #[test]
    fn build_same_start_longer_first() {
        // TranslationUnit (0, 100) and a function (0, 50)
        let nodes = make_nodes(&[(0, 50), (0, 100)]);
        let idx = SpanIndex::build(&nodes);
        // Both start at 0; longer span (100) comes before shorter (50).
        assert_eq!(idx.entries[0].span.end, 100);
        assert_eq!(idx.entries[1].span.end, 50);
    }

    #[test]
    fn find_covering_exact_match() {
        let nodes = make_nodes(&[(0, 100), (10, 50), (60, 90)]);
        let idx = SpanIndex::build(&nodes);
        // Range exactly matching (10, 50) -> id 1
        assert_eq!(
            idx.find_covering(Span { start: 10, end: 50 }),
            Some(1)
        );
    }

    #[test]
    fn find_covering_nested() {
        // 0: TranslationUnit (0..200)
        // 1: FunctionDef   (5..150)
        // 2: CompoundStmt  (20..140)
        // 3: IfStatement   (25..80)
        let nodes = make_nodes(&[(0, 200), (5, 150), (20, 140), (25, 80)]);
        let idx = SpanIndex::build(&nodes);
        // Range (30, 40) — all four cover it.  Tightest is IfStatement (25..80).
        assert_eq!(
            idx.find_covering(Span { start: 30, end: 40 }),
            Some(3)
        );
    }

    #[test]
    fn find_covering_no_match() {
        let nodes = make_nodes(&[(0, 50), (60, 100)]);
        let idx = SpanIndex::build(&nodes);
        // Range spans from 55 to 65 — crosses the gap
        assert_eq!(
            idx.find_covering(Span { start: 55, end: 65 }),
            None
        );
    }

    #[test]
    fn find_covering_empty_index() {
        let idx = SpanIndex::new();
        assert_eq!(idx.find_covering(Span { start: 0, end: 10 }), None);
    }

    #[test]
    fn rebuild_replaces_entries() {
        let nodes1 = make_nodes(&[(0, 10)]);
        let mut idx = SpanIndex::build(&nodes1);
        assert_eq!(idx.len(), 1);

        let nodes2 = make_nodes(&[(0, 10), (5, 8)]);
        idx.rebuild(&nodes2);
        assert_eq!(idx.len(), 2);
    }

    #[test]
    fn entry_accessors() {
        let entry = Entry {
            span: Span { start: 1, end: 2 },
            id: 42,
        };
        assert_eq!(entry.span(), Span { start: 1, end: 2 });
        assert_eq!(entry.id(), 42);
    }
}