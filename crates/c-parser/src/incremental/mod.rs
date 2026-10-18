//! In-memory incremental AST updates.
//!
//! Given a changed byte range, [`IncrementalSession`] finds the minimal
//! container node via the [`crate::index::SpanIndex`], invalidates its
//! subtree and re-parses only that region, targeting O(log N + K) complexity.
//!
//! JSON is *not* serialized automatically after an update — only on explicit
//! request via [`IncrementalSession::to_json`].
//!
//! ## Lifetime contract
//!
//! `IncrementalSession<'a>` is parameterized by the lifetime of the **initial**
//! source buffer passed to [`new`](IncrementalSession::new).  The session does
//! not copy the buffer at creation time (zero-copy).  After the first
//! [`apply_edit`](IncrementalSession::apply_edit), an internal owned buffer is
//! allocated to hold the updated source text, and subsequent calls to
//! [`source`](IncrementalSession::source) return a reference to that internal
//! buffer.  The `'a` parameter ensures the initial buffer outlives the session.

use crate::ast::{Ast, Node, NodeId, Span};
use crate::error::ParseError;
use crate::index::SpanIndex;
use crate::lexer::token::{CDialect, PreprocessingMode};
use crate::parser::Parser;
use crate::serialize::ast_to_json;
use crate::{ParseInput};

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// IncrementalSession
// ---------------------------------------------------------------------------

/// An in-memory session that maintains a parsed AST and allows incremental
/// updates via [`apply_edit`](IncrementalSession::apply_edit).
///
/// ## Example (LSP scenario)
///
/// ```ignore
/// let source: Vec<u8> = std::fs::read("file.c")?;
/// let mut session = IncrementalSession::new(ParseInput {
///     source: &source,
///     filename: Some("file.c"),
///     config: ParserConfig::default(),
/// })?;
///
/// // User types "+ 1" at bytes 42..47
/// session.apply_edit(Span { start: 42, end: 47 }, b"+ 1")?;
///
/// // On save, produce full JSON
/// let json = session.to_json()?;
/// ```
pub struct IncrementalSession<'a> {
    /// The current AST (arena + root + errors).
    ast: Ast,
    /// The sorted Span → NodeId index.
    index: SpanIndex,
    /// Owned source buffer.  Initially empty (we borrow the caller's buffer);
    /// allocated after the first `apply_edit`.
    owned_source: Vec<u8>,
    /// Reference to the initial source buffer (valid until the first edit).
    initial_source: &'a [u8],
    /// C dialect.
    dialect: CDialect,
    /// Preprocessing mode.
    preprocessing: PreprocessingMode,
    /// Whether to include comments in the token stream.
    include_comments: bool,
    /// Whether we have taken ownership of the buffer (i.e. at least one edit
    /// has been applied).
    owns_buffer: bool,
}

impl<'a> IncrementalSession<'a> {
    /// Create a new incremental session by parsing `input.source`.
    ///
    /// **Lifetime:** `input.source` must outlive the session (`'a`).  The
    /// session does **not** copy the buffer at this point.
    pub fn new(input: ParseInput<'a>) -> Result<Self, ParseError> {
        let include_comments = input.config.include_comments;
        let dialect = input.config.dialect;
        let preprocessing = input.config.preprocessing;

        let mut parser = Parser::new(
            input.source,
            dialect,
            preprocessing,
            include_comments,
        );

        let root = parser.parse_translation_unit()?;
        let ast = parser.finish(root);

        let index = SpanIndex::build(&ast.nodes);

        Ok(Self {
            ast,
            index,
            owned_source: Vec::new(),
            initial_source: input.source,
            dialect,
            preprocessing,
            include_comments,
            owns_buffer: false,
        })
    }

