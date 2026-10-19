//! JSON Schema validation tests for the AST output.
//!
//! This module provides:
//! 1. A **custom schema validator** that checks the AST output against
//!    `docs/ast-schema.json` using a manual structural check (since
//!    `jsonschema` crate is not available in this sandbox).
//! 2. A **custom invariant validator** that checks invariants not enforceable
//!    by pure JSON Schema (span bounds, index referential integrity,
//!    per-kind `data` pairing, `error_index` validity).
//!
//! Required by ТЗ §12.1 and `docs/ast-schema.json` §1.

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers: locate files
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests/fixtures")
}

/// Read the UTF-8 BOM bytes for prepending to test files.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

// ---------------------------------------------------------------------------
// Helper: parse a fixture file and return the JSON
// ---------------------------------------------------------------------------

fn parse_fixture_to_json(path: &std::path::Path) -> (String, Vec<u8>) {
    let source = std::fs::read(path).expect("failed to read fixture");
    let config = c_parser::ParserConfig::default();
    let parser = c_parser::create_parser(config.clone());
    let result = parser
        .parse(c_parser::ParseInput {
            source: &source,
            filename: path.to_str(),
            config,
        })
        .expect("fatal parse error on fixture");
    let json = c_parser::ast_to_json(&result.ast, &source).expect("JSON serialization failed");
    (json, source)
}

// ---------------------------------------------------------------------------
// NodeKind enum (mirrored from ast-schema.json)
// ---------------------------------------------------------------------------

const STRUCTURAL_KINDS: &[&str] = &[
    "TranslationUnit",
    "CompoundStatement",
    "IfStatement",
    "WhileStatement",
    "DoWhileStatement",
    "ForStatement",
    "SwitchStatement",
    "CaseStatement",
    "DefaultStatement",
    "ReturnStatement",
    "BreakStatement",
    "ContinueStatement",
    "GotoStatement",
    "LabelStatement",
    "ExpressionStatement",
    "NullStatement",
    "ParenExpr",
    "TypeSpecifier",
    "PointerType",
    "ArrayType",
    "FunctionType",
    "InitializerList",
];

const DATA_KIND_MAP: &[(&str, &str)] = &[
    ("FunctionDefinition", "FunctionDef"),
    ("VariableDeclaration", "VariableDecl"),
    ("ParameterDeclaration", "ParameterDecl"),
    ("TypedefDeclaration", "TypedefDecl"),
    ("StructDeclaration", "StructDef"),
    ("UnionDeclaration", "UnionDef"),
    ("EnumDeclaration", "EnumDef"),
    ("Enumerator", "Enumerator"),
    ("Identifier", "Identifier"),
    ("IntegerLiteral", "IntegerLiteral"),
    ("FloatLiteral", "FloatLiteral"),
    ("StringLiteral", "StringLiteral"),
    ("CharLiteral", "CharLiteral"),
    ("BinaryExpr", "BinaryExpr"),
    ("UnaryExpr", "UnaryExpr"),
    ("AssignmentExpr", "AssignmentExpr"),
    ("CallExpr", "CallExpr"),
    ("MemberExpr", "MemberExpr"),
    ("IndexExpr", "IndexExpr"),
    ("CastExpr", "CastExpr"),
    ("TernaryExpr", "TernaryExpr"),
    ("SizeofExpr", "SizeofExpr"),
    ("ErrorNode", "ErrorNode"),
    // Preprocessor* kinds all use "PreprocessorDirective" as data.type
    ("PreprocessorInclude", "PreprocessorDirective"),
    ("PreprocessorDefine", "PreprocessorDirective"),
    ("PreprocessorUndef", "PreprocessorDirective"),
    ("PreprocessorIf", "PreprocessorDirective"),
    ("PreprocessorIfdef", "PreprocessorDirective"),
    ("PreprocessorIfndef", "PreprocessorDirective"),
    ("PreprocessorElse", "PreprocessorDirective"),
    ("PreprocessorElif", "PreprocessorDirective"),
    ("PreprocessorEndif", "PreprocessorDirective"),
    ("PreprocessorPragma", "PreprocessorDirective"),
    ("PreprocessorLine", "PreprocessorDirective"),
    ("PreprocessorError", "PreprocessorDirective"),
    ("PreprocessorWarning", "PreprocessorDirective"),
];

