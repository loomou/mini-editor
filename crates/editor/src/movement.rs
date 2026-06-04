use crate::model::EditorModel;
use crate::selection::{Selection, SelectionGoal};
use crate::utils::{
    next_char_boundary, next_word_boundary, previous_char_boundary, previous_word_boundary,
};
use display::{DisplayPoint, DisplaySnapshot};

impl EditorModel {
    pub fn cursor_offset(&self) -> Result<usize, String> {
        Ok(self.active_selection()?.head())
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

    pub fn cursor_display_point_in(
        &self,
        display: &DisplaySnapshot,
    ) -> Result<DisplayPoint, String> {
        let cursor = self.cursor_offset()?;
        Ok(display.display_point_for_source_offset(cursor))
    }

    pub fn cursor_display_points(&self, soft_wrap_column: Option<usize>) -> Vec<DisplayPoint> {
        let display = self.display_snapshot(soft_wrap_column);
        self.cursor_display_points_in(&display)
    }

    pub fn cursor_display_points_in(&self, display: &DisplaySnapshot) -> Vec<DisplayPoint> {
        self.resolved_selections()
            .into_iter()
            .map(|selection| display.display_point_for_source_offset(selection.head()))
            .collect()
    }

    pub fn move_left(&mut self, extend: bool) -> Result<(), String> {
        let selection = self.active_selection()?;

        if !extend && !selection.is_empty() {
            let undo_selections = self.resolved_selections();
            let undo_active_selection_index = self.active_selection_index;
            self.set_active_selection(Selection::caret(selection.start))?;
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
            return Ok(());
        }

        let text = self.snapshot().text().to_string();
        let target = previous_char_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    pub fn move_right(&mut self, extend: bool) -> Result<(), String> {
        let selection = self.active_selection()?;

        if !extend && !selection.is_empty() {
            let undo_selections = self.resolved_selections();
            let undo_active_selection_index = self.active_selection_index;
            self.set_active_selection(Selection::caret(selection.end))?;
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
            return Ok(());
        }

        let text = self.snapshot().text().to_string();
        let target = next_char_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    pub fn move_up(&mut self, extend: bool, soft_wrap_column: Option<usize>) -> Result<(), String> {
        self.move_vertical(-1, extend, soft_wrap_column)
    }

    pub fn move_down(
        &mut self,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        self.move_vertical(1, extend, soft_wrap_column)
    }

    pub fn move_display_rows(
        &mut self,
        row_delta: isize,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        self.move_vertical(row_delta, extend, soft_wrap_column)
    }

    pub fn move_to_line_start(
        &mut self,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        let selection = self.active_selection()?;
        let display = self.display_snapshot(soft_wrap_column);
        let point = display.display_point_for_source_offset(selection.head());
        let target = display
            .rows()
            .get(point.row)
            .map(|row| row.source_range.start)
            .unwrap_or(0);
        self.move_active_head(target, extend)
    }

    pub fn move_to_line_end(
        &mut self,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        let selection = self.active_selection()?;
        let display = self.display_snapshot(soft_wrap_column);
        let point = display.display_point_for_source_offset(selection.head());
        let target = display
            .rows()
            .get(point.row)
            .map(|row| row.source_range.end)
            .unwrap_or(display.source_len());
        self.move_active_head(target, extend)
    }

    pub fn move_to_document_start(&mut self, extend: bool) -> Result<(), String> {
        self.move_active_head(0, extend)
    }

    pub fn move_to_document_end(&mut self, extend: bool) -> Result<(), String> {
        self.move_active_head(self.snapshot().text().len(), extend)
    }

    pub fn move_to_previous_word(&mut self, extend: bool) -> Result<(), String> {
        let selection = self.active_selection()?;
        let text = self.snapshot().text().to_string();
        let target = previous_word_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    pub fn move_to_next_word(&mut self, extend: bool) -> Result<(), String> {
        let selection = self.active_selection()?;
        let text = self.snapshot().text().to_string();
        let target = next_word_boundary(&text, selection.head());
        self.move_active_head(target, extend)
    }

    fn move_active_head(&mut self, target: usize, extend: bool) -> Result<(), String> {
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let mut selection = self.active_selection()?;
        if extend {
            selection.set_head(target);
        } else {
            selection.collapse_to(target);
        }
        self.set_active_selection(selection)?;
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
        Ok(())
    }

    fn move_vertical(
        &mut self,
        row_delta: isize,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        let selection = self.active_selection()?;
        let display = self.display_snapshot(soft_wrap_column);
        let desired_column = match selection.goal {
            SelectionGoal::Column(column) => column,
            SelectionGoal::None => {
                display
                    .display_point_for_source_offset(selection.head())
                    .column
            }
        };
        let target = display.source_offset_for_vertical_movement(
            selection.head(),
            row_delta,
            desired_column,
        );
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let mut selection = selection;
        if extend {
            selection.set_head(target);
        } else {
            selection.collapse_to(target);
        }
        selection.goal = SelectionGoal::Column(desired_column);
        self.set_active_selection(selection)?;
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
    use display::DisplayPoint;
    use language::Buffer;
    use text::BufferId;

    #[test]
    fn movement_updates_active_selection_without_dropping_other_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        editor.set_active_selection_index(0).unwrap();
        editor.move_right(false).unwrap();

        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![3..3, 8..13]
        );
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
    fn vertical_movement_preserves_column_goal() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(3..3);
        editor.move_down(false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 7..7);

        editor.move_down(false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 11..11);
    }

    #[test]
    fn vertical_movement_preserves_column_goal_through_empty_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(3..3);
        editor.move_down(false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 5..5);

        editor.move_down(false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 9..9);
    }

    #[test]
    fn vertical_movement_extends_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..1);
        editor.move_down(true, None).unwrap();

        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 1..5);
        assert!(!selection.reversed);
    }

    #[test]
    fn vertical_movement_extends_selection_through_empty_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(3..3);
        editor.move_down(true, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 3..5);

        editor.move_down(true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 3..9);
        assert!(!selection.reversed);
    }

    #[test]
    fn vertical_movement_extends_reversed_selection_through_empty_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(9..9);
        editor.move_up(true, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 5..9);

        editor.move_up(true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 3..9);
        assert!(selection.reversed);
    }

    #[test]
    fn line_boundary_movement_uses_display_rows() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(4..4);
        editor.move_to_line_start(false, Some(3)).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 3);

        editor.select(4..4);
        editor.move_to_line_end(true, Some(3)).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 4..6);
        assert_eq!(selection.head(), 6);
    }

    #[test]
    fn document_boundary_movement_can_extend_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(5..5);
        editor.move_to_document_start(true).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 0..5);
        assert!(editor.resolved_selections()[0].reversed);

        editor.move_to_document_end(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 7);
    }

    #[test]
    fn word_boundary_movement_skips_punctuation_and_respects_utf8() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one, two_é three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(0..0);
        editor.move_to_next_word(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 5);

        editor.move_to_next_word(false).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 12);

        editor.move_to_previous_word(true).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 5..12);
        assert!(selection.reversed);
    }

    #[test]
    fn selection_only_undo_restores_keyboard_movement_and_shift_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.move_right(false).unwrap();
        editor.move_right(true).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 1..2);
        assert_eq!(editor.resolved_selections()[0].head(), 2);

        assert!(editor.undo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 1..1);
        assert_eq!(editor.resolved_selections()[0].head(), 1);

        assert!(editor.redo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 1..2);
        assert_eq!(editor.resolved_selections()[0].head(), 2);
    }

    #[test]
    fn selection_only_undo_groups_multi_row_display_movement() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.move_display_rows(2, false, None).unwrap();
        assert_eq!(editor.cursor_offset().unwrap(), 4);

        assert!(editor.undo_selection());
        assert_eq!(editor.cursor_offset().unwrap(), 0);
        assert!(!editor.undo_selection());

        assert!(editor.redo_selection());
        assert_eq!(editor.cursor_offset().unwrap(), 4);
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
    fn cursor_display_points_include_all_selection_heads() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_ranges(vec![1..1, 4..4]);

        assert_eq!(
            editor.cursor_display_points(Some(3)),
            vec![
                DisplayPoint { row: 0, column: 1 },
                DisplayPoint { row: 1, column: 1 },
            ]
        );
    }
}
