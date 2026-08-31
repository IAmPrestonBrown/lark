//! Expression types, and the inference that `auto` uses.
//!
//! The inference is best effort. Delivery phase A does not read headers, so a
//! name that the front end cannot find yields the error type rather than a
//! diagnostic.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};

use crate::lower::Lowering;
use crate::ty::{FloatWidth, IntWidth, TypeId, TypeKind, TypeStore};

/// Computes the type of an expression.
pub struct Infer<'a> {
    /// The lowering that builds a type from a declaration.
    pub lowering: Lowering<'a>,
}

impl Infer<'_> {
    /// Returns the type that `auto` infers from an initializer.
    ///
    /// Rule T-10 applies the decay that C performs, and keeps `gc`.
    pub fn inferred(&mut self, initializer: &SyntaxNode) -> TypeId {
        let built = self.expr(initializer);
        self.lowering.store.decay(built)
    }

    /// Returns the type of an expression.
    pub fn expr(&mut self, node: &SyntaxNode) -> TypeId {
        match node.kind() {
            LITERAL_EXPR => self.literal(node),
            CAST_EXPR | COMPOUND_LITERAL_EXPR => self.cast(node),
            NEW_EXPR | NEW_ARRAY_EXPR => self.new_expr(node),
            PREFIX_EXPR => self.prefix(node),
            BIN_EXPR => self.binary(node),
            // A parenthesis, a postfix operator, and an assignment all take the
            // type of the expression they wrap.
            PAREN_EXPR | POSTFIX_EXPR | ASSIGN_EXPR => self.first_child_expr(node),
            COND_EXPR => self.conditional(node),
            SIZEOF_EXPR | ALIGNOF_EXPR => self.lowering.common.size,
            INDEX_EXPR => self.index(node),
            _ => self.lowering.common.error,
        }
    }

    /// Returns the type of a literal token.
    fn literal(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(token) = node.first_token() else {
            return self.lowering.common.error;
        };
        match token.kind() {
            INT_NUMBER => {
                let text = token.text();
                let unsigned = text.contains(['u', 'U']);
                let long = text.matches(['l', 'L']).count();
                let width = match long {
                    0 => IntWidth::Int,
                    1 => IntWidth::Long,
                    _ => IntWidth::LongLong,
                };
                self.lowering.store.int(width, !unsigned)
            }
            FLOAT_NUMBER => {
                let width = if token.text().contains(['f', 'F']) {
                    FloatWidth::Float
                } else {
                    FloatWidth::Double
                };
                self.lowering.store.intern(TypeKind::Float(width))
            }
            // A character constant has type `int` in C.
            CHAR_LITERAL => self.lowering.common.int,
            STRING_LITERAL => {
                let char_type = self.lowering.common.char;
                self.lowering.store.array(char_type, None)
            }
            _ => self.lowering.common.error,
        }
    }

    /// Returns the type that a cast or a compound literal names.
    fn cast(&mut self, node: &SyntaxNode) -> TypeId {
        match node.children().find(|child| child.kind() == TYPE_NAME) {
            Some(type_name) => self.lowering.type_name(&type_name),
            None => self.lowering.common.error,
        }
    }

    /// Returns the type of `new T { ... }`, which is a managed pointer to `T`.
    ///
    /// Rules O-4 and O-6 both yield `gc T*`.
    fn new_expr(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(type_name) = node.children().find(|child| child.kind() == TYPE_NAME) else {
            return self.lowering.common.error;
        };
        let target = self.lowering.type_name(&type_name);
        self.lowering.store.pointer(target, true)
    }

    /// Returns the type of a prefix expression.
    fn prefix(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(operator) = child_tokens(node).find(|token| !token.kind().is_trivia()) else {
            return self.lowering.common.error;
        };
        let operand = self.first_child_expr(node);
        match operator.kind() {
            AMP => self.lowering.store.pointer(operand, false),
            STAR => match self.lowering.store.kind(operand).clone() {
                TypeKind::Pointer { target, .. }
                | TypeKind::Array {
                    element: target, ..
                } => target,
                _ => self.lowering.common.error,
            },
            // A comparison and a logical negation both give `int` in C.
            BANG => self.lowering.common.int,
            _ => operand,
        }
    }

    /// Returns the type of a binary expression.
    fn binary(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(operator) = child_tokens(node)
            .find(|token| !token.kind().is_trivia() && is_binary_operator(token.kind()))
        else {
            return self.lowering.common.error;
        };
        if is_comparison(operator.kind()) {
            return self.lowering.common.int;
        }

        let operands: Vec<SyntaxNode> = node.children().filter(is_expression).collect();
        let [left, right] = operands.as_slice() else {
            return self.lowering.common.error;
        };
        let left = self.expr(left);
        let right = self.expr(right);
        self.usual_conversions(left, right)
    }

    /// Returns the type that the usual arithmetic conversions produce.
    ///
    /// Pointer arithmetic keeps the pointer.
    fn usual_conversions(&mut self, left: TypeId, right: TypeId) -> TypeId {
        let left = self.lowering.store.decay(left);
        let right = self.lowering.store.decay(right);
        if self.lowering.store.is_pointer(left) {
            return left;
        }
        if self.lowering.store.is_pointer(right) {
            return right;
        }
        rank_winner(self.lowering.store, left, right)
    }

    /// Returns the type of a conditional expression.
    fn conditional(&mut self, node: &SyntaxNode) -> TypeId {
        let branches: Vec<SyntaxNode> = node.children().filter(is_expression).collect();
        match branches.as_slice() {
            [_, then, other] => {
                let then = self.expr(then);
                let other = self.expr(other);
                if then == other {
                    then
                } else {
                    self.usual_conversions(then, other)
                }
            }
            _ => self.lowering.common.error,
        }
    }

    /// Returns the element type of an index expression.
    fn index(&mut self, node: &SyntaxNode) -> TypeId {
        let Some(base) = node.children().find(is_expression) else {
            return self.lowering.common.error;
        };
        let base = self.expr(&base);
        match self.lowering.store.kind(base).clone() {
            TypeKind::Pointer { target, .. }
            | TypeKind::Array {
                element: target, ..
            } => target,
            _ => self.lowering.common.error,
        }
    }

    /// Returns the type of the first child that is an expression.
    fn first_child_expr(&mut self, node: &SyntaxNode) -> TypeId {
        match node.children().find(is_expression) {
            Some(child) => self.expr(&child),
            None => self.lowering.common.error,
        }
    }
}

