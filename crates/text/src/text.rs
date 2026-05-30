mod anchor;

pub use anchor::BufferId;
use anchor::{Anchor, Bias, Point};
use rope::{Rope, RopePoint};
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct BufferSnapshot {
    id: BufferId,
    text: Rope,
    version: u64,
}

impl BufferSnapshot {
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
}

#[derive(Clone, Debug)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Debug)]
pub struct Buffer {
    id: BufferId,
    text: Rope,
    version: u64,
    anchors: Vec<Anchor>,
}

impl Buffer {
    pub fn new(id: BufferId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: Rope::new(text.into()),
            version: 0,
            anchors: Vec::new(),
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot {
            id: self.id,
            text: self.text.clone(),
            version: self.version,
        }
    }

    pub fn track_anchor(&mut self, anchor: Anchor) -> usize {
        self.anchors.push(anchor);
        self.anchors.len() - 1
    }

    pub fn tracked_anchor(&self, index: usize) -> Option<Anchor> {
        self.anchors.get(index).copied()
    }

    pub fn edit(&mut self, edit: TextEdit) {
        assert!(edit.range.start <= edit.range.end, "edit range is reversed");
        assert!(
            edit.range.end <= self.text.len(),
            "edit range is out of bounds"
        );
        let inserted_len = edit.replacement.len();
        self.text.replace(edit.range.clone(), edit.replacement);
        for anchor in &mut self.anchors {
            *anchor = anchor.transform(edit.range.clone(), inserted_len);
        }
        self.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_move_with_edits() {
        let mut buffer = Buffer::new(BufferId::new(1).unwrap(), "hello world");
        let left = buffer.track_anchor(buffer.snapshot().anchor_before(6));
        let right = buffer.track_anchor(buffer.snapshot().anchor_after(6));

        buffer.edit(TextEdit {
            range: 6..11,
            replacement: "zed".to_string(),
        });

        assert_eq!(buffer.snapshot().text(), "hello zed");
        assert_eq!(buffer.tracked_anchor(left).unwrap().offset(), 6);
        assert_eq!(buffer.tracked_anchor(right).unwrap().offset(), 9);
    }

    #[test]
    fn converts_between_offsets_and_points() {
        let buffer = Buffer::new(BufferId::new(1).unwrap(), "a\nbeta\nc");
        let snapshot = buffer.snapshot();

        assert_eq!(snapshot.point_for_offset(3), Point { row: 1, column: 1 });
        assert_eq!(snapshot.offset_for_point(Point { row: 1, column: 2 }), 4);
    }
}
