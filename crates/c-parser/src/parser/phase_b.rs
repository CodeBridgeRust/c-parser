//! Phase B: Pratt parsing for expressions.
//!
//! Expressions in C are overloaded with operators of varying precedence and
//! associativity. A classic recursive descent generates deep call stacks and
//! exhibits super-linear behaviour. Instead, we use **Pratt parsing** (top-down
//! operator precedence): a flat loop driven by a table of *binding powers*,
//! yielding O(n) linear time and bounded stack depth.
//!
//! ## Binding power table
//!
//! | Precedence | Operators                                                | Assoc    |
//! |-----------|----------------------------------------------------------|----------|
//! | 1         | `,` (comma)                                              | left     |
//! | 2         | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `<<=`, `>>=`, etc.   | right    |
//! | 3         | `?:` (ternary)                                           | right    |
//! | 4         | `\|\|`                                                   | left     |
//! | 5         | `&&`                                                     | left     |
//! | 6         | `\|`                                                     | left     |
//! | 7         | `^`                                                      | left     |
//! | 8         | `&`                                                      | left     |
//! | 9         | `==`, `!=`                                               | left     |
//! | 10        | `<`, `>`, `<=`, `>=`                                     | left     |
//! | 11        | `<<`, `>>`                                               | left     |
//! | 12        | `+`, `-`                                                 | left     |
//! | 13        | `*`, `/`, `%`                                            | left     |
//! | 14        | prefix `++`, `--`, `+`, `-`, `!`, `~`, `*`, `&`, `sizeof`| prefix   |
//! | 15        | postfix `++`, `--`, `()`, `[]`, `.`, `->`                | postfix  |
//!
//! The binding power of a token determines how tightly it binds its operands.
//! For a left-associative binary operator, its left binding power is the
//! precedence × 2, and its right binding power is precedence × 2 + 1. For a
//! right-associative operator, both are precedence × 2.

use crate::ast::node_data::NodeData;
use crate::ast::{NodeId, NodeKind, Span};
use crate::error::ParseError;
use crate::lexer::token::TokenKind;
use crate::parser::errors;
use crate::parser::Parser;

// ---------------------------------------------------------------------------
// Binding power
// ---------------------------------------------------------------------------

/// Binding power for Pratt parsing. Higher = tighter.
type BP = u8;

/// Minimum binding power (used to start the Pratt loop).
const BP_MIN: BP = 0;

/// Binding power for the comma operator.
const BP_COMMA: BP = 1;
/// Assignment operators.
const BP_ASSIGN: BP = 2;
/// Ternary (right-associative).
const BP_TERNARY: BP = 3;
/// Logical OR.
const BP_LOGICAL_OR: BP = 4;
/// Logical AND.
const BP_LOGICAL_AND: BP = 5;
/// Bitwise OR.
const BP_BIT_OR: BP = 6;
/// Bitwise XOR.
const BP_BIT_XOR: BP = 7;
/// Bitwise AND.
const BP_BIT_AND: BP = 8;
/// Equality.
const BP_EQ: BP = 9;
/// Relational.
const BP_REL: BP = 10;
/// Shift.
const BP_SHIFT: BP = 11;
/// Additive.
const BP_ADD: BP = 12;
/// Multiplicative.
const BP_MUL: BP = 13;
/// Prefix unary (highest before postfix).
const BP_PREFIX: BP = 14;
/// Postfix (highest).
const BP_POSTFIX: BP = 15;

/// Compute the left binding power for a given precedence level.
fn left_bp(prec: BP) -> BP {
    prec * 2
}

fn right_bp(_prec: BP) -> BP {
    // For non-left-assoc operators, right_bp = left_bp.
    _prec * 2
}

/// For left-assoc: the right binding power is higher so the loop continues.
fn right_bp_left_assoc(prec: BP) -> BP {
    prec * 2 + 1
}

