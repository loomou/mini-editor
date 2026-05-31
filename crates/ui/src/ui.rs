use editor::EditorModel;
use std::ops::Range;

use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

#[derive(Debug)]
pub struct EditorView {
    editor: EditorModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertChar(char),
    Backspace,
    Delete,
    Undo,
    Redo,
    MoveLeft { extend: bool },
    MoveRight { extend: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    pub changed_text: bool,
    pub moved_cursor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedEditor {
    pub title: String,
    pub is_dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub lines: Vec<RenderedLine>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLine {
    pub line_number: usize,
    pub text: String,
    pub continuation: bool,
    pub cursor_columns: Vec<usize>,
    pub active_cursor_columns: Vec<usize>,
    pub selection_ranges: Vec<Range<usize>>,
}

impl RenderedLine {
    pub fn text_with_cursors(&self) -> String {
        let mut text = self.text.clone();
        let mut cursor_columns = self.cursor_columns.clone();
        cursor_columns.sort_unstable();
        cursor_columns.dedup();

        for column in cursor_columns.into_iter().rev() {
            if column <= text.len() && text.is_char_boundary(column) {
                text.insert(column, '|');
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
            if column <= text.len() && text.is_char_boundary(column) {
                text.insert(column, marker);
            }
        }

        text
    }
}

impl EditorView {
    pub fn new(editor: EditorModel) -> Self {
        Self { editor }
    }

    pub fn dispatch_command(&mut self, command: EditorCommand) -> Result<CommandOutcome, String> {
        let before_text = self.editor.snapshot().text().to_string();
        let before_selections = selection_state(&self.editor);

        match command {
            EditorCommand::InsertChar(character) => self.editor.insert_char(character)?,
            EditorCommand::Backspace => {
                self.editor.backspace()?;
            }
            EditorCommand::Delete => {
                self.editor.delete()?;
            }
            EditorCommand::Undo => {
                self.editor.undo()?;
            }
            EditorCommand::Redo => {
                self.editor.redo()?;
            }
            EditorCommand::MoveLeft { extend } => self.editor.move_left(extend)?,
            EditorCommand::MoveRight { extend } => self.editor.move_right(extend)?,
        }

        let after_text = self.editor.snapshot().text().to_string();
        let after_selections = selection_state(&self.editor);
        Ok(CommandOutcome {
            changed_text: before_text != after_text,
            moved_cursor: before_selections != after_selections,
        })
    }

    pub fn text(&self) -> String {
        self.rendered_lines(None)
            .into_iter()
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn rendered_editor(&self, soft_wrap_column: Option<usize>) -> RenderedEditor {
        RenderedEditor {
            title: self.editor.title(),
            is_dirty: self.editor.is_dirty(),
            can_undo: self.editor.can_undo(),
            can_redo: self.editor.can_redo(),
            lines: self.rendered_lines(soft_wrap_column),
        }
    }

    pub fn refresh_buffer_ranges(&mut self) {
        self.editor.refresh_buffer_ranges();
    }

    pub fn rendered_lines(&self, soft_wrap_column: Option<usize>) -> Vec<RenderedLine> {
        let display = self.editor.display_snapshot(soft_wrap_column);
        let cursors = self.editor.cursor_display_points(soft_wrap_column);
        let active_cursor = self.editor.cursor_display_point(soft_wrap_column).ok();
        let selections = self.editor.resolved_selections();

        display
            .rows()
            .iter()
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
                        let start = selection.start.max(row.source_range.start);
                        let end = selection.end.min(row.source_range.end);
                        (start < end)
                            .then_some(start - row.source_range.start..end - row.source_range.start)
                    })
                    .collect(),
            })
            .collect()
    }
}

fn marker_priority(marker: char) -> usize {
    match marker {
        '|' => 0,
        ']' => 1,
        '[' => 2,
        _ => 3,
    }
}

fn availability_label(available: bool) -> &'static str {
    if available { "on" } else { "off" }
}

