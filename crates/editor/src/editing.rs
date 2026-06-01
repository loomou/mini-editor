use crate::model::EditorModel;
use crate::ranges::{
    SelectionEditRange, sorted_non_overlapping_edit_ranges, sorted_non_overlapping_selections,
};
use crate::selection::Selection;
use crate::utils::{floor_char_boundary, next_char_boundary, previous_char_boundary};
use multibuffer::MultiBufferEdit;
use std::rc::Rc;

impl EditorModel {
    pub fn selected_text(&self) -> String {
        let text = self.snapshot().text().to_string();
        self.resolved_selections()
            .into_iter()
            .filter(|selection| !selection.is_empty())
            .filter_map(|selection| text.get(selection.range()).map(ToString::to_string))
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn insert_text(&mut self, text: impl Into<String>) -> Result<(), String> {
        let replacement: Rc<str> = text.into().into();
        let selection_count = self.resolved_selections().len();
        self.insert_rc_texts(vec![replacement; selection_count])
    }

    pub fn insert_texts(&mut self, replacements: Vec<String>) -> Result<(), String> {
        let replacements = replacements
            .into_iter()
            .map(Rc::<str>::from)
            .collect::<Vec<_>>();
        self.insert_rc_texts(replacements)
    }

    fn insert_rc_texts(&mut self, replacements: Vec<Rc<str>>) -> Result<(), String> {
        let selections = self.resolved_selections();
        if replacements.len() != selections.len() {
            return Err(format!(
                "replacement count {} does not match selection count {}",
                replacements.len(),
                selections.len()
            ));
        }
        let undo_selections = selections.clone();
        let undo_active_selection_index = self.active_selection_index;
        let sorted_selections = sorted_non_overlapping_selections(&selections)?;
        let mut next_selections = selections;
        let mut delta = 0isize;

        for selection in &sorted_selections {
            let replacement = replacements
                .get(selection.selection_index)
                .ok_or_else(|| "missing replacement for selection".to_string())?;
            let replacement_len = isize::try_from(replacement.len())
                .map_err(|_| "replacement text is too large".to_string())?;
            let start = selection
                .start
                .checked_add_signed(delta)
                .ok_or_else(|| "selection offset overflowed while inserting text".to_string())?;
            let cursor = start
                .checked_add(replacement.len())
                .ok_or_else(|| "cursor offset overflowed while inserting text".to_string())?;
            let mut caret = Selection::caret(cursor);
            caret.id = selection.id;
            next_selections[selection.selection_index] = caret;

            let range_len = isize::try_from(selection.range.len())
                .map_err(|_| "selection range is too large".to_string())?;
            delta = delta
                .checked_add(replacement_len - range_len)
                .ok_or_else(|| {
                    "selection offset delta overflowed while inserting text".to_string()
                })?;
        }

        self.buffer.edit_group(
            sorted_selections
                .iter()
                .rev()
                .map(|selection| MultiBufferEdit {
                    range: selection.range.clone(),
                    replacement: replacements[selection.selection_index].clone(),
                })
                .collect(),
        )?;

        let redo_selections = next_selections.clone();
        let redo_active_selection_index = self.active_selection_index;
        self.set_selections_with_active_index(next_selections, self.active_selection_index);
        self.push_selection_history(
            undo_selections,
            undo_active_selection_index,
            redo_selections,
            redo_active_selection_index,
        );
        Ok(())
    }

    pub fn insert_char(&mut self, character: char) -> Result<(), String> {
        self.insert_text(character.to_string())
    }

    pub fn backspace(&mut self) -> Result<bool, String> {
        let text = self.snapshot().text().to_string();
        let edit_ranges = self
            .resolved_selections()
            .into_iter()
            .enumerate()
            .map(|(selection_index, selection)| {
                let range = if selection.is_empty() {
                    let cursor = floor_char_boundary(&text, selection.head());
                    previous_char_boundary(&text, cursor)..cursor
                } else {
                    selection.range()
                };
                SelectionEditRange {
                    selection_index,
                    selection,
                    range,
                }
            })
            .collect();
        self.delete_selection_ranges(edit_ranges)
    }

    pub fn delete(&mut self) -> Result<bool, String> {
        let text = self.snapshot().text().to_string();
        let edit_ranges = self
            .resolved_selections()
            .into_iter()
            .enumerate()
            .map(|(selection_index, selection)| {
                let range = if selection.is_empty() {
                    let cursor = floor_char_boundary(&text, selection.head());
                    cursor..next_char_boundary(&text, cursor)
                } else {
                    selection.range()
                };
                SelectionEditRange {
                    selection_index,
                    selection,
                    range,
                }
            })
            .collect();
        self.delete_selection_ranges(edit_ranges)
    }

    pub fn undo(&mut self) -> Result<bool, String> {
        let changed = self.buffer.undo()?;
        if changed {
            if let Some(history_entry) = self.selection_undo_stack.pop() {
                let selections = history_entry.undo.clone();
                let active_selection_index = history_entry.undo_active_selection_index;
                self.set_selections_with_active_index(selections, active_selection_index);
                self.selection_redo_stack.push(history_entry);
            } else {
                self.sync_selections_to_anchors();
            }
        }
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, String> {
        let changed = self.buffer.redo()?;
        if changed {
            if let Some(history_entry) = self.selection_redo_stack.pop() {
                let selections = history_entry.redo.clone();
                let active_selection_index = history_entry.redo_active_selection_index;
                self.set_selections_with_active_index(selections, active_selection_index);
                self.selection_undo_stack.push(history_entry);
            } else {
                self.sync_selections_to_anchors();
            }
        }
        Ok(changed)
    }

    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    fn delete_selection_ranges(
        &mut self,
        edit_ranges: Vec<SelectionEditRange>,
    ) -> Result<bool, String> {
        if !edit_ranges
            .iter()
            .any(|edit_range| !edit_range.range.is_empty())
        {
            return Ok(false);
        }

        let sorted_edit_ranges = sorted_non_overlapping_edit_ranges(&edit_ranges)?;
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let mut next_selections = edit_ranges
            .into_iter()
            .map(|edit_range| edit_range.selection)
            .collect::<Vec<_>>();
        let mut delta = 0isize;

        for edit_range in &sorted_edit_ranges {
            let cursor = edit_range
                .range
                .start
                .checked_add_signed(delta)
                .ok_or_else(|| "selection offset overflowed while deleting text".to_string())?;
            let mut caret = Selection::caret(cursor);
            caret.id = edit_range.selection.id;
            next_selections[edit_range.selection_index] = caret;

            let range_len = isize::try_from(edit_range.range.len())
                .map_err(|_| "selection range is too large".to_string())?;
            delta = delta.checked_sub(range_len).ok_or_else(|| {
                "selection offset delta overflowed while deleting text".to_string()
            })?;
        }

        self.buffer.edit_group(
            sorted_edit_ranges
                .iter()
                .rev()
                .filter(|edit_range| !edit_range.range.is_empty())
                .map(|edit_range| MultiBufferEdit {
                    range: edit_range.range.clone(),
                    replacement: Rc::<str>::from(""),
                })
                .collect(),
        )?;

        let redo_selections = next_selections.clone();
        let redo_active_selection_index = self.active_selection_index;
        self.set_selections_with_active_index(next_selections, self.active_selection_index);
        self.push_selection_history(
            undo_selections,
            undo_active_selection_index,
            redo_selections,
            redo_active_selection_index,
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
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
    fn insert_text_clears_vertical_column_goal() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(3..3);
        editor.move_down(false, None).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 7);

        editor.insert_text("\n").unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 8);

        editor.move_down(false, None).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 9);
    }

