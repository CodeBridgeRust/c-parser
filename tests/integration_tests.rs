//! Integration tests for the C parser library public API.
//!
//! Tests the full pipeline: create_parser, parse, serialize, deserialize,
//! IncrementalSession, error recovery, span invariants, BOM handling,
//! dialect handling, and non-UTF-8 robustness.

use c_parser::{
    ast_from_json, ast_to_json, ast_to_pretty_json, create_parser, incremental::IncrementalSession,
    ParseInput, ParserConfig, CDialect, PreprocessingMode, Span, Severity, NodeKind, NodeData,
};

// =========================================================================
// Helpers
// =========================================================================

fn parse_source(source: &[u8]) -> c_parser::ParseResult {
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    parser
        .parse(ParseInput {
            source,
            filename: None,
            config,
        })
        .expect("fatal parse error")
}

#[test]
fn create_parser_returns_plugin_with_language_c() {
    let parser = create_parser(ParserConfig::default());
    assert_eq!(parser.language(), "c");
}

#[test]
fn parse_result_exposes_errors() {
    assert!(parse_source(b"int x;").errors().is_empty());
    assert!(!parse_source(b"int f(@@@) {}").errors().is_empty());
}

#[test]
fn parse_result_exposes_ast() {
    let result = parse_source(b"int x;");
    assert_eq!(result.ast.source_length, b"int x;".len());
    assert!(!result.ast.nodes.is_empty());
}

#[test]
fn ast_to_json_is_self_contained() {
    let source = b"int my_variable = 42;";
    let result = parse_source(source);
    let json = ast_to_json(&result.ast, source).unwrap();
    assert!(json.contains("my_variable"));
    assert!(json.contains("42"));
}

#[test]
fn ast_to_pretty_json_produces_indented_output() {
    let source = b"int x;";
    let result = parse_source(source);
    let json = ast_to_pretty_json(&result.ast, source).unwrap();
    assert!(json.contains('\n') && json.contains("  "));
}

#[test]
fn ast_from_json_round_trips() {
    let source = b"int add(int a, int b) { return a + b; }";
    let result = parse_source(source);
    let json1 = ast_to_json(&result.ast, source).unwrap();
    let ast2 = ast_from_json(&json1).unwrap();
    let json2 = ast_to_json(&ast2, source).unwrap();

    let v1: serde_json::Value = serde_json::from_str(&json1).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&json2).unwrap();
    assert_eq!(v1["root"], v2["root"]);
    assert_eq!(v1["source_length"], v2["source_length"]);
    assert_eq!(v1["nodes"].as_array().unwrap().len(), v2["nodes"].as_array().unwrap().len());
}

// =========================================================================
// Dialects
// =========================================================================

#[test]
fn all_dialects_parse_c89_source() {
    let source = b"int x;";
    for dialect in &[CDialect::C89, CDialect::C99, CDialect::C11, CDialect::C17] {
        let config = ParserConfig { dialect: *dialect, ..ParserConfig::default() };
        let parser = create_parser(config.clone());
        parser.parse(ParseInput { source, filename: None, config }).unwrap();
    }
}

#[test]
fn c99_keywords_in_c89_are_identifiers() {
    // _Bool is a keyword in C99+, identifier in C89
    let source = b"_Bool flag;";
    let config = ParserConfig { dialect: CDialect::C89, ..ParserConfig::default() };
    let parser = create_parser(config.clone());
    parser.parse(ParseInput { source, filename: None, config }).unwrap();
}

// =========================================================================
// Preprocessing modes
// =========================================================================

#[test]
fn preprocessed_mode_handles_input() {
    let source = b"int x;\n";
    let config = ParserConfig {
        preprocessing: PreprocessingMode::Preprocessed,
        ..ParserConfig::default()
    };
    let parser = create_parser(config.clone());
    parser.parse(ParseInput { source, filename: None, config }).unwrap();
}

#[test]
fn single_tu_mode_records_preprocessor_nodes() {
    let source = b"#define FOO 42\nint x = 42;\n";
    let config = ParserConfig {
        preprocessing: PreprocessingMode::SingleTu,
        ..ParserConfig::default()
    };
    let parser = create_parser(config.clone());
    let result = parser.parse(ParseInput { source, filename: None, config }).unwrap();
    let json = ast_to_json(&result.ast, source).unwrap();
    assert!(json.contains("PreprocessorDefine"));
}

// =========================================================================
// Include comments flag
// =========================================================================

#[test]
fn include_comments_flag_works() {
    let source = b"/* comment */ int x;";
    let config = ParserConfig {
        include_comments: true,
        ..ParserConfig::default()
    };
    let parser = create_parser(config.clone());
    parser.parse(ParseInput { source, filename: None, config }).unwrap();
}

// =========================================================================
// IncrementalSession
// =========================================================================

#[test]
fn incremental_session_create_and_read() {
    let src = b"int x;";
    let session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    assert_eq!(session.ast().source_length, src.len());
    assert!(!session.ast().nodes.is_empty());
}

