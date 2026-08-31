use std::fmt;
use std::path::{Path, PathBuf};

use crate::Span;

/// The largest source file that the compiler accepts, in bytes.
///
/// A span holds a `u32` offset, so a file must fit in that range.
pub const MAX_SOURCE_LEN: usize = u32::MAX as usize;

/// A handle to one file in a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SourceId(u32);

impl SourceId {
    /// Returns the handle as an index into the file list.
    #[inline]
    const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A one based line and column position.
///
/// The column counts characters, not bytes, so a caret lands under the right
/// character in text that holds multi byte characters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LineCol {
    /// The line number. The first line is 1.
    pub line: u32,
    /// The column number. The first column is 1.
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// One source file, with its text and its line table.
#[derive(Debug)]
pub struct SourceFile {
    id: SourceId,
    path: PathBuf,
    text: String,
    /// The byte offset of the first character of each line. Always starts at 0.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// Returns the handle for this file.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> SourceId {
        self.id
    }

    /// Returns the path that the file came from.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the whole text of the file.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the number of lines in the file.
    ///
    /// A file with no newline holds one line. An empty file holds one line.
    #[inline]
    #[must_use]
    pub fn line_count(&self) -> u32 {
        // The vector always holds at least one entry, so the cast is exact.
        u32::try_from(self.line_starts.len()).unwrap_or(u32::MAX)
    }

    /// Turns a byte offset into a line and a column.
    ///
    /// An offset past the end of the file maps to the last position. An offset
    /// inside a multi byte character moves back to the start of that character.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = self.clamp_to_boundary(offset);
        let line_index = self.line_index(offset);
        let line_start = self.line_starts[line_index] as usize;
        let column = self.text[line_start..offset as usize].chars().count() + 1;
        LineCol {
            line: u32::try_from(line_index + 1).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
        }
    }

    /// Returns the text of one line, without its line ending.
    ///
    /// Returns `None` when the line number is outside the file.
    #[must_use]
    pub fn line_text(&self, line: u32) -> Option<&str> {
        let span = self.line_span(line)?;
        Some(self.text[span.as_range()].trim_end_matches(['\r', '\n']))
    }

    /// Returns the span of one line, including its line ending.
    ///
    /// Returns `None` when the line number is outside the file.
    #[must_use]
    pub fn line_span(&self, line: u32) -> Option<Span> {
        if line == 0 || line > self.line_count() {
            return None;
        }
        let index = line as usize - 1;
        let start = self.line_starts[index];
        let end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or_else(|| u32::try_from(self.text.len()).unwrap_or(u32::MAX));
        Some(Span::new(start, end))
    }

    /// Returns the text that a span covers.
    ///
    /// Returns `None` when the span reaches past the end of the file.
    #[must_use]
    pub fn span_text(&self, span: Span) -> Option<&str> {
        self.text.get(span.as_range())
    }

    /// Returns the index into `line_starts` for a byte offset.
    fn line_index(&self, offset: u32) -> usize {
        // The first entry is 0, so the partition point is at least 1.
        self.line_starts.partition_point(|&start| start <= offset) - 1
    }

    /// Moves an offset back to the nearest character boundary at or before it.
    fn clamp_to_boundary(&self, offset: u32) -> u32 {
        let limit = u32::try_from(self.text.len()).unwrap_or(u32::MAX);
        let mut offset = offset.min(limit);
        while offset > 0 && !self.text.is_char_boundary(offset as usize) {
            offset -= 1;
        }
        offset
    }
}

/// Every source file that one compiler run reads.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

/// The error that [`SourceMap::add`] returns for a file that does not fit.
#[derive(Debug, PartialEq, Eq)]
pub struct FileTooLarge {
    /// The path of the file.
    pub path: PathBuf,
    /// The length of the file in bytes.
    pub len: usize,
}

impl fmt::Display for FileTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} holds {} bytes, and the limit is {MAX_SOURCE_LEN} bytes",
            self.path.display(),
            self.len
        )
    }
}

