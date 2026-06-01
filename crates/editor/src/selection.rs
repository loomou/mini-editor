use multibuffer::MultiBufferAnchor;
use std::ops::Range;

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
    pub(crate) tail_anchor: Option<MultiBufferAnchor>,
    pub(crate) head_anchor: Option<MultiBufferAnchor>,
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

    pub(crate) fn clamp_to_text(&mut self, text: &str) {
        self.start = floor_char_boundary(text, self.start);
        self.end = floor_char_boundary(text, self.end);

        if self.start == self.end {
            self.reversed = false;
            self.goal = SelectionGoal::None;
        }
    }

    pub(crate) fn set_anchor_handles(
        &mut self,
        tail_anchor: MultiBufferAnchor,
        head_anchor: MultiBufferAnchor,
    ) {
        self.tail_anchor = Some(tail_anchor);
        self.head_anchor = Some(head_anchor);
    }
}

pub(crate) fn normalize_new_selections(mut selections: Vec<Selection>) -> Vec<Selection> {
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

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut clipped = offset.min(text.len());
    while clipped > 0 && !text.is_char_boundary(clipped) {
        clipped -= 1;
    }
    clipped
}

#[cfg(test)]
mod tests {
    use crate::{EditorModel, Selection};
    use language::Buffer;
    use text::BufferId;

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
}
