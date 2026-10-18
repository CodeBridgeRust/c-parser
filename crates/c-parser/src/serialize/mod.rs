//! JSON serialization with lazy materialization.
//!
//! Implements the `SerializableAst<'a> { ast, source }` wrapper pattern that
//! materializes [`TextSlice`] string payloads from the source buffer by
//! `Span` only at serialization time, producing a self-contained JSON
//! document that conforms to `docs/ast-schema.json` v1.0.
//!
//! ## Architecture
//!
//! Because `serde::Serialize` cannot accept an extra `source` parameter,
//! serialization uses a chain of wrapper types that each hold both a
//! reference to part of the AST and the source buffer:
//!
//! ```text
//! SerializableAst          — the top-level JSON object
//!   └─ SerializableNodeArray — the "nodes" array
//!        └─ SerializableNode  — a single node
//!             └─ SerializableData — the "data" payload
//! ```
//!
//! At the `SerializableData` level, every [`TextSlice::Span`] is
//! materialized through `String::from_utf8_lossy` (never panics).

use crate::ast::node::Ast;
use crate::ast::node_data::NodeData;
use crate::ast::text_slice::TextSlice;
use crate::ast::Node;
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Serialize an AST to a self-contained JSON string.
///
/// `source` must be the **original** (BOM-stripped) source buffer that was
/// given to the lexer.  String fields are materialized from `source` by
/// `Span` using `String::from_utf8_lossy`.
pub fn ast_to_json(ast: &Ast, source: &[u8]) -> Result<String, serde_json::Error> {
    serde_json::to_string(&SerializableAst { ast, source })
}

/// Serialize an AST to a pretty-printed JSON string.
pub fn ast_to_pretty_json(ast: &Ast, source: &[u8]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&SerializableAst { ast, source })
}

/// Deserialize an AST from a JSON string.
///
/// The returned AST will have all [`TextSlice`] values in `Detached` form —
/// no source buffer is needed.  Span associations with the original source
/// are **not** recovered (and are not needed for downstream consumption).
pub fn ast_from_json(json: &str) -> Result<Ast, serde_json::Error> {
    let repr: crate::ast::node::AstJson = serde_json::from_str(json)?;
    Ok(Ast::from_json_repr(repr))
}

// ---------------------------------------------------------------------------
// Wrapper types
// ---------------------------------------------------------------------------

/// Top-level serializable wrapper.
///
/// Produces the JSON object `{"version": "1.0", "language": "c",
/// "source_length": ..., "root": ..., "nodes": [...], "errors": [...]}`.
pub struct SerializableAst<'a> {
    pub ast: &'a Ast,
    pub source: &'a [u8],
}

struct SerializableNodeArray<'a> {
    nodes: &'a [Node],
    source: &'a [u8],
}

struct SerializableNode<'a> {
    node: &'a Node,
    source: &'a [u8],
}

struct SerializableData<'a> {
    data: &'a NodeData,
    source: &'a [u8],
}

// ---------------------------------------------------------------------------
// SerializableAst
// ---------------------------------------------------------------------------

impl<'a> Serialize for SerializableAst<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("version", "1.0")?;
        map.serialize_entry("language", "c")?;
        map.serialize_entry("source_length", &self.ast.source_length)?;
        map.serialize_entry("root", &self.ast.root)?;
        map.serialize_entry(
            "nodes",
            &SerializableNodeArray {
                nodes: &self.ast.nodes,
                source: self.source,
            },
        )?;
        map.serialize_entry("errors", &self.ast.errors)?;
        map.end()
    }
}

// ---------------------------------------------------------------------------
// SerializableNodeArray
// ---------------------------------------------------------------------------

impl<'a> Serialize for SerializableNodeArray<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.nodes.len()))?;
        for node in self.nodes {
            seq.serialize_element(&SerializableNode {
                node,
                source: self.source,
            })?;
        }
        seq.end()
    }
}

// ---------------------------------------------------------------------------
// SerializableNode
// ---------------------------------------------------------------------------

impl<'a> Serialize for SerializableNode<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("kind", &self.node.kind)?;
        map.serialize_entry("span", &self.node.span)?;
        map.serialize_entry("children", &self.node.children)?;
        match &self.node.data {
            Some(data) => map.serialize_entry(
                "data",
                &SerializableData {
                    data,
                    source: self.source,
                },
            )?,
            // Structural nodes MUST serialize as `"data": null`
            None => map.serialize_entry("data", &Option::<()>::None)?,
        }
        map.end()
    }
}

