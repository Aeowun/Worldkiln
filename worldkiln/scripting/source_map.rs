use super::source::{SourcePosition, SourceSpan};

/// Maps byte offsets in one source string to one-based editor positions.
///
/// The map stores only the byte offset where each line begins. This keeps the
/// structure small while allowing line/column resolution with binary search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMap {
    line_starts: Vec<u32>,
}

impl SourceMap {
    pub fn new(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity(
            source
                .as_bytes()
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count()
                + 1,
        );

        line_starts.push(0);

        for (index, &byte) in source.as_bytes().iter().enumerate() {
            if byte == b'\n' {
                let next = index.saturating_add(1);

                if next <= u32::MAX as usize {
                    line_starts.push(next as u32);
                }
            }
        }

        Self { line_starts }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn position(&self, offset: u32) -> SourcePosition {
        let index = self.line_index(offset);

        let line_start = self.line_starts[index];

        SourcePosition::new(index + 1, offset.saturating_sub(line_start) as usize + 1)
    }

    pub fn span_positions(&self, span: SourceSpan) -> (SourcePosition, SourcePosition) {
        (self.position(span.start), self.position(span.end))
    }

    pub fn line_index(&self, offset: u32) -> usize {
        let mut low = 0usize;
        let mut high = self.line_starts.len();

        while low < high {
            let middle = low + (high - low) / 2;

            if self.line_starts[middle] <= offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }

        low.saturating_sub(1)
    }

    pub fn line_start(&self, line: usize) -> Option<u32> {
        self.line_starts.get(line.saturating_sub(1)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::source::SourcePosition;

    #[test]
    fn empty_source_has_one_line() {
        let map = SourceMap::new("");

        assert_eq!(map.line_count(), 1);
        assert_eq!(map.position(0), SourcePosition::new(1, 1));
    }

    #[test]
    fn resolves_first_line() {
        let map = SourceMap::new("hello world");

        assert_eq!(map.position(0), SourcePosition::new(1, 1));

        assert_eq!(map.position(4), SourcePosition::new(1, 5));

        assert_eq!(map.position(10), SourcePosition::new(1, 11));
    }

    #[test]
    fn resolves_multiple_lines() {
        let map = SourceMap::new("hello\nworld\nthird");

        assert_eq!(map.line_count(), 3);

        assert_eq!(map.position(0), SourcePosition::new(1, 1));

        assert_eq!(map.position(5), SourcePosition::new(1, 6));

        assert_eq!(map.position(6), SourcePosition::new(2, 1));

        assert_eq!(map.position(10), SourcePosition::new(2, 5));

        assert_eq!(map.position(12), SourcePosition::new(3, 1));
    }

    #[test]
    fn trailing_newline_creates_next_empty_line() {
        let map = SourceMap::new("hello\n");

        assert_eq!(map.line_count(), 2);

        assert_eq!(map.position(6), SourcePosition::new(2, 1));
    }

    #[test]
    fn span_positions_resolve_both_ends() {
        let map = SourceMap::new("entity Test {\n    value: number = 10\n}");

        let span = SourceSpan::new(20, 32);

        let (start, end) = map.span_positions(span);

        assert!(start.line <= end.line);
        assert!(start.column > 0);
        assert!(end.column > 0);
    }

    #[test]
    fn line_index_uses_zero_based_internal_index() {
        let map = SourceMap::new("a\nb\nc");

        assert_eq!(map.line_index(0), 0);
        assert_eq!(map.line_index(2), 1);
        assert_eq!(map.line_index(4), 2);
    }

    #[test]
    fn line_start_is_one_based() {
        let map = SourceMap::new("a\nb\nc");

        assert_eq!(map.line_start(1), Some(0));
        assert_eq!(map.line_start(2), Some(2));
        assert_eq!(map.line_start(3), Some(4));
        assert_eq!(map.line_start(4), None);
    }

    #[test]
    fn offsets_beyond_source_remain_safe() {
        let map = SourceMap::new("abc");

        let position = map.position(10_000);

        assert_eq!(position.line, 1);

        assert_eq!(position.column, 10_001);
    }
}
