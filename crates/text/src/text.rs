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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryEntry {
    undo: TextEdit,
    redo: TextEdit,
}

#[derive(Debug)]
pub struct Buffer {
    id: BufferId,
    text: Rope,
    version: u64,
    anchors: Vec<Anchor>,
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
}

impl Buffer {
    pub fn new(id: BufferId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: Rope::new(text.into()),
            version: 0,
            anchors: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
        let deleted_text = self.text.slice(edit.range.clone());
        let undo_range = edit.range.start..edit.range.start + edit.replacement.len();
        let history_entry = HistoryEntry {
            undo: TextEdit {
                range: undo_range,
                replacement: deleted_text,
            },
            redo: edit.clone(),
        };
        self.apply_edit(edit);
        self.undo_stack.push(history_entry);
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(history_entry) = self.undo_stack.pop() else {
            return false;
        };
        self.apply_edit(history_entry.undo.clone());
        self.redo_stack.push(history_entry);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(history_entry) = self.redo_stack.pop() else {
            return false;
        };
        self.apply_edit(history_entry.redo.clone());
        self.undo_stack.push(history_entry);
        true
    }

    fn apply_edit(&mut self, edit: TextEdit) {
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

    #[test]
    fn undo_and_redo_reapply_text_edits() {
        let mut buffer = Buffer::new(BufferId::new(1).unwrap(), "hello world");

        buffer.edit(TextEdit {
            range: 6..11,
            replacement: "zed".to_string(),
        });

        assert_eq!(buffer.snapshot().text(), "hello zed");
        assert!(buffer.can_undo());
        assert!(buffer.undo());
        assert_eq!(buffer.snapshot().text(), "hello world");
        assert!(buffer.can_redo());
        assert!(buffer.redo());
        assert_eq!(buffer.snapshot().text(), "hello zed");
    }
}
