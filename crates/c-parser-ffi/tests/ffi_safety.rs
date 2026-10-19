//! Functional safety tests for the FFI boundary.
//!
//! These tests link against the static (`lib`) target of `c-parser-ffi` and
//! call the exported functions directly, verifying the documented contract:
//! JSON-only exchange, `NULL` on fatal failure, and a no-op
//! `c_parser_free_string(NULL)`.
//!
//! The exact C symbol names (`#[no_mangle] extern "C"`) are verified by the
//! `dynamic_loading.rs` test, which loads the built `cdylib` through
//! `libloading` — the same `dlopen` mechanism a C/C++/Go host would use.

use std::ffi::CStr;

use c_parser_ffi::{c_parser_free_string, c_parser_parse};

const SOURCE: &[u8] = b"int main(void) { return 0; }\n";
const CONFIG: &[u8] = b"{}";

/// Parse a source and return the owned JSON string (freeing it first).
fn parse_to_json(source: &[u8], config: &[u8]) -> String {
    // SAFETY: `source`/`config` point to valid, live byte slices for the
    // full lengths passed in.
    let ptr =
        unsafe { c_parser_parse(source.as_ptr(), source.len(), config.as_ptr(), config.len()) };
    assert!(!ptr.is_null(), "c_parser_parse returned NULL");
    // SAFETY: `ptr` is a valid NUL-terminated UTF-8 string returned by
    // `c_parser_parse`.
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("returned string is valid UTF-8")
        .to_owned();
    // SAFETY: `ptr` was returned by `c_parser_parse` and is not freed twice.
    unsafe { c_parser_free_string(ptr) };
    json
}

#[test]
fn free_null_is_a_noop() {
    // Must not crash; there is nothing else to assert.
    // SAFETY: `NULL` is explicitly documented as a valid no-op input.
    unsafe { c_parser_free_string(std::ptr::null_mut()) };
}

#[test]
fn parses_valid_c_into_json() {
    let json = parse_to_json(SOURCE, CONFIG);
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(value["version"], "1.0");
    assert_eq!(value["language"], "c");
    assert!(value["nodes"].is_array(), "nodes must be an array");
    assert!(value["errors"].is_array(), "errors must be an array");
}

#[test]
fn non_fatal_syntax_errors_do_not_return_null() {
    // A missing semicolon is a non-fatal syntax error: the parse succeeds and
    // the diagnostic lands in the JSON `errors` array (and as an ErrorNode).
    let bad = b"int main(void) { int x = 0\n";
    let json = parse_to_json(bad, CONFIG);
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert!(
        value["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "expected non-empty errors array, got: {json}"
    );
}

#[test]
fn rejects_invalid_config_json() {
    let bad: &[u8] = b"{ not json";
    // SAFETY: `SOURCE`/`bad` are valid, live slices.
    let ptr = unsafe { c_parser_parse(SOURCE.as_ptr(), SOURCE.len(), bad.as_ptr(), bad.len()) };
    assert!(ptr.is_null(), "invalid config JSON must fail with NULL");
    // NULL is safe to pass to free.
    unsafe { c_parser_free_string(ptr) };
}

#[test]
fn rejects_null_source_with_nonzero_len() {
    // SAFETY: a null `source_ptr` with `source_len == 1` is a documented
    // precondition violation, so this is a valid (must-not-crash) call.
    let ptr = unsafe { c_parser_parse(std::ptr::null(), 1, CONFIG.as_ptr(), CONFIG.len()) };
    assert!(ptr.is_null(), "null source with non-zero length must fail");
}

#[test]
fn rejects_null_config_with_nonzero_len() {
    // SAFETY: a null `config_ptr` with `config_len == 1` is a documented
    // precondition violation, so this is a valid (must-not-crash) call.
    let ptr = unsafe { c_parser_parse(SOURCE.as_ptr(), SOURCE.len(), std::ptr::null(), 1) };
    assert!(ptr.is_null(), "null config with non-zero length must fail");
}

#[test]
fn empty_source_is_valid() {
    // SAFETY: `source_len == 0`, so a null source pointer is permitted.
    let ptr = unsafe { c_parser_parse(std::ptr::null(), 0, CONFIG.as_ptr(), CONFIG.len()) };
    assert!(
        !ptr.is_null(),
        "empty source must parse to an empty translation unit"
    );
    unsafe { c_parser_free_string(ptr) };
}

#[test]
fn empty_config_is_valid_and_ignored() {
    // A zero-length config is treated as "no config" and must not fail.
    let json = parse_to_json(SOURCE, b"");
    assert!(json.contains("\"language\":\"c\""));
}
