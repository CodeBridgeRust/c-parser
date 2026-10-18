//! AST model: `NodeId`, `Span`, `NodeKind`, `NodeData`, `Node`, `Ast`.
//!
//! This module owns the core data types of the abstract syntax tree. It defines
//! the 32-bit arena index [`NodeId`], the half-open byte range [`Span`], and
//! (in later stages) the `NodeKind`/`NodeData` tagged union, the flat `Node`
//! record and the top-level `Ast` container together with its invariants.
//!
//! ## Scaffold status
//!
//! Only the fundamental, dependency-free types (`NodeId`, `Span`) are defined
//! here so the module tree and re-exports are stable. The full AST model and
//! JSON-mapped `NodeData` are delivered in Stage 3.

/// 32-bit index into the node arena.
///
/// Using `u32` halves the node size relative to 64-bit indices while still
/// allowing up to ~4.29 billion nodes — far beyond any realistic C code base.
pub type NodeId = u32;

/// Half-open byte range `[start, end)` into the original source buffer.
///
/// Invariants (enforced by the parser and the custom validator):
/// `start <= end` and `end <= source_length`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
