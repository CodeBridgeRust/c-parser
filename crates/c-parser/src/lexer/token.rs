//! Token types for the zero-copy C lexer.
//!
//! Defines [`TokenKind`] — the classification of every C token the lexer
//! produces — together with the zero-copy [`Token`] record `{ kind, span }`,
//! and the surrounding enums [`CDialect`] and [`PreprocessingMode`] that
//! control keyword sets and directive handling.

use crate::ast::Span;

// ---------------------------------------------------------------------------
// Dialect & preprocessing mode
// ---------------------------------------------------------------------------

/// C language dialect.
///
/// Controls which identifier sequences are recognised as keywords so that,
/// for instance, `_Atomic` is a keyword in C11/C17 but a plain identifier in
/// C89/C99.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CDialect {
    C89,
    C99,
    #[default]
    C11,
    C17,
}

/// Whether the input is already preprocessed or is a raw translation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PreprocessingMode {
    /// Input is already preprocessed (e.g. `gcc -E`); directives are absent.
    #[default]
    Preprocessed,
    /// Raw translation unit: `#` directives are emitted as tokens so the
    /// parser can fix them as AST nodes without expanding them.
    SingleTu,
}

// ---------------------------------------------------------------------------
// TokenKind
// ---------------------------------------------------------------------------

/// Classification of a single C token.
///
/// Every token the lexer yields carries exactly one `TokenKind`.  The kind is
/// all the parser needs to decide what grammatical rule to apply — the
/// lexeme's text stays in the source buffer and is reachable through the
/// token's [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // ---- Identifiers ----
    /// A user-defined identifier.
    Identifier,

    // ---- C89 keywords ----
    Auto,
    Break,
    Case,
    Char,
    Const,
    Continue,
    Default,
    Do,
    Double,
    Else,
    Enum,
    Extern,
    Float,
    For,
    Goto,
    If,
    Int,
    Long,
    Register,
    Return,
    Short,
    Signed,
    Sizeof,
    Static,
    Struct,
    Switch,
    Typedef,
    Union,
    Unsigned,
    Void,
    Volatile,
    While,

    // ---- C99 keywords ----
    /// `inline` (C99+)
    Inline,
    /// `restrict` (C99+)
    Restrict,
    /// `_Bool` (C99+)
    C99Bool,
    /// `_Complex` (C99+)
    C99Complex,
    /// `_Imaginary` (C99+)
    C99Imaginary,

    // ---- C11 keywords ----
    /// `_Alignas` (C11+)
    C11Alignas,
    /// `_Alignof` (C11+)
    C11Alignof,
    /// `_Atomic` (C11+)
    C11Atomic,
    /// `_Generic` (C11+)
    C11Generic,
    /// `_Noreturn` (C11+)
    C11Noreturn,
    /// `_Static_assert` (C11+)
    C11StaticAssert,
    /// `_Thread_local` (C11+)
    C11ThreadLocal,

    // ---- Literals ----
    /// Integer literal (decimal, hex `0x`, octal `0`), may carry an integer
    /// suffix.
    IntegerLiteral,
    /// Floating-point literal, may carry a float suffix (`f`, `F`, `l`, `L`).
    FloatLiteral,
    /// Character literal `'...'`, with optional encoding prefix `L`, `u`, `U`.
    CharLiteral,
    /// String literal `"..."`, with optional encoding prefix `L`, `u`, `U`,
    /// `u8`.
    StringLiteral,

    // ---- Single-char operators & punctuators ----
    Plus,     // `+`
    Minus,    // `-`
    Star,     // `*`
    Slash,    // `/`
    Percent,  // `%`
    Amp,      // `&`
    Pipe,     // `|`
    Caret,    // `^`
    Tilde,    // `~`
    Eq,       // `=`
    Bang,     // `!`
    Lt,       // `<`
    Gt,       // `>`
    Dot,      // `.`
    Question, // `?`
    Colon,    // `:`

    // ---- Double-char operators ----
    PlusPlus,   // `++`
    MinusMinus, // `--`
    Arrow,      // `->`
    LShift,     // `<<`
    RShift,     // `>>`
    LtEq,       // `<=`
    GtEq,       // `>=`
    EqEq,       // `==`
    NotEq,      // `!=`
    AmpAmp,     // `&&`
    PipePipe,   // `||`
    StarEq,     // `*=`
    SlashEq,    // `/=`
    PercentEq,  // `%=`
    PlusEq,     // `+=`
    MinusEq,    // `-=`
    AmpEq,      // `&=`
    PipeEq,     // `|=`
    CaretEq,    // `^=`
    LShiftEq,   // `<<=`
    RShiftEq,   // `>>=`

    // ---- Triple-char operators ----
    Ellipsis, // `...`

    // ---- Separators ----
    Semi,     // `;`
    Comma,    // `,`
    LParen,   // `(`
    RParen,   // `)`
    LBracket, // `[`
    RBracket, // `]`
    LBrace,   // `{`
    RBrace,   // `}`

    // ---- Preprocessor ----
    /// `#` — start of a preprocessor directive (emitted only in `SingleTu`
    /// mode).
    Hash,

    // ---- Comments (emitted only when `include_comments` is true) ----
    /// `// ...` line comment
    LineComment,
    /// `/* ... */` block comment
    BlockComment,

    // ---- Special ----
    /// End-of-file sentinel.
    Eof,
    /// Unrecognised / invalid token.
    Error,
}

