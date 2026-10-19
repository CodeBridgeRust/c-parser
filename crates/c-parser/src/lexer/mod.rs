//! Zero-copy C lexer.
//!
//! ## Architecture
//!
//! The [`Lexer`] wraps a source buffer `&[u8]` (with UTF-8 BOM already
//! stripped), a byte position cursor, and a small configuration block
//! (dialect, preprocessing mode, comment flag).  It implements `Iterator<Item
//! = Token>` so the parser can consume tokens one by one in a `for` loop or by
//! calling `.next()` manually.
//!
//! Every token is a [`Token`] — just a [`TokenKind`] and a [`Span`].  No
//! string data is copied: the source text for identifiers, literals, and
//! comments is always available through `&source[span.start..span.end]`.

pub mod token;

pub use token::{CDialect, PreprocessingMode, Token, TokenKind};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// True for `[a-zA-Z_]`.
#[inline]
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// True for `[a-zA-Z0-9_]`.
#[inline]
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// True for `[0-9]`.
#[inline]
fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

/// True for `[0-9a-fA-F]`.
#[inline]
fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// True for newline.
#[inline]
fn is_newline(b: u8) -> bool {
    matches!(b, b'\n' | b'\r')
}

/// UTF-8 BOM bytes.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Zero-copy C lexer.
///
/// Yields a stream of [`Token`]s by scanning `source` byte-by-byte in a
/// single forward pass.  The lexer never allocates on the heap.
///
/// # BOM handling
///
/// If `source` begins with the UTF-8 BOM sequence `EF BB BF`, the three
/// bytes are silently skipped **before** the first token is produced.
/// Consequently the first token's `span.start` is `0` in a BOM-free file and
/// also `0` in a BOM-prefixed file (the BOM bytes are logically removed from
/// the buffer).  `source.len()` after the constructor reflects the stripped
/// length.
///
/// # Examples
///
/// ```ignore
/// let mut lexer = Lexer::new(b"int x = 42;", CDialect::C11, PreprocessingMode::Preprocessed, false);
/// let tokens: Vec<Token> = lexer.collect();
/// ```
pub struct Lexer<'a> {
    /// Source buffer, without BOM.
    source: &'a [u8],
    /// Current byte offset into `source`.
    pos: usize,
    /// C dialect (controls keyword set).
    dialect: CDialect,
    /// Preprocessing mode (controls `#` handling).
    #[allow(dead_code)]
    mode: PreprocessingMode,
    /// When `true`, comment tokens are emitted; otherwise they are skipped.
    include_comments: bool,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer.
    ///
    /// If `source` starts with UTF-8 BOM, it is stripped automatically so
    /// span positions are not shifted.
    pub fn new(
        source: &'a [u8],
        dialect: CDialect,
        mode: PreprocessingMode,
        include_comments: bool,
    ) -> Self {
        let source = if source.starts_with(BOM) {
            &source[BOM.len()..]
        } else {
            source
        };
        Self {
            source,
            pos: 0,
            dialect,
            mode,
            include_comments,
        }
    }

    /// Return the (BOM-stripped) source this lexer is scanning.
    pub fn source(&self) -> &'a [u8] {
        self.source
    }

    /// Return the current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    // -- Internal helpers ---------------------------------------------------

    /// Peek at the byte at `pos`, or return `0` at EOF.
    #[inline]
    fn peek(&self) -> u8 {
        self.source.get(self.pos).copied().unwrap_or(0)
    }

    /// Peek at `pos + offset`, saturating at EOF.
    #[inline]
    fn peek_at(&self, offset: usize) -> u8 {
        self.source.get(self.pos + offset).copied().unwrap_or(0)
    }

    /// Advance `pos` by `n` (saturating).
    #[inline]
    fn bump(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.source.len());
    }

    /// Advance by 1.
    #[inline]
    fn bump_one(&mut self) {
        self.bump(1);
    }

    /// Slice `self.source[start..end]` as bytes.
    #[inline]
    fn slice(&self, start: usize, end: usize) -> &'a [u8] {
        &self.source[start..end]
    }

    /// Return a token spanning `[start, end)`.
    #[inline]
    fn tok(&self, kind: TokenKind, start: usize, end: usize) -> Token {
        Token::new(kind, start as u32, end as u32)
    }

    // -- Core dispatch ------------------------------------------------------

    /// Scan and return the next token.
    fn scan_token(&mut self) -> Token {
        self.skip_whitespace();

        let start = self.pos;
        if start >= self.source.len() {
            return self.tok(TokenKind::Eof, start, start);
        }

        let b = self.peek();

        match b {
            // Identifiers and string-prefixed literals
            c if is_ident_start(c) => self.scan_ident_or_keyword(start),

            // Numbers
            b'0'..=b'9' => self.scan_number(start),

            // Character literal
            b'\'' => self.scan_char_literal(start, None),

            // String literal
            b'"' => self.scan_string_literal(start, None),

            // Operators / separators (first byte disambiguation)
            b'+' => self.scan_plus(start),
            b'-' => self.scan_minus(start),
            b'*' => self.scan_star(start),
            b'/' => self.scan_slash_or_comment(start),
            b'%' => self.scan_percent(start),
            b'=' => self.scan_eq(start),
            b'!' => self.scan_bang(start),
            b'<' => self.scan_lt(start),
            b'>' => self.scan_gt(start),
            b'&' => self.scan_amp(start),
            b'|' => self.scan_pipe(start),
            b'^' => self.scan_caret(start),
            b'~' => self.scan_tilde(start),
            b'.' => self.scan_dot(start),
            b'?' => self.scan_question(start),
            b':' => self.scan_colon(start),
            b';' => self.scan_semi(start),
            b',' => self.scan_comma(start),
            b'(' => self.scan_lparen(start),
            b')' => self.scan_rparen(start),
            b'[' => self.scan_lbracket(start),
            b']' => self.scan_rbracket(start),
            b'{' => self.scan_lbrace(start),
            b'}' => self.scan_rbrace(start),
            b'#' => self.scan_hash(start),

            _ => {
                self.bump_one();
                self.tok(TokenKind::Error, start, self.pos)
            }
        }
    }

    // -- Whitespace ---------------------------------------------------------

    /// Skip horizontal whitespace and newlines.  Newlines are treated as
    /// whitespace here so the lexer's main loop stays simple; this is
    /// intentional for `Preprocessed` mode.
    fn skip_whitespace(&mut self) {
        while self.pos < self.source.len() {
            match self.source[self.pos] {
                b' ' | b'\t' | 0x0C | 0x0B | b'\n' | b'\r' => self.bump_one(),
                _ => break,
            }
        }
    }

    // -- Identifier / keyword -----------------------------------------------

    /// Scan an identifier or a string/char literal with an encoding prefix
    /// (`L`, `u`, `U`, `u8`).
    fn scan_ident_or_keyword(&mut self, start: usize) -> Token {
        // Consume the first identifier character.
        let first = self.peek();
        self.bump_one();

        // --- Check for single-letter string/char prefixes (L, u, U) ---
        // We check this BEFORE consuming the rest of the identifier so that
        // `L"…"`, `u"…"`, `U"…"` are recognised correctly.
        if matches!(first, b'L' | b'u' | b'U') {
            if self.peek() == b'"' {
                return self.scan_string_literal(start, Some(&self.source[start..self.pos]));
            }
            if self.peek() == b'\'' {
                return self.scan_char_literal(start, Some(&self.source[start..self.pos]));
            }
            // u8 prefix: u, then 8, then " or '
            if first == b'u' && self.peek() == b'8' && self.peek_at(1) == b'"' {
                self.bump_one(); // consume '8'
                return self.scan_string_literal(start, Some(b"u8"));
            }
            if first == b'u' && self.peek() == b'8' && self.peek_at(1) == b'\'' {
                self.bump_one(); // consume '8'
                return self.scan_char_literal(start, Some(b"u8"));
            }
        }

        // Consume the rest of the identifier.
        while self.pos < self.source.len() && is_ident_continue(self.source[self.pos]) {
            self.bump_one();
        }

        let ident_end = self.pos;
        let ident_bytes = self.slice(start, ident_end);

        // Check keywords.
        if let Some(kw) = token::keyword_from_bytes(ident_bytes, self.dialect) {
            self.tok(kw, start, ident_end)
        } else {
            self.tok(TokenKind::Identifier, start, ident_end)
        }
    }

    // -- Number literal -----------------------------------------------------

    /// Scan an integer or floating-point literal.
    fn scan_number(&mut self, start: usize) -> Token {
        // Determine radix: 0x / 0X for hex, 0 for octal (or decimal 0).
        if self.peek() == b'0' {
            match self.peek_at(1) {
                b'x' | b'X' => return self.scan_hex_number(start),
                b'b' | b'B' => return self.scan_binary_number(start),
                _ => { /* continue with octal / decimal */ }
            }
        }

        self.scan_decimal_or_octal_number(start)
    }

    /// Scan `0x...` or `0X...` hex literal.
    fn scan_hex_number(&mut self, start: usize) -> Token {
        self.bump(2); // skip "0x"

        // Hex digits
        while self.pos < self.source.len() && is_hex_digit(self.source[self.pos]) {
            self.bump_one();
        }

        let has_digits = self.pos > start + 2;

        // Hex floating point: 0x1.2p3
        if self.peek() == b'.' && is_hex_digit(self.peek_at(1)) {
            self.bump_one(); // '.'
            while self.pos < self.source.len() && is_hex_digit(self.source[self.pos]) {
                self.bump_one();
            }
            if matches!(self.peek(), b'p' | b'P') {
                self.bump_one();
                if matches!(self.peek(), b'+' | b'-') {
                    self.bump_one();
                }
                while self.pos < self.source.len() && is_digit(self.source[self.pos]) {
                    self.bump_one();
                }
            }
            self.scan_float_suffix();
            return self.tok(TokenKind::FloatLiteral, start, self.pos);
        }

        // Hex exponent without fraction: 0x1p3
        if matches!(self.peek(), b'p' | b'P') {
            self.bump_one();
            if matches!(self.peek(), b'+' | b'-') {
                self.bump_one();
            }
            while self.pos < self.source.len() && is_digit(self.source[self.pos]) {
                self.bump_one();
            }
            self.scan_float_suffix();
            return self.tok(TokenKind::FloatLiteral, start, self.pos);
        }

        if !has_digits {
            return self.tok(TokenKind::Error, start, self.pos);
        }

        self.scan_int_suffix();
        self.tok(TokenKind::IntegerLiteral, start, self.pos)
    }

    /// Scan `0b...` or `0B...` binary literal (GCC extension, accepted
    /// leniently).
    fn scan_binary_number(&mut self, start: usize) -> Token {
        self.bump(2); // skip "0b"

        while self.pos < self.source.len() && matches!(self.source[self.pos], b'0' | b'1') {
            self.bump_one();
        }

        let has_digits = self.pos > start + 2;
        if !has_digits {
            return self.tok(TokenKind::Error, start, self.pos);
        }

        self.scan_int_suffix();
        self.tok(TokenKind::IntegerLiteral, start, self.pos)
    }

    /// Scan a decimal integer, or an octal integer (starts with `0`), or a
    /// float (contains `.` or exponent).
    fn scan_decimal_or_octal_number(&mut self, start: usize) -> Token {
        let starts_with_zero = self.peek() == b'0';
        let mut is_float = false;

        // Integer part
        while self.pos < self.source.len() {
            let b = self.source[self.pos];
            if is_digit(b) {
                self.bump_one();
            } else if b == b'.' && self.peek_at(1) != b'.' {
                // '.' followed by not-'.' (so not ellipsis)
                self.bump_one(); // consume '.'
                is_float = true;
                // Fractional part
                while self.pos < self.source.len() && is_digit(self.source[self.pos]) {
                    self.bump_one();
                }
                break;
            } else if b == b'.' && self.peek_at(1) == b'.' {
                // This is actually `...` (ellipsis) — the number was `0.` or
                // similar.  Treat as integer followed by ellipsis later.
                // But that's the parser's job. For the lexer, stop digits
                // here; the dots are separate tokens.
                break;
            } else if matches!(b, b'e' | b'E' | b'p' | b'P') {
                is_float = true;
                break;
            } else {
                break;
            }
        }

        // Check for octal invalidity: if starts with 0 and has digits > 7
        if starts_with_zero && !is_float && self.pos > start + 1 {
            // Quick check: if any digit 8 or 9 appears after the initial 0,
            // this is actually a decimal starting with 0, not octal. We still
            // tokenise it as IntegerLiteral — the parser or semantic analyser
            // can warn.
        }

        // Exponent
        if matches!(self.peek(), b'e' | b'E') {
            is_float = true;
            self.bump_one();
            if matches!(self.peek(), b'+' | b'-') {
                self.bump_one();
            }
            while self.pos < self.source.len() && is_digit(self.source[self.pos]) {
                self.bump_one();
            }
        }

        // Hex exponent (for 0x without the leading 0x — shouldn't happen here,
        // but guard):
        if matches!(self.peek(), b'p' | b'P') && !is_float {
            is_float = true;
            self.bump_one();
            if matches!(self.peek(), b'+' | b'-') {
                self.bump_one();
            }
            while self.pos < self.source.len() && is_digit(self.source[self.pos]) {
                self.bump_one();
            }
        }

        if is_float {
            self.scan_float_suffix();
            self.tok(TokenKind::FloatLiteral, start, self.pos)
        } else {
            self.scan_int_suffix();
            self.tok(TokenKind::IntegerLiteral, start, self.pos)
        }
    }

    /// Consume an integer suffix: `u`, `l`, `ll`, `ul`, `ull`, `lu`, `llu`
    /// (case-insensitive, with or without the `u`).
    fn scan_int_suffix(&mut self) {
        let mut l_count: u8 = 0;
        let mut saw_u = false;
        loop {
            match self.peek() {
                b'u' | b'U' => {
                    if saw_u {
                        break; // only one 'u'
                    }
                    saw_u = true;
                    self.bump_one();
                }
                b'l' | b'L' => {
                    if l_count >= 2 {
                        break; // at most 'll'
                    }
                    l_count += 1;
                    self.bump_one();
                }
                _ => break,
            }
        }
    }

    /// Consume a float suffix: `f`, `F`, `l`, `L`.
    fn scan_float_suffix(&mut self) {
        match self.peek() {
            b'f' | b'F' | b'l' | b'L' => self.bump_one(),
            _ => {}
        }
    }

    // -- Character literal --------------------------------------------------

    /// Scan `'...'` with an optional encoding prefix (`L`, `u`, `U`).
    fn scan_char_literal(&mut self, start: usize, _prefix: Option<&[u8]>) -> Token {
        self.bump_one(); // opening quote

        while self.pos < self.source.len() {
            match self.source[self.pos] {
                b'\'' => {
                    self.bump_one();
                    return self.tok(TokenKind::CharLiteral, start, self.pos);
                }
                b'\\' => {
                    // Escape sequence — skip the backslash and the next char
                    self.bump(2);
                }
                b'\n' | b'\r' => {
                    // Unterminated char literal — report error and stop
                    self.bump_one();
                    return self.tok(TokenKind::Error, start, self.pos);
                }
                _ => self.bump_one(),
            }
        }

        // EOF before closing quote
        self.tok(TokenKind::Error, start, self.pos)
    }

    // -- String literal -----------------------------------------------------

    /// Scan `"..."` with an optional encoding prefix (`L`, `u`, `U`, `u8`).
    fn scan_string_literal(&mut self, start: usize, _prefix: Option<&[u8]>) -> Token {
        self.bump_one(); // opening quote

        while self.pos < self.source.len() {
            match self.source[self.pos] {
                b'"' => {
                    self.bump_one();
                    return self.tok(TokenKind::StringLiteral, start, self.pos);
                }
                b'\\' => {
                    // Escape sequence
                    self.bump(2);
                }
                b'\n' | b'\r' => {
                    // Unterminated string literal
                    self.bump_one();
                    return self.tok(TokenKind::Error, start, self.pos);
                }
                _ => self.bump_one(),
            }
        }

        // EOF before closing quote
        self.tok(TokenKind::Error, start, self.pos)
    }

    // -- Operators / punctuators --------------------------------------------
    // Each method handles a first-byte family, checking for multi-char forms.

    fn scan_plus(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'+' => {
                self.bump_one();
                self.tok(TokenKind::PlusPlus, start, self.pos)
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::PlusEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Plus, start, self.pos),
        }
    }

    fn scan_minus(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'-' => {
                self.bump_one();
                self.tok(TokenKind::MinusMinus, start, self.pos)
            }
            b'>' => {
                self.bump_one();
                self.tok(TokenKind::Arrow, start, self.pos)
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::MinusEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Minus, start, self.pos),
        }
    }

    fn scan_star(&mut self, start: usize) -> Token {
        self.bump_one();
        if self.peek() == b'=' {
            self.bump_one();
            self.tok(TokenKind::StarEq, start, self.pos)
        } else {
            self.tok(TokenKind::Star, start, self.pos)
        }
    }

    fn scan_slash_or_comment(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'/' => self.scan_line_comment(start),
            b'*' => self.scan_block_comment(start),
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::SlashEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Slash, start, self.pos),
        }
    }

    fn scan_percent(&mut self, start: usize) -> Token {
        self.bump_one();
        if self.peek() == b'=' {
            self.bump_one();
            self.tok(TokenKind::PercentEq, start, self.pos)
        } else {
            self.tok(TokenKind::Percent, start, self.pos)
        }
    }

    fn scan_eq(&mut self, start: usize) -> Token {
        self.bump_one();
        if self.peek() == b'=' {
            self.bump_one();
            self.tok(TokenKind::EqEq, start, self.pos)
        } else {
            self.tok(TokenKind::Eq, start, self.pos)
        }
    }

    fn scan_bang(&mut self, start: usize) -> Token {
        self.bump_one();
        if self.peek() == b'=' {
            self.bump_one();
            self.tok(TokenKind::NotEq, start, self.pos)
        } else {
            self.tok(TokenKind::Bang, start, self.pos)
        }
    }

    fn scan_lt(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'<' => {
                self.bump_one();
                if self.peek() == b'=' {
                    self.bump_one();
                    self.tok(TokenKind::LShiftEq, start, self.pos)
                } else {
                    self.tok(TokenKind::LShift, start, self.pos)
                }
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::LtEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Lt, start, self.pos),
        }
    }

    fn scan_gt(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'>' => {
                self.bump_one();
                if self.peek() == b'=' {
                    self.bump_one();
                    self.tok(TokenKind::RShiftEq, start, self.pos)
                } else {
                    self.tok(TokenKind::RShift, start, self.pos)
                }
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::GtEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Gt, start, self.pos),
        }
    }

    fn scan_amp(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'&' => {
                self.bump_one();
                self.tok(TokenKind::AmpAmp, start, self.pos)
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::AmpEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Amp, start, self.pos),
        }
    }

    fn scan_pipe(&mut self, start: usize) -> Token {
        self.bump_one();
        match self.peek() {
            b'|' => {
                self.bump_one();
                self.tok(TokenKind::PipePipe, start, self.pos)
            }
            b'=' => {
                self.bump_one();
                self.tok(TokenKind::PipeEq, start, self.pos)
            }
            _ => self.tok(TokenKind::Pipe, start, self.pos),
        }
    }

    fn scan_caret(&mut self, start: usize) -> Token {
        self.bump_one();
        if self.peek() == b'=' {
            self.bump_one();
            self.tok(TokenKind::CaretEq, start, self.pos)
        } else {
            self.tok(TokenKind::Caret, start, self.pos)
        }
    }

    fn scan_tilde(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Tilde, start, self.pos)
    }

    fn scan_dot(&mut self, start: usize) -> Token {
        self.bump_one();
        // Check for `...` (ellipsis)
        if self.peek() == b'.' && self.peek_at(1) == b'.' {
            self.bump(2);
            return self.tok(TokenKind::Ellipsis, start, self.pos);
        }
        // Check for float starting with `.`
        if is_digit(self.peek()) {
            // re-parse as number — backtrack to start
            self.pos = start;
            return self.scan_number(start);
        }
        self.tok(TokenKind::Dot, start, self.pos)
    }

    fn scan_question(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Question, start, self.pos)
    }

    fn scan_colon(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Colon, start, self.pos)
    }

    fn scan_semi(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Semi, start, self.pos)
    }

    fn scan_comma(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Comma, start, self.pos)
    }

    fn scan_lparen(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::LParen, start, self.pos)
    }

    fn scan_rparen(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::RParen, start, self.pos)
    }

    fn scan_lbracket(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::LBracket, start, self.pos)
    }

    fn scan_rbracket(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::RBracket, start, self.pos)
    }

    fn scan_lbrace(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::LBrace, start, self.pos)
    }

    fn scan_rbrace(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::RBrace, start, self.pos)
    }

    fn scan_hash(&mut self, start: usize) -> Token {
        self.bump_one();
        self.tok(TokenKind::Hash, start, self.pos)
    }

    // -- Comments -----------------------------------------------------------

    /// Scan `// ...` line comment.
    fn scan_line_comment(&mut self, start: usize) -> Token {
        self.bump_one(); // skip the second '/'

        // Consume until newline or EOF
        while self.pos < self.source.len() && !is_newline(self.source[self.pos]) {
            self.bump_one();
        }

        let end = self.pos;

        if self.include_comments {
            self.tok(TokenKind::LineComment, start, end)
        } else {
            // Skip the comment — return the next real token
            self.scan_token()
        }
    }

    /// Scan `/* ... */` block comment.
    fn scan_block_comment(&mut self, start: usize) -> Token {
        self.bump_one(); // skip the '*'

        loop {
            if self.pos >= self.source.len() {
                // Unterminated block comment
                let end = self.pos;
                if self.include_comments {
                    return self.tok(TokenKind::BlockComment, start, end);
                } else {
                    return self.tok(TokenKind::Error, start, end);
                }
            }

            if self.source[self.pos] == b'*' && self.peek_at(1) == b'/' {
                self.bump(2); // skip '*/'
                let end = self.pos;
                if self.include_comments {
                    return self.tok(TokenKind::BlockComment, start, end);
                } else {
                    return self.scan_token();
                }
            }

            self.bump_one();
        }
    }
}

