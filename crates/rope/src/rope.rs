use std::ops::Range;

const DEFAULT_CHUNK_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RopePoint {
    pub row: usize,
    pub column: usize,
}

impl RopePoint {
    fn add(self, other: Self) -> Self {
        if other.row == 0 {
            Self {
                row: self.row,
                column: self.column + other.column,
            }
        } else {
            Self {
                row: self.row + other.row,
                column: other.column,
            }
        }
    }

    fn subtract(self, other: Self) -> Self {
        debug_assert!(other <= self);
        if self.row == other.row {
            Self {
                row: 0,
                column: self.column - other.column,
            }
        } else {
            Self {
                row: self.row - other.row,
                column: self.column,
            }
        }
    }
}

impl PartialOrd for RopePoint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RopePoint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.row
            .cmp(&other.row)
            .then_with(|| self.column.cmp(&other.column))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSummary {
    pub len: usize,
    pub line_break_count: usize,
    pub extent: RopePoint,
}

impl TextSummary {
    pub fn from_text(text: &str) -> Self {
        let mut summary = Self::default();
        summary.len = text.len();

        for character in text.chars() {
            if character == '\n' {
                summary.line_break_count += 1;
                summary.extent.row += 1;
                summary.extent.column = 0;
            } else {
                summary.extent.column += character.len_utf8();
            }
        }

        summary
    }

    fn append(self, other: Self) -> Self {
        Self {
            len: self.len + other.len,
            line_break_count: self.line_break_count + other.line_break_count,
            extent: self.extent.add(other.extent),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RopeChunk {
    text: String,
    line_breaks: Vec<usize>,
    summary: TextSummary,
}

impl RopeChunk {
    fn new(text: String) -> Self {
        let line_breaks = text
            .bytes()
            .enumerate()
            .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset))
            .collect();
        let summary = TextSummary::from_text(&text);

        Self {
            text,
            line_breaks,
            summary,
        }
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

    pub fn summary(&self) -> TextSummary {
        self.summary
    }

    fn point_for_offset(&self, offset: usize) -> RopePoint {
        let clipped = offset.min(self.len());
        let mut row = 0;
        let mut line_start = 0;

        for line_break in &self.line_breaks {
            if *line_break >= clipped {
                break;
            }
            row += 1;
            line_start = *line_break + 1;
        }

        RopePoint {
            row,
            column: clipped - line_start,
        }
    }

    fn offset_for_point(&self, point: RopePoint) -> usize {
        let mut current_row = 0;
        let mut current_line_start = 0;

        for line_break in &self.line_breaks {
            if current_row == point.row {
                return (current_line_start + point.column).min(*line_break);
            }
            current_row += 1;
            current_line_start = *line_break + 1;
        }

        if current_row == point.row {
            (current_line_start + point.column).min(self.len())
        } else {
            self.len()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RopeNode {
    summary: TextSummary,
    kind: RopeNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RopeNodeKind {
    Leaf {
        chunk_index: usize,
    },
    Branch {
        left: Box<RopeNode>,
        right: Box<RopeNode>,
    },
}

impl RopeNode {
    fn leaf(chunk_index: usize, summary: TextSummary) -> Self {
        Self {
            summary,
            kind: RopeNodeKind::Leaf { chunk_index },
        }
    }

    fn branch(left: RopeNode, right: RopeNode) -> Self {
        Self {
            summary: left.summary.append(right.summary),
            kind: RopeNodeKind::Branch {
                left: Box::new(left),
                right: Box::new(right),
            },
        }
    }

    fn height(&self) -> usize {
        match &self.kind {
            RopeNodeKind::Leaf { .. } => 1,
            RopeNodeKind::Branch { left, right } => 1 + left.height().max(right.height()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rope {
    chunks: Vec<RopeChunk>,
    root: Option<Box<RopeNode>>,
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
        let root = Self::build_tree(&chunks, 0..chunks.len()).map(Box::new);

        Self {
            chunks,
            root,
            len,
            line_break_count,
        }
    }

    fn build_tree(chunks: &[RopeChunk], range: Range<usize>) -> Option<RopeNode> {
        match range.end - range.start {
            0 => None,
            1 => Some(RopeNode::leaf(range.start, chunks[range.start].summary())),
            _ => {
                let mid = range.start + (range.end - range.start) / 2;
                let left = Self::build_tree(chunks, range.start..mid)?;
                let right = Self::build_tree(chunks, mid..range.end)?;
                Some(RopeNode::branch(left, right))
            }
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

    pub fn summary(&self) -> TextSummary {
        self.root
            .as_ref()
            .map(|root| root.summary)
            .unwrap_or_default()
    }

    pub fn tree_height(&self) -> usize {
        self.root.as_ref().map(|root| root.height()).unwrap_or(0)
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
        self.root
            .as_ref()
            .map(|root| self.point_for_offset_in_node(root, clipped, RopePoint::default()))
            .unwrap_or_default()
    }

    pub fn offset_for_point(&self, point: RopePoint) -> usize {
        if point >= self.summary().extent {
            return self.len;
        }
        self.root
            .as_ref()
            .map(|root| self.offset_for_point_in_node(root, point, 0))
            .unwrap_or_default()
    }

    fn point_for_offset_in_node(
        &self,
        node: &RopeNode,
        offset: usize,
        prefix: RopePoint,
    ) -> RopePoint {
        match &node.kind {
            RopeNodeKind::Leaf { chunk_index } => {
                prefix.add(self.chunks[*chunk_index].point_for_offset(offset))
            }
            RopeNodeKind::Branch { left, right } => {
                if offset <= left.summary.len {
                    self.point_for_offset_in_node(left, offset, prefix)
                } else {
                    self.point_for_offset_in_node(
                        right,
                        offset - left.summary.len,
                        prefix.add(left.summary.extent),
                    )
                }
            }
        }
    }

    fn offset_for_point_in_node(
        &self,
        node: &RopeNode,
        point: RopePoint,
        prefix_len: usize,
    ) -> usize {
        match &node.kind {
            RopeNodeKind::Leaf { chunk_index } => {
                prefix_len + self.chunks[*chunk_index].offset_for_point(point)
            }
            RopeNodeKind::Branch { left, right } => {
                if point <= left.summary.extent {
                    self.offset_for_point_in_node(left, point, prefix_len)
                } else {
                    self.offset_for_point_in_node(
                        right,
                        point.subtract(left.summary.extent),
                        prefix_len + left.summary.len,
                    )
                }
            }
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
        assert_eq!(
            rope.summary(),
            TextSummary {
                len: 6,
                line_break_count: 2,
                extent: RopePoint { row: 2, column: 2 },
            }
        );
        assert_eq!(rope.chunks()[0].line_break_count(), 1);
        assert_eq!(rope.chunks()[1].line_break_count(), 1);
    }

    #[test]
    fn builds_a_summary_tree_over_chunks() {
        let rope = Rope::from_text_with_chunk_size("abcdefghijklmnop".to_string(), 2);

        assert_eq!(rope.chunks().len(), 8);
        assert_eq!(rope.tree_height(), 4);
        assert_eq!(rope.summary().len, 16);
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
    fn uses_tree_summaries_across_internal_nodes() {
        let rope = Rope::from_text_with_chunk_size("ab\ncd\nef\ngh\nij".to_string(), 2);

        assert_eq!(rope.point_for_offset(12), RopePoint { row: 4, column: 0 });
        assert_eq!(rope.offset_for_point(RopePoint { row: 3, column: 1 }), 10);
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
