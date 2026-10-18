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
