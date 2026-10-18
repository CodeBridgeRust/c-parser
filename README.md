# c-parser

Zero-copy, arena-based parser for the C language.

## Overview

`c-parser` performs lexical and syntactic analysis of C source code and builds
an arena-based AST that can be serialized to a self-contained JSON format (see
`docs/ast-schema.json` v1.0). The crate is designed as part of the `code-bridge`
transpilation ecosystem but works as a standalone tool.

## Project status

**Scaffold stage** — the workspace structure, module tree, and public API
skeleton are in place. Business logic is implemented in subsequent stages:

| Stage | Description |
|-------|-------------|
| 1 | ✅ Workspace scaffold & documentation (this stage) |
| 2 | Zero-copy lexer |
| 3 | Arena-based AST & serialization |
| 4 | Parser (recursive descent + Pratt) |
| 5 | Incremental mode (in-memory) |
| 6 | CLI wrapper |
| 7 | FFI layer |
| 8 | Integration examples |
| 9 | Test suite |
| 10 | Benchmarks & optimization |
| 11 | Final verification & release |

## Structure

```
c-parser/
├── docs/                    # Architecture, API contract, AST schema
├── crates/
│   ├── c-parser/            # Core library crate
│   ├── c-parser-cli/        # CLI binary
│   └── c-parser-ffi/        # Optional FFI (cdylib)
├── tests/                   # Integration & e2e tests
├── examples/                # Usage examples
└── TechnicalSpecification.md
```

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — architecture overview
- [`docs/api.md`](docs/api.md) — API contract for `code-bridge` integration
- [`docs/ast-schema.json`](docs/ast-schema.json) — JSON AST schema v1.0
- [`docs/parser-algorithm.md`](docs/parser-algorithm.md) — parsing algorithm
- [`docs/integration-with-code-bridge.md`](docs/integration-with-code-bridge.md) — integration guide

## Quick start

```bash
cargo check --workspace       # verify the scaffold compiles
cargo test --workspace        # run all tests (once implemented)
cargo fmt --all               # format everything
cargo clippy --workspace      # lint everything
```

## License

MIT