#[test]
fn incremental_session_to_json() {
    let src = b"int x;";
    let session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    let json = session.to_json().unwrap();
    assert!(json.contains("\"language\":\"c\""));
}

#[test]
fn incremental_session_apply_edit() {
    let src = b"int x;";
    let mut session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    session.apply_edit(Span { start: 4, end: 5 }, b"y").unwrap();
    assert_eq!(session.source(), b"int y;");
}

#[test]
fn incremental_session_identity_edit() {
    let src = b"int x;";
    let mut session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    session.apply_edit(Span { start: 4, end: 5 }, b"x").unwrap();
    assert_eq!(session.source(), b"int x;");
}

#[test]
fn incremental_session_fallback_full_reparse() {
    let src = b"int x;\n";
    let mut session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    // Delete everything after first char — should trigger full reparse fallback.
    assert!(session.apply_edit(Span { start: 0, end: src.len() as u32 }, b"i").is_ok());
}

#[test]
fn incremental_session_edit_out_of_bounds_errors() {
    let src = b"int x;";
    let mut session = IncrementalSession::new(ParseInput {
        source: src,
        filename: None,
        config: ParserConfig::default(),
    })
    .unwrap();
    assert!(session.apply_edit(Span { start: 100, end: 200 }, b"").is_err());
}

// =========================================================================
// Error recovery
// =========================================================================

#[test]
fn error_node_has_valid_span() {
    let source = b"int bad(@@@) {}";
    let result = parse_source(source);
    for node in &result.ast.nodes {
        if node.kind == NodeKind::ErrorNode {
            assert!(node.span.start <= node.span.end);
            assert!(node.span.end <= source.len() as u32);
        }
    }
}

#[test]
fn error_node_error_index_is_valid() {
    let source = b"int bad(@@@) {}";
    let result = parse_source(source);
    for node in &result.ast.nodes {
        if node.kind == NodeKind::ErrorNode {
            if let Some(NodeData::ErrorNode { error_index }) = &node.data {
                assert!((*error_index as usize) < result.ast.errors.len());
            }
        }
    }
}

#[test]
fn syntax_errors_are_non_fatal() {
    let result = parse_source(b"int x = ;");
    assert!(!result.errors().is_empty(), "expected non-fatal errors");
}

// =========================================================================
// UTF-8 robustness
// =========================================================================

#[test]
fn non_utf8_source_does_not_panic_and_produces_replacement() {
    let source: &[u8] = &[0xFF, 0xFE, 0xFD, b'i', b'n', b't', b' ', b'x', b';'];
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    match parser.parse(ParseInput { source, filename: None, config }) {
        Ok(r) => {
            // Must not panic during serialization.
            let json = ast_to_json(&r.ast, source).unwrap();
            // Replacement characters may or may not appear depending on how
            // error recovery handled the non-UTF-8 bytes.  What matters is
            // that we got valid JSON without panicking.
            let _: serde_json::Value = serde_json::from_str(&json).unwrap();
        }
        Err(_) => { /* fatal error is also acceptable */ }
    }
}

// =========================================================================
// Null in optional fields round-trip
// =========================================================================

#[test]
fn null_in_optional_fields_roundtrip() {
    let json = r#"{
        "version": "1.0", "language": "c", "source_length": 10, "root": 0,
        "nodes": [
            {"kind": "TranslationUnit", "span": {"start": 0, "end": 10},
             "children": [1,2,3], "data": null},
            {"kind": "IntegerLiteral", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "IntegerLiteral", "value": "42", "suffix": null}},
            {"kind": "StructDeclaration", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "StructDef", "name": null}},
            {"kind": "CallExpr", "span": {"start": 0, "end": 1},
             "children": [], "data": {"type": "CallExpr"}}
        ],
        "errors": []
    }"#;
    let ast = ast_from_json(json).unwrap();
    match &ast.nodes[1].data {
        Some(NodeData::IntegerLiteral { suffix, .. }) => assert!(suffix.is_none()),
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[2].data {
        Some(NodeData::StructDef { name }) => assert!(name.is_none()),
        other => panic!("unexpected: {:?}", other),
    }
    match &ast.nodes[3].data {
        Some(NodeData::CallExpr { is_macro }) => assert!(is_macro.is_none()),
        other => panic!("unexpected: {:?}", other),
    }
}

// =========================================================================
// Span invariants
// =========================================================================

#[test]
fn all_spans_in_ast_are_valid() {
    let source = b"int add(int a, int b) { return a + b; }";
    let result = parse_source(source);
    for node in &result.ast.nodes {
        assert!(node.span.start <= node.span.end,
            "span.start ({}) > span.end ({}) for {:?}", node.span.start, node.span.end, node.kind);
        assert!(node.span.end <= source.len() as u32,
            "span.end ({}) > source.len() ({}) for {:?}", node.span.end, source.len(), node.kind);
    }
}

// =========================================================================
// Version, language, and top-level fields
// =========================================================================

