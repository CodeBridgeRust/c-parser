//! JSON serialization with lazy materialization.
//!
//! Implements the `SerializableAst<'a> { ast, source }` wrapper pattern that
//! materializes `TextSlice` string payloads from the source buffer by `Span`
//! only at serialization time, producing a self-contained JSON document that
//! conforms to `docs/ast-schema.json` v1.0.
//!
//! ## Scaffold status
//!
//! Serialization (`ast_to_json`, `ast_from_json`) is implemented in Stage 3.
//! This module currently only documents its contract.
