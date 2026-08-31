//! The Lark lexer.
//!
//! The lexer keeps every byte. Whitespace, a comment, and a preprocessor line
//! all become tokens, so the tokens of a file join back into the file. That is
//! rule L-13, which is invariant R at the token level.
//!
//! The lexer maps a C11 keyword to its own kind, because C reserves it. The
//! lexer does not map a Lark keyword, because rule L-3 makes every Lark keyword
//! contextual. The parser recognizes those by position.

use lark_diag::{Code, LK0103, LK0104, LK0105};
use lark_span::Span;

use crate::kind::SyntaxKind;

/// One token, with the region it covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    /// What the token is.
    pub kind: SyntaxKind,
    /// Where the token sits in the source.
    pub span: Span,
}

/// A problem that the lexer found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LexError {
    /// The diagnostic code.
    pub code: Code,
    /// The region that the problem covers.
    pub span: Span,
}

/// The result of a lexer run.
#[derive(Clone, Debug, Default)]
pub struct Lexed {
    /// Every token, in source order.
    pub tokens: Vec<Token>,
    /// Every problem that the lexer found.
    pub errors: Vec<LexError>,
}

impl Lexed {
    /// Joins the text of every token back together.
    ///
    /// The result equals the input. See rule L-13.
    pub fn join(&self, source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        for token in &self.tokens {
            out.push_str(source.get(token.span.as_range()).unwrap_or_default());
        }
        out
    }
}

/// The punctuation of C11 and of Lark, longest first.
///
/// The order matters. The lexer takes the first entry that matches, so `<<=`
/// must come before `<<`, and `<<` before `<`.
///
/// The digraphs come from C11 section 6.4.6. A strict superset must read them,
/// so `a<:b:>` means `a[b]`.
const PUNCTUATION: &[(&str, SyntaxKind)] = &[
    ("%:%:", SyntaxKind::HASH2),
    ("...", SyntaxKind::ELLIPSIS),
    ("<<=", SyntaxKind::SHL_EQ),
    (">>=", SyntaxKind::SHR_EQ),
    ("->", SyntaxKind::ARROW),
    ("++", SyntaxKind::PLUS2),
    ("--", SyntaxKind::MINUS2),
    ("<<", SyntaxKind::SHL),
    (">>", SyntaxKind::SHR),
    ("<=", SyntaxKind::LT_EQ),
    (">=", SyntaxKind::GT_EQ),
    ("==", SyntaxKind::EQ2),
    ("!=", SyntaxKind::BANG_EQ),
    ("&&", SyntaxKind::AMP2),
    ("||", SyntaxKind::PIPE2),
    ("*=", SyntaxKind::STAR_EQ),
    ("/=", SyntaxKind::SLASH_EQ),
    ("%=", SyntaxKind::PERCENT_EQ),
    ("+=", SyntaxKind::PLUS_EQ),
    ("-=", SyntaxKind::MINUS_EQ),
    ("&=", SyntaxKind::AMP_EQ),
    ("^=", SyntaxKind::CARET_EQ),
    ("|=", SyntaxKind::PIPE_EQ),
    ("##", SyntaxKind::HASH2),
    ("::", SyntaxKind::COLON2),
    ("<:", SyntaxKind::L_BRACK),
    (":>", SyntaxKind::R_BRACK),
    ("<%", SyntaxKind::L_CURLY),
    ("%>", SyntaxKind::R_CURLY),
    ("%:", SyntaxKind::HASH),
    ("[", SyntaxKind::L_BRACK),
    ("]", SyntaxKind::R_BRACK),
    ("(", SyntaxKind::L_PAREN),
    (")", SyntaxKind::R_PAREN),
    ("{", SyntaxKind::L_CURLY),
    ("}", SyntaxKind::R_CURLY),
    (".", SyntaxKind::DOT),
    ("&", SyntaxKind::AMP),
    ("*", SyntaxKind::STAR),
    ("+", SyntaxKind::PLUS),
    ("-", SyntaxKind::MINUS),
    ("~", SyntaxKind::TILDE),
    ("!", SyntaxKind::BANG),
    ("/", SyntaxKind::SLASH),
    ("%", SyntaxKind::PERCENT),
    ("<", SyntaxKind::L_ANGLE),
    (">", SyntaxKind::R_ANGLE),
    ("^", SyntaxKind::CARET),
    ("|", SyntaxKind::PIPE),
    ("?", SyntaxKind::QUESTION),
    (":", SyntaxKind::COLON),
    (";", SyntaxKind::SEMICOLON),
    ("=", SyntaxKind::EQ),
    (",", SyntaxKind::COMMA),
    ("#", SyntaxKind::HASH),
    ("@", SyntaxKind::AT),
];

