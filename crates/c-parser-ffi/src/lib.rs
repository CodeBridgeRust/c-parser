//! Optional FFI layer (`c-parser-ffi`) for dynamic loading.
//!
//! The FFI boundary exchanges *only* JSON strings via C-compatible pointer +
//! length types; complex types (`Ast`, `Vec<Node>`, the arena) never cross the
//! dynamic-library boundary. The exported surface is:
//!
//! - `c_parser_parse(source_ptr, source_len, config_ptr, config_len) -> *mut c_char`
//! - `c_parser_free_string(ptr)`
//!
//! ## Scaffold status
//!
//! The `#[no_mangle] extern "C"` functions and their safe memory management
//! (`CString::into_raw` / `CString::from_raw`) are implemented in Stage 7. This
//! crate currently only provides the library skeleton so the workspace compiles.
