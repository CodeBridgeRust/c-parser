//! Heap-allocation profile of a full parse (analysis phase only).
//!
//! Uses [`dhat`](https://docs.rs/dhat) to record every allocation performed
//! during lexing + parsing and reports the **peak** heap usage.  This directly
//! validates the memory target from ТЗ §6.4:
//!
//! > memory on the analysis phase ≤ 10× the size of the source file
//! > (excluding JSON serialization).
//!
//! It also proves the zero-copy invariant: an AST node stores only `Span`s and
//! 32-bit indices, so no per-token `String` is allocated during parsing (the
//! profile will show no `String::from` / `from_utf8` allocations in the
//! hot path — only the arena `Vec<Node>` growth, symbol-table scopes, and the
//! token/lookahead state).
//!
//! # Usage
//!
//! ```text
//! cargo run -p c-parser --example dhat_profile --profile release -- <bytes> [<path-to-c-file>]
//! ```
//!
//! - With a path argument, the file is read and profiled.
//! - With only a byte count, a deterministic ~1 MB fixture is generated.
//!
//! Afterwards `dhat-heap.json` is written; visualise it with `dhat/dh_view.html`.

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let source: Vec<u8> = if args.len() >= 3 {
        // A path to a real C file was given; profile that instead.
        let mut f = std::fs::File::open(&args[2]).expect("open source file");
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).expect("read source file");
        buf
    } else {
        let bytes: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1_048_576);
        generate_fixture(bytes)
    };

    let input_size = source.len();

    // Profile only the analysis phase (lexing + parsing).  We deliberately do
    // NOT serialize to JSON here, because serialization is excluded from the
    // 10× memory budget by the specification.
    let _profiler = dhat::Profiler::new_heap();
    let input = c_parser::ParseInput {
        source: &source,
        filename: Some("sqlite3.c"),
        config: c_parser::ParserConfig::default(),
    };
    let result = c_parser::parse(input).expect("parse must succeed");
    let nodes = result.ast.nodes.len();
    let errs = result.ast.errors.len();
    // Stats must be captured while the profiler is still running.
    let peak = dhat::HeapStats::get();

    let ratio = peak.max_bytes as f64 / input_size as f64;
    println!("== c-parser analysis-phase memory profile ==");
    println!("type_sizes        : Node={}B NodeData={}B TextSlice={}B",
             std::mem::size_of::<c_parser::Node>(),
             std::mem::size_of::<c_parser::NodeData>(),
             std::mem::size_of::<c_parser::text_slice::TextSlice>());
    println!("source_bytes       : {}", input_size);
    println!("ast_nodes          : {}", nodes);
    println!("ast_errors         : {}", errs);
    println!("peak_alloc_bytes   : {}", peak.max_bytes);
    println!("peak_alloc_blocks  : {}", peak.max_blocks);
    println!("total_alloc_bytes  : {}", peak.total_bytes);
    println!("total_alloc_blocks : {}", peak.total_blocks);
    println!("curr_alloc_bytes   : {}", peak.curr_bytes);
    println!("ratio (× source)   : {:.2}x", ratio);
    println!("within 10x budget  : {}", ratio <= 10.0);
}

/// Deterministic ~N-byte C translation unit (sqlite3.c-like token mix).
///
/// The generated code must be **valid, error-free C** so the benchmark and
/// memory profile reflect a realistic clean parse (no error-recovery churn in
/// the hot path).
fn generate_fixture(target_bytes: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(target_bytes + 4096);
    // Only constructs that the MVP parser handles without error are used, so
    // the profile reflects a realistic clean parse (not error-recovery churn).
    out.extend_from_slice(
        br#"
typedef unsigned int u32;
typedef signed int i32;
typedef unsigned char u8;
struct Table { u32 nCol; i32 flags; };
struct Column { u32 iType; i32 width; };
enum opcode { OP_Integer = 101, OP_String = 103, OP_Add = 104 };
"#,
    );
    let mut i: usize = 0;
    while out.len() < target_bytes {
        let body = format!(
            r#"
int helper_fn_{i}(u32 a, i32 b, i32 c) {{
    if (a > 100) {{ return a * c + (u32)b; }}
    if (c > 0) {{ return b * 100 + a * 10 + c; }}
    if (a > b) {{ return a - b; }}
    return a + b * 2 + c * 3 - 5;
}}

int main_{i}(i32 argc, i32 argv) {{
    (void)argv;
    if (argc > 0) {{ return helper_fn_{i}(argc, argc * 3, argc + 1); }}
    return argc * 2 + 1;
}}
"#,
            i = i
        );
        out.extend_from_slice(body.as_bytes());
        i += 1;
    }
    out.truncate(target_bytes);
    out
}