/// A prefix that a character constant or a string literal can carry.
const LITERAL_PREFIXES: &[&str] = &["L", "u", "U", "u8"];

/// Turns source text into tokens.
///
/// The lexer never fails. A problem produces a diagnostic and a token, so the
/// text stays complete.
pub fn tokenize(source: &str) -> Lexed {
    Cursor::new(source).run()
}

/// The lexer state.
struct Cursor<'a> {
    source: &'a str,
    offset: usize,
    /// True when only trivia sits between the start of the line and here.
    ///
    /// A `#` at that position starts a preprocessor line.
    line_start: bool,
    lexed: Lexed,
}

impl<'a> Cursor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line_start: true,
            lexed: Lexed::default(),
        }
    }

    fn run(mut self) -> Lexed {
        while !self.at_end() {
            let start = self.offset;
            let kind = self.next_token();
            let span = Span::new(offset_of(start), offset_of(self.offset));
            if !kind.is_trivia() {
                self.line_start = false;
            }
            self.lexed.tokens.push(Token { kind, span });
        }
        self.lexed
    }

    fn rest(&self) -> &'a str {
        self.source.get(self.offset..).unwrap_or_default()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, skip: usize) -> Option<char> {
        self.rest().chars().nth(skip)
    }

    fn at_end(&self) -> bool {
        self.offset >= self.source.len()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        Some(character)
    }

    fn error(&mut self, code: Code, start: usize) {
        self.lexed.errors.push(LexError {
            code,
            span: Span::new(offset_of(start), offset_of(self.offset)),
        });
    }

    /// Reads one token and returns its kind.
    fn next_token(&mut self) -> SyntaxKind {
        let start = self.offset;
        let Some(first) = self.peek() else {
            return SyntaxKind::EOF;
        };

        if is_whitespace(first) || self.at_line_splice() {
            return self.whitespace();
        }
        if first == '/' && self.peek_at(1) == Some('/') {
            return self.line_comment();
        }
        if first == '/' && self.peek_at(1) == Some('*') {
            return self.block_comment(start);
        }
        if first == '#' && self.line_start {
            return self.preprocessor_line();
        }
        if first.is_ascii_digit() {
            return self.number();
        }
        if first == '.' && self.peek_at(1).is_some_and(|next| next.is_ascii_digit()) {
            return self.number();
        }
        if first == '\'' {
            return self.quoted(start, '\'', SyntaxKind::CHAR_LITERAL);
        }
        if first == '"' {
            return self.quoted(start, '"', SyntaxKind::STRING_LITERAL);
        }
        if is_ident_start(first) {
            return self.word(start);
        }
        if let Some(kind) = self.punctuation() {
            return kind;
        }

        self.bump();
        self.error(LK0105, start);
        SyntaxKind::ERROR_TOKEN
    }

    /// Reports whether a backslash and a newline sit at the cursor.
    ///
    /// C removes the pair in translation phase 2. Lark keeps it as whitespace,
    /// so the text stays complete.
    fn at_line_splice(&self) -> bool {
        self.peek() == Some('\\') && matches!(self.peek_at(1), Some('\n' | '\r'))
    }

    fn whitespace(&mut self) -> SyntaxKind {
        let mut saw_newline = false;
        loop {
            match self.peek() {
                Some(character) if is_whitespace(character) => {
                    if character == '\n' {
                        saw_newline = true;
                    }
                    self.bump();
                }
                _ if self.at_line_splice() => {
                    self.bump();
                    self.bump();
                }
                _ => break,
            }
        }
        if saw_newline {
            self.line_start = true;
        }
        SyntaxKind::WHITESPACE
    }

    fn line_comment(&mut self) -> SyntaxKind {
        self.bump();
        self.bump();
        while let Some(character) = self.peek() {
            if character == '\n' {
                break;
            }
            if self.at_line_splice() {
                self.bump();
                self.bump();
                continue;
            }
            self.bump();
        }
        SyntaxKind::LINE_COMMENT
    }

    fn block_comment(&mut self, start: usize) -> SyntaxKind {
        self.bump();
        self.bump();
        loop {
            let Some(character) = self.bump() else {
                // Rule L-10. The comment reaches the end of the file.
                self.error(LK0103, start);
                break;
            };
            if character == '*' && self.peek() == Some('/') {
                self.bump();
                break;
            }
        }
        SyntaxKind::BLOCK_COMMENT
    }

    /// Reads a whole preprocessor line, including its continuations.
    ///
    /// Rule C-3 passes the line through to the emitted C, so the lexer keeps it
    /// as one token.
    fn preprocessor_line(&mut self) -> SyntaxKind {
        self.bump();
        while let Some(character) = self.peek() {
            if character == '\n' {
                break;
            }
            if self.at_line_splice() {
                self.bump();
                // A file with Windows line endings splices `\` `\r` `\n`, so
                // the carriage return and the newline both belong to the
                // directive. Consuming only two characters left the newline
                // behind and ended the directive early.
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
                continue;
            }
            self.bump();
        }
        SyntaxKind::PP_DIRECTIVE
    }

    /// Reads a preprocessing number, then decides whether it is an integer.
    ///
    /// C11 section 6.4.8 gives a loose grammar for a number, so that a bad
    /// number is one token rather than several.
    fn number(&mut self) -> SyntaxKind {
        let start = self.offset;
        let mut previous = self.bump().unwrap_or('0');
        let mut saw_dot = previous == '.';
        let mut saw_exponent = false;

        while let Some(character) = self.peek() {
            if matches!(previous, 'e' | 'E' | 'p' | 'P') && matches!(character, '+' | '-') {
                saw_exponent = true;
                previous = character;
                self.bump();
                continue;
            }
            if character == '.' {
                saw_dot = true;
            } else if !is_ident_continue(character) {
                break;
            }
            previous = character;
            self.bump();
        }

        let text = self.source.get(start..self.offset).unwrap_or_default();
        let is_hex = text.len() > 1 && text.starts_with('0') && text[1..].starts_with(['x', 'X']);
        let exponent = if is_hex {
            text.contains(['p', 'P'])
        } else {
            text.contains(['e', 'E'])
        };

        if saw_dot || exponent || saw_exponent {
            SyntaxKind::FLOAT_NUMBER
        } else {
            SyntaxKind::INT_NUMBER
        }
    }

    /// Reads a character constant or a string literal.
    fn quoted(&mut self, start: usize, quote: char, kind: SyntaxKind) -> SyntaxKind {
        self.bump();
        loop {
            let Some(character) = self.peek() else {
                // Rule L-11. The literal reaches the end of the file.
                self.error(LK0104, start);
                break;
            };
            if character == '\n' {
                // Rule L-11. The literal reaches the end of the line.
                self.error(LK0104, start);
                break;
            }
            if character == '\\' {
                self.bump();
                if self.peek() != Some('\n') {
                    self.bump();
                }
                continue;
            }
            self.bump();
            if character == quote {
                break;
            }
        }
        kind
    }

    /// Reads a word, and decides whether it is a keyword or a literal prefix.
    fn word(&mut self, start: usize) -> SyntaxKind {
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        let text = self.source.get(start..self.offset).unwrap_or_default();

        if LITERAL_PREFIXES.contains(&text) {
            match self.peek() {
                Some('\'') => return self.quoted(start, '\'', SyntaxKind::CHAR_LITERAL),
                Some('"') => return self.quoted(start, '"', SyntaxKind::STRING_LITERAL),
                _ => {}
            }
        }

        SyntaxKind::c_keyword(text).unwrap_or(SyntaxKind::IDENT)
    }

    /// Reads punctuation, longest match first.
    fn punctuation(&mut self) -> Option<SyntaxKind> {
        let rest = self.rest();
        for (text, kind) in PUNCTUATION {
            if rest.starts_with(text) {
                self.offset += text.len();
                return Some(*kind);
            }
        }
        None
    }
}

