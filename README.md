# c-parser 🦀

> *Zero-copy, arena-based C language parser with lazy JSON serialization*

<p align="center">
  <a href="README.md">🇬🇧 English</a> &nbsp;|&nbsp;
  <a href="README.ru.md">🇷🇺 Русский</a>
</p>

<p align="center">
  <a href="https://github.com/Fishman/c-parser/actions/workflows/ci.yml"><img src="https://github.com/Fishman/c-parser/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://crates.io/crates/c-parser"><img src="https://img.shields.io/crates/v/c-parser.svg" alt="crates.io version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="License: GPL-3.0-or-later"></a>
</p>

---

## Overview

`c-parser` performs lexical and syntactic analysis of C source code (C89–C17) and builds an **arena-based AST** that can be serialized to a self-contained JSON format (see [`docs/ast-schema.json`](docs/ast-schema.json) v1.0). The crate is designed as part of the `code-bridge` transpilation ecosystem but works as a standalone tool.

**Key innovations:**
- **Zero-copy lexing** — tokens store only a class and a `Span`; string data stays in the source buffer.
- **Arena-based AST** — nodes live in a flat `Vec<Node>`, linked by 32-bit `NodeId` indices. O(1) insertion, spatial locality, stable indices.
- **Lazy materialization** — string payloads (`name`, `value`, `op`, etc.) are extracted from the source buffer only at JSON serialization time via `String::from_utf8_lossy`.
- **Two-phase parsing** — recursive descent for declarations/statements and **Pratt parsing** for expressions (O(n) linear time, bounded stack).
- **Incremental mode** — in-memory AST updates via a sorted `Span → NodeId` index (O(log N + K)).
- **Error recovery** — syntax errors produce `ErrorNode` entries in the AST; the parser never panics on malformed input.

## Project status

All development stages are complete. Release **v1.0.0** is ready for publication.

| Stage | Description                                 | Status |
|:-----:|:--------------------------------------------|:------:|
|   1   | Workspace scaffold & documentation          |   ✅   |
|   2   | Zero-copy lexer (BOM, comments, all tokens) |   ✅   |
|   3   | Arena-based AST & JSON serialization        |   ✅   |
|   4   | Parser: recursive descent + Pratt           |   ✅   |
|   5   | Incremental mode (in-memory)                |   ✅   |
|   6   | CLI wrapper                                 |   ✅   |
|   7   | FFI layer (JSON-only cdylib)                |   ✅   |
|   8   | Integration examples                        |   ✅   |
|   9   | Extended test suite                         |   ✅   |
|  10   | Benchmarks & optimization                   |   ✅   |
|  11   | Final verification & release                |   ✅   |

- ✅ 282 tests pass (lexer, AST, parser, serialization, incremental, CLI e2e, FFI safety, schema validation, property tests, snapshots)
- ✅ `cargo clippy --workspace --all-targets -- -D warnings` is clean
- ✅ `cargo fmt --all -- --check` is clean
- ✅ Public parser types are `Send + Sync` (compile-time asserted)
- ✅ CI runs `test`, `clippy`, `fmt` on Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
- ✅ `struct`/`union`/`enum`/`typedef` declarations
- ✅ Function definitions with parameters
- ✅ All C statements (`if`, `while`, `do`-`while`, `for`, `switch`, `return`, `break`, `continue`, `goto`, labels)
- ✅ Pratt expression parsing — 15 precedence levels, left/right associativity
- ✅ Preprocessor directive nodes in `SingleTu` mode
- ✅ Error recovery with `ErrorNode` and `errors[]` array
- ✅ Public API via `create_parser` → `ParserPlugin` trait
- ✅ JSON round-trip (`ast_to_json` / `ast_from_json`)

## Installation

### As a library (Cargo)

Add `c-parser` to your `Cargo.toml`:

```toml
[dependencies]
c-parser = "1.0.0"
```

Or pin to the release tag for `code-bridge` integration:

```toml
[dependencies]
c-parser = { git = "https://github.com/Fishman/c-parser", tag = "v1.0.0" }
```

