//! Finds the `#include` directives in a Lark source file.
//!
//! The lexer marks a directive line as trivia, so the parser ignores it and
//! the emitter copies it through. Rule C-3 keeps the line in the generated C.
//! This module reads the same lines to learn which headers to preprocess.

use lark_syntax::{SyntaxKind, tokenize};

/// One `#include` directive, with its place in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Include {
    /// The whole directive line, trimmed.
    pub text: String,
    /// The header name, without the brackets or the quotes.
    pub header: String,
    /// True when the directive used angle brackets.
    pub system: bool,
    /// The byte offset of the directive in the source.
    pub offset: u32,
}

/// Returns every `#include` directive in a source file, in order.
#[must_use]
pub fn includes_of(source: &str) -> Vec<Include> {
    let mut found = Vec::new();
    for token in tokenize(source).tokens {
        if token.kind != SyntaxKind::PP_DIRECTIVE {
            continue;
        }
        let text = &source[token.span.start as usize..token.span.end as usize];
        let Some(include) = parse_line(text, token.span.start) else {
            continue;
        };
        found.push(include);
    }
    found
}

/// Reads one directive line, and returns it when the line is an `#include`.
fn parse_line(line: &str, offset: u32) -> Option<Include> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("include")?;
    // `#includes` is not a directive, so the next character must be a space or
    // the start of the header name.
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim();
    let (header, system) = if let Some(inner) = rest.strip_prefix('<') {
        (inner.split_once('>')?.0, true)
    } else if let Some(inner) = rest.strip_prefix('"') {
        (inner.split_once('"')?.0, false)
    } else {
        // A computed include expands a macro. Lark cannot read it here, so the
        // caller keeps the table incomplete. See rule L-15.
        return None;
    };
    Some(Include {
        text: trimmed.to_owned(),
        header: header.to_owned(),
        system,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_angle_include_reads_as_a_system_header() {
        let found = includes_of("#include <stdio.h>\nint main(void) { return 0; }\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].header, "stdio.h");
        assert!(found[0].system);
    }

    #[test]
    fn a_quoted_include_reads_as_a_local_header() {
        let found = includes_of("#include \"local.h\"\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].header, "local.h");
        assert!(!found[0].system);
    }

    #[test]
    fn another_directive_is_not_an_include() {
        assert!(includes_of("#define X 1\n#pragma once\n#ifdef A\n#endif\n").is_empty());
    }

    #[test]
    fn a_computed_include_is_skipped() {
        assert!(includes_of("#include HEADER\n").is_empty());
    }

    #[test]
    fn a_hash_inside_code_is_not_a_directive() {
        assert!(includes_of("int x = a # include;\n").is_empty());
    }
}
