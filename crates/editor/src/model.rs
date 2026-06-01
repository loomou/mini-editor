use crate::selection::Selection;
use crate::selection_history::SelectionHistoryEntry;
use multibuffer::MultiBuffer;

#[derive(Debug)]
pub struct EditorModel {
    pub(crate) buffer: MultiBuffer,
    pub(crate) selections: Vec<Selection>,
    pub(crate) active_selection_index: usize,
    pub(crate) selection_undo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_redo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_only_undo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_only_redo_stack: Vec<SelectionHistoryEntry>,
}
