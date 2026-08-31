//! Rule DQ-2. An error from a generic body names the instantiation.
//!
//! A generic has no C form of its own. Rule G-1 emits one copy per
//! instantiation, so an error inside the body belongs to every use. Rule G-4
//! makes that the whole of version 1 constraint reporting: there is no
//! constraint syntax, so a type error appears after substitution and the
//! diagnostic must say which use caused it.
//!
//! The pass runs after the type checks and before the report. It adds a label
//! rather than a diagnostic, so one problem still produces one report. That is
//! rule DQ-4.

use lark_diag::Diagnostics;
use lark_mono::Program;
use lark_span::Span;

/// The number of instantiations that one diagnostic names.
///
/// A generic with a hundred uses would bury the error under its own labels.
/// The note then says how many more there are.
const LIMIT: usize = 3;

/// Adds an instantiation label to every diagnostic inside a generic body.
pub fn attribute(diagnostics: &mut Diagnostics, program: &Program) {
    if program.generics.is_empty() {
        return;
    }
    for diagnostic in diagnostics.items_mut() {
        let primary = &diagnostic.primary;
        let Some(generic) = program.generics.values().find(|generic| {
            generic.source == primary.file && covers(node_span(generic), primary.span)
        }) else {
            continue;
        };

        let uses: Vec<_> = program
            .instances
            .values()
            .flatten()
            .filter(|instance| instance.name == generic.name)
            .collect();
        if uses.is_empty() {
            continue;
        }

        for instance in uses.iter().take(LIMIT) {
            let arguments = instance.arguments.join(", ");
            diagnostic.secondary.push(lark_diag::Label {
                file: instance.source,
                span: instance.span,
                text: format!("`{}<{arguments}>` instantiates it here", instance.name),
            });
        }
        if uses.len() > LIMIT {
            diagnostic
                .notes
                .push(format!("and {} more instantiations", uses.len() - LIMIT));
        }
        diagnostic.notes.push(format!(
            "rule G-4. `{}` has no constraint, so the error appears after substitution",
            generic.name
        ));
    }
}

/// Returns the span of a generic declaration.
fn node_span(generic: &lark_mono::Generic) -> Span {
    let range = generic.node.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/// Reports whether one span holds another.
fn covers(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
