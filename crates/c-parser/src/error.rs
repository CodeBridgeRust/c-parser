//! Error and diagnostic types.
//!
//! Defines the stable structured diagnostics returned by the parser: a
//! machine-readable [`ParseError`] with a stable error code, a human-readable
//! message, a [`Span`] and a [`Severity`]. Non-fatal diagnostics are collected
//! into the AST's `errors` array; fatal errors are returned as `Err` through the
//! public API.
//!
//! ## Scaffold status
//!
//! The data types below are part of the public skeleton. The full set of stable
//! error codes (`E_LEX_*`, `E_SYNTAX_*`, `E_PP_*`) and the `thiserror`-based
//! error handling are completed in later stages.

use serde::{Deserialize, Serialize};

use crate::ast::Span;

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Info,
}

/// A single structured diagnostic produced during lexing/parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseError {
    /// Stable machine-readable error code, e.g. `"E_SYNTAX_EXPECTED_SEMICOLON"`.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Byte range of the problematic fragment in the source buffer.
    pub span: Span,
    /// Diagnostic severity.
    pub severity: Severity,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ParseError {}
