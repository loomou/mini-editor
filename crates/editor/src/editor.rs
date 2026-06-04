use crate::anchors::{attach_selection_anchors, resolve_selection_from_anchors};
use crate::model::EditorModel;

use crate::selection::{Selection, normalize_new_selections};
use crate::selection_history::{
    SelectionHistoryCheckpoint, SelectionHistoryEntry, selection_history_key,
};
use crate::utils::floor_char_boundary;
use display::{DisplayMap, DisplayPoint, DisplaySnapshot};
use language::BufferHandle;
use multibuffer::{MultiBuffer, MultiBufferSnapshot};
use std::ops::Range;

impl EditorModel {
    pub fn for_buffer(path_key: impl Into<String>, buffer: BufferHandle) -> Self {
        let mut buffer = MultiBuffer::singleton(path_key, buffer);
        let mut selection = Selection::caret(0);
        attach_selection_anchors(&mut buffer, &mut selection);

        Self {
            buffer,
            selections: vec![selection],
            active_selection_index: 0,
            selection_undo_stack: Vec::new(),
            selection_redo_stack: Vec::new(),
            selection_only_undo_stack: Vec::new(),
            selection_only_redo_stack: Vec::new(),
        }
    }

    pub fn snapshot(&self) -> MultiBufferSnapshot {
        self.buffer.snapshot()
    }

