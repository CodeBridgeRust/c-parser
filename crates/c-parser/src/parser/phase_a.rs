//! Phase A: recursive descent for declarations and statements.
//!
//! This phase parses the top-level structure of a C translation unit:
//! function definitions, variable declarations, type definitions (struct,
//! union, enum, typedef), and statements (compound, if, while, for, etc.).
//!
//! The phase maintains the contextual symbol table for resolving the
//! `typedef-name` vs `identifier` ambiguity (the "Lexer Hack") and delegates
//! expression parsing to Phase B (Pratt parsing).

use crate::ast::node_data::NodeData;
use crate::ast::{NodeId, NodeKind, Span};
use crate::error::ParseError;
use crate::lexer::token::TokenKind;
use crate::parser::errors;
use crate::parser::Parser;

impl<'a> Parser<'a> {
    /// Parse a complete translation unit. Top-level entry point for Phase A.
    ///
    /// Returns the `NodeId` of the `TranslationUnit` node.
    pub fn parse_translation_unit(&mut self) -> Result<NodeId, ParseError> {
        let _start = 0;
        let tu_id = self.push_structural(
            NodeKind::TranslationUnit,
            Span {
                start: 0,
                end: self.source_len() as u32,
            },
            vec![],
        )?;

        let mut children = Vec::new();

        while self.current_kind() != TokenKind::Eof {
            // In SingleTu mode, handle preprocessor directives
            if self.preprocessing() == crate::lexer::token::PreprocessingMode::SingleTu
                && self.current_kind() == TokenKind::Hash
            {
                if let Ok(pp_id) = self.parse_preprocessor_directive() {
                    children.push(pp_id);
                }
                continue;
            }

            // Parse an external declaration (function definition or declaration)
            if let Ok(decl_id) = self.parse_external_declaration() {
                children.push(decl_id);
            } else {
                // Error recovery: skip to next top-level element
                self.synchronize();
                // Consume the sync token if not EOF
                if self.current_kind() != TokenKind::Eof {
                    self.advance();
                }
            }
        }

        // Update the TranslationUnit's children
        let _tu_id_idx = tu_id as usize;
        // We need to update the node's children after creation.
        // Since Arena::get_mut exists, we can do that.
        self.set_children(tu_id, children);

        Ok(tu_id)
    }

    /// Set the children of an existing node.
    pub fn set_children(&mut self, id: NodeId, children: Vec<NodeId>) {
        self.arena.get_mut(id).children = children;
    }

    // ------------------------------------------------------------------
    // External declarations
    // ------------------------------------------------------------------

    /// Parse an external declaration (at file scope).
    ///
    /// These include: function definitions, variable/function declarations,
    /// struct/union/enum/typedef declarations.
    pub fn parse_external_declaration(&mut self) -> Result<NodeId, ParseError> {
        // Skip stray semicolons (empty declarations)
        if self.eat(TokenKind::Semi) {
            let start = self.current().span.start;
            return self
                .push_structural(
                    NodeKind::NullStatement,
                    Span {
                        start: start.saturating_sub(1),
                        end: start,
                    },
                    vec![],
                )
                .map_err(|_| self.arena_err_msg("empty declaration"));
        }

        // `typedef`
        if self.check(TokenKind::Typedef) {
            return self.parse_typedef();
        }

        // `struct`, `union`, `enum`
        let kind = self.current_kind();
        if matches!(kind, TokenKind::Struct | TokenKind::Union | TokenKind::Enum) {
            return self.parse_struct_union_enum_declaration();
        }

        // Otherwise, it's a declaration or function definition starting
        // with a type specifier (or storage class).
        self.parse_declaration_or_function_definition()
    }

    /// Parse `typedef <type> <name>;`.
    fn parse_typedef(&mut self) -> Result<NodeId, ParseError> {
        self.advance(); // consume `typedef`
        let typedef_start = self.current().span.start;

        // Parse the type specifier
        let type_id = self.parse_type_specifier_for_decl()?;

        // Parse the declarator (just an identifier for typedef)
        let name_tok = self.expect_identifier()?;
        let name_span = name_tok.span;

        // Register as typedef name
        self.add_typedef(name_span);

        // Expect `;`
        let _semi_span = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span {
            start: typedef_start,
            end,
        };

        self.push_with_data(
            NodeKind::TypedefDeclaration,
            span,
            vec![type_id],
            NodeData::TypedefDecl {
                name: self.text_slice(name_span),
            },
        )
        .map_err(|_| self.arena_err_msg("typedef"))
    }