    #[test]
    fn select_all_and_selected_text_use_current_buffer() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello\nworld");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_all();

        assert_eq!(editor.resolved_selections()[0].range(), 0..11);
        assert_eq!(editor.selected_text(), "hello\nworld");
    }

    #[test]
    fn select_all_matches_uses_active_selected_text() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "aba ab aba");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..3);

        assert!(editor.select_all_matches());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..3, 8..10]
        );
        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(editor.selected_text(), "baba");
    }

    #[test]
    fn movement_uses_resolved_selection_after_buffer_edits() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.move_right(false).unwrap();

        assert_eq!(editor.snapshot().text(), "say hello world");
        assert_eq!(editor.cursor_offset().unwrap(), 11);

        let buffer = Buffer::local(BufferId::new(2).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..11);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.move_left(false).unwrap();

        assert_eq!(editor.cursor_offset().unwrap(), 10);
        assert_eq!(editor.selections()[0].range(), 10..10);
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
    fn selection_offsets_sync_from_tracked_anchors_after_buffer_edits() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.sync_selections_to_anchors();

        assert_eq!(editor.snapshot().text(), "say hello world");
        assert_eq!(editor.cursor_offset().unwrap(), 10);
    }

    #[test]
    fn insertion_uses_resolved_selection_after_buffer_edits() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..11);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.insert_text("zed").unwrap();

        assert_eq!(editor.snapshot().text(), "say hello zed");
        assert_eq!(editor.cursor_offset().unwrap(), 13);
    }

    #[test]
    fn insertion_replaces_all_non_overlapping_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        editor.insert_text("x").unwrap();

        assert_eq!(editor.snapshot().text(), "x two x");
        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..1, 7..7]
        );
        assert_eq!(editor.cursor_offset().unwrap(), 7);
    }

    #[test]
    fn undo_and_redo_restore_batch_insert_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);
        editor.set_active_selection_index(0).unwrap();

        editor.insert_text("x").unwrap();
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..1, 7..7]
        );

        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().text(), "one two three");
        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..13]
        );

        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().text(), "x two x");
        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..1, 7..7]
        );
    }

    #[test]
    fn insertion_can_use_one_replacement_per_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        editor
            .insert_texts(vec!["alpha".to_string(), "omega".to_string()])
            .unwrap();

        assert_eq!(editor.snapshot().text(), "alpha two omega");
        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![5..5, 15..15]
        );
        assert_eq!(editor.cursor_offset().unwrap(), 15);
    }

    #[test]
    fn insertion_rejects_replacement_count_mismatch() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        let error = editor.insert_texts(vec!["alpha".to_string()]).unwrap_err();

        assert!(error.contains("replacement count"));
        assert_eq!(editor.snapshot().text(), "one two three");
    }

    #[test]
    fn batch_insert_undoes_and_redoes_as_one_transaction() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        editor.insert_text("x").unwrap();
        assert_eq!(editor.snapshot().text(), "x two x");

        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().text(), "one two three");
        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().text(), "x two x");
    }

    #[test]
    fn insertion_rejects_overlapping_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.selections = vec![
            Selection::from_anchor_head(0, 1, 4),
            Selection::from_anchor_head(1, 3, 5),
        ];
        editor.active_selection_index = 1;
        editor.reattach_selection_anchors();

        let error = editor.insert_text("x").unwrap_err();

        assert!(error.contains("overlaps"));
        assert_eq!(editor.snapshot().text(), "abcdef");
    }

    #[test]
    fn delete_removes_all_non_overlapping_selection_ranges() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        assert!(editor.delete().unwrap());

        assert_eq!(editor.snapshot().text(), " two ");
        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 5..5]
        );
        assert_eq!(editor.cursor_offset().unwrap(), 5);
    }

    #[test]
    fn undo_and_redo_restore_batch_delete_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        assert!(editor.delete().unwrap());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 5..5]
        );

        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().text(), "one two three");
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..13]
        );

        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().text(), " two ");
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 5..5]
        );
    }

    #[test]
    fn batch_delete_undoes_and_redoes_as_one_transaction() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        assert!(editor.delete().unwrap());
        assert_eq!(editor.snapshot().text(), " two ");

        assert!(editor.undo().unwrap());
        assert_eq!(editor.snapshot().text(), "one two three");
        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().text(), " two ");
    }

    #[test]
    fn backspace_removes_previous_character_for_all_carets() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![1..1, 3..3]);

        assert!(editor.backspace().unwrap());

        assert_eq!(editor.snapshot().text(), "bd");
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 1..1]
        );
    }

    #[test]
    fn deletion_rejects_overlapping_selection_ranges() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.selections = vec![
            Selection::from_anchor_head(0, 1, 4),
            Selection::from_anchor_head(1, 3, 5),
        ];
        editor.active_selection_index = 1;
        editor.reattach_selection_anchors();

        let error = editor.delete().unwrap_err();

        assert!(error.contains("overlaps"));
        assert_eq!(editor.snapshot().text(), "abcdef");
    }

    #[test]
    fn deletion_uses_resolved_selection_after_buffer_edits() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();
        assert!(editor.backspace().unwrap());

        assert_eq!(editor.snapshot().text(), "say helloworld");
        assert_eq!(editor.cursor_offset().unwrap(), 9);

        let buffer = Buffer::local(BufferId::new(2).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();
        assert!(editor.delete().unwrap());

        assert_eq!(editor.snapshot().text(), "say hello orld");
        assert_eq!(editor.cursor_offset().unwrap(), 10);
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
}
