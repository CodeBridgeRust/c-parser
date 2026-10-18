//! End-to-end tests for the `c-parser-cli` binary.
//!
//! Covers `parse` and `parse-incremental` subcommands, output options, dialect /
//! preprocessing flags and exit codes (0/1/2).
//!
//! ## Scaffold status
//!
//! The e2e tests are implemented in Stage 9. This file currently only provides
//! a placeholder test target so the workspace compiles.