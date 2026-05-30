use display::{DisplayMap, DisplayPoint, DisplaySnapshot};
use language::BufferHandle;
use multibuffer::{MultiBuffer, MultiBufferSnapshot};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionGoal {
    #[default]
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub id: usize,
    pub start: usize,
    pub end: usize,
    pub reversed: bool,
    pub goal: SelectionGoal,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            id: 0,
            start: offset,
            end: offset,
            reversed: false,
            goal: SelectionGoal::None,
        }
    }

    pub fn from_anchor_head(id: usize, anchor: usize, head: usize) -> Self {
        if head < anchor {
            Self {
                id,
                start: head,
                end: anchor,
                reversed: true,
                goal: SelectionGoal::None,
            }
        } else {
            Self {
                id,
                start: anchor,
                end: head,
                reversed: false,
                goal: SelectionGoal::None,
            }
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn head(&self) -> usize {
        if self.reversed { self.start } else { self.end }
    }

    pub fn tail(&self) -> usize {
        if self.reversed { self.end } else { self.start }
    }

    pub fn collapse_to(&mut self, offset: usize) {
        self.start = offset;
        self.end = offset;
        self.reversed = false;
        self.goal = SelectionGoal::None;
    }

    pub fn set_head(&mut self, head: usize) {
        let tail = self.tail();
        *self = Self::from_anchor_head(self.id, tail, head);
    }

    fn clamp_to_text(&mut self, text: &str) {
        self.start = floor_char_boundary(text, self.start);
        self.end = floor_char_boundary(text, self.end);

        if self.start == self.end {
            self.reversed = false;
            self.goal = SelectionGoal::None;
        }
    }
}

#[derive(Debug)]
pub struct EditorModel {
    buffer: MultiBuffer,
    selections: Vec<Selection>,
}

impl EditorModel {
    pub fn for_buffer(path_key: impl Into<String>, buffer: BufferHandle) -> Self {
        Self {
            buffer: MultiBuffer::singleton(path_key, buffer),
            selections: vec![Selection::caret(0)],
        }
    }

    pub fn snapshot(&self) -> MultiBufferSnapshot {
        self.buffer.snapshot()
    }

    pub fn title(&self) -> String {
        self.snapshot()
            .excerpts()
            .first()
            .map(|excerpt| excerpt.path_key.clone())
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn is_dirty(&self) -> bool {
        self.snapshot().is_dirty()
    }

    pub fn display_snapshot(&self, soft_wrap_column: Option<usize>) -> DisplaySnapshot {
        DisplayMap::new(soft_wrap_column).snapshot(&self.snapshot())
    }

    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    pub fn select(&mut self, range: Range<usize>) {
        let snapshot = self.snapshot();
        let text = snapshot.text();
        let start = floor_char_boundary(text, range.start);
        let end = floor_char_boundary(text, range.end);
        self.selections = vec![Selection::from_anchor_head(
            0,
            start.min(end),
            start.max(end),
        )];
    }

    pub fn select_anchor_head(&mut self, anchor: usize, head: usize) {
        let snapshot = self.snapshot();
        let text = snapshot.text();
        self.selections = vec![Selection::from_anchor_head(
            0,
            floor_char_boundary(text, anchor),
            floor_char_boundary(text, head),
        )];
    }

    pub fn cursor_offset(&self) -> Result<usize, String> {
        self.selections
            .first()
            .map(Selection::head)
            .ok_or_else(|| "editor has no active selection".to_string())
    }

    pub fn cursor_display_point(
        &self,
        soft_wrap_column: Option<usize>,
    ) -> Result<DisplayPoint, String> {
        let cursor = self.cursor_offset()?;
        Ok(self
            .display_snapshot(soft_wrap_column)
            .display_point_for_source_offset(cursor))
    }

    pub fn move_left(&mut self, extend: bool) -> Result<(), String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();

        if !extend && !selection.is_empty() {
            self.selections = vec![Selection::caret(selection.start)];
            return Ok(());
        }

        let text = self.snapshot().text().to_string();
        let target = previous_char_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    pub fn move_right(&mut self, extend: bool) -> Result<(), String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();

        if !extend && !selection.is_empty() {
            self.selections = vec![Selection::caret(selection.end)];
            return Ok(());
        }

        let text = self.snapshot().text().to_string();
        let target = next_char_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    fn move_active_head(&mut self, target: usize, extend: bool) -> Result<(), String> {
        let selection = self
            .selections
            .first_mut()
            .ok_or_else(|| "editor has no active selection".to_string())?;
        if extend {
            selection.set_head(target);
        } else {
            selection.collapse_to(target);
        }
        Ok(())
    }

    pub fn insert_text(&mut self, text: impl Into<String>) -> Result<(), String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();
        let replacement = text.into();
        let cursor = selection.start + replacement.len();
        self.buffer.edit(selection.range(), replacement)?;
        self.selections = vec![Selection::caret(cursor)];
        Ok(())
    }

    pub fn insert_char(&mut self, character: char) -> Result<(), String> {
        self.insert_text(character.to_string())
    }

    pub fn backspace(&mut self) -> Result<bool, String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();

        if !selection.is_empty() {
            return self.delete_range(selection.range());
        }

        let text = self.snapshot().text().to_string();
        let cursor = floor_char_boundary(&text, selection.head());
        let start = previous_char_boundary(&text, cursor);
        self.delete_range(start..cursor)
    }

    pub fn delete(&mut self) -> Result<bool, String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();

        if !selection.is_empty() {
            return self.delete_range(selection.range());
        }

        let text = self.snapshot().text().to_string();
        let cursor = floor_char_boundary(&text, selection.head());
        let end = next_char_boundary(&text, cursor);
        self.delete_range(cursor..end)
    }

    fn delete_range(&mut self, range: Range<usize>) -> Result<bool, String> {
        if range.start == range.end {
            return Ok(false);
        }

        self.buffer.edit(range.clone(), "")?;
        self.selections = vec![Selection::caret(range.start)];
        Ok(true)
    }

    pub fn undo(&mut self) -> Result<bool, String> {
        self.buffer.undo()
    }

    pub fn redo(&mut self) -> Result<bool, String> {
        self.buffer.redo()
    }

    pub fn refresh_buffer_ranges(&mut self) {
        self.buffer.refresh();
        let text = self.snapshot().text().to_string();
        for selection in &mut self.selections {
            selection.clamp_to_text(&text);
        }
    }
}

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = offset.min(text.len());
    while clipped > 0 && !text.is_char_boundary(clipped) {
        clipped -= 1;
    }
    clipped
}