impl std::error::Error for FileTooLarge {}

impl SourceMap {
    /// Builds an empty map.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file and returns its handle.
    ///
    /// # Errors
    ///
    /// Returns [`FileTooLarge`] when the text is longer than
    /// [`MAX_SOURCE_LEN`].
    pub fn add(
        &mut self,
        path: impl Into<PathBuf>,
        text: impl Into<String>,
    ) -> Result<SourceId, FileTooLarge> {
        let path = path.into();
        let text = text.into();
        if text.len() > MAX_SOURCE_LEN {
            return Err(FileTooLarge {
                path,
                len: text.len(),
            });
        }
        let id = SourceId(u32::try_from(self.files.len()).unwrap_or(u32::MAX));
        let line_starts = line_starts(&text);
        self.files.push(SourceFile {
            id,
            path,
            text,
            line_starts,
        });
        Ok(id)
    }

    /// Returns the file for a handle.
    ///
    /// # Panics
    ///
    /// Panics when the handle came from a different map. A handle is only
    /// valid for the map that produced it.
    #[inline]
    #[must_use]
    pub fn file(&self, id: SourceId) -> &SourceFile {
        assert!(
            id.index() < self.files.len(),
            "the source handle came from a different map"
        );
        &self.files[id.index()]
    }

    /// Returns every file in the map, in the order they were added.
    #[inline]
    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Returns the number of files in the map.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Reports whether the map holds no file.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Builds the line table for a text.
fn line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            // The loop index is inside a string that fits in a u32.
            starts.push(u32::try_from(index + 1).unwrap_or(u32::MAX));
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::{LineCol, SourceMap};

    fn map_with(text: &str) -> (SourceMap, super::SourceId) {
        let mut map = SourceMap::new();
        let id = match map.add("test.lark", text) {
            Ok(id) => id,
            Err(error) => unreachable!("the fixture text is short: {error}"),
        };
        (map, id)
    }

    #[test]
    fn empty_file_holds_one_line() {
        let (map, id) = map_with("");
        assert_eq!(map.file(id).line_count(), 1);
        assert_eq!(map.file(id).line_col(0), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn maps_offsets_to_lines_and_columns() {
        let (map, id) = map_with("abc\ndef\n");
        let file = map.file(id);
        assert_eq!(file.line_col(0), LineCol { line: 1, column: 1 });
        assert_eq!(file.line_col(2), LineCol { line: 1, column: 3 });
        assert_eq!(file.line_col(4), LineCol { line: 2, column: 1 });
        assert_eq!(file.line_col(6), LineCol { line: 2, column: 3 });
    }

    #[test]
    fn a_trailing_newline_opens_a_final_empty_line() {
        let (map, id) = map_with("abc\n");
        let file = map.file(id);
        assert_eq!(file.line_count(), 2);
        assert_eq!(file.line_text(2), Some(""));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // The euro sign takes three bytes in UTF-8.
        let (map, id) = map_with("a\u{20ac}b");
        let file = map.file(id);
        assert_eq!(file.line_col(4), LineCol { line: 1, column: 3 });
    }

    #[test]
    fn an_offset_inside_a_character_moves_back() {
        let (map, id) = map_with("a\u{20ac}b");
        let file = map.file(id);
        assert_eq!(file.line_col(2), LineCol { line: 1, column: 2 });
        assert_eq!(file.line_col(3), LineCol { line: 1, column: 2 });
    }

    #[test]
    fn an_offset_past_the_end_maps_to_the_last_position() {
        let (map, id) = map_with("abc");
        let file = map.file(id);
        assert_eq!(file.line_col(999), LineCol { line: 1, column: 4 });
    }

    #[test]
    fn line_text_drops_the_line_ending() {
        let (map, id) = map_with("abc\r\ndef\n");
        let file = map.file(id);
        assert_eq!(file.line_text(1), Some("abc"));
        assert_eq!(file.line_text(2), Some("def"));
        assert_eq!(file.line_text(4), None);
        assert_eq!(file.line_text(0), None);
    }
}
