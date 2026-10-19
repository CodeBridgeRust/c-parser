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

// ---------------------------------------------------------------------------
// Conversion from ArenaError
// ---------------------------------------------------------------------------

impl From<crate::arena::ArenaError> for ParseError {
    fn from(_err: crate::arena::ArenaError) -> Self {
        ParseError {
            code: "E_FATAL".to_string(),
            message: "arena overflow: exceeded maximum node capacity".to_string(),
            span: crate::ast::Span { start: 0, end: 0 },
            severity: Severity::Error,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ArenaError;
    use crate::ast::Span;

    #[test]
    fn severity_display() {
        assert_eq!(format!("{:?}", Severity::Error), "Error");
        assert_eq!(format!("{:?}", Severity::Warning), "Warning");
        assert_eq!(format!("{:?}", Severity::Note), "Note");
        assert_eq!(format!("{:?}", Severity::Info), "Info");
    }

    #[test]
    fn severity_serialize_lowercase() {
        assert_eq!(
            serde_json::to_string(&Severity::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
        assert_eq!(serde_json::to_string(&Severity::Note).unwrap(), "\"note\"");
        assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"info\"");
    }

    #[test]
    fn severity_deserialize() {
        assert_eq!(
            serde_json::from_str::<Severity>("\"error\"").unwrap(),
            Severity::Error
        );
        assert_eq!(
            serde_json::from_str::<Severity>("\"warning\"").unwrap(),
            Severity::Warning
        );
    }

    #[test]
    fn parse_error_display() {
        let err = ParseError {
            code: "E_TEST".into(),
            message: "test message".into(),
            span: Span { start: 1, end: 2 },
            severity: Severity::Error,
        };
        let displayed = format!("{err}");
        assert!(displayed.contains("E_TEST"));
        assert!(displayed.contains("test message"));
    }

    #[test]
    fn parse_error_is_std_error() {
        let err = ParseError {
            code: "E_TEST".into(),
            message: "oops".into(),
            span: Span { start: 0, end: 0 },
            severity: Severity::Warning,
        };
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn parse_error_serialize_deserialize() {
        let err = ParseError {
            code: "E_SYNTAX".into(),
            message: "bad".into(),
            span: Span { start: 10, end: 15 },
            severity: Severity::Warning,
        };
        let json = serde_json::to_string(&err).unwrap();
        let d: ParseError = serde_json::from_str(&json).unwrap();
        assert_eq!(err.code, d.code);
        assert_eq!(err.message, d.message);
        assert_eq!(err.span, d.span);
        assert_eq!(err.severity, d.severity);
    }

    #[test]
    fn arena_error_to_parse_error() {
        let err: ParseError = ArenaError::Overflow.into();
        assert_eq!(err.code, "E_FATAL");
        assert!(err.message.contains("overflow"));
    }

    #[test]
    fn arena_error_display() {
        let e = ArenaError::Overflow;
        let s = format!("{e}");
        assert!(s.contains("overflow"));
    }

    #[test]
    fn severity_clone_debug() {
        let s = Severity::Note;
        assert_eq!(s.clone(), Severity::Note);
        let _ = format!("{:?}", s);
    }
}