// ---------------------------------------------------------------------------
// Operator classification
// ---------------------------------------------------------------------------

/// Get the prefix binding power of a token kind (as a prefix operator).
fn prefix_bp(kind: TokenKind) -> Option<BP> {
    match kind {
        TokenKind::PlusPlus | TokenKind::MinusMinus => Some(left_bp(BP_PREFIX)),
        TokenKind::Plus | TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => {
            Some(left_bp(BP_PREFIX))
        }
        TokenKind::Star | TokenKind::Amp => Some(left_bp(BP_PREFIX)),
        _ => None,
    }
}

/// Get the infix binding powers for a binary operator token kind.
/// Returns `(left_bp, right_bp)`.
fn infix_bp(kind: TokenKind) -> Option<(BP, BP)> {
    let prec = match kind {
        TokenKind::Comma => BP_COMMA,
        TokenKind::Eq
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::AmpEq
        | TokenKind::PipeEq
        | TokenKind::CaretEq
        | TokenKind::LShiftEq
        | TokenKind::RShiftEq => BP_ASSIGN,
        TokenKind::Question => BP_TERNARY,
        TokenKind::PipePipe => BP_LOGICAL_OR,
        TokenKind::AmpAmp => BP_LOGICAL_AND,
        TokenKind::Pipe => BP_BIT_OR,
        TokenKind::Caret => BP_BIT_XOR,
        TokenKind::Amp => BP_BIT_AND,
        TokenKind::EqEq | TokenKind::NotEq => BP_EQ,
        TokenKind::Lt | TokenKind::Gt | TokenKind::LtEq | TokenKind::GtEq => BP_REL,
        TokenKind::LShift | TokenKind::RShift => BP_SHIFT,
        TokenKind::Plus | TokenKind::Minus => BP_ADD,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => BP_MUL,
        TokenKind::LParen | TokenKind::LBracket | TokenKind::Dot | TokenKind::Arrow => BP_POSTFIX,
        TokenKind::PlusPlus | TokenKind::MinusMinus => BP_POSTFIX,
        _ => return None,
    };

    let is_right_assoc = matches!(kind, TokenKind::Eq | TokenKind::Question) || is_assignment(kind);

    let lbp = left_bp(prec);
    let rbp = if is_right_assoc {
        right_bp(prec)
    } else {
        right_bp_left_assoc(prec)
    };

    Some((lbp, rbp))
}

fn is_assignment(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Eq
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq
            | TokenKind::AmpEq
            | TokenKind::PipeEq
            | TokenKind::CaretEq
            | TokenKind::LShiftEq
            | TokenKind::RShiftEq
    )
}

