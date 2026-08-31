//! Builds a type from a declaration.
//!
//! A C declaration has two halves. The specifiers give a base type, and the
//! declarator wraps it in pointers, arrays, and parameter lists. Rule T-1a
//! places the `gc` qualifier on the pointer levels.

// This module walks 31 of the kinds, and a list that long in the header helps
// no reader, so it imports the variants. A module that uses a few names spells
// them out instead.
#![allow(clippy::enum_glob_use)]

use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};

use crate::ty::{Common, FloatWidth, IntWidth, NamedKind, TypeId, TypeKind, TypeStore};

/// What the declaration specifiers say.
#[derive(Clone, Copy, Debug)]
pub struct Specifiers {
    /// The base type that the specifiers name.
    pub base: TypeId,
    /// How many `gc` markers the specifiers carry. See rule T-1a.
    pub gc_count: usize,
    /// Whether the declaration introduces a type name.
    pub is_typedef: bool,
    /// Whether the declaration asks for type inference. See rule L-5.
    pub is_inference: bool,
}

/// Builds types from a tree.
pub struct Lowering<'a> {
    /// The store that holds every type.
    pub store: &'a mut TypeStore,
    /// The types that every run needs.
    pub common: Common,
}

impl Lowering<'_> {
    /// Reads the declaration specifiers of a declaration.
    pub fn specifiers(&mut self, node: &SyntaxNode) -> Specifiers {
        let mut keywords = Keywords::default();
        let mut gc_count = 0;
        let mut is_typedef = false;
        let mut saw_auto = false;

        for token in child_tokens(node) {
            match token.kind() {
                VOID_KW => keywords.void = true,
                BOOL_KW => keywords.bool = true,
                CHAR_KW => keywords.char = true,
                SHORT_KW => keywords.short = true,
                INT_KW => keywords.int = true,
                LONG_KW => keywords.long += 1,
                FLOAT_KW => keywords.float = true,
                DOUBLE_KW => keywords.double = true,
                SIGNED_KW => keywords.signed = true,
                UNSIGNED_KW => keywords.unsigned = true,
                TYPEDEF_KW => is_typedef = true,
                AUTO_KW => saw_auto = true,
                IDENT if token.text() == "gc" => gc_count += 1,
                _ => {}
            }
        }

        let named = self.named_base(node);
        let base = match named {
            Some(id) => id,
            None if keywords.is_empty() => self.common.error,
            None => self.keyword_base(keywords),
        };

        // Rule L-5. `auto` with no type specifier asks for inference.
        let is_inference = saw_auto && named.is_none() && keywords.is_empty();

        Specifiers {
            base,
            gc_count,
            is_typedef,
            is_inference,
        }
    }

    /// Returns the base type when the specifiers name a record or a type name.
    fn named_base(&mut self, node: &SyntaxNode) -> Option<TypeId> {
        let mut args = Vec::new();
        for child in node.children() {
            if child.kind() == GENERIC_ARGS {
                args = self.generic_args(&child);
            }
        }

        for child in node.children() {
            match child.kind() {
                STRUCT_DEF | UNION_DEF | ENUM_DEF => {
                    let kind = match child.kind() {
                        UNION_DEF => NamedKind::Union,
                        ENUM_DEF => NamedKind::Enum,
                        _ => NamedKind::Struct,
                    };
                    let name = child
                        .children()
                        .find(|item| item.kind() == NAME)
                        .and_then(|item| item.first_token())
                        .map_or_else(|| "<anonymous>".to_owned(), |token| token.text().to_owned());
                    let args = child
                        .children()
                        .find(|item| item.kind() == GENERIC_PARAMS)
                        .map(|params| self.generic_params(&params))
                        .unwrap_or_default();
                    return Some(self.store.intern(TypeKind::Named { name, kind, args }));
                }
                NAME_REF | PATH => {
                    let name = child
                        .first_token()
                        .map_or_else(String::new, |token| token.text().to_owned());
                    if name.is_empty() {
                        continue;
                    }
                    let full = if child.kind() == PATH {
                        let tail = child_tokens(&child)
                            .filter(|token| token.kind() == IDENT)
                            .nth(1)
                            .map_or_else(String::new, |token| token.text().to_owned());
                        format!("{name}::{tail}")
                    } else {
                        name
                    };
                    return Some(self.store.intern(TypeKind::Named {
                        name: full,
                        kind: NamedKind::Unknown,
                        args: args.clone(),
                    }));
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the types inside a generic argument list.
    fn generic_args(&mut self, node: &SyntaxNode) -> Vec<TypeId> {
        node.children()
            .filter(|child| child.kind() == TYPE_NAME)
            .map(|child| self.type_name(&child))
            .collect()
    }

    /// Returns one parameter per name in a generic parameter list.
    fn generic_params(&mut self, node: &SyntaxNode) -> Vec<TypeId> {
        node.children()
            .filter(|child| child.kind() == NAME)
            .filter_map(|child| child.first_token())
            .map(|token| self.store.intern(TypeKind::Param(token.text().to_owned())))
            .collect()
    }

    /// Builds the type that a `TYPE_NAME` node names.
    pub fn type_name(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(specifiers) = node
            .children()
            .find(|child| child.kind() == DECL_SPECIFIERS)
        else {
            return self.common.error;
        };
        let info = self.specifiers(&specifiers);
        match node.children().find(|child| child.kind() == DECLARATOR) {
            Some(declarator) => self.declarator(&info, &declarator).0,
            None => self.apply_gc(info.base, info.gc_count).0,
        }
    }

    /// Builds the type that a declarator names.
    ///
    /// The second value reports whether every `gc` marker found a pointer. See
    /// rule T-2.
    pub fn declarator(&mut self, info: &Specifiers, node: &SyntaxNode) -> (TypeId, bool) {
        let built = self.walk_declarator(info.base, node);
        self.apply_gc(built, info.gc_count)
    }

    /// Applies the pointers and the suffixes of one declarator level.
    fn walk_declarator(&mut self, base: TypeId, node: &SyntaxNode) -> TypeId {
        let mut current = base;

        // A pointer binds looser than a suffix, so the pointers apply first.
        for pointer in node.children().filter(|child| child.kind() == POINTER) {
            let managed =
                child_tokens(&pointer).any(|token| token.kind() == IDENT && token.text() == "gc");
            current = self.store.pointer(current, managed);
        }

        // `x[3][4]` is an array of three arrays of four, so the suffixes apply
        // from the right.
        let suffixes: Vec<SyntaxNode> = node
            .children()
            .filter(|child| matches!(child.kind(), ARRAY_SUFFIX | PARAM_LIST))
            .collect();
        for suffix in suffixes.iter().rev() {
            current = if suffix.kind() == ARRAY_SUFFIX {
                let length = array_length(suffix);
                self.store.array(current, length)
            } else {
                let (params, variadic) = self.param_types(suffix);
                self.store.intern(TypeKind::Function {
                    result: current,
                    params,
                    variadic,
                })
            };
        }

        // A nested declarator wraps the result, as in `int (*f)(void)`.
        match node.children().find(|child| child.kind() == DECLARATOR) {
            Some(nested) => self.walk_declarator(current, &nested),
            None => current,
        }
    }

    /// Returns the parameter types of a parameter list.
    fn param_types(&mut self, node: &SyntaxNode) -> (Vec<TypeId>, bool) {
        let mut params = Vec::new();
        let mut variadic = false;
        for param in node.children().filter(|child| child.kind() == PARAM) {
            if child_tokens(&param).any(|token| token.kind() == ELLIPSIS) {
                variadic = true;
                continue;
            }
            let Some(specifiers) = param
                .children()
                .find(|child| child.kind() == DECL_SPECIFIERS)
            else {
                continue;
            };
            let info = self.specifiers(&specifiers);
            let built = match param.children().find(|child| child.kind() == DECLARATOR) {
                Some(declarator) => self.declarator(&info, &declarator).0,
                None => self.apply_gc(info.base, info.gc_count).0,
            };
            // A single `void` parameter means the function takes none.
            if params.is_empty() && matches!(self.store.kind(built), TypeKind::Void) {
                continue;
            }
            let decayed = self.store.decay(built);
            params.push(decayed);
        }
        (params, variadic)
    }

    /// Marks the outermost pointer levels as managed. See rule T-1a.
    ///
    /// The second value is false when a `gc` marker found no pointer, which
    /// rule T-2 forbids.
    fn apply_gc(&mut self, id: TypeId, count: usize) -> (TypeId, bool) {
        if count == 0 {
            return (id, true);
        }
        // Rule T-1a. A function declarator builds a function, and the
        // specifier qualifies what the function returns, the same way `const`
        // does in C. Without this, no function can return a managed pointer.
        if let TypeKind::Function {
            result,
            params,
            variadic,
        } = self.store.kind(id).clone()
        {
            let (result, ok) = self.apply_gc(result, count);
            return (
                self.store.intern(TypeKind::Function {
                    result,
                    params,
                    variadic,
                }),
                ok,
            );
        }
        let TypeKind::Pointer { target, .. } = self.store.kind(id).clone() else {
            return (id, false);
        };
        let (inner, ok) = self.apply_gc(target, count - 1);
        (self.store.pointer(inner, true), ok)
    }

    /// Builds the base type that a run of C keywords names.
    fn keyword_base(&mut self, keywords: Keywords) -> TypeId {
        if keywords.void {
            return self.common.void;
        }
        if keywords.bool {
            return self.store.int(IntWidth::Bool, false);
        }
        if keywords.double {
            let width = if keywords.long > 0 {
                FloatWidth::LongDouble
            } else {
                FloatWidth::Double
            };
            return self.store.intern(TypeKind::Float(width));
        }
        if keywords.float {
            return self.store.intern(TypeKind::Float(FloatWidth::Float));
        }

        let width = if keywords.char {
            IntWidth::Char
        } else if keywords.short {
            IntWidth::Short
        } else if keywords.long >= 2 {
            IntWidth::LongLong
        } else if keywords.long == 1 {
            IntWidth::Long
        } else {
            IntWidth::Int
        };
        // Plain `char` has an implementation defined sign. Lark reads it as
        // signed, which matches every supported target.
        let signed = !keywords.unsigned;
        self.store.int(width, signed)
    }
}

/// The C type keywords that one specifier list carries.
///
/// C combines its type keywords rather than choosing one, so a flag per keyword
/// is the shape the language has.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default)]
struct Keywords {
    void: bool,
    bool: bool,
    char: bool,
    short: bool,
    int: bool,
    long: u8,
    float: bool,
    double: bool,
    signed: bool,
    unsigned: bool,
}

impl Keywords {
    /// Reports whether the list names no type at all.
    fn is_empty(self) -> bool {
        !self.void
            && !self.bool
            && !self.char
            && !self.short
            && !self.int
            && self.long == 0
            && !self.float
            && !self.double
            && !self.signed
            && !self.unsigned
    }
}

/// Returns the element count that an array suffix gives.
fn array_length(node: &SyntaxNode) -> Option<u64> {
    let literal = node
        .descendants()
        .find(|child| child.kind() == LITERAL_EXPR)?;
    let token = literal.first_token()?;
    if token.kind() != INT_NUMBER {
        return None;
    }
    let text = token.text().trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    text.parse().ok()
}
