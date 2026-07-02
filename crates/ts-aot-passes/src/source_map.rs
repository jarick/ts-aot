use ts_aot_core::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone)]
pub struct SourceMap {
    source: String,
    line_starts: Vec<u32>,
}

impl SourceMap {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    #[must_use]
    pub fn lookup(&self, span: Span) -> (LineCol, LineCol) {
        let start = self.lookup_one(span.start);
        let end = self.lookup_one(span.end);
        (start, end)
    }

    #[must_use]
    pub fn line_text(&self, line: u32) -> Option<&str> {
        let line = line as usize;
        if line >= self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line] as usize;
        let end = self
            .line_starts
            .get(line + 1)
            .map_or(self.source.len(), |&n| n as usize);
        let raw = self.source.get(start..end)?;
        let trimmed = raw
            .strip_suffix("\r\n")
            .or_else(|| raw.strip_suffix('\n'))
            .unwrap_or(raw);
        Some(trimmed)
    }

    fn lookup_one(&self, offset: u32) -> LineCol {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        LineCol {
            line: line_idx as u32,
            column: offset - line_start,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_has_one_line() {
        let map = SourceMap::new("");
        assert_eq!(map.line_count(), 1);
        let (start, end) = map.lookup(Span::new(0, 0));
        assert_eq!(start, LineCol { line: 0, column: 0 });
        assert_eq!(end, LineCol { line: 0, column: 0 });
    }

    #[test]
    fn single_line_no_newline() {
        let map = SourceMap::new("hello");
        assert_eq!(map.line_count(), 1);
        let (start, end) = map.lookup(Span::new(0, 5));
        assert_eq!(start, LineCol { line: 0, column: 0 });
        assert_eq!(end, LineCol { line: 0, column: 5 });
    }

    #[test]
    fn two_lines() {
        let map = SourceMap::new("abc\nde");
        assert_eq!(map.line_count(), 2);
        let (start, end) = map.lookup(Span::new(4, 6));
        assert_eq!(start, LineCol { line: 1, column: 0 });
        assert_eq!(end, LineCol { line: 1, column: 2 });
    }

    #[test]
    fn trailing_newline_records_virtual_empty_line() {
        let map = SourceMap::new("a\nb\nc\n");
        assert_eq!(map.line_count(), 4);
        let (start, _) = map.lookup(Span::new(4, 5));
        assert_eq!(start, LineCol { line: 2, column: 0 });
        let (eof, _) = map.lookup(Span::new(6, 6));
        assert_eq!(eof, LineCol { line: 3, column: 0 });
    }

    #[test]
    fn line_text_strips_crlf() {
        let map = SourceMap::new("aaa\r\nbbb");
        assert_eq!(map.line_text(0), Some("aaa"));
        assert_eq!(map.line_text(1), Some("bbb"));
    }

    #[test]
    fn lookup_middle_of_line() {
        let map = SourceMap::new("hello\nworld\n");
        let (start, _) = map.lookup(Span::new(8, 9));
        assert_eq!(start, LineCol { line: 1, column: 2 });
    }

    #[test]
    fn lookup_out_of_range_clamps_to_last_line() {
        let map = SourceMap::new("ab\ncd");
        let (start, _) = map.lookup(Span::new(100, 105));
        assert_eq!(start.line, 1);
    }

    #[test]
    fn line_text_returns_slice() {
        let map = SourceMap::new("aaa\nbbb\nccc");
        assert_eq!(map.line_text(0), Some("aaa"));
        assert_eq!(map.line_text(1), Some("bbb"));
        assert_eq!(map.line_text(2), Some("ccc"));
    }

    #[test]
    fn line_text_excludes_newline() {
        let map = SourceMap::new("aaa\nbbb");
        assert_eq!(map.line_text(0), Some("aaa"));
        assert_eq!(map.line_text(1), Some("bbb"));
    }

    #[test]
    fn line_text_out_of_range_returns_none() {
        let map = SourceMap::new("aaa");
        assert_eq!(map.line_text(0), Some("aaa"));
        assert_eq!(map.line_text(1), None);
        assert_eq!(map.line_text(99), None);
    }

    #[test]
    fn source_returns_original_text() {
        let map = SourceMap::new("hello world");
        assert_eq!(map.source(), "hello world");
    }
}