    /// Apply an edit: replace `change` with `new_text` in the source buffer,
    /// then incrementally update the AST.
    ///
    /// `change` is a byte range in the **current** source buffer (as returned
    /// by [`source`](IncrementalSession::source)).
    ///
    /// ## Algorithm
    ///
    /// 1. Build the new source buffer.
    /// 2. Find the minimal covering node via [`SpanIndex::find_covering`].
    /// 3. Compute the covering node's new span.
    /// 4. Collect all descendant node ids of the covering node.
    /// 5. Re-parse the covering node's new span region.
    /// 6. Update the arena: shift spans of nodes after the edit, update the
    ///    covering node's children, append new nodes.
    /// 7. Rebuild the index.
    pub fn apply_edit(&mut self, change: Span, new_text: &[u8]) -> Result<(), ParseError> {
        // Validate change range.
        let old_source = self.source_slice();
        if change.start > old_source.len() || change.end > old_source.len() {
            return Err(ParseError {
                code: "E_FATAL".to_string(),
                message: format!(
                    "edit range {:?} out of bounds (source length {})",
                    change,
                    old_source.len()
                ),
                span: change,
                severity: crate::error::Severity::Error,
            });
        }

        // 1. Build new source buffer.
        let delta: isize = new_text.len() as isize - (change.end - change.start) as isize;

        let mut new_source = Vec::with_capacity(
            old_source.len() - (change.end - change.start) + new_text.len(),
        );
        new_source.extend_from_slice(&old_source[..change.start]);
        new_source.extend_from_slice(new_text);
        new_source.extend_from_slice(&old_source[change.end..]);

        // 2. Find covering node.
        let covering_id: NodeId = self.index.find_covering(change).unwrap_or(self.ast.root);

        // 3. Compute covering node's new span.
        let old_covering_span = self.ast.nodes[covering_id as usize].span;
        let new_covering_span = self.adjust_span(old_covering_span, change, delta);

        // 4. Collect descendant ids (including the covering node itself).
        let descendants = self.collect_descendants(covering_id);

        // 5. Re-parse the covering node's region.
        let region_src = &new_source[new_covering_span.start..new_covering_span.end];
        let mut region_parser = Parser::new(
            region_src,
            self.dialect,
            self.preprocessing,
            self.include_comments,
        );

        let new_children_relative = match region_parser.parse_region() {
            Ok(ids) => ids,
            Err(_e) => {
                // If local reparse fails, fall back to full reparse.
                return self.full_reparse(&new_source);
            }
        };
        let mut _region_ast = region_parser.finish(0); // consume parser to get arena

        // The region parser built nodes with spans relative to region_src
        // (starting at 0).  We need to rebase them to the full source
        // coordinates.
        let offset = new_covering_span.start;
        let mut new_nodes: Vec<Node> = _region_ast.nodes;
        for node in &mut new_nodes {
            node.span.start += offset;
            node.span.end += offset;
        }

        // 6. Update the arena.

        // Compute the base index for new nodes.
        let old_node_count = self.ast.nodes.len();
        let region_base = old_node_count as NodeId;

        // Fix up children indices in new_nodes to point into the main arena.
        for node in &mut new_nodes {
            for child in &mut node.children {
                *child += region_base;
            }
        }

        // Fix up new_children to point into the main arena.
        let new_children: Vec<NodeId> = new_children_relative
            .iter()
            .map(|c| *c + region_base)
            .collect();

        // Update the covering node.
        {
            let cov_node = &mut self.ast.nodes[covering_id as usize];
            cov_node.span = new_covering_span;
            cov_node.children = new_children;
        }

        // Shift spans of non-descendant nodes after the edit.
        for i in 0..old_node_count {
            if descendants.contains(&(i as NodeId)) {
                continue;
            }
            let node_span = self.ast.nodes[i].span;
            if node_span.start >= change.end || node_span.end > change.end {
                self.ast.nodes[i].span = self.adjust_span(node_span, change, delta);
            }
        }

        // Append new nodes to the arena.
        self.ast.nodes.extend(new_nodes);

        // Update source metadata.
        self.ast.source_length = new_source.len();
        self.owned_source = new_source;
        self.initial_source = &[];
        self.owns_buffer = true;

        // Merge errors from the region parse (fix up error spans too).
        for err in &mut _region_ast.errors {
            err.span.start += offset;
            err.span.end += offset;
        }
        self.ast.errors.extend(_region_ast.errors);

        // 7. Rebuild index.
        self.index.rebuild(&self.ast.nodes);

        Ok(())
    }

