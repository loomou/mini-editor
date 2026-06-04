use crate::node::{RopeNode, RopeNodeKind};
use crate::{RopeChunk, RopePoint, TextSummary};
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

const DEFAULT_CHUNK_SIZE: usize = 32;

pub struct Rope {
    chunks: OnceLock<Arc<[RopeChunk]>>,
    root: Option<Arc<RopeNode>>,
    len: usize,
    line_break_count: usize,
}

impl Clone for Rope {
    fn clone(&self) -> Self {
        let chunks = OnceLock::new();
        if let Some(cached_chunks) = self.chunks.get() {
            let _ = chunks.set(cached_chunks.clone());
        }

        Self {
            chunks,
            root: self.root.clone(),
            len: self.len,
            line_break_count: self.line_break_count,
        }
    }
}

impl fmt::Debug for Rope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Rope")
            .field("chunks", &self.chunks())
            .field("root", &self.root)
            .field("len", &self.len)
            .field("line_break_count", &self.line_break_count)
            .finish()
    }
}

impl Default for Rope {
    fn default() -> Self {
        Self {
            chunks: OnceLock::new(),
            root: None,
            len: 0,
            line_break_count: 0,
        }
    }
}

impl PartialEq for Rope {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self.line_break_count == other.line_break_count
            && self.chunks() == other.chunks()
    }
}

