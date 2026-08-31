use std::fmt::Write as _;

use lark_span::{SourceFile, SourceMap, Span};

use crate::diagnostic::{Diagnostic, Diagnostics, Label};

/// Renders one diagnostic in the format from chapter 12 of the specification.
///
/// The output ends with a newline.
#[must_use]
pub fn render(diagnostic: &Diagnostic, map: &SourceMap) -> String {
    let file = map.file(diagnostic.primary.file);
    let position = file.line_col(diagnostic.primary.span.start);
    let width = gutter_width(position.line);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{severity}[{code}]: {message}",
        severity = diagnostic.severity,
        code = diagnostic.code,
        message = diagnostic.message
    );
    let _ = writeln!(
        out,
        "{blank:width$}--> {path}:{position}",
        blank = "",
        path = file.path().display()
    );

    write_bar(&mut out, width);
    write_source_line(&mut out, file, &diagnostic.primary, width, '^');

    for label in &diagnostic.secondary {
        let other = map.file(label.file);
        write_bar(&mut out, width);
        write_source_line(&mut out, other, label, width, '-');
    }

    for note in &diagnostic.notes {
        let _ = writeln!(out, "{blank:width$} = note: {note}", blank = "");
    }

    if let Some(help) = &diagnostic.help {
        write_bar(&mut out, width);
        let _ = writeln!(out, "help: {help}");
        if let Some(suggestion) = &diagnostic.suggestion {
            write_suggestion(&mut out, map.file(suggestion.file), suggestion, width);
        }
    }

    out
}

/// Renders every diagnostic, separated by a blank line.
#[must_use]
pub fn render_all(diagnostics: &Diagnostics, map: &SourceMap) -> String {
    diagnostics
        .items()
        .iter()
        .map(|diagnostic| render(diagnostic, map))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the width of the line number column.
fn gutter_width(line: u32) -> usize {
    line.to_string().len()
}

/// Writes a gutter line that holds no source.
fn write_bar(out: &mut String, width: usize) {
    let _ = writeln!(out, "{blank:width$} |", blank = "");
}

/// Writes one source line and the caret line under it.
fn write_source_line(
    out: &mut String,
    file: &SourceFile,
    label: &Label,
    width: usize,
    caret: char,
) {
    let position = file.line_col(label.span.start);
    let Some(text) = file.line_text(position.line) else {
        return;
    };

    let _ = writeln!(out, "{line:>width$} | {text}", line = position.line);

    let pad = " ".repeat(position.column as usize - 1);
    let marks = caret
        .to_string()
        .repeat(caret_len(file, label.span, position.line));
    if label.text.is_empty() {
        let _ = writeln!(out, "{blank:width$} | {pad}{marks}", blank = "");
    } else {
        let _ = writeln!(
            out,
            "{blank:width$} | {pad}{marks} {text}",
            blank = "",
            text = label.text
        );
    }
}

/// Returns the number of caret characters for a span.
///
/// A span that reaches past the end of its first line stops at that end. An
/// empty span draws one caret, so a missing token still has a position.
fn caret_len(file: &SourceFile, span: Span, line: u32) -> usize {
    let Some(line_span) = file.line_span(line) else {
        return 1;
    };
    let end = span.end.min(line_span.end);
    let start = span.start.min(end);
    let Some(text) = file.span_text(Span::new(start, end)) else {
        return 1;
    };
    text.chars()
        .filter(|character| *character != '\n' && *character != '\r')
        .count()
        .max(1)
}

/// Writes the source line as it looks after the suggested change.
fn write_suggestion(
    out: &mut String,
    file: &SourceFile,
    suggestion: &crate::diagnostic::Suggestion,
    width: usize,
) {
    let position = file.line_col(suggestion.span.start);
    let (Some(line_span), Some(text)) =
        (file.line_span(position.line), file.line_text(position.line))
    else {
        return;
    };

    let head_len = (suggestion.span.start - line_span.start) as usize;
    let tail_len = (suggestion.span.end.min(line_span.end) - line_span.start) as usize;
    let (Some(head), Some(tail)) = (text.get(..head_len), text.get(tail_len..)) else {
        return;
    };

    write_bar(out, width);
    let _ = writeln!(
        out,
        "{line:>width$} | {head}{replacement}{tail}",
        line = position.line,
        replacement = suggestion.replacement
    );

    let mark = if suggestion.span.is_empty() { '+' } else { '~' };
    let pad = " ".repeat(head.chars().count());
    let marks = mark
        .to_string()
        .repeat(suggestion.replacement.chars().count().max(1));
    let _ = writeln!(out, "{blank:width$} | {pad}{marks}", blank = "");
}

#[cfg(test)]
mod tests {
    use lark_span::{SourceId, SourceMap, Span};

    use super::render;
    use crate::code::LK0301;
    use crate::diagnostic::Diagnostic;

    const SOURCE: &str = "void main(void) {\n    handle_opaque_data(count);\n}\n";

    fn fixture() -> (SourceMap, SourceId) {
        let mut map = SourceMap::new();
        let id = match map.add("app.lark", SOURCE) {
            Ok(id) => id,
            Err(error) => unreachable!("the fixture text is short: {error}"),
        };
        (map, id)
    }

    /// The offset of `count` on line 2.
    fn count_span() -> Span {
        let start = SOURCE.find("count").unwrap_or(0);
        Span::new(
            u32::try_from(start).unwrap_or(0),
            u32::try_from(start + "count".len()).unwrap_or(0),
        )
    }

    /// covers: DQ-1
    #[test]
    fn renders_the_header_the_location_and_the_caret() {
        let (map, id) = fixture();
        let diagnostic = Diagnostic::new(LK0301, id, count_span())
            .label("this is `gc Data<int>*`, the parameter is `void*`");
        let text = render(&diagnostic, &map);

        assert!(
            text.starts_with("error[LK0301]: no implicit conversion"),
            "{text}"
        );
        assert!(text.contains("--> app.lark:2:24"), "{text}");
        assert!(
            text.contains("2 |     handle_opaque_data(count);"),
            "{text}"
        );
        assert!(text.contains("^^^^^ this is `gc Data<int>*`"), "{text}");
    }

    /// covers: DQ-3
    #[test]
    fn renders_an_insertion_suggestion_with_plus_marks() {
        let (map, id) = fixture();
        let span = count_span();
        let diagnostic = Diagnostic::new(LK0301, id, span)
            .help("write the cast")
            .suggest(id, Span::at(span.start), "(void*)");
        let text = render(&diagnostic, &map);

        assert!(text.contains("help: write the cast"), "{text}");
        assert!(text.contains("handle_opaque_data((void*)count);"), "{text}");
        assert!(text.contains("+++++++"), "{text}");
    }

    #[test]
    fn an_empty_span_still_draws_one_caret() {
        let (map, id) = fixture();
        let diagnostic = Diagnostic::new(LK0301, id, Span::at(count_span().start));
        let text = render(&diagnostic, &map);
        assert!(text.contains(" ^\n"), "{text}");
    }

    #[test]
    fn a_note_appears_under_the_source() {
        let (map, id) = fixture();
        let diagnostic = Diagnostic::new(LK0301, id, count_span()).note("rule T-5");
        let text = render(&diagnostic, &map);
        assert!(text.contains("= note: rule T-5"), "{text}");
    }
}
