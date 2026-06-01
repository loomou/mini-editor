use crate::model::EditorModel;
use crate::selection::{Selection, SelectionGoal, normalize_new_selections};
use crate::utils::{floor_char_boundary, line_range_at_offset, word_range_at_offset};
use display::DisplayPoint;

impl EditorModel {
    pub fn select_display_rectangle(
        &mut self,
        anchor_row: usize,
        anchor_column: usize,
        head_row: usize,
        head_column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        self.select_display_rectangle_impl(
            anchor_row,
            anchor_column,
            head_row,
            head_column,
            soft_wrap_column,
            true,
        );
    }

    pub fn select_display_rectangle_transient(
        &mut self,
        anchor_row: usize,
        anchor_column: usize,
        head_row: usize,
        head_column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        self.select_display_rectangle_impl(
            anchor_row,
            anchor_column,
            head_row,
            head_column,
            soft_wrap_column,
            false,
        );
    }

    fn select_display_rectangle_impl(
        &mut self,
        anchor_row: usize,
        anchor_column: usize,
        head_row: usize,
        head_column: usize,
        soft_wrap_column: Option<usize>,
        record_history: bool,
    ) {
        let display = self.display_snapshot(soft_wrap_column);
        let row_start = anchor_row.min(head_row);
        let row_end = anchor_row.max(head_row);
        let column_start = anchor_column.min(head_column);
        let column_end = anchor_column.max(head_column);
        let ranges = display
            .rows()
            .iter()
            .filter(|row| row.row >= row_start && row.row <= row_end)
            .map(|row| {
                let start = display.source_offset_for_display_point(DisplayPoint {
                    row: row.row,
                    column: column_start,
                });
                let end = display.source_offset_for_display_point(DisplayPoint {
                    row: row.row,
                    column: column_end,
                });
                start..end
            })
            .collect();
        self.select_ranges_impl(ranges, record_history);
    }

    pub fn add_caret_at_display_point(
        &mut self,
        row: usize,
        column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        let display = self.display_snapshot(soft_wrap_column);
        let offset = display.source_offset_for_display_point(DisplayPoint { row, column });
        self.add_caret(offset);
    }

    pub fn add_caret_at_display_point_transient(
        &mut self,
        row: usize,
        column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        let display = self.display_snapshot(soft_wrap_column);
        let offset = display.source_offset_for_display_point(DisplayPoint { row, column });
        self.add_caret_impl(offset, false);
    }

    pub fn add_caret_above(&mut self, soft_wrap_column: Option<usize>) -> Result<(), String> {
        self.add_caret_vertically(-1, soft_wrap_column)
    }

    pub fn add_caret_below(&mut self, soft_wrap_column: Option<usize>) -> Result<(), String> {
        self.add_caret_vertically(1, soft_wrap_column)
    }

    pub fn add_caret(&mut self, offset: usize) {
        self.add_caret_impl(offset, true);
    }

    fn add_caret_impl(&mut self, offset: usize, record_history: bool) {
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let text = self.snapshot().text().to_string();
        let offset = floor_char_boundary(&text, offset);
        let mut selections = self.resolved_selections();
        selections.push(Selection::caret(offset));
        let normalized = normalize_new_selections(selections);
        let active_selection_index = normalized
            .iter()
            .position(|selection| selection.is_empty() && selection.head() == offset)
            .or_else(|| {
                normalized
                    .iter()
                    .position(|selection| selection.start <= offset && offset <= selection.end)
            })
            .unwrap_or_else(|| normalized.len().saturating_sub(1));
        self.set_selections_with_active_index(normalized, active_selection_index);
        if record_history {
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
        }
    }

    fn add_caret_vertically(
        &mut self,
        row_delta: isize,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        let selection = self.active_selection()?;
        let display = self.display_snapshot(soft_wrap_column);
        let point = display.display_point_for_source_offset(selection.head());
        let target =
            display.source_offset_for_vertical_movement(selection.head(), row_delta, point.column);
        self.add_caret(target);
        Ok(())
    }

    pub fn select_display_point(
        &mut self,
        row: usize,
        column: usize,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        self.select_display_point_impl(row, column, extend, soft_wrap_column, true)
    }

    pub fn select_display_point_transient(
        &mut self,
        row: usize,
        column: usize,
        extend: bool,
        soft_wrap_column: Option<usize>,
    ) -> Result<(), String> {
        self.select_display_point_impl(row, column, extend, soft_wrap_column, false)
    }

    fn select_display_point_impl(
        &mut self,
        row: usize,
        column: usize,
        extend: bool,
        soft_wrap_column: Option<usize>,
        record_history: bool,
    ) -> Result<(), String> {
        let display = self.display_snapshot(soft_wrap_column);
        let target = display.source_offset_for_display_point(DisplayPoint { row, column });
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let mut selection = self.active_selection()?;
        if extend {
            selection.set_head(target);
        } else {
            selection.collapse_to(target);
        }
        selection.goal = SelectionGoal::None;
        self.set_active_selection(selection)?;
        if record_history {
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
        }
        Ok(())
    }

