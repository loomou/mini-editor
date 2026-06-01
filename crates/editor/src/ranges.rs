use crate::selection::Selection;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct SelectionEditRange {
    pub(crate) selection_index: usize,
    pub(crate) selection: Selection,
    pub(crate) range: Range<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct SortedSelection {
    pub(crate) selection_index: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) id: usize,
    pub(crate) range: Range<usize>,
}

pub(crate) fn sorted_non_overlapping_selections(
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

pub(crate) fn sorted_non_overlapping_edit_ranges(
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
