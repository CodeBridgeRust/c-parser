//! JSON Schema validation tests for the AST output.
//!
//! Validates that `ast_to_json` output conforms to `docs/ast-schema.json`
//! (Draft 2020-12) and checks invariants that pure JSON Schema cannot express
//! (span bounds, index referential integrity, per-kind `data` pairing).
//!
//! ## Scaffold status
//!
//! The actual validation tests are implemented in Stage 9. This file currently
//! only provides a placeholder test target so the workspace compiles.
