//! Every kind of token and node in a Lark syntax tree.
//!
//! One enum covers both, because [`rowan`] tags a token and a node with the
//! same type. Token kinds come first, then node kinds.
//!
//! A Lark keyword is not in this list. Rule L-3 makes every Lark keyword
//! contextual, so the lexer emits [`SyntaxKind::IDENT`] for `gc`, `managed`,
//! `iface`, `impl`, `new`, `export`, `init`, `gc_leaf`, and `gc_safe`. The
//! parser recognizes them by position. A C11 keyword is reserved in C, so the
//! lexer does map it.

macro_rules! syntax_kinds {
    (
        tokens: [ $($token:ident,)* ]
        nodes: [ $($node:ident,)* ]
    ) => {
        /// A tag for one token or one node.
        ///
        /// The variant names describe themselves, so they carry no separate
        /// documentation. They use the shape that a tree snapshot prints, so
        /// they are not camel case.
        #[allow(missing_docs, non_camel_case_types)]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $($token,)*
            $($node,)*
        }

        impl SyntaxKind {
            /// Every kind, in declaration order.
            pub const ALL: &'static [SyntaxKind] = &[
                $(SyntaxKind::$token,)*
                $(SyntaxKind::$node,)*
            ];

            /// Every kind that the lexer can produce.
            pub const TOKENS: &'static [SyntaxKind] = &[$(SyntaxKind::$token,)*];

            /// Returns the name of the kind, for a tree snapshot.
            pub const fn name(self) -> &'static str {
                match self {
                    $(SyntaxKind::$token => stringify!($token),)*
                    $(SyntaxKind::$node => stringify!($node),)*
                }
            }

            /// Reports whether the kind tags a token rather than a node.
            pub fn is_token(self) -> bool {
                Self::TOKENS.contains(&self)
            }
        }
    };
}