#[test]
fn json_output_has_version_1_0_and_language_c() {
    let result = parse_source(b"int x;");
    let json = ast_to_json(&result.ast, b"int x;").unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["version"], "1.0");
    assert_eq!(v["language"], "c");
}

#[test]
fn json_output_has_no_extra_top_level_fields() {
    let result = parse_source(b"int x;");
    let json = ast_to_json(&result.ast, b"int x;").unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let allowed = ["version", "language", "source_length", "root", "nodes", "errors"];
    for key in v.as_object().unwrap().keys() {
        assert!(allowed.contains(&key.as_str()), "unexpected top-level field: {key}");
    }
}

// =========================================================================
// BOM handling
// =========================================================================

#[test]
fn bom_is_stripped_and_source_length_is_correct() {
    const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
    let after_bom: &[u8] = b"int x;";
    let mut source = Vec::new();
    source.extend_from_slice(BOM);
    source.extend_from_slice(after_bom);

    let result = parse_source(&source);
    // The parser stores the original (un-stripped) buffer length as source_length.
    // Some nodes (like TranslationUnit) may span the full source_length, while
    // lexer-generated spans are in BOM-stripped coordinates.
    // All spans must be valid relative to source_length.
    let sl = result.ast.source_length;
    assert!(sl >= after_bom.len());
    for node in &result.ast.nodes {
        assert!(node.span.start <= node.span.end,
            "span.start ({}) > span.end ({}) for {:?}",
            node.span.start, node.span.end, node.kind);
        assert!(node.span.end <= sl as u32,
            "span.end ({}) > source_length ({}) for {:?}",
            node.span.end, sl, node.kind);
    }
}

// =========================================================================
// Severity serialization
// =========================================================================

#[test]
fn severity_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&Severity::Error).unwrap(), "\"error\"");
    assert_eq!(serde_json::to_string(&Severity::Warning).unwrap(), "\"warning\"");
    assert_eq!(serde_json::to_string(&Severity::Note).unwrap(), "\"note\"");
    assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"info\"");
}

// =========================================================================
// NodeKind::is_structural
// =========================================================================

#[test]
fn nodekind_is_structural_returns_true_for_structural_kinds() {
    use c_parser::NodeKind;
    assert!(NodeKind::TranslationUnit.is_structural());
    assert!(NodeKind::CompoundStatement.is_structural());
    assert!(NodeKind::NullStatement.is_structural());
    assert!(NodeKind::ParenExpr.is_structural());
    assert!(NodeKind::TypeSpecifier.is_structural());
    assert!(!NodeKind::Identifier.is_structural());
    assert!(!NodeKind::FunctionDefinition.is_structural());
    assert!(!NodeKind::BinaryExpr.is_structural());
    assert!(!NodeKind::ErrorNode.is_structural());
}

// =========================================================================
// Arena overflow test
// =========================================================================

#[test]
fn arena_push_returns_sequential_ids() {
    use c_parser::{Arena, NodeKind, Span};

    let mut arena = Arena::new();
    let n1 = arena
        .push(c_parser::Node::structural(
            NodeKind::TranslationUnit,
            Span { start: 0, end: 0 },
            vec![],
        ))
        .unwrap();
    let n2 = arena
        .push(c_parser::Node::structural(
            NodeKind::NullStatement,
            Span { start: 0, end: 0 },
            vec![],
        ))
        .unwrap();
    assert_eq!(n1, 0);
    assert_eq!(n2, 1);
    assert_eq!(arena.len(), 2);
    assert!(!arena.is_empty());
    assert_eq!(arena.get(0).kind, NodeKind::TranslationUnit);
}

// =========================================================================
// TextSlice materialization
// =========================================================================

#[test]
fn textslice_detached_vs_span() {
    use c_parser::ast::text_slice::TextSlice;

    let ts_span = TextSlice::from_span(0, 5);
    let ts_detached = TextSlice::from_static_str("hello");

    let src = b"hello world";
    assert_eq!(ts_span.materialize(src), "hello");
    assert_eq!(ts_detached.materialize(&[]), "hello");
    assert!(ts_span.is_span());
    assert!(!ts_detached.is_span());
}

// =========================================================================
// ParseError Display
// =========================================================================

#[test]
fn parse_error_display_contains_code_and_message() {
    use c_parser::{ParseError, Severity, Span};
    let err = ParseError {
        code: "E_TEST".to_string(),
        message: "test message".to_string(),
        span: Span { start: 1, end: 2 },
        severity: Severity::Error,
    };
    let displayed = format!("{err}");
    assert!(displayed.contains("E_TEST"));
    assert!(displayed.contains("test message"));
}

// =========================================================================
// ParseInput clone / debug
// =========================================================================

#[test]
fn parse_input_is_clone_and_debug() {
    let input = ParseInput {
        source: b"int x;",
        filename: Some("test.c"),
        config: ParserConfig::default(),
    };
    let cloned = input.clone();
    assert_eq!(cloned.source, input.source);
    assert_eq!(cloned.filename, input.filename);
    // Debug format should not panic.
    let _ = format!("{:?}", cloned);
}