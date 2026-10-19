//! Error construction helpers for the parser.
//!
//! Provides functions that create [`ParseError`] values with stable error
//! codes, following the `E_SYNTAX_*` prefix convention.

use crate::ast::Span;
use crate::error::{ParseError, Severity};

/// Build a syntax error with the given code, message, and span.
pub fn syntax_error(code: &str, message: String, span: Span) -> ParseError {
    ParseError {
        code: code.to_string(),
        message,
        span,
        severity: Severity::Error,
    }
}

/// Unexpected token error.
pub fn unexpected_token(span: Span, expected: &str, found: &str) -> ParseError {
    syntax_error(
        "E_SYNTAX_UNEXPECTED_TOKEN",
        format!("expected {}, found {}", expected, found),
        span,
    )
}

/// Missing semicolon.
pub fn expected_semicolon(span: Span) -> ParseError {
    syntax_error(
        "E_SYNTAX_EXPECTED_SEMICOLON",
        "expected `;`".to_string(),
        span,
    )
}

/// Missing closing bracket/brace/paren.
pub fn expected_closing(span: Span, what: &str) -> ParseError {
    syntax_error(
        "E_SYNTAX_EXPECTED_CLOSING",
        format!("expected closing `{}`", what),
        span,
    )
}

/// Expected an identifier but found something else.
pub fn expected_identifier(span: Span) -> ParseError {
    syntax_error(
        "E_SYNTAX_EXPECTED_IDENTIFIER",
        "expected identifier".to_string(),
        span,
    )
}

/// Expected an expression.
pub fn expected_expression(span: Span) -> ParseError {
    syntax_error(
        "E_SYNTAX_EXPECTED_EXPRESSION",
        "expected expression".to_string(),
        span,
    )
}

/// Generic syntax error.
pub fn generic_syntax(span: Span, msg: String) -> ParseError {
    syntax_error("E_SYNTAX_ERROR", msg, span)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    #[test]
    fn syntax_error_has_correct_severity() {
        let err = syntax_error("E_FOO", "bar".into(), Span { start: 0, end: 1 });
        assert_eq!(err.code, "E_FOO");
        assert_eq!(err.message, "bar");
        assert_eq!(err.severity, Severity::Error);
    }

    #[test]
    fn unexpected_token_error() {
        let err = unexpected_token(Span { start: 5, end: 6 }, "';'", "'}'");
        assert!(err.message.contains("';'"));
        assert!(err.message.contains("'}'"));
        assert_eq!(err.code, "E_SYNTAX_UNEXPECTED_TOKEN");
    }

    #[test]
    fn expected_semicolon_error() {
        let err = expected_semicolon(Span { start: 10, end: 10 });
        assert!(err.message.contains(";"));
        assert_eq!(err.code, "E_SYNTAX_EXPECTED_SEMICOLON");
    }

    #[test]
    fn expected_closing_error() {
        let err = expected_closing(Span { start: 1, end: 2 }, ")");
        assert!(err.message.contains(")"));
        assert_eq!(err.code, "E_SYNTAX_EXPECTED_CLOSING");
    }

    #[test]
    fn expected_identifier_error() {
        let err = expected_identifier(Span { start: 0, end: 0 });
        assert_eq!(err.code, "E_SYNTAX_EXPECTED_IDENTIFIER");
    }

    #[test]
    fn expected_expression_error() {
        let err = expected_expression(Span { start: 0, end: 0 });
        assert_eq!(err.code, "E_SYNTAX_EXPECTED_EXPRESSION");
    }

    #[test]
    fn generic_syntax_error() {
        let err = generic_syntax(Span { start: 1, end: 5 }, "oops".into());
        assert_eq!(err.code, "E_SYNTAX_ERROR");
        assert_eq!(err.message, "oops");
    }
}
