use crate::TextEdit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryEntry {
    pub(crate) undo: Vec<TextEdit>,
    pub(crate) redo: Vec<TextEdit>,
}