/// Reports whether a node kind is an expression.
fn is_expression(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        LITERAL_EXPR
            | NAME_EXPR
            | PAREN_EXPR
            | CALL_EXPR
            | INDEX_EXPR
            | FIELD_EXPR
            | METHOD_EXPR
            | POSTFIX_EXPR
            | PREFIX_EXPR
            | CAST_EXPR
            | BIN_EXPR
            | COND_EXPR
            | ASSIGN_EXPR
            | SIZEOF_EXPR
            | ALIGNOF_EXPR
            | NEW_EXPR
            | NEW_ARRAY_EXPR
            | COMPOUND_LITERAL_EXPR
    )
}

/// Reports whether a token kind is a binary operator.
fn is_binary_operator(kind: lark_syntax::SyntaxKind) -> bool {
    matches!(
        kind,
        PLUS | MINUS
            | STAR
            | SLASH
            | PERCENT
            | SHL
            | SHR
            | AMP
            | PIPE
            | CARET
            | AMP2
            | PIPE2
            | L_ANGLE
            | R_ANGLE
            | LT_EQ
            | GT_EQ
            | EQ2
            | BANG_EQ
            | COMMA
    )
}

/// Reports whether an operator yields `int` whatever its operands are.
fn is_comparison(kind: lark_syntax::SyntaxKind) -> bool {
    matches!(
        kind,
        L_ANGLE | R_ANGLE | LT_EQ | GT_EQ | EQ2 | BANG_EQ | AMP2 | PIPE2
    )
}

/// Returns the wider of two arithmetic types.
fn rank_winner(store: &TypeStore, left: TypeId, right: TypeId) -> TypeId {
    match (store.kind(left), store.kind(right)) {
        (TypeKind::Float(a), TypeKind::Float(b)) => {
            if a >= b {
                left
            } else {
                right
            }
        }
        (TypeKind::Float(_), _) => left,
        (_, TypeKind::Float(_)) => right,
        (
            TypeKind::Int {
                width: a,
                signed: sa,
            },
            TypeKind::Int {
                width: b,
                signed: sb,
            },
        ) => {
            if a.rank() > b.rank() || (a.rank() == b.rank() && !sa && *sb) {
                left
            } else {
                right
            }
        }
        _ if store.is_error(left) => right,
        _ => left,
    }
}
