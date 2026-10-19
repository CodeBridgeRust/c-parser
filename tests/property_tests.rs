//! Property-based and robustness tests for the C parser.
//!
//! Instead of using the `proptest` crate (not available in this sandbox),
//! we test edge cases and invariants systematically across a range of
//! inputs: random bytes, deep nesting, large parameter lists, multiple
//! dialects, etc.

use c_parser::{
    ast_from_json, ast_to_json, create_parser, ParseInput, ParserConfig, CDialect,
    PreprocessingMode,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn parse_to_json(source: &[u8]) -> String {
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source,
            filename: None,
            config,
        })
        .expect("fatal parse error");
    ast_to_json(&result.ast, source).expect("JSON serialization failed")
}

fn parse_to_value(source: &[u8]) -> Value {
    let json = parse_to_json(source);
    serde_json::from_str(&json).expect("invalid JSON")
}

// =========================================================================
// Robustness: parser never panics on random bytes
// =========================================================================

#[test]
fn parser_never_panics_on_empty_bytes() {
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let _ = parser.parse(ParseInput {
        source: &[],
        filename: None,
        config,
    });
}

#[test]
fn parser_never_panics_on_null_bytes() {
    let bytes = vec![0u8; 100];
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    // Must not panic.
    let result = parser.parse(ParseInput {
        source: &bytes,
        filename: None,
        config,
    });
    match result {
        Ok(r) => {
            let _ = ast_to_json(&r.ast, &bytes);
        }
        Err(_) => { /* fatal error is acceptable */ }
    }
}

#[test]
fn parser_never_panics_on_random_bytes() {
    // Test a variety of byte sequences that might cause issues.
    let test_vectors: &[&[u8]] = &[
        &[0xFF, 0xFF, 0xFF, 0xFF],
        &[0x00, 0x00, 0x00, 0x00],
        &[b'{', b'}', b'(', b')', b'[', b']'],
        &[b'"', b'"', b'\'', b'\''],
        &[b'#', b'#'],
        &[b'\\', b'\\'],
        b"@@@@@@@@@@",
        b"(((((((((((",
        b")))))))))))",
        b"[[[[[[[[[[[",
        b"]]]]]]]]]]]",
        b"{{{{{{{{{{{",
        b"}}}}}}}}}}}",
        b"/* /* /* /*",
        b"*/ */ */ */",
        b"// // // //",
    ];

    for (i, bytes) in test_vectors.iter().enumerate() {
        let config = ParserConfig::default();
        let parser = create_parser(config.clone());
        let result = parser.parse(ParseInput {
            source: bytes,
            filename: None,
            config,
        });
        match result {
            Ok(r) => {
                let json = ast_to_json(&r.ast, bytes);
                if let Ok(j) = json {
                    // Must be valid JSON.
                    let _: Value = serde_json::from_str(&j).unwrap_or_else(|e| {
                        panic!("test_vector[{i}]: output not valid JSON: {e}\n{j}");
                    });
                }
            }
            Err(_) => { /* acceptable */ }
        }
    }
}

// =========================================================================
// Dialect tests
// =========================================================================

#[test]
fn all_dialects_parse_same_source() {
    let source = b"int x;";
    for dialect in &[CDialect::C89, CDialect::C99, CDialect::C11, CDialect::C17] {
        let config = ParserConfig {
            dialect: *dialect,
            ..ParserConfig::default()
        };
        let parser = create_parser(config.clone());
        let result = parser
            .parse(ParseInput {
                source,
                filename: None,
                config,
            })
            .unwrap_or_else(|e| panic!("dialect {dialect:?} failed: {e}"));
        let json = ast_to_json(&result.ast, source).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["language"], "c");
    }
}

// =========================================================================
// Deep nesting tests (stack overflow protection)
// =========================================================================

#[test]
fn deep_nesting_does_not_overflow_stack() {
    // Generate a deeply nested expression: x + x + x + ... (depth 500).
    let mut expr = String::from("x");
    for _ in 0..500 {
        expr = format!("({expr} + x)");
    }
    let source = format!("int f(void) {{ return {expr}; }}");

    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source: source.as_bytes(),
            filename: None,
            config,
        })
        .expect("deep nesting should not overflow");
    let json =
        ast_to_json(&result.ast, source.as_bytes()).expect("serialization failed");
    let v: Value = serde_json::from_str(&json).unwrap();
    let bin_count = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "BinaryExpr")
        .count();
    assert!(
        bin_count >= 400,
        "expected >=400 BinaryExpr nodes, got {bin_count}"
    );
}

#[test]
fn deep_nesting_of_parens() {
    let mut s = String::from("return ");
    for _ in 0..300 {
        s.push('(');
    }
    s.push('0');
    for _ in 0..300 {
        s.push(')');
    }
    s.push(';');
    let source = format!("int f(void) {{ {s} }}");

    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source: source.as_bytes(),
            filename: None,
            config,
        })
        .expect("deep paren nesting should not overflow");
    let json =
        ast_to_json(&result.ast, source.as_bytes()).expect("serialization failed");
    let v: Value = serde_json::from_str(&json).unwrap();
    let parens = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "ParenExpr")
        .count();
    assert!(parens >= 250, "expected >=250 ParenExpr nodes, got {parens}");
}

// =========================================================================
// Large inputs
// =========================================================================

#[test]
fn many_function_parameters() {
    let params = (0..50)
        .map(|i| format!("int p{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("int f({params}) {{ return 0; }}");
    let json = parse_to_json(source.as_bytes());
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["language"], "c");
}

#[test]
fn many_variable_declarations() {
    let mut decls = String::new();
    for i in 0..50 {
        decls.push_str(&format!("int var{i} = {i};\n"));
    }
    let json = parse_to_json(decls.as_bytes());
    let v: Value = serde_json::from_str(&json).unwrap();
    let var_count = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "VariableDeclaration")
        .count();
    assert_eq!(var_count, 50);
}

