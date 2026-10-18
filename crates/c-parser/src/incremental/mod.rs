//! In-memory incremental AST updates.
//!
//! Given a changed byte range, finds the minimal container node via the
//! [`crate::index`] module, invalidates its subtree and re-parses only that
//! region, targeting O(log N + K) complexity. JSON is *not* serialized
//! automatically after an update — only on explicit request.
//!
//! ## Scaffold status
//!
//! Incremental updates are implemented in Stage 5. This module currently only
//! documents its contract.