fn is_structural_kind(kind: &str) -> bool {
    STRUCTURAL_KINDS.contains(&kind)
}

fn expected_data_type(kind: &str) -> Option<&'static str> {
    DATA_KIND_MAP
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, dt)| *dt)
}

// ---------------------------------------------------------------------------
// Custom schema validator
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SchemaError {
    pub message: String,
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Validate an AST JSON value against the structural schema rules.
/// Returns a list of violations (empty = valid).
pub fn validate_against_schema(ast_json: &Value) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    // Top-level must be an object.
    let obj = match ast_json.as_object() {
        Some(o) => o,
        None => {
            errors.push(SchemaError {
                message: "root is not an object".into(),
            });
            return errors;
        }
    };

    // Required top-level fields.
    for field in &[
        "version",
        "language",
        "source_length",
        "root",
        "nodes",
        "errors",
    ] {
        if !obj.contains_key(*field) {
            errors.push(SchemaError {
                message: format!("missing required top-level field: {field}"),
            });
        }
    }

    // version must be "1.0".
    if obj.get("version").and_then(|v| v.as_str()) != Some("1.0") {
        errors.push(SchemaError {
            message: "version is not \"1.0\"".into(),
        });
    }

    // language must be "c".
    if obj.get("language").and_then(|v| v.as_str()) != Some("c") {
        errors.push(SchemaError {
            message: "language is not \"c\"".into(),
        });
    }

    // source_length must be a non-negative integer.
    if let Some(sl) = obj.get("source_length") {
        if !sl.is_u64() {
            errors.push(SchemaError {
                message: "source_length is not an integer".into(),
            });
        }
    }

    // root must be a non-negative integer.
    if let Some(r) = obj.get("root") {
        if !r.is_u64() {
            errors.push(SchemaError {
                message: "root is not an integer".into(),
            });
        }
    }

    // nodes must be an array with at least 1 element.
    let nodes = match obj.get("nodes").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        Some(_) => {
            errors.push(SchemaError {
                message: "nodes array is empty".into(),
            });
            return errors;
        }
        None => {
            errors.push(SchemaError {
                message: "nodes is not a non-empty array".into(),
            });
            return errors;
        }
    };

    // errors must be an array.
    if !obj.get("errors").is_some_and(|v| v.is_array()) {
        errors.push(SchemaError {
            message: "errors is not an array".into(),
        });
    }

    // No additional top-level properties.
    let allowed_top: HashSet<&str> = [
        "version",
        "language",
        "source_length",
        "root",
        "nodes",
        "errors",
    ]
    .iter()
    .copied()
    .collect();
    for key in obj.keys() {
        if !allowed_top.contains(key.as_str()) {
            errors.push(SchemaError {
                message: format!("unexpected top-level field: {key}"),
            });
        }
    }

    // Validate each node.
    for (i, node) in nodes.iter().enumerate() {
        errors.append(&mut validate_node(node, i));
    }

    // Validate each error entry.
    if let Some(errs) = obj.get("errors").and_then(|v| v.as_array()) {
        for (i, err) in errs.iter().enumerate() {
            errors.append(&mut validate_error_entry(err, i));
        }
    }

    errors
}