impl TokenKind {
    /// Returns `true` for keyword variants that are valid in the given dialect.
    pub fn is_valid_in(self, dialect: CDialect) -> bool {
        match self {
            // C89 keywords — always valid
            Self::Auto
            | Self::Break
            | Self::Case
            | Self::Char
            | Self::Const
            | Self::Continue
            | Self::Default
            | Self::Do
            | Self::Double
            | Self::Else
            | Self::Enum
            | Self::Extern
            | Self::Float
            | Self::For
            | Self::Goto
            | Self::If
            | Self::Int
            | Self::Long
            | Self::Register
            | Self::Return
            | Self::Short
            | Self::Signed
            | Self::Sizeof
            | Self::Static
            | Self::Struct
            | Self::Switch
            | Self::Typedef
            | Self::Union
            | Self::Unsigned
            | Self::Void
            | Self::Volatile
            | Self::While => true,

            // C99 keywords — valid from C99 onward
            Self::Inline
            | Self::Restrict
            | Self::C99Bool
            | Self::C99Complex
            | Self::C99Imaginary => {
                matches!(dialect, CDialect::C99 | CDialect::C11 | CDialect::C17)
            }

            // C11 keywords — valid from C11 onward (includes C17)
            Self::C11Alignas
            | Self::C11Alignof
            | Self::C11Atomic
            | Self::C11Generic
            | Self::C11Noreturn
            | Self::C11StaticAssert
            | Self::C11ThreadLocal => {
                matches!(dialect, CDialect::C11 | CDialect::C17)
            }

            // Non-keyword tokens are always "valid"
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A single token — just a classification and a byte range into the
/// (BOM-stripped) source buffer.
///
/// **Zero-copy invariant:** the token does not own or borrow any string data.
/// The source text can be recovered later via `&source[token.span.start..token.span.end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    /// Construct a new token.
    pub fn new(kind: TokenKind, start: u32, end: u32) -> Self {
        Self {
            kind,
            span: Span { start, end },
        }
    }
}

// ---------------------------------------------------------------------------
// Keyword lookup (zero-copy)
// ---------------------------------------------------------------------------

/// If the byte slice matches a C keyword for the given `dialect`, return its
/// [`TokenKind`].  Otherwise return `None` (the lexer should emit
/// [`TokenKind::Identifier`]).
///
/// This function is the heart of the "lexer hack" escape hatch: it is *not*
/// called for identifiers that have already been resolved as `typedef` names
/// by the parser's symbol table.
pub fn keyword_from_bytes(bytes: &[u8], dialect: CDialect) -> Option<TokenKind> {
    let kw = match bytes {
        // --- C89 ---
        b"auto" => TokenKind::Auto,
        b"break" => TokenKind::Break,
        b"case" => TokenKind::Case,
        b"char" => TokenKind::Char,
        b"const" => TokenKind::Const,
        b"continue" => TokenKind::Continue,
        b"default" => TokenKind::Default,
        b"do" => TokenKind::Do,
        b"double" => TokenKind::Double,
        b"else" => TokenKind::Else,
        b"enum" => TokenKind::Enum,
        b"extern" => TokenKind::Extern,
        b"float" => TokenKind::Float,
        b"for" => TokenKind::For,
        b"goto" => TokenKind::Goto,
        b"if" => TokenKind::If,
        b"int" => TokenKind::Int,
        b"long" => TokenKind::Long,
        b"register" => TokenKind::Register,
        b"return" => TokenKind::Return,
        b"short" => TokenKind::Short,
        b"signed" => TokenKind::Signed,
        b"sizeof" => TokenKind::Sizeof,
        b"static" => TokenKind::Static,
        b"struct" => TokenKind::Struct,
        b"switch" => TokenKind::Switch,
        b"typedef" => TokenKind::Typedef,
        b"union" => TokenKind::Union,
        b"unsigned" => TokenKind::Unsigned,
        b"void" => TokenKind::Void,
        b"volatile" => TokenKind::Volatile,
        b"while" => TokenKind::While,

        // --- C99 ---
        b"inline" => TokenKind::Inline,
        b"restrict" => TokenKind::Restrict,
        b"_Bool" => TokenKind::C99Bool,
        b"_Complex" => TokenKind::C99Complex,
        b"_Imaginary" => TokenKind::C99Imaginary,

        // --- C11 ---
        b"_Alignas" => TokenKind::C11Alignas,
        b"_Alignof" => TokenKind::C11Alignof,
        b"_Atomic" => TokenKind::C11Atomic,
        b"_Generic" => TokenKind::C11Generic,
        b"_Noreturn" => TokenKind::C11Noreturn,
        b"_Static_assert" => TokenKind::C11StaticAssert,
        b"_Thread_local" => TokenKind::C11ThreadLocal,

        _ => return None,
    };

    // Only emit a keyword token when the keyword is valid for this dialect.
    if kw.is_valid_in(dialect) {
        Some(kw)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c89_keywords_always_valid() {
        let c89_kws: &[&[u8]] = &[
            b"auto",
            b"break",
            b"case",
            b"char",
            b"const",
            b"continue",
            b"default",
            b"do",
            b"double",
            b"else",
            b"enum",
            b"extern",
            b"float",
            b"for",
            b"goto",
            b"if",
            b"int",
            b"long",
            b"register",
            b"return",
            b"short",
            b"signed",
            b"sizeof",
            b"static",
            b"struct",
            b"switch",
            b"typedef",
            b"union",
            b"unsigned",
            b"void",
            b"volatile",
            b"while",
        ];
        for &kw in c89_kws {
            assert!(keyword_from_bytes(kw, CDialect::C89).is_some(), "{:?}", kw);
            assert!(keyword_from_bytes(kw, CDialect::C99).is_some(), "{:?}", kw);
            assert!(keyword_from_bytes(kw, CDialect::C11).is_some(), "{:?}", kw);
            assert!(keyword_from_bytes(kw, CDialect::C17).is_some(), "{:?}", kw);
        }
    }

    #[test]
    fn c99_keywords_invalid_in_c89() {
        let c99_only: &[&[u8]] = &[b"inline", b"restrict", b"_Bool", b"_Complex", b"_Imaginary"];
        for &kw in c99_only {
            assert!(
                keyword_from_bytes(kw, CDialect::C89).is_none(),
                "C99 keyword {:?} should not be valid in C89",
                kw
            );
            assert!(
                keyword_from_bytes(kw, CDialect::C99).is_some(),
                "C99 keyword {:?} should be valid in C99",
                kw
            );
            assert!(
                keyword_from_bytes(kw, CDialect::C11).is_some(),
                "C99 keyword {:?} should be valid in C11",
                kw
            );
        }
    }

    #[test]
    fn c11_keywords_invalid_in_c89_c99() {
        let c11_only: &[&[u8]] = &[
            b"_Alignas",
            b"_Alignof",
            b"_Atomic",
            b"_Generic",
            b"_Noreturn",
            b"_Static_assert",
            b"_Thread_local",
        ];
        for &kw in c11_only {
            assert!(
                keyword_from_bytes(kw, CDialect::C89).is_none(),
                "C11 keyword {:?} should not be valid in C89",
                kw
            );
            assert!(
                keyword_from_bytes(kw, CDialect::C99).is_none(),
                "C11 keyword {:?} should not be valid in C99",
                kw
            );
            assert!(
                keyword_from_bytes(kw, CDialect::C11).is_some(),
                "C11 keyword {:?} should be valid in C11",
                kw
            );
            assert!(
                keyword_from_bytes(kw, CDialect::C17).is_some(),
                "C11 keyword {:?} should be valid in C17",
                kw
            );
        }
    }

    #[test]
    fn non_keywords_return_none() {
        assert!(keyword_from_bytes(b"foo", CDialect::C11).is_none());
        assert!(keyword_from_bytes(b"MyType", CDialect::C11).is_none());
        assert!(keyword_from_bytes(b"int32_t", CDialect::C11).is_none());
        assert!(keyword_from_bytes(b"x", CDialect::C17).is_none());
    }
}
