use lark_span::{SourceId, Span};

use crate::code::{Code, Severity};

/// A span with a note attached, drawn under the source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// The file that the span belongs to.
    pub file: SourceId,
    /// The region that the label points at.
    pub span: Span,
    /// The text under the caret. An empty string draws a bare caret.
    pub text: String,
}

impl Label {
    /// Builds a label.
    pub fn new(file: SourceId, span: Span, text: impl Into<String>) -> Self {
        Self {
            file,
            span,
            text: text.into(),
        }
    }
}

/// A change that fixes the problem.
///
/// The renderer draws the line as it looks after the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// The file that the span belongs to.
    pub file: SourceId,
    /// The region to replace. An empty span inserts text.
    pub span: Span,
    /// The text that takes the place of the span.
    pub replacement: String,
}

/// One problem that the compiler reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The stable code from the catalogue.
    pub code: Code,
    /// How serious the problem is.
    pub severity: Severity,
    /// The headline. It defaults to the catalogue message.
    pub message: String,
    /// The region that the problem belongs to.
    pub primary: Label,
    /// Extra regions that explain the problem.
    pub secondary: Vec<Label>,
    /// Extra lines of context.
    pub notes: Vec<String>,
    /// The headline of the fix.
    pub help: Option<String>,
    /// The fix itself.
    pub suggestion: Option<Suggestion>,
}

impl Diagnostic {
    /// Builds a diagnostic from a code and the region it belongs to.
    ///
    /// The severity and the message come from the catalogue.
    #[must_use]
    pub fn new(code: Code, file: SourceId, span: Span) -> Self {
        let (severity, message) = match code.info() {
            Some(info) => (info.severity, info.message.to_owned()),
            None => (Severity::Error, format!("unknown diagnostic code {code}")),
        };
        Self {
            code,
            severity,
            message,
            primary: Label::new(file, span, String::new()),
            secondary: Vec::new(),
            notes: Vec::new(),
            help: None,
            suggestion: None,
        }
    }

    /// Replaces the headline.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Sets the text under the primary caret.
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.primary.text = text.into();
        self
    }

    /// Adds a second region with its own note.
    #[must_use]
    pub fn secondary(mut self, file: SourceId, span: Span, text: impl Into<String>) -> Self {
        self.secondary.push(Label::new(file, span, text));
        self
    }

    /// Adds a line of context under the source.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Sets the headline of the fix.
    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Sets the fix itself.
    #[must_use]
    pub fn suggest(mut self, file: SourceId, span: Span, replacement: impl Into<String>) -> Self {
        self.suggestion = Some(Suggestion {
            file,
            span,
            replacement: replacement.into(),
        });
        self
    }
}

/// Every diagnostic that one compiler run produced.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Builds an empty collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Returns every diagnostic, in the order they arrived.
    #[must_use]
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Returns every diagnostic, so a later pass can add a label to one.
    ///
    /// Rule DQ-4 keeps one problem to one report, so a pass adds a label
    /// rather than a second diagnostic.
    pub fn items_mut(&mut self) -> &mut [Diagnostic] {
        &mut self.items
    }

    /// Reports whether any diagnostic stops the compiler from accepting the input.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|item| item.severity.is_fatal())
    }

    /// Returns the number of diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Reports whether the collection holds no diagnostic.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Sorts the diagnostics by file and then by position.
    ///
    /// The compiler produces diagnostics in pass order. A report reads better
    /// in source order.
    pub fn sort_by_position(&mut self) {
        self.items
            .sort_by_key(|item| (item.primary.file, item.primary.span.start, item.code));
    }

    /// Returns the diagnostics and consumes the collection.
    #[must_use]
    pub fn into_items(self) -> Vec<Diagnostic> {
        self.items
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<T: IntoIterator<Item = Diagnostic>>(&mut self, iter: T) {
        self.items.extend(iter);
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use lark_span::{SourceMap, Span};

    use super::{Diagnostic, Diagnostics};
    use crate::code::{LK0210, LK0301, LK0801, Severity};

    fn file() -> lark_span::SourceId {
        let mut map = SourceMap::new();
        match map.add("a.lark", "x") {
            Ok(id) => id,
            Err(error) => unreachable!("the fixture text is short: {error}"),
        }
    }

    #[test]
    fn takes_the_message_and_severity_from_the_catalogue() {
        let diagnostic = Diagnostic::new(LK0301, file(), Span::new(0, 1));
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("managed pointer"));
    }

    #[test]
    fn a_warning_code_builds_a_warning() {
        let diagnostic = Diagnostic::new(LK0801, file(), Span::new(0, 1));
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert!(!diagnostic.severity.is_fatal());
    }

    #[test]
    fn has_errors_ignores_a_warning() {
        let mut sink = Diagnostics::new();
        sink.push(Diagnostic::new(LK0801, file(), Span::new(0, 1)));
        assert!(!sink.has_errors());
        sink.push(Diagnostic::new(LK0301, file(), Span::new(0, 1)));
        assert!(sink.has_errors());
    }

    #[test]
    fn sorts_by_position_then_code() {
        let id = file();
        let mut sink = Diagnostics::new();
        sink.push(Diagnostic::new(LK0301, id, Span::new(5, 6)));
        sink.push(Diagnostic::new(LK0210, id, Span::new(1, 2)));
        sink.sort_by_position();
        assert_eq!(sink.items()[0].code, LK0210);
        assert_eq!(sink.items()[1].code, LK0301);
    }
}
