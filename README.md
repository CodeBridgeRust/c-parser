# c-parser 🦀

> *Zero-copy, arena-based C language parser with lazy JSON serialization*

<p align="center">
  <a href="README.md">🇬🇧 English</a> &nbsp;|&nbsp;
  <a href="README.ru.md">🇷🇺 Русский</a>
</p>

---

## Overview

`c-parser` performs lexical and syntactic analysis of C source code (C89–C17) and builds an **arena-based AST** that can be serialized to a self-contained JSON format (see [`docs/ast-schema.json`](docs/ast-schema.json) v1.0). The crate is designed as part of the `code-bridge` transpilation ecosystem but works as a standalone tool.

**Key innovations:**
- **Zero-copy lexing** — tokens store only a class and a `Span`; string data stays in the source buffer.
- **Arena-based AST** — nodes live in a flat `Vec<Node>`, linked by 32-bit `NodeId` indices. O(1) insertion, spatial locality, stable indices.
- **Lazy materialization** — string payloads (`name`, `value`, `op`, etc.) are extracted from the source buffer only at JSON serialization time via `String::from_utf8_lossy`.
- **Two-phase parsing** — recursive descent for declarations/statements and **Pratt parsing** for expressions (O(n) linear time, bounded stack).
- **Error recovery** — syntax errors produce `ErrorNode` entries in the AST; the parser never panics on malformed input.

## Project status

| Stage | Description                                 | Status |
|:-----:|:--------------------------------------------|:------:|
|   1   | Workspace scaffold & documentation          |   ✅   |
|   2   | Zero-copy lexer (BOM, comments, all tokens) |   ✅   |
|   3   | Arena-based AST & JSON serialization        |   ✅   |
|   4   | **Parser: recursive descent + Pratt**       |   ✅   |
|   5   | Incremental mode (in-memory)                |   ⬜   |
|   6   | CLI wrapper                                 |   ⬜   |
|   7   | FFI layer                                   |   ⬜   |
|   8   | Integration examples                        |   ⬜   |
|   9   | Extended test suite                         |   ⬜   |
|  10   | Benchmarks & optimization                   |   ⬜   |
|  11   | Final verification & release                |   ⬜   |

### What's implemented (Stage 4)

| Component                  | File                                     | Description                                            |
|:---------------------------|:-----------------------------------------|:-------------------------------------------------------|
| Symbol table               | `src/parser/symbol_table.rs`             | Scope-based table for `typedef`-vs-`identifier`        |
| Error helpers              | `src/parser/errors.rs`                   | `E_SYNTAX_*` error codes                               |
| Core parser                | `src/parser/mod.rs`                      | Token stream, arena bridge, error recovery             |
| Phase A (decl & stmts)     | `src/parser/phase_a.rs`                  | Recursive descent — functions, structs, `if`, `for`... |
| Phase B (expressions)      | `src/parser/phase_b.rs`                  | Pratt parsing — all C operators, O(n) linear time       |

- ✅ 95 tests pass (lexer, AST, parser, serialization)
- ✅ `struct`/`union`/`enum`/`typedef` declarations
- ✅ Function definitions with parameters
- ✅ All C statements (`if`, `while`, `do`-`while`, `for`, `switch`, `return`, `break`, `continue`, `goto`, labels)
- ✅ Pratt expression parsing — 15 precedence levels, left/right associativity
- ✅ Preprocessor directive nodes in `SingleTu` mode
- ✅ Error recovery with `ErrorNode` and `errors[]` array
- ✅ Public API via `create_parser` → `ParserPlugin` trait
- ✅ JSON round-trip (`ast_to_json` / `ast_from_json`)

## Structure

```
c-parser/
├── docs/                          # Architecture, API contract, AST schema
│   ├── architecture.md
│   ├── api.md
│   ├── ast-schema.json
│   └── parser-algorithm.md
├── crates/
│   ├── c-parser/                  # Core library crate
│   │   └── src/
│   │       ├── arena.rs           # Arena storage (Vec<Node>, NodeId = u32)
│   │       ├── ast/               # AST types: Node, NodeData, Span, TextSlice
│   │       ├── error.rs           # ParseError, Severity
│   │       ├── lexer/             # Zero-copy C lexer
│   │       ├── parser/            # Two-phase parser
│   │       │   ├── mod.rs         # Parser struct & token stream
│   │       │   ├── phase_a.rs     # Recursive descent
│   │       │   ├── phase_b.rs     # Pratt parsing
│   │       │   ├── symbol_table.rs
│   │       │   └── errors.rs
│   │       ├── serialize/         # JSON serialization with lazy materialization
│   │       ├── index/             # Span→NodeId index (Stage 5)
│   │       └── incremental/       # Incremental updates (Stage 5)
│   ├── c-parser-cli/              # CLI binary (Stage 6)
│   └── c-parser-ffi/              # FFI cdylib (Stage 7)
├── examples/                      # Usage examples
├── README.md
└── README.ru.md
```

## Quick start

```bash
# Build the library
cargo build -p c-parser

# Run all tests (95 tests)
cargo test -p c-parser

# Format & lint
cargo fmt --all
cargo clippy --workspace
```

## Rust API

```rust
use c_parser::{create_parser, ast_to_json, ParseInput, ParserConfig};

fn parse_and_serialize(source: &[u8]) -> anyhow::Result<String> {
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

## Documentation

- [`docs/parser-algorithm.md`](docs/parser-algorithm.md) – hybrid parser algorithm (recursive descent + Pratt)
- [`docs/api.md`](docs/api.md) – full API contract for `code-bridge` integration
- [`docs/ast-schema.json`](docs/ast-schema.json) – JSON AST schema v1.0 (Draft 2020-12)
- [`docs/architecture.md`](docs/architecture.md) – architecture overview

## License

MIT