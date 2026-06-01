use crate::model::EditorModel;
use crate::selection::Selection;

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionHistoryCheckpoint {
    pub(crate) selections: Vec<Selection>,
    pub(crate) active_selection_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectionHistoryEntry {
    pub(crate) undo: Vec<Selection>,
    pub(crate) undo_active_selection_index: usize,
    pub(crate) redo: Vec<Selection>,
    pub(crate) redo_active_selection_index: usize,
}

pub(crate) fn selection_history_key(
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

impl EditorModel {
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
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
    use language::Buffer;
    use text::BufferId;

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
}
