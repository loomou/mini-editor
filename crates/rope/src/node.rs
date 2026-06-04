use crate::{RopeChunk, TextSummary};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RopeNode {
    pub(crate) summary: TextSummary,
    pub(crate) kind: RopeNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RopeNodeKind {
    Leaf {
        chunk: RopeChunk,
    },
    Branch {
        left: Arc<RopeNode>,
        right: Arc<RopeNode>,
    },
}

impl RopeNode {
    pub(crate) fn leaf(chunk: RopeChunk) -> Self {
        Self {
            summary: chunk.summary(),
            kind: RopeNodeKind::Leaf { chunk },
        }
    }

    pub(crate) fn branch(left: Arc<RopeNode>, right: Arc<RopeNode>) -> Self {
        Self {
            summary: left.summary.append(right.summary),
            kind: RopeNodeKind::Branch { left, right },
        }
    }

    pub(crate) fn height(&self) -> usize {
        match &self.kind {
            RopeNodeKind::Leaf { .. } => 1,
            RopeNodeKind::Branch { left, right } => 1 + left.height().max(right.height()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Rope;

    #[test]
    fn builds_a_summary_tree_over_chunks() {
        let rope = Rope::from_text_with_chunk_size("abcdefghijklmnop".to_string(), 2);

        assert_eq!(rope.chunks().len(), 8);
        assert_eq!(rope.tree_height(), 4);
        assert_eq!(rope.summary().len, 16);
    }
}
