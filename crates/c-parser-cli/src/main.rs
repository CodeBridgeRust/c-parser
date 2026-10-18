//! `c-parser-cli` — standalone CLI wrapper for the `c-parser` library.
//!
//! ## Scaffold status
//!
//! The `clap`-based CLI (`parse` / `parse-incremental` subcommands, `--dialect`,
//! `--preprocessing`, `--include-comments`, `--pretty`, `--log-level`,
//! `--config`, `-o/--output`) is implemented in Stage 6. This binary currently
//! only provides a placeholder entry point so the workspace compiles.

fn main() {
    // Stage 6: parse CLI arguments and delegate to `c_parser`.
}
