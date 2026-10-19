//! End-to-end tests for the `c-parser-cli` binary.
//!
//! Uses `std::process::Command` to exercise the compiled CLI binary directly.
//! Covers `parse` and `parse-incremental` subcommands, output options,
//! dialect/preprocessing flags, and exit codes (0/1/2).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests/fixtures")
}

fn cli_binary() -> PathBuf {
    // The binary is built as `c-parser-cli` in the workspace.
    // Cargo builds it to target/<profile>/c-parser-cli
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join(format!("target/{profile}/c-parser-cli"))
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(cli_binary())
        .args(args)
        .output()
        .expect("failed to execute CLI binary")
}

/// Write content to a temp file and return its path.
fn temp_file(content: &[u8]) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
    f.write_all(content).expect("failed to write temp file");
    f
}

fn assert_success(output: &std::process::Output, context: &str) {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "{context}: expected success, got exit {:?}\nstderr: {stderr}",
            output.status.code()
        );
    }
}

fn assert_failure(output: &std::process::Output, context: &str) {
    if output.status.success() {
        panic!("{context}: expected failure, got success");
    }
}

fn stdout_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[allow(dead_code)]
fn stderr_str(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

// =========================================================================
// Parse subcommand
// =========================================================================

#[test]
fn parse_empty_file() {
    let out = run_cli(&["parse", &fixtures_dir().join("empty.c").to_string_lossy()]);
    assert_success(&out, "parse empty.c");
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("\"language\":\"c\""),
        "missing language field"
    );
    assert!(stdout.contains("\"nodes\":["), "missing nodes array");
}

