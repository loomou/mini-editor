use editor::EditorModel;
use std::ops::Range;

use gpui::{
    App, Application, Bounds, Context, Element, ElementId, ElementInputHandler, Entity,
    FocusHandle, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent, Keystroke,
    LayoutId, Modifiers, MouseButton, Pixels, Render, Style, UTF16Selection, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};

#[derive(Debug)]
pub struct EditorView {
    editor: EditorModel,
    focus_handle: Option<FocusHandle>,
    marked_range: Option<Range<usize>>,
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
        Self {
            editor,
            focus_handle: None,
            marked_range: None,
        }
    }

    fn with_focus(editor: EditorModel, focus_handle: FocusHandle) -> Self {
        Self {
            editor,
            focus_handle: Some(focus_handle),
            marked_range: None,
        }
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

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = command_for_keystroke(&event.keystroke) {
            match self.dispatch_command(command) {
                Ok(_) => {
                    cx.notify();
                    cx.stop_propagation();
                }
                Err(error) => eprintln!("mini_ui command failed: {error}"),
            }
        }
    }

    fn render_focus_handle(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.focus_handle
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    fn active_input_range(&self) -> Range<usize> {
        self.editor
            .resolved_selections()
            .get(self.editor.active_selection_index())
            .map(|selection| selection.range())
            .unwrap_or(0..0)
    }

    fn replace_input_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
    ) -> Result<(), String> {
        let range = self.input_range_from_utf16(range_utf16);
        self.marked_range = None;
        self.editor.select(range);
        self.editor.insert_text(new_text.to_string())
    }

    fn replace_and_mark_input_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
    ) -> Result<(), String> {
        let range = self.input_range_from_utf16(range_utf16);
        let inserted_range = range.start..range.start + new_text.len();

        self.editor.select(range.clone());
        self.editor.insert_text(new_text.to_string())?;
        self.marked_range = (!new_text.is_empty()).then_some(inserted_range.clone());

        let selection = new_selected_range_utf16
            .map(|range| utf16_range_to_utf8(new_text, range))
            .map(|range| inserted_range.start + range.start..inserted_range.start + range.end)
            .unwrap_or(inserted_range.end..inserted_range.end);
        self.editor.select(selection);
        Ok(())
    }

    fn input_range_from_utf16(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| utf16_range_to_utf8(self.editor.snapshot().text(), range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.active_input_range())
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

struct EditorInputElement {
    view: Entity<EditorView>,
    focus_handle: FocusHandle,
}

impl IntoElement for EditorInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
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

fn command_for_keystroke(keystroke: &Keystroke) -> Option<EditorCommand> {
    let modifiers = keystroke.modifiers;

    match keystroke.key.as_str() {
        "backspace" if navigation_modifiers(modifiers) => Some(EditorCommand::Backspace),
        "delete" if navigation_modifiers(modifiers) => Some(EditorCommand::Delete),
        "left" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveLeft {
            extend: modifiers.shift,
        }),
        "right" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveRight {
            extend: modifiers.shift,
        }),
        "z" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Undo),
        "z" if shortcut_modifiers(modifiers, true) => Some(EditorCommand::Redo),
        "y" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Redo),
        _ => None,
    }
}

fn navigation_modifiers(modifiers: Modifiers) -> bool {
    !modifiers.control && !modifiers.alt && !modifiers.platform && !modifiers.function
}

fn shortcut_modifiers(modifiers: Modifiers, shift: bool) -> bool {
    if modifiers.alt || modifiers.function || modifiers.shift != shift {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        modifiers.platform && !modifiers.control
    }

    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control && !modifiers.platform
    }
}

fn utf16_range_to_utf8(text: &str, range_utf16: Range<usize>) -> Range<usize> {
    let start = utf16_offset_to_utf8(text, range_utf16.start);
    let end = utf16_offset_to_utf8(text, range_utf16.end);
    start.min(end)..start.max(end)
}

fn utf16_offset_to_utf8(text: &str, offset_utf16: usize) -> usize {
    let mut offset_utf8 = 0;
    let mut utf16_count = 0;

    for character in text.chars() {
        if utf16_count >= offset_utf16 {
            break;
        }
        utf16_count += character.len_utf16();
        offset_utf8 += character.len_utf8();
    }

    offset_utf8
}