fn selection_state(editor: &EditorModel) -> Vec<(usize, usize, usize, bool)> {
    let mut state = editor
        .resolved_selections()
        .into_iter()
        .map(|selection| {
            (
                selection.id,
                selection.start,
                selection.end,
                selection.reversed,
            )
        })
        .collect::<Vec<_>>();
    state.push((usize::MAX, editor.active_selection_index(), 0, false));
    state
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rendered = self.rendered_editor(Some(100));
        div()
            .size_full()
            .p_4()
            .bg(rgb(0x1f2328))
            .text_color(rgb(0xd0d7de))
            .font_family("monospace")
            .child(
                div()
                    .flex()
                    .gap_3()
                    .mb_3()
                    .text_color(if rendered.is_dirty {
                        rgb(0xffd33d)
                    } else {
                        rgb(0x8b949e)
                    })
                    .child(rendered.header_text())
                    .child(rendered.command_status_text()),
            )
            .children(rendered.lines.into_iter().map(|line| {
                div()
                    .flex()
                    .gap_3()
                    .child(div().w(px(48.0)).text_color(rgb(0x6e7681)).child(
                        if line.continuation {
                            "...".to_string()
                        } else {
                            line.line_number.to_string()
                        },
                    ))
                    .child(div().child(line.text_with_overlays()))
            }))
    }
}

pub fn run(editor: EditorModel) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| EditorView::new(editor)),
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
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
                },
                RenderedLine {
                    line_number: 2,
                    text: "def".to_string(),
                    continuation: true,
                    cursor_columns: Vec::new(),
                    active_cursor_columns: Vec::new(),
                    selection_ranges: Vec::new(),
                },
            ]
        );
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

    #[test]
    fn view_dispatches_insert_to_multiple_selections() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc").into_handle(),
        );
        editor.select_ranges(vec![0..1, 2..3]);
        let mut view = EditorView::new(editor);

        let outcome = view
            .dispatch_command(EditorCommand::InsertChar('x'))
            .unwrap();

        assert!(outcome.changed_text);
        assert_eq!(view.text(), "xbx");
    }

    #[test]
    fn view_dispatches_delete_to_multiple_selections() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc").into_handle(),
        );
        editor.select_ranges(vec![0..1, 2..3]);
        let mut view = EditorView::new(editor);

        let outcome = view.dispatch_command(EditorCommand::Delete).unwrap();

        assert!(outcome.changed_text);
        assert_eq!(view.text(), "b");
    }

    #[test]
    fn command_outcome_tracks_secondary_cursor_movement() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "ab").into_handle(),
        );
        editor.select_ranges(vec![0..0, 1..1]);
        editor.set_active_selection_index(0).unwrap();
        let mut view = EditorView::new(editor);

        assert_eq!(
            view.dispatch_command(EditorCommand::Delete).unwrap(),
            CommandOutcome {
                changed_text: true,
                moved_cursor: true,
            }
        );
        assert_eq!(view.text(), "");
    }

    #[test]
    fn view_dispatches_insert_and_backspace_commands() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "").into_handle(),
        );
        let mut view = EditorView::new(editor);

        assert_eq!(
            view.dispatch_command(EditorCommand::InsertChar('a'))
                .unwrap(),
            CommandOutcome {
                changed_text: true,
                moved_cursor: true,
            }
        );
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a|");

        assert_eq!(
            view.dispatch_command(EditorCommand::Backspace).unwrap(),
            CommandOutcome {
                changed_text: true,
                moved_cursor: true,
            }
        );
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|");
    }

    #[test]
    fn view_dispatches_undo_and_redo_commands() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "hello").into_handle(),
        );
        let mut view = EditorView::new(editor);

        view.dispatch_command(EditorCommand::InsertChar('/'))
            .unwrap();
        assert_eq!(view.text(), "/hello");

        assert_eq!(
            view.dispatch_command(EditorCommand::Undo).unwrap(),
            CommandOutcome {
                changed_text: true,
                moved_cursor: true,
            }
        );
        assert_eq!(view.text(), "hello");

        assert_eq!(
            view.dispatch_command(EditorCommand::Redo).unwrap(),
            CommandOutcome {
                changed_text: true,
                moved_cursor: true,
            }
        );
        assert_eq!(view.text(), "/hello");
    }

    #[test]
    fn view_dispatches_movement_commands() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "ab").into_handle(),
        );
        let mut view = EditorView::new(editor);

        assert_eq!(
            view.dispatch_command(EditorCommand::MoveRight { extend: false })
                .unwrap(),
            CommandOutcome {
                changed_text: false,
                moved_cursor: true,
            }
        );
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a|b");

        assert_eq!(
            view.dispatch_command(EditorCommand::MoveRight { extend: true })
                .unwrap(),
            CommandOutcome {
                changed_text: false,
                moved_cursor: true,
            }
        );
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[b]|");
    }

    #[test]
    fn view_reports_noop_command_outcomes() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a").into_handle(),
        );
        let mut view = EditorView::new(editor);

        assert_eq!(
            view.dispatch_command(EditorCommand::Backspace).unwrap(),
            CommandOutcome {
                changed_text: false,
                moved_cursor: false,
            }
        );
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|a");
    }
}
