//! The lossless syntax tree.
//!
//! The tree keeps every token, so the text of the root equals the source. That
//! is invariant R at the tree level. A formatter, a rename, and an accurate
//! hover all depend on it.

use std::fmt::Write as _;

use crate::kind::SyntaxKind;

/// The language tag for the tree library.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Lark;

impl rowan::Language for Lark {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from_raw(raw.0).unwrap_or(SyntaxKind::ERROR)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.to_raw())
    }
}

/// A node in a Lark syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<Lark>;
/// A token in a Lark syntax tree.
pub type SyntaxToken = rowan::SyntaxToken<Lark>;
/// A node or a token in a Lark syntax tree.
pub type SyntaxElement = rowan::SyntaxElement<Lark>;

/// Returns every direct token child of a node, trivia included.
///
/// The helper keeps the tree library out of the crates that read a tree.
pub fn child_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
}

/// Returns every token in a node and its descendants, trivia included.
pub fn all_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    node.descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
}

/// Prints a tree for a snapshot test.
///
/// The print shows every token, trivia included, so a reader sees that the tree
/// holds the whole file.
///
/// ```text
/// SOURCE_FILE@0..12
///   DECLARATION@0..12
///     DECL_SPECIFIERS@0..3
///       INT_KW@0..3 "int"
/// ```
#[must_use]
pub fn print(node: &SyntaxNode) -> String {
    let mut out = String::new();
    print_into(&mut out, &node.clone().into(), 0);
    out
}

fn print_into(out: &mut String, element: &SyntaxElement, depth: usize) {
    let indent = "  ".repeat(depth);
    match element {
        rowan::NodeOrToken::Node(node) => {
            let range = node.text_range();
            let _ = writeln!(
                out,
                "{indent}{}@{}..{}",
                node.kind().name(),
                u32::from(range.start()),
                u32::from(range.end())
            );
            for child in node.children_with_tokens() {
                print_into(out, &child, depth + 1);
            }
        }
        rowan::NodeOrToken::Token(token) => {
            let range = token.text_range();
            let _ = writeln!(
                out,
                "{indent}{}@{}..{} {:?}",
                token.kind().name(),
                u32::from(range.start()),
                u32::from(range.end()),
                token.text()
            );
        }
    }
}
