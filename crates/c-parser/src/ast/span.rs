//! Additional unit tests for the `Span` type.
//!
//! Lives alongside the `span` module definition for direct access.

#[cfg(test)]
mod tests {
    use crate::ast::Span;

    #[test]
    fn span_default_clone_eq() {
        let s1 = Span { start: 0, end: 10 };
        let s2 = s1;
        assert_eq!(s1, s2);
        let s3 = s1.clone();
        assert_eq!(s1, s3);
    }

    #[test]
    fn span_debug_format() {
        let s = Span { start: 5, end: 42 };
        let d = format!("{:?}", s);
        assert!(d.contains("5"));
        assert!(d.contains("42"));
    }

    #[test]
    fn span_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Span { start: 0, end: 10 });
        assert!(set.contains(&Span { start: 0, end: 10 }));
        assert!(!set.contains(&Span { start: 0, end: 5 }));
    }

    #[test]
    fn span_partial_eq_reflexive() {
        let s = Span { start: 3, end: 7 };
        assert_eq!(s, s);
    }

    #[test]
    fn span_serialize_deserialize() {
        let s = Span { start: 10, end: 20 };
        let json = serde_json::to_string(&s).unwrap();
        let d: Span = serde_json::from_str(&json).unwrap();
        assert_eq!(s, d);
    }

    #[test]
    fn span_zero_length() {
        let s = Span { start: 5, end: 5 };
        assert_eq!(s.start, s.end);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"start\":5"));
        assert!(json.contains("\"end\":5"));
    }
}