//! Zero-copy string representation inside AST nodes.
//!
//! [`TextSlice`] is the key type that enables lazy materialization: during
//! parsing every string payload is stored as a `Span` into the source buffer
//! (zero-copy).  Only at serialization time or after deserialization from JSON
//! does the text become a detached owned string.
//!
//! # Behaviour
//!
//! - **`Span(Span)`** — built by the parser.  Materialize on demand with
//!   [`materialize`](TextSlice::materialize).
//! - **`Detached(Box<str>)`** — built by [`TextSlice::deserialize`] when
//!   reading JSON.  No source buffer needed.
//!
//! # `null` handling in optional fields
//!
//! `TextSlice` does NOT implement `Deserialize` for `null` — that is handled
//! by `Option<TextSlice>` at the `serde` framework level.  See
//! `docs/api.md` §4.5 for the full contract.

use crate::ast::Span;
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt;

// ---------------------------------------------------------------------------
// TextSlice
// ---------------------------------------------------------------------------

/// Internal string reference: either a byte range into the source buffer
/// (`Span`) or a detached, already-materialized string (`Detached`).
///
/// **Note:** this type does **not** implement `Serialize` / `Deserialize`
/// directly, because serialization requires reifying the byte slice into a
/// lossy UTF-8 string.  That work happens inside `SerializableData`
/// (serialize) and via the custom `Deserialize` below.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextSlice {
    /// Zero-copy reference into the source buffer.  Used exclusively during
    /// parsing and in-memory operations.
    Span(Span),
    /// Already-owning string (from JSON deserialization or programmatic
    /// construction).  Format: `Box<str>` to avoid the 8-byte `capacity`
    /// overhead of `String`.
    Detached(Box<str>),
}

impl TextSlice {
    /// Build a zero-copy slice from a byte range.
    pub fn from_span(start: u32, end: u32) -> Self {
        Self::Span(Span { start, end })
    }

    /// Build a detached slice from an owned string.
    pub fn from_string(s: String) -> Self {
        Self::Detached(s.into_boxed_str())
    }

    /// Build a detached slice from a `&str`.
    pub fn from_static_str(s: &str) -> Self {
        Self::Detached(s.to_owned().into_boxed_str())
    }

    /// Materialize the text.
    ///
    /// - `Span` → `String::from_utf8_lossy(&source[span])` (never panics).
    /// - `Detached` → borrowed `&str`.
    pub fn materialize<'a>(&'a self, source: &'a [u8]) -> Cow<'a, str> {
        match self {
            TextSlice::Span(s) => {
                String::from_utf8_lossy(&source[s.start as usize..s.end as usize])
            }
            TextSlice::Detached(s) => Cow::Borrowed(s),
        }
    }

    /// Return `true` if this is a `Span` variant.
    pub fn is_span(&self) -> bool {
        matches!(self, TextSlice::Span(_))
    }
}

// ---------------------------------------------------------------------------
// Deserialize — ONLY called when the JSON value is a string.
// `null` is intercepted by `Option<TextSlice>::deserialize` before we're
// invoked, so we never see `null`.  See docs/api.md §4.5.
// ---------------------------------------------------------------------------

impl<'de> Deserialize<'de> for TextSlice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TextSliceVisitor;

        impl<'de> Visitor<'de> for TextSliceVisitor {
            type Value = TextSlice;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<TextSlice, E> {
                Ok(TextSlice::from_static_str(value))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<TextSlice, E> {
                Ok(TextSlice::from_string(value))
            }
        }

        deserializer.deserialize_string(TextSliceVisitor)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_materialize_valid_utf8() {
        let src = b"hello world";
        let ts = TextSlice::from_span(0, 5);
        assert_eq!(ts.materialize(src), "hello");
    }

    #[test]
    fn span_materialize_invalid_utf8_does_not_panic() {
        let src: &[u8] = &[0xFF, 0xFE, 0xFD];
        let ts = TextSlice::from_span(0, 3);
        let result = ts.materialize(src);
        // Should contain replacement characters
        assert!(result.contains('\u{FFFD}'));
    }

    #[test]
    fn detached_materialize_returns_str() {
        let ts = TextSlice::from_static_str("hello");
        let empty_src: &[u8] = &[];
        assert_eq!(ts.materialize(empty_src), "hello");
    }

    #[test]
    fn deserialize_from_json() {
        let json = r#""hello world""#;
        let ts: TextSlice = serde_json::from_str(json).unwrap();
        assert!(matches!(ts, TextSlice::Detached(_)));
        assert_eq!(ts.materialize(&[]), "hello world");
    }

    #[test]
    fn option_text_slice_null_is_none() {
        let json = r#"null"#;
        let ts: Option<TextSlice> = serde_json::from_str(json).unwrap();
        assert!(ts.is_none());
    }

    #[test]
    fn option_text_slice_string_is_some() {
        let json = r#""hello""#;
        let ts: Option<TextSlice> = serde_json::from_str(json).unwrap();
        assert!(ts.is_some());
        assert_eq!(ts.unwrap().materialize(&[]), "hello");
    }
}