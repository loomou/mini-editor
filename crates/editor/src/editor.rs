use display::{DisplayMap, DisplayPoint, DisplaySnapshot};
use language::BufferHandle;
use multibuffer::{MultiBuffer, MultiBufferAnchor, MultiBufferEdit, MultiBufferSnapshot};
use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionGoal {
    #[default]
    None,
    Column(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub id: usize,
    pub start: usize,
    pub end: usize,
    pub reversed: bool,
    pub goal: SelectionGoal,
    tail_anchor: Option<MultiBufferAnchor>,
    head_anchor: Option<MultiBufferAnchor>,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            id: 0,
            start: offset,
            end: offset,
            reversed: false,
            goal: SelectionGoal::None,
            tail_anchor: None,
            head_anchor: None,
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
                tail_anchor: None,
                head_anchor: None,
            }
        } else {
            Self {
                id,
                start: anchor,
                end: head,
                reversed: false,
                goal: SelectionGoal::None,
                tail_anchor: None,
                head_anchor: None,
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
        self.tail_anchor = None;
        self.head_anchor = None;
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

    fn set_anchor_handles(
        &mut self,
        tail_anchor: MultiBufferAnchor,
        head_anchor: MultiBufferAnchor,
    ) {
        self.tail_anchor = Some(tail_anchor);
        self.head_anchor = Some(head_anchor);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionHistoryCheckpoint {
    selections: Vec<Selection>,
    active_selection_index: usize,
}

#[derive(Debug)]
pub struct EditorModel {
    buffer: MultiBuffer,
    selections: Vec<Selection>,
    active_selection_index: usize,
    selection_undo_stack: Vec<SelectionHistoryEntry>,
    selection_redo_stack: Vec<SelectionHistoryEntry>,
    selection_only_undo_stack: Vec<SelectionHistoryEntry>,
    selection_only_redo_stack: Vec<SelectionHistoryEntry>,
}

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

    fn select_ranges_impl(&mut self, ranges: Vec<Range<usize>>, record_history: bool) {
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

    pub fn cursor_display_points(&self, soft_wrap_column: Option<usize>) -> Vec<DisplayPoint> {
        let display = self.display_snapshot(soft_wrap_column);
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

    pub fn select_all(&mut self) {
        let len = self.snapshot().text().len();
        self.select(0..len);
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

    pub fn select_next_match(&mut self) -> bool {
        let text = self.snapshot().text().to_string();
        let mut selections = self.resolved_selections();
        let Some(active_selection) = selections.get(self.active_selection_index).cloned() else {
            return false;
        };

        if active_selection.is_empty() {
            let range = word_range_at_offset(&text, active_selection.head());
            if range.is_empty() {
                return false;
            }
            self.select(range);
            return true;
        }

        let query = &text[active_selection.range()];
        if query.is_empty() {
            return false;
        }

        let undo_selections = selections.clone();
        let undo_active_selection_index = self.active_selection_index;
        let search_start = selections
            .iter()
            .map(|selection| selection.end)
            .max()
            .unwrap_or(active_selection.end);
        let Some(next_range) =
            find_next_non_overlapping_match(&text, query, search_start, &selections)
        else {
            return false;
        };

        selections.push(Selection::from_anchor_head(
            selections.len(),
            next_range.start,
            next_range.end,
        ));
        let normalized = normalize_new_selections(selections);
        let active_selection_index = normalized
            .iter()
            .position(|selection| {
                selection.start == next_range.start && selection.end == next_range.end
            })
            .unwrap_or_else(|| normalized.len().saturating_sub(1));
        self.set_selections_with_active_index(normalized, active_selection_index);
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
        true
    }

    pub fn select_all_matches(&mut self) -> bool {
        let text = self.snapshot().text().to_string();
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let Some(active_selection) = self
            .resolved_selections()
            .get(self.active_selection_index)
            .cloned()
        else {
            return false;
        };
        let range = if active_selection.is_empty() {
            word_range_at_offset(&text, active_selection.head())
        } else {
            active_selection.range()
        };
        if range.is_empty() {
            return false;
        }

        let query = &text[range.clone()];
        let matches = find_all_non_overlapping_matches(&text, query);
        if matches.is_empty() {
            return false;
        }

        let selections = matches
            .into_iter()
            .enumerate()
            .map(|(id, range)| Selection::from_anchor_head(id, range.start, range.end))
            .collect::<Vec<_>>();
        let active_selection_index = selections
            .iter()
            .position(|selection| selection.start == range.start && selection.end == range.end)
            .unwrap_or_else(|| selections.len().saturating_sub(1));
        self.set_selections_with_active_index(selections, active_selection_index);
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
        true
    }

    pub fn skip_active_match(&mut self) -> bool {
        let text = self.snapshot().text().to_string();
        let undo_selections = self.resolved_selections();
        let undo_active_selection_index = self.active_selection_index;
        let mut selections = self.resolved_selections();
        let Some(active_selection) = selections.get(self.active_selection_index).cloned() else {
            return false;
        };
        if active_selection.is_empty() {
            return false;
        }

        let query = &text[active_selection.range()];
        if query.is_empty() {
            return false;
        }

        selections.remove(self.active_selection_index);
        let mut blocked_matches = selections.clone();
        blocked_matches.push(active_selection.clone());
        if let Some(next_range) =
            find_next_non_overlapping_match(&text, query, active_selection.end, &blocked_matches)
        {
            selections.push(Selection::from_anchor_head(
                selections.len(),
                next_range.start,
                next_range.end,
            ));
            let normalized = normalize_new_selections(selections);
            let active_selection_index = normalized
                .iter()
                .position(|selection| {
                    selection.start == next_range.start && selection.end == next_range.end
                })
                .unwrap_or_else(|| normalized.len().saturating_sub(1));
            self.set_selections_with_active_index(normalized, active_selection_index);
            self.push_selection_only_history_from_current(
                undo_selections,
                undo_active_selection_index,
            );
            return true;
        }

        if selections.is_empty() {
            self.select(active_selection.head()..active_selection.head());
            return true;
        }

        let active_selection_index = selections
            .iter()
            .position(|selection| selection.start >= active_selection.end)
            .unwrap_or_else(|| selections.len().saturating_sub(1));
        self.set_selections_with_active_index(selections, active_selection_index);
        self.push_selection_only_history_from_current(undo_selections, undo_active_selection_index);
        true
    }

    pub fn undo_selection(&mut self) -> bool {
        let Some(history_entry) = self.selection_only_undo_stack.pop() else {
            return false;
        };
        let selections = history_entry.undo.clone();
        let active_selection_index = history_entry.undo_active_selection_index;
        self.set_selections_with_active_index(selections, active_selection_index);
        self.selection_only_redo_stack.push(history_entry);
        true
    }

    pub fn redo_selection(&mut self) -> bool {
        let Some(history_entry) = self.selection_only_redo_stack.pop() else {
            return false;
        };
        let selections = history_entry.redo.clone();
        let active_selection_index = history_entry.redo_active_selection_index;
        self.set_selections_with_active_index(selections, active_selection_index);
        self.selection_only_undo_stack.push(history_entry);
        true
    }

    pub fn selected_text(&self) -> String {
        let text = self.snapshot().text().to_string();
        self.resolved_selections()
            .into_iter()
            .filter(|selection| !selection.is_empty())
            .filter_map(|selection| text.get(selection.range()).map(ToString::to_string))
            .collect::<Vec<_>>()
            .join("")
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

    fn set_selections_with_active_index(
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

    fn set_active_selection(&mut self, mut selection: Selection) -> Result<(), String> {
        let active_selection = self
            .selections
            .get(self.active_selection_index)
            .ok_or_else(|| "editor has no active selection".to_string())?;
        selection.id = active_selection.id;
        attach_selection_anchors(&mut self.buffer, &mut selection);
        self.selections[self.active_selection_index] = selection;
        Ok(())
    }

    fn active_selection(&self) -> Result<Selection, String> {
        self.selections
            .get(self.active_selection_index)
            .map(|selection| resolve_selection_from_anchors(&self.buffer, selection))
            .ok_or_else(|| "editor has no active selection".to_string())
    }

    fn sync_selections_to_anchors(&mut self) {
        let buffer = &self.buffer;
        for selection in &mut self.selections {
            *selection = resolve_selection_from_anchors(buffer, selection);
        }
    }

    fn reattach_selection_anchors(&mut self) {
        let buffer = &mut self.buffer;
        for selection in &mut self.selections {
            attach_selection_anchors(buffer, selection);
        }
    }

    fn push_selection_history(
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

    fn push_selection_only_history_from_current(
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

#[derive(Clone, Debug)]
struct SelectionEditRange {
    selection_index: usize,
    selection: Selection,
    range: Range<usize>,
}

#[derive(Clone, Debug)]
struct SelectionHistoryEntry {
    undo: Vec<Selection>,
    undo_active_selection_index: usize,
    redo: Vec<Selection>,
    redo_active_selection_index: usize,
}

fn normalize_new_selections(mut selections: Vec<Selection>) -> Vec<Selection> {
    selections.sort_by_key(|selection| (selection.start, selection.end));

    let mut normalized: Vec<Selection> = Vec::new();
    for selection in selections {
        let Some(last) = normalized.last_mut() else {
            normalized.push(selection);
            continue;
        };

        if selections_overlap_or_duplicate(last, &selection) {
            let start = last.start.min(selection.start);
            let end = last.end.max(selection.end);
            *last = Selection::from_anchor_head(last.id, start, end);
        } else {
            normalized.push(selection);
        }
    }

    for (id, selection) in normalized.iter_mut().enumerate() {
        selection.id = id;
    }

    normalized
}

fn selections_overlap_or_duplicate(left: &Selection, right: &Selection) -> bool {
    if left.is_empty() && right.is_empty() {
        return left.start == right.start;
    }

    left.start < right.end && right.start < left.end
}

fn selection_history_key(
    selections: &[Selection],
    active_selection_index: usize,
) -> Vec<(usize, usize, usize, bool)> {
    let mut key = selections
        .iter()
        .map(|selection| {
            (
                selection.id,
                selection.start,
                selection.end,
                selection.reversed,
            )
        })
        .collect::<Vec<_>>();
    key.push((usize::MAX, active_selection_index, 0, false));
    key
}

#[derive(Clone, Debug)]
struct SortedSelection {
    selection_index: usize,
    start: usize,
    end: usize,
    id: usize,
    range: Range<usize>,
}

fn sorted_non_overlapping_selections(
    selections: &[Selection],
) -> Result<Vec<SortedSelection>, String> {
    let mut sorted_selections = selections
        .iter()
        .enumerate()
        .map(|(selection_index, selection)| SortedSelection {
            selection_index,
            start: selection.start,
            end: selection.end,
            id: selection.id,
            range: selection.range(),
        })
        .collect::<Vec<_>>();
    sorted_selections.sort_by_key(|selection| (selection.start, selection.end));

    for window in sorted_selections.windows(2) {
        let previous = &window[0];
        let current = &window[1];
        if previous.end > current.start {
            return Err(format!(
                "selection {} overlaps selection {}",
                previous.id, current.id
            ));
        }
    }

    Ok(sorted_selections)
}

fn sorted_non_overlapping_edit_ranges(
    edit_ranges: &[SelectionEditRange],
) -> Result<Vec<SelectionEditRange>, String> {
    let mut sorted_edit_ranges = edit_ranges.to_vec();
    sorted_edit_ranges.sort_by_key(|edit_range| (edit_range.range.start, edit_range.range.end));

    for window in sorted_edit_ranges.windows(2) {
        let previous = &window[0];
        let current = &window[1];
        if previous.range.end > current.range.start {
            return Err(format!(
                "selection {} overlaps selection {}",
                previous.selection.id, current.selection.id
            ));
        }
    }

    Ok(sorted_edit_ranges)
}

fn resolve_selection_from_anchors(buffer: &MultiBuffer, selection: &Selection) -> Selection {
    let Some(tail_anchor) = selection.tail_anchor else {
        return selection.clone();
    };
    let Some(head_anchor) = selection.head_anchor else {
        return selection.clone();
    };
    let Some(tail) = buffer.offset_for_tracked_anchor(tail_anchor) else {
        return selection.clone();
    };
    let Some(head) = buffer.offset_for_tracked_anchor(head_anchor) else {
        return selection.clone();
    };

    let mut resolved = Selection::from_anchor_head(selection.id, tail, head);
    resolved.goal = selection.goal;
    resolved.set_anchor_handles(tail_anchor, head_anchor);
    resolved
}

fn attach_selection_anchors(buffer: &mut MultiBuffer, selection: &mut Selection) {
    let tail = selection.tail();
    let head = selection.head();
    let tail_anchor = if selection.is_empty() {
        buffer.track_anchor_after(tail)
    } else if selection.reversed {
        buffer.track_anchor_after(tail)
    } else {
        buffer.track_anchor_before(tail)
    };
    let head_anchor = if selection.is_empty() || !selection.reversed {
        buffer.track_anchor_after(head)
    } else {
        buffer.track_anchor_before(head)
    };

    if let (Some(tail_anchor), Some(head_anchor)) = (tail_anchor, head_anchor) {
        selection.set_anchor_handles(tail_anchor, head_anchor);
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

fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let clipped = floor_char_boundary(text, offset);
    let chars = text[..clipped].char_indices().collect::<Vec<_>>();
    let Some((mut index, _)) = chars
        .len()
        .checked_sub(1)
        .map(|index| (index, chars[index].1))
    else {
        return 0;
    };

    if is_word_char(chars[index].1) {
        while index > 0 && is_word_char(chars[index - 1].1) {
            index -= 1;
        }
        return chars[index].0;
    }

    while index > 0 && !is_word_char(chars[index].1) {
        index -= 1;
    }
    if !is_word_char(chars[index].1) {
        return 0;
    }
    while index > 0 && is_word_char(chars[index - 1].1) {
        index -= 1;
    }
    chars[index].0
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = floor_char_boundary(text, offset);
    if clipped >= text.len() {
        return text.len();
    }

    while clipped < text.len() {
        let Some(character) = text[clipped..].chars().next() else {
            return text.len();
        };
        if !is_word_char(character) {
            break;
        }
        clipped += character.len_utf8();
    }

    while clipped < text.len() {
        let Some(character) = text[clipped..].chars().next() else {
            return text.len();
        };
        if is_word_char(character) {
            break;
        }
        clipped += character.len_utf8();
    }

    clipped
}

fn is_word_char(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn word_range_at_offset(text: &str, offset: usize) -> Range<usize> {
    let mut clipped = floor_char_boundary(text, offset);
    if clipped == text.len() && clipped > 0 {
        clipped = previous_char_boundary(text, clipped);
    }

    let Some(character) = text.get(clipped..).and_then(|text| text.chars().next()) else {
        return clipped..clipped;
    };
    if !is_word_char(character) {
        return clipped..next_char_boundary(text, clipped);
    }

    let mut start = clipped;
    while start > 0 {
        let previous = previous_char_boundary(text, start);
        let Some(character) = text
            .get(previous..start)
            .and_then(|text| text.chars().next())
        else {
            break;
        };
        if !is_word_char(character) {
            break;
        }
        start = previous;
    }

    let mut end = clipped;
    while end < text.len() {
        let Some(character) = text.get(end..).and_then(|text| text.chars().next()) else {
            break;
        };
        if !is_word_char(character) {
            break;
        }
        end += character.len_utf8();
    }

    start..end
}

fn line_range_at_offset(text: &str, offset: usize) -> Range<usize> {
    let clipped = floor_char_boundary(text, offset);
    let start = text[..clipped]
        .rfind('\n')
        .map(|offset| offset + 1)
        .unwrap_or(0);
    let end = text[clipped..]
        .find('\n')
        .map(|offset| clipped + offset)
        .unwrap_or(text.len());
    start..end
}

fn find_next_non_overlapping_match(
    text: &str,
    query: &str,
    search_start: usize,
    selections: &[Selection],
) -> Option<Range<usize>> {
    find_non_overlapping_match(text, query, search_start, text.len(), selections)
        .or_else(|| find_non_overlapping_match(text, query, 0, search_start, selections))
}

fn find_non_overlapping_match(
    text: &str,
    query: &str,
    start: usize,
    end: usize,
    selections: &[Selection],
) -> Option<Range<usize>> {
    if start >= end {
        return None;
    }

    let mut cursor = floor_char_boundary(text, start);
    let end = floor_char_boundary(text, end);
    while cursor < end {
        let haystack = &text[cursor..end];
        let Some(relative_offset) = haystack.find(query) else {
            return None;
        };
        let match_start = cursor + relative_offset;
        let match_end = match_start + query.len();
        let range = match_start..match_end;
        if !selections
            .iter()
            .any(|selection| selection.start == range.start && selection.end == range.end)
            && !selections
                .iter()
                .any(|selection| selection.start < range.end && range.start < selection.end)
        {
            return Some(range);
        }
        cursor = next_char_boundary(text, match_start);
    }

    None
}

fn find_all_non_overlapping_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let haystack = &text[cursor..];
        let Some(relative_offset) = haystack.find(query) else {
            break;
        };
        let match_start = cursor + relative_offset;
        let match_end = match_start + query.len();
        ranges.push(match_start..match_end);
        cursor = match_end;
    }
    ranges
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
    fn select_ranges_tracks_multiple_selections_independently() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        assert_eq!(
            editor
                .selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..13]
        );

        editor.buffer.edit(4..4, "big ").unwrap();

        let resolved = editor.resolved_selections();
        assert_eq!(
            resolved.iter().map(Selection::range).collect::<Vec<_>>(),
            vec![0..3, 12..17]
        );

        editor.sync_selections_to_anchors();
        assert_eq!(
            editor
                .selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 12..17]
        );
    }

    #[test]
    fn select_anchor_heads_preserves_multiple_selection_directions() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_heads(vec![(3, 0), (4, 7)]);

        let selections = editor.selections();
        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].range(), 0..3);
        assert_eq!(selections[0].head(), 0);
        assert_eq!(selections[0].tail(), 3);
        assert!(selections[0].reversed);
        assert_eq!(selections[1].range(), 4..7);
        assert_eq!(selections[1].head(), 7);
        assert_eq!(selections[1].tail(), 4);
        assert!(!selections[1].reversed);
    }

    #[test]
    fn select_ranges_normalizes_overlapping_and_duplicate_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdefghi");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![2..5, 0..3, 7..7, 7..7]);

        assert_eq!(
            editor
                .selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..5, 7..7]
        );
        assert_eq!(editor.active_selection_index(), 1);
    }

    #[test]
    fn select_anchor_heads_normalizes_overlaps_to_forward_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdefghi");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_heads(vec![(5, 1), (3, 8)]);

        let selections = editor.selections();
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].range(), 1..8);
        assert_eq!(selections[0].head(), 8);
        assert_eq!(selections[0].tail(), 1);
        assert!(!selections[0].reversed);
    }

    #[test]
    fn active_selection_index_controls_cursor_queries() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_ranges(vec![0..3, 8..13]);

        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(editor.cursor_offset().unwrap(), 13);

        editor.set_active_selection_index(0).unwrap();

        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(editor.cursor_offset().unwrap(), 3);
        assert!(editor.set_active_selection_index(2).is_err());
        assert_eq!(editor.active_selection_index(), 0);
    }

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
    fn select_all_and_selected_text_use_current_buffer() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello\nworld");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_all();

        assert_eq!(editor.resolved_selections()[0].range(), 0..11);
        assert_eq!(editor.selected_text(), "hello\nworld");
    }

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
    fn select_next_match_selects_current_word_then_adds_matches() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..1);
        assert!(editor.select_next_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3]
        );
        assert_eq!(editor.active_selection_index(), 0);

        assert!(editor.select_next_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11]
        );
        assert_eq!(editor.active_selection_index(), 1);

        assert!(editor.select_next_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11, 12..15]
        );
        assert_eq!(editor.active_selection_index(), 2);
        assert_eq!(editor.selected_text(), "foofoofoo");

        assert!(!editor.select_next_match());
    }

    #[test]
    fn select_next_match_wraps_to_earlier_non_overlapping_match() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_ranges(vec![8..11, 12..15]);

        assert!(editor.select_next_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11, 12..15]
        );
        assert_eq!(editor.active_selection_index(), 0);
    }

    #[test]
    fn select_all_matches_selects_every_current_word_match() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(9..9);

        assert!(editor.select_all_matches());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11, 12..15]
        );
        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(editor.selected_text(), "foofoofoo");
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
    fn skip_active_match_removes_active_and_adds_next_unselected_match() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo foo foo foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_ranges(vec![0..3, 4..7]);
        editor.set_active_selection_index(0).unwrap();

        assert!(editor.skip_active_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![4..7, 8..11]
        );
        assert_eq!(editor.active_selection_index(), 1);
        assert_eq!(editor.selected_text(), "foofoo");
    }

    #[test]
    fn skip_active_match_removes_active_when_no_unselected_match_remains() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_ranges(vec![0..3, 8..11]);
        editor.set_active_selection_index(1).unwrap();

        assert!(editor.skip_active_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3]
        );
        assert_eq!(editor.active_selection_index(), 0);
        assert_eq!(editor.selected_text(), "foo");
    }

    #[test]
    fn selection_only_undo_redo_restore_match_selection_changes() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..1);
        assert!(editor.select_next_match());
        assert!(editor.select_next_match());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11]
        );
        assert_eq!(editor.active_selection_index(), 1);

        assert!(editor.undo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3]
        );
        assert_eq!(editor.active_selection_index(), 0);

        assert!(editor.redo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 8..11]
        );
        assert_eq!(editor.active_selection_index(), 1);
    }

    #[test]
    fn selection_only_undo_does_not_change_text() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "foo bar foo");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(1..1);
        assert!(editor.select_next_match());
        editor.insert_text("baz").unwrap();
        assert_eq!(editor.snapshot().text(), "baz bar foo");

        assert!(editor.undo_selection());
        assert_eq!(editor.snapshot().text(), "baz bar foo");
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..1]
        );
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
    fn selection_only_undo_restores_display_point_selection_changes() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_display_point(1, 2, false, None).unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 6..6);

        assert!(editor.undo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 0..0);

        assert!(editor.redo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 6..6);
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
    fn selection_only_checkpoint_groups_transient_display_point_changes() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abc\ndef");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        let checkpoint = editor.selection_history_checkpoint();
        editor
            .select_display_point_transient(0, 1, false, None)
            .unwrap();
        editor
            .select_display_point_transient(1, 2, true, None)
            .unwrap();
        assert_eq!(editor.resolved_selections()[0].range(), 1..6);

        editor.commit_selection_only_history_from_checkpoint(checkpoint);
        assert!(editor.undo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 0..0);
        assert!(!editor.undo_selection());

        assert!(editor.redo_selection());
        assert_eq!(editor.resolved_selections()[0].range(), 1..6);
    }

    #[test]
    fn selection_only_undo_redo_preserves_active_index_for_collapsed_selections() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdefghi");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select_anchor_heads(vec![(0, 3), (8, 5)]);
        editor.set_active_selection_index(0).unwrap();
        editor.collapse_selections_to_heads();
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![3..3, 5..5]
        );
        assert_eq!(editor.active_selection_index(), 0);

        assert!(editor.undo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 5..8]
        );
        assert_eq!(editor.active_selection_index(), 0);

        assert!(editor.redo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![3..3, 5..5]
        );
        assert_eq!(editor.active_selection_index(), 0);
    }

    #[test]
    fn selection_only_undo_restores_multi_caret_and_rectangle_changes() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());

        editor.select(6..6);
        editor.add_caret_at_display_point(2, 1, None);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![6..6, 9..9]
        );

        assert!(editor.undo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![6..6]
        );

        editor.select_display_rectangle(0, 1, 2, 3, None);
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![1..3, 6..7, 9..11]
        );

        assert!(editor.undo_selection());
        assert_eq!(
            editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![6..6]
        );
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
    fn cursor_queries_resolve_selection_anchors_without_mutating_cached_offsets() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();

        assert_eq!(editor.snapshot().text(), "say hello world");
        assert_eq!(editor.selections()[0].head(), 6);
        assert_eq!(editor.cursor_offset().unwrap(), 10);
        assert_eq!(editor.resolved_selections()[0].head(), 10);
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
    fn reversed_selection_syncs_head_and_tail_from_tracked_anchors() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_head(8, 2);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.sync_selections_to_anchors();

        let selection = &editor.selections()[0];
        assert_eq!(selection.range(), 6..12);
        assert_eq!(selection.head(), 6);
        assert_eq!(selection.tail(), 12);
        assert!(selection.reversed);
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
