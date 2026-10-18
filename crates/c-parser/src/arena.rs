//! Arena-based AST storage.
//!
//! This is one of the key engineering innovations of the project: all AST nodes
//! are stored in a single flat `Vec<Node>`, linked through 32-bit `NodeId`
//! indices. This provides O(1) insertion, spatial locality, stable indices and
//! simple traversal/serialization, and it avoids the pointer/lifetime plumbing
//! of `Box`/`Rc`-based trees.
//!
//! ## Scaffold status
//!
//! The arena implementation (a typed wrapper around `Vec<Node>` with bounds
//! checking and a fatal `NodeId`-overflow guard) is delivered in Stage 3.
//! This file currently only documents the module's contract.
