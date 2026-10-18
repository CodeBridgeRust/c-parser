//! Sorted `Span -> NodeId` index for incremental updates.
//!
//! Because nodes are appended to the arena in traversal order (not guaranteed
//! sorted by `span.start`), a plain binary search over `Vec<Node>` is
//! incorrect. This module maintains a separate sorted index (`Vec<(Span,
//! NodeId)>` for the MVP) that enables honest O(log N) container lookup.
//!
//! ## Scaffold status
//!
//! The index is implemented in Stage 5. This module currently only documents
//! its contract.
