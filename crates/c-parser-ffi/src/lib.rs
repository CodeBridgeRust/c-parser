//! FFI layer (`c-parser-ffi`) for dynamic loading from C/C++/Go.
//!
//! The FFI boundary exchanges *only* JSON strings via C-compatible pointer +
//! length types. Complex types (`Ast`, `Vec<Node>`, the arena) never cross the
//! dynamic-library boundary — this keeps the ABI stable and the memory safe.
//!
//! ## Exported surface
//!
//! - [`c_parser_parse`] — parse C source and return a JSON AST string, or
//!   `NULL` on failure.
//! - [`c_parser_free_string`] — free a string returned by [`c_parser_parse`].
//!
//! ## Memory contract
//!
//! [`c_parser_parse`] allocates the returned string with Rust's allocator via
//! [`CString::into_raw`]. The caller **must** release it exclusively with
//! [`c_parser_free_string`] (which reconstructs the `CString` through
//! [`CString::from_raw`]). Mixing with the system `free()`/`delete` is
//! undefined behavior. Passing `NULL` to [`c_parser_free_string`] is a no-op.
//!
//! ## Configuration
//!
//! The `config_json` argument is reserved for future use and is currently
//! **ignored** (the default [`ParserConfig`] is used). When a non-empty
//! configuration is supplied, it must be well-formed JSON, otherwise
//! [`c_parser_parse`] fails and returns `NULL`.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;

use c_parser::{ast_to_json, create_parser, ParseInput, ParserConfig};

/// Parse C source code and return a JSON AST string, or `NULL` on failure.
///
/// The returned string is a self-contained JSON document (schema v1.0). It is
/// allocated by Rust's allocator and must be freed with
/// [`c_parser_free_string`] and nothing else.
///
/// Non-fatal syntax errors do **not** cause `NULL` to be returned: they are
/// reported inside the JSON document's `errors` array and as `ErrorNode`
/// entries, exactly like the library API. `NULL` is returned only for fatal
/// problems: an invalid pointer/length combination, malformed configuration
/// JSON, or a fatal parse error.
///
/// # Safety
///
/// - `source_ptr` must be valid for reads of `source_len` bytes, or `NULL`
///   when `source_len == 0`.
/// - `config_ptr` must be valid for reads of `config_len` bytes, or `NULL`
///   when `config_len == 0`.
/// - The returned pointer must be freed with [`c_parser_free_string`].
#[no_mangle]
pub unsafe extern "C" fn c_parser_parse(
    source_ptr: *const u8,
    source_len: usize,
    config_ptr: *const u8,
    config_len: usize,
) -> *mut c_char {
    // A panic must never unwind across the FFI boundary: it is undefined
    // behavior. We contain every panic and translate it into a `NULL` return.
    match catch_unwind(AssertUnwindSafe(|| {
        parse_impl(source_ptr, source_len, config_ptr, config_len)
    })) {
        Ok(Some(ptr)) => ptr,
        Ok(None) | Err(_) => ptr::null_mut(),
    }
}

/// Non-panicking core of [`c_parser_parse`].
///
/// Returns `None` on any fatal failure, so the caller (which never unwinds)
/// can translate it to `NULL`.
fn parse_impl(
    source_ptr: *const u8,
    source_len: usize,
    config_ptr: *const u8,
    config_len: usize,
) -> Option<*mut c_char> {
    // Validate the reserved configuration. Its contents are currently ignored,
    // but the boundary contract is "only JSON strings", so a supplied config
    // must at least be well-formed JSON.
    if config_len > 0 {
        if config_ptr.is_null() {
            return None;
        }
        // SAFETY: `config_ptr` is non-null and valid for `config_len` bytes
        // (caller obligation).
        let config_bytes = unsafe { slice::from_raw_parts(config_ptr, config_len) };
        if serde_json::from_slice::<serde_json::Value>(config_bytes).is_err() {
            return None;
        }
    }

    // Build the source slice. An empty source is valid; a `NULL` pointer is
    // only tolerated when the length is zero.
    let source: &[u8] = if source_len == 0 {
        &[]
    } else {
        if source_ptr.is_null() {
            return None;
        }
        // SAFETY: `source_ptr` is non-null and valid for `source_len` bytes
        // (caller obligation).
        unsafe { slice::from_raw_parts(source_ptr, source_len) }
    };

    let config = ParserConfig::default();
    let parser = create_parser(config.clone());
    let result = parser
        .parse(ParseInput {
            source,
            filename: None,
            config,
        })
        .ok()?;

    let json = ast_to_json(&result.ast, source).ok()?;

    // serde_json escapes control bytes, so the JSON never contains an interior
    // NUL byte and `CString::new` succeeds. If it somehow does not, fail
    // instead of panicking.
    let cstring = CString::new(json).ok()?;
    Some(cstring.into_raw())
}

/// Free a string previously returned by [`c_parser_parse`].
///
/// Passing `NULL` is safe and is a no-op.
///
/// # Safety
///
/// `ptr` must be `NULL` or a pointer previously returned by
/// [`c_parser_parse`] and not yet freed. Passing any other pointer (including
/// memory from `malloc`) is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn c_parser_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `ptr` came from `CString::into_raw` in
    // `c_parser_parse`. Reconstructing the `CString` and dropping it returns
    // the memory to Rust's allocator — the same allocator that produced it.
    unsafe {
        drop(CString::from_raw(ptr));
    }
}