    pub fn text_version_key(&self) -> Vec<(u64, u64)> {
        self.buffer.text_version_key()
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

    pub fn source_offset_for_display_point(
        &self,
        row: usize,
        column: usize,
        soft_wrap_column: Option<usize>,
    ) -> usize {
        self.display_snapshot(soft_wrap_column)
            .source_offset_for_display_point(DisplayPoint { row, column })
    }

    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    pub fn active_selection_index(&self) -> usize {
        self.active_selection_index
    }

    pub fn set_active_selection_index(&mut self, index: usize) -> Result<(), String> {
        if index >= self.selections.len() {
            return Err(format!(
                "active selection index {index} is out of range for {} selections",
                self.selections.len()
            ));
        }

        self.active_selection_index = index;
        Ok(())
    }

    pub fn resolved_selections(&self) -> Vec<Selection> {
        self.selections
            .iter()
            .map(|selection| resolve_selection_from_anchors(&self.buffer, selection))
            .collect()
    }

    pub fn select(&mut self, range: Range<usize>) {
        self.select_ranges(vec![range]);
    }

    pub fn select_ranges(&mut self, ranges: Vec<Range<usize>>) {
        self.select_ranges_impl(ranges, true);
    }

    pub fn selection_history_checkpoint(&self) -> SelectionHistoryCheckpoint {
        SelectionHistoryCheckpoint {
            selections: self.resolved_selections(),
            active_selection_index: self.active_selection_index,
        }
    }

    pub fn commit_selection_only_history_from_checkpoint(
        &mut self,
        checkpoint: SelectionHistoryCheckpoint,
    ) {
        self.push_selection_only_history_from_current(
            checkpoint.selections,
            checkpoint.active_selection_index,
        );
    }

    pub(crate) fn select_ranges_impl(&mut self, ranges: Vec<Range<usize>>, record_history: bool) {
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let text = self.snapshot().text().to_string();
        let selections = ranges
            .into_iter()
            .enumerate()
            .map(|(id, range)| {
                let start = floor_char_boundary(&text, range.start);
                let end = floor_char_boundary(&text, range.end);
                Selection::from_anchor_head(id, start.min(end), start.max(end))
            })
            .collect();
        self.set_selections(normalize_new_selections(selections));
        if record_history {
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
        }
    }

    pub fn select_anchor_head(&mut self, anchor: usize, head: usize) {
        self.select_anchor_heads(vec![(anchor, head)]);
    }

    pub fn select_anchor_heads(&mut self, anchor_heads: Vec<(usize, usize)>) {
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let text = self.snapshot().text().to_string();
        let selections = anchor_heads
            .into_iter()
            .enumerate()
            .map(|(id, (anchor, head))| {
                Selection::from_anchor_head(
                    id,
                    floor_char_boundary(&text, anchor),
                    floor_char_boundary(&text, head),
                )
            })
            .collect();
        self.set_selections(normalize_new_selections(selections));
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
    }

    pub fn select_all(&mut self) {
        let len = self.snapshot().text().len();
        self.select(0..len);
    }

    pub fn collapse_selections_to_heads(&mut self) {
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let active_head = undo_selections
            .get(self.active_selection_index)
            .map(Selection::head);

        let selections = undo_selections
            .iter()
            .map(|selection| {
                let mut caret = Selection::caret(selection.head());
                caret.id = selection.id;
                caret
            })
            .collect();
        let normalized = normalize_new_selections(selections);
        let active_selection_index = active_head
            .and_then(|head| {
                normalized
                    .iter()
                    .position(|selection| selection.is_empty() && selection.head() == head)
            })
            .unwrap_or_else(|| undo_active_selection_index.min(normalized.len().saturating_sub(1)));

        self.set_selections_with_active_index(normalized, active_selection_index);
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
    }

    pub fn refresh_buffer_ranges(&mut self) {
        self.buffer.refresh();
        let text = self.snapshot().text().to_string();
        for selection in &mut self.selections {
            selection.clamp_to_text(&text);
        }
        self.reattach_selection_anchors();
    }

    fn set_selections(&mut self, selections: Vec<Selection>) {
        let active_selection_index = selections.len().saturating_sub(1);
        self.set_selections_with_active_index(selections, active_selection_index);
    }

    pub(crate) fn set_selections_with_active_index(
        &mut self,
        mut selections: Vec<Selection>,
        active_selection_index: usize,
    ) {
        if selections.is_empty() {
            selections.push(Selection::caret(0));
        }

        for selection in &mut selections {
            attach_selection_anchors(&mut self.buffer, selection);
        }
        self.active_selection_index = active_selection_index.min(selections.len() - 1);
        self.selections = selections;
    }

    pub(crate) fn set_active_selection(&mut self, mut selection: Selection) -> Result<(), String> {
        let active_selection = self
            .selections
            .get(self.active_selection_index)
            .ok_or_else(|| "editor has no active selection".to_string())?;
        selection.id = active_selection.id;
        attach_selection_anchors(&mut self.buffer, &mut selection);
        self.selections[self.active_selection_index] = selection;
        Ok(())
    }

    pub(crate) fn active_selection(&self) -> Result<Selection, String> {
        self.selections
            .get(self.active_selection_index)
            .map(|selection| resolve_selection_from_anchors(&self.buffer, selection))
            .ok_or_else(|| "editor has no active selection".to_string())
    }

    pub(crate) fn sync_selections_to_anchors(&mut self) {
        let buffer = &self.buffer;
        for selection in &mut self.selections {
            *selection = resolve_selection_from_anchors(buffer, selection);
        }
    }

    pub(crate) fn reattach_selection_anchors(&mut self) {
        let buffer = &mut self.buffer;
        for selection in &mut self.selections {
            attach_selection_anchors(buffer, selection);
        }
    }

    pub(crate) fn push_selection_history(
        &mut self,
        undo: Vec<Selection>,
        undo_active_selection_index: usize,
        redo: Vec<Selection>,
        redo_active_selection_index: usize,
    ) {
        self.selection_undo_stack.push(SelectionHistoryEntry {
            undo,
            undo_active_selection_index,
            redo,
            redo_active_selection_index,
        });
        self.selection_redo_stack.clear();
    }

    pub(crate) fn push_selection_only_history_from_current(
        &mut self,
        undo: Vec<Selection>,
        undo_active_selection_index: usize,
    ) {
        let redo = self.resolved_selections();
        let redo_active_selection_index = self.active_selection_index;
        if selection_history_key(&undo, undo_active_selection_index)
            == selection_history_key(&redo, redo_active_selection_index)
        {
            return;
        }

        self.selection_only_undo_stack.push(SelectionHistoryEntry {
            undo,
            undo_active_selection_index,
            redo,
            redo_active_selection_index,
        });
        self.selection_only_redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use crate::EditorModel;
    use language::Buffer;
    use text::BufferId;

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