// ---------------------------------------------------------------------------
// SerializableData — the core of lazy materialization
// ---------------------------------------------------------------------------

impl<'a> SerializableData<'a> {
    /// Materialize a `TextSlice` from the source buffer.
    fn text(&self, ts: &'a TextSlice) -> Cow<'a, str> {
        ts.materialize(self.source)
    }

    /// Materialize an optional `TextSlice`.  Returns `None` if the option
    /// is `None`, which serializes as `null`.
    fn opt_text(&self, ts: &'a Option<TextSlice>) -> Option<Cow<'a, str>> {
        ts.as_ref().map(|t| self.text(t))
    }
}

impl<'a> Serialize for SerializableData<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self.data {
            // ------- Leaf / expression payloads -------
            NodeData::Identifier { name } => {
                map.serialize_entry("type", "Identifier")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::IntegerLiteral { value, suffix } => {
                map.serialize_entry("type", "IntegerLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("suffix", &self.opt_text(suffix))?;
            }
            NodeData::FloatLiteral { value, suffix } => {
                map.serialize_entry("type", "FloatLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("suffix", &self.opt_text(suffix))?;
            }
            NodeData::StringLiteral { value, encoding } => {
                map.serialize_entry("type", "StringLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("encoding", &self.opt_text(encoding))?;
            }
            NodeData::CharLiteral { value, encoding } => {
                map.serialize_entry("type", "CharLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("encoding", &self.opt_text(encoding))?;
            }
            NodeData::BinaryExpr { op } => {
                map.serialize_entry("type", "BinaryExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::UnaryExpr { op, prefix } => {
                map.serialize_entry("type", "UnaryExpr")?;
                map.serialize_entry("op", &self.text(op))?;
                map.serialize_entry("prefix", prefix)?;
            }
            NodeData::AssignmentExpr { op } => {
                map.serialize_entry("type", "AssignmentExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::CallExpr { is_macro } => {
                map.serialize_entry("type", "CallExpr")?;
                // INTENTIONALLY skip the key when `None`:
                // the schema treats absence as "unknown / likely plain call",
                // which matches MVP behaviour (no macro detection).
                if let Some(v) = is_macro {
                    map.serialize_entry("is_macro", v)?;
                }
            }
            NodeData::MemberExpr { op } => {
                map.serialize_entry("type", "MemberExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::IndexExpr => {
                map.serialize_entry("type", "IndexExpr")?;
            }
            NodeData::CastExpr => {
                map.serialize_entry("type", "CastExpr")?;
            }
            NodeData::TernaryExpr => {
                map.serialize_entry("type", "TernaryExpr")?;
            }
            NodeData::SizeofExpr { is_variadic } => {
                map.serialize_entry("type", "SizeofExpr")?;
                map.serialize_entry("is_variadic", is_variadic)?;
            }

            // ------- Declarations (shortened discriminators) -------
            NodeData::FunctionDef {
                name,
                storage_class,
            } => {
                map.serialize_entry("type", "FunctionDef")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("storage_class", &self.text(storage_class))?;
            }
            NodeData::VariableDecl {
                name,
                storage_class,
            } => {
                map.serialize_entry("type", "VariableDecl")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("storage_class", &self.text(storage_class))?;
            }
            NodeData::ParameterDecl { name } => {
                map.serialize_entry("type", "ParameterDecl")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::TypedefDecl { name } => {
                map.serialize_entry("type", "TypedefDecl")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::StructDef { name } => {
                map.serialize_entry("type", "StructDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::UnionDef { name } => {
                map.serialize_entry("type", "UnionDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::EnumDef { name } => {
                map.serialize_entry("type", "EnumDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::Enumerator { name, value } => {
                map.serialize_entry("type", "Enumerator")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("value", &self.opt_text(value))?;
            }

            // ------- Preprocessor -------
            NodeData::PreprocessorDirective {
                directive,
                argument,
            } => {
                map.serialize_entry("type", "PreprocessorDirective")?;
                map.serialize_entry("directive", directive)?;
                // argument is ALWAYS a string (empty string for `#endif` etc.)
                map.serialize_entry("argument", &self.text(argument))?;
            }

            // ------- Error -------
            NodeData::ErrorNode { error_index } => {
                map.serialize_entry("type", "ErrorNode")?;
                map.serialize_entry("error_index", error_index)?;
            }
        }
        map.end()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::text_slice::TextSlice;
    use crate::ast::{NodeKind, Span};
    use crate::error::{ParseError, Severity};

    /// Build a minimal `Ast` for testing.
    fn make_ast() -> Ast {
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 28 }, vec![1]);
        let ident = Node::with_data(
            NodeKind::Identifier,
            Span { start: 0, end: 3 },
            vec![],
            NodeData::Identifier {
                name: TextSlice::from_span(0, 3),
            },
        );
        Ast::new(0, vec![tu, ident], 28, vec![])
    }

    #[test]
    fn round_trip_json() {
        let source = b"int main(void) { return 0; }";
        let ast = make_ast();
        let json = ast_to_json(&ast, source).unwrap();

        // Parse back
        let ast2 = ast_from_json(&json).unwrap();
        assert_eq!(ast2.root, ast.root);
        assert_eq!(ast2.source_length, ast.source_length);
        assert_eq!(ast2.nodes.len(), ast.nodes.len());

        // Verify the Identifier node's data round-trips
        let ident_data = ast2.nodes[1].data.as_ref().unwrap();
        match ident_data {
            NodeData::Identifier { name } => {
                assert_eq!(name.materialize(&[]), "int");
            }
            _ => panic!("expected Identifier, got {:?}", ident_data),
        }
    }

    #[test]
    fn round_trip_preserves_structural_null_data() {
        let source = b"x;";
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 2 }, vec![1]);
        let es = Node::structural(
            NodeKind::ExpressionStatement,
            Span { start: 0, end: 2 },
            vec![],
        );
        let ast = Ast::new(0, vec![tu, es], 2, vec![]);

        let json = ast_to_json(&ast, source).unwrap();
        // Verify JSON has "data": null for TranslationUnit
        assert!(json.contains("\"data\":null"));

        let ast2 = ast_from_json(&json).unwrap();
        assert!(ast2.nodes[0].data.is_none());
        assert!(ast2.nodes[1].data.is_none());
    }

    #[test]
    fn non_utf8_source_does_not_panic() {
        let source: &[u8] = &[0xFF, 0xFE, 0xFD, b'x'];
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 4 }, vec![1]);
        let ident = Node::with_data(
            NodeKind::Identifier,
            Span { start: 0, end: 4 },
            vec![],
            NodeData::Identifier {
                name: TextSlice::from_span(0, 4),
            },
        );
        let ast = Ast::new(0, vec![tu, ident], 4, vec![]);

        let json = ast_to_json(&ast, source).unwrap();
        // Should contain replacement characters
        assert!(json.contains('\u{FFFD}'));
    }

    #[test]
    fn null_in_optional_fields_json_loads_correctly() {
        // JSON with explicit null in optional fields
        let json = r#"{
            "version": "1.0",
            "language": "c",
            "source_length": 10,
            "root": 0,
            "nodes": [
                {
                    "kind": "TranslationUnit",
                    "span": {"start": 0, "end": 10},
                    "children": [1, 2, 3, 4],
                    "data": null
                },
                {
                    "kind": "IntegerLiteral",
                    "span": {"start": 0, "end": 1},
                    "children": [],
                    "data": {"type": "IntegerLiteral", "value": "42", "suffix": null}
                },
                {
                    "kind": "StringLiteral",
                    "span": {"start": 0, "end": 1},
                    "children": [],
                    "data": {"type": "StringLiteral", "value": "\"hello\"", "encoding": null}
                },
                {
                    "kind": "Enumerator",
                    "span": {"start": 0, "end": 1},
                    "children": [],
                    "data": {"type": "Enumerator", "name": "X", "value": null}
                },
                {
                    "kind": "CallExpr",
                    "span": {"start": 0, "end": 1},
                    "children": [],
                    "data": {"type": "CallExpr", "is_macro": null}
                }
            ],
            "errors": []
        }"#;

        let ast = ast_from_json(json).unwrap();
        assert_eq!(ast.nodes.len(), 5);

        // IntegerLiteral with null suffix → None
        match &ast.nodes[1].data {
            Some(NodeData::IntegerLiteral { suffix, .. }) => assert!(suffix.is_none()),
            other => panic!("unexpected: {:?}", other),
        }

        // StringLiteral with null encoding → None
        match &ast.nodes[2].data {
            Some(NodeData::StringLiteral { encoding, .. }) => assert!(encoding.is_none()),
            other => panic!("unexpected: {:?}", other),
        }

        // Enumerator with null value → None
        match &ast.nodes[3].data {
            Some(NodeData::Enumerator { value, .. }) => assert!(value.is_none()),
            other => panic!("unexpected: {:?}", other),
        }

        // CallExpr with null is_macro → None
        match &ast.nodes[4].data {
            Some(NodeData::CallExpr { is_macro }) => assert!(is_macro.is_none()),
            other => panic!("unexpected: {:?}", other),
        }

        // Round-trip: serialize again.  The null fields should serialize as
        // null (for string optional fields) or be absent (for is_macro).
        let source: &[u8] = b"dummy src ";
        let json2 = ast_to_pretty_json(&ast, source).unwrap();
        // Verify the JSON is valid (can be parsed back)
        let _ast3 = ast_from_json(&json2).unwrap();
    }

    #[test]
    fn call_expr_without_is_macro_omits_key() {
        let source = b"foo()";
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 5 }, vec![1]);
        let call = Node::with_data(
            NodeKind::CallExpr,
            Span { start: 0, end: 5 },
            vec![],
            NodeData::CallExpr { is_macro: None },
        );
        let ast = Ast::new(0, vec![tu, call], 5, vec![]);

        let json = ast_to_json(&ast, source).unwrap();
        // The key "is_macro" should NOT appear in the output
        assert!(!json.contains("is_macro"));
    }

    #[test]
    fn preprocessor_directive_empty_argument() {
        let source = b"#endif\n";
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 7 }, vec![1]);
        let pp = Node::with_data(
            NodeKind::PreprocessorEndif,
            Span { start: 0, end: 6 },
            vec![],
            NodeData::PreprocessorDirective {
                directive: "endif".to_string(),
                argument: TextSlice::from_static_str(""),
            },
        );
        let ast = Ast::new(0, vec![tu, pp], 7, vec![]);

        let json = ast_to_json(&ast, source).unwrap();
        // argument should be "" (not null)
        assert!(json.contains("\"argument\":\"\""));
    }

    #[test]
    fn error_node_serializes_correctly() {
        let source = b"error!";
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 6 }, vec![1]);
        let err_node = Node::with_data(
            NodeKind::ErrorNode,
            Span { start: 0, end: 6 },
            vec![],
            NodeData::ErrorNode { error_index: 0 },
        );
        let errors = vec![ParseError {
            code: "E_SYNTAX_TEST".to_string(),
            message: "test error".to_string(),
            span: Span { start: 0, end: 6 },
            severity: Severity::Error,
        }];
        let ast = Ast {
            root: 0,
            nodes: vec![tu, err_node],
            source_length: 6,
            errors,
        };

        let json = ast_to_json(&ast, source).unwrap();
        assert!(json.contains("\"error_index\":0"));
        assert!(json.contains("E_SYNTAX_TEST"));
    }