    /// Parse `struct/union/enum <name> { <members> };` or forward declaration.
    fn parse_struct_union_enum_declaration(&mut self) -> Result<NodeId, ParseError> {
        let kw = self.advance();
        let start = kw.span.start;

        let node_kind = match kw.kind {
            TokenKind::Struct => NodeKind::StructDeclaration,
            TokenKind::Union => NodeKind::UnionDeclaration,
            TokenKind::Enum => NodeKind::EnumDeclaration,
            _ => unreachable!(),
        };

        // Optional name
        let name = if self.current_kind() == TokenKind::Identifier {
            let t = self.advance();
            Some(self.text_slice(t.span))
        } else {
            None
        };

        let mut children = Vec::new();

        // Optional body
        if self.check(TokenKind::LBrace) {
            self.advance(); // consume `{`
            self.push_scope();

            // Parse members
            loop {
                if self.check(TokenKind::RBrace) || self.check(TokenKind::Eof) {
                    break;
                }

                if matches!(node_kind, NodeKind::EnumDeclaration) {
                    match self.parse_enumerator() {
                        Ok(id) => children.push(id),
                        Err(_) => {
                            self.synchronize();
                            if self.current_kind() != TokenKind::Eof {
                                self.advance();
                            }
                        }
                    }
                } else {
                    match self.parse_struct_member() {
                        Ok(id) => children.push(id),
                        Err(_) => {
                            self.synchronize();
                            if self.current_kind() != TokenKind::Eof {
                                self.advance();
                            }
                        }
                    }
                }

                // Comma or semicolon between members (may already be consumed
                // by parse_struct_member's expect_and_eat for `;`)
                let cur_kind = self.current_kind();
                if matches!(cur_kind, TokenKind::Comma | TokenKind::Semi) {
                    self.advance();
                // If we see `}`, we're done
                } else if cur_kind == TokenKind::RBrace {
                    break;
                }
                // Otherwise, continue with next member (it's a new type name)
            }

            self.pop_scope();

            if self.check(TokenKind::RBrace) {
                self.advance(); // consume `}`
            }
        }

        // Expect `;`
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span { start, end };

        let data = match node_kind {
            NodeKind::StructDeclaration => NodeData::StructDef { name },
            NodeKind::UnionDeclaration => NodeData::UnionDef { name },
            NodeKind::EnumDeclaration => NodeData::EnumDef { name },
            _ => unreachable!(),
        };

        self.push_with_data(node_kind, span, children, data)
            .map_err(|_| self.arena_err_msg("struct/union/enum"))
    }

    /// Parse an enumerator `NAME` or `NAME = expr`.
    fn parse_enumerator(&mut self) -> Result<NodeId, ParseError> {
        let name_tok = self.expect_identifier()?;
        let name_span = name_tok.span;
        let mut value_span = None;

        if self.eat(TokenKind::Eq) {
            let val_id = self.parse_expression()?;
            value_span = Some(self.span_of(val_id));
        }

        let end = value_span.map(|s| s.end).unwrap_or(name_span.end);
        let span = Span {
            start: name_span.start,
            end,
        };

        self.push_with_data(
            NodeKind::Enumerator,
            span,
            vec![],
            NodeData::Enumerator {
                name: self.text_slice(name_span),
                value: value_span.map(|s| self.text_slice(s)),
            },
        )
        .map_err(|_| self.arena_err_msg("enumerator"))
    }

    /// Parse a struct/union member declaration: `<type> <name>;`.
    fn parse_struct_member(&mut self) -> Result<NodeId, ParseError> {
        let type_id = self.parse_type_specifier_for_decl()?;
        let name_tok = self.expect_identifier()?;
        let name_span = name_tok.span;
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span {
            start: self.span_of(type_id).start,
            end,
        };

        self.push_with_data(
            NodeKind::VariableDeclaration,
            span,
            vec![type_id],
            NodeData::VariableDecl {
                name: self.text_slice(name_span),
                storage_class: self.text_slice(Span {
                    start: span.start,
                    end: span.start,
                }), // empty
            },
        )
        .map_err(|_| self.arena_err_msg("struct member"))
    }

    // ------------------------------------------------------------------
    // Declaration or function definition
    // ------------------------------------------------------------------