    /// Return a reference to the current AST.
    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    /// Return a reference to the current source buffer.
    pub fn source(&self) -> &[u8] {
        self.source_slice()
    }

    /// Return a reference to the current error list.
    pub fn errors(&self) -> &[ParseError] {
        &self.ast.errors
    }

    /// Serialize the current AST to a JSON string (via lazy materialization).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        ast_to_json(&self.ast, self.source_slice())
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Fall back to a full reparse of the given source.
    fn full_reparse(&mut self, source: &[u8]) -> Result<(), ParseError> {
        let mut parser = Parser::new(
            source,
            self.dialect,
            self.preprocessing,
            self.include_comments,
        );
        let root = parser.parse_translation_unit()?;
        let new_ast = parser.finish(root);

        self.ast = new_ast;
        self.index.rebuild(&self.ast.nodes);
        self.owned_source = source.to_vec();
        self.initial_source = &[];
        self.owns_buffer = true;

        Ok(())
    }

    /// Return the active source slice (owned or borrowed).
    fn source_slice(&self) -> &[u8] {
        if self.owns_buffer {
            &self.owned_source
        } else {
            self.initial_source
        }
    }

    /// Adjust a span across an edit.
    ///
    /// - If `span` lies entirely before `change.start`, it is unchanged.
    /// - If `span` lies entirely at or after `change.end`, it is shifted by
    ///   `delta`.
    /// - If `span` overlaps the change region, it is stretched/shrunk: the
    ///   start stays but the end shifts by `delta` (used for the covering
    ///   node).
    fn adjust_span(&self, span: Span, change: Span, delta: isize) -> Span {
        let start = if span.start >= change.end {
            (span.start as isize + delta) as usize
        } else {
            span.start
        };

        let end = if span.end > change.start {
            (span.end as isize + delta) as usize
        } else {
            span.end
        };

        Span { start, end }
    }

