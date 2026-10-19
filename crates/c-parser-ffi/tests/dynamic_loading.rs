//! Dynamic-loading test: proves `c-parser-ffi` works when loaded as a `cdylib`
//! through the same `libloading`/`dlopen` mechanism a C/C++/Go host would use.

use std::ffi::{c_char, CStr};
use std::path::PathBuf;

/// Locate the `cdylib` built by Cargo, cross-platform.
fn cdylib_path() -> PathBuf {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // During `cargo test`, the cdylib is emitted next to the test binary in
    // `target/<profile>/deps/`; also try its parent (`target/<profile>/`).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(deps) = exe.parent() {
            dirs.push(deps.to_path_buf());
            if let Some(profile_dir) = deps.parent() {
                dirs.push(profile_dir.to_path_buf());
            }
        }
    }

    // Fall back to the workspace target dir derived from the manifest path,
    // checking both profile names (the profile is not exposed to tests at
    // compile time).
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = manifest.join("../../target");
    for profile in ["debug", "release"] {
        dirs.push(target.join(profile).join("deps"));
        dirs.push(target.join(profile));
    }

    const LIB_NAMES: [&str; 3] = [
        "libc_parser_ffi.so",    // Linux
        "libc_parser_ffi.dylib", // macOS
        "c_parser_ffi.dll",      // Windows
    ];

    for dir in &dirs {
        for name in LIB_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    panic!(
        "cdylib not found; searched: {}",
        dirs.iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn loads_and_parses_through_dynamic_boundary() {
    let path = cdylib_path();

    // SAFETY: the library is loaded for the duration of this scope; symbols
    // are only called while it is alive.
    unsafe {
        let lib = libloading::Library::new(&path).expect("failed to load cdylib");

        let parse: libloading::Symbol<
            unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> *mut c_char,
        > = lib.get(b"c_parser_parse").expect("c_parser_parse symbol");
        let free: libloading::Symbol<unsafe extern "C" fn(*mut c_char)> = lib
            .get(b"c_parser_free_string")
            .expect("c_parser_free_string symbol");

        let source = b"int add(int a, int b) { return a + b; }\n";
        let config = b"{}";

        // Paired call.
        let out = parse(source.as_ptr(), source.len(), config.as_ptr(), config.len());
        assert!(!out.is_null(), "c_parser_parse returned NULL");
        let json = CStr::from_ptr(out)
            .to_str()
            .expect("valid UTF-8")
            .to_owned();
        free(out);

        assert!(
            json.contains("\"language\":\"c\""),
            "unexpected JSON: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["version"], "1.0");

        // NULL is a no-op through the dynamic boundary as well.
        free(std::ptr::null_mut());
    }
}
