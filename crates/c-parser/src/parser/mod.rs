//! Syntactic analysis: recursive descent + Pratt parsing.
//!
//! The parser operates in two coordinated phases:
//!
//! - **Phase A** — recursive descent with a context symbol table for resolving
//!   the `typedef-name` vs `identifier` ambiguity, parsing declarations and
//!   statements.
//! - **Phase B** — Pratt parsing for expressions with operator precedence and
//!   associativity, in linear time.
//!
//! The [`Parser`] struct owns the arena, the token stream, the symbol table,
//! and the error list. Phases coordinate by delegating expression parsing to
//! Phase B and returning control to Phase A.

pub mod errors;
pub mod phase_a;
pub mod phase_b;
pub mod symbol_table;

use crate::arena::{Arena, ArenaError};
use crate::ast::node_data::NodeData;
use crate::ast::text_slice::TextSlice;
use crate::ast::{Ast, Node, NodeId, NodeKind, Span};
use crate::error::ParseError;
use crate::lexer::token::{CDialect, PreprocessingMode, Token, TokenKind};
use crate::lexer::Lexer;

use symbol_table::SymbolTable;

/// The C parser.
///
/// Owns the source buffer reference, a tokenizer (Lexer), an arena for
/// building AST nodes, a symbol table for typedef resolution, and the error
/// accumulator.
pub struct Parser<'a> {
    /// Source buffer (BOM-stripped).
    source: &'a [u8],
    /// C dialect.
    dialect: CDialect,
    /// Preprocessing mode.
    preprocessing: PreprocessingMode,
    /// The lexer that produces tokens on demand.
    lexer: Lexer<'a>,
    /// Look-ahead token (one token of peek buffer).
    peek: Option<Token>,
    /// The node arena.
    arena: Arena,
    /// Contextual symbol table for typedef resolution.
    sym: SymbolTable,
    /// Accumulated non-fatal errors.
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given source buffer and configuration.
    pub fn new(
        source: &'a [u8],
        dialect: CDialect,
        preprocessing: PreprocessingMode,
        include_comments: bool,
    ) -> Self {
        let lexer = Lexer::new(source, dialect, preprocessing, include_comments);
        Self {
            source,
            dialect,
            preprocessing,
            lexer,
            peek: None,
            // Pre-size the node arena from the source length.  Dense C code
            // produces roughly one AST node per ~12 bytes of source, so this
            // estimate avoids the repeated exponential reallocations (and their
            // transient 2× peak) that an empty arena would otherwise incur on
            // multi-megabyte inputs.  The estimate is deliberately
            // conservative; the arena still grows if it is exceeded.
            arena: Arena::with_capacity(arena_capacity_hint(source.len())),
            sym: SymbolTable::new(),
            errors: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Token stream helpers
    // ------------------------------------------------------------------

    /// Return the current look-ahead token, fetching from the lexer if needed.
    pub fn current(&mut self) -> Token {
        if self.peek.is_none() {
            self.peek = self.lexer.next();
        }
        self.peek.unwrap_or(Token::new(
            TokenKind::Eof,
            self.source.len() as u32,
            self.source.len() as u32,
        ))
    }

    /// Consume the current token and advance to the next.
    pub fn advance(&mut self) -> Token {
        let tok = self.current();
        self.peek = None;
        tok
    }

    /// Peek at the current token kind.
    pub fn current_kind(&mut self) -> TokenKind {
        self.current().kind
    }

    /// Return `true` if the current token has the given kind.
    pub fn check(&mut self, kind: TokenKind) -> bool {
        self.current_kind() == kind
    }

    /// If the current token is `kind`, consume it and return `true`.
    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume the current token and assert it is `kind`. If not, record an
    /// error and return the structured diagnostic.
    pub fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        let tok = self.current();
        if tok.kind == kind {
            self.peek = None;
            Ok(tok)
        } else {
            self.errors.push(errors::unexpected_token(
                tok.span,
                &format!("{:?}", kind),
                &format!("{:?}", tok.kind),
            ));
            Err(ParseError {
                code: "E_SYNTAX_UNEXPECTED_TOKEN".to_string(),
                message: format!("expected {:?}, found {:?}", kind, tok.kind),
                span: tok.span,
                severity: crate::error::Severity::Error,
            })
        }
    }

    /// Try to match `kind`; on success consume and return token. On failure
    /// return `None` without consuming or recording an error.
    pub fn try_eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.check(kind) {
            Some(self.advance())
        } else {
            None
        }
    }

    // ------------------------------------------------------------------
    // Source helpers
    // ------------------------------------------------------------------

    /// Return a reference to the source buffer.
    pub fn source(&self) -> &'a [u8] {
        self.source
    }

    /// Return the source length.
    pub fn source_len(&self) -> usize {
        self.source.len()
    }

    /// Materialize a span as a TextSlice.
    pub fn text_slice(&self, span: Span) -> TextSlice {
        TextSlice::from_span(span.start, span.end)
    }

    /// Materialize the bytes in a span to a lossy String.
    pub fn span_str(&self, span: Span) -> String {
        String::from_utf8_lossy(&self.source[span.start as usize..span.end as usize]).into_owned()
    }

    // ------------------------------------------------------------------
    // Arena helpers
    // ------------------------------------------------------------------

    /// Push a node into the arena. Returns a fatal error on overflow.
    pub fn push_node(&mut self, node: Node) -> Result<NodeId, ArenaError> {
        self.arena.push(node)
    }

    /// Create a structural node and push it.
    pub fn push_structural(
        &mut self,
        kind: NodeKind,
        span: Span,
        children: Vec<NodeId>,
    ) -> Result<NodeId, ArenaError> {
        self.push_node(Node::structural(kind, span, children))
    }

    /// Create a node with payload and push it.
    pub fn push_with_data(
        &mut self,
        kind: NodeKind,
        span: Span,
        children: Vec<NodeId>,
        data: NodeData,
    ) -> Result<NodeId, ArenaError> {
        self.push_node(Node::with_data(kind, span, children, data))
    }

    /// Return the number of nodes in the arena.
    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    // ------------------------------------------------------------------
    // Error helpers
    // ------------------------------------------------------------------

    /// Record a non-fatal parse error.
    pub fn error(&mut self, err: ParseError) {
        self.errors.push(err);
    }

    /// Build an ErrorNode, push it into the arena, and return its id.
    /// The `children` are the nodes that were built so far for the
    /// problematic construct (may be empty).
    pub fn push_error_node(
        &mut self,
        span: Span,
        children: Vec<NodeId>,
    ) -> Result<NodeId, ArenaError> {
        let error_index = self.errors.len() as u32;
        // We need a pre-allocated error; the caller should have already
        // called `self.error(...)` with a suitable message before
        // calling this method.
        // If not, push a generic one.
        if error_index as usize >= self.errors.len() {
            self.errors.push(ParseError {
                code: "E_SYNTAX_ERROR".to_string(),
                message: "syntax error".to_string(),
                span,
                severity: crate::error::Severity::Error,
            });
        }
        let idx = self.errors.len() - 1;
        self.push_with_data(
            NodeKind::ErrorNode,
            span,
            children,
            NodeData::ErrorNode {
                error_index: idx as u32,
            },
        )
    }

    /// Skip tokens until one of the synchronization points is reached:
    /// `;`, `{`, `}`, or EOF.
    /// Returns the span covering the skipped region.
    pub fn synchronize(&mut self) -> Span {
        let start = self.current().span.start;
        loop {
            match self.current_kind() {
                TokenKind::Semi | TokenKind::LBrace | TokenKind::RBrace | TokenKind::Eof => break,
                _ => {
                    self.advance();
                }
            }
        }
        let end = self.current().span.start;
        Span { start, end }
    }

    // ------------------------------------------------------------------
    // Symbol table
    // ------------------------------------------------------------------

    /// Register a typedef name.
    pub fn add_typedef(&mut self, span: Span) {
        self.sym.add_typedef(span);
    }

    /// Push a new scope.
    pub fn push_scope(&mut self) {
        self.sym.push_scope();
    }

    /// Pop the current scope.
    pub fn pop_scope(&mut self) {
        self.sym.pop_scope();
    }

    /// Returns `true` if the identifier at `span` is a known typedef name.
    pub fn is_typedef(&self, span: Span) -> bool {
        self.sym.is_typedef(span, self.source)
    }

    // ------------------------------------------------------------------
    // Dialect / Preprocessing
    // ------------------------------------------------------------------

    /// Return the C dialect.
    pub fn dialect(&self) -> CDialect {
        self.dialect
    }

    /// Return the preprocessing mode.
    pub fn preprocessing(&self) -> PreprocessingMode {
        self.preprocessing
    }

    // ------------------------------------------------------------------
    // Finalize
    // ------------------------------------------------------------------

    /// Consume the parser and build the final `Ast`.
    ///
    /// `root` is the `NodeId` of the `TranslationUnit` node.
    pub fn finish(self, root: NodeId) -> Ast {
        Ast::new(
            root,
            self.arena.into_nodes(),
            self.source.len(),
            self.errors,
        )
    }

    /// Return the accumulated errors (for inspection during parsing).
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    /// Parse a region of source text and return the top-level node ids.
    ///
    /// This is used by [`crate::incremental::IncrementalSession`] for local
    /// re-parsing.  If the region starts with `{`, it is parsed as a compound
    /// statement and the returned ids are the statement nodes inside it.
    /// Otherwise it is parsed as a sequence of external declarations.
    ///
    /// The returned `NodeId`s are relative to this parser's arena.
    pub fn parse_region(&mut self) -> Result<Vec<NodeId>, ParseError> {
        if self.current_kind() == TokenKind::LBrace {
            let compound_id = self.parse_compound_statement()?;
            let children = self.arena.get(compound_id).children.clone();
            Ok(children)
        } else {
            let mut children = Vec::new();
            while self.current_kind() != TokenKind::Eof {
                match self.parse_external_declaration() {
                    Ok(id) => children.push(id),
                    Err(_e) => {
                        // Error recovery: synchronize and continue.
                        self.synchronize();
                        if self.current_kind() != TokenKind::Eof {
                            self.advance();
                        }
                    }
                }
            }
            Ok(children)
        }
    }
}

