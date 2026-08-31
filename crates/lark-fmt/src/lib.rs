//! The Lark source formatter.
//!
//! Invariant R says the text of the tree equals the source, byte for byte. So
//! a formatter is a second printer over the same tree, and it needs no parser
//! of its own.
//!
//! # One style, no options
//!
//! Rule Z-1. There is one canonical style and nothing to configure. An option
//! set turns every project into an argument about the option set, and the
//! value of a formatter is that the argument stops.
//!
//! | Choice | The style |
//! |---|---|
//! | Indent | Four spaces. Never a tab. |
//! | A brace | On the line of the construct that opens it. |
//! | A statement | One per line. |
//! | A binary operator | One space on each side. |
//! | A comma | No space before, one space after. |
//! | A call | No space between the name and the parenthesis. |
//! | A keyword before a group | One space, as in `if (x)`. |
//! | A blank line | At most one, anywhere. |
//! | The end of a line | No trailing space. |
//! | The end of a file | Exactly one newline. |
//!
//! # What it must never do
//!
//! Rule Z-2. Formatting changes no token. The sequence of tokens that are not
//! whitespace is the same before and after, so the program means the same
//! thing. A property test proves it over every file the project holds.
//!
//! Rule Z-3. Formatting twice equals formatting once. A style that does not
//! settle would make every save a change, and every diff noise.
//!
//! Rule Z-4. A file that does not parse still formats. The tree keeps the text
//! that the parser could not read, so the parts that read are laid out and the
//! rest is left as it stands. An editor holds a file in that state most of the
//! time.

// This module walks 41 of the kinds, and a list that long in the header helps
// no reader, so it imports the variants. A module that uses a few names spells
// them out instead.
#![allow(clippy::enum_glob_use)]

use lark_syntax::SyntaxKind::*;
use lark_syntax::{NoNames, SyntaxKind, SyntaxToken, all_tokens, parse, tokenize};

/// The width of one level of indentation.
const INDENT: usize = 4;

/// Formats one file.
///
/// Rule Z-4. A file that does not parse still formats, because the tree keeps
/// every byte that the parser could not read.
#[must_use]
pub fn format(source: &str) -> String {
    let parsed = parse(source, &NoNames);
    let tokens: Vec<SyntaxToken> = all_tokens(&parsed.syntax()).collect();
    Printer::new().run(&tokens)
}

/// Reports whether a file is already formatted.
#[must_use]
pub fn is_formatted(source: &str) -> bool {
    format(source) == source
}

/// The state that the walk carries from one token to the next.
struct Printer {
    out: String,
    /// How many levels of indentation the next line starts with.
    depth: usize,
    /// How many parentheses are open. A `;` inside one stays on its line.
    parens: usize,
    /// The last token that reached the output, trivia included.
    previous: Option<SyntaxKind>,
    /// The last token that was not trivia.
    previous_code: Option<SyntaxKind>,
    /// The text of the last token that was not trivia.
    previous_text: String,
    /// Whether the last token binds to the one after it.
    ///
    /// A generic list closes with `>` and the next token joins it, as in
    /// `Box<int> value`. The flag says so, because the kind alone cannot: a
    /// `>` is also a comparison.
    previous_tight: bool,
    /// How many initializer lists are open.
    ///
    /// An initializer stays on one line. A block does not. The tree says which
    /// one a brace opens, so the formatter needs no guess.
    lists: usize,
    /// Whether the output ends at the start of a line.
    at_line_start: bool,
    /// How many newlines are pending, so a blank line collapses to one.
    pending_newlines: usize,
}

impl Printer {
    fn new() -> Self {
        Self {
            out: String::new(),
            depth: 0,
            parens: 0,
            previous: None,
            previous_code: None,
            previous_text: String::new(),
            previous_tight: false,
            lists: 0,
            at_line_start: true,
            pending_newlines: 0,
        }
    }

