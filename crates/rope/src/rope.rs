use std::ops::Range;

const DEFAULT_CHUNK_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RopePoint {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RopeChunk {
    text: String,
    line_breaks: Vec<usize>,
}

impl RopeChunk {
    fn new(text: String) -> Self {
        let line_breaks = text
            .bytes()
            .enumerate()
            .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset))
            .collect();
        Self { text, line_breaks }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_break_count(&self) -> usize {
        self.line_breaks.len()
    }

    fn last_line_start(&self) -> Option<usize> {
        self.line_breaks.last().map(|offset| offset + 1)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rope {
    chunks: Vec<RopeChunk>,
    len: usize,
    line_break_count: usize,
}

impl Rope {
    pub fn new(text: impl Into<String>) -> Self {
        Self::from_text_with_chunk_size(text.into(), DEFAULT_CHUNK_SIZE)
    }

    pub fn from_text_with_chunk_size(text: String, chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk size must be non-zero");
        let mut raw_chunks = Vec::new();
        let mut chunk = String::new();

        for char in text.chars() {
            if chunk.len() + char.len_utf8() > chunk_size && !chunk.is_empty() {
                raw_chunks.push(chunk);
                chunk = String::new();
            }
            chunk.push(char);
        }

        if !chunk.is_empty() || raw_chunks.is_empty() {
            raw_chunks.push(chunk);
        }

        let chunks = raw_chunks
            .into_iter()
            .map(RopeChunk::new)
            .collect::<Vec<_>>();
        let len = chunks.iter().map(RopeChunk::len).sum();
        let line_break_count = chunks.iter().map(RopeChunk::line_break_count).sum();

        Self {
            chunks,
            len,
            line_break_count,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn line_count(&self) -> usize {
        self.line_break_count + 1
    }

    pub fn chunks(&self) -> &[RopeChunk] {
        &self.chunks
    }

    pub fn chunk_texts(&self) -> Vec<&str> {
        self.chunks.iter().map(RopeChunk::text).collect()
    }

    pub fn text(&self) -> String {
        self.chunks.iter().map(RopeChunk::text).collect()
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end, "slice range is reversed");
        assert!(range.end <= self.len, "slice range is out of bounds");

        let mut output = String::new();
        let mut chunk_start = 0;
        for chunk in &self.chunks {
            let chunk_end = chunk_start + chunk.len();
            let start = range.start.max(chunk_start);
            let end = range.end.min(chunk_end);
            if start < end {
                output.push_str(&chunk.text()[start - chunk_start..end - chunk_start]);
            }
            chunk_start = chunk_end;
            if chunk_start >= range.end {
                break;
            }
        }
        output
    }

    pub fn replace(&mut self, range: Range<usize>, replacement: impl Into<String>) {
        assert!(range.start <= range.end, "replace range is reversed");
        assert!(range.end <= self.len, "replace range is out of bounds");

        let mut text = self.text();
        text.replace_range(range, &replacement.into());
        *self = Self::from_text_with_chunk_size(text, DEFAULT_CHUNK_SIZE);
    }

    pub fn point_for_offset(&self, offset: usize) -> RopePoint {
        let clipped = offset.min(self.len);
        let mut row = 0;
        let mut line_start = 0;
        let mut chunk_start = 0;

        for chunk in &self.chunks {
            let chunk_end = chunk_start + chunk.len();
            if clipped > chunk_end {
                row += chunk.line_break_count();
                if let Some(last_line_start) = chunk.last_line_start() {
                    line_start = chunk_start + last_line_start;
                }
                chunk_start = chunk_end;
                continue;
            }

            for line_break in &chunk.line_breaks {
                let absolute_break = chunk_start + *line_break;
                if absolute_break >= clipped {
                    break;
                }
                row += 1;
                line_start = absolute_break + 1;
            }

            return RopePoint {
                row,
                column: clipped - line_start,
            };
        }

        RopePoint {
            row,
            column: clipped.saturating_sub(line_start),
        }
    }

    pub fn offset_for_point(&self, point: RopePoint) -> usize {
        let mut current_row = 0;
        let mut current_line_start = 0;
        let mut chunk_start = 0;

        for chunk in &self.chunks {
            for line_break in &chunk.line_breaks {
                let absolute_break = chunk_start + *line_break;
                if current_row == point.row {
                    return (current_line_start + point.column).min(absolute_break);
                }
                current_row += 1;
                current_line_start = absolute_break + 1;
            }
            chunk_start += chunk.len();
        }

        if current_row == point.row {
            (current_line_start + point.column).min(self.len)
        } else {
            self.len
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_into_chunks() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.chunk_texts(), vec!["ab", "cd", "ef"]);
        assert_eq!(rope.text(), "abcdef");
    }

    #[test]
    fn stores_line_summaries_per_chunk() {
        let rope = Rope::from_text_with_chunk_size("a\nb\ncd".to_string(), 3);

        assert_eq!(rope.line_count(), 3);
        assert_eq!(rope.chunks()[0].line_break_count(), 1);
        assert_eq!(rope.chunks()[1].line_break_count(), 1);
    }

    #[test]
    fn slices_across_chunk_boundaries() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.slice(1..5), "bcde");
    }

    #[test]
    fn converts_between_offsets_and_points_using_summaries() {
        let rope = Rope::from_text_with_chunk_size("a\nbeta\nc".to_string(), 3);

        assert_eq!(rope.point_for_offset(3), RopePoint { row: 1, column: 1 });
        assert_eq!(rope.offset_for_point(RopePoint { row: 1, column: 2 }), 4);
    }

    #[test]
    fn replaces_text_and_rebalances_chunks() {
        let mut rope = Rope::from_text_with_chunk_size(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(),
            5,
        );

        rope.replace(10..20, "zed");

        assert_eq!(rope.text(), "abcdefghijzeduvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert!(rope.chunks().len() > 1);
    }
}