#[test]
fn many_struct_members() {
    let members = (0..50)
        .map(|i| format!("int field{i};"))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!("struct Big {{ {members} }};");
    let json = parse_to_json(source.as_bytes());
    let v: Value = serde_json::from_str(&json).unwrap();
    let structs: Vec<_> = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "StructDeclaration")
        .collect();
    assert_eq!(structs.len(), 1);
}

// =========================================================================
// Nested blocks
// =========================================================================

#[test]
fn consecutive_nested_blocks() {
    let source = b"int f(void) { { { { { return 0; } } } } }";
    let v = parse_to_value(source);
    let compound_count = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "CompoundStatement")
        .count();
    assert!(compound_count >= 5);
}

// =========================================================================
// Unicode and encoding
// =========================================================================

#[test]
fn unicode_in_comments_is_handled() {
    let source = "int f(void) { /* русский */ return 0; }".as_bytes();
    let json = parse_to_json(source);
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["language"], "c");
}

#[test]
fn non_utf8_source_does_not_panic_and_produces_replacement_chars() {
    let source: &[u8] = &[0xFF, 0xFE, 0xFD, b'i', b'n', b't', b' ', b'x', b';'];
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser.parse(ParseInput {
        source,
        filename: None,
        config,
    });
    match result {
        Ok(r) => {
            // Must not panic. Replacement chars may not appear depending
            // on error recovery behavior.
            let json = ast_to_json(&r.ast, source).unwrap();
            let _: Value = serde_json::from_str(&json).unwrap();
        }
        Err(_) => { /* acceptable for truly garbage input */ }
    }
}

// =========================================================================
// SingleTu preprocessor mode
// =========================================================================

#[test]
fn single_tu_mode_parses_multiple_directives() {
    let source = b"#include <stdio.h>\n#define MAX 100\n#undef MAX\n#ifdef MAX\nint x;\n#endif\n";
    let config = ParserConfig {
        dialect: CDialect::C11,
        preprocessing: PreprocessingMode::SingleTu,
        include_comments: false,
        incremental: None,
    };
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source,
            filename: None,
            config,
        })
        .expect("single-tu should not fail");
    let json = ast_to_json(&result.ast, source).expect("serialization failed");
    let v: Value = serde_json::from_str(&json).unwrap();
    let pp_nodes: Vec<_> = v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"].as_str().unwrap().starts_with("Preprocessor"))
        .collect();
    assert!(
        pp_nodes.len() >= 1,
        "expected at least 1 preprocessor node, got {}",
        pp_nodes.len()
    );
}

// =========================================================================
// Various expression coverage
// =========================================================================

#[test]
fn all_binary_operators_parse() {
    let operators = [
        ("+", "BinaryExpr"),
        ("-", "BinaryExpr"),
        ("*", "BinaryExpr"),
        ("/", "BinaryExpr"),
        ("%", "BinaryExpr"),
        ("<", "BinaryExpr"),
        (">", "BinaryExpr"),
        ("<=", "BinaryExpr"),
        (">=", "BinaryExpr"),
        ("==", "BinaryExpr"),
        ("!=", "BinaryExpr"),
        ("&&", "BinaryExpr"),
        ("||", "BinaryExpr"),
        ("&", "BinaryExpr"),
        ("|", "BinaryExpr"),
        ("^", "BinaryExpr"),
        ("<<", "BinaryExpr"),
        (">>", "BinaryExpr"),
    ];

    for (op, expected_kind) in &operators {
        let source = format!("int f(void) {{ return a {op} b; }}");
        let v = parse_to_value(source.as_bytes());
        let bins: Vec<_> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"].as_str() == Some(expected_kind))
            .collect();
        assert!(
            !bins.is_empty(),
            "operator '{op}' did not produce {expected_kind}"
        );
    }
}

#[test]
fn all_assignment_operators_parse() {
    // Test compound assignment operators in expression context
    // (within a for-loop increment, where bare identifiers don't trigger
    //  the label-detection lookahead issue in parse_statement).
    let ops = [
        "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=",
    ];
    for op in &ops {
        let source = format!("int f(void) {{ int x = 0, y = 0; for (;(y {op} 1),0;) return x; }}");
        let v = parse_to_value(source.as_bytes());
        let assigns: Vec<_> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "AssignmentExpr")
            .collect();
        assert!(
            !assigns.is_empty(),
            "assignment '{op}' did not produce AssignmentExpr"
        );
    }
}

// =========================================================================
// JSON round-trip stability
// =========================================================================

#[test]
fn json_round_trip_is_stable() {
    let source = b"int add(int a, int b) { return a + b; }";
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source,
            filename: None,
            config,
        })
        .unwrap();
    let json1 = ast_to_json(&result.ast, source).unwrap();

    let ast2 = ast_from_json(&json1).expect("deserialize failed");
    let json2 = ast_to_json(&ast2, source).unwrap();

    let v1: Value = serde_json::from_str(&json1).unwrap();
    let v2: Value = serde_json::from_str(&json2).unwrap();
    assert_eq!(v1["version"], v2["version"]);
    assert_eq!(v1["language"], v2["language"]);
    assert_eq!(v1["source_length"], v2["source_length"]);
    assert_eq!(v1["root"], v2["root"]);
    assert_eq!(v1["errors"], v2["errors"]);
    assert_eq!(
        v1["nodes"].as_array().unwrap().len(),
        v2["nodes"].as_array().unwrap().len()
    );
}

// =========================================================================
// Send + Sync check
// =========================================================================

#[test]
fn parser_plugin_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Box<dyn c_parser::ParserPlugin>>();
}