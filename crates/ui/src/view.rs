use crate::{
    CURSOR_BLINK_INTERVAL, Context, DEFAULT_SOFT_WRAP_COLUMN, DEFAULT_VIEWPORT_ROWS, EditorModel,
    FocusHandle, Pixels, Point, RenderedEditor, RenderedLine, SelectionHistoryCheckpoint,
    display_range_for_source_range,
};
use display::DisplaySnapshot;
use std::ops::Range;

#[derive(Debug)]
pub struct EditorView {
    pub(crate) editor: EditorModel,
    pub(crate) focus_handle: Option<FocusHandle>,
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) selecting_with_mouse: bool,
    pub(crate) mouse_selection_checkpoint: Option<SelectionHistoryCheckpoint>,
    pub(crate) rectangular_selection_start: Option<(usize, usize)>,
    pub(crate) drag_autoscroll: Option<DragAutoscroll>,
    pub(crate) drag_autoscroll_generation: u64,
    pub(crate) scrollbar_drag: Option<ScrollbarDrag>,
    pub(crate) scrollbar_track_press: Option<ScrollbarTrackPress>,
    pub(crate) scrollbar_track_press_generation: u64,
    pub(crate) hovered_scrollbar_row: Option<usize>,
    pub(crate) viewport_start_row: usize,
    pub(crate) viewport_rows: usize,
    pub(crate) linewise_clipboard_text: Option<String>,
    pub(crate) cursor_blink_visible: bool,
    pub(crate) cursor_blink_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DragAutoscroll {
    pub(crate) position: Point<Pixels>,
    pub(crate) generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarDrag {
    pub(crate) thumb_grab_offset: Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScrollbarTrackPress {
    pub(crate) direction: isize,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScrollbarHit {
    pub(crate) visible_row: usize,
    pub(crate) thumb_rows: Range<usize>,
}
impl EditorView {
    pub fn new(editor: EditorModel) -> Self {
        Self {
            editor,
            focus_handle: None,
            marked_range: None,
            selecting_with_mouse: false,
            mouse_selection_checkpoint: None,
            rectangular_selection_start: None,
            drag_autoscroll: None,
            drag_autoscroll_generation: 0,
            scrollbar_drag: None,
            scrollbar_track_press: None,
            scrollbar_track_press_generation: 0,
            hovered_scrollbar_row: None,
            viewport_start_row: 0,
            viewport_rows: DEFAULT_VIEWPORT_ROWS,
            linewise_clipboard_text: None,
            cursor_blink_visible: true,
            cursor_blink_generation: 0,
        }
    }

    pub(crate) fn with_focus(editor: EditorModel, focus_handle: FocusHandle) -> Self {
        Self {
            editor,
            focus_handle: Some(focus_handle),
            marked_range: None,
            selecting_with_mouse: false,
            mouse_selection_checkpoint: None,
            rectangular_selection_start: None,
            drag_autoscroll: None,
            drag_autoscroll_generation: 0,
            scrollbar_drag: None,
            scrollbar_track_press: None,
            scrollbar_track_press_generation: 0,
            hovered_scrollbar_row: None,
            viewport_start_row: 0,
            viewport_rows: DEFAULT_VIEWPORT_ROWS,
            linewise_clipboard_text: None,
            cursor_blink_visible: true,
            cursor_blink_generation: 0,
        }
    }

    pub fn with_viewport_rows(mut self, viewport_rows: usize) -> Self {
        self.viewport_rows = viewport_rows.max(1);
        self
    }

    pub fn text(&self) -> String {
        self.editor.snapshot().text().to_string()
    }

    pub fn rendered_editor(&self, soft_wrap_column: Option<usize>) -> RenderedEditor {
        let display = self.editor.display_snapshot(soft_wrap_column);
        RenderedEditor {
            title: self.editor.title(),
            is_dirty: self.editor.is_dirty(),
            can_undo: self.editor.can_undo(),
            can_redo: self.editor.can_redo(),
            lines: self.rendered_lines_for_display(&display),
            scrollbar: self.rendered_scrollbar_for_row_count(display.rows().len()),
        }
    }

    pub fn refresh_buffer_ranges(&mut self) {
        self.editor.refresh_buffer_ranges();
        self.clamp_viewport(Some(DEFAULT_SOFT_WRAP_COLUMN));
    }

    pub fn viewport_start_row(&self) -> usize {
        self.viewport_start_row
    }

    pub fn rendered_lines(&self, soft_wrap_column: Option<usize>) -> Vec<RenderedLine> {
        let display = self.editor.display_snapshot(soft_wrap_column);
        self.rendered_lines_for_display(&display)
    }

    fn rendered_lines_for_display(&self, display: &DisplaySnapshot) -> Vec<RenderedLine> {
        let cursors = self.editor.cursor_display_points_in(display);
        let active_cursor = self.editor.cursor_display_point_in(display).ok();
        let selections = self.editor.resolved_selections();
        let marked_range = self.marked_range.clone();

        display
            .rows()
            .iter()
            .skip(self.viewport_start_row)
            .take(self.viewport_rows.max(1))
            .map(|row| RenderedLine {
                line_number: row.row + 1,
                text: row.text.clone(),
                continuation: row.continuation,
                cursor_columns: cursors
                    .iter()
                    .filter(|cursor| cursor.row == row.row)
                    .map(|cursor| cursor.column)
                    .collect(),
                active_cursor_columns: active_cursor
                    .filter(|cursor| cursor.row == row.row)
                    .map(|cursor| vec![cursor.column])
                    .unwrap_or_default(),
                selection_ranges: selections
                    .iter()
                    .filter(|selection| !selection.is_empty())
                    .filter_map(|selection| {
                        display_range_for_source_range(
                            &row.text,
                            &row.source_range,
                            selection.range(),
                        )
                    })
                    .collect(),
                marked_ranges: marked_range
                    .clone()
                    .into_iter()
                    .filter_map(|range| {
                        display_range_for_source_range(&row.text, &row.source_range, range)
                    })
                    .collect(),
            })
            .collect()
    }

    pub(crate) fn restart_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_visible = true;
        self.cursor_blink_generation = self.cursor_blink_generation.wrapping_add(1);
        let generation = self.cursor_blink_generation;

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;

                let should_continue = this
                    .update(cx, |view, cx| {
                        if view.cursor_blink_generation != generation {
                            return false;
                        }

                        view.cursor_blink_visible = !view.cursor_blink_visible;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn ensure_cursor_blink(&mut self, cx: &mut Context<Self>) {
        if self.cursor_blink_generation == 0 {
            self.restart_cursor_blink(cx);
        }
    }

    pub(crate) fn cancel_interaction(&mut self) {
        self.marked_range = None;
        self.selecting_with_mouse = false;
        self.mouse_selection_checkpoint = None;
        self.rectangular_selection_start = None;
        self.drag_autoscroll = None;
        self.scrollbar_drag = None;
        self.scrollbar_track_press = None;

        self.editor.collapse_selections_to_heads();
    }

    pub(crate) fn commit_mouse_selection_history(&mut self) {
        if let Some(checkpoint) = self.mouse_selection_checkpoint.take() {
            self.editor
                .commit_selection_only_history_from_checkpoint(checkpoint);
        }
    }

    pub(crate) fn render_focus_handle(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.focus_handle
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use language::{Buffer, SourceFile};
    use text::BufferId;

    #[test]
    fn view_exposes_editor_text_for_rendering() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "hello gpui").into_handle(),
        );
        let view = EditorView::new(editor);

        assert_eq!(view.text(), "hello gpui");
    }

    #[test]
    fn view_exposes_rendered_editor_title_and_dirty_state() {
        let editor = EditorModel::for_buffer(
            "src/main.rs",
            Buffer::from_file(
                BufferId::new(1).unwrap(),
                SourceFile::new("src/main.rs"),
                "hello world",
            )
            .into_handle(),
        );
        let mut view = EditorView::new(editor);

        let clean = view.rendered_editor(None);
        assert_eq!(clean.title, "src/main.rs");
        assert!(!clean.is_dirty);
        assert!(!clean.can_undo);
        assert!(!clean.can_redo);
        assert_eq!(clean.header_text(), "  src/main.rs");
        assert_eq!(clean.command_status_text(), "undo:off redo:off");

        view.dispatch_command(EditorCommand::InsertChar('/'))
            .unwrap();

        let dirty = view.rendered_editor(None);
        assert!(dirty.is_dirty);
        assert!(dirty.can_undo);
        assert!(!dirty.can_redo);
        assert_eq!(dirty.header_text(), "* src/main.rs");
        assert_eq!(dirty.command_status_text(), "undo:on redo:off");

        view.dispatch_command(EditorCommand::Undo).unwrap();
        let after_undo = view.rendered_editor(None);
        assert!(!after_undo.can_undo);
        assert!(after_undo.can_redo);
        assert_eq!(after_undo.command_status_text(), "undo:off redo:on");
    }

    #[test]
    fn view_marks_cursor_on_wrapped_display_row() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        editor.select(4..4);
        let view = EditorView::new(editor);

        let lines = view.rendered_lines(Some(3));

        assert_eq!(lines[0].cursor_columns, Vec::<usize>::new());
        assert_eq!(lines[1].cursor_columns, vec![1]);
        assert_eq!(lines[1].text_with_cursors(), "d|ef");
    }

    #[test]
    fn view_marks_all_cursors_on_wrapped_display_rows() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        editor.select_ranges(vec![1..1, 4..4]);
        let view = EditorView::new(editor);

        let lines = view.rendered_lines(Some(3));

        assert_eq!(lines[0].cursor_columns, vec![1]);
        assert_eq!(lines[1].cursor_columns, vec![1]);
        assert_eq!(lines[0].active_cursor_columns, Vec::<usize>::new());
        assert_eq!(lines[1].active_cursor_columns, vec![1]);
        assert_eq!(lines[0].text_with_cursors(), "a|bc");
        assert_eq!(lines[1].text_with_cursors(), "d|ef");
    }

    #[test]
    fn view_marks_active_cursor_separately_from_secondary_cursors() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        editor.select_ranges(vec![1..1, 4..4]);
        editor.set_active_selection_index(0).unwrap();
        let view = EditorView::new(editor);

        let lines = view.rendered_lines(Some(3));

        assert_eq!(lines[0].cursor_columns, vec![1]);
        assert_eq!(lines[0].active_cursor_columns, vec![1]);
        assert_eq!(lines[1].cursor_columns, vec![1]);
        assert_eq!(lines[1].active_cursor_columns, Vec::<usize>::new());
    }

    #[test]
    fn view_marks_selection_ranges_on_wrapped_rows() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        editor.select(1..4);
        let view = EditorView::new(editor);

        let lines = view.rendered_lines(Some(3));

        assert_eq!(lines[0].selection_ranges, vec![1..3]);
        assert_eq!(lines[1].selection_ranges, vec![0..1]);
        assert_eq!(lines[0].text_with_overlays(), "a[bc]");
        assert_eq!(lines[1].text_with_overlays(), "[d]|ef");
    }

    #[test]
    fn view_marks_multiple_selection_ranges() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        editor.select_ranges(vec![0..1, 3..4]);
        let view = EditorView::new(editor);

        let lines = view.rendered_lines(None);

        assert_eq!(lines[0].selection_ranges, vec![0..1, 3..4]);
    }
}