syntax_kinds! {
    tokens: [
        // Control and trivia.
        TOMBSTONE,
        EOF,
        WHITESPACE,
        LINE_COMMENT,
        BLOCK_COMMENT,
        PP_DIRECTIVE,
        ERROR_TOKEN,

        // Literals and names.
        INT_NUMBER,
        FLOAT_NUMBER,
        CHAR_LITERAL,
        STRING_LITERAL,
        IDENT,

        // Punctuation.
        L_BRACK,
        R_BRACK,
        L_PAREN,
        R_PAREN,
        L_CURLY,
        R_CURLY,
        DOT,
        ARROW,
        PLUS2,
        MINUS2,
        AMP,
        STAR,
        PLUS,
        MINUS,
        TILDE,
        BANG,
        SLASH,
        PERCENT,
        SHL,
        SHR,
        L_ANGLE,
        R_ANGLE,
        LT_EQ,
        GT_EQ,
        EQ2,
        BANG_EQ,
        CARET,
        PIPE,
        AMP2,
        PIPE2,
        QUESTION,
        COLON,
        SEMICOLON,
        ELLIPSIS,
        EQ,
        STAR_EQ,
        SLASH_EQ,
        PERCENT_EQ,
        PLUS_EQ,
        MINUS_EQ,
        SHL_EQ,
        SHR_EQ,
        AMP_EQ,
        CARET_EQ,
        PIPE_EQ,
        COMMA,
        HASH,
        HASH2,
        COLON2,
        AT,

        // C11 keywords.
        AUTO_KW,
        BREAK_KW,
        CASE_KW,
        CHAR_KW,
        CONST_KW,
        CONTINUE_KW,
        DEFAULT_KW,
        DO_KW,
        DOUBLE_KW,
        ELSE_KW,
        ENUM_KW,
        EXTERN_KW,
        FLOAT_KW,
        FOR_KW,
        GOTO_KW,
        IF_KW,
        INLINE_KW,
        INT_KW,
        LONG_KW,
        REGISTER_KW,
        RESTRICT_KW,
        RETURN_KW,
        SHORT_KW,
        SIGNED_KW,
        SIZEOF_KW,
        STATIC_KW,
        STRUCT_KW,
        SWITCH_KW,
        TYPEDEF_KW,
        UNION_KW,
        UNSIGNED_KW,
        VOID_KW,
        VOLATILE_KW,
        WHILE_KW,
        ALIGNAS_KW,
        ALIGNOF_KW,
        ATOMIC_KW,
        BOOL_KW,
        COMPLEX_KW,
        GENERIC_KW,
        IMAGINARY_KW,
        NORETURN_KW,
        STATIC_ASSERT_KW,
        THREAD_LOCAL_KW,
    ]
    nodes: [
        // The root.
        SOURCE_FILE,

        // Lark items.
        IMPORT_DIRECTIVE,
        GLOBAL_BLOCK,
        GLOBAL_ATTACH,
        IFACE_DEF,
        IFACE_METHOD,
        IMPL_DEF,

        // C items.
        FN_DEF,
        DECLARATION,
        DECL_SPECIFIERS,
        STRUCT_DEF,
        UNION_DEF,
        ENUM_DEF,
        STRUCT_BODY,
        FIELD_DECL,
        ENUM_BODY,
        ENUMERATOR,
        STATIC_ASSERT_DECL,
        LINKAGE_BLOCK,

        // Declarators.
        INIT_DECLARATOR,
        DECLARATOR,
        POINTER,
        PARAM_LIST,
        PARAM,
        ARRAY_SUFFIX,
        FN_SUFFIX,
        TYPE_NAME,
        GENERIC_PARAMS,
        GENERIC_ARGS,
        NAME,
        NAME_REF,
        PATH,
        KR_PARAM_LIST,

        // Statements.
        BLOCK_STMT,
        EXPR_STMT,
        DECL_STMT,
        IF_STMT,
        WHILE_STMT,
        DO_STMT,
        FOR_STMT,
        SWITCH_STMT,
        CASE_STMT,
        DEFAULT_STMT,
        LABELED_STMT,
        GOTO_STMT,
        BREAK_STMT,
        CONTINUE_STMT,
        RETURN_STMT,
        EMPTY_STMT,
        INIT_STMT,
        ASM_STMT,

        // Expressions.
        LITERAL_EXPR,
        NAME_EXPR,
        PAREN_EXPR,
        STMT_EXPR,
        GENERIC_SELECTION,
        GENERIC_ASSOC,
        CALL_EXPR,
        INDEX_EXPR,
        FIELD_EXPR,
        METHOD_EXPR,
        POSTFIX_EXPR,
        PREFIX_EXPR,
        CAST_EXPR,
        BIN_EXPR,
        COND_EXPR,
        ASSIGN_EXPR,
        SIZEOF_EXPR,
        ALIGNOF_EXPR,
        NEW_EXPR,
        NEW_ARRAY_EXPR,
        COMPOUND_LITERAL_EXPR,
        INIT_LIST,
        DESIGNATED_INIT,
        DESIGNATOR,
        ARG_LIST,

        // Recovery and compiler extensions.
        EXTENSION,
        ERROR,
    ]
}

