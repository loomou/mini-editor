use crate::model::EditorModel;
use crate::selection::{Selection, normalize_new_selections};
use crate::utils::{
    find_all_non_overlapping_matches, find_next_non_overlapping_match, word_range_at_offset,
};

impl EditorModel {
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
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
    use language::Buffer;
    use text::BufferId;

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
}