fn utf8_range_to_utf16(text: &str, range_utf8: Range<usize>) -> Range<usize> {
    utf8_offset_to_utf16(text, range_utf8.start)..utf8_offset_to_utf16(text, range_utf8.end)
}

fn utf8_offset_to_utf16(text: &str, offset_utf8: usize) -> usize {
    let mut offset_utf16 = 0;
    let mut counted_utf8 = 0;

    for character in text.chars() {
        if counted_utf8 >= offset_utf8 {
            break;
        }
        counted_utf8 += character.len_utf8();
        offset_utf16 += character.len_utf16();
    }

    offset_utf16
}

impl gpui::EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.editor.snapshot().text().to_string();
        let range = utf16_range_to_utf8(&text, range_utf16);
        actual_range.replace(utf8_range_to_utf16(&text, range.clone()));
        text.get(range).map(ToString::to_string)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let text = self.editor.snapshot().text().to_string();
        let selection = self
            .editor
            .resolved_selections()
            .get(self.editor.active_selection_index())
            .cloned()?;

        Some(UTF16Selection {
            range: utf8_range_to_utf16(&text, selection.range()),
            reversed: selection.reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let text = self.editor.snapshot().text().to_string();
        self.marked_range
            .clone()
            .map(|range| utf8_range_to_utf16(&text, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.replace_input_text(range_utf16, new_text) {
            Ok(()) => cx.notify(),
            Err(error) => eprintln!("mini_ui input replacement failed: {error}"),
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.replace_and_mark_input_text(range_utf16, new_text, new_selected_range_utf16) {
            Ok(()) => cx.notify(),
            Err(error) => eprintln!("mini_ui marked input replacement failed: {error}"),
        }
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rendered = self.rendered_editor(Some(100));
        let focus_handle = self.render_focus_handle(cx);
        let click_focus_handle = focus_handle.clone();
        div()
            .size_full()
            .p_4()
            .bg(rgb(0x1f2328))
            .text_color(rgb(0xd0d7de))
            .key_context("EditorView")
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                click_focus_handle.focus(window);
            })
            .child(EditorInputElement {
                view: cx.entity(),
                focus_handle: focus_handle.clone(),
            })
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
            |window, cx| {
                let focus_handle = cx.focus_handle();
                window.focus(&focus_handle);
                cx.new(|_| EditorView::with_focus(editor, focus_handle))
            },
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

    #[test]
    fn keystrokes_map_to_editing_commands() {
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("backspace").unwrap()),
            Some(EditorCommand::Backspace)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("delete").unwrap()),
            Some(EditorCommand::Delete)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("left").unwrap()),
            Some(EditorCommand::MoveLeft { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("shift-right").unwrap()),
            Some(EditorCommand::MoveRight { extend: true })
        );
    }

    #[test]
    fn keystrokes_map_to_undo_redo_shortcuts() {
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-z").unwrap()),
            Some(EditorCommand::Undo)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-z").unwrap()),
            Some(EditorCommand::Redo)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-y").unwrap()),
            Some(EditorCommand::Redo)
        );
    }

    #[test]
    fn modified_printable_keystrokes_do_not_insert_text() {
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-a").unwrap()),
            None
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("alt-a").unwrap()),
            None
        );
        assert_eq!(command_for_keystroke(&Keystroke::parse("a").unwrap()), None);
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("space").unwrap()),
            None
        );
    }

    #[test]
    fn gpui_text_input_replaces_active_selection() {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "hello").into_handle(),
        );
        editor.select(1..4);
        let mut view = EditorView::new(editor);

        view.replace_input_text(None, "i").unwrap();

        assert_eq!(view.text(), "hio");
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "hi|o");
    }

    #[test]
    fn gpui_text_input_uses_utf16_replacement_ranges() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
        );
        let mut view = EditorView::new(editor);

        view.replace_input_text(Some(1..3), "b").unwrap();

        assert_eq!(view.text(), "abc");
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "ab|c");
    }

    #[test]
    fn gpui_marked_text_tracks_composition_range() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "ac").into_handle(),
        );
        let mut view = EditorView::new(editor);

        view.editor.select(1..1);
        view.replace_and_mark_input_text(None, "b", Some(1..1))
            .unwrap();

        assert_eq!(view.text(), "abc");
        assert_eq!(view.marked_range, Some(1..2));
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "ab|c");
    }

    #[gpui::test]
    fn gpui_simulated_platform_input_edits_view(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });
        cx.simulate_input("abc");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc|");
        });
    }
}