/// Validate a single node.
fn validate_node(node: &Value, idx: usize) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    let obj = match node.as_object() {
        Some(o) => o,
        None => {
            errors.push(SchemaError {
                message: format!("node[{idx}] is not an object"),
            });
            return errors;
        }
    };

    // Required fields.
    for field in &["kind", "span", "children", "data"] {
        if !obj.contains_key(*field) {
            errors.push(SchemaError {
                message: format!("node[{idx}] missing required field: {field}"),
            });
        }
    }

    // No additional properties.
    let allowed: HashSet<&str> = ["kind", "span", "children", "data"]
        .iter()
        .copied()
        .collect();
    for key in obj.keys() {
        if !allowed.contains(key.as_str()) {
            errors.push(SchemaError {
                message: format!("node[{idx}] unexpected field: {key}"),
            });
        }
    }

    // kind must be a valid NodeKind string.
    let kind = match obj.get("kind").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            errors.push(SchemaError {
                message: format!("node[{idx}] kind is not a string"),
            });
            return errors;
        }
    };

    let all_kinds: HashSet<&str> = STRUCTURAL_KINDS
        .iter()
        .copied()
        .chain(DATA_KIND_MAP.iter().map(|(k, _)| *k))
        .collect();
    if !all_kinds.contains(kind) {
        errors.push(SchemaError {
            message: format!("node[{idx}] unknown kind: {kind}"),
        });
    }

    // span must have start and end (non-negative integers).
    if let Some(span) = obj.get("span") {
        if !span.get("start").is_some_and(|v| v.is_u64()) {
            errors.push(SchemaError {
                message: format!("node[{idx}] span.start missing or not integer"),
            });
        }
        if !span.get("end").is_some_and(|v| v.is_u64()) {
            errors.push(SchemaError {
                message: format!("node[{idx}] span.end missing or not integer"),
            });
        }
    }

    // children must be an array of integers.
    if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
        for (ci, child) in children.iter().enumerate() {
            if !child.is_u64() {
                errors.push(SchemaError {
                    message: format!("node[{idx}] children[{ci}] is not an integer"),
                });
            }
        }
    }

    // Per-kind data enforcement.
    let data = obj.get("data");
    if is_structural_kind(kind) {
        // data must be null.
        if data.is_some_and(|v| !v.is_null()) {
            errors.push(SchemaError {
                message: format!("node[{idx}] kind={kind} is structural but data is not null"),
            });
        }
    } else if data.is_none() || data.is_some_and(|v| v.is_null()) {
        errors.push(SchemaError {
            message: format!("node[{idx}] kind={kind} is not structural but data is null"),
        });
    } else if let Some(data_obj) = data.and_then(|v| v.as_object()) {
        let expected_type = expected_data_type(kind);
        let actual_type = data_obj.get("type").and_then(|v| v.as_str());
        if expected_type != actual_type {
            errors.push(SchemaError {
                message: format!(
                    "node[{idx}] kind={kind} but data.type={:?} (expected {:?})",
                    actual_type, expected_type
                ),
            });
        }
    }

    errors
}

