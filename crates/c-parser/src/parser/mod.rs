//! Syntactic analysis: recursive descent + Pratt parsing.
//!
//! - **Phase A** — recursive descent with a context symbol table for resolving
//!   the `typedef-name` vs `identifier` ambiguity, parsing declarations and
//!   statements.
//! - **Phase B** — Pratt parsing for expressions with operator precedence and
//!   associativity, in linear time.
//!
//! ## Scaffold status
//!
//! The parser is implemented in Stage 4. This module currently only documents
//! its contract.
