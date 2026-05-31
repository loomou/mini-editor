mod anchor;

use anchor::Point;
pub use anchor::{Anchor, Bias, BufferId};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryEntry {
    undo: Vec<TextEdit>,
    redo: Vec<TextEdit>,
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
        self.edit_group(vec![edit]);
    }

    pub fn edit_group(&mut self, edits: Vec<TextEdit>) {
        if edits.is_empty() {
            return;
        }

        let mut undo_edits = Vec::new();
        for edit in edits.iter().cloned() {
            let deleted_text = self.text.slice(edit.range.clone());
            let undo_range = edit.range.start..edit.range.start + edit.replacement.len();
            undo_edits.push(TextEdit {
                range: undo_range,
                replacement: deleted_text,
            });
            self.apply_edit(edit);
        }

        undo_edits.reverse();
        let history_entry = HistoryEntry {
            undo: undo_edits,
            redo: edits,
        };

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
        for edit in history_entry.undo.iter().cloned() {
            self.apply_edit(edit);
        }
        self.redo_stack.push(history_entry);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(history_entry) = self.redo_stack.pop() else {
            return false;
        };
        for edit in history_entry.redo.iter().cloned() {
            self.apply_edit(edit);
        }
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
        assert_eq!(
            buffer
                .snapshot()
                .offset_for_anchor(buffer.tracked_anchor(left).unwrap()),
            Some(6)
        );
        assert_eq!(
            buffer
                .snapshot()
                .offset_for_anchor(buffer.tracked_anchor(right).unwrap()),
            Some(9)
        );
    }

    #[test]
    fn anchors_resolve_to_current_offsets_after_deletion() {
        let mut buffer = Buffer::new(BufferId::new(1).unwrap(), "hello world");
        let left = buffer.track_anchor(buffer.snapshot().anchor_before(6));
        let right = buffer.track_anchor(buffer.snapshot().anchor_after(6));

        buffer.edit(TextEdit {
            range: 0..6,
            replacement: String::new(),
        });

        let snapshot = buffer.snapshot();
        assert_eq!(
            snapshot.offset_for_anchor(buffer.tracked_anchor(left).unwrap()),
            Some(0)
        );
        assert_eq!(
            snapshot.offset_for_anchor(buffer.tracked_anchor(right).unwrap()),
            Some(0)
        );
    }

    #[test]
    fn offset_for_anchor_rejects_anchors_from_other_buffers() {
        let buffer = Buffer::new(BufferId::new(1).unwrap(), "hello");
        let other_anchor = Anchor::new(BufferId::new(2).unwrap(), 3, Bias::Left);

        assert_eq!(buffer.snapshot().offset_for_anchor(other_anchor), None);
    }

    #[test]
    fn offset_for_anchor_clips_to_utf8_boundary() {
        let buffer = Buffer::new(BufferId::new(1).unwrap(), "aé");
        let inside_multibyte_character = Anchor::new(buffer.id(), 2, Bias::Right);

        assert_eq!(
            buffer
                .snapshot()
                .offset_for_anchor(inside_multibyte_character),
            Some(1)
        );
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

    #[test]
    fn undo_and_redo_reapply_grouped_text_edits() {
        let mut buffer = Buffer::new(BufferId::new(1).unwrap(), "one two three");

        buffer.edit_group(vec![
            TextEdit {
                range: 8..13,
                replacement: "3".to_string(),
            },
            TextEdit {
                range: 0..3,
                replacement: "1".to_string(),
            },
        ]);

        assert_eq!(buffer.snapshot().text(), "1 two 3");
        assert!(buffer.undo());
        assert_eq!(buffer.snapshot().text(), "one two three");
        assert!(buffer.redo());
        assert_eq!(buffer.snapshot().text(), "1 two 3");
    }
}