#[test]
fn parse_simple_function() {
    let out = run_cli(&[
        "parse",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse simple_function.c");
    assert!(stdout_str(&out).contains("FunctionDefinition"));
}

#[test]
fn parse_with_pretty_flag() {
    let out = run_cli(&[
        "parse",
        "--pretty",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse --pretty");
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("  "),
        "pretty output should have indentation"
    );
}

#[test]
fn parse_with_output_to_file() {
    let output_dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = output_dir.path().join("output.json");

    let out = run_cli(&[
        "parse",
        "-o",
        &output_path.to_string_lossy(),
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse -o");

    let content = std::fs::read_to_string(&output_path).expect("failed to read output file");
    assert!(content.contains("\"language\":\"c\""));
}

#[test]
fn parse_with_dialect_c89() {
    let out = run_cli(&[
        "parse",
        "--dialect",
        "c89",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse --dialect c89");
}

#[test]
fn parse_with_include_comments() {
    let out = run_cli(&[
        "parse",
        "--include-comments",
        &fixtures_dir().join("empty.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse --include-comments");
}

#[test]
fn parse_nonexistent_file() {
    let out = run_cli(&["parse", "/nonexistent/file/that/does/not/exist.c"]);
    assert_failure(&out, "parse nonexistent file");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn parse_syntax_error_file_succeeds_with_diagnostics() {
    let out = run_cli(&[
        "parse",
        &fixtures_dir().join("syntax_error.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse syntax_error.c");
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("\"errors\":["),
        "should contain errors array"
    );
}

#[test]
fn parse_via_temp_file() {
    let source = b"int x;\n";
    let f = temp_file(source);

    let out = run_cli(&["parse", &f.path().to_string_lossy()]);
    assert_success(&out, "parse temp file");
    assert!(stdout_str(&out).contains("VariableDeclaration"));
}

// =========================================================================
// Parse-incremental subcommand
// =========================================================================

#[test]
fn parse_incremental_simple() {
    let out = run_cli(&[
        "parse-incremental",
        "--start",
        "0",
        "--end",
        "0",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse-incremental");
}

#[test]
fn parse_incremental_with_invalid_range() {
    let out = run_cli(&[
        "parse-incremental",
        "--start",
        "100",
        "--end",
        "200",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_failure(&out, "parse-incremental invalid range");
}

// =========================================================================
// Exit codes
// =========================================================================

#[test]
fn help_exits_zero() {
    let out = run_cli(&["--help"]);
    assert_success(&out, "--help");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn version_exits_zero() {
    let out = run_cli(&["--version"]);
    assert_success(&out, "--version");
}

#[test]
fn invalid_subcommand_exits_two() {
    let out = run_cli(&["nonexistent"]);
    assert!(out.status.code() == Some(2) || out.status.code() == Some(1));
}

#[test]
fn invalid_dialect_exits_two() {
    let out = run_cli(&[
        "parse",
        "--dialect",
        "c42",
        &fixtures_dir().join("empty.c").to_string_lossy(),
    ]);
    // clap should reject invalid enum with exit code 2.
    assert_eq!(
        out.status.code(),
        Some(2),
        "should exit 2 for invalid dialect"
    );
}

// =========================================================================
// Preprocessing mode
// =========================================================================

#[test]
fn parse_single_tu_mode() {
    let source = b"#define FOO 42\nint x;\n";
    let f = temp_file(source);

    let out = run_cli(&[
        "parse",
        "--preprocessing",
        "single-tu",
        &f.path().to_string_lossy(),
    ]);
    assert_success(&out, "parse --preprocessing single-tu");
}

// =========================================================================
// Config file resolution
// =========================================================================

#[test]
fn config_file_takes_effect() {
    let output_dir = tempfile::tempdir().expect("failed to create temp dir");
    let config_path = output_dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[parser]\ndialect = \"c89\"\npreprocessing = \"preprocessed\"\n\
         include_comments = true\n",
    )
    .expect("failed to write config");

    let out = run_cli(&[
        "parse",
        "--config",
        &config_path.to_string_lossy(),
        &fixtures_dir().join("empty.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse --config");
}

// =========================================================================
// Serialization integrity
// =========================================================================

#[test]
fn output_is_valid_json() {
    let out = run_cli(&[
        "parse",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse");
    let stdout = stdout_str(&out).trim().to_string();
    serde_json::from_str::<serde_json::Value>(&stdout).expect("CLI output is not valid JSON");
}

#[test]
fn output_contains_meta_fields() {
    let out = run_cli(&[
        "parse",
        &fixtures_dir().join("simple_function.c").to_string_lossy(),
    ]);
    assert_success(&out, "parse");
    let stdout = stdout_str(&out);
    assert!(stdout.contains("\"version\":\"1.0\""));
    assert!(stdout.contains("\"language\":\"c\""));
    assert!(stdout.contains("\"source_length\":"));
}

// =========================================================================
// Robustness tests
// =========================================================================

#[test]
fn empty_input_file() {
    let out = run_cli(&["parse", &fixtures_dir().join("empty.c").to_string_lossy()]);
    assert_success(&out, "parse empty");
}

#[test]
fn large_expression_file() {
    let source =
        b"int f(void) { return 1 + 2 * 3 - 4 / 5 + 6 % 7 << 8 >> 9 & 10 | 11 ^ 12 && 13 || 14; }\n";
    let f = temp_file(source);

    let out = run_cli(&["parse", &f.path().to_string_lossy()]);
    assert_success(&out, "parse large expression");
    assert!(stdout_str(&out).contains("BinaryExpr"));
}

#[test]
fn non_utf8_file_is_handled_gracefully() {
    let source: &[u8] = &[0xFF, 0xFE, b'i', b'n', b't', b' ', b'x', b';'];
    let f = temp_file(source);

    let out = run_cli(&["parse", &f.path().to_string_lossy()]);
    assert_success(&out, "parse non-utf8");
}

#[test]
fn parse_all_dialects() {
    for dialect in &["c89", "c99", "c11", "c17"] {
        let out = run_cli(&[
            "parse",
            "--dialect",
            dialect,
            &fixtures_dir().join("empty.c").to_string_lossy(),
        ]);
        assert_success(&out, &format!("parse --dialect {dialect}"));
    }
}
