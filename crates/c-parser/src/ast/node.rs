//! Arena-level types: [`Node`], [`Ast`], and the JSON-deserialization
//! adapter [`AstJson`].
//!
//! ## `AstJson` — deserialization bridge
//!
//! The public [`Ast`] stores string fields as [`TextSlice`] (usually
//! `Span`).  When reading JSON the spans are unknown, so we deserialize into
//! `AstJson` (which uses `TextSlice::Detached` via the normal `Deserialize`
//! impl), then convert to `Ast` with [`Ast::from_json_repr`].

use crate::ast::node_data::NodeData;
use crate::ast::{NodeId, NodeKind, Span};
use crate::error::ParseError;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// A single AST node in the arena.
///
/// Nodes reference children by 32-bit index (`NodeId`) into the arena's
/// `Vec<Node>`.  String payloads are stored as `TextSlice::Span` during
/// parsing and lazily materialized at serialization time.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
    pub children: Vec<NodeId>,
    /// `None` for structural nodes (e.g. `IfStatement`, `CompoundStatement`),
    /// which serialize as `"data": null`.
    pub data: Option<NodeData>,
}

impl Node {
    /// Create a structural node (no payload).
    pub fn structural(kind: NodeKind, span: Span, children: Vec<NodeId>) -> Self {
        Self {
            kind,
            span,
            children,
            data: None,
        }
    }

    /// Create a node with payload.
    pub fn with_data(
        kind: NodeKind,
        span: Span,
        children: Vec<NodeId>,
        data: NodeData,
    ) -> Self {
        Self {
            kind,
            span,
            children,
            data: Some(data),
        }
    }
}

// ---------------------------------------------------------------------------
// Ast
// ---------------------------------------------------------------------------

/// The complete AST, represented as a flat arena of [`Node`]s.
///
/// ## Invariants (checked by parser, not by the type system)
///
/// - `root` and every `children[i]` entry must reference a valid index in
///   `nodes`.
/// - `span.start <= span.end` and `span.end <= source_length` for every node.
/// - `ErrorNode.data.error_index` must reference a valid entry in `errors`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Ast {
    pub root: NodeId,
    pub nodes: Vec<Node>,
    /// Length of the (BOM-stripped) source buffer.
    pub source_length: usize,
    /// Non-fatal diagnostics.  Serialized as the top-level `"errors"` array.
    pub errors: Vec<ParseError>,
}

impl Ast {
    /// Create a new AST with the given root index, node arena, source length
    /// and error list.
    pub fn new(
        root: NodeId,
        nodes: Vec<Node>,
        source_length: usize,
        errors: Vec<ParseError>,
    ) -> Self {
        Self {
            root,
            nodes,
            source_length,
            errors,
        }
    }
}

// ---------------------------------------------------------------------------
// AstJson (deserialization bridge)
// ---------------------------------------------------------------------------

/// Internal mirror of [`Ast`] used **only** for JSON deserialization.
///
/// The public `Ast` is `Deserialize` too, but `Ast::deserialize` may produce
/// `TextSlice::Detached` values from JSON strings.  `AstJson` exists so that
/// the conversion `AstJson → Ast` is explicit and documented; it is otherwise
/// structurally identical.
///
/// **Not exposed in the public API.**
#[derive(Debug, Deserialize)]
#[serde(rename = "Ast")]
pub(crate) struct AstJson {
    #[serde(default)]
    #[allow(dead_code)]
    pub version: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub language: String,
    pub source_length: usize,
    pub root: NodeId,
    pub nodes: Vec<Node>,
    pub errors: Vec<ParseError>,
}

impl Ast {
    /// Convert the internal deserialization form to the public `Ast`.
    ///
    /// This method exists so that the deserialization path is explicit and
    /// separate from the main `Ast` type.
    pub(crate) fn from_json_repr(j: AstJson) -> Self {
        Self {
            root: j.root,
            nodes: j.nodes,
            source_length: j.source_length,
            errors: j.errors,
        }
    }
}