/// Estimate an initial arena capacity from the source length.
///
/// Realistic C (e.g. `sqlite3.c`) produces roughly one AST node per ~12–15
/// source bytes when measured over whole files (comments, whitespace and
/// string literals dilute the token density).  We reserve at `source_len / 12`
/// so the common case does not trigger a follow-up reallocation, while keeping
/// the up-front reservation close to the final working set.  Very-dense
/// synthetic inputs may still grow the arena; `Vec`'s amortized growth handles
/// that without a pathological overhead.
#[inline]
fn arena_capacity_hint(source_len: usize) -> usize {
    source_len / 12
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    fn make_parser(source: &[u8]) -> Parser<'_> {
        Parser::new(
            source,
            CDialect::C11,
            PreprocessingMode::Preprocessed,
            false,
        )
    }

    #[test]
    fn token_stream_basics() {
        let mut p = make_parser(b"int x;");
        assert_eq!(p.current_kind(), TokenKind::Int);
        p.advance();
        assert_eq!(p.current_kind(), TokenKind::Identifier);
        p.advance();
        assert_eq!(p.current_kind(), TokenKind::Semi);
        p.advance();
        assert_eq!(p.current_kind(), TokenKind::Eof);
    }

    #[test]
    fn push_node_works() {
        let mut p = make_parser(b"");
        let id = p
            .push_structural(NodeKind::TranslationUnit, Span { start: 0, end: 0 }, vec![])
            .unwrap();
        assert_eq!(id, 0);
        assert_eq!(p.node_count(), 1);
    }

    #[test]
    fn typedef_symbol_table() {
        let source = b"typedef int myint; myint x;";
        // myint at 12..17, use at 19..24
        let mut p = make_parser(source);
        p.add_typedef(Span { start: 12, end: 17 }); // "myint"
        assert!(p.is_typedef(Span { start: 12, end: 17 }));
        // Same name at a different location should still be recognized
        assert!(p.is_typedef(Span { start: 19, end: 24 }));
    }
}