/// Validate an error entry.
fn validate_error_entry(err: &Value, idx: usize) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    let obj = match err.as_object() {
        Some(o) => o,
        None => {
            errors.push(SchemaError {
                message: format!("errors[{idx}] is not an object"),
            });
            return errors;
        }
    };

    for field in &["code", "message", "span", "severity"] {
        if !obj.contains_key(*field) {
            errors.push(SchemaError {
                message: format!("errors[{idx}] missing required field: {field}"),
            });
        }
    }

    if let Some(sev) = obj.get("severity").and_then(|v| v.as_str()) {
        let valid = ["error", "warning", "note", "info"];
        if !valid.contains(&sev) {
            errors.push(SchemaError {
                message: format!("errors[{idx}] invalid severity: {sev}"),
            });
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Custom invariant validator
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub struct InvariantError {
    pub message: String,
    pub node_idx: Option<usize>,
}

/// Validate invariants that pure JSON Schema cannot express.
pub fn validate_ast_invariants(ast_json: &Value) -> Vec<InvariantError> {
    let mut errors = Vec::new();

    let source_length = match ast_json.get("source_length").and_then(|v| v.as_u64()) {
        Some(n) => n as usize,
        None => {
            errors.push(InvariantError {
                message: "missing or invalid source_length".to_string(),
                node_idx: None,
            });
            return errors;
        }
    };

    let nodes = match ast_json.get("nodes").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            errors.push(InvariantError {
                message: "missing or invalid nodes array".to_string(),
                node_idx: None,
            });
            return errors;
        }
    };

    let errors_arr = match ast_json.get("errors").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => {
            errors.push(InvariantError {
                message: "missing or invalid errors array".to_string(),
                node_idx: None,
            });
            return errors;
        }
    };

    if let Some(root) = ast_json.get("root").and_then(|v| v.as_u64()) {
        if root as usize >= nodes.len() {
            errors.push(InvariantError {
                message: format!(
                    "root index {root} out of bounds (nodes.len() = {})",
                    nodes.len()
                ),
                node_idx: None,
            });
        }
    }

    for (i, node) in nodes.iter().enumerate() {
        if let Some(span) = node.get("span") {
            let start = span.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
            let end = span.get("end").and_then(|v| v.as_u64()).unwrap_or(0);
            if start > end {
                errors.push(InvariantError {
                    message: format!("span.start ({start}) > span.end ({end})"),
                    node_idx: Some(i),
                });
            }
            if end as usize > source_length {
                errors.push(InvariantError {
                    message: format!("span.end ({end}) > source_length ({source_length})"),
                    node_idx: Some(i),
                });
            }
        }

        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            for child in children {
                if let Some(ci) = child.as_u64() {
                    if ci as usize >= nodes.len() {
                        errors.push(InvariantError {
                            message: format!(
                                "child index {ci} out of bounds (nodes.len() = {})",
                                nodes.len()
                            ),
                            node_idx: Some(i),
                        });
                    }
                }
            }
        }

        if let Some(kind) = node.get("kind").and_then(|v| v.as_str()) {
            if kind == "ErrorNode" {
                if let Some(data) = node.get("data") {
                    if let Some(ei) = data.get("error_index").and_then(|v| v.as_u64()) {
                        if ei as usize >= errors_arr.len() {
                            errors.push(InvariantError {
                                message: format!(
                                    "ErrorNode.error_index ({ei}) out of bounds \
                                     (errors.len() = {})",
                                    errors_arr.len()
                                ),
                                node_idx: Some(i),
                            });
                        }
                    }
                }
            }
        }
    }

    errors
}

// =========================================================================
// Tests
// =========================================================================

#[test]
fn ast_schema_json_is_valid_json() {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/ast-schema.json");
    let text = std::fs::read_to_string(&schema_path).expect("failed to read schema");
    serde_json::from_str::<Value>(&text).expect("ast-schema.json is not valid JSON");
}

#[test]
fn empty_file_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("empty.c"));
    let value: Value = serde_json::from_str(&json).expect("invalid JSON");

    let schema_errors = validate_against_schema(&value);
    assert!(
        schema_errors.is_empty(),
        "Schema validation failed: {:?}",
        schema_errors
    );

    let invariant_errors = validate_ast_invariants(&value);
    assert!(
        invariant_errors.is_empty(),
        "Invariant violations: {:?}",
        invariant_errors
    );
}