/// Return true if `kind` can be a postfix operator.
fn is_postfix(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::PlusPlus
            | TokenKind::MinusMinus
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::Dot
            | TokenKind::Arrow
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Helper: map an arena error into a ParseError.
fn arena_err(span: Span) -> ParseError {
    ParseError {
        code: "E_FATAL".to_string(),
        message: "arena overflow".to_string(),
        span,
        severity: crate::error::Severity::Error,
    }
}

impl<'a> Parser<'a> {
    /// Parse an expression using Pratt parsing. This is the entry point for
    /// Phase B. Called from Phase A when an expression is expected.
    ///
    /// Returns the `NodeId` of the root of the parsed expression subtree.
    pub fn parse_expression(&mut self) -> Result<NodeId, ParseError> {
        self.parse_expr_with_bp(BP_MIN)
    }

    /// Parse an expression, stopping at operators with binding power ≤ `min_bp`.
    /// This is the core Pratt loop.
    pub fn parse_expr_with_bp(&mut self, min_bp: BP) -> Result<NodeId, ParseError> {
        // --- Prefix: nud (null denotation) ---
        let mut lhs = self.parse_prefix()?;

        // --- Infix / postfix loop: led (left denotation) ---
        loop {
            let tok = self.current();

            // Postfix operators
            if is_postfix(tok.kind) {
                let (lbp, _) = infix_bp(tok.kind).unwrap_or((0, 0));
                if lbp <= min_bp {
                    break;
                }
                lhs = self.parse_postfix(lhs)?;
                continue;
            }

            // Infix binary operators
            if let Some((lbp, rbp)) = infix_bp(tok.kind) {
                if lbp <= min_bp {
                    break;
                }
                lhs = self.parse_infix(lhs, rbp)?;
                continue;
            }

            // Colon (part of ternary) — handled inside parse_infix for `?`
            if tok.kind == TokenKind::Colon {
                break;
            }

            // Anything else: end of expression
            break;
        }

        Ok(lhs)
    }

    /// Parse a prefix expression (nud — null denotation).
    fn parse_prefix(&mut self) -> Result<NodeId, ParseError> {
        let tok = self.current();

        // --- Literals ---
        if let Some(node_id) = self.try_parse_literal()? {
            return Ok(node_id);
        }

        // --- Identifier ---
        if tok.kind == TokenKind::Identifier {
            return self.parse_identifier_expr();
        }

        // --- `sizeof` (checked BEFORE prefix operators because `sizeof`
        // is a keyword, not a regular prefix operator)
        if tok.kind == TokenKind::Sizeof {
            return self.parse_sizeof();
        }

        // --- Prefix operators ---
        if prefix_bp(tok.kind).is_some() {
            return self.parse_prefix_op();
        }

        // --- Parenthesized expression or cast ---
        if tok.kind == TokenKind::LParen {
            return self.parse_paren_or_cast();
        }

        // --- Error: unexpected token ---
        let error_span = tok.span;
        let synced = self.synchronize();
        let err_span = Span {
            start: error_span.start,
            end: synced.end,
        };
        self.error(errors::unexpected_token(
            err_span,
            "expression",
            &format!("{:?}", tok.kind),
        ));
        self.push_error_node(err_span, vec![])
            .map_err(|_| arena_err(err_span))
    }

    /// Try to parse a literal token (integer, float, char, string).
    fn try_parse_literal(&mut self) -> Result<Option<NodeId>, ParseError> {
        let tok = self.current();
        match tok.kind {
            TokenKind::IntegerLiteral => {
                self.advance();
                let node = self
                    .push_with_data(
                        NodeKind::IntegerLiteral,
                        tok.span,
                        vec![],
                        NodeData::IntegerLiteral {
                            value: self.text_slice(tok.span),
                            suffix: None,
                        },
                    )
                    .map_err(|_| arena_err(tok.span))?;
                Ok(Some(node))
            }
            TokenKind::FloatLiteral => {
                self.advance();
                let node = self
                    .push_with_data(
                        NodeKind::FloatLiteral,
                        tok.span,
                        vec![],
                        NodeData::FloatLiteral {
                            value: self.text_slice(tok.span),
                            suffix: None,
                        },
                    )
                    .map_err(|_| arena_err(tok.span))?;
                Ok(Some(node))
            }
            TokenKind::CharLiteral => {
                self.advance();
                let node = self
                    .push_with_data(
                        NodeKind::CharLiteral,
                        tok.span,
                        vec![],
                        NodeData::CharLiteral {
                            value: self.text_slice(tok.span),
                            encoding: None,
                        },
                    )
                    .map_err(|_| arena_err(tok.span))?;
                Ok(Some(node))
            }
            TokenKind::StringLiteral => {
                self.advance();
                let node = self
                    .push_with_data(
                        NodeKind::StringLiteral,
                        tok.span,
                        vec![],
                        NodeData::StringLiteral {
                            value: self.text_slice(tok.span),
                            encoding: None,
                        },
                    )
                    .map_err(|_| arena_err(tok.span))?;
                Ok(Some(node))
            }
            _ => Ok(None),
        }
    }

    /// Parse an identifier — could be a simple reference or a typedef name
    /// (which is still an Identifier node at the AST level).
    fn parse_identifier_expr(&mut self) -> Result<NodeId, ParseError> {
        let tok = self.advance();
        self.push_with_data(
            NodeKind::Identifier,
            tok.span,
            vec![],
            NodeData::Identifier {
                name: self.text_slice(tok.span),
            },
        )
        .map_err(|_| arena_err(tok.span))
    }

    /// Parse a prefix operator expression: `++x`, `--x`, `+x`, `-x`, `!x`,
    /// `~x`, `*x`, `&x`.
    fn parse_prefix_op(&mut self) -> Result<NodeId, ParseError> {
        let op_tok = self.advance();
        let op_span = op_tok.span;
        let op_text = self.text_slice(op_span);

        let operand = self.parse_expr_with_bp(left_bp(BP_PREFIX))?;

        let span = Span {
            start: op_span.start,
            end: self.span_of(operand).end,
        };

        self.push_with_data(
            NodeKind::UnaryExpr,
            span,
            vec![operand],
            NodeData::UnaryExpr {
                op: op_text,
                prefix: true,
            },
        )
        .map_err(|_| arena_err(span))
    }

    /// Parse `sizeof` expression or `sizeof(type)`.
    fn parse_sizeof(&mut self) -> Result<NodeId, ParseError> {
        let sizeof_tok = self.advance(); // consume `sizeof`
        let start = sizeof_tok.span.start;

        // `sizeof` unary expression (no parens): sizeof x
        if !self.check(TokenKind::LParen) {
            let operand = self.parse_expr_with_bp(left_bp(BP_PREFIX))?;
            let span = Span {
                start,
                end: self.span_of(operand).end,
            };
            return self
                .push_with_data(
                    NodeKind::SizeofExpr,
                    span,
                    vec![operand],
                    NodeData::SizeofExpr { is_variadic: false },
                )
                .map_err(|_| arena_err(span));
        }

        // `sizeof(...)` — could be expression or type name.
        // For MVP, we treat `sizeof(...)` with parens as a unary expr on
        // the parenthesized content.
        self.advance(); // consume `(`
        let inner = self.parse_expression()?;
        // Try to consume `)`
        if self.check(TokenKind::RParen) {
            self.advance();
        }
        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_with_data(
            NodeKind::SizeofExpr,
            span,
            vec![inner],
            NodeData::SizeofExpr { is_variadic: false },
        )
        .map_err(|_| arena_err(span))
    }

    /// Parse a parenthesized expression, or a cast expression.
    fn parse_paren_or_cast(&mut self) -> Result<NodeId, ParseError> {
        let lparen = self.advance(); // consume `(`
        let start = lparen.span.start;

        // Check for cast: `(type-name) expr`
        // We detect casts when the content looks like a type keyword or
        // a typedef name. Need to peek ahead.
        let kind = self.current_kind();
        let cur_span = self.current().span;
        let is_cast = self.is_type_keyword(kind)
            || (kind == TokenKind::Identifier && self.is_typedef(cur_span));

        if is_cast {
            let type_id = self.parse_type_specifier()?;
            // Consume `)`
            if self.check(TokenKind::RParen) {
                self.advance();
            }

            // Cast binds like prefix.
            let operand = self.parse_expr_with_bp(left_bp(BP_PREFIX))?;

            let span = Span {
                start,
                end: self.span_of(operand).end,
            };
            return self
                .push_with_data(
                    NodeKind::CastExpr,
                    span,
                    vec![type_id, operand],
                    NodeData::CastExpr,
                )
                .map_err(|_| arena_err(span));
        }

        // Otherwise, it's a parenthesized expression.
        let inner = self.parse_expression()?;
        if self.check(TokenKind::RParen) {
            self.advance();
        }
        let span = Span {
            start,
            end: self.current().span.start,
        };
        // ParenExpr is a structural kind (data: null per schema).
        self.push_structural(NodeKind::ParenExpr, span, vec![inner])
            .map_err(|_| arena_err(span))
    }

    /// Parse a postfix expression: `x++`, `x--`, `f()`, `a[i]`, `s.m`, `p->m`.
    fn parse_postfix(&mut self, lhs: NodeId) -> Result<NodeId, ParseError> {
        let tok = self.current();

        match tok.kind {
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                self.advance();
                let op_text = self.text_slice(tok.span);
                let span = Span {
                    start: self.span_of(lhs).start,
                    end: tok.span.end,
                };
                self.push_with_data(
                    NodeKind::UnaryExpr,
                    span,
                    vec![lhs],
                    NodeData::UnaryExpr {
                        op: op_text,
                        prefix: false,
                    },
                )
                .map_err(|_| arena_err(span))
            }
            TokenKind::LParen => {
                // Function call
                self.advance(); // consume `(`
                let mut args = Vec::new();
                if !self.check(TokenKind::RParen) {
                    loop {
                        args.push(self.parse_assignment_or_expr()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                if self.check(TokenKind::RParen) {
                    self.advance();
                }
                let span = Span {
                    start: self.span_of(lhs).start,
                    end: self.current().span.start,
                };
                let mut children = vec![lhs];
                children.extend(args);
                self.push_with_data(
                    NodeKind::CallExpr,
                    span,
                    children,
                    NodeData::CallExpr { is_macro: None },
                )
                .map_err(|_| arena_err(span))
            }
            TokenKind::LBracket => {
                // Array subscript
                self.advance(); // consume `[`
                let index = self.parse_expression()?;
                if self.check(TokenKind::RBracket) {
                    self.advance();
                }
                let span = Span {
                    start: self.span_of(lhs).start,
                    end: self.current().span.start,
                };
                self.push_with_data(
                    NodeKind::IndexExpr,
                    span,
                    vec![lhs, index],
                    NodeData::IndexExpr,
                )
                .map_err(|_| arena_err(span))
            }
            TokenKind::Dot | TokenKind::Arrow => {
                // Member access
                self.advance();
                let op_text = self.text_slice(tok.span);
                let member_tok = self.advance(); // consume the member name
                let member = self
                    .push_with_data(
                        NodeKind::Identifier,
                        member_tok.span,
                        vec![],
                        NodeData::Identifier {
                            name: self.text_slice(member_tok.span),
                        },
                    )
                    .map_err(|_| arena_err(member_tok.span))?;
                let span = Span {
                    start: self.span_of(lhs).start,
                    end: member_tok.span.end,
                };
                self.push_with_data(
                    NodeKind::MemberExpr,
                    span,
                    vec![lhs, member],
                    NodeData::MemberExpr { op: op_text },
                )
                .map_err(|_| arena_err(span))
            }
            _ => Ok(lhs),
        }
    }

    /// Parse an infix binary operator (including `=` and `?:`).
    fn parse_infix(&mut self, lhs: NodeId, min_rbp: BP) -> Result<NodeId, ParseError> {
        let op_tok = self.advance();
        let op_kind = op_tok.kind;

        // Special: ternary `?:`
        if op_kind == TokenKind::Question {
            let true_expr = self.parse_expression()?;
            if self.check(TokenKind::Colon) {
                self.advance();
            }
            let false_expr = self.parse_expr_with_bp(left_bp(BP_TERNARY))?;
            let span = Span {
                start: self.span_of(lhs).start,
                end: self.span_of(false_expr).end,
            };
            return self
                .push_with_data(
                    NodeKind::TernaryExpr,
                    span,
                    vec![lhs, true_expr, false_expr],
                    NodeData::TernaryExpr,
                )
                .map_err(|_| arena_err(span));
        }

        // Regular binary operator (including assignments)
        let rhs = self.parse_expr_with_bp(min_rbp)?;
        let op_text = self.text_slice(op_tok.span);

        let span = Span {
            start: self.span_of(lhs).start,
            end: self.span_of(rhs).end,
        };

        if is_assignment(op_kind) {
            self.push_with_data(
                NodeKind::AssignmentExpr,
                span,
                vec![lhs, rhs],
                NodeData::AssignmentExpr { op: op_text },
            )
            .map_err(|_| arena_err(span))
        } else {
            self.push_with_data(
                NodeKind::BinaryExpr,
                span,
                vec![lhs, rhs],
                NodeData::BinaryExpr { op: op_text },
            )
            .map_err(|_| arena_err(span))
        }
    }

    /// Parse an assignment expression (or anything up to comma).
    /// Used for argument parsing so that `foo(a = 1)` works correctly,
    /// and commas between args are not consumed as comma operators.
    pub fn parse_assignment_or_expr(&mut self) -> Result<NodeId, ParseError> {
        // Parse with binding power above comma (BP_COMMA*2 = 2) so that
        // comma is never consumed as a binary operator within an argument.
        self.parse_expr_with_bp(left_bp(BP_COMMA) + 1)
    }

    // ------------------------------------------------------------------
    // Helpers (shared with phase_a via Parser)
    // ------------------------------------------------------------------

    /// Get the span of a node in the arena.
    pub fn span_of(&self, id: NodeId) -> Span {
        self.arena.get(id).span
    }

    /// Check if a token kind is a C type keyword.
    pub fn is_type_keyword(&self, kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Int
                | TokenKind::Char
                | TokenKind::Short
                | TokenKind::Long
                | TokenKind::Float
                | TokenKind::Double
                | TokenKind::Void
                | TokenKind::Signed
                | TokenKind::Unsigned
                | TokenKind::Struct
                | TokenKind::Union
                | TokenKind::Enum
                | TokenKind::C99Bool
                | TokenKind::C99Complex
                | TokenKind::C99Imaginary
        )
    }

    /// Parse a type specifier (type name without the declarator).
    /// Returns the NodeId of the TypeSpecifier node.
    pub fn parse_type_specifier(&mut self) -> Result<NodeId, ParseError> {
        let start_tok = self.advance();
        let start = start_tok.span.start;
        let kind = start_tok.kind;

        // Handle struct/union/enum specially.
        if matches!(kind, TokenKind::Struct | TokenKind::Union | TokenKind::Enum) {
            return self.parse_struct_union_enum_type(start);
        }

        // Simple type (int, char, etc.) — consume keywords like
        // `unsigned long long`, etc.
        let mut end = start_tok.span.end;
        loop {
            let next_kind = self.current_kind();
            if !self.is_type_keyword(next_kind) {
                break;
            }
            let t = self.advance();
            end = t.span.end;
        }

        let span = Span { start, end };
        self.push_structural(NodeKind::TypeSpecifier, span, vec![])
            .map_err(|_| arena_err(span))
    }

    /// Parse `struct/union/enum [identifier] { ... }` or `struct/union/enum identifier`
    /// and return the TypeSpecifier node id.
    pub fn parse_struct_union_enum_type(&mut self, start: u32) -> Result<NodeId, ParseError> {
        // Optional tag name
        let tag_span = if self.current_kind() == TokenKind::Identifier {
            let t = self.advance();
            Some(t.span)
        } else {
            None
        };

        let end;

        // Check for body `{ ... }`
        if self.check(TokenKind::LBrace) {
            self.advance(); // `{`
            let mut depth = 1;
            let mut body_end = self.current().span.start;
            while depth > 0 {
                match self.current_kind() {
                    TokenKind::LBrace => depth += 1,
                    TokenKind::RBrace => depth -= 1,
                    TokenKind::Eof => break,
                    _ => {}
                }
                body_end = self.current().span.end;
                self.advance();
            }
            end = body_end;
        } else {
            end = tag_span.map(|s| s.end).unwrap_or(start);
        }

        let span = Span { start, end };
        self.push_structural(NodeKind::TypeSpecifier, span, vec![])
            .map_err(|_| arena_err(span))
    }

    /// Consume the current token. Alias for `advance`.
    pub fn bump(&mut self) -> crate::lexer::token::Token {
        self.advance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::token::{CDialect, PreprocessingMode};

    fn parse_expr(source: &[u8]) -> (Parser<'_>, NodeId) {
        let mut p = Parser::new(
            source,
            CDialect::C11,
            PreprocessingMode::Preprocessed,
            false,
        );
        let id = p.parse_expression().unwrap();
        (p, id)
    }

    #[test]
    fn parse_integer_literal() {
        let (p, id) = parse_expr(b"42");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::IntegerLiteral);
    }

    #[test]
    fn parse_identifier() {
        let (p, id) = parse_expr(b"foo");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::Identifier);
    }

    #[test]
    fn parse_simple_binary() {
        let (p, id) = parse_expr(b"a + b");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::BinaryExpr);
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn parse_precedence_multiply_before_add() {
        let (p, id) = parse_expr(b"a + b * c");
        let node = p.arena.get(id);
        // Should be: a + (b * c)
        assert_eq!(node.kind, NodeKind::BinaryExpr);
        let lhs = p.arena.get(node.children[0]);
        assert_eq!(lhs.kind, NodeKind::Identifier);
        let rhs = p.arena.get(node.children[1]);
        assert_eq!(rhs.kind, NodeKind::BinaryExpr); // b * c
    }

    #[test]
    fn parse_assignment() {
        let (p, id) = parse_expr(b"x = 42");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::AssignmentExpr);
    }

    #[test]
    fn parse_function_call() {
        let (p, id) = parse_expr(b"foo(1, 2)");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::CallExpr);
        assert_eq!(node.children.len(), 3);
    }

    #[test]
    fn parse_array_subscript() {
        let (p, id) = parse_expr(b"a[0]");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::IndexExpr);
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn parse_member_access() {
        let (p, id) = parse_expr(b"s.x");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::MemberExpr);
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn parse_arrow_access() {
        let (p, id) = parse_expr(b"p->x");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::MemberExpr);
    }

    #[test]
    fn parse_ternary() {
        let (p, id) = parse_expr(b"a ? b : c");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::TernaryExpr);
        assert_eq!(node.children.len(), 3);
    }

    #[test]
    fn parse_logical_and_or() {
        let (p, id) = parse_expr(b"a && b || c");
        let node = p.arena.get(id);
        // || is lower precedence, so: (a && b) || c
        assert_eq!(node.kind, NodeKind::BinaryExpr);
        let lhs = p.arena.get(node.children[0]);
        assert_eq!(lhs.kind, NodeKind::BinaryExpr); // a && b
    }

    #[test]
    fn parse_unary_prefix() {
        let (p, id) = parse_expr(b"!x");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::UnaryExpr);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn parse_unary_postfix() {
        let (p, id) = parse_expr(b"x++");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::UnaryExpr);
        if let Some(NodeData::UnaryExpr { prefix, .. }) = &node.data {
            assert!(!prefix);
        } else {
            panic!("expected UnaryExpr");
        }
    }

    #[test]
    fn parse_sizeof_expr() {
        let (p, id) = parse_expr(b"sizeof(x)");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::SizeofExpr);
    }

    #[test]
    fn parse_chained_comparisons() {
        let (p, id) = parse_expr(b"a == b != c");
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::BinaryExpr);
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn error_on_unexpected_token() {
        let mut p = Parser::new(b"}", CDialect::C11, PreprocessingMode::Preprocessed, false);
        let result = p.parse_expression();
        match result {
            Ok(_) | Err(_) => {}
        }
        assert!(!p.errors().is_empty());
    }
}