    /// Prints every token.
    fn run(mut self, tokens: &[SyntaxToken]) -> String {
        for (index, token) in tokens.iter().enumerate() {
            // The next token that is not trivia. A layout choice must not
            // depend on whether the source held a space, or the second pass
            // would decide differently from the first.
            let next = tokens[index + 1..]
                .iter()
                .find(|item| !item.kind().is_trivia());
            self.token(token, next);
        }
        // Rule Z-1. Exactly one newline at the end.
        while self.out.ends_with(' ') || self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    /// Prints one token, with the layout that its neighbours ask for.
    fn token(&mut self, token: &SyntaxToken, next: Option<&SyntaxToken>) {
        let kind = token.kind();

        if kind == WHITESPACE {
            // Whitespace never reaches the output on its own. A blank line in
            // the source becomes one pending blank line.
            let lines = token.text().matches('\n').count();
            if lines >= 2 {
                self.pending_newlines = self.pending_newlines.max(2);
            } else if lines == 1 {
                self.pending_newlines = self.pending_newlines.max(1);
            }
            return;
        }

        // A directive owns its line, whatever came before it.
        if kind == PP_DIRECTIVE {
            self.break_line(1);
            self.write_raw(token.text().trim_end());
            self.pending_newlines = 1;
            self.previous = Some(kind);
            return;
        }

        // A closing brace leaves its block before it prints. One that closes
        // an initializer stays on the line it started on.
        if kind == R_CURLY {
            if self.lists > 0 {
                self.lists -= 1;
                self.pending_newlines = 0;
                self.space();
                self.write_raw(token.text());
                self.previous = Some(kind);
                self.previous_code = Some(kind);
                token.text().clone_into(&mut self.previous_text);
                return;
            }
            self.depth = self.depth.saturating_sub(1);
            self.break_line(1);
        }

        // A comment that stood on its own line keeps its own line.
        if matches!(kind, LINE_COMMENT | BLOCK_COMMENT) {
            if self.pending_newlines > 0 || self.previous.is_none() {
                self.break_line(self.pending_newlines.max(1));
            } else {
                self.space();
            }
            self.write_raw(token.text().trim_end());
            if kind == LINE_COMMENT {
                self.pending_newlines = 1;
            }
            self.previous = Some(kind);
            return;
        }

        // Two tokens bind so tight that nothing goes between them. A pointer
        // star belongs to the type on its left, as in `gc char* name`. An
        // angle bracket belongs to its generic list, as in `Box<Data<int>>`.
        //
        // The angle bracket matters for more than looks. The parser splits a
        // `>>` into two tokens to close two lists, and a space between them
        // would leave text that lexes as two tokens rather than one.
        let tight = (kind == STAR && is_pointer(token)) || is_generic_angle(token);
        if tight {
            self.write_raw(token.text());
            self.previous = Some(kind);
            self.previous_code = Some(kind);
            token.text().clone_into(&mut self.previous_text);
            // A `>` that closes a list binds to what follows it. A pointer
            // star takes a space, because a name follows it.
            self.previous_tight = matches!(kind, L_ANGLE) || is_generic_angle(token);
            if kind == STAR {
                self.previous_tight = false;
            }
            return;
        }

        if self.pending_newlines > 0 {
            let wanted = self.newlines_before(kind);
            if wanted > 0 {
                self.break_line(wanted);
            } else {
                self.pending_newlines = 0;
                if self.wants_space_before(kind, token.text()) {
                    self.space();
                }
            }
        } else if self.wants_space_before(kind, token.text()) {
            self.space();
        }

        self.write_raw(token.text());

        match kind {
            L_PAREN | L_BRACK => self.parens += 1,
            R_PAREN | R_BRACK => self.parens = self.parens.saturating_sub(1),
            L_CURLY => {
                if is_initializer(token) {
                    self.lists += 1;
                } else {
                    self.depth += 1;
                    self.pending_newlines = 1;
                }
            }
            R_CURLY => {
                // A `}` that ends a body stands alone. One that a keyword
                // follows keeps that keyword on its line, as in `} else {`.
                let follower = next.map(SyntaxToken::kind);
                if !matches!(
                    follower,
                    Some(SEMICOLON | COMMA | R_PAREN | ELSE_KW | WHILE_KW)
                ) {
                    self.pending_newlines = 1;
                }
            }
            // A `;` inside a `for` header or an initializer keeps its place.
            SEMICOLON if self.parens == 0 && self.lists == 0 => self.pending_newlines = 1,
            COLON if matches!(self.previous_code, Some(IDENT | DEFAULT_KW)) => {
                // A label and a `case` both end their line.
                self.pending_newlines = 1;
            }
            _ => {}
        }

        self.previous = Some(kind);
        self.previous_code = Some(kind);
        token.text().clone_into(&mut self.previous_text);
        self.previous_tight = false;
    }

    /// Returns how many newlines belong before a token.
    ///
    /// A top level item gets a blank line before it, so the file reads in
    /// sections. Everything else gets one.
    fn newlines_before(&self, kind: SyntaxKind) -> usize {
        if self.pending_newlines >= 2 && self.depth == 0 && kind != R_CURLY {
            return 2;
        }
        self.pending_newlines.min(1)
    }

    /// Reports whether a space belongs before a token.
    fn wants_space_before(&self, kind: SyntaxKind, text: &str) -> bool {
        let Some(previous) = self.previous_code else {
            return false;
        };
        if self.at_line_start {
            return false;
        }
        // Rule Z-2 first. Two tokens that would read as one, or as two other
        // tokens, need a space whatever the style says. `a++ + ++a` is the
        // case that finds this: without the space it reads `a ++ ++ a`.
        if must_separate(&self.previous_text, text) {
            return true;
        }
        // A generic list binds to the name on each side of it.
        if self.previous_tight {
            return false;
        }
        // The `@` of a directive binds to the word after it.
        if previous == AT {
            return false;
        }

        // A separator always takes a space after it, whatever follows. So
        // does the brace of an initializer, as in `new Person { .name = x }`.
        if matches!(previous, COMMA) || (previous == L_CURLY && self.lists > 0) {
            return true;
        }
        // Nothing follows an opening bracket, and nothing precedes a closing
        // one or a separator.
        if matches!(previous, L_PAREN | L_BRACK) {
            return false;
        }
        if matches!(kind, R_PAREN | R_BRACK | COMMA | SEMICOLON) {
            return false;
        }
        // A member access binds tight on both sides.
        if matches!(kind, DOT | ARROW | COLON2) || matches!(previous, DOT | ARROW | COLON2) {
            return false;
        }
        // A step operator binds to the value it steps.
        if matches!(kind, PLUS2 | MINUS2) || matches!(previous, PLUS2 | MINUS2) {
            return false;
        }
        // A call, an index, and a generic argument list all bind to the name.
        if matches!(kind, L_PAREN) && self.name_takes_a_group() {
            return false;
        }
        if kind == L_BRACK {
            return false;
        }
        // A prefix operator binds to what follows it.
        if self.previous_is_prefix() {
            return false;
        }
        true
    }

    /// Reports whether the name before a parenthesis takes it as a group.
    ///
    /// `if (x)` puts a space, because `if` is a keyword. `f(x)` does not,
    /// because `f` is a name.
    fn name_takes_a_group(&self) -> bool {
        !matches!(
            self.previous_code,
            Some(IF_KW | WHILE_KW | FOR_KW | SWITCH_KW | RETURN_KW | SIZEOF_KW | ALIGNOF_KW)
        )
    }

    /// Reports whether the last token was a prefix operator.
    fn previous_is_prefix(&self) -> bool {
        matches!(self.previous_code, Some(AMP | TILDE | BANG))
            && !matches!(self.previous_text.as_str(), "")
    }

    /// Writes text with no layout of its own.
    fn write_raw(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.out.push_str(text);
        self.at_line_start = false;
    }

    /// Writes one space, unless the line already ends with one.
    fn space(&mut self) {
        if self.out.ends_with(' ') || self.out.is_empty() || self.at_line_start {
            return;
        }
        self.out.push(' ');
    }

    /// Ends the line and indents the next one.
    fn break_line(&mut self, count: usize) {
        if self.out.is_empty() {
            self.pending_newlines = 0;
            return;
        }
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        // Rule Z-1. At most one blank line, so a count above two collapses.
        let wanted = count.min(2);
        let existing = trailing_newlines(&self.out);
        for _ in existing..wanted {
            self.out.push('\n');
        }
        for _ in 0..self.depth * INDENT {
            self.out.push(' ');
        }
        self.at_line_start = true;
        self.pending_newlines = 0;
    }
}

/// Reports whether two tokens need a space between them.
///
/// Rule Z-2. Writing them next to each other must give the same two tokens.
/// The check lexes the pair, which is exact and needs no table of operators.
///
/// | Pair | Without a space |
/// |---|---|
/// | `++` then `+` | `++ ++` |
/// | `/` then `*` | the start of a comment |
/// | `<` then `<` | a shift |
fn must_separate(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let joined = format!("{left}{right}");
    let together: Vec<&str> = split_tokens(&joined);
    together != vec![left, right]
}

/// Returns the text of every token in one string, trivia left out.
fn split_tokens(text: &str) -> Vec<&str> {
    tokenize(text)
        .tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token| &text[token.span.start as usize..token.span.end as usize])
        .collect()
}

/// Reports whether a brace opens an initializer rather than a block.
fn is_initializer(token: &SyntaxToken) -> bool {
    token
        .parent()
        .is_some_and(|node| matches!(node.kind(), INIT_LIST | ENUM_BODY))
}

/// Reports whether a star is a pointer declarator rather than a product.
fn is_pointer(token: &SyntaxToken) -> bool {
    token.parent().is_some_and(|node| node.kind() == POINTER)
}

/// Reports whether an angle bracket belongs to a generic list.
///
/// Rule L-6 makes `<` a comparison or the start of a list, and the tree
/// already decided which. A comparison keeps its spaces.
fn is_generic_angle(token: &SyntaxToken) -> bool {
    if !matches!(token.kind(), L_ANGLE | R_ANGLE | SHR) {
        return false;
    }
    token
        .parent()
        .is_some_and(|node| matches!(node.kind(), GENERIC_ARGS | GENERIC_PARAMS))
}

/// Returns how many newlines the text already ends with.
fn trailing_newlines(text: &str) -> usize {
    text.chars().rev().take_while(|c| *c == '\n').count()
}