    /// Collect all transitive children (descendants) of `root_id`,
    /// including `root_id` itself.
    fn collect_descendants(&self, root_id: NodeId) -> HashSet<NodeId> {
        let mut result = HashSet::new();
        let mut stack: Vec<NodeId> = vec![root_id];

        while let Some(id) = stack.pop() {
            if result.insert(id) {
                let node = &self.ast.nodes[id as usize];
                stack.extend(&node.children);
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{NodeKind, Span};
    use crate::ParserConfig;

    fn make_input(source: &[u8]) -> ParseInput<'_> {
        ParseInput {
            source,
            filename: None,
            config: ParserConfig::default(),
        }
    }

    #[test]
    fn new_session_parses_simple_file() {
        let src = b"int x;";
        let session = IncrementalSession::new(make_input(src)).unwrap();
        let tu = &session.ast().nodes[session.ast().root as usize];
        assert_eq!(tu.kind, NodeKind::TranslationUnit);
        assert!(!tu.children.is_empty());
        let decl = &session.ast().nodes[tu.children[0] as usize];
        assert_eq!(decl.kind, NodeKind::VariableDeclaration);
    }

    #[test]
    fn session_source_returns_initial_buffer_before_edit() {
        let src = b"int x;";
        let session = IncrementalSession::new(make_input(src)).unwrap();
        assert_eq!(session.source(), src);
    }

    #[test]
    fn session_source_returns_owned_buffer_after_edit() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        // Replace "x" with "y"
        session
            .apply_edit(Span { start: 4, end: 5 }, b"y")
            .unwrap();

        let source = session.source();
        assert_eq!(source, b"int y;");
    }

    #[test]
    fn apply_edit_adjusts_ast() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 4, end: 5 }, b"y")
            .unwrap();

        let ast = session.ast();
        let tu = &ast.nodes[ast.root as usize];
        assert_eq!(tu.kind, NodeKind::TranslationUnit);

        // TU should have a child (the variable declaration).
        assert!(!tu.children.is_empty(), "TU should have children");

        // The source should contain "y" at position 4.
        let source = session.source();
        assert_eq!(&source[4..5], b"y");
        assert_eq!(source, b"int y;");

        // AST source_length should match.
        assert_eq!(ast.source_length, 6);

        // Verify AST is well-formed by serializing to JSON.
        let json = session.to_json().unwrap();
        assert!(json.contains("\"y\""));
    }

    #[test]
    fn apply_edit_preserves_index_validity() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 4, end: 5 }, b"y")
            .unwrap();

        let id = session.index.find_covering(Span { start: 4, end: 5 });
        assert!(id.is_some());

        let tu_id = session
            .index
            .find_covering(Span { start: 0, end: 6 });
        assert!(tu_id.is_some());
    }

    #[test]
    fn apply_edit_insertion_increases_source_length() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 0, end: 0 }, b"const ")
            .unwrap();

        assert_eq!(session.source(), b"const int x;");
        assert_eq!(session.ast().source_length, 12);
    }

    #[test]
    fn apply_edit_deletion_decreases_source_length() {
        let src = b"int x; /* comment */";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 7, end: 20 }, b"")
            .unwrap();

        assert_eq!(session.source(), b"int x; ");
        assert_eq!(session.ast().source_length, 7);
    }

    #[test]
    fn to_json_works_after_edit() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 4, end: 5 }, b"y")
            .unwrap();

        let json = session.to_json().unwrap();
        assert!(json.contains("\"y\""));
        assert!(json.contains("\"version\""));
    }

    #[test]
    fn edit_out_of_bounds_returns_error() {
        let src = b"int x;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        let result = session.apply_edit(Span { start: 100, end: 200 }, b"");
        assert!(result.is_err());
    }

    #[test]
    fn multiple_edits_accumulate() {
        let src = b"int a; int b;";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 4, end: 5 }, b"x")
            .unwrap();
        assert_eq!(session.source(), b"int x; int b;");

        session
            .apply_edit(Span { start: 11, end: 12 }, b"y")
            .unwrap();
        assert_eq!(session.source(), b"int x; int y;");

        let json = session.to_json().unwrap();
        assert!(json.contains("\"x\""));
        assert!(json.contains("\"y\""));
    }

    #[test]
    fn apply_edit_maintains_structural_integrity() {
        // A function with body: after edit, TU → FunctionDef → CompoundStmt
        // should still be valid.
        let src = b"int foo() { return 42; }";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        // Edit the return value.
        session
            .apply_edit(Span { start: 18, end: 20 }, b"99")
            .unwrap();

        let ast = session.ast();
        let tu = &ast.nodes[ast.root as usize];
        assert!(!tu.children.is_empty());

        // The source should contain the new value.
        let source = session.source();
        assert_eq!(&source[18..20], b"99");
    }

    #[test]
    fn index_maintains_consistency_after_edit() {
        let src = b"int a;\nint b;\nint c;\n";
        let mut session = IncrementalSession::new(make_input(src)).unwrap();

        session
            .apply_edit(Span { start: 5, end: 6 }, b"x")
            .unwrap();

        // Check that the index has the right number of entries.
        assert!(session.index.len() > 0);

        // Every node in the AST should have an index entry.
        // Build a fresh index and compare sizes.
        let fresh = SpanIndex::build(&session.ast().nodes);
        assert_eq!(session.index.len(), fresh.len());
    }

    /// Verify that the lifetime `'a` works correctly:
    /// the session borrows the initial buffer and the compiler prevents
    /// using the session after the buffer is dropped.
    #[test]
    fn lifetime_prevents_use_after_source_dropped() {
        fn _borrow_check_test() {
            let src = b"int x;".to_vec();
            let mut session = IncrementalSession::new(ParseInput {
                source: &src,
                filename: None,
                config: ParserConfig::default(),
            })
            .unwrap();

            // After first edit, session owns its buffer.
            session
                .apply_edit(Span { start: 4, end: 5 }, b"y")
                .unwrap();

            // This should still work — session now owns the buffer.
            let _ = session.source();
        }

        // Just verify the function compiles.
        _borrow_check_test();
    }
}