#[test]
fn simple_function_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("simple_function.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn hello_world_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("hello_world.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn struct_enum_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("struct_enum.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn control_flow_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("control_flow.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn expressions_produces_valid_ast() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("expressions.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn syntax_error_produces_valid_ast_with_errors() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("syntax_error.c"));
    let value: Value = serde_json::from_str(&json).unwrap();

    let schema_errors = validate_against_schema(&value);
    assert!(
        schema_errors.is_empty(),
        "Schema violations: {:?}",
        schema_errors
    );

    let invariant_errors = validate_ast_invariants(&value);
    assert!(
        invariant_errors.is_empty(),
        "Invariant violations: {:?}",
        invariant_errors
    );

    let errors_arr = value["errors"].as_array().unwrap();
    assert!(!errors_arr.is_empty(), "expected non-empty errors array");
}

#[test]
fn bom_file_handled_correctly() {
    let source_no_bom = std::fs::read(fixtures_dir().join("bom.c")).expect("failed to read bom.c");
    let mut source = Vec::with_capacity(BOM.len() + source_no_bom.len());
    source.extend_from_slice(BOM);
    source.extend_from_slice(&source_no_bom);

    let config = c_parser::ParserConfig::default();
    let parser = c_parser::create_parser(config.clone());
    let result = parser
        .parse(c_parser::ParseInput {
            source: &source,
            filename: Some("bom.c"),
            config,
        })
        .expect("fatal parse error");
    let json = c_parser::ast_to_json(&result.ast, &source).expect("JSON serialization failed");

    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
    assert!(validate_ast_invariants(&value).is_empty());
}

#[test]
fn non_utf8_source_does_not_panic() {
    let source: &[u8] = &[
        0xFF, 0xFE, 0xFD, b'i', b'n', b't', b' ', b'x', b';', 0xC0, 0x80,
    ];
    let config = c_parser::ParserConfig::default();
    let parser = c_parser::create_parser(config.clone());
    let result = parser
        .parse(c_parser::ParseInput {
            source,
            filename: None,
            config,
        })
        .expect("should not panic");
    let json = c_parser::ast_to_json(&result.ast, source).expect("should serialize");
    // Replacement characters may or may not appear depending on error recovery.
    // What matters: parser didn't panic, output is valid JSON, schema is valid.
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(validate_against_schema(&value).is_empty());
}

#[test]
fn null_in_optional_fields_roundtrip() {
    let json = r#"{
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 10},
             "children": [1,2,3,4,5], "data": null},
            {"kind": "IntegerLiteral", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "IntegerLiteral", "value": "42", "suffix": null}},
            {"kind": "StringLiteral", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "StringLiteral", "value": "\"hello\"", "encoding": null}},
            {"kind": "Enumerator", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "Enumerator", "name": "X", "value": null}},
            {"kind": "CallExpr", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "CallExpr"}},
            {"kind": "StructDeclaration", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "StructDef", "name": null}}
        ],
        "errors": []
    }"#;

    let ast = c_parser::ast_from_json(json).expect("should deserialize");
    assert_eq!(ast.nodes.len(), 6);

    match &ast.nodes[1].data {
        Some(c_parser::NodeData::IntegerLiteral { suffix, .. }) => {
            assert!(suffix.is_none());
        }
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[2].data {
        Some(c_parser::NodeData::StringLiteral { encoding, .. }) => {
            assert!(encoding.is_none());
        }
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[3].data {
        Some(c_parser::NodeData::Enumerator { value, .. }) => {
            assert!(value.is_none());
        }
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[4].data {
        Some(c_parser::NodeData::CallExpr { is_macro }) => {
            assert!(is_macro.is_none());
        }
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[5].data {
        Some(c_parser::NodeData::StructDef { name }) => {
            assert!(name.is_none());
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[test]
fn error_node_connectivity() {
    let source = b"int bad_function(@@@) { int x = ; return x; }";
    let config = c_parser::ParserConfig::default();
    let parser = c_parser::create_parser(config.clone());
    let result = parser
        .parse(c_parser::ParseInput {
            source,
            filename: None,
            config,
        })
        .expect("fatal parse error");
    let json = c_parser::ast_to_json(&result.ast, source).expect("JSON serialization failed");
    let value: Value = serde_json::from_str(&json).unwrap();

    let errors_arr = value["errors"].as_array().unwrap();
    let nodes = value["nodes"].as_array().unwrap();

    let mut found_error_nodes = false;
    for node in nodes {
        if node["kind"] == "ErrorNode" {
            found_error_nodes = true;
            let error_index = node["data"]["error_index"].as_u64().unwrap() as usize;
            assert!(error_index < errors_arr.len(), "error_index out of bounds");
        }
    }
    if !errors_arr.is_empty() {
        assert!(found_error_nodes, "errors but no ErrorNode");
    }
}

#[test]
fn invariant_span_end_le_source_length() {
    let mut json = serde_json::json!({
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 999},
             "children": [], "data": null}
        ],
        "errors": []
    });
    let errors = validate_ast_invariants(&json);
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.message.contains("span.end")));

    json["nodes"][0]["span"]["end"] = serde_json::json!(10);
    assert!(validate_ast_invariants(&json).is_empty());
}

#[test]
fn invariant_span_start_le_end() {
    let mut json = serde_json::json!({
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 5, "end": 3},
             "children": [], "data": null}
        ],
        "errors": []
    });
    let errors = validate_ast_invariants(&json);
    assert!(!errors.is_empty());

    json["nodes"][0]["span"]["start"] = serde_json::json!(0);
    assert!(validate_ast_invariants(&json).is_empty());
}

