/// A compact range inside an AeoScript source file.
///
/// AST nodes store only this 8-byte byte-offset range instead of carrying
/// line/column data through the entire syntax tree. The source text remains
/// owned externally and can be used later to resolve the range to editor
/// line/column coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn single(position: u32) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn join(self, other: SourceSpan) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }
}

/// One-based editor position used by diagnostics.
///
/// This is intentionally separate from SourceSpan so line/column information
/// does not have to be stored on every AST node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

impl SourcePosition {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl Default for SourcePosition {
    fn default() -> Self {
        Self { line: 1, column: 1 }
    }
}

/// A source location combines an optional file identity with a byte span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub span: SourceSpan,
}

impl SourceLocation {
    pub fn new(file: Option<String>, span: SourceSpan) -> Self {
        Self { file, span }
    }

    pub fn unknown() -> Self {
        Self {
            file: None,
            span: SourceSpan::single(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceLocation, SourcePosition, SourceSpan};

    #[test]
    fn spans_use_compact_byte_offsets() {
        let span = SourceSpan::new(12, 27);

        assert_eq!(span.start, 12);
        assert_eq!(span.end, 27);
        assert_eq!(span.len(), 15);
        assert!(!span.is_empty());
    }

    #[test]
    fn empty_span_has_zero_length() {
        let span = SourceSpan::single(42);

        assert_eq!(span.start, 42);
        assert_eq!(span.end, 42);
        assert_eq!(span.len(), 0);
        assert!(span.is_empty());
    }

    #[test]
    fn spans_can_join() {
        let left = SourceSpan::new(10, 20);
        let right = SourceSpan::new(20, 35);

        assert_eq!(left.join(right), SourceSpan::new(10, 35));
    }

    #[test]
    fn positions_are_one_based() {
        let position = SourcePosition::new(4, 12);

        assert_eq!(position.line, 4);
        assert_eq!(position.column, 12);
    }

    #[test]
    fn unknown_location_has_safe_defaults() {
        let location = SourceLocation::unknown();

        assert_eq!(location.file, None);
        assert_eq!(location.span, SourceSpan::single(0));
    }

    #[test]
    fn file_location_preserves_file_name() {
        let span = SourceSpan::new(3, 9);

        let location = SourceLocation::new(Some("player.aeo".to_string()), span);

        assert_eq!(location.file.as_deref(), Some("player.aeo"));
        assert_eq!(location.span, span);
    }
}
