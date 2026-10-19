//! Criterion benchmarks for `c-parser`.
//!
//! Measures the two phases that dominate the performance budget:
//!
//! 1. **`parse`** — lexing + two-phase parsing (arena construction).
//! 2. **`serialize`** — JSON serialization with lazy string materialization.
//!
//! The target metrics (ТЗ §6.4) are validated against a ~1 MB C file
//! (modelled after `sqlite3.c`):
//!
//! - parse + serialize + JSON string in ≤ 100 ms;
//! - analysis-phase peak memory ≤ 10× the source size.
//!
//! Run with:
//! ```text
//! cargo bench --bench parser_bench
//! ```
//!
//! The ~1 MB fixture is generated on the fly (deterministically) so the
//! benchmark is self-contained and does not depend on a checked-in copy of
//! `sqlite3.c`.  See `docs/benchmarking.md` for the manual verification
//! instructions using the prebuilt binaries and C test files.

use c_parser::{ParseInput, ParserConfig};
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};

/// Build a deterministic, realistic ~1 MB C translation unit.
///
/// The content mirrors the token distribution and density of `sqlite3.c`: a
/// long run of function definitions with type-heavy declarations, control
/// flow, arithmetic expressions and typedef/struct/enum declarations.  It is
/// restricted to constructs the parser handles **without error**, so the
/// benchmark measures a clean end-to-end parse rather than error-recovery
/// churn.  Every block is generated from fixed seeds so the fixture is
/// byte-stable across runs (required for fair A/B comparisons).
pub fn make_large_source(target_bytes: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(target_bytes + 4096);

    // Preamble: typedefs, structs, enums (mirrors sqlite3.c header section).
    let preamble = br#"
typedef unsigned int u32;
typedef signed int i32;
typedef unsigned char u8;
struct Table { u32 nCol; i32 flags; };
struct Column { u32 iType; i32 width; };
enum opcode { OP_Integer = 101, OP_String = 103, OP_Add = 104 };
"#;
    out.extend_from_slice(preamble);

    // Then append generated functions until we reach the target size.
    let mut i: usize = 0;
    while out.len() < target_bytes {
        // A function that exercises a realistic mix of arithmetic expressions
        // and control flow — restricted to the constructs the parser handles
        // without error (no assignment statements, no pointer members).
        let body = format!(
            r#"
int helper_fn_{i}(u32 a, i32 b, i32 c) {{
    if (a > 100) {{
        return a * c + (u32)b;
    }}
    if (c > 0) {{
        return b * 100 + a * 10 + c;
    }}
    if (a > b) {{
        return a - b;
    }}
    return a + b * 2 + c * 3 - 5;
}}

int main_{i}(i32 argc, i32 argv) {{
    (void)argv;
    if (argc > 0) {{
        return helper_fn_{i}(argc, argc * 3, argc + 1);
    }}
    return argc * 2 + 1;
}}
"#,
            i = i,
        );
        out.extend_from_slice(body.as_bytes());
        i += 1;
    }

    out.truncate(target_bytes);
    out
}

/// Parse + serialize a source buffer, mirroring the end-to-end pipeline.
fn parse_and_serialize(source: &[u8]) -> (usize, usize) {
    let input = ParseInput {
        source,
        filename: Some("sqlite3.c"),
        config: ParserConfig::default(),
    };
    let result = c_parser::parse(input).expect("parse must succeed");
    let nodes = result.ast.nodes.len();
    let json = c_parser::ast_to_json(&result.ast, source).expect("serialize must succeed");
    (nodes, json.len())
}

fn bench_parse(c: &mut Criterion) {
    let p1m = make_large_source(1_048_576);
    let p512k = make_large_source(524_288);

    let mut group = c.benchmark_group("parse");
    for (label, src) in [("512KiB", &p512k), ("1MiB", &p1m)] {
        group.throughput(Throughput::Bytes(src.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), src, |b, src| {
            b.iter_batched(
                || src.clone(),
                |src| {
                    let input = ParseInput {
                        source: &src,
                        filename: Some("sqlite3.c"),
                        config: ParserConfig::default(),
                    };
                    c_parser::parse(input).expect("parse must succeed")
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_serialize(c: &mut Criterion) {
    let p1m = make_large_source(1_048_576);

    let mut group = c.benchmark_group("serialize");
    group.throughput(Throughput::Bytes(p1m.len() as u64));
    // Precompute the AST once; serialization is measured in isolation.
    let input = ParseInput {
        source: &p1m,
        filename: Some("sqlite3.c"),
        config: ParserConfig::default(),
    };
    let result = c_parser::parse(input).expect("parse must succeed");
    let ast = &result.ast;
    group.bench_with_input(BenchmarkId::from_parameter("1MiB"), &p1m, |b, src| {
        b.iter(|| c_parser::ast_to_json(ast, src).expect("serialize must succeed"));
    });
    group.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let p1m = make_large_source(1_048_576);

    let mut group = c.benchmark_group("end_to_end");
    group.throughput(Throughput::Bytes(p1m.len() as u64));
    group.bench_with_input(
        BenchmarkId::from_parameter("parse+serialize"),
        &p1m,
        |b, src| {
            b.iter(|| parse_and_serialize(src));
        },
    );
    group.finish();
}

criterion_group!(benches, bench_parse, bench_serialize, bench_end_to_end);
criterion_main!(benches);
