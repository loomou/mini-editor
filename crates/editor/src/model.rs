use crate::selection::Selection;
use crate::selection_history::SelectionHistoryEntry;
use display::DisplaySnapshot;
use multibuffer::MultiBuffer;
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DisplayCacheKey {
    pub(crate) text_versions: Vec<(u64, u64)>,
    pub(crate) excerpt_versions: Vec<(u64, usize, usize, usize, usize)>,
    pub(crate) soft_wrap_column: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedDisplaySnapshot {
    pub(crate) key: DisplayCacheKey,
    pub(crate) snapshot: DisplaySnapshot,
}

#[derive(Debug)]
pub struct EditorModel {
    pub(crate) buffer: MultiBuffer,
    pub(crate) selections: Vec<Selection>,
    pub(crate) active_selection_index: usize,
    pub(crate) selection_undo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_redo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_only_undo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) selection_only_redo_stack: Vec<SelectionHistoryEntry>,
    pub(crate) display_cache: RefCell<Option<CachedDisplaySnapshot>>,
}
