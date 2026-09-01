//! The Lark parser.
//!
//! The parser is a hand written recursive descent parser. It always produces a
//! tree, even for a file that does not parse, so a language server can work on
//! broken code.
//!
//! The parser asks a [`NameOracle`] whenever a decision needs to know if an
//! identifier binds to a type. Rule L-6 is the main case. Phase 1 uses
//! [`NoNames`], so a decision that needs a user type falls back to the
//! syntactic rules. Phase 2 supplies the real oracle.
//!
//! [`NoNames`]: crate::oracle::NoNames

use lark_diag::{Code, LK0102, LK0110};
use lark_span::Span;
use rowan::{Checkpoint, GreenNode, GreenNodeBuilder};

// The parser matches on nearly every variant of the enum, so it imports
// them all. Rule C-2.1 asks for the shape of the walk to stay readable,
// and a list of 175 names in the header does not help a reader.
#[allow(clippy::enum_glob_use)]
use crate::kind::SyntaxKind::{self, *};
use crate::lexer::{Token, tokenize};
use crate::oracle::{Binding, NameOracle};
use crate::tree::{Lark, SyntaxNode};

/// A problem that the lexer or the parser found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyntaxError {
    /// The diagnostic code.
    pub code: Code,
    /// The region that the problem covers.
    pub span: Span,
}

/// The result of a parser run.
#[derive(Clone, Debug)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    /// Returns the root of the tree.
    #[must_use]
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Returns every problem, in source order.
    #[must_use]
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Returns the text of the whole tree.
    ///
    /// The result equals the source. That is invariant R.
    #[must_use]
    pub fn text(&self) -> String {
        self.syntax().text().to_string()
    }

    /// Returns the tree in the form that a snapshot test compares.
    #[must_use]
    pub fn tree_text(&self) -> String {
        crate::tree::print(&self.syntax())
    }
}

/// Parses source text into a tree.
pub fn parse(source: &str, oracle: &dyn NameOracle) -> Parse {
    let lexed = tokenize(source);
    let mut errors: Vec<SyntaxError> = lexed
        .errors
        .iter()
        .map(|error| SyntaxError {
            code: error.code,
            span: error.span,
        })
        .collect();

    let mut parser = Parser {
        source,
        tokens: lexed.tokens,
        position: 0,
        builder: GreenNodeBuilder::new(),
        errors: Vec::new(),
        oracle,
        half_angle: false,
        scopes: Vec::new(),
    };
    parser.source_file();

    errors.append(&mut parser.errors);
    errors.sort_by_key(|error| (error.span.start, error.code));
    Parse {
        green: parser.builder.finish(),
        errors,
    }
}

/// The parser state.
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    /// The index of the next token, trivia included.
    position: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
    oracle: &'a dyn NameOracle,
    /// True when the first half of a `>>` token closed a generic list.
    ///
    /// `Box<Data<int>>` ends with one `>>` token, because C needs `>>` for a
    /// shift. The parser splits it into two `>` tokens in the tree.
    half_angle: bool,
    /// The local scopes that are open at the cursor, outermost first.
    ///
    /// Rule L-6 resolves a name in the innermost enclosing scope. A local
    /// declaration therefore hides a type of the same name from that point on.
    /// See rule L-16.
    scopes: Vec<Vec<(String, Binding)>>,
}

// ---------------------------------------------------------------------------
// Token navigation
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Returns the index of the token `steps` places ahead, trivia skipped.
    fn index_of(&self, steps: usize) -> Option<usize> {
        let mut left = steps;
        for index in self.position..self.tokens.len() {
            if self.tokens[index].kind.is_trivia() {
                continue;
            }
            if left == 0 {
                return Some(index);
            }
            left -= 1;
        }
        None
    }

    /// Returns the kind of the token `steps` places ahead, trivia skipped.
    fn nth(&self, steps: usize) -> SyntaxKind {
        if steps == 0 && self.half_angle {
            return R_ANGLE;
        }
        self.index_of(steps)
            .map_or(EOF, |index| self.tokens[index].kind)
    }

    /// Returns the text of the token `steps` places ahead, trivia skipped.
    fn nth_text(&self, steps: usize) -> &str {
        self.index_of(steps)
            .and_then(|index| self.source.get(self.tokens[index].span.as_range()))
            .unwrap_or_default()
    }

    /// Returns the span of the current token, or an empty span at the end.
    fn span(&self) -> Span {
        self.index_of(0).map_or_else(
            || Span::at(u32::try_from(self.source.len()).unwrap_or(u32::MAX)),
            |index| self.tokens[index].span,
        )
    }

    /// Reports whether the current token has the given kind.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.nth(0) == kind
    }

    /// Reports whether the current token is an identifier with the given text.
    ///
    /// Every Lark keyword is contextual, so the parser matches on text here.
    /// See rule L-3.
    fn at_word(&self, word: &str) -> bool {
        self.nth(0) == IDENT && self.nth_text(0) == word
    }

    /// Reports whether the token `steps` ahead is an identifier with the text.
    fn nth_word(&self, steps: usize, word: &str) -> bool {
        self.nth(steps) == IDENT && self.nth_text(steps) == word
    }

    /// Reports whether the parser reached the end of the file.
    fn at_end(&self) -> bool {
        self.index_of(0).is_none()
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Adds every trivia token at the cursor to the tree.
    ///
    /// The parser calls this before it opens a node, so trivia attaches to the
    /// parent rather than to the node that follows it.
    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.position) {
            if !token.kind.is_trivia() {
                break;
            }
            let text = self.source.get(token.span.as_range()).unwrap_or_default();
            self.builder.token(Lark::raw(token.kind), text);
            self.position += 1;
        }
    }

    /// Opens a node.
    fn start(&mut self, kind: SyntaxKind) {
        self.eat_trivia();
        self.builder.start_node(Lark::raw(kind));
    }

    /// Closes the innermost open node.
    fn finish(&mut self) {
        self.builder.finish_node();
    }

    /// Marks a place where a node can start later.
    fn checkpoint(&mut self) -> Checkpoint {
        self.eat_trivia();
        self.builder.checkpoint()
    }

    /// Wraps everything added since the checkpoint in a node.
    fn wrap(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, Lark::raw(kind));
        self.builder.finish_node();
    }

    /// Adds the current token to the tree and advances.
    fn bump(&mut self) {
        // A `>>` that closed one generic list still holds a second `>`. Any
        // path that leaves the generic grammar must consume it, or `nth` keeps
        // reporting a token that the parser never takes.
        if self.half_angle {
            self.half_angle = false;
            self.eat_trivia();
            self.builder.token(Lark::raw(R_ANGLE), ">");
            self.position += 1;
            return;
        }
        self.eat_trivia();
        let Some(token) = self.tokens.get(self.position) else {
            return;
        };
        let text = self.source.get(token.span.as_range()).unwrap_or_default();
        self.builder.token(Lark::raw(token.kind), text);
        self.position += 1;
    }

    /// Reports a problem at the current token.
    fn error(&mut self, code: Code) {
        let span = self.span();
        // One report per position is enough. A cascade helps nobody.
        if self
            .errors
            .last()
            .is_some_and(|last| last.span.start == span.start)
        {
            return;
        }
        self.errors.push(SyntaxError { code, span });
    }

    /// Adds the token when it matches, and reports a problem when it does not.
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            return true;
        }
        self.error(LK0110);
        false
    }

    /// Wraps the current token in an error node and advances.
    ///
    /// The token stays in the tree, so invariant R holds.
    fn bump_error(&mut self) {
        if self.at_end() {
            return;
        }
        self.error(LK0110);
        self.start(ERROR);
        self.bump();
        self.finish();
    }

    /// Adds one `>` that closes a generic list, and splits `>>` when needed.
    fn bump_close_angle(&mut self) {
        if self.half_angle {
            self.half_angle = false;
            self.builder.token(Lark::raw(R_ANGLE), ">");
            self.position += 1;
            return;
        }
        if self.at(SHR) {
            self.eat_trivia();
            self.half_angle = true;
            self.builder.token(Lark::raw(R_ANGLE), ">");
            return;
        }
        self.bump();
    }

    /// Skips tokens into an error node until one of the kinds, or the end.
    fn recover_to(&mut self, stops: &[SyntaxKind]) {
        if self.at_end() || stops.contains(&self.nth(0)) {
            return;
        }
        self.error(LK0110);
        self.start(ERROR);
        while !self.at_end() && !stops.contains(&self.nth(0)) {
            self.bump();
        }
        self.finish();
    }
}

impl Lark {
    /// Converts a kind for the tree builder.
    fn raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.to_raw())
    }
}

// ---------------------------------------------------------------------------
// Local scopes
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Opens a scope.
    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    /// Closes every scope past a depth.
    fn truncate_scopes(&mut self, depth: usize) {
        self.scopes.truncate(depth);
    }

    /// Records a name in the innermost open scope.
    fn declare(&mut self, name: &str, binding: Binding) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_owned(), binding));
        }
    }

    /// Returns the binding of a name, innermost scope first.
    ///
    /// Rule L-6. A local declaration hides a module level type of the same
    /// name.
    fn binding_of(&self, name: &str) -> Binding {
        for scope in self.scopes.iter().rev() {
            for (declared, binding) in scope.iter().rev() {
                if declared == name {
                    return *binding;
                }
            }
        }
        self.oracle.binding(name)
    }
}

// ---------------------------------------------------------------------------
// Classification helpers
// ---------------------------------------------------------------------------

/// The words that mark a declaration in Lark. Rule L-3 makes each contextual.
const DECL_MARKERS: &[&str] = &["init", "gc_leaf", "gc_safe"];

/// Compiler extensions that carry a parenthesized group. See rule C-4.
const EXTENSIONS_WITH_GROUP: &[&str] = &[
    "__attribute__",
    "__attribute",
    "__asm",
    "__asm__",
    "__declspec",
];

/// Compiler extensions that name a type. See rule C-4f.
///
/// `__typeof__(x)` names the type of `x`, and `__auto_type` names an inferred
/// type. Neither carries a meaning in Lark, but both stand where a type stands,
/// so the parser reads them as a type specifier instead of skipping them.
const TYPEOF_WORDS: &[&str] = &["__typeof", "__typeof__", "typeof", "__auto_type"];

/// Compiler extensions that stand alone. See rule C-4.
const EXTENSIONS_ALONE: &[&str] = &[
    "__extension__",
    "_Nullable",
    "_Nonnull",
    "_Null_unspecified",
    "_Nullable_result",
];

/// What the declaration specifiers say about the declaration that follows.
#[derive(Clone, Copy, Debug, Default)]
struct Specifiers {
    /// The specifiers end with a `}` body. Rule O-25 then drops the semicolon.
    saw_body: bool,
    /// The declaration introduces a type name rather than a variable.
    is_typedef: bool,
}

/// What a declarator introduces.
#[derive(Clone, Debug, Default)]
struct Declared {
    /// The name, when the declarator is not abstract.
    name: Option<String>,
    /// The last suffix is a parameter list.
    is_function: bool,
}

