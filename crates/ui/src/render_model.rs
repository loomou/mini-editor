use crate::{availability_label, byte_offset_for_display_column, marker_priority};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedEditor {
    pub title: String,
    pub is_dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub lines: Vec<RenderedLine>,
    pub scrollbar: Option<RenderedScrollbar>,
}

impl RenderedEditor {
    pub fn header_text(&self) -> String {
        if self.is_dirty {
            format!("* {}", self.title)
        } else {
            format!("  {}", self.title)
        }
    }

    pub fn command_status_text(&self) -> String {
        format!(
            "undo:{} redo:{}",
            availability_label(self.can_undo),
            availability_label(self.can_redo)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderedScrollbar {
    pub first_visible_row: usize,
    pub visible_rows: usize,
    pub total_rows: usize,
    pub hovered_row: Option<usize>,
    pub pressed: bool,
}

impl RenderedScrollbar {
    pub fn thumb_rows(&self) -> Range<usize> {
        if self.total_rows <= self.visible_rows {
            return 0..self.visible_rows.max(1);
        }

        let visible_rows = self.visible_rows.max(1);
        let thumb_len = (visible_rows * visible_rows)
            .div_ceil(self.total_rows)
            .max(1);
        let max_start = visible_rows.saturating_sub(thumb_len);
        let scrollable_rows = self.total_rows.saturating_sub(visible_rows).max(1);
        let thumb_start = (self.first_visible_row.min(scrollable_rows) * max_start
            + scrollable_rows / 2)
            / scrollable_rows;
        thumb_start..thumb_start + thumb_len
    }

    pub fn row_state(&self, row: usize) -> RenderedScrollbarRowState {
        let hovered = self.hovered_row == Some(row);
        let thumb = self.thumb_rows().contains(&row);
        let pressed = self.pressed && hovered;
        RenderedScrollbarRowState {
            thumb,
            hovered,
            pressed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderedScrollbarRowState {
    pub thumb: bool,
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLine {
    pub line_number: usize,
    pub text: String,
    pub continuation: bool,
    pub cursor_columns: Vec<usize>,
    pub active_cursor_columns: Vec<usize>,
    pub selection_ranges: Vec<Range<usize>>,
    pub marked_ranges: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLineFragment {
    pub text: String,
    pub selected: bool,
    pub marked: bool,
    pub cursor: bool,
    pub active_cursor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RenderedLineOverlay {
    pub(crate) start_column: usize,
    pub(crate) column_span: usize,
    pub(crate) kind: RenderedLineOverlayKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderedLineOverlayKind {
    Selection,
    Marked,
    Cursor { active: bool },
}

impl RenderedLine {
    pub fn text_with_cursors(&self) -> String {
        let mut text = self.text.clone();
        let mut cursor_columns = self.cursor_columns.clone();
        cursor_columns.sort_unstable();
        cursor_columns.dedup();

        for column in cursor_columns.into_iter().rev() {
            if let Some(byte_offset) = byte_offset_for_display_column(&text, column) {
                text.insert(byte_offset, '|');
            }
        }

        text
    }

    pub fn text_with_overlays(&self) -> String {
        let mut text = self.text.clone();
        let mut markers = Vec::new();

        for range in &self.selection_ranges {
            markers.push((range.start, '['));
            markers.push((range.end, ']'));
        }
        for range in &self.marked_ranges {
            markers.push((range.start, '{'));
            markers.push((range.end, '}'));
        }
        for column in &self.cursor_columns {
            markers.push((*column, '|'));
        }

        markers.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| marker_priority(left.1).cmp(&marker_priority(right.1)))
        });

        for (column, marker) in markers {
            if let Some(byte_offset) = byte_offset_for_display_column(&text, column) {
                text.insert(byte_offset, marker);
            }
        }

        text
    }

    pub fn visual_fragments(&self) -> Vec<RenderedLineFragment> {
        let text_column_count = self.text.chars().count();
        let mut boundaries = vec![0, text_column_count];

        for range in &self.selection_ranges {
            if range.start <= text_column_count {
                boundaries.push(range.start);
            }
            if range.end <= text_column_count {
                boundaries.push(range.end);
            }
        }
        for range in &self.marked_ranges {
            if range.start <= text_column_count {
                boundaries.push(range.start);
            }
            if range.end <= text_column_count {
                boundaries.push(range.end);
            }
        }
        for column in &self.cursor_columns {
            if *column <= text_column_count {
                boundaries.push(*column);
            }
        }

        boundaries.sort_unstable();
        boundaries.dedup();

        let mut fragments = Vec::new();
        for window in boundaries.windows(2) {
            let start_column = window[0];
            let end_column = window[1];
            let Some(start_byte) = byte_offset_for_display_column(&self.text, start_column) else {
                continue;
            };
            let Some(end_byte) = byte_offset_for_display_column(&self.text, end_column) else {
                continue;
            };

            self.push_cursor_fragment(start_column, &mut fragments);

            if start_byte < end_byte {
                fragments.push(RenderedLineFragment {
                    text: self.text[start_byte..end_byte].to_string(),
                    selected: self.range_is_selected(start_column..end_column),
                    marked: self.range_is_marked(start_column..end_column),
                    cursor: false,
                    active_cursor: false,
                });
            }
        }

        self.push_cursor_fragment(text_column_count, &mut fragments);

        if fragments.is_empty() {
            fragments.push(RenderedLineFragment {
                text: String::new(),
                selected: false,
                marked: false,
                cursor: false,
                active_cursor: false,
            });
        }

        fragments
    }

    fn push_cursor_fragment(&self, column: usize, fragments: &mut Vec<RenderedLineFragment>) {
        let cursor_count = self
            .cursor_columns
            .iter()
            .filter(|cursor| **cursor == column)
            .count();
        if cursor_count == 0 {
            return;
        }

        let active_cursor = self
            .active_cursor_columns
            .iter()
            .any(|cursor| *cursor == column);
        for cursor_index in 0..cursor_count {
            fragments.push(RenderedLineFragment {
                text: String::new(),
                selected: false,
                marked: false,
                cursor: true,
                active_cursor: active_cursor && cursor_index == 0,
            });
        }
    }

    fn range_is_selected(&self, range: Range<usize>) -> bool {
        self.selection_ranges
            .iter()
            .any(|selection| selection.start < range.end && selection.end > range.start)
    }

    fn range_is_marked(&self, range: Range<usize>) -> bool {
        self.marked_ranges
            .iter()
            .any(|marked| marked.start < range.end && marked.end > range.start)
    }

    pub(crate) fn overlays(&self) -> Vec<RenderedLineOverlay> {
        let mut overlays = Vec::new();
        overlays.extend(
            self.selection_ranges
                .iter()
                .cloned()
                .map(|range| RenderedLineOverlay {
                    start_column: range.start,
                    column_span: range.end.saturating_sub(range.start),
                    kind: RenderedLineOverlayKind::Selection,
                }),
        );
        overlays.extend(
            self.marked_ranges
                .iter()
                .cloned()
                .map(|range| RenderedLineOverlay {
                    start_column: range.start,
                    column_span: range.end.saturating_sub(range.start),
                    kind: RenderedLineOverlayKind::Marked,
                }),
        );

        let mut sorted_cursor_columns = self.cursor_columns.clone();
        sorted_cursor_columns.sort_unstable();
        for column in sorted_cursor_columns {
            let earlier_same_column = overlays
                .iter()
                .filter(|overlay| {
                    overlay.start_column == column
                        && matches!(overlay.kind, RenderedLineOverlayKind::Cursor { .. })
                })
                .count();
            let active = earlier_same_column == 0
                && self
                    .active_cursor_columns
                    .iter()
                    .any(|active_column| *active_column == column);
            overlays.push(RenderedLineOverlay {
                start_column: column,
                column_span: 0,
                kind: RenderedLineOverlayKind::Cursor { active },
            });
        }

        overlays
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

    #[test]
    fn view_builds_rendered_lines_from_display_snapshot() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        let view = EditorView::new(editor);

        assert_eq!(
            view.rendered_lines(Some(3)),
            vec![
                RenderedLine {
                    line_number: 1,
                    text: "abc".to_string(),
                    continuation: false,
                    cursor_columns: vec![0],
                    active_cursor_columns: vec![0],
                    selection_ranges: Vec::new(),
                    marked_ranges: Vec::new(),
                },
                RenderedLine {
                    line_number: 2,
                    text: "def".to_string(),
                    continuation: true,
                    cursor_columns: Vec::new(),
                    active_cursor_columns: Vec::new(),
                    selection_ranges: Vec::new(),
                    marked_ranges: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn rendered_line_builds_visual_fragments_for_selection_and_cursors() {
        let line = RenderedLine {
            line_number: 1,
            text: "abcd".to_string(),
            continuation: false,
            cursor_columns: vec![1, 3],
            active_cursor_columns: vec![3],
            selection_ranges: vec![1..3],
            marked_ranges: Vec::new(),
        };

        assert_eq!(
            line.visual_fragments(),
            vec![
                RenderedLineFragment {
                    text: "a".to_string(),
                    selected: false,
                    marked: false,
                    cursor: false,
                    active_cursor: false,
                },
                RenderedLineFragment {
                    text: String::new(),
                    selected: false,
                    marked: false,
                    cursor: true,
                    active_cursor: false,
                },
                RenderedLineFragment {
                    text: "bc".to_string(),
                    selected: true,
                    marked: false,
                    cursor: false,
                    active_cursor: false,
                },
                RenderedLineFragment {
                    text: String::new(),
                    selected: false,
                    marked: false,
                    cursor: true,
                    active_cursor: true,
                },
                RenderedLineFragment {
                    text: "d".to_string(),
                    selected: false,
                    marked: false,
                    cursor: false,
                    active_cursor: false,
                },
            ]
        );
    }

    #[test]
    fn rendered_line_overlays_keep_cursors_out_of_text_flow() {
        let line = RenderedLine {
            line_number: 1,
            text: "abcd".to_string(),
            continuation: false,
            cursor_columns: vec![2],
            active_cursor_columns: vec![2],
            selection_ranges: vec![1..3],
            marked_ranges: vec![0..1],
        };

        assert_eq!(
            line.overlays(),
            vec![
                RenderedLineOverlay {
                    start_column: 1,
                    column_span: 2,
                    kind: RenderedLineOverlayKind::Selection,
                },
                RenderedLineOverlay {
                    start_column: 0,
                    column_span: 1,
                    kind: RenderedLineOverlayKind::Marked,
                },
                RenderedLineOverlay {
                    start_column: 2,
                    column_span: 0,
                    kind: RenderedLineOverlayKind::Cursor { active: true },
                },
            ]
        );
    }
}
