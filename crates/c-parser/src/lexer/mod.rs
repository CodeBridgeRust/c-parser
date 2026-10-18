//! Zero-copy lexer.
//!
//! Tokenizes the source buffer into a stream of `{ class, span }` tokens without
//! copying any string data. Handles all C tokens (identifiers, keywords,
//! literals with C11 encoding prefixes, operators, separators), comments, and —
//! in `SingleTu` mode — preprocessor directives as AST nodes.
//!
//! ## Scaffold status
//!
//! The tokenizer and BOM normalization are implemented in Stage 2. This module
//! currently only documents its contract.
