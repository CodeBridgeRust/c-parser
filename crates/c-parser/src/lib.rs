//! `c-parser` — zero-copy, arena-based parser for the C language.
//!
//! ## Overview
//!
//! This crate performs lexical and syntactic analysis of C source code and
//! builds an arena-based AST that can be serialized to a self-contained JSON
//! format (see `docs/ast-schema.json` v1.0). The design follows the technical
//! specification and architectural documents in `docs/`:
//!
//! - **Arena-based AST** — nodes live in a flat `Vec<Node>`, linked by 32-bit
//!   `NodeId` indices (see [`arena`]).
//! - **Zero-copy lexing** — tokens store only a class and a `Span`, never
//!   copying source text (see [`lexer`]).
//! - **Lazy materialization** — string payloads are extracted from the source
//!   buffer only at serialization time (see [`serialize`]).
//! - **Two-phase parsing** — recursive descent for declarations/statements and
//!   Pratt parsing for expressions (see [`parser`]).
//! - **In-memory incremental updates** — backed by a sorted `Span -> NodeId`
//!   index (see [`incremental`] and [`index`]).
//!
//! ## Scaffold status
//!
//! This is the workspace scaffold stage: the module tree and public type
//! skeleton are in place so `cargo check` passes, but business logic is
//! implemented in subsequent stages.

pub mod arena;
pub mod ast;
pub mod error;
pub mod incremental;
pub mod index;
pub mod lexer;
pub mod parser;
pub mod serialize;

// Re-export the core types for a flat public API surface
// (`pub use c_parser::*` per the API contract).
pub use ast::{NodeId, Span};
pub use error::{ParseError, Severity};