impl Parser<'_> {
    /// Reports whether the cursor stands on a compiler extension.
    fn at_extension(&self) -> bool {
        if self.nth(0) != IDENT {
            return false;
        }
        let text = self.nth_text(0);
        EXTENSIONS_ALONE.contains(&text)
            || (EXTENSIONS_WITH_GROUP.contains(&text) && self.nth(1) == L_PAREN)
    }

    /// Returns the number of steps to the token after a parenthesized group.
    ///
    /// The scan stops after a fixed number of steps, so a malformed file costs
    /// nothing.
    fn steps_after_paren_group(&self, open: usize) -> Option<usize> {
        const LIMIT: usize = 128;
        let mut depth = 0usize;
        let mut steps = open;
        while steps < open + LIMIT {
            match self.nth(steps) {
                L_PAREN => depth += 1,
                R_PAREN => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(steps + 1);
                    }
                }
                EOF => return None,
                _ => {}
            }
            steps += 1;
        }
        None
    }

    /// Reports whether the cursor stands on a macro used as an attribute.
    ///
    /// Rule C-4k. A module is not preprocessed, so `static int PRINTF(2) name(`
    /// arrives with the macro unexpanded. A declarator would end the group, so
    /// a name or a star after the group marks the macro instead. The emitted C
    /// keeps the text, and the C compiler expands it.
    fn at_attribute_macro(&self) -> bool {
        if self.nth(0) != IDENT || self.nth(1) != L_PAREN {
            return false;
        }
        // A declarator can carry an attribute of its own, as in
        // `int printf(const char *, ...) __attribute__((format(...)));`. The
        // group there belongs to `printf`, so an extension after it does not
        // make `printf` a macro.
        let Some(after) = self.steps_after_paren_group(1) else {
            return false;
        };
        match self.nth(after) {
            STAR => true,
            IDENT => {
                let text = self.nth_text(after);
                !EXTENSIONS_ALONE.contains(&text) && !EXTENSIONS_WITH_GROUP.contains(&text)
            }
            _ => false,
        }
    }

    /// Reads every compiler extension at the cursor.
    ///
    /// Rule C-4 makes the parser read past an extension without a meaning for
    /// it. Rule C-6 keeps a header from stopping a build.
    fn eat_extensions(&mut self) {
        while self.at_extension() {
            self.start(EXTENSION);
            let text = self.nth_text(0).to_owned();
            self.bump();
            if EXTENSIONS_WITH_GROUP.contains(&text.as_str()) {
                self.paren_group();
            }
            self.finish();
        }
    }

    /// Reports whether the token `steps` ahead can begin a type specifier.
    fn nth_starts_type(&self, steps: usize) -> bool {
        match self.nth(steps) {
            VOID_KW | CHAR_KW | SHORT_KW | INT_KW | LONG_KW | FLOAT_KW | DOUBLE_KW | SIGNED_KW
            | UNSIGNED_KW | BOOL_KW | COMPLEX_KW | IMAGINARY_KW | STRUCT_KW | UNION_KW
            | ENUM_KW | ATOMIC_KW => true,
            IDENT => {
                let text = self.nth_text(steps);
                if TYPEOF_WORDS.contains(&text) {
                    // `__auto_type x = e;` takes no group. The others do.
                    return text == "__auto_type" || self.nth(steps + 1) == L_PAREN;
                }
                text == "gc" || text == "managed" || self.binding_of(text) == Binding::Type
            }
            _ => false,
        }
    }

    /// Reports whether the token `steps` ahead can begin a declaration specifier.
    fn nth_starts_decl_specifier(&self, steps: usize) -> bool {
        match self.nth(steps) {
            CONST_KW | VOLATILE_KW | RESTRICT_KW | TYPEDEF_KW | EXTERN_KW | STATIC_KW
            | REGISTER_KW | THREAD_LOCAL_KW | INLINE_KW | NORETURN_KW | AUTO_KW | ALIGNAS_KW => {
                true
            }
            IDENT => {
                let text = self.nth_text(steps);
                DECL_MARKERS.contains(&text) || self.nth_starts_type(steps)
            }
            _ => self.nth_starts_type(steps),
        }
    }

    /// Reports whether the tokens at `steps` look like a type, by shape.
    ///
    /// The strict test needs the oracle, which phase 1 does not have. A user
    /// type in `gc Person* p` is an identifier that the parser must still read
    /// as a type. The shape of the tokens after it answers the question.
    fn nth_looks_like_type(&self, steps: usize) -> bool {
        if self.nth_starts_type(steps) {
            return true;
        }
        self.nth(steps) == IDENT && matches!(self.nth(steps + 1), IDENT | STAR | L_ANGLE | COLON2)
    }

    /// Reports whether `export` at the cursor is the Lark keyword.
    ///
    /// Rule L-3 recognizes it only where valid C11 cannot parse. In C, `export`
    /// is an ordinary identifier, so `export x;` declares `x` of type `export`.
    /// The keyword reading needs a type after it.
    fn export_is_keyword(&self) -> bool {
        if !self.at_word("export") {
            return false;
        }
        // `export Person* p;` has no keyword after `export`, so the shape of
        // the following tokens decides.
        self.nth(1) == AT || self.nth_starts_decl_specifier(1) || self.nth_looks_like_type(1)
    }

    /// Reports whether `auto` at the cursor asks for type inference.
    ///
    /// Rule L-5. A type specifier after `auto` makes it the C storage class.
    fn auto_is_inference(&self) -> bool {
        self.at(AUTO_KW) && !self.nth_starts_decl_specifier(1)
    }

    /// Reports whether the cursor starts a declaration rather than an expression.
    ///
    /// Inside a block, `a * b;` is a declaration when `a` names a type, and an
    /// expression when it does not. The oracle answers that. Phase 1 falls back
    /// to the shape of the tokens.
    fn at_declaration(&self) -> bool {
        match self.nth(0) {
            CONST_KW | VOLATILE_KW | RESTRICT_KW | TYPEDEF_KW | EXTERN_KW | STATIC_KW
            | REGISTER_KW | THREAD_LOCAL_KW | INLINE_KW | NORETURN_KW | AUTO_KW | ALIGNAS_KW
            | STATIC_ASSERT_KW | VOID_KW | CHAR_KW | SHORT_KW | INT_KW | LONG_KW | FLOAT_KW
            | DOUBLE_KW | SIGNED_KW | UNSIGNED_KW | BOOL_KW | COMPLEX_KW | IMAGINARY_KW
            | STRUCT_KW | UNION_KW | ENUM_KW => true,
            IDENT => {
                let text = self.nth_text(0);
                if text == "gc" || text == "managed" || DECL_MARKERS.contains(&text) {
                    return true;
                }
                // Rule C-4f. `__typeof__(x) y;` declares `y`.
                if TYPEOF_WORDS.contains(&text) {
                    return text == "__auto_type" || self.nth(1) == L_PAREN;
                }
                if self.binding_of(text) == Binding::Type {
                    return true;
                }
                if self.binding_of(text) == Binding::Value {
                    return false;
                }
                // `Person p;` and `Person* p;` are declarations by shape.
                if self.nth(1) == IDENT {
                    return true;
                }
                // `Box<int> a;` is a declaration and `swap<Person>(&a, &b);`
                // is a call. Both start the same way, so look past the closing
                // angle. See rule L-6.
                if self.nth(1) == L_ANGLE && self.nth_starts_type(2) {
                    return self
                        .steps_after_generic_args(1)
                        .is_some_and(|after| matches!(self.nth(after), IDENT | STAR));
                }
                // `stdio::FILE* f;` is a declaration. `stdio::printf(x);` is a
                // call. The token after the name decides. See rule N-2.
                if self.nth(1) == COLON2 && self.nth(2) == IDENT {
                    return matches!(self.nth(3), IDENT | STAR | L_ANGLE);
                }
                self.nth(1) == STAR && self.stars_then_name_then_declaration_end()
            }
            _ => false,
        }
    }

    /// Returns the number of steps to the token after a generic argument list.
    ///
    /// The scan stops at a token that cannot appear in a type, and it stops
    /// after a fixed number of steps, so a malformed file costs nothing.
    fn steps_after_generic_args(&self, open: usize) -> Option<usize> {
        const LIMIT: usize = 64;
        let mut depth = 0usize;
        let mut steps = open;
        while steps < open + LIMIT {
            match self.nth(steps) {
                L_ANGLE => depth += 1,
                R_ANGLE => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(steps + 1);
                    }
                }
                SHR => {
                    depth = depth.checked_sub(2)?;
                    if depth == 0 {
                        return Some(steps + 1);
                    }
                }
                SEMICOLON | L_CURLY | R_CURLY | EOF => return None,
                _ => {}
            }
            steps += 1;
        }
        None
    }

    /// Reports whether a run of stars leads to a name and then a declaration end.
    fn stars_then_name_then_declaration_end(&self) -> bool {
        let mut steps = 1;
        while self.nth(steps) == STAR {
            steps += 1;
        }
        self.nth(steps) == IDENT && matches!(self.nth(steps + 1), SEMICOLON | EQ | COMMA | L_BRACK)
    }

    /// Reports whether the cursor is a `<` that opens a generic argument list.
    ///
    /// Rule L-6. In type position the answer is always yes, because a
    /// comparison cannot appear there. In expression position the oracle
    /// decides.
    fn at_generic_args(&self, type_position: bool) -> bool {
        if !self.at(L_ANGLE) {
            return false;
        }
        if type_position {
            return true;
        }
        if self.nth(1) != IDENT {
            return self.nth_starts_type(1);
        }
        // Rule L-6, and rule L-15 for the unbound case.
        match self.binding_of(self.nth_text(1)) {
            Binding::Type => true,
            Binding::Value => false,
            Binding::Unbound => {
                self.oracle.is_complete() || matches!(self.nth_text(1), "gc" | "managed")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Parses the whole file.
    fn source_file(&mut self) {
        self.builder.start_node(Lark::raw(SOURCE_FILE));
        // The file itself is a scope. A typedef here must answer rule L-6 in
        // expression position, so a cast such as `(size_t) 0` reads correctly.
        self.push_scope();
        while !self.at_end() {
            let before = self.position;
            self.item();
            if self.position == before {
                self.bump_error();
            }
        }
        // Trailing trivia belongs to the root, so invariant R holds.
        self.eat_trivia();
        self.builder.finish_node();
    }

    /// Parses one top level item.
    fn item(&mut self) {
        if self.at(AT) && !self.export_is_keyword() {
            self.at_item(None);
            return;
        }
        // C11 6.7.10. A static assertion is a declaration at file scope too.
        if self.at(STATIC_ASSERT_KW) {
            self.static_assert_decl();
            return;
        }
        // Rule C-4j. A header guards its declarations with `extern "C" { }`
        // behind `#ifdef __cplusplus`. Lark evaluates no directive, so it sees
        // the block and reads it as a plain group of items.
        if self.at(EXTERN_KW) && self.nth(1) == STRING_LITERAL {
            self.linkage_block();
            return;
        }

        let exported = self.export_is_keyword();
        let skip = usize::from(exported);

        if self.nth(skip) == AT {
            let checkpoint = self.checkpoint();
            self.bump();
            self.at_item(Some(checkpoint));
            return;
        }
        // Rule N-19. `namespace a { ... }` nests inside the namespace that the
        // directory gives. Rule L-3 allows the word elsewhere, and no valid C11
        // reads `namespace a {` at item level.
        if self.nth_word(skip, "namespace")
            && self.nth(skip + 1) == IDENT
            && self.nth(skip + 2) == L_CURLY
        {
            self.namespace_def(exported);
            return;
        }
        if self.nth_word(skip, "iface") && self.nth(skip + 1) == IDENT {
            self.iface_def(exported);
            return;
        }
        // `for` is a C11 keyword, so it arrives as FOR_KW, not as an identifier.
        // Rule O-26 lets the interface carry arguments, so the word before
        // `for` can be `Show<int>` rather than `Show`.
        if self.nth_word(skip, "impl")
            && self.nth(skip + 1) == IDENT
            && self.nth(self.after_generic_args(skip + 2)) == FOR_KW
        {
            self.impl_def(exported);
            return;
        }
        self.declaration_or_function(exported);
    }

    /// Parses `extern "C" { ... }` or `extern "C" declaration`.
    ///
    /// See rule C-4j. The linkage name carries no meaning in Lark.
    fn linkage_block(&mut self) {
        self.start(LINKAGE_BLOCK);
        self.bump();
        self.bump();
        if self.at(L_CURLY) {
            self.bump();
            while !self.at_end() && !self.at(R_CURLY) {
                let before = self.position;
                self.item();
                if self.position == before {
                    self.bump_error();
                }
            }
            self.expect(R_CURLY);
        } else {
            self.item();
        }
        self.finish();
    }

    /// Parses an item that starts with `@`.
    fn at_item(&mut self, exported: Option<Checkpoint>) {
        let checkpoint = exported.unwrap_or_else(|| self.checkpoint());
        let word = self.nth_text(1).to_owned();
        match word.as_str() {
            "import" => {
                self.bump();
                self.bump();
                if self.at(IDENT) {
                    // Rule N-16. A directory contributes one segment, so an
                    // import names a path rather than a single word.
                    self.start(NAME);
                    self.expect(IDENT);
                    self.path_tail();
                    self.finish();
                } else {
                    self.error(LK0110);
                }
                self.wrap(checkpoint, IMPORT_DIRECTIVE);
            }
            "global" => {
                self.bump();
                self.bump();
                if self.at(L_PAREN) {
                    self.global_attach();
                }
                if self.at(IDENT) {
                    self.name();
                } else {
                    self.error(LK0110);
                }
                self.block_of_declarations();
                self.wrap(checkpoint, GLOBAL_BLOCK);
            }
            _ => {
                self.bump();
                self.error(LK0110);
                self.recover_to(&[SEMICOLON, R_CURLY]);
                if self.at(SEMICOLON) {
                    self.bump();
                }
                self.wrap(checkpoint, ERROR);
            }
        }
    }

    /// Parses `(function)` or `(function, order)` after `@global`.
    fn global_attach(&mut self) {
        self.start(GLOBAL_ATTACH);
        self.expect(L_PAREN);
        if self.at(IDENT) {
            self.name_ref();
        } else {
            self.error(LK0110);
        }
        if self.at(COMMA) {
            self.bump();
            if self.at(INT_NUMBER) {
                self.bump();
            } else {
                self.error(LK0110);
            }
        }
        self.expect(R_PAREN);
        self.finish();
    }

    /// Parses a brace group that holds declarations.
    fn block_of_declarations(&mut self) {
        if !self.at(L_CURLY) {
            self.error(LK0110);
            return;
        }
        self.bump();
        while !self.at_end() && !self.at(R_CURLY) {
            let before = self.position;
            self.declaration_or_function(false);
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_CURLY);
    }

    /// Parses `iface Name { ... }`.
    fn iface_def(&mut self, exported: bool) {
        self.start(IFACE_DEF);
        if exported {
            self.bump();
        }
        self.bump();
        self.name();
        // Rule O-25. An interface takes parameters, and each set of arguments
        // gives one method table, the way rule G-1 gives one record layout.
        if self.at(L_ANGLE) {
            self.generic_params();
        }
        if self.at(L_CURLY) {
            self.bump();
            while !self.at_end() && !self.at(R_CURLY) {
                let before = self.position;
                self.iface_method();
                if self.position == before {
                    self.bump_error();
                }
            }
            self.expect(R_CURLY);
        } else {
            self.error(LK0110);
        }
        self.finish();
    }

    /// Parses `namespace name { items }`.
    ///
    /// Rule N-19. The block nests inside the namespace that holds it, and it
    /// takes no `export` of its own, because rule N-7 exports each item.
    fn namespace_def(&mut self, exported: bool) {
        self.start(NAMESPACE_DEF);
        if exported {
            self.bump();
        }
        self.bump();
        self.name();
        self.push_scope();
        self.expect(L_CURLY);
        while !self.at_end() && !self.at(R_CURLY) {
            let before = self.position;
            self.item();
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_CURLY);
        self.scopes.pop();
        self.finish();
    }

    /// Parses one method signature inside an interface.
    fn iface_method(&mut self) {
        self.start(IFACE_METHOD);
        let _ = self.decl_specifiers();
        let _ = self.declarator();
        self.expect(SEMICOLON);
        self.finish();
    }

    /// Parses `impl Iface for Type { ... }`.
    fn impl_def(&mut self, exported: bool) {
        self.start(IMPL_DEF);
        if exported {
            self.bump();
        }
        self.bump();
        self.name_ref();
        // Rule O-26. `impl Show<int> for Buf<int>` names the instantiation on
        // each side, and either side stands alone when it takes no parameters.
        if self.at(L_ANGLE) {
            self.generic_args();
        }
        self.bump();
        self.name_ref();
        if self.at(L_ANGLE) {
            self.generic_args();
        }
        if self.at(L_CURLY) {
            self.bump();
            while !self.at_end() && !self.at(R_CURLY) {
                let before = self.position;
                self.declaration_or_function(false);
                if self.position == before {
                    self.bump_error();
                }
            }
            self.expect(R_CURLY);
        } else {
            self.error(LK0110);
        }
        self.finish();
    }

    /// Parses a declaration, or a function definition.
    fn declaration_or_function(&mut self, exported: bool) {
        let checkpoint = self.checkpoint();
        if exported {
            self.bump();
        }

        let specifiers = self.decl_specifiers();
        let binding = if specifiers.is_typedef {
            Binding::Type
        } else {
            Binding::Value
        };

        if self.at(SEMICOLON) {
            self.bump();
            self.wrap(checkpoint, DECLARATION);
            return;
        }

        // Rule O-25. A definition that ends with `}` needs no semicolon when it
        // declares no variable.
        if specifiers.saw_body && !self.at_declarator_start() {
            self.wrap(checkpoint, DECLARATION);
            return;
        }

        // A parameter belongs to the function, not to the enclosing scope.
        let depth = self.scopes.len();
        self.push_scope();

        let declarator = self.checkpoint();
        let declared = self.declarator();

        // C11 6.9.1. An old style definition puts the parameter types between
        // the declarator and the body, as in `int add(a, b) int a; int b; {`.
        if declared.is_function && !self.at(L_CURLY) && self.at_declaration() {
            self.kr_param_list();
        }

        if declared.is_function && self.at(L_CURLY) {
            self.block_stmt();
            self.truncate_scopes(depth);
            if let Some(name) = declared.name {
                self.declare(&name, binding);
            }
            self.wrap(checkpoint, FN_DEF);
            return;
        }

        self.truncate_scopes(depth);
        if let Some(name) = &declared.name {
            let name = name.clone();
            self.declare(&name, binding);
        }

        if self.at(EQ) {
            self.bump();
            self.initializer();
        }
        self.wrap(declarator, INIT_DECLARATOR);

        while self.at(COMMA) {
            self.bump();
            let next = self.checkpoint();
            let depth = self.scopes.len();
            self.push_scope();
            let declared = self.declarator();
            self.truncate_scopes(depth);
            if let Some(name) = &declared.name {
                let name = name.clone();
                self.declare(&name, binding);
            }
            if self.at(EQ) {
                self.bump();
                self.initializer();
            }
            self.wrap(next, INIT_DECLARATOR);
        }

        if !self.at(SEMICOLON) {
            self.recover_to(&[SEMICOLON, R_CURLY, EOF]);
        }
        if self.at(SEMICOLON) {
            self.bump();
        } else {
            self.error(LK0110);
        }
        self.wrap(checkpoint, DECLARATION);
    }
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Reports whether the cursor begins a declarator after a `}` body.
    ///
    /// Rule O-25 lets a definition end without a semicolon, so the parser must
    /// tell `struct S { } x;` from `struct S { }` followed by the next item.
    /// A name is a declarator only when a declaration can continue after it.
    fn at_declarator_start(&self) -> bool {
        match self.nth(0) {
            STAR | L_PAREN | L_BRACK => true,
            IDENT => matches!(self.nth(1), SEMICOLON | COMMA | EQ | L_BRACK | L_PAREN),
            _ => false,
        }
    }

    /// Parses a name that a declaration introduces.
    fn name(&mut self) {
        self.start(NAME);
        self.expect(IDENT);
        self.finish();
    }

    /// Parses a name that a declaration refers to.
    fn name_ref(&mut self) {
        self.start(NAME_REF);
        self.expect(IDENT);
        self.finish();
    }

    /// Parses the specifiers at the head of a declaration.
    ///
    /// Returns what the specifiers say about the declaration that follows.
    fn decl_specifiers(&mut self) -> Specifiers {
        self.start(DECL_SPECIFIERS);
        let mut saw_type = false;
        let mut result = Specifiers::default();

        loop {
            self.eat_extensions();
            match self.nth(0) {
                ATOMIC_KW if self.nth(1) == L_PAREN => {
                    self.bump();
                    self.paren_group();
                    saw_type = true;
                }
                // Rule C-4k. An unexpanded macro stands where an attribute
                // stands. It must come after a type, so a declarator that
                // opens a parameter list is never mistaken for one.
                IDENT if saw_type && self.at_attribute_macro() => {
                    self.start(EXTENSION);
                    self.bump();
                    self.paren_group();
                    self.finish();
                }
                // Rule C-4f. `__typeof__(x)` names the type of `x`, and
                // `__auto_type` names an inferred type.
                IDENT if TYPEOF_WORDS.contains(&self.nth_text(0)) && !saw_type => {
                    let takes_group = self.nth_text(0) != "__auto_type";
                    self.bump();
                    if takes_group && self.at(L_PAREN) {
                        self.paren_group();
                    }
                    saw_type = true;
                }
                TYPEDEF_KW => {
                    result.is_typedef = true;
                    self.bump();
                }
                CONST_KW | VOLATILE_KW | RESTRICT_KW | EXTERN_KW | STATIC_KW | REGISTER_KW
                | THREAD_LOCAL_KW | INLINE_KW | NORETURN_KW | ATOMIC_KW => {
                    self.bump();
                }
                AUTO_KW => {
                    // Rule L-5. Inference ends the specifiers.
                    let inference = self.auto_is_inference();
                    self.bump();
                    if inference {
                        break;
                    }
                }
                ALIGNAS_KW => {
                    self.bump();
                    self.paren_group();
                }
                VOID_KW | CHAR_KW | SHORT_KW | INT_KW | LONG_KW | FLOAT_KW | DOUBLE_KW
                | SIGNED_KW | UNSIGNED_KW | BOOL_KW | COMPLEX_KW | IMAGINARY_KW => {
                    self.bump();
                    saw_type = true;
                }
                // C allows one struct, union, or enum specifier. A second one
                // starts the next item, as rule O-25 makes the semicolon
                // optional.
                STRUCT_KW | UNION_KW if !saw_type => {
                    result.saw_body = self.record_specifier(false);
                    saw_type = true;
                }
                ENUM_KW if !saw_type => {
                    result.saw_body = self.enum_specifier();
                    saw_type = true;
                }
                IDENT => {
                    let text = self.nth_text(0).to_owned();
                    if text == "managed" && !saw_type && matches!(self.nth(1), STRUCT_KW | UNION_KW)
                    {
                        result.saw_body = self.record_specifier(true);
                        saw_type = true;
                    } else if (text == "gc" || DECL_MARKERS.contains(&text.as_str()))
                        && !saw_type
                        && self.nth_looks_like_type(1)
                    {
                        // A qualifier precedes the type it qualifies. After a
                        // type specifier, a `gc` starts the next declaration,
                        // which rule O-25 allows without a semicolon.
                        self.bump();
                    } else if !saw_type
                        && !matches!(self.nth(1), SEMICOLON | COMMA | L_PAREN | DOT | ARROW)
                    {
                        self.type_name_ref();
                        saw_type = true;
                    } else {
                        break;
                    }
                }
                _ => break,
            }

            // Rule O-25 drops the semicolon after a `}` body, so nothing can
            // follow the body except a declarator. A qualifier and a storage
            // class both come before the type.
            if result.saw_body {
                // An attribute can trail a record body, as in
                // `struct S { ... } __attribute__((aligned(4)));`. Rule C-4
                // reads it, and rule O-25a still ends the specifiers here.
                self.eat_extensions();
                break;
            }
        }

        self.finish();
        result
    }

    /// Consumes `::name` for as long as the input holds one.
    ///
    /// Rule N-17. A qualified name reaches any depth, because a namespace
    /// nests inside another one. The last segment is the name, and every
    /// segment before it is the path that holds it.
    ///
    /// Returns true when at least one segment followed.
    fn path_tail(&mut self) -> bool {
        let mut qualified = false;
        while self.at(COLON2) {
            self.bump();
            self.expect(IDENT);
            qualified = true;
        }
        qualified
    }

    /// Parses a type name that a declaration refers to, with its arguments.
    ///
    /// The name can carry a module prefix, as in `stdio::FILE`. See rule N-2.
    fn type_name_ref(&mut self) {
        let checkpoint = self.checkpoint();
        self.bump();
        let qualified = self.path_tail();
        self.wrap(checkpoint, if qualified { PATH } else { NAME_REF });
        if self.at(L_ANGLE) {
            self.generic_args();
        }
    }

    /// Parses `struct Name<T> { ... }`, with or without the `managed` marker.
    fn record_specifier(&mut self, managed: bool) -> bool {
        let union = self.nth(usize::from(managed)) == UNION_KW;
        self.start(if union { UNION_DEF } else { STRUCT_DEF });
        if managed {
            self.bump();
        }
        self.bump();
        if self.at(IDENT) {
            self.name();
            if self.at(L_ANGLE) {
                self.generic_params();
            }
        }
        let body = self.at(L_CURLY);
        if body {
            self.struct_body();
        }
        self.finish();
        body
    }

    /// Parses the field list of a struct or a union.
    fn struct_body(&mut self) {
        self.start(STRUCT_BODY);
        self.bump();
        // C11 6.2.3 puts a member in its own namespace, so a field named
        // `value` must not hide a type named `value` outside the record.
        let depth = self.scopes.len();
        self.push_scope();
        while !self.at_end() && !self.at(R_CURLY) {
            let before = self.position;
            self.field_decl();
            if self.position == before {
                self.bump_error();
            }
        }
        self.truncate_scopes(depth);
        self.expect(R_CURLY);
        self.finish();
    }

    /// Parses one field of a struct or a union.
    fn field_decl(&mut self) {
        self.start(FIELD_DECL);
        let _ = self.decl_specifiers();
        if !self.at(SEMICOLON) {
            loop {
                let _ = self.declarator();
                if self.at(COLON) {
                    self.bump();
                    self.conditional_expr();
                }
                if self.at(COMMA) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        if !self.at(SEMICOLON) {
            self.recover_to(&[SEMICOLON, R_CURLY]);
        }
        if self.at(SEMICOLON) {
            self.bump();
        }
        self.finish();
    }

    /// Parses `enum Name { ... }`.
    fn enum_specifier(&mut self) -> bool {
        self.start(ENUM_DEF);
        self.bump();
        if self.at(IDENT) {
            self.name();
        }
        let body = self.at(L_CURLY);
        if body {
            self.start(ENUM_BODY);
            self.bump();
            while !self.at_end() && !self.at(R_CURLY) {
                self.start(ENUMERATOR);
                self.name();
                self.eat_extensions();
                if self.at(EQ) {
                    self.bump();
                    self.conditional_expr();
                }
                self.finish();
                if self.at(COMMA) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(R_CURLY);
            self.finish();
        }
        self.finish();
        body
    }

    /// Parses a declarator, and reports what it introduces.
    fn declarator(&mut self) -> Declared {
        self.start(DECLARATOR);
        let mut declared = Declared::default();
        while self.at(STAR) || self.at(CARET) {
            self.start(POINTER);
            self.bump();
            loop {
                self.eat_extensions();
                if matches!(
                    self.nth(0),
                    CONST_KW | VOLATILE_KW | RESTRICT_KW | ATOMIC_KW
                ) || self.at_word("gc")
                {
                    self.bump();
                    continue;
                }
                break;
            }
            self.finish();
        }

        if self.at(L_PAREN) && self.nth(1) != R_PAREN && !self.nth_starts_decl_specifier(1) {
            self.bump();
            declared.name = self.declarator().name;
            self.expect(R_PAREN);
        } else if self.at(IDENT) {
            declared.name = Some(self.nth_text(0).to_owned());
            self.name();
            if self.at(L_ANGLE) {
                self.generic_params();
            }
        }

        // A parameter is visible from its own declarator onward.
        if let Some(name) = &declared.name {
            let name = name.clone();
            self.declare(&name, Binding::Value);
        }

        loop {
            // An extension can follow a declarator, as in `__asm("name")`.
            self.eat_extensions();
            if self.at(L_BRACK) {
                self.array_suffix();
                declared.is_function = false;
            } else if self.at(L_PAREN) {
                self.param_list();
                declared.is_function = true;
            } else {
                break;
            }
        }
        self.eat_extensions();

        self.finish();
        declared
    }

    /// Parses `[ ... ]` after a declarator.
    fn array_suffix(&mut self) {
        self.start(ARRAY_SUFFIX);
        self.bump();
        // C11 6.7.6.2. A parameter can carry a qualifier or `static` inside the
        // brackets, as in `char buf[restrict]` or `char buf[static 8]`.
        while matches!(
            self.nth(0),
            CONST_KW | VOLATILE_KW | RESTRICT_KW | STATIC_KW
        ) {
            self.bump();
        }
        if !self.at(R_BRACK) {
            self.expr();
        }
        self.expect(R_BRACK);
        self.finish();
    }

    /// Parses the declaration list of an old style function definition.
    ///
    /// C11 6.9.1 allows `int add(a, b) int a; int b; { ... }`. The list holds
    /// ordinary declarations, and it ends at the body.
    fn kr_param_list(&mut self) {
        self.start(KR_PARAM_LIST);
        while !self.at_end() && !self.at(L_CURLY) && self.at_declaration() {
            let before = self.position;
            self.declaration_or_function(false);
            if self.position == before {
                self.bump_error();
                break;
            }
        }
        self.finish();
    }

    /// Parses a parameter list.
    fn param_list(&mut self) {
        self.start(PARAM_LIST);
        self.bump();
        while !self.at_end() && !self.at(R_PAREN) {
            let before = self.position;
            self.start(PARAM);
            if self.at(ELLIPSIS) {
                self.bump();
            } else {
                let _ = self.decl_specifiers();
                let _ = self.declarator();
            }
            self.finish();
            if self.at(COMMA) {
                self.bump();
            } else {
                break;
            }
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_PAREN);
        self.finish();
    }

    /// Parses `<T, U>` in a definition.
    fn generic_params(&mut self) {
        self.start(GENERIC_PARAMS);
        self.bump();
        while !self.at_end() && !self.at(R_ANGLE) && !self.at(SHR) {
            self.name();
            if self.at(COMMA) {
                self.bump();
            } else {
                break;
            }
        }
        if self.at(R_ANGLE) || self.at(SHR) {
            self.bump_close_angle();
        } else {
            self.error(LK0102);
        }
        self.finish();
    }

    /// Returns the index after a balanced `<...>` that starts at `index`.
    ///
    /// The index comes back unchanged when no list starts there, so a caller
    /// reads the same position whether the list is present or absent. The scan
    /// counts a `>>` as two closes, because the lexer gives one token for it.
    ///
    /// The scan stops at a token that no argument list holds, so a comparison
    /// never reads as a list. Rule L-6 gives the same answer in the grammar.
    fn after_generic_args(&self, index: usize) -> usize {
        if self.nth(index) != L_ANGLE {
            return index;
        }
        let mut depth = 0usize;
        let mut at = index;
        loop {
            match self.nth(at) {
                L_ANGLE => depth += 1,
                R_ANGLE => {
                    depth -= 1;
                    if depth == 0 {
                        return at + 1;
                    }
                }
                SHR => {
                    if depth <= 2 {
                        return at + 1;
                    }
                    depth -= 2;
                }
                // A list holds a type, so anything that ends a statement or an
                // item ends the scan as well.
                SEMICOLON | L_CURLY | R_CURLY | EOF => return index,
                _ => {}
            }
            at += 1;
        }
    }

    /// Parses `<int, Person*>` at a use.
    fn generic_args(&mut self) {
        self.start(GENERIC_ARGS);
        self.bump();
        while !self.at_end() && !self.at(R_ANGLE) && !self.at(SHR) {
            let before = self.position;
            self.type_name();
            if self.at(COMMA) {
                self.bump();
            } else {
                break;
            }
            if self.position == before {
                self.bump_error();
            }
        }
        if self.at(R_ANGLE) || self.at(SHR) {
            self.bump_close_angle();
        } else {
            self.error(LK0102);
        }
        self.finish();
    }

    /// Parses a type name, which is a declaration with no name.
    fn type_name(&mut self) {
        self.start(TYPE_NAME);
        let _ = self.decl_specifiers();
        let _ = self.declarator();
        self.finish();
    }

    /// Parses a balanced `( ... )` group and keeps every token.
    fn paren_group(&mut self) {
        if !self.at(L_PAREN) {
            return;
        }
        let mut depth = 0;
        loop {
            match self.nth(0) {
                L_PAREN => depth += 1,
                R_PAREN => depth -= 1,
                EOF => break,
                _ => {}
            }
            if self.at_end() {
                break;
            }
            self.bump();
            if depth == 0 {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Parses `{ ... }`.
    fn block_stmt(&mut self) {
        self.start(BLOCK_STMT);
        self.push_scope();
        self.expect(L_CURLY);
        while !self.at_end() && !self.at(R_CURLY) {
            let before = self.position;
            self.statement();
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_CURLY);
        self.scopes.pop();
        self.finish();
    }

    /// Parses one statement.
    fn statement(&mut self) {
        match self.nth(0) {
            L_CURLY => self.block_stmt(),
            SEMICOLON => {
                self.start(EMPTY_STMT);
                self.bump();
                self.finish();
            }
            IF_KW => self.if_stmt(),
            WHILE_KW => self.while_stmt(),
            DO_KW => self.do_stmt(),
            FOR_KW => self.for_stmt(),
            SWITCH_KW => self.switch_stmt(),
            CASE_KW => {
                self.start(CASE_STMT);
                self.bump();
                self.conditional_expr();
                self.expect(COLON);
                self.statement();
                self.finish();
            }
            DEFAULT_KW => {
                self.start(DEFAULT_STMT);
                self.bump();
                self.expect(COLON);
                self.statement();
                self.finish();
            }
            GOTO_KW => {
                self.start(GOTO_STMT);
                self.bump();
                // Rule C-4h. `goto *p;` jumps to a computed label.
                if self.at(STAR) {
                    self.bump();
                    self.unary_expr();
                } else {
                    self.name_ref();
                }
                self.expect(SEMICOLON);
                self.finish();
            }
            BREAK_KW => {
                self.start(BREAK_STMT);
                self.bump();
                self.expect(SEMICOLON);
                self.finish();
            }
            CONTINUE_KW => {
                self.start(CONTINUE_STMT);
                self.bump();
                self.expect(SEMICOLON);
                self.finish();
            }
            RETURN_KW => {
                self.start(RETURN_STMT);
                self.bump();
                if !self.at(SEMICOLON) {
                    self.expr();
                }
                self.expect(SEMICOLON);
                self.finish();
            }
            AT => self.init_stmt(),
            // C11 6.7.10. A static assertion is a declaration, so it stands
            // wherever a declaration stands.
            STATIC_ASSERT_KW => self.static_assert_decl(),
            // A `__asm__` statement carries a template and operand groups that
            // Lark gives no meaning. See rule C-4e.
            IDENT if self.at_asm() => self.asm_stmt(),
            IDENT if self.nth(1) == COLON => {
                self.start(LABELED_STMT);
                self.name();
                self.bump();
                self.statement();
                self.finish();
            }
            _ if self.at_declaration() => {
                self.start(DECL_STMT);
                self.declaration_or_function(false);
                self.finish();
            }
            _ => {
                self.start(EXPR_STMT);
                self.expr();
                if !self.at(SEMICOLON) {
                    self.recover_to(&[SEMICOLON, R_CURLY]);
                }
                if self.at(SEMICOLON) {
                    self.bump();
                } else {
                    self.error(LK0110);
                }
                self.finish();
            }
        }
    }

    /// Reports whether the cursor stands on an inline assembly statement.
    fn at_asm(&self) -> bool {
        if self.nth(0) != IDENT || !matches!(self.nth_text(0), "__asm" | "__asm__" | "asm") {
            return false;
        }
        // A qualifier can sit between the word and the group, as in
        // `__asm__ volatile ("nop")`.
        let mut step = 1;
        while matches!(self.nth(step), VOLATILE_KW | INLINE_KW | GOTO_KW) {
            step += 1;
        }
        self.nth(step) == L_PAREN
    }

    /// Parses an inline assembly statement. See rule C-4e.
    fn asm_stmt(&mut self) {
        self.start(ASM_STMT);
        self.bump();
        while matches!(self.nth(0), VOLATILE_KW | INLINE_KW | GOTO_KW) {
            self.bump();
        }
        if self.at(L_PAREN) {
            self.paren_group();
        }
        if self.at(SEMICOLON) {
            self.bump();
        }
        self.finish();
    }

    /// Parses `_Static_assert (expr, "message");`. C11 6.7.10.
    fn static_assert_decl(&mut self) {
        self.start(STATIC_ASSERT_DECL);
        self.bump();
        if self.at(L_PAREN) {
            self.paren_group();
        }
        if self.at(SEMICOLON) {
            self.bump();
        }
        self.finish();
    }

    /// Parses `@init name;`.
    fn init_stmt(&mut self) {
        self.start(INIT_STMT);
        self.bump();
        if self.nth_word(0, "init") {
            self.bump();
            self.name_ref();
            self.expect(SEMICOLON);
        } else {
            self.error(LK0110);
            self.recover_to(&[SEMICOLON, R_CURLY]);
            if self.at(SEMICOLON) {
                self.bump();
            }
        }
        self.finish();
    }

    fn if_stmt(&mut self) {
        self.start(IF_STMT);
        self.bump();
        self.paren_condition();
        self.statement();
        if self.at(ELSE_KW) {
            self.bump();
            self.statement();
        }
        self.finish();
    }

    fn while_stmt(&mut self) {
        self.start(WHILE_STMT);
        self.bump();
        self.paren_condition();
        self.statement();
        self.finish();
    }

    fn do_stmt(&mut self) {
        self.start(DO_STMT);
        self.bump();
        self.statement();
        self.expect(WHILE_KW);
        self.paren_condition();
        self.expect(SEMICOLON);
        self.finish();
    }

    fn for_stmt(&mut self) {
        self.start(FOR_STMT);
        self.push_scope();
        self.bump();
        self.expect(L_PAREN);
        if self.at(SEMICOLON) {
            self.bump();
        } else if self.at_declaration() {
            self.start(DECL_STMT);
            self.declaration_or_function(false);
            self.finish();
        } else {
            self.expr();
            self.expect(SEMICOLON);
        }
        if !self.at(SEMICOLON) {
            self.expr();
        }
        self.expect(SEMICOLON);
        if !self.at(R_PAREN) {
            self.expr();
        }
        self.expect(R_PAREN);
        self.statement();
        self.scopes.pop();
        self.finish();
    }

    fn switch_stmt(&mut self) {
        self.start(SWITCH_STMT);
        self.bump();
        self.paren_condition();
        self.statement();
        self.finish();
    }

    /// Parses `( expression )` after a control keyword.
    fn paren_condition(&mut self) {
        if !self.expect(L_PAREN) {
            return;
        }
        self.expr();
        self.expect(R_PAREN);
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// Returns the binding power of a binary operator, highest last.
fn binding_power(kind: SyntaxKind) -> Option<u8> {
    let power = match kind {
        PIPE2 => 1,
        AMP2 => 2,
        PIPE => 3,
        CARET => 4,
        AMP => 5,
        EQ2 | BANG_EQ => 6,
        L_ANGLE | R_ANGLE | LT_EQ | GT_EQ => 7,
        SHL | SHR => 8,
        PLUS | MINUS => 9,
        STAR | SLASH | PERCENT => 10,
        _ => return None,
    };
    Some(power)
}

/// Reports whether the kind is an assignment operator.
fn is_assign_op(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        EQ | STAR_EQ
            | SLASH_EQ
            | PERCENT_EQ
            | PLUS_EQ
            | MINUS_EQ
            | SHL_EQ
            | SHR_EQ
            | AMP_EQ
            | CARET_EQ
            | PIPE_EQ
    )
}

impl Parser<'_> {
    /// Parses a full expression, comma operator included.
    fn expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.assignment_expr();
        while self.at(COMMA) {
            self.bump();
            self.assignment_expr();
            self.wrap(checkpoint, BIN_EXPR);
        }
    }

    /// Parses an assignment, which groups to the right.
    fn assignment_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.conditional_expr();
        if is_assign_op(self.nth(0)) {
            self.bump();
            self.assignment_expr();
            self.wrap(checkpoint, ASSIGN_EXPR);
        }
    }

    /// Parses `a ? b : c`, which groups to the right.
    fn conditional_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.binary_expr(1);
        if self.at(QUESTION) {
            self.bump();
            self.expr();
            self.expect(COLON);
            self.conditional_expr();
            self.wrap(checkpoint, COND_EXPR);
        }
    }

    /// Parses a run of binary operators at or above the given binding power.
    fn binary_expr(&mut self, minimum: u8) {
        let checkpoint = self.checkpoint();
        self.unary_expr();
        while let Some(power) = binding_power(self.nth(0)) {
            if power < minimum {
                break;
            }
            self.bump();
            self.binary_expr(power + 1);
            self.wrap(checkpoint, BIN_EXPR);
        }
    }

    /// Parses a prefix operator, a cast, or a postfix expression.
    fn unary_expr(&mut self) {
        match self.nth(0) {
            // Rule C-4h. `&&label` takes the address of a label. GCC and Clang
            // write it, and the operand is a label name, not an expression.
            AMP2 if self.nth(1) == IDENT => {
                self.start(PREFIX_EXPR);
                self.bump();
                self.name_ref();
                self.finish();
            }
            PLUS2 | MINUS2 | AMP | STAR | PLUS | MINUS | TILDE | BANG => {
                self.start(PREFIX_EXPR);
                self.bump();
                self.unary_expr();
                self.finish();
            }
            SIZEOF_KW | ALIGNOF_KW => {
                let alignof = self.at(ALIGNOF_KW);
                self.start(if alignof { ALIGNOF_EXPR } else { SIZEOF_EXPR });
                self.bump();
                if self.at(L_PAREN) && self.nth_starts_decl_specifier(1) {
                    self.bump();
                    self.type_name();
                    self.expect(R_PAREN);
                } else {
                    self.unary_expr();
                }
                self.finish();
            }
            L_PAREN if self.nth_starts_decl_specifier(1) => {
                let checkpoint = self.checkpoint();
                self.bump();
                self.type_name();
                self.expect(R_PAREN);
                if self.at(L_CURLY) {
                    self.initializer();
                    self.wrap(checkpoint, COMPOUND_LITERAL_EXPR);
                    self.postfix_tail(checkpoint);
                } else {
                    self.unary_expr();
                    self.wrap(checkpoint, CAST_EXPR);
                }
            }
            _ => self.postfix_expr(),
        }
    }

    /// Parses a primary expression and every suffix after it.
    fn postfix_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.primary_expr();
        self.postfix_tail(checkpoint);
    }

    /// Parses the suffixes of a postfix expression.
    fn postfix_tail(&mut self, checkpoint: Checkpoint) {
        loop {
            match self.nth(0) {
                L_BRACK => {
                    self.bump();
                    self.expr();
                    self.expect(R_BRACK);
                    self.wrap(checkpoint, INDEX_EXPR);
                }
                L_PAREN => {
                    self.arg_list();
                    self.wrap(checkpoint, CALL_EXPR);
                }
                DOT | ARROW => {
                    self.bump();
                    self.member_name();
                    if self.at(L_PAREN) {
                        self.arg_list();
                        self.wrap(checkpoint, METHOD_EXPR);
                    } else {
                        self.wrap(checkpoint, FIELD_EXPR);
                    }
                }
                PLUS2 | MINUS2 => {
                    self.bump();
                    self.wrap(checkpoint, POSTFIX_EXPR);
                }
                _ => break,
            }
        }
    }

    /// Parses `name` or `Iface::name` after a dot or an arrow.
    ///
    /// The qualified form disambiguates a method that two interfaces declare.
    /// See rule O-21.
    fn member_name(&mut self) {
        let checkpoint = self.checkpoint();
        if !self.at(IDENT) {
            self.error(LK0110);
            return;
        }
        self.bump();
        if self.path_tail() {
            self.wrap(checkpoint, PATH);
        } else {
            self.wrap(checkpoint, NAME_REF);
        }
    }

    /// Parses an argument list.
    fn arg_list(&mut self) {
        self.start(ARG_LIST);
        self.bump();
        while !self.at_end() && !self.at(R_PAREN) {
            let before = self.position;
            // Rule C-4g. A macro such as `va_arg(ap, int)` or
            // `offsetof(struct S, m)` puts a type name where an argument goes.
            // Lark does not expand the macro, so the parser reads the type.
            if self.argument_is_a_type_name() {
                self.type_name();
            } else {
                self.assignment_expr();
            }
            if self.at(COMMA) {
                self.bump();
            } else {
                break;
            }
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_PAREN);
        self.finish();
    }

    /// Reports whether the argument at the cursor is a type name.
    ///
    /// The test is deliberately narrow. The tokens must start a type, and the
    /// argument must end right after the type, so an ordinary expression that
    /// begins with a type name never takes this path.
    fn argument_is_a_type_name(&self) -> bool {
        const LIMIT: usize = 16;
        if !self.nth_starts_type(0) {
            return false;
        }
        // A record specifier with a body is a definition, not an argument.
        if matches!(self.nth(0), STRUCT_KW | UNION_KW | ENUM_KW) && self.nth(2) == L_CURLY {
            return false;
        }
        let mut step = 0;
        let mut after_tag_keyword = false;
        while step < LIMIT {
            let kind = self.nth(step);
            match kind {
                COMMA | R_PAREN => return step > 0,
                // Only the tokens that a type name can hold.
                VOID_KW | CHAR_KW | SHORT_KW | INT_KW | LONG_KW | FLOAT_KW | DOUBLE_KW
                | SIGNED_KW | UNSIGNED_KW | BOOL_KW | COMPLEX_KW | IMAGINARY_KW | STRUCT_KW
                | UNION_KW | ENUM_KW | CONST_KW | VOLATILE_KW | RESTRICT_KW | STAR | L_BRACK
                | R_BRACK | INT_NUMBER => step += 1,
                // A tag follows `struct`, `union`, or `enum`, and the name is
                // a tag rather than a type, so the oracle cannot answer it.
                IDENT if after_tag_keyword || step == 0 || self.nth_starts_type(step) => step += 1,
                _ => return false,
            }
            after_tag_keyword = matches!(kind, STRUCT_KW | UNION_KW | ENUM_KW);
        }
        false
    }

    /// Parses a literal, a name, a parenthesized expression, or `new`.
    fn primary_expr(&mut self) {
        match self.nth(0) {
            INT_NUMBER | FLOAT_NUMBER | CHAR_LITERAL => {
                self.start(LITERAL_EXPR);
                self.bump();
                self.finish();
            }
            STRING_LITERAL => {
                self.start(LITERAL_EXPR);
                while self.at(STRING_LITERAL) {
                    self.bump();
                }
                self.finish();
            }
            // A statement expression, `({ ... })`. GCC and Clang write it, and
            // rule C-4d reads it as an expression whose value is the last one.
            L_PAREN if self.nth(1) == L_CURLY => {
                self.start(STMT_EXPR);
                self.bump();
                self.block_stmt();
                self.expect(R_PAREN);
                self.finish();
            }
            L_PAREN => {
                self.start(PAREN_EXPR);
                self.bump();
                self.expr();
                self.expect(R_PAREN);
                self.finish();
            }
            // C11 6.5.1.1.
            GENERIC_KW if self.nth(1) == L_PAREN => self.generic_selection(),
            // Rule L-3. In expression position, a name after `new` is a type.
            IDENT if self.at_word("new") && (self.nth(1) == IDENT || self.nth_starts_type(1)) => {
                self.new_expr();
            }
            IDENT => {
                self.start(NAME_EXPR);
                let checkpoint = self.checkpoint();
                self.bump();
                if self.path_tail() {
                    self.wrap(checkpoint, PATH);
                } else {
                    self.wrap(checkpoint, NAME_REF);
                }
                if self.at_generic_args(false) {
                    self.generic_args();
                }
                self.finish();
            }
            _ => self.missing_expr(),
        }
    }

    /// Parses `_Generic (expr, type: expr, default: expr)`. C11 6.5.1.1.
    ///
    /// The controlling expression comes first. Each association names a type
    /// or `default`, so the parser reads a type name where rule L-6 would
    /// otherwise weigh a comparison.
    fn generic_selection(&mut self) {
        self.start(GENERIC_SELECTION);
        self.bump();
        self.expect(L_PAREN);
        self.assignment_expr();
        while self.at(COMMA) {
            self.bump();
            self.start(GENERIC_ASSOC);
            if self.at(DEFAULT_KW) {
                self.bump();
            } else {
                self.type_name();
            }
            self.expect(COLON);
            self.assignment_expr();
            self.finish();
        }
        self.expect(R_PAREN);
        self.finish();
    }

    /// Reports a missing expression.
    ///
    /// A token that ends a statement or a group is not consumed. The caller
    /// needs it to finish, and eating it turns one problem into two.
    fn missing_expr(&mut self) {
        if matches!(
            self.nth(0),
            SEMICOLON | R_PAREN | R_CURLY | R_BRACK | COMMA | COLON | EOF
        ) {
            self.error(LK0110);
            self.start(ERROR);
            self.finish();
            return;
        }
        self.bump_error();
    }

    /// Parses `new Type { ... }` or `new Type[n]`.
    ///
    /// Rule L-3 recognizes `new` only in expression position with a type after
    /// it, so a C program that names a variable `new` still parses.
    fn new_expr(&mut self) {
        let checkpoint = self.checkpoint();
        self.bump();
        self.start(TYPE_NAME);
        let _ = self.decl_specifiers();
        while self.at(STAR) || self.at(CARET) {
            self.start(POINTER);
            self.bump();
            self.finish();
        }
        self.finish();

        if self.at(L_BRACK) {
            self.bump();
            self.expr();
            self.expect(R_BRACK);
            self.wrap(checkpoint, NEW_ARRAY_EXPR);
        } else if self.at(L_CURLY) {
            self.initializer();
            self.wrap(checkpoint, NEW_EXPR);
        } else {
            self.error(LK0110);
            self.wrap(checkpoint, NEW_EXPR);
        }
    }

    /// Parses an initializer, which is a brace list or an expression.
    fn initializer(&mut self) {
        if self.at(L_CURLY) {
            self.init_list();
        } else {
            self.assignment_expr();
        }
    }

    /// Parses `{ .field = value, ... }`.
    fn init_list(&mut self) {
        self.start(INIT_LIST);
        self.bump();
        while !self.at_end() && !self.at(R_CURLY) {
            let before = self.position;
            if self.at(DOT) || self.at(L_BRACK) {
                self.start(DESIGNATED_INIT);
                self.start(DESIGNATOR);
                while self.at(DOT) || self.at(L_BRACK) {
                    if self.at(DOT) {
                        self.bump();
                        self.name_ref();
                    } else {
                        self.bump();
                        self.conditional_expr();
                        // Rule C-4i. `[0 ... 3] = v` sets a range of elements.
                        // GCC and Clang write it, and Lark reads the bounds.
                        if self.at(ELLIPSIS) {
                            self.bump();
                            self.conditional_expr();
                        }
                        self.expect(R_BRACK);
                    }
                }
                self.finish();
                self.expect(EQ);
                self.initializer();
                self.finish();
            } else {
                self.initializer();
            }
            if self.at(COMMA) {
                self.bump();
            } else {
                break;
            }
            if self.position == before {
                self.bump_error();
            }
        }
        self.expect(R_CURLY);
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use lark_diag::{LK0102, LK0110};

    use super::{Parse, parse};
    use crate::kind::SyntaxKind;
    use crate::oracle::{KnownNames, NameOracle, NoNames};

    fn parse_ok(source: &str) -> Parse {
        let parsed = parse(source, &NoNames);
        assert!(
            parsed.errors().is_empty(),
            "{source:?} reported {:?}\n{}",
            parsed.errors(),
            parsed.tree_text()
        );
        assert_eq!(parsed.text(), source, "invariant R fails for {source:?}");
        parsed
    }

    /// Returns every node kind in the tree, in document order.
    fn nodes(parsed: &Parse) -> Vec<SyntaxKind> {
        parsed
            .syntax()
            .descendants()
            .map(|node| node.kind())
            .collect()
    }

    /// Reports whether the tree holds a node of the given kind.
    fn has(parsed: &Parse, kind: SyntaxKind) -> bool {
        nodes(parsed).contains(&kind)
    }

    /// Returns the token kinds of the last node with the given kind.
    fn tokens_of(parsed: &Parse, kind: SyntaxKind) -> Vec<SyntaxKind> {
        parsed
            .syntax()
            .descendants()
            .filter(|node| node.kind() == kind)
            .last()
            .map(|node| {
                node.descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .map(|token| token.kind())
                    .filter(|kind| !kind.is_trivia())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_with(source: &str, oracle: &dyn NameOracle) -> Parse {
        parse(source, oracle)
    }

    // -- the C subset ------------------------------------------------------

    #[test]
    fn parses_a_function_definition() {
        let parsed = parse_ok("int main(void) { return 0; }");
        assert!(has(&parsed, SyntaxKind::FN_DEF));
        assert!(has(&parsed, SyntaxKind::PARAM_LIST));
        assert!(has(&parsed, SyntaxKind::RETURN_STMT));
    }

    #[test]
    fn parses_every_control_statement() {
        let parsed = parse_ok(
            "void f(void) {\n\
             if (a) { } else { }\n\
             while (a) { }\n\
             do { } while (a);\n\
             for (int i = 0; i < 3; i++) { }\n\
             switch (a) { case 1: break; default: break; }\n\
             goto end;\n\
             end: ;\n\
             }",
        );
        for kind in [
            SyntaxKind::IF_STMT,
            SyntaxKind::WHILE_STMT,
            SyntaxKind::DO_STMT,
            SyntaxKind::FOR_STMT,
            SyntaxKind::SWITCH_STMT,
            SyntaxKind::CASE_STMT,
            SyntaxKind::DEFAULT_STMT,
            SyntaxKind::GOTO_STMT,
            SyntaxKind::LABELED_STMT,
        ] {
            assert!(has(&parsed, kind), "{} is missing", kind.name());
        }
    }

    #[test]
    fn binary_operators_group_by_precedence() {
        // `a + b * c` must nest the product inside the sum.
        let parsed = parse_ok("void f(void) { x = a + b * c; }");
        let text = parsed.tree_text();
        let sum = text.find("BIN_EXPR").unwrap_or(0);
        let product = text.rfind("BIN_EXPR").unwrap_or(0);
        assert!(
            product > sum,
            "the product must nest inside the sum\n{text}"
        );
    }

    #[test]
    fn parses_a_cast_and_a_paren_expression() {
        let parsed = parse_ok("void f(void) { g((void*)p); g((p)); }");
        assert!(has(&parsed, SyntaxKind::CAST_EXPR));
        assert!(has(&parsed, SyntaxKind::PAREN_EXPR));
    }

    #[test]
    fn parses_a_struct_an_enum_and_a_union() {
        let parsed = parse_ok("struct S { int x; }; enum E { A, B = 2 }; union U { int a; };");
        assert!(has(&parsed, SyntaxKind::STRUCT_DEF));
        assert!(has(&parsed, SyntaxKind::ENUM_DEF));
        assert!(has(&parsed, SyntaxKind::UNION_DEF));
    }

    #[test]
    fn a_preprocessor_line_stays_in_the_tree() {
        let parsed = parse_ok("#include <stdio.h>\nint x;");
        assert!(parsed.text().contains("#include <stdio.h>"));
    }

    // -- rule L-5, auto ----------------------------------------------------

    /// covers: L-5
    #[test]
    fn auto_with_no_type_asks_for_inference() {
        let parsed = parse_ok("void f(void) { auto x = 5; }");
        let specifiers = tokens_of(&parsed, SyntaxKind::DECL_SPECIFIERS);
        assert_eq!(
            specifiers,
            vec![SyntaxKind::AUTO_KW],
            "auto must end the specifiers"
        );
    }

    /// covers: L-5
    #[test]
    fn auto_before_a_type_is_the_c_storage_class() {
        let parsed = parse_ok("void f(void) { auto int x; }");
        let specifiers = tokens_of(&parsed, SyntaxKind::DECL_SPECIFIERS);
        assert_eq!(specifiers, vec![SyntaxKind::AUTO_KW, SyntaxKind::INT_KW]);
    }

    // -- rules L-3 and L-4, contextual keywords ----------------------------

    /// covers: L-3, L-4
    #[test]
    fn a_c_program_that_uses_a_lark_word_as_a_name_still_parses() {
        for source in [
            "int new; void f(void) { new = 5; }",
            "int gc; void f(void) { gc = gc + 1; }",
            "int init; int impl; int iface;",
            "void f(void) { int export; export = 1; }",
        ] {
            let parsed = parse(source, &NoNames);
            assert!(
                parsed.errors().is_empty(),
                "{source:?} reported {:?}",
                parsed.errors()
            );
        }
    }

    /// covers: L-3
    #[test]
    fn export_before_a_type_is_the_lark_keyword() {
        let parsed = parse_ok("export int f(void) { return 0; }");
        let tokens = tokens_of(&parsed, SyntaxKind::FN_DEF);
        assert_eq!(tokens.first(), Some(&SyntaxKind::IDENT));
    }

    // -- rule L-6, generics ------------------------------------------------

    /// covers: L-6
    #[test]
    fn a_name_that_is_not_a_type_before_an_angle_is_a_comparison() {
        let parsed = parse_with("void f(void) { g(a<b>(c)); }", &NoNames);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        assert!(
            has(&parsed, SyntaxKind::BIN_EXPR),
            "a<b>(c) must be a comparison"
        );
        assert!(!has(&parsed, SyntaxKind::GENERIC_ARGS));
    }

    /// covers: L-6
    #[test]
    fn a_name_that_is_a_type_before_an_angle_opens_generic_arguments() {
        let oracle = KnownNames::new(["Person"]);
        let parsed = parse_with("void f(void) { swap<Person>(&a, &b); }", &oracle);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-6, L-7
    #[test]
    fn a_type_keyword_after_an_angle_opens_generic_arguments() {
        let parsed = parse_ok("void f(void) { g(swap<int>(a)); }");
        assert!(has(&parsed, SyntaxKind::GENERIC_ARGS));
    }

    #[test]
    fn a_generic_declaration_takes_parameters_and_a_use_takes_arguments() {
        let parsed = parse_ok("struct Data<T> { T* p; } gc Data<int>* count;");
        assert!(has(&parsed, SyntaxKind::GENERIC_PARAMS));
        assert!(has(&parsed, SyntaxKind::GENERIC_ARGS));
    }

    /// covers: L-14
    #[test]
    fn a_half_consumed_shift_never_stalls_the_parser() {
        // The inner list closes with the first half of `>>`. The rest of the
        // input then leaves the generic grammar, and the second half must
        // still be consumed.
        for source in [
            "void f(void) { Box<Data<int>> nested; }",
            "void f(void) { g(a<Data<int>> b); }",
            "gc Box<Data<int>>* p;",
            "void f(void) { x = a >> b >> c; }",
        ] {
            let parsed = parse(source, &NoNames);
            assert_eq!(parsed.text(), source, "invariant R fails for {source:?}");
        }
    }

    /// covers: L-6, L-14
    #[test]
    fn nested_generic_arguments_split_a_shift_token() {
        let parsed = parse_ok("gc Box<Data<int>>* b;");
        assert_eq!(
            parsed.text(),
            "gc Box<Data<int>>* b;",
            "invariant R must survive the split"
        );
        let angles = parsed
            .syntax()
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::R_ANGLE)
            .count();
        assert_eq!(
            angles,
            2,
            "the shift token must become two closing angles\n{}",
            parsed.tree_text()
        );
    }

    /// covers: L-6
    #[test]
    fn a_generic_call_is_not_a_declaration() {
        let oracle = KnownNames::new(["Person"]);
        let parsed = parse_with("void f(void) { swap<Person>(&a, &b); }", &oracle);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        assert!(
            has(&parsed, SyntaxKind::CALL_EXPR),
            "{}",
            parsed.tree_text()
        );
        assert!(!has(&parsed, SyntaxKind::DECL_STMT));
    }

    /// covers: L-14
    #[test]
    fn a_shift_stays_a_shift_outside_a_generic_list() {
        let parsed = parse_ok("void f(void) { x = a >> b; }");
        assert!(has(&parsed, SyntaxKind::BIN_EXPR));
    }

    #[test]
    fn an_unterminated_generic_argument_list_reports_lk0102() {
        let parsed = parse("gc Data<int x;", &NoNames);
        assert!(
            parsed.errors().iter().any(|error| error.code == LK0102),
            "{:?}",
            parsed.errors()
        );
    }

    // -- Lark items --------------------------------------------------------

    #[test]
    fn parses_every_lark_item() {
        let parsed = parse_ok(
            "@import stdio\n\
             managed struct Person { gc char* name; }\n\
             iface Greet { void say_hi(Self this); }\n\
             impl Greet for Person { void say_hi(Person this) { } }\n\
             @global main_globals { gc Person* p = new Person { .name = \"x\" }; }\n\
             @global(main, 0) other { }\n\
             init void main(void) { @init main_globals; p.say_hi(); }\n",
        );
        for kind in [
            SyntaxKind::IMPORT_DIRECTIVE,
            SyntaxKind::STRUCT_DEF,
            SyntaxKind::IFACE_DEF,
            SyntaxKind::IFACE_METHOD,
            SyntaxKind::IMPL_DEF,
            SyntaxKind::GLOBAL_BLOCK,
            SyntaxKind::GLOBAL_ATTACH,
            SyntaxKind::INIT_STMT,
            SyntaxKind::NEW_EXPR,
            SyntaxKind::METHOD_EXPR,
            SyntaxKind::DESIGNATED_INIT,
        ] {
            assert!(has(&parsed, kind), "{} is missing", kind.name());
        }
    }

    #[test]
    fn parses_a_new_array_expression() {
        let parsed = parse_ok("void f(void) { gc char* b = new char[256]; }");
        assert!(has(&parsed, SyntaxKind::NEW_ARRAY_EXPR));
    }

    #[test]
    fn parses_a_qualified_method_call() {
        let parsed = parse_ok("void f(void) { x.Greet::say_hi(); }");
        assert!(has(&parsed, SyntaxKind::METHOD_EXPR));
        assert!(has(&parsed, SyntaxKind::PATH));
    }

    /// covers: N-2
    #[test]
    fn parses_a_qualified_type_name() {
        let parsed = parse_ok("gc stdio::FILE* handle;");
        assert!(has(&parsed, SyntaxKind::PATH), "{}", parsed.tree_text());
        assert!(has(&parsed, SyntaxKind::DECLARATION));
    }

    /// covers: L-15
    #[test]
    fn an_unbound_name_before_an_angle_needs_a_complete_table() {
        let source = "void f(void) { g(a<b>(c)); }";

        // An incomplete table reads a comparison.
        let parsed = parse(source, &NoNames);
        assert!(!has(&parsed, SyntaxKind::GENERIC_ARGS));

        // A complete table reads a generic argument list.
        let complete = KnownNames::new(Vec::<String>::new()).complete();
        let parsed = parse(source, &complete);
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-16, L-6
    #[test]
    fn a_local_declaration_hides_a_module_type_of_the_same_name() {
        let oracle = KnownNames::new(["Person"]).complete();

        // At module scope `Person` is a type, so this reads generic arguments.
        let parsed = parse_with("void f(void) { g(swap<Person>(a)); }", &oracle);
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );

        // A local variable named `Person` hides the type from that point on.
        let parsed = parse_with("void f(void) { int Person; g(swap<Person>(a)); }", &oracle);
        assert!(
            !has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-16
    #[test]
    fn a_local_declaration_stops_at_the_end_of_its_block() {
        let oracle = KnownNames::new(["Person"]).complete();
        let parsed = parse_with(
            "void f(void) { { int Person; } g(swap<Person>(a)); }",
            &oracle,
        );
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-16
    #[test]
    fn a_parameter_does_not_reach_the_next_function() {
        let oracle = KnownNames::new(["Person"]).complete();
        let parsed = parse_with(
            "void f(int Person) { }\nvoid g(void) { h(swap<Person>(a)); }",
            &oracle,
        );
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-16
    #[test]
    fn a_parameter_hides_a_type_inside_its_own_body() {
        let oracle = KnownNames::new(["Person"]).complete();
        let parsed = parse_with("void f(int Person) { g(swap<Person>(a)); }", &oracle);
        assert!(
            !has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-16
    #[test]
    fn a_local_typedef_makes_a_name_a_type() {
        let parsed = parse_ok("void f(void) { typedef int Local; g(swap<Local>(a)); }");
        assert!(
            has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    /// covers: L-6
    #[test]
    fn a_value_before_an_angle_stays_a_comparison_even_with_a_complete_table() {
        let oracle = KnownNames::new(Vec::<String>::new())
            .with_values(["a", "b"])
            .complete();
        let parsed = parse_with("void f(void) { g(a<b>(c)); }", &oracle);
        assert!(
            !has(&parsed, SyntaxKind::GENERIC_ARGS),
            "{}",
            parsed.tree_text()
        );
    }

    #[test]
    fn parses_a_module_qualified_call() {
        let parsed = parse_ok("void f(void) { stdio::printf(\"x\"); }");
        assert!(has(&parsed, SyntaxKind::PATH));
        assert!(has(&parsed, SyntaxKind::CALL_EXPR));
    }

    /// covers: O-25
    #[test]
    fn a_definition_that_ends_with_a_brace_needs_no_semicolon() {
        parse_ok("struct S { int x; }\nstruct T { int y; }\n");
        parse_ok("struct S { int x; };\n");
    }

    /// covers: O-25a
    #[test]
    fn a_storage_class_after_a_brace_body_starts_the_next_item() {
        // Rule O-25 drops the semicolon, so `static` here begins a new item.
        let parsed = parse_ok("struct Point { int x; }\nstatic int total = 0;\n");
        let declarations: Vec<SyntaxKind> = parsed
            .syntax()
            .children()
            .map(|node| node.kind())
            .filter(|kind| *kind == SyntaxKind::DECLARATION)
            .collect();
        assert_eq!(declarations.len(), 2, "{}", parsed.tree_text());
    }

    /// covers: O-25
    #[test]
    fn a_definition_that_declares_a_variable_keeps_the_semicolon() {
        let parsed = parse_ok("struct Point { int x; } origin;");
        assert!(has(&parsed, SyntaxKind::INIT_DECLARATOR));
    }

    // -- error recovery ----------------------------------------------------

    #[test]
    fn a_broken_file_still_produces_a_tree_and_an_error() {
        for source in [
            "int main(void) {",
            "}}}",
            "int x = ;",
            "iface {",
            "new",
            "@ bad",
        ] {
            let parsed = parse(source, &NoNames);
            assert_eq!(parsed.text(), source, "invariant R fails for {source:?}");
            assert!(
                !parsed.errors().is_empty(),
                "{source:?} must report a problem"
            );
            assert_eq!(parsed.syntax().kind(), SyntaxKind::SOURCE_FILE);
        }
    }

    #[test]
    fn one_position_reports_one_problem() {
        let parsed = parse("int x = ;", &NoNames);
        let mut starts: Vec<u32> = parsed
            .errors()
            .iter()
            .map(|error| error.span.start)
            .collect();
        let before = starts.len();
        starts.dedup();
        assert_eq!(
            starts.len(),
            before,
            "a position reported twice: {:?}",
            parsed.errors()
        );
    }

    #[test]
    fn an_unexpected_token_reports_lk0110() {
        let parsed = parse("void f(void) { x = ; }", &NoNames);
        assert!(parsed.errors().iter().any(|error| error.code == LK0110));
    }

    #[test]
    fn a_deeply_nested_input_terminates() {
        let source = format!(
            "void f(void) {{ x = {}1{}; }}",
            "(".repeat(200),
            ")".repeat(200)
        );
        let parsed = parse(&source, &NoNames);
        assert_eq!(parsed.text(), source);
    }

    #[test]
    fn a_run_of_unmatched_braces_terminates() {
        let source = "{".repeat(500);
        let parsed = parse(&source, &NoNames);
        assert_eq!(parsed.text(), source);
    }

    // -----------------------------------------------------------------------
    // Compiler extensions and full C11 declarations.
    // covers: C-4, C-4a, C-4b, C-4c
    // -----------------------------------------------------------------------

    /// Parses a source with no oracle, and returns the error count.
    fn error_count(source: &str) -> usize {
        parse(source, &NoNames).errors().len()
    }

    #[test]
    fn an_attribute_in_the_specifiers_is_read_and_ignored() {
        assert_eq!(error_count("__attribute__((noreturn)) void f(void);"), 0);
        assert_eq!(
            error_count("static __inline__ int g(void) { return 0; }"),
            0
        );
    }

    #[test]
    fn an_attribute_after_a_declarator_is_read() {
        assert_eq!(
            error_count(
                "int printf(const char *, ...) __attribute__((__format__(__printf__, 1, 2)));"
            ),
            0
        );
        assert_eq!(
            error_count("int fopen(const char *) __asm(\"_\" \"fopen\");"),
            0
        );
    }

    #[test]
    fn an_attribute_after_a_record_body_is_read() {
        assert_eq!(
            error_count("struct s { char z[4]; } __attribute__((aligned(4)));"),
            0
        );
    }

    #[test]
    fn an_attribute_on_an_enumerator_is_read() {
        assert_eq!(
            error_count("enum e { a __attribute__((deprecated)) = 1, b };"),
            0
        );
    }

    #[test]
    fn a_nullability_qualifier_is_read() {
        assert_eq!(error_count("int (* _Nullable close_it)(void *);"), 0);
        assert_eq!(error_count("void f(char * _Nonnull text);"), 0);
    }

    #[test]
    fn a_block_pointer_reads_as_a_pointer() {
        assert_eq!(
            error_count("void f(int (^ _Nonnull compare)(const void *));"),
            0
        );
    }

    #[test]
    fn a_reserved_spelling_of_a_keyword_reads_as_the_keyword() {
        assert_eq!(
            error_count("void f(int *__restrict a, __signed__ int b);"),
            0
        );
        assert_eq!(error_count("__const int x = 1;"), 0);
    }

    #[test]
    fn an_array_parameter_takes_a_qualifier() {
        // C11 6.7.6.2. The brackets can hold a qualifier or `static`.
        assert_eq!(error_count("int f(int a[restrict]);"), 0);
        assert_eq!(error_count("int f(int a[static 4]);"), 0);
        assert_eq!(error_count("int f(char a[const 8]);"), 0);
    }

    #[test]
    fn a_file_scope_typedef_answers_in_expression_position() {
        // Without a file scope the parser dropped the name, and the cast below
        // read as a parenthesized expression followed by a literal.
        assert_eq!(
            error_count("typedef unsigned long word;\nint x = (word) 0;"),
            0
        );
        assert_eq!(
            error_count("typedef unsigned long word;\nint f(void) { return (int) (word) 0; }"),
            0
        );
    }

    #[test]
    fn a_field_name_does_not_hide_a_type_of_the_same_name() {
        // C11 6.2.3 puts a member in its own namespace.
        let source = "typedef union value { int n; } value;\n\
                      struct node { int value; };\n\
                      int f(void) { value v; return v.n; }";
        assert_eq!(error_count(source), 0);
    }

    #[test]
    fn a_parameter_name_does_not_escape_its_function() {
        let source = "typedef int count;\nvoid f(int count);\ncount total = 1;";
        assert_eq!(error_count(source), 0);
    }

    // -----------------------------------------------------------------------
    // Full C11 statements and expressions, and the extensions beside them.
    // covers: C-4d, C-4e, C-4f, C-4g, C-4h, C-4i, C-4j, C-4k
    // -----------------------------------------------------------------------

    #[test]
    fn a_generic_selection_parses() {
        // C11 6.5.1.1.
        assert_eq!(
            error_count("int f(void) { return _Generic(1, int: 1, double: 2, default: 0); }"),
            0
        );
        assert_eq!(
            error_count("int f(char *s) { return _Generic(s, char *: 1, default: 0); }"),
            0
        );
    }

    #[test]
    fn a_static_assertion_stands_where_a_declaration_stands() {
        // C11 6.7.10.
        assert_eq!(error_count("_Static_assert(1, \"ok\");"), 0);
        assert_eq!(
            error_count("int f(void) { _Static_assert(1, \"ok\"); return 0; }"),
            0
        );
    }

    #[test]
    fn an_old_style_definition_parses() {
        // C11 6.9.1.
        let source = "int add(a, b)\nint a;\nint b;\n{ return a + b; }";
        assert_eq!(error_count(source), 0);
        assert_eq!(error_count("int one() { return 1; }"), 0);
    }

    #[test]
    fn a_type_name_stands_as_a_call_argument() {
        // A macro such as `va_arg` puts a type where an argument goes.
        assert_eq!(
            error_count("int f(void *ap) { return __builtin_va_arg(ap, int); }"),
            0
        );
        assert_eq!(
            error_count(
                "struct s { int a; int b; };\nint f(void) { return __builtin_offsetof(struct s, b); }"
            ),
            0
        );
        // An ordinary call still reads as a call.
        assert_eq!(
            error_count("int g(int, int);\nint f(int a, int b) { return g(a, b); }"),
            0
        );
    }

    #[test]
    fn a_statement_expression_parses() {
        assert_eq!(
            error_count("int f(void) { return ({ int x = 1; x; }); }"),
            0
        );
    }

    #[test]
    fn an_inline_assembly_statement_parses() {
        assert_eq!(
            error_count("void f(void) { __asm__ volatile (\"nop\"); }"),
            0
        );
        assert_eq!(
            error_count("void f(int x) { __asm__ (\"m %0\" : \"=r\"(x) : : \"memory\"); }"),
            0
        );
    }

    #[test]
    fn typeof_names_a_type() {
        assert_eq!(
            error_count("int f(void) { int a = 1; __typeof__(a) b = a; return b; }"),
            0
        );
        assert_eq!(
            error_count("int f(void) { int a = 1; return (__typeof__(a)) a; }"),
            0
        );
        assert_eq!(
            error_count("int f(void) { __auto_type b = 1; return b; }"),
            0
        );
    }

    #[test]
    fn a_label_is_a_value() {
        assert_eq!(
            error_count("void f(void) { void *p = &&here; goto *p; here: ; }"),
            0
        );
        // A plain logical and still reads as an operator.
        assert_eq!(error_count("int f(int a, int b) { return a && b; }"), 0);
    }

    #[test]
    fn a_range_designator_parses() {
        assert_eq!(error_count("int a[8] = { [0 ... 3] = 1, [4] = 2 };"), 0);
        assert_eq!(error_count("int a[8] = { [2] = 3 };"), 0);
    }

    #[test]
    fn a_linkage_block_parses() {
        // A header writes this behind `#ifdef __cplusplus`, and Lark evaluates
        // no directive, so the parser reads the block.
        let source = "extern \"C\" {\nint f(void);\nint g(int);\n}";
        assert_eq!(error_count(source), 0);
        assert_eq!(error_count("extern \"C\" int f(void);"), 0);
    }

    #[test]
    fn an_unexpanded_macro_stands_where_an_attribute_stands() {
        // Rule C-4k. A module is not preprocessed, so the macro is still here.
        assert_eq!(
            error_count("static int PRINTF(2) print_message(int a, const char *f);"),
            0
        );
        // A declarator that opens a parameter list is not a macro.
        assert_eq!(
            error_count("static int print_message(int a, const char *f);"),
            0
        );
        // A trailing attribute belongs to the declarator, not to a macro.
        assert_eq!(
            error_count(
                "int printf(const char *, ...) __attribute__((__format__(__printf__, 1, 2)));"
            ),
            0
        );
    }
}
