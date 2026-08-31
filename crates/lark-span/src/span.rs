use std::fmt;

/// A half open byte range in one source file.
///
/// The range covers `start .. end`. An empty span, where `start == end`,
/// marks a position rather than a region. The parser uses an empty span to
/// point at the place where a token is missing.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    /// The first byte of the range.
    pub start: u32,
    /// The byte after the last byte of the range.
    pub end: u32,
}

impl Span {
    /// Builds a span from a start offset and an end offset.
    ///
    /// # Panics
    ///
    /// Panics when `end` is less than `start`. A reversed span is always a
    /// defect in the caller, never bad input from a user.
    #[inline]
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        assert!(start <= end, "a span cannot end before it starts");
        Self { start, end }
    }

    /// Builds an empty span at one offset.
    #[inline]
    #[must_use]
    pub const fn at(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Returns the length of the span in bytes.
    #[inline]
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    /// Reports whether the span covers no bytes.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Reports whether the span covers the given byte offset.
    ///
    /// An empty span contains no offset.
    #[inline]
    #[must_use]
    pub const fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Returns the smallest span that covers both inputs.
    #[inline]
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Returns the span as a range, for slicing into the source text.
    #[inline]
    #[must_use]
    pub const fn as_range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::Span;

    #[test]
    fn reports_length_and_emptiness() {
        assert_eq!(Span::new(3, 7).len(), 4);
        assert!(!Span::new(3, 7).is_empty());
        assert!(Span::at(3).is_empty());
        assert_eq!(Span::at(3).len(), 0);
    }

    #[test]
    fn contains_excludes_the_end_offset() {
        let span = Span::new(3, 7);
        assert!(!span.contains(2));
        assert!(span.contains(3));
        assert!(span.contains(6));
        assert!(!span.contains(7));
    }

    #[test]
    fn empty_span_contains_nothing() {
        assert!(!Span::at(3).contains(3));
    }

    #[test]
    fn join_covers_both_inputs_and_the_gap() {
        let joined = Span::new(1, 3).join(Span::new(8, 10));
        assert_eq!(joined, Span::new(1, 10));
    }

    #[test]
    fn join_is_symmetric() {
        let a = Span::new(1, 3);
        let b = Span::new(8, 10);
        assert_eq!(a.join(b), b.join(a));
    }

    #[test]
    #[should_panic(expected = "a span cannot end before it starts")]
    fn rejects_a_reversed_span() {
        let _ = Span::new(7, 3);
    }
}