// ---------------------------------------------------------------------------
// Iterator
// ---------------------------------------------------------------------------

impl<'a> Iterator for Lexer<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.scan_token();
        if token.kind == TokenKind::Eof {
            None
        } else {
            Some(token)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    /// Helper: collect all tokens from source (not including Eof).
    fn lex(source: &[u8]) -> Vec<Token> {
        Lexer::new(source, CDialect::C11, PreprocessingMode::Preprocessed, false).collect()
    }

    /// Helper: collect all tokens with `include_comments: true`.
    fn lex_with_comments(source: &[u8]) -> Vec<Token> {
        Lexer::new(source, CDialect::C11, PreprocessingMode::Preprocessed, true).collect()
    }

    /// Helper: lex with a specific dialect.
    fn lex_dialect(source: &[u8], dialect: CDialect) -> Vec<Token> {
        Lexer::new(source, dialect, PreprocessingMode::Preprocessed, false).collect()
    }

    // -- BOM tests ----------------------------------------------------------

    #[test]
    fn bom_is_stripped() {
        // File with BOM
        let with_bom: &[u8] = &[0xEF, 0xBB, 0xBF, b'i', b'n', b't', b' ', b'x', b';'];
        let tokens = lex(with_bom);
        assert_eq!(tokens.len(), 3);
        // First token should be "int" at span [0, 3), NOT [3, 6)
        assert_eq!(tokens[0].kind, TokenKind::Int);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 3);
    }

    #[test]
    fn no_bom_first_token_starts_at_zero() {
        let source = b"int x;";
        let tokens = lex(source);
        assert_eq!(tokens[0].kind, TokenKind::Int);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 3);
    }

    #[test]
    fn source_length_reflects_stripped_bom() {
        let with_bom: &[u8] = &[0xEF, 0xBB, 0xBF, b'h', b'e', b'l', b'l', b'o'];
        let lexer = Lexer::new(with_bom, CDialect::C11, PreprocessingMode::Preprocessed, false);
        assert_eq!(lexer.source().len(), 5); // 8 - 3
    }

    // -- Keywords -----------------------------------------------------------

    #[test]
    fn all_c89_keywords() {
        let source = b"auto break case char const continue default do double else \
                       enum extern float for goto if int long register return \
                       short signed sizeof static struct switch typedef union \
                       unsigned void volatile while";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds[0], TokenKind::Auto);
        assert_eq!(kinds[1], TokenKind::Break);
        assert_eq!(kinds[2], TokenKind::Case);
        assert_eq!(kinds[3], TokenKind::Char);
        assert_eq!(kinds[4], TokenKind::Const);
        assert_eq!(kinds[5], TokenKind::Continue);
        assert_eq!(kinds[6], TokenKind::Default);
        assert_eq!(kinds[7], TokenKind::Do);
        assert_eq!(kinds[8], TokenKind::Double);
        assert_eq!(kinds[9], TokenKind::Else);
        assert_eq!(kinds[10], TokenKind::Enum);
        assert_eq!(kinds[11], TokenKind::Extern);
        assert_eq!(kinds[12], TokenKind::Float);
        assert_eq!(kinds[13], TokenKind::For);
        assert_eq!(kinds[14], TokenKind::Goto);
        assert_eq!(kinds[15], TokenKind::If);
        assert_eq!(kinds[16], TokenKind::Int);
        assert_eq!(kinds[17], TokenKind::Long);
        assert_eq!(kinds[18], TokenKind::Register);
        assert_eq!(kinds[19], TokenKind::Return);
        assert_eq!(kinds[20], TokenKind::Short);
        assert_eq!(kinds[21], TokenKind::Signed);
        assert_eq!(kinds[22], TokenKind::Sizeof);
        assert_eq!(kinds[23], TokenKind::Static);
        assert_eq!(kinds[24], TokenKind::Struct);
        assert_eq!(kinds[25], TokenKind::Switch);
        assert_eq!(kinds[26], TokenKind::Typedef);
        assert_eq!(kinds[27], TokenKind::Union);
        assert_eq!(kinds[28], TokenKind::Unsigned);
        assert_eq!(kinds[29], TokenKind::Void);
        assert_eq!(kinds[30], TokenKind::Volatile);
        assert_eq!(kinds[31], TokenKind::While);
    }

    #[test]
    fn c99_keywords_in_c11() {
        let source = b"inline restrict _Bool _Complex _Imaginary";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Inline,
            TokenKind::Restrict,
            TokenKind::C99Bool,
            TokenKind::C99Complex,
            TokenKind::C99Imaginary,
        ]);
    }

    #[test]
    fn c99_keywords_are_identifiers_in_c89() {
        let source = b"inline restrict _Bool";
        let tokens = lex_dialect(source, CDialect::C89);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
        ]);
    }

    #[test]
    fn c11_keywords_in_c11() {
        let source = b"_Alignas _Alignof _Atomic _Generic _Noreturn _Static_assert _Thread_local";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::C11Alignas,
            TokenKind::C11Alignof,
            TokenKind::C11Atomic,
            TokenKind::C11Generic,
            TokenKind::C11Noreturn,
            TokenKind::C11StaticAssert,
            TokenKind::C11ThreadLocal,
        ]);
    }

    // -- Identifiers --------------------------------------------------------

    #[test]
    fn identifiers() {
        let source = b"foo bar_123 _private MyStruct x";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Identifier; 5]);
        // Verify spans
        assert_eq!(tokens[0].span, Span { start: 0, end: 3 }); // foo
        assert_eq!(tokens[1].span, Span { start: 4, end: 11 }); // bar_123
        assert_eq!(tokens[2].span, Span { start: 12, end: 20 }); // _private
        assert_eq!(tokens[3].span, Span { start: 21, end: 29 }); // MyStruct
        assert_eq!(tokens[4].span, Span { start: 30, end: 31 }); // x
    }

    // -- Integer literals ---------------------------------------------------

    #[test]
    fn decimal_integers() {
        let source = b"0 42 999";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::IntegerLiteral; 3]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"0");
        assert_eq!(&source[tokens[1].span.start as usize..tokens[1].span.end as usize], b"42");
        assert_eq!(&source[tokens[2].span.start as usize..tokens[2].span.end as usize], b"999");
    }

    #[test]
    fn hex_integers() {
        let source = b"0x0 0xDEAD 0xBEEF 0XABC";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::IntegerLiteral; 4]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"0x0");
        assert_eq!(&source[tokens[1].span.start as usize..tokens[1].span.end as usize], b"0xDEAD");
    }

    #[test]
    fn octal_integers() {
        let source = b"0777 0123 00";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::IntegerLiteral; 3]);
    }

    #[test]
    fn integer_suffixes() {
        let source = b"42u 42U 42l 42L 42ul 42UL 42ll 42ull 42LLU";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::IntegerLiteral; 9]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"42u");
        assert_eq!(&source[tokens[4].span.start as usize..tokens[4].span.end as usize], b"42ul");
        assert_eq!(&source[tokens[6].span.start as usize..tokens[6].span.end as usize], b"42ll");
    }

    // -- Float literals -----------------------------------------------------

    #[test]
    fn float_literals() {
        let source = b"3.14 .5 1e10 2.0f 3.14L 1e-5 0x1.2p3";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::FloatLiteral; 7]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"3.14");
        assert_eq!(&source[tokens[1].span.start as usize..tokens[1].span.end as usize], b".5");
        assert_eq!(&source[tokens[2].span.start as usize..tokens[2].span.end as usize], b"1e10");
        assert_eq!(&source[tokens[3].span.start as usize..tokens[3].span.end as usize], b"2.0f");
        assert_eq!(&source[tokens[4].span.start as usize..tokens[4].span.end as usize], b"3.14L");
        assert_eq!(&source[tokens[5].span.start as usize..tokens[5].span.end as usize], b"1e-5");
    }

    // -- Character literals -------------------------------------------------

    #[test]
    fn char_literals() {
        let source = b"'a' '\\n' '\\x1F' '\\\\'";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::CharLiteral; 4]);
    }

    #[test]
    fn char_literals_c11_prefixes() {
        let source = b"L'a' u'a' U'a'";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::CharLiteral; 3]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"L'a'");
        assert_eq!(&source[tokens[1].span.start as usize..tokens[1].span.end as usize], b"u'a'");
        assert_eq!(&source[tokens[2].span.start as usize..tokens[2].span.end as usize], b"U'a'");
    }

    // -- String literals ----------------------------------------------------

    #[test]
    fn string_literals() {
        let source = br#""hello" "world\n""#;
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::StringLiteral; 2]);
    }

    #[test]
    fn string_literals_c11_prefixes() {
        let source = br#"L"wide" u"utf16" U"utf32" u8"utf8""#;
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::StringLiteral; 4]);
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"L\"wide\"");
        assert_eq!(&source[tokens[1].span.start as usize..tokens[1].span.end as usize], b"u\"utf16\"");
        assert_eq!(&source[tokens[2].span.start as usize..tokens[2].span.end as usize], b"U\"utf32\"");
        assert_eq!(&source[tokens[3].span.start as usize..tokens[3].span.end as usize], b"u8\"utf8\"");
    }

    // -- Operators ----------------------------------------------------------

    #[test]
    fn single_char_operators() {
        let source = b"+ - * / % = < > ! & | ^ ~ . ? :";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Plus, TokenKind::Minus, TokenKind::Star,
            TokenKind::Slash, TokenKind::Percent, TokenKind::Eq,
            TokenKind::Lt, TokenKind::Gt, TokenKind::Bang,
            TokenKind::Amp, TokenKind::Pipe, TokenKind::Caret,
            TokenKind::Tilde, TokenKind::Dot, TokenKind::Question,
            TokenKind::Colon,
        ]);
    }

    #[test]
    fn double_char_operators() {
        let source = b"++ -- -> << >> <= >= == != && || *= /= %= += -= &= |= ^= <<= >>=";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::PlusPlus, TokenKind::MinusMinus, TokenKind::Arrow,
            TokenKind::LShift, TokenKind::RShift, TokenKind::LtEq, TokenKind::GtEq,
            TokenKind::EqEq, TokenKind::NotEq, TokenKind::AmpAmp, TokenKind::PipePipe,
            TokenKind::StarEq, TokenKind::SlashEq, TokenKind::PercentEq,
            TokenKind::PlusEq, TokenKind::MinusEq, TokenKind::AmpEq,
            TokenKind::PipeEq, TokenKind::CaretEq, TokenKind::LShiftEq,
            TokenKind::RShiftEq,
        ]);
    }

    #[test]
    fn ellipsis() {
        let source = b"...";
        let tokens = lex(source);
        assert_eq!(tokens[0].kind, TokenKind::Ellipsis);
        assert_eq!(tokens[0].span.end, 3);
    }

    // -- Separators ---------------------------------------------------------

    #[test]
    fn separators() {
        let source = b"; , ( ) [ ] { }";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Semi, TokenKind::Comma,
            TokenKind::LParen, TokenKind::RParen,
            TokenKind::LBracket, TokenKind::RBracket,
            TokenKind::LBrace, TokenKind::RBrace,
        ]);
    }

    // -- Comments -----------------------------------------------------------

    #[test]
    fn comments_are_skipped_by_default() {
        let source = b"int /* block */ x; // line\nint y;";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Int, TokenKind::Identifier, TokenKind::Semi,
            TokenKind::Int, TokenKind::Identifier, TokenKind::Semi,
        ]);
    }

    #[test]
    fn include_comments_flag() {
        let source = b"/* block */ // line\nint";
        let tokens = lex_with_comments(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::BlockComment,
            TokenKind::LineComment,
            TokenKind::Int,
        ]);
    }

    #[test]
    fn line_comment_span() {
        let source = b"// hello world\nint";
        let tokens = lex_with_comments(source);
        assert_eq!(tokens[0].kind, TokenKind::LineComment);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 14); // "// hello world"
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"// hello world");
    }

    #[test]
    fn block_comment_span() {
        let source = b"/* block */int";
        let tokens = lex_with_comments(source);
        assert_eq!(tokens[0].kind, TokenKind::BlockComment);
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 11); // "/* block */"
        assert_eq!(&source[tokens[0].span.start as usize..tokens[0].span.end as usize], b"/* block */");
    }

    // -- Hash / preprocessor ------------------------------------------------

    #[test]
    fn hash_token() {
        let source = b"#";
        let tokens = lex(source);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Hash);
    }

    #[test]
    fn hash_define_in_single_tu_mode() {
        let source = b"#define FOO 42";
        let lexer = Lexer::new(source, CDialect::C11, PreprocessingMode::SingleTu, false);
        let tokens: Vec<Token> = lexer.collect();
        // Iterator stops before Eof, so 4 tokens: # define FOO 42
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::Hash);
        assert_eq!(tokens[1].kind, TokenKind::Identifier); // define (not a keyword)
        assert_eq!(tokens[2].kind, TokenKind::Identifier); // FOO
        assert_eq!(tokens[3].kind, TokenKind::IntegerLiteral); // 42
    }

    // -- Edge cases ---------------------------------------------------------

    #[test]
    fn empty_source() {
        let tokens = lex(b"");
        assert!(tokens.is_empty());
    }

    #[test]
    fn only_whitespace() {
        let tokens = lex(b"   \t\n\r  ");
        assert!(tokens.is_empty());
    }

    #[test]
    fn mixed_tokens() {
        let source = b"int main(void) { return 0; }";
        let tokens = lex(source);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Int, TokenKind::Identifier, TokenKind::LParen,
            TokenKind::Void, TokenKind::RParen, TokenKind::LBrace,
            TokenKind::Return, TokenKind::IntegerLiteral, TokenKind::Semi,
            TokenKind::RBrace,
        ]);
    }

    #[test]
    fn span_positions_are_sequential() {
        // Verify that spans correctly track byte positions
        let source = b"int x = 42;";
        let tokens = lex(source);
        // int (0..3), x (4..5), = (6..7), 42 (8..10), ; (10..11)
        assert_eq!(tokens[0].span, Span { start: 0, end: 3 });
        assert_eq!(tokens[1].span, Span { start: 4, end: 5 });
        assert_eq!(tokens[2].span, Span { start: 6, end: 7 });
        assert_eq!(tokens[3].span, Span { start: 8, end: 10 });
        assert_eq!(tokens[4].span, Span { start: 10, end: 11 });
    }

    #[test]
    fn unterminated_string_is_error() {
        let source = b"\"unterminated";
        let tokens = lex(source);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Error);
    }

    #[test]
    fn unterminated_char_is_error() {
        let source = b"'x";
        let tokens = lex(source);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Error);
    }

    #[test]
    fn iterator_stops_before_eof() {
        let source = b"x";
        let tokens = lex(source);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        // No Eof token — iterator yields None
    }

    #[test]
    fn binary_literal_extension() {
        let source = b"0b1010 0B1100";
        let tokens = lex(source);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::IntegerLiteral);
        assert_eq!(tokens[1].kind, TokenKind::IntegerLiteral);
    }
}