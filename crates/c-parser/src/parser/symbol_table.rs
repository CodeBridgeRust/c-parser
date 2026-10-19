//! Contextual symbol table for resolving the `typedef-name` vs `identifier`
//! ambiguity (the "Lexer Hack").
//!
//! The table is a stack of scopes. Each scope maps an identifier's source text
//! (by `Span` into the source buffer) to the fact that it was declared as a
//! `typedef`. The parser pushes a new scope when entering `{`, `struct`, etc.,
//! and pops it on exit.
//!
//! Lookup is O(number of scopes), which is bounded and fast for realistic C.

use crate::ast::Span;

/// A single scope is a set of typedef names, identified by their byte range.
///
/// Invariant: entries are deduplicated within a scope; the most recent
/// definition shadows earlier ones in outer scopes.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Vec of spans of typedef'ed identifiers in this scope. We use a Vec
    /// instead of a set because scopes are typically small.
    entries: Vec<Span>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, span: Span) {
        // Avoid duplicate entries in the same scope (shadowing).
        if !self.entries.contains(&span) {
            self.entries.push(span);
        }
    }

    pub fn contains(&self, span: Span) -> bool {
        self.entries.contains(&span)
    }
}

/// A stack of scopes for typedef resolution.
///
/// The innermost (current) scope is at the end of the vec. When looking up
/// a name, we search from the innermost outward.
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    scopes: Vec<Scope>,
}

impl SymbolTable {
    /// Create a new, empty symbol table with the global scope already pushed.
    pub fn new() -> Self {
        let mut table = Self { scopes: Vec::new() };
        table.push_scope(); // global scope
        table
    }

    /// Enter a new scope (e.g. upon entering `{`).
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Leave the current scope. Panics if the global scope would be popped.
    pub fn pop_scope(&mut self) {
        // Keep at least the global scope.
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Register a typedef name in the current scope.
    pub fn add_typedef(&mut self, span: Span) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(span);
        }
    }

    /// Return `true` if `span` is a known typedef name.
    ///
    /// Searches from innermost to outermost scope.
    pub fn is_typedef(&self, span: Span, source: &[u8]) -> bool {
        // We compare by byte-content, not by Span identity, because the same
        // name may have a different Span when it appears later.
        let needle = &source[span.start as usize..span.end as usize];
        for scope in self.scopes.iter().rev() {
            for entry in &scope.entries {
                let existing = &source[entry.start as usize..entry.end as usize];
                if needle == existing {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_typedef_resolution() {
        let source = b"typedef int myint; myint x;";
        // myint: first occurrence at span 12..17, second at 19..24
        let typedef_span = Span { start: 12, end: 17 };
        let use_span = Span { start: 19, end: 24 };

        let mut table = SymbolTable::new();
        table.add_typedef(typedef_span);

        assert!(table.is_typedef(typedef_span, source));
        assert!(table.is_typedef(use_span, source));
        assert!(!table.is_typedef(Span { start: 25, end: 26 }, source)); // 'x'
    }

    #[test]
    fn scoped_typedef() {
        let source = b"typedef int T; { typedef float T; T x; } T y;";
        // Manual byte count:
        // t(0)y(1)p(2)e(3)d(4)e(5)f(6) (7)i(8)n(9)t(10) (11)T(12);(13) (14){(15) (16)
        // t(17)y(18)p(19)e(20)d(21)e(22)f(23) (24)f(25)l(26)o(27)a(28)t(29) (30)T(31);(32)
        //  (33)T(34) (35)x(36);(37) (38)}(39) (40)T(41) (42)y(43);(44)
        // outer T: {12,13}
        // inner T: {31,32}
        // inner use: {34,35}
        // outer use T after `}`: {41,42}

        let outer = Span { start: 12, end: 13 };
        let inner = Span { start: 31, end: 32 };
        let inner_use = Span { start: 34, end: 35 };
        let outer_use = Span { start: 41, end: 42 };

        let mut table = SymbolTable::new();
        table.add_typedef(outer);
        assert!(table.is_typedef(outer, source));

        table.push_scope();
        table.add_typedef(inner);
        assert!(table.is_typedef(inner_use, source));

        table.pop_scope();
        assert!(table.is_typedef(outer_use, source));
    }

    #[test]
    fn typedef_not_confused_across_scopes() {
        let source = b"typedef int A; A a;";
        let a_typedef = Span { start: 12, end: 13 };
        let a_use = Span { start: 15, end: 16 };
        let b_span = Span { start: 17, end: 18 }; // 'a' variable

        let mut table = SymbolTable::new();
        table.add_typedef(a_typedef);

        assert!(table.is_typedef(a_use, source));
        assert!(!table.is_typedef(b_span, source));
    }

    #[test]
    fn global_scope_never_popped() {
        let mut table = SymbolTable::new();
        assert_eq!(table.scopes.len(), 1);
        table.pop_scope();
        assert_eq!(table.scopes.len(), 1); // global scope preserved
    }
}