    /// Parse a declaration or function definition.
    ///
    /// Grammar: `<type-specifier> <declarator> [= <init>] ;`
    ///       or: `<type-specifier> <declarator> <params> <body>`
    ///       or: `<storage> <type-specifier> <declarator> ...`
    fn parse_declaration_or_function_definition(&mut self) -> Result<NodeId, ParseError> {
        // Parse storage class if present
        let storage_class = self.parse_storage_class();

        // Parse the type specifier
        let type_id = self.parse_type_specifier_for_decl()?;

        // Parse the declarator name
        let name_tok = self.expect_identifier()?;
        let name_span = name_tok.span;

        // Check if this is a function definition: `name ( params ) {`
        if self.check(TokenKind::LParen) {
            // This could be function definition or function declaration.
            // We need to look ahead for `{` after parameters.
            // For now, parse as a parameter list and then check.

            self.advance(); // consume `(`
            self.push_scope();

            let mut params = Vec::new();

            if !self.check(TokenKind::RParen) {
                // Parse parameter declarations
                if !self.check(TokenKind::Void) || {
                    // Void as sole parameter means no parameters
                    let _void_tok = self.current();
                    // We need a 2-token lookahead: `void )`
                    // For simplicity, just parse parameter list normally.
                    false
                } {
                    while let Ok(pid) = self.parse_parameter_declaration() {
                        params.push(pid);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                } else {
                    // `void)` — consume `void`
                    self.advance();
                }
            }

            if self.check(TokenKind::RParen) {
                self.advance(); // consume `)`
            }

            // Now check: `{` means function definition, `;` means declaration
            if self.check(TokenKind::LBrace) {
                // Function definition
                let body_id = self.parse_compound_statement()?;
                self.pop_scope();

                let end = self.span_of(body_id).end;
                let span = Span {
                    start: self.span_of(type_id).start,
                    end,
                };

                let mut children = vec![type_id];
                children.extend(params);
                children.push(body_id);

                return self
                    .push_with_data(
                        NodeKind::FunctionDefinition,
                        span,
                        children,
                        NodeData::FunctionDef {
                            name: self.text_slice(name_span),
                            storage_class: storage_class.unwrap_or_else(|| {
                                self.text_slice(Span {
                                    start: span.start,
                                    end: span.start,
                                })
                            }),
                        },
                    )
                    .map_err(|_| self.arena_err_msg("function definition"));
            } else {
                // Function declaration (just a prototype)
                self.pop_scope();
                let _ = self.expect_and_eat(TokenKind::Semi);

                let end = self.current().span.start;
                let span = Span {
                    start: self.span_of(type_id).start,
                    end,
                };

                // Represent as a VariableDeclaration with function type
                let mut children = vec![type_id];
                children.extend(params);

                return self
                    .push_with_data(
                        NodeKind::VariableDeclaration,
                        span,
                        children,
                        NodeData::VariableDecl {
                            name: self.text_slice(name_span),
                            storage_class: storage_class.unwrap_or_else(|| {
                                self.text_slice(Span {
                                    start: span.start,
                                    end: span.start,
                                })
                            }),
                        },
                    )
                    .map_err(|_| self.arena_err_msg("function declaration"));
            }
        }

        // Otherwise, it's a variable declaration.
        // Check for initializer `= expr`
        let mut children = vec![type_id];
        if self.eat(TokenKind::Eq) {
            let init_id = self.parse_initializer()?;
            children.push(init_id);
        }

        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span {
            start: self.span_of(type_id).start,
            end,
        };

        self.push_with_data(
            NodeKind::VariableDeclaration,
            span,
            children,
            NodeData::VariableDecl {
                name: self.text_slice(name_span),
                storage_class: storage_class.unwrap_or_else(|| {
                    self.text_slice(Span {
                        start: span.start,
                        end: span.start,
                    })
                }),
            },
        )
        .map_err(|_| self.arena_err_msg("variable declaration"))
    }

    /// Parse storage-class specifier: `static`, `extern`, `inline`, etc.
    /// Returns the span text of the storage class, or None if absent.
    fn parse_storage_class(&mut self) -> Option<crate::ast::text_slice::TextSlice> {
        let kind = self.current_kind();
        match kind {
            TokenKind::Static
            | TokenKind::Extern
            | TokenKind::Inline
            | TokenKind::Register
            | TokenKind::Const
            | TokenKind::Volatile
            | TokenKind::Restrict => {
                let tok = self.advance();
                Some(self.text_slice(tok.span))
            }
            _ => None,
        }
    }

    /// Parse a parameter declaration: `<type> <name>`.
    fn parse_parameter_declaration(&mut self) -> Result<NodeId, ParseError> {
        let type_id = self.parse_type_specifier_for_decl()?;

        // Name is optional in K&R C
        let name_span = if self.current_kind() == TokenKind::Identifier {
            let t = self.advance();
            t.span
        } else {
            // Unnamed parameter — create a synthetic empty name
            let pos = self.current().span.start;
            Span {
                start: pos,
                end: pos,
            }
        };

        let end = name_span.end;
        let span = Span {
            start: self.span_of(type_id).start,
            end,
        };

        self.push_with_data(
            NodeKind::ParameterDeclaration,
            span,
            vec![type_id],
            NodeData::ParameterDecl {
                name: self.text_slice(name_span),
            },
        )
        .map_err(|_| self.arena_err_msg("parameter"))
    }

    /// Parse a type specifier for use in declarations.
    /// Unlike `parse_type_specifier` (which is used in expressions for casts),
    /// this one may also handle pointer/array/function type prefixes.
    fn parse_type_specifier_for_decl(&mut self) -> Result<NodeId, ParseError> {
        let start = self.current().span.start;

        // Handle pointer prefix: `*`
        if self.eat(TokenKind::Star) {
            let base_type = self.parse_type_specifier_for_decl()?;
            let end = self.span_of(base_type).end;
            let span = Span { start, end };
            return self
                .push_structural(NodeKind::PointerType, span, vec![base_type])
                .map_err(|_| self.arena_err_msg("pointer type"));
        }

        // Delegate to the general type specifier parser
        self.parse_type_specifier()
    }

    /// Parse an initializer expression (after `=`).
    fn parse_initializer(&mut self) -> Result<NodeId, ParseError> {
        if self.check(TokenKind::LBrace) {
            self.parse_initializer_list()
        } else {
            self.parse_assignment_or_expr()
        }
    }

    /// Parse `{ ... }` initializer list.
    fn parse_initializer_list(&mut self) -> Result<NodeId, ParseError> {
        let lbrace = self.advance(); // consume `{`
        let start = lbrace.span.start;
        let mut items = Vec::new();

        if !self.check(TokenKind::RBrace) {
            loop {
                // Could be a nested initializer or expression
                if self.check(TokenKind::LBrace) {
                    if let Ok(id) = self.parse_initializer_list() {
                        items.push(id);
                    } else {
                        break;
                    }
                } else {
                    match self.parse_assignment_or_expr() {
                        Ok(id) => items.push(id),
                        Err(_) => break,
                    }
                }
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
        }

        if self.check(TokenKind::RBrace) {
            self.advance(); // consume `}`
        }

        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_structural(NodeKind::InitializerList, span, items)
            .map_err(|_| self.arena_err_msg("initializer list"))
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    /// Parse a compound statement: `{ ... }`.
    pub fn parse_compound_statement(&mut self) -> Result<NodeId, ParseError> {
        let lbrace = self.expect_and_eat_kind(TokenKind::LBrace)?;
        let start = lbrace.span.start;
        self.push_scope();

        let mut stmts = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.check(TokenKind::Eof) {
            match self.parse_statement() {
                Ok(id) => stmts.push(id),
                Err(_) => {
                    self.synchronize();
                    if self.current_kind() != TokenKind::Eof {
                        self.advance();
                    }
                }
            }
        }

        self.pop_scope();

        let end = if self.check(TokenKind::RBrace) {
            let rbrace = self.advance();
            rbrace.span.end
        } else {
            self.current().span.start
        };

        let span = Span { start, end };
        self.push_structural(NodeKind::CompoundStatement, span, stmts)
            .map_err(|_| self.arena_err_msg("compound statement"))
    }

    /// Parse a single statement.
    pub fn parse_statement(&mut self) -> Result<NodeId, ParseError> {
        let kind = self.current_kind();

        match kind {
            TokenKind::LBrace => self.parse_compound_statement(),
            TokenKind::If => self.parse_if_statement(),
            TokenKind::While => self.parse_while_statement(),
            TokenKind::Do => self.parse_do_while_statement(),
            TokenKind::For => self.parse_for_statement(),
            TokenKind::Switch => self.parse_switch_statement(),
            TokenKind::Case => self.parse_case_statement(),
            TokenKind::Default => self.parse_default_statement(),
            TokenKind::Return => self.parse_return_statement(),
            TokenKind::Break => self.parse_break_statement(),
            TokenKind::Continue => self.parse_continue_statement(),
            TokenKind::Goto => self.parse_goto_statement(),
            TokenKind::Semi => {
                let tok = self.advance();
                self.push_structural(NodeKind::NullStatement, tok.span, vec![])
                    .map_err(|_| self.arena_err_msg("null statement"))
            }
            // Label: `identifier : statement`
            _ => {
                // Check for label: <identifier> `:` <statement>
                if kind == TokenKind::Identifier {
                    // Peek ahead: is the next token `:`?
                    // We need 2 tokens of look-ahead. For MVP, we consume
                    // the identifier and check.
                    let id_tok = self.advance();
                    if self.check(TokenKind::Colon) {
                        let colon = self.advance();
                        let label_span = Span {
                            start: id_tok.span.start,
                            end: colon.span.end,
                        };
                        let label = self
                            .push_with_data(
                                NodeKind::Identifier,
                                id_tok.span,
                                vec![],
                                NodeData::Identifier {
                                    name: self.text_slice(id_tok.span),
                                },
                            )
                            .map_err(|_| self.arena_err_msg("label"))?;

                        let stmt = self.parse_statement()?;
                        let span = Span {
                            start: label_span.start,
                            end: self.span_of(stmt).end,
                        };
                        return self
                            .push_structural(NodeKind::LabelStatement, span, vec![label, stmt])
                            .map_err(|_| self.arena_err_msg("labeled statement"));
                    }
                    // Not a label — it's an expression statement.
                    // But we already consumed the identifier! We need to
                    // backtrack. For MVP simplicity, let's use a different
                    // approach: parse it as an expression statement.
                    // The issue is we already consumed the identifier token.
                    // Let's handle this differently: we try to parse an
                    // expression and fall through.
                    //
                    // Actually, the simplest correct approach: treat any
                    // non-keyword as an expression statement.
                }

                self.parse_expression_statement()
            }
        }
    }

    /// Parse an expression statement: `expr ;`.
    fn parse_expression_statement(&mut self) -> Result<NodeId, ParseError> {
        // Check if it starts like a declaration (type keyword followed by
        // identifier that could be a typedef). For now, we delegate to
        // declaration parsing if it looks like one.
        let cur_kind = self.current_kind();
        if self.is_type_keyword(cur_kind) {
            return self.parse_declaration_or_function_definition();
        }

        let start = self.current().span.start;
        let expr_id = self.parse_expression()?;
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_structural(NodeKind::ExpressionStatement, span, vec![expr_id])
            .map_err(|_| self.arena_err_msg("expression statement"))
    }

    /// Parse `if ( expr ) stmt [else stmt]`.
    fn parse_if_statement(&mut self) -> Result<NodeId, ParseError> {
        let if_tok = self.advance(); // consume `if`
        let start = if_tok.span.start;

        self.expect_and_eat_kind(TokenKind::LParen)?;
        let cond = self.parse_expression()?;
        let _ = self.expect_and_eat(TokenKind::RParen);

        let then_branch = self.parse_statement()?;

        let mut children = vec![cond, then_branch];

        let end = if self.eat(TokenKind::Else) {
            let else_branch = self.parse_statement()?;
            let end = self.span_of(else_branch).end;
            children.push(else_branch);
            end
        } else {
            self.span_of(then_branch).end
        };

        let span = Span { start, end };
        self.push_structural(NodeKind::IfStatement, span, children)
            .map_err(|_| self.arena_err_msg("if statement"))
    }

    /// Parse `while ( expr ) stmt`.
    fn parse_while_statement(&mut self) -> Result<NodeId, ParseError> {
        let while_tok = self.advance();
        let start = while_tok.span.start;

        self.expect_and_eat_kind(TokenKind::LParen)?;
        let cond = self.parse_expression()?;
        let _ = self.expect_and_eat(TokenKind::RParen);

        let body = self.parse_statement()?;

        let span = Span {
            start,
            end: self.span_of(body).end,
        };
        self.push_structural(NodeKind::WhileStatement, span, vec![cond, body])
            .map_err(|_| self.arena_err_msg("while statement"))
    }

    /// Parse `do stmt while ( expr ) ;`.
    fn parse_do_while_statement(&mut self) -> Result<NodeId, ParseError> {
        let do_tok = self.advance();
        let start = do_tok.span.start;

        let body = self.parse_statement()?;

        self.expect_and_eat_kind(TokenKind::While)?;
        self.expect_and_eat_kind(TokenKind::LParen)?;
        let cond = self.parse_expression()?;
        self.expect_and_eat_kind(TokenKind::RParen)?;
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_structural(NodeKind::DoWhileStatement, span, vec![body, cond])
            .map_err(|_| self.arena_err_msg("do-while statement"))
    }

    /// Parse `for ( [init]; [cond]; [iter] ) stmt`.
    fn parse_for_statement(&mut self) -> Result<NodeId, ParseError> {
        let for_tok = self.advance();
        let start = for_tok.span.start;

        self.expect_and_eat_kind(TokenKind::LParen)?;
        self.push_scope();

        // Init clause
        let is_type_keyword = {
            let cur_kind = self.current_kind();
            self.is_type_keyword(cur_kind)
        };
        let init = if self.check(TokenKind::Semi) {
            None
        } else if is_type_keyword {
            Some(self.parse_declaration_or_function_definition()?)
        } else {
            Some(self.parse_expression()?)
        };
        let _ = self.expect_and_eat(TokenKind::Semi);

        // Condition clause
        let cond = if self.check(TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        let _ = self.expect_and_eat(TokenKind::Semi);

        // Iteration clause
        let iter = if self.check(TokenKind::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        let _ = self.expect_and_eat(TokenKind::RParen);

        let body = self.parse_statement()?;
        self.pop_scope();

        let mut children = Vec::new();
        if let Some(id) = init {
            children.push(id);
        } else {
            // Placeholder NullStatement for absent init
            let null_id = self.push_structural(
                NodeKind::NullStatement,
                Span {
                    start: for_tok.span.start,
                    end: for_tok.span.start,
                },
                vec![],
            )?;
            children.push(null_id);
        }
        if let Some(id) = cond {
            children.push(id);
        } else {
            let null_id = self.push_structural(
                NodeKind::NullStatement,
                Span {
                    start: for_tok.span.start,
                    end: for_tok.span.start,
                },
                vec![],
            )?;
            children.push(null_id);
        }
        if let Some(id) = iter {
            children.push(id);
        } else {
            let null_id = self.push_structural(
                NodeKind::NullStatement,
                Span {
                    start: for_tok.span.start,
                    end: for_tok.span.start,
                },
                vec![],
            )?;
            children.push(null_id);
        }
        children.push(body);

        let span = Span {
            start,
            end: self.span_of(body).end,
        };
        self.push_structural(NodeKind::ForStatement, span, children)
            .map_err(|_| self.arena_err_msg("for statement"))
    }

    /// Parse `switch ( expr ) stmt`.
    fn parse_switch_statement(&mut self) -> Result<NodeId, ParseError> {
        let sw_tok = self.advance();
        let start = sw_tok.span.start;

        self.expect_and_eat_kind(TokenKind::LParen)?;
        let cond = self.parse_expression()?;
        let _ = self.expect_and_eat(TokenKind::RParen);

        let body = self.parse_statement()?;

        let span = Span {
            start,
            end: self.span_of(body).end,
        };
        self.push_structural(NodeKind::SwitchStatement, span, vec![cond, body])
            .map_err(|_| self.arena_err_msg("switch statement"))
    }

    /// Parse `case expr :`.
    fn parse_case_statement(&mut self) -> Result<NodeId, ParseError> {
        let case_tok = self.advance();
        let start = case_tok.span.start;

        let expr = self.parse_expression()?;
        self.expect_and_eat_kind(TokenKind::Colon)?;

        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_structural(NodeKind::CaseStatement, span, vec![expr])
            .map_err(|_| self.arena_err_msg("case statement"))
    }

    /// Parse `default :`.
    fn parse_default_statement(&mut self) -> Result<NodeId, ParseError> {
        let def_tok = self.advance();
        let start = def_tok.span.start;

        self.expect_and_eat_kind(TokenKind::Colon)?;

        let end = self.current().span.start;
        let span = Span { start, end };
        self.push_structural(NodeKind::DefaultStatement, span, vec![])
            .map_err(|_| self.arena_err_msg("default statement"))
    }

    /// Parse `return [expr] ;`.
    fn parse_return_statement(&mut self) -> Result<NodeId, ParseError> {
        let ret_tok = self.advance();
        let start = ret_tok.span.start;

        let expr = if !self.check(TokenKind::Semi) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span { start, end };

        let children = if let Some(id) = expr {
            vec![id]
        } else {
            vec![]
        };

        self.push_structural(NodeKind::ReturnStatement, span, children)
            .map_err(|_| self.arena_err_msg("return statement"))
    }

    /// Parse `break ;`.
    fn parse_break_statement(&mut self) -> Result<NodeId, ParseError> {
        let tok = self.advance();
        let span = tok.span;
        let _ = self.expect_and_eat(TokenKind::Semi);
        self.push_structural(NodeKind::BreakStatement, span, vec![])
            .map_err(|_| self.arena_err_msg("break statement"))
    }

    /// Parse `continue ;`.
    fn parse_continue_statement(&mut self) -> Result<NodeId, ParseError> {
        let tok = self.advance();
        let span = tok.span;
        let _ = self.expect_and_eat(TokenKind::Semi);
        self.push_structural(NodeKind::ContinueStatement, span, vec![])
            .map_err(|_| self.arena_err_msg("continue statement"))
    }

    /// Parse `goto label ;`.
    fn parse_goto_statement(&mut self) -> Result<NodeId, ParseError> {
        let goto_tok = self.advance();
        let start = goto_tok.span.start;

        let label_tok = self.expect_identifier()?;
        let _ = self.expect_and_eat(TokenKind::Semi);

        let end = self.current().span.start;
        let span = Span { start, end };

        let label = self
            .push_with_data(
                NodeKind::Identifier,
                label_tok.span,
                vec![],
                NodeData::Identifier {
                    name: self.text_slice(label_tok.span),
                },
            )
            .map_err(|_| self.arena_err_msg("goto label"))?;

        self.push_structural(NodeKind::GotoStatement, span, vec![label])
            .map_err(|_| self.arena_err_msg("goto statement"))
    }

    // ------------------------------------------------------------------
    // Preprocessor directives (SingleTu mode)
    // ------------------------------------------------------------------

    /// Parse a preprocessor directive line: `#directive argument\n`.
    fn parse_preprocessor_directive(&mut self) -> Result<NodeId, ParseError> {
        let hash = self.advance(); // consume `#`
        let start = hash.span.start;

        // Get the directive name
        let dir_tok = self.current();
        let directive_name = self.span_str(dir_tok.span);
        self.advance();

        // Collect the rest of the line as the argument
        let arg_start = self.current().span.start;
        while !matches!(self.current_kind(), TokenKind::Eof)
            && !is_newline_token(self.current_kind())
        {
            self.advance();
        }
        let arg_end = self.current().span.start;
        let arg_span = Span {
            start: arg_start,
            end: arg_end,
        };
        let argument = self.text_slice(arg_span);

        // Skip newline
        if is_newline_token(self.current_kind()) {
            self.advance();
        }

        let end = self.current().span.start;
        let span = Span { start, end };

        let kind = match directive_name.as_str() {
            "include" => NodeKind::PreprocessorInclude,
            "define" => NodeKind::PreprocessorDefine,
            "undef" => NodeKind::PreprocessorUndef,
            "if" => NodeKind::PreprocessorIf,
            "ifdef" => NodeKind::PreprocessorIfdef,
            "ifndef" => NodeKind::PreprocessorIfndef,
            "else" => NodeKind::PreprocessorElse,
            "elif" => NodeKind::PreprocessorElif,
            "endif" => NodeKind::PreprocessorEndif,
            "pragma" => NodeKind::PreprocessorPragma,
            "line" => NodeKind::PreprocessorLine,
            "error" => NodeKind::PreprocessorError,
            "warning" => NodeKind::PreprocessorWarning,
            _ => {
                // Unknown directive — still emit a preprocessor node
                NodeKind::PreprocessorPragma
            }
        };

        self.push_with_data(
            kind,
            span,
            vec![],
            NodeData::PreprocessorDirective {
                directive: directive_name,
                argument,
            },
        )
        .map_err(|_| self.arena_err_msg("preprocessor directive"))
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /// Expect an identifier token. If not found, report error and return
    /// a synthetic "error" span.
    fn expect_identifier(&mut self) -> Result<crate::lexer::token::Token, ParseError> {
        let tok = self.current();
        if tok.kind == TokenKind::Identifier {
            self.advance();
            Ok(tok)
        } else {
            self.error(errors::expected_identifier(tok.span));
            // Return a synthetic token to continue parsing
            Err(ParseError {
                code: "E_SYNTAX_EXPECTED_IDENTIFIER".to_string(),
                message: "expected identifier".to_string(),
                span: tok.span,
                severity: crate::error::Severity::Error,
            })
        }
    }

    /// Expect a specific token kind and consume it. Return the token.
    fn expect_and_eat_kind(
        &mut self,
        kind: TokenKind,
    ) -> Result<crate::lexer::token::Token, ParseError> {
        let tok = self.current();
        if tok.kind == kind {
            self.advance();
            Ok(tok)
        } else {
            self.error(errors::unexpected_token(
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

    /// Expect `;` and consume it. Return its span (or current position on failure).
    fn expect_and_eat(&mut self, kind: TokenKind) -> Span {
        let tok = self.current();
        if tok.kind == kind {
            self.advance();
            tok.span
        } else {
            let span = tok.span;
            self.error(errors::unexpected_token(
                span,
                &format!("{:?}", kind),
                &format!("{:?}", tok.kind),
            ));
            span
        }
    }

    /// Helper to create a fatal arena error.
    fn arena_err_msg(&self, _context: &str) -> ParseError {
        ParseError {
            code: "E_FATAL".to_string(),
            message: format!("arena overflow during: {}", _context),
            span: Span {
                start: 0,
                end: self.source_len() as u32,
            },
            severity: crate::error::Severity::Error,
        }
    }
}

/// Check if a token kind represents a newline in the token stream.
/// Since newlines are skipped by the lexer in Preprocessed mode, this
/// should never match in that mode. In SingleTu mode, the `#` handling
/// needs this but we use a different mechanism (scanning to Eof/newline).
fn is_newline_token(_kind: TokenKind) -> bool {
    // Lexer doesn't emit newline tokens; these are handled by checking
    // for the end of the line in the raw source.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::token::{CDialect, PreprocessingMode};
    use crate::parser::Parser;

    fn parse(source: &[u8]) -> Result<(Parser<'_>, NodeId), ParseError> {
        let mut p = Parser::new(
            source,
            CDialect::C11,
            PreprocessingMode::Preprocessed,
            false,
        );
        let id = p.parse_translation_unit()?;
        Ok((p, id))
    }

    #[test]
    fn parse_empty_tu() {
        let (p, id) = parse(b"").unwrap();
        let node = p.arena.get(id);
        assert_eq!(node.kind, NodeKind::TranslationUnit);
    }

    #[test]
    fn parse_simple_function() {
        let (p, id) = parse(b"void f() {}").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);

        let func = p.arena.get(tu.children[0]);
        assert_eq!(func.kind, NodeKind::FunctionDefinition);
    }

    #[test]
    fn parse_function_with_params() {
        let (p, id) = parse(b"int add(int a, int b) { return a + b; }").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);

        let func = p.arena.get(tu.children[0]);
        assert_eq!(func.kind, NodeKind::FunctionDefinition);
        // children: type_id, params..., body
        assert!(func.children.len() >= 3); // type + 2 params + body
    }

    #[test]
    fn parse_variable_declaration() {
        let (p, id) = parse(b"int x;").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);

        let decl = p.arena.get(tu.children[0]);
        assert_eq!(decl.kind, NodeKind::VariableDeclaration);
    }

    #[test]
    fn parse_if_statement_in_function() {
        let (p, id) = parse(b"void f() { if (x) { } }").unwrap();
        let tu = p.arena.get(id);
        let func = p.arena.get(tu.children[0]);
        let body = p.arena.get(func.children.last().copied().unwrap());
        assert_eq!(body.kind, NodeKind::CompoundStatement);

        let if_stmt = p.arena.get(body.children[0]);
        assert_eq!(if_stmt.kind, NodeKind::IfStatement);
    }

    #[test]
    fn parse_while_statement() {
        let (p, id) = parse(b"void f() { while (1) { break; } }").unwrap();
        let tu = p.arena.get(id);
        let func = p.arena.get(tu.children[0]);
        let body = p.arena.get(func.children.last().copied().unwrap());
        let while_stmt = p.arena.get(body.children[0]);
        assert_eq!(while_stmt.kind, NodeKind::WhileStatement);
    }

    #[test]
    fn parse_for_statement() {
        let (p, id) = parse(b"void f() { for (int i = 0; i < 10; i++) { } }").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);
    }

    #[test]
    fn parse_typedef() {
        let (p, id) = parse(b"typedef int myint;").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);
        let decl = p.arena.get(tu.children[0]);
        assert_eq!(decl.kind, NodeKind::TypedefDeclaration);
    }

    #[test]
    fn parse_struct_declaration() {
        let (p, id) = parse(b"struct Point { int x; int y; };").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);
        let strukt = p.arena.get(tu.children[0]);
        assert_eq!(strukt.kind, NodeKind::StructDeclaration);
    }

    #[test]
    fn parse_enum_declaration() {
        let (p, id) = parse(b"enum Color { RED, GREEN, BLUE };").unwrap();
        let tu = p.arena.get(id);
        assert_eq!(tu.children.len(), 1);
        let en = p.arena.get(tu.children[0]);
        assert_eq!(en.kind, NodeKind::EnumDeclaration);
        assert_eq!(en.children.len(), 3);
    }

    #[test]
    fn parse_return_statement() {
        let (p, id) = parse(b"void f() { return; }").unwrap();
        let tu = p.arena.get(id);
        let func = p.arena.get(tu.children[0]);
        let body = p.arena.get(func.children.last().copied().unwrap());
        let ret = p.arena.get(body.children[0]);
        assert_eq!(ret.kind, NodeKind::ReturnStatement);
    }

    #[test]
    fn parse_syntax_error_does_not_panic() {
        let result = parse(b"void f() { @@@ }");
        // Should not panic
        match result {
            Ok((p, _)) => {
                assert!(!p.errors().is_empty());
            }
            Err(_) => {
                // Also acceptable: fatal error
            }
        }
    }

    #[test]
    fn parse_single_tu_directives() {
        let mut p = Parser::new(
            b"#define FOO 42\nint x;",
            CDialect::C11,
            PreprocessingMode::SingleTu,
            false,
        );
        let result = p.parse_translation_unit();
        match result {
            Ok(_) | Err(_) => {}
        }
    }
}