/// Reports whether a character separates tokens.
fn is_whitespace(character: char) -> bool {
    character.is_whitespace()
}

/// Reports whether a character can start an identifier.
///
/// A character outside ASCII counts, so a source file that names an identifier
/// in another script still lexes.
fn is_ident_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic() || !character.is_ascii()
}

/// Reports whether a character can continue an identifier.
fn is_ident_continue(character: char) -> bool {
    is_ident_start(character) || character.is_ascii_digit()
}

/// Narrows a byte offset for a span.
fn offset_of(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use lark_diag::{LK0103, LK0104, LK0105};

    use super::{Lexed, tokenize};
    use crate::kind::SyntaxKind::{self, *};

    /// Every kind in order, trivia included.
    fn all_kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source)
            .tokens
            .iter()
            .map(|token| token.kind)
            .collect()
    }

    /// Every kind in order, trivia removed.
    fn kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source)
            .tokens
            .iter()
            .map(|token| token.kind)
            .filter(|kind| !kind.is_trivia())
            .collect()
    }

    fn lex(source: &str) -> Lexed {
        tokenize(source)
    }

    /// Source samples that the invariant test uses. Some are malformed on purpose.
    const SAMPLES: &[&str] = &[
        "",
        " ",
        "\n\n\n",
        "int main(void) { return 0; }",
        "// comment\n",
        "// comment with no newline",
        "/* block */",
        "/* unterminated",
        "\"string\"",
        "\"unterminated",
        "'c'",
        "'unterminated",
        "u8\"prefixed\"",
        "L'w'",
        "0x1p-3",
        "1.5e+10f",
        "0b1010",
        "#include <stdio.h>\n",
        "#define A(x) \\\n    x\n",
        "a\\\nb",
        "gc Person* p = new Person { .age = 1 };",
        "@import stdio",
        "stdio::printf",
        "a<:b:>",
        "x <<= 1;",
        "$ ` \u{20ac}",
        "identifier_with_\u{20ac}_inside",
        "a ? b : c",
        "...",
        "/**/",
        "'\\''",
        "\"\\\"\"",
    ];

    /// covers: L-13
    #[test]
    fn the_tokens_join_back_into_the_source() {
        for sample in SAMPLES {
            let lexed = lex(sample);
            assert_eq!(
                &lexed.join(sample),
                sample,
                "invariant R fails for {sample:?}"
            );
        }
    }

    /// covers: L-13
    #[test]
    fn the_tokens_cover_the_source_with_no_gap_and_no_overlap() {
        for sample in SAMPLES {
            let lexed = lex(sample);
            let mut next = 0;
            for token in &lexed.tokens {
                assert_eq!(
                    token.span.start, next,
                    "a gap before {token:?} in {sample:?}"
                );
                assert!(
                    token.span.end > token.span.start,
                    "an empty token in {sample:?}"
                );
                next = token.span.end;
            }
            assert_eq!(
                next as usize,
                sample.len(),
                "the tokens stop early in {sample:?}"
            );
        }
    }

    #[test]
    fn an_empty_source_produces_no_token() {
        assert!(lex("").tokens.is_empty());
    }

    /// covers: L-3, L-4
    #[test]
    fn a_lark_keyword_lexes_as_an_identifier() {
        for word in [
            "gc", "managed", "iface", "impl", "new", "export", "init", "gc_safe",
        ] {
            assert_eq!(kinds(word), vec![IDENT], "{word} must lex as an identifier");
        }
    }

    #[test]
    fn a_c11_keyword_lexes_as_a_keyword() {
        assert_eq!(kinds("int"), vec![INT_KW]);
        assert_eq!(kinds("auto"), vec![AUTO_KW]);
        assert_eq!(kinds("_Static_assert"), vec![STATIC_ASSERT_KW]);
        assert_eq!(kinds("intx"), vec![IDENT]);
    }

    /// covers: L-1
    #[test]
    fn an_at_sign_is_its_own_token() {
        assert_eq!(kinds("@import stdio"), vec![AT, IDENT, IDENT]);
    }

    /// covers: L-2
    #[test]
    fn a_double_colon_is_one_token() {
        assert_eq!(kinds("stdio::printf"), vec![IDENT, COLON2, IDENT]);
        assert_eq!(
            kinds("a ? b : c"),
            vec![IDENT, QUESTION, IDENT, COLON, IDENT]
        );
    }

    #[test]
    fn punctuation_takes_the_longest_match() {
        assert_eq!(kinds("<<="), vec![SHL_EQ]);
        assert_eq!(kinds("<<"), vec![SHL]);
        assert_eq!(kinds("<"), vec![L_ANGLE]);
        assert_eq!(kinds("..."), vec![ELLIPSIS]);
        assert_eq!(kinds(".."), vec![DOT, DOT]);
        assert_eq!(kinds("->"), vec![ARROW]);
        assert_eq!(kinds("-->"), vec![MINUS2, R_ANGLE]);
    }

    #[test]
    fn a_digraph_reads_as_the_token_it_stands_for() {
        assert_eq!(kinds("a<:b:>"), vec![IDENT, L_BRACK, IDENT, R_BRACK]);
        assert_eq!(kinds("<% %>"), vec![L_CURLY, R_CURLY]);
    }

    #[test]
    fn an_integer_and_a_float_get_different_kinds() {
        assert_eq!(kinds("0"), vec![INT_NUMBER]);
        assert_eq!(kinds("42u"), vec![INT_NUMBER]);
        assert_eq!(kinds("0xFFull"), vec![INT_NUMBER]);
        assert_eq!(kinds("1.5"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds(".5"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds("1e10"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds("1e+10"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds("0x1p-3"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds("0xEf"), vec![INT_NUMBER]);
    }

    #[test]
    fn a_dot_after_a_number_belongs_to_the_number() {
        assert_eq!(kinds("1.5f"), vec![FLOAT_NUMBER]);
        assert_eq!(kinds("x.y"), vec![IDENT, DOT, IDENT]);
    }

    #[test]
    fn a_literal_prefix_joins_its_literal() {
        assert_eq!(kinds("L\"wide\""), vec![STRING_LITERAL]);
        assert_eq!(kinds("u8\"utf8\""), vec![STRING_LITERAL]);
        assert_eq!(kinds("U'c'"), vec![CHAR_LITERAL]);
        assert_eq!(kinds("u8"), vec![IDENT]);
        assert_eq!(kinds("Lx\"not a prefix\""), vec![IDENT, STRING_LITERAL]);
    }

    #[test]
    fn an_escape_does_not_end_a_literal() {
        assert_eq!(kinds("\"a\\\"b\""), vec![STRING_LITERAL]);
        assert_eq!(kinds("'\\''"), vec![CHAR_LITERAL]);
        assert!(lex("\"a\\\"b\"").errors.is_empty());
    }

    #[test]
    fn a_comment_is_trivia_and_stays_in_the_token_list() {
        assert_eq!(all_kinds("// hi\n"), vec![LINE_COMMENT, WHITESPACE]);
        assert_eq!(all_kinds("/* hi */"), vec![BLOCK_COMMENT]);
        assert_eq!(kinds("/* hi */ x"), vec![IDENT]);
    }

    /// covers: L-10
    #[test]
    fn an_unterminated_block_comment_reports_lk0103() {
        let lexed = lex("/* no end");
        assert_eq!(lexed.errors.len(), 1);
        assert_eq!(lexed.errors[0].code, LK0103);
        assert_eq!(lexed.tokens.len(), 1);
        assert_eq!(lexed.tokens[0].kind, BLOCK_COMMENT);
    }

    /// covers: L-11
    #[test]
    fn a_literal_that_runs_past_the_line_reports_lk0104() {
        let lexed = lex("\"no end\nnext");
        assert_eq!(lexed.errors.len(), 1);
        assert_eq!(lexed.errors[0].code, LK0104);

        let lexed = lex("'no end");
        assert_eq!(lexed.errors.len(), 1);
        assert_eq!(lexed.errors[0].code, LK0104);
    }

    /// covers: L-12
    #[test]
    fn a_character_that_cannot_start_a_token_reports_lk0105() {
        let lexed = lex("`");
        assert_eq!(lexed.errors.len(), 1);
        assert_eq!(lexed.errors[0].code, LK0105);
        assert_eq!(lexed.tokens[0].kind, ERROR_TOKEN);
    }

    #[test]
    fn a_preprocessor_line_is_one_token() {
        assert_eq!(
            all_kinds("#include <stdio.h>\n"),
            vec![PP_DIRECTIVE, WHITESPACE]
        );
        assert_eq!(
            all_kinds("  # define A 1\n"),
            vec![WHITESPACE, PP_DIRECTIVE, WHITESPACE]
        );
        assert_eq!(
            all_kinds("/* c */ #if 0\n"),
            vec![BLOCK_COMMENT, WHITESPACE, PP_DIRECTIVE, WHITESPACE]
        );
    }

    #[test]
    fn a_directive_continues_over_a_backslash_newline() {
        let source = "#define M(a) \\\n    call(a)\nint x;";
        let first = tokenize(source).tokens[0];
        assert_eq!(first.kind, PP_DIRECTIVE);
        assert!(source[..first.span.end as usize].contains("call(a)"));
    }

    #[test]
    fn a_directive_continues_over_windows_line_endings() {
        // A file with CRLF splices `\` `\r` `\n`. Consuming only two
        // characters left the newline behind and ended the directive early.
        let source = "#define M(a) \\\r\n    call(a)\r\nint x;";
        let first = tokenize(source).tokens[0];
        assert_eq!(first.kind, PP_DIRECTIVE);
        assert!(source[..first.span.end as usize].contains("call(a)"));
    }

    #[test]
    fn a_hash_after_code_is_punctuation_not_a_preprocessor_line() {
        assert_eq!(kinds("x # y"), vec![IDENT, HASH, IDENT]);
    }

    #[test]
    fn a_preprocessor_line_takes_its_continuations() {
        let lexed = lex("#define A(x) \\\n    x\nint y;");
        let first = lexed.tokens.first().map(|token| token.kind);
        assert_eq!(first, Some(PP_DIRECTIVE));
        assert_eq!(
            kinds("#define A \\\n 1\nint y;"),
            vec![INT_KW, IDENT, SEMICOLON]
        );
    }

    #[test]
    fn a_line_splice_is_whitespace() {
        assert_eq!(all_kinds("a\\\nb"), vec![IDENT, WHITESPACE, IDENT]);
    }

    #[test]
    fn an_identifier_can_hold_a_character_outside_ascii() {
        assert_eq!(kinds("caf\u{e9}"), vec![IDENT]);
        assert_eq!(kinds("\u{20ac}"), vec![IDENT]);
    }

    #[test]
    fn a_span_points_at_the_text_of_its_token() {
        let source = "int x;";
        let lexed = lex(source);
        let texts: Vec<&str> = lexed
            .tokens
            .iter()
            .map(|token| source.get(token.span.as_range()).unwrap_or_default())
            .collect();
        assert_eq!(texts, vec!["int", " ", "x", ";"]);
    }
}
