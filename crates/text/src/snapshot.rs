use crate::anchor::Point;
use crate::{Anchor, Bias, BufferId};
use rope::{Rope, RopePoint};
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct BufferSnapshot {
    pub(crate) id: BufferId,
    pub(crate) text: Rope,
    pub(crate) version: u64,
}

impl BufferSnapshot {
    pub(crate) fn new(id: BufferId, text: Rope, version: u64) -> Self {
        Self { id, text, version }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn text(&self) -> String {
        self.text.text()
    }

    pub fn text_slice(&self, range: Range<usize>) -> String {
        self.text.slice(range)
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn point_for_offset(&self, offset: usize) -> Point {
        let point = self.text.point_for_offset(offset);

        Point {
            row: point.row,
            column: point.column,
        }
    }

    pub fn offset_for_point(&self, point: Point) -> usize {
        self.text.offset_for_point(RopePoint {
            row: point.row,
            column: point.column,
        })
    }

    pub fn anchor_before(&self, offset: usize) -> Anchor {
        Anchor::new(self.id, offset.min(self.text.len()), Bias::Left)
    }

    pub fn anchor_after(&self, offset: usize) -> Anchor {
        Anchor::new(self.id, offset.min(self.text.len()), Bias::Right)
    }

    pub fn offset_for_anchor(&self, anchor: Anchor) -> Option<usize> {
        if anchor.buffer_id() != self.id {
            return None;
        }

        Some(self.floor_char_boundary(anchor.offset()))
    }

    fn floor_char_boundary(&self, offset: usize) -> usize {
        let text = self.text.text();
        let mut clipped = offset.min(text.len());
        while clipped > 0 && !text.is_char_boundary(clipped) {
            clipped -= 1;
        }
        clipped
    }
}