fn previous_char_boundary(text: &str, offset: usize) -> usize {
    let clipped = floor_char_boundary(text, offset);
    if clipped == 0 {
        return 0;
    }
    text[..clipped]
        .char_indices()
        .last()
        .map(|(offset, _)| offset)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = offset.min(text.len());
    while clipped < text.len() && !text.is_char_boundary(clipped) {
        clipped += 1;
    }
    if clipped == text.len() {
        return text.len();
    }
    let mut chars = text[clipped..].char_indices();
    let _current = chars.next();
    chars
        .next()
        .map(|(relative_offset, _)| clipped + relative_offset)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use language::Buffer;
    use text::BufferId;

    #[test]
    fn insertion_replaces_active_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(6..11);
        editor.insert_text("zed").unwrap();

        assert_eq!(editor.snapshot().text(), "hello zed");
        assert_eq!(editor.selections()[0].range(), 9..9);
        assert_eq!(editor.cursor_offset().unwrap(), 9);
    }

    #[test]
    fn insert_char_inserts_at_cursor() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "ac");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.move_right(false).unwrap();
        editor.insert_char('b').unwrap();

        assert_eq!(editor.snapshot().text(), "abc");
        assert_eq!(editor.cursor_offset().unwrap(), 2);
    }

    #[test]
    fn backspace_deletes_previous_character_or_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "aéz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(3..3);
        assert!(editor.backspace().unwrap());
        assert_eq!(editor.snapshot().text(), "az");
        assert_eq!(editor.cursor_offset().unwrap(), 1);

        editor.select(0..2);
        assert!(editor.backspace().unwrap());
        assert_eq!(editor.snapshot().text(), "");
        assert_eq!(editor.cursor_offset().unwrap(), 0);
    }

    #[test]
    fn delete_removes_next_character_or_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "aéz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..1);
        assert!(editor.delete().unwrap());
        assert_eq!(editor.snapshot().text(), "az");
        assert_eq!(editor.cursor_offset().unwrap(), 1);

        editor.select(0..2);
        assert!(editor.delete().unwrap());
        assert_eq!(editor.snapshot().text(), "");
        assert_eq!(editor.cursor_offset().unwrap(), 0);
    }

    #[test]
    fn delete_actions_report_noop_at_document_edges() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "a");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        assert!(!editor.backspace().unwrap());
        assert_eq!(editor.snapshot().text(), "a");

        editor.select(1..1);
        assert!(!editor.delete().unwrap());
        assert_eq!(editor.snapshot().text(), "a");
    }

    #[test]
    fn selection_tracks_head_tail_and_normalized_range() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_anchor_head(8, 2);

        let selection = &editor.selections()[0];
        assert_eq!(selection.range(), 2..8);
        assert_eq!(selection.head(), 2);
        assert_eq!(selection.tail(), 8);
        assert!(selection.reversed);
    }

    #[test]
    fn movement_collapses_or_extends_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(2..5);
        editor.move_left(false).unwrap();
        assert_eq!(editor.selections()[0].range(), 2..2);

        editor.move_right(true).unwrap();
        editor.move_right(true).unwrap();
        assert_eq!(editor.selections()[0].range(), 2..4);
        assert_eq!(editor.selections()[0].head(), 4);
        assert_eq!(editor.selections()[0].tail(), 2);
    }

    #[test]
    fn movement_respects_utf8_character_boundaries() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "aéz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.move_right(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 1);

        editor.move_right(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 3);

        editor.move_left(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 1);
    }

    #[test]
    fn cursor_display_point_uses_display_map() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(4..4);

        assert_eq!(
            editor.cursor_display_point(Some(3)).unwrap(),
            DisplayPoint { row: 1, column: 1 }
        );
    }

    #[test]
    fn display_snapshot_wraps_editor_text() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        let display = editor.display_snapshot(Some(3));

        assert_eq!(display.rows()[0].text, "abc");
        assert_eq!(display.rows()[1].text, "def");
    }

    #[test]
    fn undo_and_redo_flow_through_editor_model() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(6..11);
        editor.insert_text("zed").unwrap();
        assert_eq!(editor.snapshot().text(), "hello zed");

        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().text(), "hello world");

        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().text(), "hello zed");
    }

    #[test]
    fn editor_exposes_title_and_dirty_state_from_snapshot() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            language::SourceFile::new("src/main.rs"),
            "hello world",
        );
        let mut editor = EditorModel::for_buffer("src/main.rs", buffer.into_handle());

        assert_eq!(editor.title(), "src/main.rs");
        assert!(!editor.is_dirty());

        editor.select(6..11);
        editor.insert_text("zed").unwrap();

        assert!(editor.is_dirty());
    }

    #[test]
    fn refresh_buffer_ranges_tracks_external_buffer_length_changes() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            language::SourceFile::new("src/main.rs"),
            "old",
        )
        .into_handle();
        let mut editor = EditorModel::for_buffer("src/main.rs", buffer.clone());

        buffer.borrow_mut().reload_saved_text("new longer text");
        editor.refresh_buffer_ranges();

        assert_eq!(editor.snapshot().text(), "new longer text");
    }

    #[test]
    fn refresh_buffer_ranges_clamps_selection_after_external_shrink() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            language::SourceFile::new("src/main.rs"),
            "hello world",
        )
        .into_handle();
        let mut editor = EditorModel::for_buffer("src/main.rs", buffer.clone());
        editor.select_anchor_head(11, 6);

        buffer.borrow_mut().reload_saved_text("hello");
        editor.refresh_buffer_ranges();

        let selection = &editor.selections()[0];
        assert_eq!(selection.range(), 5..5);
        assert_eq!(selection.head(), 5);
        assert!(!selection.reversed);
    }

    #[test]
    fn refresh_buffer_ranges_clamps_to_utf8_boundary_after_external_shrink() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            language::SourceFile::new("src/main.rs"),
            "aéz",
        )
        .into_handle();
        let mut editor = EditorModel::for_buffer("src/main.rs", buffer.clone());
        editor.select(4..4);

        buffer.borrow_mut().reload_saved_text("aé");
        editor.refresh_buffer_ranges();

        assert_eq!(editor.cursor_offset().unwrap(), 3);
    }
}