#[test]
fn invariant_child_index_in_bounds() {
    let json = serde_json::json!({
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 10},
             "children": [999], "data": null}
        ],
        "errors": []
    });
    assert!(!validate_ast_invariants(&json).is_empty());
}

#[test]
fn invariant_root_in_bounds() {
    let json = serde_json::json!({
        "version": "1.0", "language": "c", "source_length": 10, "root": 5,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 10},
             "children": [], "data": null}
        ],
        "errors": []
    });
    assert!(!validate_ast_invariants(&json).is_empty());
}

#[test]
fn invariant_error_index_in_bounds() {
    let json = serde_json::json!({
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 10},
             "children": [1], "data": null},
            {"kind": "ErrorNode", "span": {"start": 0, "end": 5},
             "children": [], "data": {"type": "ErrorNode", "error_index": 999}}
        ],
        "errors": []
    });
    assert!(!validate_ast_invariants(&json).is_empty());
}

#[test]
fn all_fixtures_produce_valid_json() {
    let dir = fixtures_dir();
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "c"))
        .map(|e| e.path())
        .collect();

    assert!(!files.is_empty());

    for path in &files {
        let (json, _source) = parse_fixture_to_json(path);
        let value: Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("invalid JSON from {}: {e}", path.display()));
        let schema_errors = validate_against_schema(&value);
        assert!(
            schema_errors.is_empty(),
            "Schema violations for {}: {:?}",
            path.display(),
            schema_errors
        );
        let inv = validate_ast_invariants(&value);
        assert!(
            inv.is_empty(),
            "Invariant violations for {}: {:?}",
            path.display(),
            inv
        );
    }
}

#[test]
fn output_contains_all_required_top_level_fields() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("empty.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    for field in &[
        "version",
        "language",
        "source_length",
        "root",
        "nodes",
        "errors",
    ] {
        assert!(
            value.get(field).is_some(),
            "missing top-level field: {field}"
        );
    }
}

#[test]
fn node_has_all_required_fields() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("simple_function.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    for (i, node) in value["nodes"].as_array().unwrap().iter().enumerate() {
        for field in &["kind", "span", "children", "data"] {
            assert!(node.get(field).is_some(), "node[{i}] missing: {field}");
        }
    }
}

#[test]
fn error_has_all_required_fields() {
    let (json, _source) = parse_fixture_to_json(&fixtures_dir().join("syntax_error.c"));
    let value: Value = serde_json::from_str(&json).unwrap();
    for (i, err) in value["errors"].as_array().unwrap().iter().enumerate() {
        for field in &["code", "message", "span", "severity"] {
            assert!(err.get(field).is_some(), "errors[{i}] missing: {field}");
        }
    }
}
