//! `c-parser` — zero-copy, arena-based parser for the C language.
//!
//! ## Overview
//!
//! This crate performs lexical and syntactic analysis of C source code and
//! builds an arena-based AST that can be serialized to a self-contained JSON
//! format (see `docs/ast-schema.json` v1.0).  The design follows the
//! technical specification and architectural documents in `docs/`:
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

pub mod arena;
pub mod ast;
pub mod error;
pub mod incremental;
pub mod index;
pub mod lexer;
pub mod parser;
pub mod serialize;

// Re-export core types for a flat public API surface.
pub use arena::Arena;
pub use ast::{Ast, Node, NodeData, NodeId, NodeKind, Span};
pub use error::{ParseError, Severity};
pub use lexer::token::{CDialect, PreprocessingMode};
pub use serialize::{ast_from_json, ast_to_json, ast_to_pretty_json};

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// Input to the parser.
#[derive(Clone, Debug)]
pub struct ParseInput<'a> {
    /// Source buffer.  NOT required to be valid UTF-8.
    pub source: &'a [u8],
    /// Filename hint (diagnostics only).
    pub filename: Option<&'a str>,
    /// Parser configuration.
    pub config: ParserConfig,
}

/// Parser configuration.
#[derive(Clone, Debug, Default)]
pub struct ParserConfig {
    /// C dialect (default: C11).
    pub dialect: CDialect,
    /// Preprocessing mode (default: Preprocessed).
    pub preprocessing: PreprocessingMode,
    /// Emit comment tokens instead of skipping them.
    pub include_comments: bool,
    /// Incremental mode config (default: None — full parse).
    pub incremental: Option<IncrementalConfig>,
}

/// Configuration for incremental mode.
#[derive(Clone, Debug, Default)]
pub struct IncrementalConfig {
    pub enabled: bool,
}

/// Result of a parse operation.
#[derive(Clone, Debug)]
pub struct ParseResult {
    /// The parsed AST (includes the `errors` array internally).
    pub ast: Ast,
}

impl ParseResult {
    /// Convenience accessor for non-fatal diagnostics.
    pub fn errors(&self) -> &[ParseError] {
        &self.ast.errors
    }
}

// ---------------------------------------------------------------------------
// ParserPlugin trait
// ---------------------------------------------------------------------------

/// The plugin contract that `code-bridge` uses to invoke the parser.
///
/// Implementations must be `Send + Sync` so that independent parser instances
/// can operate in parallel.
pub trait ParserPlugin: Send + Sync {
    /// Returns `"c"` — the language identifier for the JSON AST.
    fn language(&self) -> &'static str;

    /// Full parse.  Returns `Err` only for fatal errors (resource exhaustion,
    /// invalid span, etc.).  Non-fatal syntax errors appear in
    /// `ParseResult::errors()` and as `ErrorNode` entries in the AST.
    fn parse(&self, input: ParseInput<'_>) -> Result<ParseResult, ParseError>;

    /// In-memory incremental update.  `change` describes the byte range that
    /// was modified in `input.source` relative to the parser's current state.
    ///
    /// **Note:** the API contract matches `ТЗ §7.1`, but for most consumers
    /// the preferred interface is [`IncrementalSession`] (see `docs/api.md` §6).
    fn parse_incremental(
        &self,
        input: ParseInput<'_>,
        change: Span,
    ) -> Result<ParseResult, ParseError>;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Create a parser plugin with the given configuration.
///
/// ```rust
/// use c_parser::{create_parser, ParserConfig};
/// let parser = create_parser(ParserConfig::default());
/// ```
pub fn create_parser(_config: ParserConfig) -> Box<dyn ParserPlugin> {
    // Stub — full implementation in Stage 4 (parser).
    struct StubParser;
    impl ParserPlugin for StubParser {
        fn language(&self) -> &'static str {
            "c"
        }
        fn parse(&self, _input: ParseInput<'_>) -> Result<ParseResult, ParseError> {
            Err(ParseError {
                code: "E_STUB".to_string(),
                message: "parser not yet implemented".to_string(),
                span: Span { start: 0, end: 0 },
                severity: Severity::Error,
            })
        }
        fn parse_incremental(
            &self,
            _input: ParseInput<'_>,
            _change: Span,
        ) -> Result<ParseResult, ParseError> {
            Err(ParseError {
                code: "E_STUB".to_string(),
                message: "parser not yet implemented".to_string(),
                span: Span { start: 0, end: 0 },
                severity: Severity::Error,
            })
        }
    }

    Box::new(StubParser)
}