    pub fn select_word_at_display_point(
        &mut self,
        row: usize,
        column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        let display = self.display_snapshot(soft_wrap_column);
        let text = self.snapshot().text().to_string();
        let offset = display.source_offset_for_display_point(DisplayPoint { row, column });
        let range = word_range_at_offset(&text, offset);
        self.select(range);
    }

    pub fn select_line_at_display_point(
        &mut self,
        row: usize,
        column: usize,
        soft_wrap_column: Option<usize>,
    ) {
        let display = self.display_snapshot(soft_wrap_column);
        let offset = display.source_offset_for_display_point(DisplayPoint { row, column });
        let text = self.snapshot().text().to_string();
        let range = line_range_at_offset(&text, offset);
        self.select(range);
    }
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
    use language::Buffer;
    use text::BufferId;

    #[test]
    fn select_display_point_moves_or_extends_active_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_point(1, 2, false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 6..6);

        editor.select_display_point(0, 1, true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 1..6);
        assert_eq!(selection.head(), 1);
        assert_eq!(selection.tail(), 6);
        assert!(selection.reversed);
    }

    #[test]
    fn select_display_point_extends_to_clamped_empty_and_short_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_point(0, 0, false, None).unwrap();
        editor.select_display_point(1, 8, true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 0..4);
        assert_eq!(selection.head(), 4);
        assert_eq!(selection.tail(), 0);
        assert!(!selection.reversed);

        editor.select_display_point(0, 0, false, None).unwrap();
        editor.select_display_point(2, 8, true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 0..7);
        assert_eq!(selection.head(), 7);
        assert_eq!(selection.tail(), 0);
        assert!(!selection.reversed);
    }

    #[test]
    fn select_display_point_extends_reversed_to_clamped_empty_and_short_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy\nzz");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_point(3, 2, false, None).unwrap();
        editor.select_display_point(1, 8, true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 4..10);
        assert_eq!(selection.head(), 4);
        assert_eq!(selection.tail(), 10);
        assert!(selection.reversed);

        editor.select_display_point(3, 2, false, None).unwrap();
        editor.select_display_point(2, 8, true, None).unwrap();
        let selection = &editor.resolved_selections()[0];
        assert_eq!(selection.range(), 7..10);
        assert_eq!(selection.head(), 7);
        assert_eq!(selection.tail(), 10);
        assert!(selection.reversed);
    }

    #[test]
    fn select_display_rectangle_selects_ranges_per_display_row() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_rectangle(0, 1, 2, 3, None);

        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..3, 6..7, 9..11]
        );
        assert_eq!(editor.active_selection_index(), 2);
        assert_eq!(editor.selected_text(), "bcfhi");
    }

    #[test]
    fn select_display_rectangle_clamps_empty_and_short_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_rectangle(0, 0, 2, 8, None);

        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 4..4, 5..7]
        );
        assert_eq!(editor.active_selection_index(), 2);
        assert_eq!(editor.selected_text(), "abcxy");
    }

    #[test]
    fn select_display_rectangle_clamps_reversed_empty_and_short_lines() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_rectangle(2, 8, 0, 0, None);

        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 4..4, 5..7]
        );
        assert_eq!(editor.active_selection_index(), 2);
        assert_eq!(editor.selected_text(), "abcxy");
    }

    #[test]
    fn add_caret_at_display_point_adds_or_activates_caret() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.add_caret_at_display_point(1, 2, None);

        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 6..6]
        );
        assert_eq!(editor.active_selection_index(), 1);

        editor.add_caret_at_display_point(0, 0, None);

        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..0, 6..6]
        );
        assert_eq!(editor.active_selection_index(), 0);
    }

    #[test]
    fn add_caret_above_and_below_use_current_display_column() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(6..6);
        editor.add_caret_below(None).unwrap();
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![6..6, 9..9]
        );
        assert_eq!(editor.active_selection_index(), 1);

        editor.add_caret_above(None).unwrap();
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![6..6, 9..9]
        );
        assert_eq!(editor.active_selection_index(), 0);
    }

    #[test]
    fn select_word_at_display_point_uses_word_boundaries() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one, two_é");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_word_at_display_point(0, 6, None);
        assert_eq!(editor.resolved_selections()[0].range(), 5..11);
        assert_eq!(editor.selected_text(), "two_é");

        editor.select_word_at_display_point(0, 3, None);
        assert_eq!(editor.resolved_selections()[0].range(), 3..4);
        assert_eq!(editor.selected_text(), ",");
    }

    #[test]
    fn select_line_at_display_point_selects_source_line_without_newline() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef\n");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_line_at_display_point(1, 1, None);
        assert_eq!(editor.resolved_selections()[0].range(), 4..7);
        assert_eq!(editor.selected_text(), "def");

        editor.select_line_at_display_point(2, 0, None);
        assert_eq!(editor.resolved_selections()[0].range(), 8..8);
    }
}
