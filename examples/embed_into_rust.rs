//! Static embedding example: parse a C file and serialize the AST to JSON.
//!
//! Usage: `cargo run --example embed_into_rust -- path/to/file.c`
//!
//! This demonstrates the static-integration workflow from `docs/api.md` §8:
//!
//! 1. create a parser with [`c_parser::create_parser`];
//! 2. parse with [`c_parser::ParserPlugin::parse`];
//! 3. report non-fatal diagnostics via [`c_parser::ParseResult::errors`];
//! 4. serialize the AST with [`c_parser::ast_to_json`].
//!
//! The example depends only on the `c-parser` crate and the standard library.
//! No internal types (`Ast`, arena, `Span`, `NodeId`) leak outside the
//! library calls — the only exchange is a self-contained JSON string.

use std::process::ExitCode;

use c_parser::{ast_to_json, create_parser, ParseInput, ParserConfig};

fn main() -> ExitCode {
    let path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: cargo run --example embed_into_rust -- <file.c>");
            return ExitCode::from(2);
        }
    };

    // Read the source as raw bytes — it does not need to be valid UTF-8.
    let source = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: cannot read `{path}`: {err}");
            return ExitCode::from(1);
        }
    };

    // 1. Create a parser (default config: C11 dialect, preprocessed input).
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());

    // 2. Parse. `Err` signals a *fatal* error only; non-fatal syntax errors
    //    are reported inside `result.ast.errors` (see step 3).
    let result = match parser.parse(ParseInput {
        source: &source,
        filename: Some(&path),
        config,
    }) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: fatal parse error: {err}");
            return ExitCode::from(1);
        }
    };

    // 3. Diagnostics — reported separately from the AST.
    for err in result.errors() {
        eprintln!(
            "{}:{}: {} ({}..{})",
            path, err.code, err.message, err.span.start, err.span.end
        );
    }

    // 4. Serialize to a self-contained JSON document. `source` is required
    //    for lazy materialization of string payloads (identifiers, literals).
    match ast_to_json(&result.ast, &source) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: JSON serialization failed: {err}");
            ExitCode::from(1)
        }
    }
}