    #[test]
    fn structural_kinds_have_null_data() {
        let source = b"";
        let tu = Node::structural(NodeKind::TranslationUnit, Span { start: 0, end: 0 }, vec![]);
        let if_stmt = Node::structural(NodeKind::IfStatement, Span { start: 0, end: 0 }, vec![]);
        let ast = Ast::new(0, vec![tu, if_stmt], 0, vec![]);

        let json = ast_to_json(&ast, source).unwrap();
        // Both nodes should have "data":null
        assert_eq!(json.matches("\"data\":null").count(), 2);
    }

    #[test]
    fn shortened_discriminators() {
        let source = b"void f() {}";
        let ast = Ast::new(
            0,
            vec![
                Node::structural(
                    NodeKind::TranslationUnit,
                    Span { start: 0, end: 11 },
                    vec![1],
                ),
                Node::with_data(
                    NodeKind::FunctionDefinition,
                    Span { start: 0, end: 11 },
                    vec![],
                    NodeData::FunctionDef {
                        name: TextSlice::from_span(5, 6),
                        storage_class: TextSlice::from_static_str(""),
                    },
                ),
            ],
            11,
            vec![],
        );

        let json = ast_to_json(&ast, source).unwrap();
        // data.type should be "FunctionDef", not "FunctionDefinition"
        assert!(json.contains("\"type\":\"FunctionDef\""));
    }
}