impl Eq for Rope {}

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

        if !chunk.is_empty() {
            raw_chunks.push(chunk);
        }

        let chunks = raw_chunks
            .into_iter()
            .map(RopeChunk::new)
            .collect::<Vec<_>>();

        Self::from_chunks(Arc::from(chunks))
    }

    fn from_chunks(chunks: Arc<[RopeChunk]>) -> Self {
        let len = chunks.iter().map(RopeChunk::len).sum();
        let line_break_count = chunks.iter().map(RopeChunk::line_break_count).sum();
        let root = Self::build_tree(&chunks, 0..chunks.len());
        let chunk_cache = OnceLock::new();
        let _ = chunk_cache.set(chunks);

        Self {
            chunks: chunk_cache,
            root,
            len,
            line_break_count,
        }
    }

    fn build_tree(chunks: &[RopeChunk], range: Range<usize>) -> Option<Arc<RopeNode>> {
        match range.end - range.start {
            0 => None,
            1 => Some(Arc::new(RopeNode::leaf(chunks[range.start].clone()))),
            _ => {
                let mid = range.start + (range.end - range.start) / 2;
                let left = Self::build_tree(chunks, range.start..mid)?;
                let right = Self::build_tree(chunks, mid..range.end)?;
                Some(Arc::new(RopeNode::branch(left, right)))
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
        self.chunks.get_or_init(|| {
            let mut chunks = Vec::new();
            if let Some(root) = &self.root {
                Self::collect_chunks(root, &mut chunks);
            }
            Arc::from(chunks)
        })
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
        self.chunks().iter().map(RopeChunk::text).collect()
    }

    pub fn text(&self) -> String {
        self.chunks().iter().map(RopeChunk::text).collect()
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        assert!(range.start <= range.end, "slice range is reversed");
        assert!(range.end <= self.len, "slice range is out of bounds");

        let mut output = String::new();
        let mut chunk_start = 0;
        for chunk in self.chunks().iter() {
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

        let replacement = replacement.into();
        let (before_range, range_and_after) = self.split_at(range.start);
        let (_, after_range) = range_and_after.split_at(range.end - range.start);
        let replacement = Self::from_text_with_chunk_size(replacement, DEFAULT_CHUNK_SIZE);

        *self = before_range.concat(replacement).concat(after_range);
    }

    fn split_at(&self, offset: usize) -> (Self, Self) {
        assert!(offset <= self.len, "split offset is out of bounds");
        let (left_root, right_root) = self
            .root
            .as_ref()
            .map(|root| Self::split_node(root, offset))
            .unwrap_or((None, None));

        (Self::from_root(left_root), Self::from_root(right_root))
    }

    fn concat(self, other: Self) -> Self {
        Self::from_root(Self::concat_roots(self.root, other.root))
    }

    fn from_root(root: Option<Arc<RopeNode>>) -> Self {
        let Some(root) = root else {
            return Self::default();
        };

        let summary = root.summary;
        Self {
            chunks: OnceLock::new(),
            root: Some(root),
            len: summary.len,
            line_break_count: summary.line_break_count,
        }
    }

    fn collect_chunks(node: &RopeNode, chunks: &mut Vec<RopeChunk>) {
        match &node.kind {
            RopeNodeKind::Leaf { chunk } => chunks.push(chunk.clone()),
            RopeNodeKind::Branch { left, right } => {
                Self::collect_chunks(left, chunks);
                Self::collect_chunks(right, chunks);
            }
        }
    }

    fn split_node(
        node: &Arc<RopeNode>,
        offset: usize,
    ) -> (Option<Arc<RopeNode>>, Option<Arc<RopeNode>>) {
        if offset == 0 {
            return (None, Some(node.clone()));
        }
        if offset >= node.summary.len {
            return (Some(node.clone()), None);
        }

        match &node.kind {
            RopeNodeKind::Leaf { chunk } => {
                let left_text = &chunk.text()[..offset];
                let right_text = &chunk.text()[offset..];
                (
                    Self::leaf_from_text(left_text),
                    Self::leaf_from_text(right_text),
                )
            }
            RopeNodeKind::Branch { left, right } => {
                if offset < left.summary.len {
                    let (left_left, left_right) = Self::split_node(left, offset);
                    (
                        left_left,
                        Self::concat_roots(left_right, Some(right.clone())),
                    )
                } else if offset == left.summary.len {
                    (Some(left.clone()), Some(right.clone()))
                } else {
                    let (right_left, right_right) =
                        Self::split_node(right, offset - left.summary.len);
                    (
                        Self::concat_roots(Some(left.clone()), right_left),
                        right_right,
                    )
                }
            }
        }
    }

    fn concat_roots(
        left: Option<Arc<RopeNode>>,
        right: Option<Arc<RopeNode>>,
    ) -> Option<Arc<RopeNode>> {
        match (left, right) {
            (None, None) => None,
            (Some(node), None) | (None, Some(node)) => Some(node),
            (Some(left), Some(right)) => Some(Arc::new(RopeNode::branch(left, right))),
        }
    }

    fn leaf_from_text(text: &str) -> Option<Arc<RopeNode>> {
        (!text.is_empty()).then(|| Arc::new(RopeNode::leaf(RopeChunk::new(text.to_string()))))
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
            RopeNodeKind::Leaf { chunk } => prefix.add(chunk.point_for_offset(offset)),
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
            RopeNodeKind::Leaf { chunk } => prefix_len + chunk.offset_for_point(point),
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
    use crate::{Rope, RopePoint};
    use std::sync::Arc;

    #[test]
    fn splits_text_into_chunks() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.chunk_texts(), vec!["ab", "cd", "ef"]);
        assert_eq!(rope.text(), "abcdef");
    }

    #[test]
    fn slices_across_chunk_boundaries() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        assert_eq!(rope.slice(1..5), "bcde");
    }

    #[test]
    fn clones_share_rope_storage_for_snapshots() {
        let rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);
        let clone = rope.clone();

        assert!(Arc::ptr_eq(
            rope.chunks.get().unwrap(),
            clone.chunks.get().unwrap()
        ));
        assert!(
            rope.root
                .as_ref()
                .zip(clone.root.as_ref())
                .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
        );
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

    #[test]
    fn replace_defers_rebuilding_the_chunk_view() {
        let mut rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        rope.replace(2..4, "XY");

        assert!(rope.chunks.get().is_none());
        assert_eq!(rope.text(), "abXYef");
    }

    #[test]
    fn replaces_text_at_chunk_boundaries() {
        let mut rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        rope.replace(2..4, "XY");

        assert_eq!(rope.text(), "abXYef");
        assert_eq!(rope.chunk_texts(), vec!["ab", "XY", "ef"]);
    }

    #[test]
    fn inserts_text_at_chunk_boundary_without_merging_neighbor_chunks() {
        let mut rope = Rope::from_text_with_chunk_size("abcdef".to_string(), 2);

        rope.replace(2..2, "XY");

        assert_eq!(rope.text(), "abXYcdef");
        assert_eq!(rope.chunk_texts(), vec!["ab", "XY", "cd", "ef"]);
    }
}