### CLI (`c-parser-cli`)

```bash
cargo install c-parser-cli
```

Or build from source:

```bash
git clone https://github.com/Fishman/c-parser
cd c-parser
cargo install --path crates/c-parser-cli
```

## Usage

### Library (Rust)

```rust
use c_parser::{create_parser, ast_to_json, ParseInput, ParserConfig};

fn parse_and_serialize(source: &[u8]) -> Result<String, c_parser::ParseError> {
    let parser = create_parser(ParserConfig::default());
    let result = parser.parse(ParseInput {
        source,
        filename: Some("example.c"),
        config: ParserConfig::default(),
    })?;

    let json = ast_to_json(&result.ast, source)?;
    Ok(json)
}
```

For in-memory incremental updates (LSP-style):

```rust
use c_parser::{incremental::IncrementalSession, ParseInput, ParserConfig, Span};

let source: Vec<u8> = std::fs::read("file.c")?;
let mut session = IncrementalSession::new(ParseInput {
    source: &source,
    filename: Some("file.c"),
    config: ParserConfig::default(),
})?;

// Replace bytes 42..47 with "+ 1"
session.apply_edit(Span { start: 42, end: 47 }, b"+ 1")?;
let json = session.to_json()?;
```

### CLI

```bash
# Full parse → pretty JSON to stdout
c-parser-cli parse example.c --pretty

# Save to file, choose a dialect
c-parser-cli parse example.c --dialect c17 --pretty -o ast.json

# Raw translation unit (record #-directives as AST nodes)
c-parser-cli parse example.c --preprocessing single-tu

# Incremental edit: replace bytes 10..20
c-parser-cli parse-incremental example.c --start 10 --end 20
```

## Structure

```
c-parser/
├── .github/workflows/ci.yml      # CI: test + clippy + fmt on 5 platform targets
├── docs/                         # Architecture, API contract, AST schema
│   ├── architecture.md
│   ├── api.md
│   ├── ast-schema.json
│   ├── parser-algorithm.md
│   ├── benchmarking.md
│   └── integration-with-code-bridge.md
├── crates/
│   ├── c-parser/                 # Core library crate
│   │   └── src/
│   │       ├── arena.rs          # Arena storage (Vec<Node>, NodeId = u32)
│   │       ├── ast/              # AST types: Node, NodeData, Span, TextSlice
│   │       ├── error.rs          # ParseError, Severity
│   │       ├── lexer/            # Zero-copy C lexer
│   │       ├── parser/           # Two-phase parser (recursive descent + Pratt)
│   │       ├── serialize/        # JSON serialization with lazy materialization
│   │       ├── index/            # Span→NodeId index (O(log N) lookup)
│   │       └── incremental/      # In-memory incremental updates
│   ├── c-parser-cli/             # CLI binary
│   └── c-parser-ffi/             # FFI cdylib (JSON-only)
├── examples/                     # Usage examples
├── tests/                        # Integration, e2e, schema, property, snapshot tests
├── README.md
└── README.ru.md
```

## Quick start (development)

```bash
# Build the workspace
cargo build --workspace

# Run all tests (282 tests)
cargo test --workspace

# Format & lint
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings

# Run benchmarks
cargo bench -p c-parser
```

## Documentation

- [`docs/parser-algorithm.md`](docs/parser-algorithm.md) – hybrid parser algorithm (recursive descent + Pratt)
- [`docs/api.md`](docs/api.md) – full API contract for `code-bridge` integration
- [`docs/ast-schema.json`](docs/ast-schema.json) – JSON AST schema v1.0 (Draft 2020-12)
- [`docs/architecture.md`](docs/architecture.md) – architecture overview
- [`docs/benchmarking.md`](docs/benchmarking.md) – performance targets and measurements
- [`docs/integration-with-code-bridge.md`](docs/integration-with-code-bridge.md) – `code-bridge` integration guide

## Compatibility

- **Rust:** stable ≥ 1.75
- **OS:** Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
- **AST schema:** `docs/ast-schema.json` v1.0

## License

[GNU General Public License v3.0](LICENSE)