impl SyntaxKind {
    /// Reports whether the kind is whitespace, a comment, or a preprocessor line.
    ///
    /// The parser skips trivia. The tree keeps it, so invariant R holds.
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::LINE_COMMENT | Self::BLOCK_COMMENT | Self::PP_DIRECTIVE
        )
    }

    /// Returns the kind for a C11 keyword, or `None` for any other word.
    ///
    /// A Lark keyword is contextual, so this function never returns one. See
    /// rule L-3.
    ///
    /// A spelling with two leading underscores maps to the same keyword, as
    /// `__restrict` maps to `restrict`. C reserves such a name to the
    /// implementation, and a header uses it so that a program that redefines
    /// the plain word still compiles. See rule C-4b.
    pub fn c_keyword(text: &str) -> Option<Self> {
        let kind = match text {
            "auto" => Self::AUTO_KW,
            "break" => Self::BREAK_KW,
            "case" => Self::CASE_KW,
            "char" => Self::CHAR_KW,
            "const" | "__const" => Self::CONST_KW,
            "continue" => Self::CONTINUE_KW,
            "default" => Self::DEFAULT_KW,
            "do" => Self::DO_KW,
            "double" => Self::DOUBLE_KW,
            "else" => Self::ELSE_KW,
            "enum" => Self::ENUM_KW,
            "extern" => Self::EXTERN_KW,
            "float" => Self::FLOAT_KW,
            "for" => Self::FOR_KW,
            "goto" => Self::GOTO_KW,
            "if" => Self::IF_KW,
            "inline" | "__inline" | "__inline__" => Self::INLINE_KW,
            "int" => Self::INT_KW,
            "long" => Self::LONG_KW,
            "register" => Self::REGISTER_KW,
            "restrict" | "__restrict" | "__restrict__" => Self::RESTRICT_KW,
            "return" => Self::RETURN_KW,
            "short" => Self::SHORT_KW,
            "signed" | "__signed" | "__signed__" => Self::SIGNED_KW,
            "sizeof" => Self::SIZEOF_KW,
            "static" => Self::STATIC_KW,
            "struct" => Self::STRUCT_KW,
            "switch" => Self::SWITCH_KW,
            "typedef" => Self::TYPEDEF_KW,
            "union" => Self::UNION_KW,
            "unsigned" => Self::UNSIGNED_KW,
            "void" => Self::VOID_KW,
            "volatile" | "__volatile" | "__volatile__" => Self::VOLATILE_KW,
            "while" => Self::WHILE_KW,
            "_Alignas" => Self::ALIGNAS_KW,
            "_Alignof" => Self::ALIGNOF_KW,
            "_Atomic" => Self::ATOMIC_KW,
            "_Bool" => Self::BOOL_KW,
            "_Complex" => Self::COMPLEX_KW,
            "_Generic" => Self::GENERIC_KW,
            "_Imaginary" => Self::IMAGINARY_KW,
            "_Noreturn" => Self::NORETURN_KW,
            "_Static_assert" => Self::STATIC_ASSERT_KW,
            "_Thread_local" => Self::THREAD_LOCAL_KW,
            _ => return None,
        };
        Some(kind)
    }

    /// Returns the kind for a raw tag from the tree library.
    pub fn from_raw(raw: u16) -> Option<Self> {
        Self::ALL.get(raw as usize).copied()
    }

    /// Returns the raw tag for the tree library.
    pub fn to_raw(self) -> u16 {
        u16::try_from(self as usize).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind;

    #[test]
    fn every_kind_round_trips_through_its_raw_tag() {
        for kind in SyntaxKind::ALL {
            assert_eq!(
                SyntaxKind::from_raw(kind.to_raw()),
                Some(*kind),
                "{}",
                kind.name()
            );
        }
    }

    #[test]
    fn a_raw_tag_past_the_end_has_no_kind() {
        let past = u16::try_from(SyntaxKind::ALL.len()).unwrap_or(u16::MAX);
        assert_eq!(SyntaxKind::from_raw(past), None);
    }

    #[test]
    fn the_token_kinds_come_before_the_node_kinds() {
        let last_token = SyntaxKind::TOKENS.len() - 1;
        assert_eq!(SyntaxKind::ALL[last_token], SyntaxKind::THREAD_LOCAL_KW);
        assert_eq!(SyntaxKind::ALL[last_token + 1], SyntaxKind::SOURCE_FILE);
    }

    /// covers: L-3, L-4
    #[test]
    fn a_lark_keyword_is_not_a_lexer_keyword() {
        for word in [
            "gc", "managed", "iface", "impl", "new", "export", "init", "gc_leaf",
        ] {
            assert_eq!(
                SyntaxKind::c_keyword(word),
                None,
                "{word} must stay an identifier"
            );
        }
    }

    #[test]
    fn every_c11_keyword_maps_to_a_kind() {
        assert_eq!(SyntaxKind::c_keyword("auto"), Some(SyntaxKind::AUTO_KW));
        assert_eq!(
            SyntaxKind::c_keyword("_Static_assert"),
            Some(SyntaxKind::STATIC_ASSERT_KW)
        );
        assert_eq!(SyntaxKind::c_keyword("Auto"), None);
    }

    #[test]
    fn trivia_is_whitespace_a_comment_or_a_preprocessor_line() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::LINE_COMMENT.is_trivia());
        assert!(SyntaxKind::BLOCK_COMMENT.is_trivia());
        assert!(SyntaxKind::PP_DIRECTIVE.is_trivia());
        assert!(!SyntaxKind::IDENT.is_trivia());
    }
}
