use editor::{EditorModel, Selection, SelectionHistoryCheckpoint};
use std::{ops::Range, time::Duration};

use gpui::{
    App, Application, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler,
    Entity, FocusHandle, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent, Keystroke,
    LayoutId, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, ShapedLine, SharedString, Style, TextRun,
    UTF16Selection, Window, WindowBounds, WindowOptions, canvas, div, fill, point, prelude::*, px,
    rgb, size,
};

const DEFAULT_SOFT_WRAP_COLUMN: usize = 100;
const EDITOR_PADDING: Pixels = px(16.0);
const HEADER_HEIGHT: Pixels = px(28.0);
const LINE_HEIGHT: Pixels = px(24.0);
const LINE_NUMBER_WIDTH: Pixels = px(48.0);
const CONTENT_GAP: Pixels = px(12.0);
const DISPLAY_COLUMN_WIDTH: Pixels = px(8.0);
const CARET_WIDTH: Pixels = px(2.0);
const SCROLLBAR_WIDTH: Pixels = px(6.0);
const SCROLLBAR_GAP: Pixels = px(8.0);
const DEFAULT_VIEWPORT_ROWS: usize = 20;
const SCROLLBAR_TRACK_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(350);
const SCROLLBAR_TRACK_REPEAT_INTERVAL: Duration = Duration::from_millis(75);
const DRAG_AUTOSCROLL_REPEAT_INTERVAL: Duration = Duration::from_millis(75);

#[derive(Debug)]
pub struct EditorView {
    editor: EditorModel,
    focus_handle: Option<FocusHandle>,
    marked_range: Option<Range<usize>>,
    selecting_with_mouse: bool,
    mouse_selection_checkpoint: Option<SelectionHistoryCheckpoint>,
    rectangular_selection_start: Option<(usize, usize)>,
    drag_autoscroll: Option<DragAutoscroll>,
    drag_autoscroll_generation: u64,
    scrollbar_drag: Option<ScrollbarDrag>,
    scrollbar_track_press: Option<ScrollbarTrackPress>,
    scrollbar_track_press_generation: u64,
    hovered_scrollbar_row: Option<usize>,
    viewport_start_row: usize,
    viewport_rows: usize,
    linewise_clipboard_text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DragAutoscroll {
    position: Point<Pixels>,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollbarDrag {
    thumb_grab_offset: Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScrollbarTrackPress {
    direction: isize,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScrollbarHit {
    visible_row: usize,
    thumb_rows: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertChar(char),
    InsertText(&'static str),
    Backspace,
    Delete,
    Undo,
    Redo,
    UndoSelection,
    RedoSelection,
    MoveLeft { extend: bool },
    MoveRight { extend: bool },
    MoveUp { extend: bool },
    MoveDown { extend: bool },
    MoveToLineStart { extend: bool },
    MoveToLineEnd { extend: bool },
    MoveToDocumentStart { extend: bool },
    MoveToDocumentEnd { extend: bool },
    MoveToPreviousWord { extend: bool },
    MoveToNextWord { extend: bool },
    AddCaretAbove,
    AddCaretBelow,
    PageUp { extend: bool },
    PageDown { extend: bool },
    ScrollUp,
    ScrollDown,
    SelectAll,
    SelectNextMatch,
    SelectAllMatches,
    SkipActiveMatch,
    Cancel,
    Copy,
    Cut,
    Paste,
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
struct RenderedLineOverlay {
    start_column: usize,
    column_span: usize,
    kind: RenderedLineOverlayKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderedLineOverlayKind {
    Selection,
    Marked,
    Cursor { active: bool },
}

struct VisualLinePaintState {
    line: ShapedLine,
    background_quads: Vec<PaintQuad>,
    cursor_quads: Vec<PaintQuad>,
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

    fn overlays(&self) -> Vec<RenderedLineOverlay> {
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

fn byte_offset_for_display_column(text: &str, display_column: usize) -> Option<usize> {
    if display_column == text.chars().count() {
        return Some(text.len());
    }

    text.char_indices()
        .nth(display_column)
        .map(|(offset, _)| offset)
}

fn byte_offset_for_display_column_or_end(text: &str, display_column: usize) -> usize {
    byte_offset_for_display_column(text, display_column).unwrap_or(text.len())
}

fn display_column_for_byte_offset(text: &str, byte_offset: usize) -> usize {
    text.char_indices()
        .take_while(|(offset, _)| *offset < byte_offset)
        .count()
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
        }
    }

    fn with_focus(editor: EditorModel, focus_handle: FocusHandle) -> Self {
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
        }
    }

    pub fn with_viewport_rows(mut self, viewport_rows: usize) -> Self {
        self.viewport_rows = viewport_rows.max(1);
        self
    }

    pub fn dispatch_command(&mut self, command: EditorCommand) -> Result<CommandOutcome, String> {
        let before_text = self.editor.snapshot().text().to_string();
        let before_selections = selection_state(&self.editor);

        let mut reveal_cursor = true;
        match command {
            EditorCommand::InsertChar(character) => self.editor.insert_char(character)?,
            EditorCommand::InsertText(text) => self.editor.insert_text(text)?,
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
            EditorCommand::UndoSelection => {
                self.editor.undo_selection();
            }
            EditorCommand::RedoSelection => {
                self.editor.redo_selection();
            }
            EditorCommand::MoveLeft { extend } => self.editor.move_left(extend)?,
            EditorCommand::MoveRight { extend } => self.editor.move_right(extend)?,
            EditorCommand::MoveUp { extend } => {
                self.editor
                    .move_up(extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::MoveDown { extend } => {
                self.editor
                    .move_down(extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::MoveToLineStart { extend } => {
                self.editor
                    .move_to_line_start(extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::MoveToLineEnd { extend } => {
                self.editor
                    .move_to_line_end(extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::MoveToDocumentStart { extend } => {
                self.editor.move_to_document_start(extend)?;
            }
            EditorCommand::MoveToDocumentEnd { extend } => {
                self.editor.move_to_document_end(extend)?;
            }
            EditorCommand::MoveToPreviousWord { extend } => {
                self.editor.move_to_previous_word(extend)?;
            }
            EditorCommand::MoveToNextWord { extend } => {
                self.editor.move_to_next_word(extend)?;
            }
            EditorCommand::AddCaretAbove => {
                self.editor
                    .add_caret_above(Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::AddCaretBelow => {
                self.editor
                    .add_caret_below(Some(DEFAULT_SOFT_WRAP_COLUMN))?;
            }
            EditorCommand::PageUp { extend } => self.move_page(-1, extend)?,
            EditorCommand::PageDown { extend } => self.move_page(1, extend)?,
            EditorCommand::ScrollUp => {
                self.scroll_viewport(-1);
                reveal_cursor = false;
            }
            EditorCommand::ScrollDown => {
                self.scroll_viewport(1);
                reveal_cursor = false;
            }
            EditorCommand::SelectAll => self.editor.select_all(),
            EditorCommand::SelectNextMatch => {
                self.editor.select_next_match();
            }
            EditorCommand::SelectAllMatches => {
                self.editor.select_all_matches();
            }
            EditorCommand::SkipActiveMatch => {
                self.editor.skip_active_match();
            }
            EditorCommand::Cancel => self.cancel_interaction(),
            EditorCommand::Copy | EditorCommand::Cut | EditorCommand::Paste => {}
        }

        if reveal_cursor {
            self.reveal_active_cursor(Some(DEFAULT_SOFT_WRAP_COLUMN));
        }
        let after_text = self.editor.snapshot().text().to_string();
        let after_selections = selection_state(&self.editor);
        Ok(CommandOutcome {
            changed_text: before_text != after_text,
            moved_cursor: before_selections != after_selections,
        })
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
            lines: self.rendered_lines(soft_wrap_column),
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

    fn rendered_scrollbar_for_row_count(&self, row_count: usize) -> Option<RenderedScrollbar> {
        let visible_rows = self.viewport_rows.max(1);
        (row_count > visible_rows).then_some(RenderedScrollbar {
            first_visible_row: self.viewport_start_row,
            visible_rows,
            total_rows: row_count,
            hovered_row: self.hovered_scrollbar_row.filter(|row| *row < visible_rows),
            pressed: self.scrollbar_drag.is_some() || self.scrollbar_track_press.is_some(),
        })
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = command_for_keystroke(&event.keystroke) {
            match self.dispatch_context_command(command, cx) {
                Ok(_) => {
                    cx.notify();
                    cx.stop_propagation();
                }
                Err(error) => eprintln!("mini_ui command failed: {error}"),
            }
        }
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus_handle) = self.focus_handle.as_ref() {
            focus_handle.focus(window);
        }

        if let Some(hit) = self.scrollbar_hit_for_position(event.position) {
            self.hovered_scrollbar_row = Some(hit.visible_row);
            self.selecting_with_mouse = false;
            self.mouse_selection_checkpoint = None;
            self.rectangular_selection_start = None;
            self.drag_autoscroll = None;
            if hit.thumb_rows.contains(&hit.visible_row) {
                let thumb_top = scrollbar_track_top() + LINE_HEIGHT * hit.thumb_rows.start;
                self.scrollbar_drag = Some(ScrollbarDrag {
                    thumb_grab_offset: event.position.y - thumb_top,
                });
                self.scrollbar_track_press = None;
            } else if hit.visible_row < hit.thumb_rows.start {
                self.scroll_viewport(-1);
                self.scrollbar_drag = None;
                self.start_scrollbar_track_repeat(-1, cx);
            } else {
                self.scroll_viewport(1);
                self.scrollbar_drag = None;
                self.start_scrollbar_track_repeat(1, cx);
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }

        self.hovered_scrollbar_row = None;
        let (row, column) = self.display_point_for_mouse_position(event.position);
        let result = match event.click_count {
            3.. => {
                self.commit_mouse_selection_history();
                self.selecting_with_mouse = false;
                self.rectangular_selection_start = None;
                self.drag_autoscroll = None;
                self.scrollbar_drag = None;
                self.scrollbar_track_press = None;
                self.editor.select_line_at_display_point(
                    row,
                    column,
                    Some(DEFAULT_SOFT_WRAP_COLUMN),
                );
                Ok(())
            }
            2 => {
                self.commit_mouse_selection_history();
                self.selecting_with_mouse = false;
                self.rectangular_selection_start = None;
                self.drag_autoscroll = None;
                self.scrollbar_drag = None;
                self.scrollbar_track_press = None;
                self.editor.select_word_at_display_point(
                    row,
                    column,
                    Some(DEFAULT_SOFT_WRAP_COLUMN),
                );
                Ok(())
            }
            _ => {
                self.drag_autoscroll = None;
                self.scrollbar_drag = None;
                self.scrollbar_track_press = None;
                if event.modifiers.alt && !event.modifiers.shift {
                    self.selecting_with_mouse = true;
                    self.mouse_selection_checkpoint =
                        Some(self.editor.selection_history_checkpoint());
                    self.rectangular_selection_start = Some((row, column));
                    self.editor.add_caret_at_display_point_transient(
                        row,
                        column,
                        Some(DEFAULT_SOFT_WRAP_COLUMN),
                    );
                    Ok(())
                } else {
                    self.selecting_with_mouse = true;
                    self.mouse_selection_checkpoint =
                        Some(self.editor.selection_history_checkpoint());
                    self.rectangular_selection_start = None;
                    self.editor.select_display_point_transient(
                        row,
                        column,
                        event.modifiers.shift,
                        Some(DEFAULT_SOFT_WRAP_COLUMN),
                    )
                }
            }
        };

        match result {
            Ok(()) => {
                self.reveal_active_cursor(Some(DEFAULT_SOFT_WRAP_COLUMN));
                cx.notify();
                cx.stop_propagation();
            }
            Err(error) => eprintln!("mini_ui mouse selection failed: {error}"),
        }
    }

    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_hovered_scrollbar_row = self.hovered_scrollbar_row;
        self.hovered_scrollbar_row = self.scrollbar_visible_row_for_position(event.position);

        if let Some(drag) = self.scrollbar_drag {
            let before = self.viewport_start_row;
            self.scroll_viewport_to_scrollbar_y(event.position.y, drag.thumb_grab_offset);
            if self.viewport_start_row != before
                || self.hovered_scrollbar_row != previous_hovered_scrollbar_row
            {
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }

        if !self.selecting_with_mouse {
            if self.hovered_scrollbar_row != previous_hovered_scrollbar_row {
                cx.notify();
            }
            return;
        }

        let result = if self.rectangular_selection_start.is_some() {
            self.extend_rectangular_selection_for_drag_position(event.position)
        } else {
            self.extend_selection_for_drag_position(event.position)
        };

        match result {
            Ok(_) => {
                self.update_drag_autoscroll(event.position, cx);
                cx.notify();
                cx.stop_propagation();
            }
            Err(error) => eprintln!("mini_ui mouse drag selection failed: {error}"),
        }
    }

    fn handle_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = scroll_rows_for_delta(event.delta);
        if rows == 0 {
            return;
        }

        let before = self.viewport_start_row;
        self.scroll_viewport_by_rows(rows);
        if self.viewport_start_row != before {
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn handle_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selecting_with_mouse
            || self.drag_autoscroll.is_some()
            || self.scrollbar_drag.is_some()
            || self.scrollbar_track_press.is_some()
        {
            self.selecting_with_mouse = false;
            self.commit_mouse_selection_history();
            self.rectangular_selection_start = None;
            self.drag_autoscroll = None;
            self.scrollbar_drag = None;
            self.scrollbar_track_press = None;
            self.hovered_scrollbar_row = self.scrollbar_visible_row_for_position(_event.position);
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn dispatch_context_command(
        &mut self,
        command: EditorCommand,
        cx: &mut Context<Self>,
    ) -> Result<CommandOutcome, String> {
        match command {
            EditorCommand::Copy => self.copy_selection_to_clipboard(cx, false),
            EditorCommand::Cut => Ok(self.copy_selection_to_clipboard(cx, true)?),
            EditorCommand::Paste => {
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return Ok(CommandOutcome {
                        changed_text: false,
                        moved_cursor: false,
                    });
                };
                self.dispatch_paste_text(&text)
            }
            command => self.dispatch_command(command),
        }
    }

    fn cancel_interaction(&mut self) {
        self.marked_range = None;
        self.selecting_with_mouse = false;
        self.mouse_selection_checkpoint = None;
        self.rectangular_selection_start = None;
        self.drag_autoscroll = None;
        self.scrollbar_drag = None;
        self.scrollbar_track_press = None;

        self.editor.collapse_selections_to_heads();
    }

    fn commit_mouse_selection_history(&mut self) {
        if let Some(checkpoint) = self.mouse_selection_checkpoint.take() {
            self.editor
                .commit_selection_only_history_from_checkpoint(checkpoint);
        }
    }

    fn move_page(&mut self, direction: isize, extend: bool) -> Result<(), String> {
        let page_rows = isize::try_from(self.viewport_rows.max(1)).unwrap_or(isize::MAX);
        let row_delta = if direction < 0 { -page_rows } else { page_rows };
        self.editor
            .move_display_rows(row_delta, extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
        self.scroll_viewport(direction);
        Ok(())
    }

    fn scroll_viewport(&mut self, direction: isize) {
        let page_rows = self.viewport_rows.max(1);
        self.scroll_viewport_by_rows(direction.saturating_mul(page_rows as isize));
    }

    fn scroll_viewport_by_rows(&mut self, rows: isize) {
        self.viewport_start_row = self.viewport_start_row.saturating_add_signed(rows);
        self.clamp_viewport(Some(DEFAULT_SOFT_WRAP_COLUMN));
    }

    fn update_drag_autoscroll(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selecting_with_mouse
            || self
                .drag_autoscroll_direction_for_position(position)
                .is_none()
        {
            self.drag_autoscroll = None;
            return;
        }

        if let Some(autoscroll) = self.drag_autoscroll.as_mut() {
            autoscroll.position = position;
            return;
        }

        self.drag_autoscroll_generation = self.drag_autoscroll_generation.wrapping_add(1);
        let generation = self.drag_autoscroll_generation;
        self.drag_autoscroll = Some(DragAutoscroll {
            position,
            generation,
        });

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(DRAG_AUTOSCROLL_REPEAT_INTERVAL)
                    .await;

                let should_continue = this
                    .update(cx, |view, cx| {
                        let changed = match view.repeat_drag_autoscroll(generation) {
                            Ok(changed) => changed,
                            Err(error) => {
                                eprintln!("mini_ui drag autoscroll failed: {error}");
                                false
                            }
                        };
                        if changed {
                            cx.notify();
                        }
                        changed
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }
            }
        })
        .detach();
    }

    fn repeat_drag_autoscroll(&mut self, generation: u64) -> Result<bool, String> {
        let Some(autoscroll) = self.drag_autoscroll else {
            return Ok(false);
        };
        if autoscroll.generation != generation || !self.selecting_with_mouse {
            return Ok(false);
        }

        let before_viewport = self.viewport_start_row;
        let before_selections = selection_state(&self.editor);
        if self.rectangular_selection_start.is_some() {
            self.extend_rectangular_selection_for_drag_position(autoscroll.position)?;
        } else {
            self.extend_selection_for_drag_position(autoscroll.position)?;
        }
        let changed = self.viewport_start_row != before_viewport
            || selection_state(&self.editor) != before_selections;
        if !changed {
            self.drag_autoscroll = None;
        }
        Ok(changed)
    }

    fn start_scrollbar_track_repeat(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.scrollbar_track_press_generation =
            self.scrollbar_track_press_generation.wrapping_add(1);
        let generation = self.scrollbar_track_press_generation;
        let press = ScrollbarTrackPress {
            direction,
            generation,
        };
        self.scrollbar_track_press = Some(press);

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(SCROLLBAR_TRACK_REPEAT_INITIAL_DELAY)
                .await;

            loop {
                let should_continue = this
                    .update(cx, |view, cx| {
                        let moved = view.repeat_scrollbar_track_press(press);
                        if moved {
                            cx.notify();
                        }
                        moved
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }

                cx.background_executor()
                    .timer(SCROLLBAR_TRACK_REPEAT_INTERVAL)
                    .await;
            }
        })
        .detach();
    }

    fn repeat_scrollbar_track_press(&mut self, press: ScrollbarTrackPress) -> bool {
        if self.scrollbar_track_press != Some(press) {
            return false;
        }

        let before = self.viewport_start_row;
        self.scroll_viewport(press.direction);
        self.viewport_start_row != before
    }

    #[cfg(test)]
    fn scroll_viewport_to_scrollbar_row(&mut self, visible_row: usize, thumb_grab_row: usize) {
        self.scroll_viewport_to_scrollbar_y(
            scrollbar_track_top() + LINE_HEIGHT * visible_row,
            LINE_HEIGHT * thumb_grab_row,
        );
    }

    fn scroll_viewport_to_scrollbar_y(&mut self, y: Pixels, thumb_grab_offset: Pixels) {
        let Some(scrollbar) = self.rendered_scrollbar_for_current_view() else {
            return;
        };
        let thumb_rows = scrollbar.thumb_rows();
        let thumb_len = thumb_rows.len().max(1);
        let max_thumb_start = scrollbar.visible_rows.saturating_sub(thumb_len);
        if max_thumb_start == 0 {
            self.viewport_start_row = 0;
            return;
        }

        let max_thumb_top = LINE_HEIGHT * max_thumb_start;
        let thumb_top = y - scrollbar_track_top() - thumb_grab_offset;
        let thumb_top = if thumb_top < Pixels::ZERO {
            Pixels::ZERO
        } else if thumb_top > max_thumb_top {
            max_thumb_top
        } else {
            thumb_top
        };

        let scrollable_rows = scrollbar.total_rows.saturating_sub(scrollbar.visible_rows);
        let scroll_ratio = thumb_top / max_thumb_top;
        self.viewport_start_row = (scroll_ratio * scrollable_rows as f32).round() as usize;
        self.clamp_viewport(Some(DEFAULT_SOFT_WRAP_COLUMN));
    }

    fn reveal_active_cursor(&mut self, soft_wrap_column: Option<usize>) {
        let Ok(cursor) = self.editor.cursor_display_point(soft_wrap_column) else {
            return;
        };
        if cursor.row < self.viewport_start_row {
            self.viewport_start_row = cursor.row;
        } else {
            let viewport_end = self.viewport_start_row + self.viewport_rows.max(1);
            if cursor.row >= viewport_end {
                self.viewport_start_row = cursor.row + 1 - self.viewport_rows.max(1);
            }
        }
        self.clamp_viewport(soft_wrap_column);
    }

    fn clamp_viewport(&mut self, soft_wrap_column: Option<usize>) {
        let row_count = self.editor.display_snapshot(soft_wrap_column).rows().len();
        let max_start = row_count.saturating_sub(self.viewport_rows.max(1));
        self.viewport_start_row = self.viewport_start_row.min(max_start);
    }

    fn rendered_scrollbar_for_current_view(&self) -> Option<RenderedScrollbar> {
        let display = self.editor.display_snapshot(Some(DEFAULT_SOFT_WRAP_COLUMN));
        self.rendered_scrollbar_for_row_count(display.rows().len())
    }

    fn scrollbar_hit_for_position(&self, position: Point<Pixels>) -> Option<ScrollbarHit> {
        let visible_row = self.scrollbar_visible_row_for_position(position)?;
        let scrollbar = self.rendered_scrollbar_for_current_view()?;
        let thumb_rows = scrollbar.thumb_rows();
        Some(ScrollbarHit {
            visible_row,
            thumb_rows,
        })
    }

    fn scrollbar_visible_row_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        let track_left = scrollbar_track_left();
        if position.x < track_left || position.x > track_left + SCROLLBAR_WIDTH {
            return None;
        }

        self.scrollbar_visible_row_for_y(position.y)
    }

    fn scrollbar_visible_row_for_y(&self, y: Pixels) -> Option<usize> {
        let row_origin = EDITOR_PADDING + HEADER_HEIGHT;
        if y < row_origin {
            return None;
        }

        let visible_row = ((y - row_origin) / LINE_HEIGHT).floor() as usize;
        (visible_row < self.viewport_rows.max(1)).then_some(visible_row)
    }

    fn display_point_for_mouse_position(&self, position: Point<Pixels>) -> (usize, usize) {
        let (visible_row, display_column) = visible_display_point_for_mouse_position(position);
        (self.viewport_start_row + visible_row, display_column)
    }

    fn utf8_offset_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (row, display_column) = self.display_point_for_mouse_position(position);
        self.editor.source_offset_for_display_point(
            row,
            display_column,
            Some(DEFAULT_SOFT_WRAP_COLUMN),
        )
    }

    fn bounds_for_utf16_range(
        &self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let text = self.editor.snapshot().text().to_string();
        let range = utf16_range_to_utf8(&text, range_utf16);
        let display = self.editor.display_snapshot(Some(DEFAULT_SOFT_WRAP_COLUMN));
        let row_count = self.viewport_rows.max(1);
        let visible_start = self.viewport_start_row;
        let visible_end = visible_start + row_count;

        if range.is_empty() {
            let point = display.display_point_for_source_offset(range.start);
            if point.row < visible_start || point.row >= visible_end {
                return None;
            }
            return Some(bounds_for_visible_display_range(
                element_bounds,
                point.row - visible_start,
                point.column..point.column,
            ));
        }

        display
            .rows()
            .iter()
            .filter(|row| row.row >= visible_start && row.row < visible_end)
            .find_map(|row| {
                if range.end <= row.source_range.start || range.start >= row.source_range.end {
                    return None;
                }
                let start = range.start.max(row.source_range.start);
                let end = range.end.min(row.source_range.end);
                let start_column =
                    display_column_for_byte_offset(&row.text, start - row.source_range.start);
                let end_column =
                    display_column_for_byte_offset(&row.text, end - row.source_range.start);
                Some(bounds_for_visible_display_range(
                    element_bounds.clone(),
                    row.row - visible_start,
                    start_column..end_column,
                ))
            })
    }

    fn extend_selection_for_drag_position(
        &mut self,
        position: Point<Pixels>,
    ) -> Result<(), String> {
        let (row, column) = self.display_point_for_drag_position(position);
        self.editor.select_display_point_transient(
            row,
            column,
            true,
            Some(DEFAULT_SOFT_WRAP_COLUMN),
        )?;
        self.reveal_active_cursor(Some(DEFAULT_SOFT_WRAP_COLUMN));
        Ok(())
    }

    fn extend_rectangular_selection_for_drag_position(
        &mut self,
        position: Point<Pixels>,
    ) -> Result<(), String> {
        let Some((anchor_row, anchor_column)) = self.rectangular_selection_start else {
            return Ok(());
        };
        let (head_row, head_column) = self.display_point_for_drag_position(position);
        self.editor.select_display_rectangle_transient(
            anchor_row,
            anchor_column,
            head_row,
            head_column,
            Some(DEFAULT_SOFT_WRAP_COLUMN),
        );
        self.reveal_active_cursor(Some(DEFAULT_SOFT_WRAP_COLUMN));
        Ok(())
    }

    fn display_point_for_drag_position(&mut self, position: Point<Pixels>) -> (usize, usize) {
        let (visible_row, display_column) = visible_display_point_for_mouse_position(position);
        let viewport_rows = self.viewport_rows.max(1);

        match self.drag_autoscroll_direction_for_position(position) {
            Some(direction) if direction < 0 => {
                self.scroll_viewport_by_rows(-1);
                (self.viewport_start_row, display_column)
            }
            Some(_) => {
                self.scroll_viewport_by_rows(1);
                (self.viewport_start_row + viewport_rows - 1, display_column)
            }
            None => (self.viewport_start_row + visible_row, display_column),
        }
    }

    fn drag_autoscroll_direction_for_position(&self, position: Point<Pixels>) -> Option<isize> {
        let (visible_row, _) = visible_display_point_for_mouse_position(position);
        let row_origin = EDITOR_PADDING + HEADER_HEIGHT;
        if position.y < row_origin {
            Some(-1)
        } else if visible_row >= self.viewport_rows.max(1) {
            Some(1)
        } else {
            None
        }
    }

    fn dispatch_paste_text(&mut self, text: &str) -> Result<CommandOutcome, String> {
        let before_text = self.editor.snapshot().text().to_string();
        let before_selections = selection_state(&self.editor);
        let selections = self.editor.resolved_selections();
        let is_linewise_paste =
            is_linewise_paste(text, &selections, self.linewise_clipboard_text.as_deref());
        let replacements = if is_linewise_paste {
            let insert_ranges = line_start_ranges_for_offsets(
                &before_text,
                selections.iter().map(|selection| selection.head()),
            );
            self.editor.select_ranges(insert_ranges);
            distributed_paste_replacements(text, self.editor.resolved_selections().len(), true)
        } else {
            distributed_paste_replacements(text, selections.len(), false)
        };

        if let Some(replacements) = replacements {
            self.editor.insert_texts(replacements)?;
        } else {
            self.editor.insert_text(text.to_string())?;
        }
        self.reveal_active_cursor(Some(DEFAULT_SOFT_WRAP_COLUMN));
        self.linewise_clipboard_text = None;

        let after_text = self.editor.snapshot().text().to_string();
        let after_selections = selection_state(&self.editor);
        Ok(CommandOutcome {
            changed_text: before_text != after_text,
            moved_cursor: before_selections != after_selections,
        })
    }

    fn copy_selection_to_clipboard(
        &mut self,
        cx: &mut Context<Self>,
        delete_after_copy: bool,
    ) -> Result<CommandOutcome, String> {
        let selected_text = self.editor.selected_text();
        if selected_text.is_empty() {
            return self.copy_current_lines_to_clipboard(cx, delete_after_copy);
        }

        cx.write_to_clipboard(ClipboardItem::new_string(selected_text));
        self.linewise_clipboard_text = None;

        if delete_after_copy {
            self.dispatch_command(EditorCommand::Delete)
        } else {
            Ok(CommandOutcome {
                changed_text: false,
                moved_cursor: false,
            })
        }
    }

    fn copy_current_lines_to_clipboard(
        &mut self,
        cx: &mut Context<Self>,
        delete_after_copy: bool,
    ) -> Result<CommandOutcome, String> {
        let text = self.editor.snapshot().text().to_string();
        let (clipboard_text, delete_ranges) = current_line_clipboard_text_and_delete_ranges(
            &text,
            self.editor
                .resolved_selections()
                .iter()
                .map(|selection| selection.head()),
        );
        self.linewise_clipboard_text = Some(clipboard_text.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(clipboard_text));

        if delete_after_copy {
            self.editor.select_ranges(delete_ranges);
            self.dispatch_command(EditorCommand::Delete)
        } else {
            Ok(CommandOutcome {
                changed_text: false,
                moved_cursor: false,
            })
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
}

fn display_range_for_source_range(
    row_text: &str,
    row_source_range: &Range<usize>,
    range: Range<usize>,
) -> Option<Range<usize>> {
    if range.end <= row_source_range.start || range.start >= row_source_range.end {
        return None;
    }
    let start = range.start.max(row_source_range.start);
    let end = range.end.min(row_source_range.end);
    (start < end).then_some(
        display_column_for_byte_offset(row_text, start - row_source_range.start)
            ..display_column_for_byte_offset(row_text, end - row_source_range.start),
    )
}

fn current_line_clipboard_text_and_delete_ranges(
    text: &str,
    offsets: impl IntoIterator<Item = usize>,
) -> (String, Vec<Range<usize>>) {
    let mut ranges = offsets
        .into_iter()
        .map(|offset| current_line_ranges(text, offset))
        .collect::<Vec<_>>();
    ranges.sort_by_key(|(copy_range, delete_range)| {
        (
            copy_range.start,
            copy_range.end,
            delete_range.start,
            delete_range.end,
        )
    });
    ranges.dedup();

    let clipboard_text = ranges
        .iter()
        .map(|(copy_range, _)| {
            let mut line = text.get(copy_range.clone()).unwrap_or_default().to_string();
            line.push('\n');
            line
        })
        .collect::<String>();

    let delete_ranges = ranges
        .into_iter()
        .map(|(_, delete_range)| delete_range)
        .collect();

    (clipboard_text, delete_ranges)
}

fn is_linewise_paste(
    text: &str,
    selections: &[Selection],
    linewise_clipboard_text: Option<&str>,
) -> bool {
    linewise_clipboard_text == Some(text)
        && text.ends_with('\n')
        && selections.iter().all(|selection| selection.is_empty())
}

fn distributed_paste_replacements(
    text: &str,
    selection_count: usize,
    linewise: bool,
) -> Option<Vec<String>> {
    if selection_count <= 1 {
        return None;
    }

    let replacements = if linewise {
        text.split_inclusive('\n')
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    } else {
        text.split('\n')
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };

    (replacements.len() == selection_count).then_some(replacements)
}

fn line_start_ranges_for_offsets(
    text: &str,
    offsets: impl IntoIterator<Item = usize>,
) -> Vec<Range<usize>> {
    let mut ranges = offsets
        .into_iter()
        .map(|offset| {
            let line_start = current_line_ranges(text, offset).0.start;
            line_start..line_start
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    ranges.dedup();
    ranges
}

fn current_line_ranges(text: &str, offset: usize) -> (Range<usize>, Range<usize>) {
    let offset = floor_char_boundary(text, offset.min(text.len()));
    let line_start = text[..offset]
        .rfind('\n')
        .map(|offset| offset + 1)
        .unwrap_or(0);
    let next_newline = text[offset..]
        .find('\n')
        .map(|relative_offset| offset + relative_offset);
    let line_end = next_newline.unwrap_or(text.len());
    let copy_range = line_start..line_end;
    let delete_range = if let Some(newline) = next_newline {
        line_start..newline + 1
    } else if line_start > 0 {
        line_start - 1..text.len()
    } else {
        line_start..text.len()
    };
    (copy_range, delete_range)
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
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
        '}' => 2,
        '[' => 3,
        '{' => 4,
        _ => 5,
    }
}

fn availability_label(available: bool) -> &'static str {
    if available { "on" } else { "off" }
}

fn visible_display_point_for_mouse_position(position: Point<Pixels>) -> (usize, usize) {
    let row_origin = scrollbar_track_top();
    let column_origin = EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP;
    let row = if position.y <= row_origin {
        0
    } else {
        ((position.y - row_origin) / LINE_HEIGHT).floor() as usize
    };
    let column = if position.x <= column_origin {
        0
    } else {
        ((position.x - column_origin) / DISPLAY_COLUMN_WIDTH).round() as usize
    };
    (row, column)
}

fn editor_text_width() -> Pixels {
    DISPLAY_COLUMN_WIDTH * DEFAULT_SOFT_WRAP_COLUMN
}

fn scrollbar_track_left() -> Pixels {
    EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + editor_text_width() + SCROLLBAR_GAP
}

fn scrollbar_track_top() -> Pixels {
    EDITOR_PADDING + HEADER_HEIGHT
}

fn bounds_for_visible_display_range(
    element_bounds: Bounds<Pixels>,
    visible_row: usize,
    columns: Range<usize>,
) -> Bounds<Pixels> {
    let column_count = columns.end.saturating_sub(columns.start);
    let width = if column_count == 0 {
        CARET_WIDTH
    } else {
        DISPLAY_COLUMN_WIDTH * column_count
    };
    Bounds {
        origin: Point {
            x: element_bounds.origin.x
                + EDITOR_PADDING
                + LINE_NUMBER_WIDTH
                + CONTENT_GAP
                + DISPLAY_COLUMN_WIDTH * columns.start,
            y: element_bounds.origin.y + EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * visible_row,
        },
        size: size(width, LINE_HEIGHT),
    }
}

fn scroll_rows_for_delta(delta: ScrollDelta) -> isize {
    let rows = match delta {
        ScrollDelta::Lines(delta) => delta.y,
        ScrollDelta::Pixels(delta) => delta.y / LINE_HEIGHT,
    };

    if rows == 0.0 {
        return 0;
    }

    let row_count = rows.abs().ceil() as isize;
    if rows.is_sign_positive() {
        -row_count
    } else {
        row_count
    }
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
        "left" if word_modifiers(modifiers) => Some(EditorCommand::MoveToPreviousWord {
            extend: modifiers.shift,
        }),
        "right" if word_modifiers(modifiers) => Some(EditorCommand::MoveToNextWord {
            extend: modifiers.shift,
        }),
        "left" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveLeft {
            extend: modifiers.shift,
        }),
        "right" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveRight {
            extend: modifiers.shift,
        }),
        "up" if add_caret_modifiers(modifiers) => Some(EditorCommand::AddCaretAbove),
        "down" if add_caret_modifiers(modifiers) => Some(EditorCommand::AddCaretBelow),
        "up" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveUp {
            extend: modifiers.shift,
        }),
        "down" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveDown {
            extend: modifiers.shift,
        }),
        "pageup" if navigation_modifiers(modifiers) => Some(EditorCommand::PageUp {
            extend: modifiers.shift,
        }),
        "pagedown" if navigation_modifiers(modifiers) => Some(EditorCommand::PageDown {
            extend: modifiers.shift,
        }),
        "home" if document_modifiers(modifiers) => Some(EditorCommand::MoveToDocumentStart {
            extend: modifiers.shift,
        }),
        "end" if document_modifiers(modifiers) => Some(EditorCommand::MoveToDocumentEnd {
            extend: modifiers.shift,
        }),
        "home" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveToLineStart {
            extend: modifiers.shift,
        }),
        "end" if navigation_modifiers(modifiers) => Some(EditorCommand::MoveToLineEnd {
            extend: modifiers.shift,
        }),
        "enter" if navigation_modifiers(modifiers) => Some(EditorCommand::InsertText("\n")),
        "a" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::SelectAll),
        "d" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::SelectNextMatch),
        "d" if shortcut_modifiers(modifiers, true) => Some(EditorCommand::SkipActiveMatch),
        "l" if shortcut_modifiers(modifiers, true) => Some(EditorCommand::SelectAllMatches),
        "escape" if navigation_modifiers(modifiers) => Some(EditorCommand::Cancel),
        "c" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Copy),
        "x" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Cut),
        "v" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Paste),
        "z" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Undo),
        "z" if shortcut_modifiers(modifiers, true) => Some(EditorCommand::Redo),
        "y" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::Redo),
        "u" if shortcut_modifiers(modifiers, false) => Some(EditorCommand::UndoSelection),
        "u" if shortcut_modifiers(modifiers, true) => Some(EditorCommand::RedoSelection),
        _ => None,
    }
}

fn navigation_modifiers(modifiers: Modifiers) -> bool {
    !modifiers.control && !modifiers.alt && !modifiers.platform && !modifiers.function
}

fn add_caret_modifiers(modifiers: Modifiers) -> bool {
    modifiers.alt
        && !modifiers.shift
        && !modifiers.control
        && !modifiers.platform
        && !modifiers.function
}

fn word_modifiers(modifiers: Modifiers) -> bool {
    shortcut_navigation_modifiers(modifiers)
}

fn document_modifiers(modifiers: Modifiers) -> bool {
    shortcut_navigation_modifiers(modifiers)
}

fn shortcut_navigation_modifiers(modifiers: Modifiers) -> bool {
    if modifiers.alt || modifiers.function {
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
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.bounds_for_utf16_range(range_utf16, element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let text = self.editor.snapshot().text().to_string();
        let offset = self.utf8_offset_for_mouse_position(point);
        Some(utf8_offset_to_utf16(&text, offset))
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rendered = self.rendered_editor(Some(100));
        let focus_handle = self.render_focus_handle(cx);
        let header_text = rendered.header_text();
        let command_status_text = rendered.command_status_text();
        let is_dirty = rendered.is_dirty;
        let lines = rendered.lines;
        let scrollbar = rendered.scrollbar;
        div()
            .size_full()
            .p_4()
            .bg(rgb(0x1f2328))
            .text_color(rgb(0xd0d7de))
            .key_context("EditorView")
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(Self::handle_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .on_scroll_wheel(cx.listener(Self::handle_scroll_wheel))
            .child(EditorInputElement {
                view: cx.entity(),
                focus_handle: focus_handle.clone(),
            })
            .child(
                div()
                    .flex()
                    .gap_3()
                    .mb_3()
                    .text_color(if is_dirty {
                        rgb(0xffd33d)
                    } else {
                        rgb(0x8b949e)
                    })
                    .child(header_text)
                    .child(command_status_text),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(SCROLLBAR_GAP)
                    .child(div().children(lines.into_iter().map(render_editor_row)))
                    .when_some(scrollbar, |this, scrollbar| {
                        this.child(render_scrollbar(scrollbar))
                    }),
            )
    }
}

fn render_editor_row(line: RenderedLine) -> impl IntoElement {
    div()
        .flex()
        .gap_3()
        .h(LINE_HEIGHT)
        .items_start()
        .line_height(LINE_HEIGHT)
        .child(
            div()
                .w(LINE_NUMBER_WIDTH)
                .h(LINE_HEIGHT)
                .line_height(LINE_HEIGHT)
                .text_color(rgb(0x6e7681))
                .child(if line.continuation {
                    "...".to_string()
                } else {
                    line.line_number.to_string()
                }),
        )
        .child(render_visual_line(line))
}

fn render_visual_line(line: RenderedLine) -> impl IntoElement {
    canvas(
        move |bounds, window, _cx| prepaint_visual_line(line, bounds, window),
        move |bounds, state, window, cx| {
            let VisualLinePaintState {
                line,
                background_quads,
                cursor_quads,
            } = state;
            for quad in background_quads {
                window.paint_quad(quad);
            }
            if let Err(error) = line.paint(bounds.origin, LINE_HEIGHT, window, cx) {
                eprintln!("mini_ui line paint failed: {error}");
            }
            for quad in cursor_quads {
                window.paint_quad(quad);
            }
        },
    )
    .w(editor_text_width())
    .h(LINE_HEIGHT)
    .line_height(LINE_HEIGHT)
}

fn prepaint_visual_line(
    rendered_line: RenderedLine,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> VisualLinePaintState {
    let line = shape_visual_line(&rendered_line.text, window);
    let mut background_quads = Vec::new();
    let mut cursor_quads = Vec::new();

    for overlay in rendered_line.overlays() {
        match overlay.kind {
            RenderedLineOverlayKind::Selection => {
                background_quads.push(pixel_overlay_quad(
                    &rendered_line.text,
                    &line,
                    bounds,
                    overlay,
                    rgb(0x264f78),
                ));
            }
            RenderedLineOverlayKind::Marked => {
                background_quads.push(pixel_overlay_quad(
                    &rendered_line.text,
                    &line,
                    bounds,
                    overlay,
                    rgb(0x3a2f14),
                ));
            }
            RenderedLineOverlayKind::Cursor { active } => {
                cursor_quads.push(pixel_overlay_quad(
                    &rendered_line.text,
                    &line,
                    bounds,
                    overlay,
                    if active { rgb(0xf0f6fc) } else { rgb(0x8b949e) },
                ));
            }
        }
    }

    VisualLinePaintState {
        line,
        background_quads,
        cursor_quads,
    }
}

fn shape_visual_line(text: &str, window: &mut Window) -> ShapedLine {
    let style = window.text_style();
    let shared_text: SharedString = text.to_string().into();
    let run = TextRun {
        len: shared_text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let font_size = style.font_size.to_pixels(window.rem_size());
    window
        .text_system()
        .shape_line(shared_text, font_size, &[run], None)
}

fn pixel_overlay_quad(
    text: &str,
    line: &ShapedLine,
    bounds: Bounds<Pixels>,
    overlay: RenderedLineOverlay,
    color: impl Into<gpui::Background>,
) -> PaintQuad {
    let left = line_x_for_display_column(text, line, overlay.start_column);
    let width = if matches!(overlay.kind, RenderedLineOverlayKind::Cursor { .. }) {
        CARET_WIDTH
    } else {
        let end_column = overlay.start_column + overlay.column_span;
        line_x_for_display_column(text, line, end_column) - left
    };

    let width = if width < CARET_WIDTH {
        CARET_WIDTH
    } else {
        width
    };

    fill(
        Bounds::new(
            point(bounds.left() + left, bounds.top()),
            size(width, LINE_HEIGHT),
        ),
        color,
    )
}

fn line_x_for_display_column(text: &str, line: &ShapedLine, display_column: usize) -> Pixels {
    line.x_for_index(byte_offset_for_display_column_or_end(text, display_column))
}

fn render_scrollbar(scrollbar: RenderedScrollbar) -> impl IntoElement {
    div()
        .w(SCROLLBAR_WIDTH)
        .children((0..scrollbar.visible_rows).map(move |row| {
            let row_state = scrollbar.row_state(row);
            div()
                .w(SCROLLBAR_WIDTH)
                .h(LINE_HEIGHT)
                .bg(if row_state.pressed {
                    rgb(0xf0f6fc)
                } else if row_state.thumb && row_state.hovered {
                    rgb(0x8b949e)
                } else if row_state.thumb {
                    rgb(0x6e7681)
                } else if row_state.hovered {
                    rgb(0x484f58)
                } else {
                    rgb(0x30363d)
                })
        }))
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
    fn empty_selection_copy_cut_ranges_use_current_lines() {
        assert_eq!(
            current_line_clipboard_text_and_delete_ranges("one\ntwo\nthree", [4]),
            ("two\n".to_string(), vec![4..8])
        );
        assert_eq!(
            current_line_clipboard_text_and_delete_ranges("one\ntwo\nthree", [10]),
            ("three\n".to_string(), vec![7..13])
        );
        assert_eq!(
            current_line_clipboard_text_and_delete_ranges("one\ntwo\nthree", [0, 2, 4]),
            ("one\ntwo\n".to_string(), vec![0..4, 4..8])
        );
    }

    #[test]
    fn linewise_paste_targets_current_line_starts() {
        assert_eq!(
            line_start_ranges_for_offsets("one\ntwo\nthree", [10]),
            vec![8..8]
        );
        assert_eq!(
            line_start_ranges_for_offsets("one\ntwo\nthree", [0, 2, 5]),
            vec![0..0, 4..4]
        );
        assert_eq!(line_start_ranges_for_offsets("", [0]), vec![0..0]);
    }

    #[test]
    fn distributed_paste_replacements_match_selection_count() {
        assert_eq!(
            distributed_paste_replacements("alpha\nbeta", 2, false),
            Some(vec!["alpha".to_string(), "beta".to_string()])
        );
        assert_eq!(
            distributed_paste_replacements("alpha\nbeta\n", 2, true),
            Some(vec!["alpha\n".to_string(), "beta\n".to_string()])
        );
        assert_eq!(
            distributed_paste_replacements("alpha\nbeta", 3, false),
            None
        );
        assert_eq!(
            distributed_paste_replacements("alpha\nbeta", 1, false),
            None
        );
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
    fn rendered_editor_hides_scrollbar_when_all_rows_are_visible() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1").into_handle(),
        );
        let view = EditorView::new(editor).with_viewport_rows(2);

        assert_eq!(view.rendered_editor(None).scrollbar, None);
    }

    #[test]
    fn rendered_editor_exposes_scrollbar_for_scrollable_viewport() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        let rendered = view.rendered_editor(None);
        assert_eq!(
            rendered.scrollbar,
            Some(RenderedScrollbar {
                first_visible_row: 0,
                visible_rows: 2,
                total_rows: 5,
                hovered_row: None,
                pressed: false,
            })
        );

        view.dispatch_command(EditorCommand::ScrollDown).unwrap();
        assert_eq!(
            view.rendered_editor(None).scrollbar,
            Some(RenderedScrollbar {
                first_visible_row: 2,
                visible_rows: 2,
                total_rows: 5,
                hovered_row: None,
                pressed: false,
            })
        );
    }

    #[test]
    fn scrollbar_thumb_rows_track_viewport_position() {
        let scrollbar = RenderedScrollbar {
            first_visible_row: 0,
            visible_rows: 2,
            total_rows: 5,
            hovered_row: None,
            pressed: false,
        };
        assert_eq!(scrollbar.thumb_rows(), 0..1);

        let scrollbar = RenderedScrollbar {
            first_visible_row: 2,
            visible_rows: 2,
            total_rows: 5,
            hovered_row: None,
            pressed: false,
        };
        assert_eq!(scrollbar.thumb_rows(), 1..2);

        let scrollbar = RenderedScrollbar {
            first_visible_row: 0,
            visible_rows: 10,
            total_rows: 5,
            hovered_row: None,
            pressed: false,
        };
        assert_eq!(scrollbar.thumb_rows(), 0..10);
    }

    #[test]
    fn scrollbar_row_state_tracks_hover_and_pressed_rows() {
        let scrollbar = RenderedScrollbar {
            first_visible_row: 0,
            visible_rows: 2,
            total_rows: 5,
            hovered_row: Some(0),
            pressed: false,
        };
        assert_eq!(
            scrollbar.row_state(0),
            RenderedScrollbarRowState {
                thumb: true,
                hovered: true,
                pressed: false,
            }
        );
        assert_eq!(
            scrollbar.row_state(1),
            RenderedScrollbarRowState {
                thumb: false,
                hovered: false,
                pressed: false,
            }
        );

        let scrollbar = RenderedScrollbar {
            first_visible_row: 0,
            visible_rows: 2,
            total_rows: 5,
            hovered_row: Some(1),
            pressed: true,
        };
        assert_eq!(
            scrollbar.row_state(1),
            RenderedScrollbarRowState {
                thumb: false,
                hovered: true,
                pressed: true,
            }
        );
    }

    #[test]
    fn scrollbar_hit_testing_uses_render_track_geometry() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4").into_handle(),
        );
        let view = EditorView::new(editor).with_viewport_rows(2);
        let track_x = scrollbar_track_left() + px(3.0);
        let first_row_y = EDITOR_PADDING + HEADER_HEIGHT;
        let second_row_y = first_row_y + LINE_HEIGHT;

        assert_eq!(
            view.scrollbar_hit_for_position(Point {
                x: track_x,
                y: first_row_y,
            }),
            Some(ScrollbarHit {
                visible_row: 0,
                thumb_rows: 0..1,
            })
        );
        assert_eq!(
            view.scrollbar_hit_for_position(Point {
                x: track_x,
                y: second_row_y,
            }),
            Some(ScrollbarHit {
                visible_row: 1,
                thumb_rows: 0..1,
            })
        );
        assert_eq!(
            view.scrollbar_hit_for_position(Point {
                x: scrollbar_track_left() - px(1.0),
                y: first_row_y,
            }),
            None
        );
        assert_eq!(
            view.scrollbar_hit_for_position(Point {
                x: track_x,
                y: second_row_y + LINE_HEIGHT,
            }),
            None
        );
    }

    #[test]
    fn scrollbar_row_mapping_updates_viewport() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        view.scroll_viewport_to_scrollbar_row(1, 0);
        assert_eq!(view.viewport_start_row(), 4);
        assert_eq!(
            view.rendered_editor(None).scrollbar.unwrap().thumb_rows(),
            1..2
        );

        view.scroll_viewport_to_scrollbar_row(0, 0);
        assert_eq!(view.viewport_start_row(), 0);
        assert_eq!(
            view.rendered_editor(None).scrollbar.unwrap().thumb_rows(),
            0..1
        );
    }

    #[test]
    fn scrollbar_pixel_mapping_updates_viewport_between_visible_rows() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(
                BufferId::new(1).unwrap(),
                "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11",
            )
            .into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(4);

        view.scroll_viewport_to_scrollbar_y(scrollbar_track_top() + px(5.0), Pixels::ZERO);
        assert_eq!(view.viewport_start_row(), 1);

        view.scroll_viewport_to_scrollbar_y(scrollbar_track_top() + px(15.0), Pixels::ZERO);
        assert_eq!(view.viewport_start_row(), 3);

        view.scroll_viewport_to_scrollbar_y(scrollbar_track_top() - px(10.0), Pixels::ZERO);
        assert_eq!(view.viewport_start_row(), 0);

        view.scroll_viewport_to_scrollbar_y(scrollbar_track_top() + px(100.0), Pixels::ZERO);
        assert_eq!(view.viewport_start_row(), 8);
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
    fn view_marks_ime_composition_ranges_on_wrapped_rows() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.marked_range = Some(1..4);

        let lines = view.rendered_lines(Some(3));

        assert_eq!(lines[0].marked_ranges, vec![1..3]);
        assert_eq!(lines[1].marked_ranges, vec![0..1]);
        assert_eq!(lines[0].text_with_overlays(), "|a{bc}");
        assert_eq!(lines[1].text_with_overlays(), "{d}ef");
        assert!(lines[0].visual_fragments()[2].marked);
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

    #[gpui::test]
    fn visual_line_caret_uses_shaped_text_widths(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
        );
        let (_view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle)
        });

        cx.update(|window, _cx| {
            let bounds = Bounds::new(Point::default(), size(editor_text_width(), LINE_HEIGHT));
            let state = prepaint_visual_line(
                RenderedLine {
                    line_number: 1,
                    text: "a😀c".to_string(),
                    continuation: false,
                    cursor_columns: vec![2],
                    active_cursor_columns: vec![2],
                    selection_ranges: vec![1..2],
                    marked_ranges: Vec::new(),
                },
                bounds,
                window,
            );

            assert_eq!(state.cursor_quads.len(), 1);
            assert_eq!(state.background_quads.len(), 1);
            let expected_caret_x = line_x_for_display_column("a😀c", &state.line, 2);
            assert_eq!(state.cursor_quads[0].bounds.left(), expected_caret_x);
            assert_eq!(
                state.background_quads[0].bounds.left(),
                line_x_for_display_column("a😀c", &state.line, 1)
            );
            assert!(expected_caret_x > DISPLAY_COLUMN_WIDTH * 2);
        });
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
    fn view_dispatches_enter_up_down_and_select_all_commands() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "ab\ncde").into_handle(),
        );
        let mut view = EditorView::new(editor);

        view.dispatch_command(EditorCommand::MoveDown { extend: false })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|cde");

        view.dispatch_command(EditorCommand::MoveRight { extend: false })
            .unwrap();
        view.dispatch_command(EditorCommand::MoveRight { extend: false })
            .unwrap();
        view.dispatch_command(EditorCommand::MoveUp { extend: true })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "ab|");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[cd]e");

        view.dispatch_command(EditorCommand::InsertText("\n"))
            .unwrap();
        assert_eq!(view.text(), "ab\ne");

        view.dispatch_command(EditorCommand::SelectAll).unwrap();
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[ab]");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[e]|");
    }

    #[test]
    fn view_enter_resets_vertical_movement_column_goal() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abcd\nef\nghij").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.editor.select(3..3);
        view.dispatch_command(EditorCommand::MoveDown { extend: false })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "ef|");
        view.dispatch_command(EditorCommand::InsertText("\n"))
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text_with_overlays())
                .collect::<Vec<_>>(),
            vec![
                "abcd".to_string(),
                "ef".to_string(),
                "|".to_string(),
                "ghij".to_string()
            ]
        );
        view.dispatch_command(EditorCommand::MoveDown { extend: false })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "|ghij");
    }

    #[test]
    fn view_vertical_movement_preserves_column_goal_through_empty_lines() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.editor.select(3..3);

        view.dispatch_command(EditorCommand::MoveDown { extend: false })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");

        view.dispatch_command(EditorCommand::MoveDown { extend: false })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "wxy|z");
    }

    #[test]
    fn view_shift_vertical_movement_extends_selection_through_empty_lines() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.editor.select(3..3);

        view.dispatch_command(EditorCommand::MoveDown { extend: true })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc[d]");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");

        view.dispatch_command(EditorCommand::MoveDown { extend: true })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc[d]");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
        assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[wxy]|z");
    }

    #[test]
    fn view_shift_vertical_movement_extends_reversed_selection_through_empty_lines() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.editor.select(9..9);

        view.dispatch_command(EditorCommand::MoveUp { extend: true })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
        assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[wxy]z");

        view.dispatch_command(EditorCommand::MoveUp { extend: true })
            .unwrap();
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc[|d]");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
        assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[wxy]z");
    }

    #[test]
    fn view_renders_only_visible_viewport_rows() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text)
                .collect::<Vec<_>>(),
            vec!["0".to_string(), "1".to_string()]
        );
        assert_eq!(view.text(), "0\n1\n2\n3\n4");

        view.dispatch_command(EditorCommand::ScrollDown).unwrap();
        assert_eq!(view.viewport_start_row(), 2);
        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text)
                .collect::<Vec<_>>(),
            vec!["2".to_string(), "3".to_string()]
        );

        view.dispatch_command(EditorCommand::ScrollDown).unwrap();
        assert_eq!(view.viewport_start_row(), 3);
        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text)
                .collect::<Vec<_>>(),
            vec!["3".to_string(), "4".to_string()]
        );
    }

    #[test]
    fn view_dispatches_page_up_down_and_reveals_cursor() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        view.dispatch_command(EditorCommand::PageDown { extend: false })
            .unwrap();
        assert_eq!(view.viewport_start_row(), 2);
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|2");

        view.dispatch_command(EditorCommand::PageDown { extend: true })
            .unwrap();
        assert_eq!(view.viewport_start_row(), 4);
        assert_eq!(view.editor.selected_text(), "2\n3\n");
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|4");

        view.dispatch_command(EditorCommand::PageUp { extend: false })
            .unwrap();
        assert_eq!(view.viewport_start_row(), 2);
        assert!(view.editor.selected_text().is_empty());
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|2");
    }

    #[test]
    fn paste_reveals_active_cursor() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        view.dispatch_paste_text("alpha\nbeta\ngamma\n").unwrap();

        assert_eq!(view.viewport_start_row(), 2);
        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text_with_overlays())
                .collect::<Vec<_>>(),
            vec!["gamma".to_string(), "|0".to_string()]
        );
    }

    #[test]
    fn linewise_paste_reveals_active_cursor() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);
        view.editor.select(6..6);
        view.linewise_clipboard_text = Some("alpha\nbeta\n".to_string());

        view.dispatch_paste_text("alpha\nbeta\n").unwrap();

        assert_eq!(view.linewise_clipboard_text, None);
        assert_eq!(view.viewport_start_row(), 4);
        assert_eq!(
            view.rendered_lines(None)
                .into_iter()
                .map(|line| line.text_with_overlays())
                .collect::<Vec<_>>(),
            vec!["beta".to_string(), "|3".to_string()]
        );
    }

    #[test]
    fn view_groups_page_navigation_as_one_selection_history_entry() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        view.dispatch_command(EditorCommand::PageDown { extend: false })
            .unwrap();
        assert_eq!(view.editor.cursor_display_point(None).unwrap().row, 2);

        view.dispatch_command(EditorCommand::UndoSelection).unwrap();
        assert_eq!(view.editor.cursor_display_point(None).unwrap().row, 0);
        assert!(!view.editor.undo_selection());

        view.dispatch_command(EditorCommand::RedoSelection).unwrap();
        assert_eq!(view.editor.cursor_display_point(None).unwrap().row, 2);
    }

    #[test]
    fn drag_beyond_viewport_autoscrolls_and_extends_selection() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);

        view.editor
            .select_display_point(1, 0, false, Some(DEFAULT_SOFT_WRAP_COLUMN))
            .unwrap();
        view.selecting_with_mouse = true;

        let (row, column) = view.display_point_for_drag_position(Point {
            x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
            y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
        });
        view.editor
            .select_display_point(row, column, true, Some(DEFAULT_SOFT_WRAP_COLUMN))
            .unwrap();

        assert_eq!(view.viewport_start_row(), 1);
        assert_eq!(view.editor.selected_text(), "1\n");
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[1]");
        assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|2");
    }

    #[test]
    fn view_dispatches_line_document_and_word_navigation_commands() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one two\nthree_four").into_handle(),
        );
        let mut view = EditorView::new(editor);

        view.dispatch_command(EditorCommand::MoveToNextWord { extend: false })
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)[0].text_with_overlays(),
            "one |two"
        );

        view.dispatch_command(EditorCommand::MoveToLineEnd { extend: true })
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)[0].text_with_overlays(),
            "one [two]|"
        );

        view.dispatch_command(EditorCommand::MoveToDocumentEnd { extend: false })
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)[1].text_with_overlays(),
            "three_four|"
        );

        view.dispatch_command(EditorCommand::MoveToPreviousWord { extend: true })
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)[1].text_with_overlays(),
            "[|three_four]"
        );

        view.dispatch_command(EditorCommand::MoveToDocumentStart { extend: false })
            .unwrap();
        view.dispatch_command(EditorCommand::MoveToLineStart { extend: false })
            .unwrap();
        assert_eq!(
            view.rendered_lines(None)[0].text_with_overlays(),
            "|one two"
        );
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
    fn cancel_clears_selection_and_transient_interaction_state() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdefghi").into_handle(),
        );
        let mut view = EditorView::new(editor);
        view.editor.select_anchor_heads(vec![(0, 3), (8, 5)]);
        view.editor.set_active_selection_index(1).unwrap();
        view.marked_range = Some(1..2);
        view.selecting_with_mouse = true;
        view.drag_autoscroll = Some(DragAutoscroll {
            position: Point {
                x: EDITOR_PADDING,
                y: EDITOR_PADDING,
            },
            generation: 7,
        });
        view.scrollbar_drag = Some(ScrollbarDrag {
            thumb_grab_offset: px(3.0),
        });
        view.scrollbar_track_press = Some(ScrollbarTrackPress {
            direction: 1,
            generation: 11,
        });

        assert_eq!(
            view.dispatch_command(EditorCommand::Cancel).unwrap(),
            CommandOutcome {
                changed_text: false,
                moved_cursor: true,
            }
        );

        assert_eq!(view.marked_range, None);
        assert!(!view.selecting_with_mouse);
        assert_eq!(view.drag_autoscroll, None);
        assert_eq!(view.scrollbar_drag, None);
        assert_eq!(view.scrollbar_track_press, None);
        assert_eq!(view.editor.active_selection_index(), 1);
        assert_eq!(view.editor.selected_text(), "");
        assert_eq!(
            view.rendered_lines(None)[0].text_with_overlays(),
            "abc|de|fghi"
        );

        view.dispatch_command(EditorCommand::UndoSelection).unwrap();
        assert_eq!(view.editor.active_selection_index(), 1);
        assert_eq!(
            view.editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![0..3, 5..8]
        );

        view.dispatch_command(EditorCommand::RedoSelection).unwrap();
        assert_eq!(view.editor.active_selection_index(), 1);
        assert_eq!(
            view.editor
                .resolved_selections()
                .iter()
                .map(Selection::range)
                .collect::<Vec<_>>(),
            vec![3..3, 5..5]
        );
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
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("up").unwrap()),
            Some(EditorCommand::MoveUp { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("shift-down").unwrap()),
            Some(EditorCommand::MoveDown { extend: true })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("alt-up").unwrap()),
            Some(EditorCommand::AddCaretAbove)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("alt-down").unwrap()),
            Some(EditorCommand::AddCaretBelow)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("pageup").unwrap()),
            Some(EditorCommand::PageUp { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("shift-pagedown").unwrap()),
            Some(EditorCommand::PageDown { extend: true })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("enter").unwrap()),
            Some(EditorCommand::InsertText("\n"))
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("escape").unwrap()),
            Some(EditorCommand::Cancel)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-a").unwrap()),
            Some(EditorCommand::SelectAll)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-d").unwrap()),
            Some(EditorCommand::SelectNextMatch)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-d").unwrap()),
            Some(EditorCommand::SkipActiveMatch)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-l").unwrap()),
            Some(EditorCommand::SelectAllMatches)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-c").unwrap()),
            Some(EditorCommand::Copy)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-x").unwrap()),
            Some(EditorCommand::Cut)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-v").unwrap()),
            Some(EditorCommand::Paste)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("home").unwrap()),
            Some(EditorCommand::MoveToLineStart { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("shift-end").unwrap()),
            Some(EditorCommand::MoveToLineEnd { extend: true })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-home").unwrap()),
            Some(EditorCommand::MoveToDocumentStart { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-end").unwrap()),
            Some(EditorCommand::MoveToDocumentEnd { extend: true })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-left").unwrap()),
            Some(EditorCommand::MoveToPreviousWord { extend: false })
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-right").unwrap()),
            Some(EditorCommand::MoveToNextWord { extend: true })
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
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-u").unwrap()),
            Some(EditorCommand::UndoSelection)
        );
        assert_eq!(
            command_for_keystroke(&Keystroke::parse("secondary-shift-u").unwrap()),
            Some(EditorCommand::RedoSelection)
        );
    }

    #[test]
    fn modified_printable_keystrokes_do_not_insert_text() {
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
    fn mouse_positions_map_to_display_points() {
        assert_eq!(
            visible_display_point_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            }),
            (0, 0)
        );
        assert_eq!(
            visible_display_point_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 3,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            }),
            (2, 3)
        );
    }

    #[test]
    fn scroll_wheel_deltas_map_to_viewport_rows() {
        assert_eq!(
            scroll_rows_for_delta(ScrollDelta::Lines(Point::new(0.0, -3.0))),
            3
        );
        assert_eq!(
            scroll_rows_for_delta(ScrollDelta::Lines(Point::new(0.0, 2.0))),
            -2
        );
        assert_eq!(
            scroll_rows_for_delta(ScrollDelta::Pixels(Point::new(
                Pixels::ZERO,
                LINE_HEIGHT * -2.0
            ))),
            2
        );
        assert_eq!(
            scroll_rows_for_delta(ScrollDelta::Pixels(Point::new(Pixels::ZERO, Pixels::ZERO))),
            0
        );
    }

    #[test]
    fn viewport_offsets_mouse_display_points() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(2);
        view.dispatch_command(EditorCommand::ScrollDown).unwrap();

        assert_eq!(
            view.display_point_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            }),
            (2, 0)
        );
    }

    #[test]
    fn mouse_positions_map_to_utf8_safe_source_offsets() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
        );
        let view = EditorView::new(editor);

        assert_eq!(
            view.utf8_offset_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            }),
            5
        );
        assert_eq!(
            view.utf8_offset_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 3,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            }),
            6
        );

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abc\n\nxy").into_handle(),
        );
        let view = EditorView::new(editor);

        assert_eq!(
            view.utf8_offset_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            }),
            4
        );
        assert_eq!(
            view.utf8_offset_for_mouse_position(Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            }),
            7
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
        assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a{b}|c");
    }

    #[test]
    fn input_range_bounds_use_utf16_and_display_columns() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
        );
        let view = EditorView::new(editor);
        let element_bounds = Bounds {
            origin: Point {
                x: px(100.0),
                y: px(50.0),
            },
            size: size(px(400.0), px(200.0)),
        };

        assert_eq!(
            view.bounds_for_utf16_range(1..3, element_bounds.clone()),
            Some(Bounds {
                origin: Point {
                    x: px(100.0)
                        + EDITOR_PADDING
                        + LINE_NUMBER_WIDTH
                        + CONTENT_GAP
                        + DISPLAY_COLUMN_WIDTH,
                    y: px(50.0) + EDITOR_PADDING + HEADER_HEIGHT,
                },
                size: size(DISPLAY_COLUMN_WIDTH, LINE_HEIGHT),
            })
        );
        assert_eq!(
            view.bounds_for_utf16_range(3..3, element_bounds),
            Some(Bounds {
                origin: Point {
                    x: px(100.0)
                        + EDITOR_PADDING
                        + LINE_NUMBER_WIDTH
                        + CONTENT_GAP
                        + DISPLAY_COLUMN_WIDTH * 2,
                    y: px(50.0) + EDITOR_PADDING + HEADER_HEIGHT,
                },
                size: size(CARET_WIDTH, LINE_HEIGHT),
            })
        );
    }

    #[test]
    fn input_range_bounds_track_viewport_rows() {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2").into_handle(),
        );
        let mut view = EditorView::new(editor).with_viewport_rows(1);
        let element_bounds = Bounds {
            origin: Point {
                x: px(10.0),
                y: px(20.0),
            },
            size: size(px(400.0), px(200.0)),
        };

        assert_eq!(
            view.bounds_for_utf16_range(4..5, element_bounds.clone()),
            None
        );

        view.dispatch_command(EditorCommand::ScrollDown).unwrap();
        view.dispatch_command(EditorCommand::ScrollDown).unwrap();

        assert_eq!(
            view.bounds_for_utf16_range(4..5, element_bounds),
            Some(Bounds {
                origin: Point {
                    x: px(10.0) + EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                    y: px(20.0) + EDITOR_PADDING + HEADER_HEIGHT,
                },
                size: size(DISPLAY_COLUMN_WIDTH, LINE_HEIGHT),
            })
        );
    }

    #[gpui::test]
    fn input_handler_character_index_for_point_uses_utf16_offsets(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle)
        });

        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert_eq!(
                    gpui::EntityInputHandler::character_index_for_point(
                        view,
                        Point {
                            x: EDITOR_PADDING
                                + LINE_NUMBER_WIDTH
                                + CONTENT_GAP
                                + DISPLAY_COLUMN_WIDTH * 2,
                            y: EDITOR_PADDING + HEADER_HEIGHT,
                        },
                        window,
                        cx,
                    ),
                    Some(3)
                );
                assert_eq!(
                    gpui::EntityInputHandler::character_index_for_point(
                        view,
                        Point {
                            x: EDITOR_PADDING
                                + LINE_NUMBER_WIDTH
                                + CONTENT_GAP
                                + DISPLAY_COLUMN_WIDTH * 3,
                            y: EDITOR_PADDING + HEADER_HEIGHT,
                        },
                        window,
                        cx,
                    ),
                    Some(4)
                );
            });
        });
    }

    #[gpui::test]
    fn gpui_mouse_click_uses_utf8_safe_display_columns(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "a😀c").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a😀|c");
        });
    }

    #[gpui::test]
    fn gpui_mouse_click_clamps_empty_line_and_line_end(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
        });

        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "xy|");
        });
    }

    #[gpui::test]
    fn gpui_shift_click_clamps_empty_line_and_line_end(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers::default(),
        );
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
        });

        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers::default(),
        );
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");
        });
    }

    #[gpui::test]
    fn gpui_reversed_shift_click_clamps_empty_line_and_line_end(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy\nzz").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            Modifiers::default(),
        );
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");
        });

        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            Modifiers::default(),
        );
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "xy|");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");
        });
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

    #[gpui::test]
    fn gpui_keyboard_navigation_newline_and_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "ab\ncde").into_handle(),
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
        cx.simulate_keystrokes("down right right shift-up enter secondary-a");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "ab\ne");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[ab]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[e]|");
        });
    }

    #[gpui::test]
    fn gpui_vertical_movement_preserves_column_goal_through_empty_lines(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("right right right down down");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcd".to_string(), "".to_string(), "wxy|z".to_string()]
            );
        });
    }

    #[gpui::test]
    fn gpui_shift_vertical_movement_extends_selection_through_empty_lines(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("right right right shift-down shift-down");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[d]".to_string(), "".to_string(), "[wxy]|z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });
    }

    #[gpui::test]
    fn gpui_reversed_shift_vertical_movement_extends_selection_through_empty_lines(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("down down right right right shift-up shift-up");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[|d]".to_string(), "".to_string(), "[wxy]z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });
    }

    #[gpui::test]
    fn gpui_alt_up_down_add_carets_by_display_column(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij").into_handle(),
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
        cx.simulate_keystrokes("down right alt-down alt-up");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcd".to_string(), "e|f".to_string(), "g|hij".to_string()]
            );
            assert_eq!(view.editor.active_selection_index(), 0);
        });
    }

    #[gpui::test]
    fn gpui_select_next_match_adds_matching_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "foo bar foo").into_handle(),
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
        cx.simulate_keystrokes("right secondary-d secondary-d");

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 1);
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[foo]| bar [foo]|"
            );
        });
    }

    #[gpui::test]
    fn gpui_select_all_matches_selects_every_matching_range(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "foo bar foo").into_handle(),
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
        cx.simulate_keystrokes("right secondary-shift-l");

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 0);
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[foo]| bar [foo]|"
            );
        });
    }

    #[gpui::test]
    fn gpui_skip_active_match_replaces_active_match(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "foo foo foo").into_handle(),
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
        cx.simulate_keystrokes("right secondary-d secondary-d secondary-shift-d");

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 1);
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[foo]| foo [foo]|"
            );
        });
    }

    #[gpui::test]
    fn gpui_undo_redo_selection_restores_match_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "foo bar foo").into_handle(),
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
        cx.simulate_keystrokes("right secondary-d secondary-d secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 0);
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[foo]| bar foo"
            );
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 1);
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[foo]| bar [foo]|"
            );
        });
    }

    #[gpui::test]
    fn gpui_undo_redo_selection_restores_navigation_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc").into_handle(),
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
        cx.simulate_keystrokes("right shift-right secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a|bc");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[b]|c");
        });
    }

    #[gpui::test]
    fn gpui_undo_redo_selection_restores_mouse_click(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\ndef").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers::default(),
        );
        cx.simulate_keystrokes("secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "de|f");
        });
    }

    #[gpui::test]
    fn gpui_undo_redo_selection_restores_clamped_shift_click(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
        });

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");
        });
    }

    #[gpui::test]
    fn gpui_undo_redo_selection_restores_empty_line_shift_vertical_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("right right right shift-down shift-down secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[d]".to_string(), "|".to_string(), "wxyz".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n");
        });

        cx.simulate_keystrokes("secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc|d".to_string(), "".to_string(), "wxyz".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[d]".to_string(), "|".to_string(), "wxyz".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[d]".to_string(), "".to_string(), "[wxy]|z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("down down right right right shift-up shift-up secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcd".to_string(), "|".to_string(), "[wxy]z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "\nwxy");
        });

        cx.simulate_keystrokes("secondary-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcd".to_string(), "".to_string(), "wxy|z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcd".to_string(), "|".to_string(), "[wxy]z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "\nwxy");
        });

        cx.simulate_keystrokes("secondary-shift-u");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[|d]".to_string(), "".to_string(), "[wxy]z".to_string()]
            );
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });
    }

    #[gpui::test]
    fn gpui_escape_cancels_active_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcdef").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(1..4));

        cx.simulate_keystrokes("escape");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcdef");
            assert_eq!(view.editor.selected_text(), "");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abcd|ef");
        });
    }

    #[gpui::test]
    fn gpui_keyboard_line_document_and_word_navigation(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one two\nthree_four").into_handle(),
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

        cx.simulate_keystrokes("secondary-right shift-end secondary-end secondary-shift-left");

        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "[|three_four]"
            );
        });

        cx.simulate_keystrokes("home");
        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "|three_four"
            );
        });
    }

    #[gpui::test]
    fn gpui_keyboard_page_navigation_updates_viewport(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        cx.simulate_keystrokes("pagedown shift-pagedown pageup");

        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 2);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|2");
        });
    }

    #[gpui::test]
    fn gpui_scroll_wheel_updates_viewport(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        cx.simulate_event(ScrollWheelEvent {
            position: Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            delta: ScrollDelta::Lines(Point::new(0.0, -1.0)),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });

        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 1);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "1");
        });

        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|1");
        });
    }

    #[gpui::test]
    fn gpui_scrollbar_hover_and_pressed_state_reach_render_model(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        let track_x = scrollbar_track_left() + SCROLLBAR_WIDTH / 2.0;
        let first_row_y = EDITOR_PADDING + HEADER_HEIGHT;

        cx.simulate_mouse_move(
            Point {
                x: track_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            let scrollbar = view.rendered_editor(None).scrollbar.unwrap();
            assert_eq!(scrollbar.hovered_row, Some(0));
            assert!(!scrollbar.pressed);
            assert_eq!(
                scrollbar.row_state(0),
                RenderedScrollbarRowState {
                    thumb: true,
                    hovered: true,
                    pressed: false,
                }
            );
        });

        cx.simulate_mouse_down(
            Point {
                x: track_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            let scrollbar = view.rendered_editor(None).scrollbar.unwrap();
            assert_eq!(scrollbar.hovered_row, Some(0));
            assert!(scrollbar.pressed);
            assert_eq!(
                scrollbar.row_state(0),
                RenderedScrollbarRowState {
                    thumb: true,
                    hovered: true,
                    pressed: true,
                }
            );
        });

        cx.simulate_mouse_up(
            Point {
                x: track_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            let scrollbar = view.rendered_editor(None).scrollbar.unwrap();
            assert_eq!(scrollbar.hovered_row, Some(0));
            assert!(!scrollbar.pressed);
            assert_eq!(view.scrollbar_drag, None);
        });
    }

    #[gpui::test]
    fn gpui_scrollbar_track_clicks_page_viewport(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        let track_x = scrollbar_track_left() + SCROLLBAR_WIDTH / 2.0;
        let first_row_y = EDITOR_PADDING + HEADER_HEIGHT;
        let second_row_y = first_row_y + LINE_HEIGHT;

        cx.simulate_mouse_down(
            Point {
                x: track_x,
                y: second_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: track_x,
                y: second_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 2);
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.scrollbar_drag, None);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "2");
        });

        view.update(cx, |view, _| view.scroll_viewport_to_scrollbar_row(1, 0));
        cx.simulate_mouse_down(
            Point {
                x: track_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: track_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );

        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 2);
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.scrollbar_drag, None);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "2");
        });
    }

    #[gpui::test]
    fn gpui_scrollbar_track_hold_repeats_page_viewport(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(
                BufferId::new(1).unwrap(),
                "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11",
            )
            .into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        let track_x = scrollbar_track_left() + SCROLLBAR_WIDTH / 2.0;
        let second_row_y = EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT;

        cx.simulate_mouse_down(
            Point {
                x: track_x,
                y: second_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 2);
            assert_eq!(
                view.scrollbar_track_press,
                Some(ScrollbarTrackPress {
                    direction: 1,
                    generation: 1,
                })
            );
            assert!(view.rendered_editor(None).scrollbar.unwrap().pressed);
        });

        let press = view.update(cx, |view, _| view.scrollbar_track_press.unwrap());
        view.update(cx, |view, _| {
            assert!(view.repeat_scrollbar_track_press(press));
        });
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 4);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "4");
        });

        view.update(cx, |view, _| {
            assert!(view.repeat_scrollbar_track_press(press));
        });
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 6);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "6");
        });

        cx.simulate_mouse_up(
            Point {
                x: track_x,
                y: second_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.scrollbar_track_press, None);
            assert!(!view.rendered_editor(None).scrollbar.unwrap().pressed);
        });

        view.update(cx, |view, _| {
            assert!(!view.repeat_scrollbar_track_press(press));
        });
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 6);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "6");
        });
    }

    #[gpui::test]
    fn gpui_scrollbar_drag_uses_pixel_position(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(
                BufferId::new(1).unwrap(),
                "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11",
            )
            .into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(4)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        let track_x = scrollbar_track_left() + SCROLLBAR_WIDTH / 2.0;
        let track_top = scrollbar_track_top();

        cx.simulate_mouse_down(
            Point {
                x: track_x,
                y: track_top,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: track_x,
                y: track_top + px(5.0),
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 1);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "1");
        });

        cx.simulate_mouse_move(
            Point {
                x: track_x,
                y: track_top + px(15.0),
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: track_x,
                y: track_top + px(15.0),
            },
            MouseButton::Left,
            Modifiers::default(),
        );

        view.update(cx, |view, _| {
            assert_eq!(view.viewport_start_row(), 3);
            assert_eq!(view.scrollbar_drag, None);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "3");
        });
    }

    #[gpui::test]
    fn gpui_clipboard_shortcuts_copy_cut_and_paste(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "hello world").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(0..5));
        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("hello".to_string())
        );

        cx.simulate_keystrokes("secondary-x");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), " world");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "| world");
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "hello world");
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "[hello]| world"
            );
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), " world");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "| world");
        });

        cx.write_to_clipboard(ClipboardItem::new_string("zed\neditor".to_string()));
        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "zed\neditor world");
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "editor| world"
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), " world");
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "| world");
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "zed\neditor world");
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "editor| world"
            );
        });
    }

    #[gpui::test]
    fn gpui_empty_line_shift_selection_uses_regular_clipboard_paths(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("right right right shift-down shift-down secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("d\n\nwxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcd\n\nwxyz");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });

        cx.simulate_keystrokes("secondary-x");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("d\n\nwxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcz");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc|z");
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcd\n\nwxyz");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[d]".to_string(), "".to_string(), "[wxy]|z".to_string()]
            );
        });

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abcd\n\nwxyz").into_handle(),
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
        cx.simulate_keystrokes("down down right right right shift-up shift-up secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("d\n\nwxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(view.editor.selected_text(), "d\n\nwxy");
        });

        cx.write_to_clipboard(ClipboardItem::new_string("Q\nR".to_string()));
        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcQ\nRz");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcQ".to_string(), "R|z".to_string()]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcd\n\nwxyz");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abc[|d]".to_string(), "".to_string(), "[wxy]z".to_string()]
            );
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcQ\nRz");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["abcQ".to_string(), "R|z".to_string()]
            );
        });
    }

    #[gpui::test]
    fn gpui_empty_selection_copy_and_cut_use_current_line(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one\ntwo\nthree").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(5..5));

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("two\n".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo\nthree");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "t|wo");
        });

        cx.simulate_keystrokes("secondary-x");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("two\n".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\nthree");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|three");
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo\nthree");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[two]");
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\nthree");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|three");
        });
    }

    #[gpui::test]
    fn gpui_multi_cursor_empty_selection_copy_and_cut_deduplicates_lines(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one\ntwo\nthree\nfour").into_handle(),
        );
        editor.select_ranges(vec![1..1, 2..2, 5..5, 16..16]);
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

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("one\ntwo\nfour\n".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo\nthree\nfour");
            assert_eq!(
                view.linewise_clipboard_text,
                Some("one\ntwo\nfour\n".to_string())
            );
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "o|n|e".to_string(),
                    "t|wo".to_string(),
                    "three".to_string(),
                    "fo|ur".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-x");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("one\ntwo\nfour\n".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "three");
            assert_eq!(
                view.linewise_clipboard_text,
                Some("one\ntwo\nfour\n".to_string())
            );
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "||three|"
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo\nthree\nfour");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "[one]".to_string(),
                    "[|two]".to_string(),
                    "|three".to_string(),
                    "[four]|".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "three");
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "||three|"
            );
        });
    }

    #[gpui::test]
    fn gpui_clamped_rectangular_selection_copy_and_cut_use_selected_text(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
        );
        editor.select_display_rectangle(0, 0, 2, 8, None);
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

        view.update(cx, |view, _| {
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
            assert_eq!(view.editor.selected_text(), "abcxy");
        });

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("abcxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.simulate_keystrokes("secondary-x");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("abcxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "\n");
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.editor.selected_text(), "abcxy");
            assert_eq!(view.linewise_clipboard_text, None);
        });
    }

    #[gpui::test]
    fn gpui_clamped_rectangular_selection_paste_replaces_each_range(cx: &mut gpui::TestAppContext) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
        );
        editor.select_display_rectangle(0, 0, 2, 8, None);
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

        cx.write_to_clipboard(ClipboardItem::new_string("Z".to_string()));
        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "Z\nZ\nZ");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["Z|".to_string(), "Z|".to_string(), "Z|".to_string()]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.editor.selected_text(), "abcxy");
        });

        cx.write_to_clipboard(ClipboardItem::new_string("A\nB\nC".to_string()));
        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "A\nB\nC");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["A|".to_string(), "B|".to_string(), "C|".to_string()]
            );
        });
    }

    #[gpui::test]
    fn gpui_linewise_clipboard_paste_inserts_before_current_line(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\none\ntwo\nthree").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(1..1));
        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\n".to_string())
        );

        view.update(cx, |view, _| view.editor.select(15..15));
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\none\ntwo\nalpha\nthree");
            assert_eq!(view.rendered_lines(None)[4].text_with_overlays(), "|three");
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\none\ntwo\nthree");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "|three");
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\none\ntwo\nalpha\nthree");
            assert_eq!(view.rendered_lines(None)[4].text_with_overlays(), "|three");
        });
    }

    #[gpui::test]
    fn gpui_external_trailing_newline_pastes_as_regular_text(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one\ntwo\nthree").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(10..10));
        cx.write_to_clipboard(ClipboardItem::new_string("alpha\n".to_string()));
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo\nthalpha\nree");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "|ree");
        });
    }

    #[gpui::test]
    fn gpui_linewise_clipboard_intent_is_consumed_after_paste(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\none\ntwo\nthree").into_handle(),
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
        view.update(cx, |view, _| view.editor.select(1..1));
        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\n".to_string())
        );

        view.update(cx, |view, _| view.editor.select(15..15));
        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\none\ntwo\nalpha\nthree");
            assert_eq!(view.rendered_lines(None)[4].text_with_overlays(), "|three");
        });

        view.update(cx, |view, _| {
            let three_start = view.text().find("three").expect("three line");
            view.editor.select(three_start + 2..three_start + 2);
        });
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\none\ntwo\nalpha\nthalpha\nree");
            assert_eq!(view.rendered_lines(None)[5].text_with_overlays(), "|ree");
        });
    }

    #[gpui::test]
    fn gpui_multi_cursor_paste_distributes_clipboard_lines(cx: &mut gpui::TestAppContext) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one\ntwo").into_handle(),
        );
        editor.select_ranges(vec![0..0, 4..4]);
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
        cx.write_to_clipboard(ClipboardItem::new_string("alpha\nbeta".to_string()));
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alphaone\nbetatwo");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["alpha|one".to_string(), "beta|two".to_string()]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "one\ntwo");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["|one".to_string(), "|two".to_string()]
            );
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alphaone\nbetatwo");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["alpha|one".to_string(), "beta|two".to_string()]
            );
        });
    }

    #[gpui::test]
    fn gpui_multi_cursor_linewise_paste_distributes_lines_at_line_starts(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\nbeta\none\ntwo\nthree").into_handle(),
        );
        editor.select_ranges(vec![1..1, 7..7]);
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
        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\nbeta\n".to_string())
        );

        view.update(cx, |view, _| {
            view.editor.select_ranges(vec![16..16, 20..20])
        });
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\none\nalpha\ntwo\nbeta\nthree");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "one".to_string(),
                    "alpha".to_string(),
                    "|two".to_string(),
                    "beta".to_string(),
                    "|three".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\none\ntwo\nthree");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "one".to_string(),
                    "|two".to_string(),
                    "|three".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-y");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\none\nalpha\ntwo\nbeta\nthree");
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "one".to_string(),
                    "alpha".to_string(),
                    "|two".to_string(),
                    "beta".to_string(),
                    "|three".to_string(),
                ]
            );
        });
    }

    #[gpui::test]
    fn gpui_clamped_rectangular_carets_linewise_paste_at_line_starts(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\nbeta\ngamma\none\n\nxy").into_handle(),
        );
        editor.select_ranges(vec![0..0, 6..6, 11..11]);
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

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\nbeta\ngamma\n".to_string())
        );

        view.update(cx, |view, _| {
            view.editor.select_display_rectangle(3, 8, 5, 8, None);
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![20..20, 21..21, 24..24]
            );
        });
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(
                view.text(),
                "alpha\nbeta\ngamma\nalpha\none\nbeta\n\ngamma\nxy"
            );
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string(),
                    "alpha".to_string(),
                    "|one".to_string(),
                    "beta".to_string(),
                    "|".to_string(),
                    "gamma".to_string(),
                    "|xy".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\ngamma\none\n\nxy");
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![17..17, 21..21, 22..22]
            );
        });
    }

    #[gpui::test]
    fn gpui_mouse_click_moves_cursor_and_shift_click_extends_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\ndef").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "de|f");
        });

        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        view.update(cx, |view, _| {
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[|bc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[de]f");
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "abc");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "de|f");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "de|f");

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[|bc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[de]f");
        });
    }

    #[gpui::test]
    fn gpui_alt_click_adds_secondary_caret(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\ndef").into_handle(),
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
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            Modifiers::default(),
        );
        cx.simulate_click(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            Modifiers {
                alt: true,
                ..Default::default()
            },
        );

        view.update(cx, |view, _| {
            assert_eq!(view.editor.active_selection_index(), 0);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a|bc");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "de|f");
            assert!(!view.selecting_with_mouse);
        });
    }

    #[gpui::test]
    fn gpui_alt_drag_creates_rectangular_selections(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abcd\nef\nghij").into_handle(),
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
        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 3,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 3,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rectangular_selection_start, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "a[bc]|d".to_string(),
                    "e[f]|".to_string(),
                    "g[hi]|j".to_string()
                ]
            );
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.editor.selected_text(), "");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![1..3, 6..7, 9..11]
            );
        });
    }

    #[gpui::test]
    fn gpui_alt_drag_clamps_empty_line_and_line_end_history(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rectangular_selection_start, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["[abc]|".to_string(), "|".to_string(), "[xy]|".to_string()]
            );
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert_eq!(view.editor.selected_text(), "");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");
        });
    }

    #[gpui::test]
    fn gpui_alt_drag_clamped_selection_copy_and_paste(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("abcxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abcxy\nabcxy\nabcxy");
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "abcxy|".to_string(),
                    "abcxy|".to_string(),
                    "abcxy|".to_string()
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
        });
    }

    #[gpui::test]
    fn gpui_reversed_alt_drag_clamped_selection_copy_and_cut(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
            assert_eq!(view.editor.selected_text(), "abcxy");
        });

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("abcxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.simulate_keystrokes("secondary-x");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("abcxy".to_string())
        );
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "\n");
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "abc\n\nxy");
            assert_eq!(view.editor.selected_text(), "abcxy");
        });
    }

    #[gpui::test]
    fn gpui_alt_drag_clamped_carets_linewise_paste(cx: &mut gpui::TestAppContext) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\nbeta\ngamma\none\n\nxy").into_handle(),
        );
        editor.select_ranges(vec![0..0, 6..6, 11..11]);
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

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\nbeta\ngamma\n".to_string())
        );

        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 5,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 5,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![20..20, 21..21, 24..24]
            );
        });

        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(
                view.text(),
                "alpha\nbeta\ngamma\nalpha\none\nbeta\n\ngamma\nxy"
            );
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string(),
                    "alpha".to_string(),
                    "|one".to_string(),
                    "beta".to_string(),
                    "|".to_string(),
                    "gamma".to_string(),
                    "|xy".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\ngamma\none\n\nxy");
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![17..17, 21..21, 22..22]
            );
        });
    }

    #[gpui::test]
    fn gpui_reversed_alt_drag_clamped_carets_linewise_paste(cx: &mut gpui::TestAppContext) {
        let mut editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\nbeta\ngamma\none\n\nxy").into_handle(),
        );
        editor.select_ranges(vec![0..0, 6..6, 11..11]);
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

        cx.simulate_keystrokes("secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("alpha\nbeta\ngamma\n".to_string())
        );

        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 5,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert_eq!(view.rectangular_selection_start, None);
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![20..20, 21..21, 24..24]
            );
        });

        cx.simulate_keystrokes("secondary-v");
        view.update(cx, |view, _| {
            assert_eq!(
                view.text(),
                "alpha\nbeta\ngamma\nalpha\none\nbeta\n\ngamma\nxy"
            );
            assert_eq!(view.linewise_clipboard_text, None);
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string(),
                    "alpha".to_string(),
                    "|one".to_string(),
                    "beta".to_string(),
                    "|".to_string(),
                    "gamma".to_string(),
                    "|xy".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\ngamma\none\n\nxy");
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![17..17, 21..21, 22..22]
            );
        });
    }

    #[gpui::test]
    fn gpui_alt_drag_external_mismatched_lines_pastes_full_text(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "alpha\nbeta\ngamma\none\n\nxy").into_handle(),
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

        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 5,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 5,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![20..20, 21..21, 24..24]
            );
            assert_eq!(view.linewise_clipboard_text, None);
        });

        cx.write_to_clipboard(ClipboardItem::new_string("red\ngreen".to_string()));
        cx.simulate_keystrokes("secondary-v");

        view.update(cx, |view, _| {
            assert_eq!(
                view.text(),
                "alpha\nbeta\ngamma\nonered\ngreen\nred\ngreen\nxyred\ngreen"
            );
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string(),
                    "onered".to_string(),
                    "green|".to_string(),
                    "red".to_string(),
                    "green|".to_string(),
                    "xyred".to_string(),
                    "green|".to_string(),
                ]
            );
        });

        cx.simulate_keystrokes("secondary-z");
        view.update(cx, |view, _| {
            assert_eq!(view.text(), "alpha\nbeta\ngamma\none\n\nxy");
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![20..20, 21..21, 24..24]
            );
        });
    }

    #[gpui::test]
    fn gpui_reversed_alt_drag_clamps_empty_line_and_line_end_history(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        let modifiers = Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            modifiers,
        );

        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rectangular_selection_start, None);
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
            assert_eq!(
                view.rendered_lines(None)
                    .into_iter()
                    .map(|line| line.text_with_overlays())
                    .collect::<Vec<_>>(),
                vec!["[abc]|".to_string(), "|".to_string(), "[xy]|".to_string()]
            );
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert_eq!(view.editor.selected_text(), "");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.editor
                    .resolved_selections()
                    .iter()
                    .map(Selection::range)
                    .collect::<Vec<_>>(),
                vec![0..3, 4..4, 5..7]
            );
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");
        });
    }

    #[gpui::test]
    fn gpui_mouse_drag_extends_selection(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\ndef").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );

        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[bc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[de]|f");
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert_eq!(view.editor.selected_text(), "");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "a[bc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "[de]|f");
        });
    }

    #[gpui::test]
    fn gpui_mouse_drag_clamps_empty_line_and_line_end_history(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
        });

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abc\n\nxy").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "[abc]");
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]|");
        });
    }

    #[gpui::test]
    fn gpui_reversed_mouse_drag_clamps_empty_line_and_line_end_history(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc\n\nxy\nzz").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "|");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "[xy]");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");
        });

        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(2).unwrap(), "abc\n\nxy\nzz").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 2,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 3,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 8,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT * 2,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "xy|");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[0].text_with_overlays(), "|abc");
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.rendered_lines(None)[1].text_with_overlays(), "");
            assert_eq!(view.rendered_lines(None)[2].text_with_overlays(), "xy|");
            assert_eq!(view.rendered_lines(None)[3].text_with_overlays(), "[zz]");
        });
    }

    #[gpui::test]
    fn gpui_mouse_drag_autoscroll_repeats_while_outside_viewport(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "0\n1\n2\n3\n4\n5").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle).with_viewport_rows(2)
        });

        cx.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.as_ref().unwrap().clone();
            window.focus(&focus_handle);
            window.activate_window();
        });

        let text_x = EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP;
        let first_row_y = EDITOR_PADDING + HEADER_HEIGHT;
        let below_viewport_y = first_row_y + LINE_HEIGHT * 3;

        cx.simulate_mouse_down(
            Point {
                x: text_x,
                y: first_row_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            Point {
                x: text_x,
                y: below_viewport_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );

        let autoscroll = view.update(cx, |view, _| {
            assert!(view.selecting_with_mouse);
            assert_eq!(view.viewport_start_row(), 1);
            assert_eq!(view.editor.selected_text(), "0\n1\n");
            view.drag_autoscroll.unwrap()
        });
        assert_eq!(
            autoscroll,
            DragAutoscroll {
                position: Point {
                    x: text_x,
                    y: below_viewport_y,
                },
                generation: 1,
            }
        );

        view.update(cx, |view, _| {
            assert!(view.repeat_drag_autoscroll(autoscroll.generation).unwrap());
            assert_eq!(view.viewport_start_row(), 2);
            assert_eq!(view.editor.selected_text(), "0\n1\n2\n");
        });

        cx.simulate_mouse_up(
            Point {
                x: text_x,
                y: below_viewport_y,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert!(!view.selecting_with_mouse);
            assert_eq!(view.drag_autoscroll, None);
            assert!(!view.repeat_drag_autoscroll(autoscroll.generation).unwrap());
            assert_eq!(view.viewport_start_row(), 2);
            assert_eq!(view.editor.selected_text(), "0\n1\n2\n");
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(view.editor.selected_text(), "");
            assert_eq!(view.editor.cursor_offset().unwrap(), 0);
            assert!(!view.editor.undo_selection());

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(view.editor.selected_text(), "0\n1\n2\n");
        });
    }

    #[gpui::test]
    fn gpui_double_click_selects_word_and_triple_click_selects_line(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "one two\nthree four").into_handle(),
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
        cx.simulate_mouse_down(
            Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 5,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            MouseButton::Left,
            Modifiers::default(),
        );
        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one t|wo"
            );
        });

        cx.simulate_event(MouseDownEvent {
            position: Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH * 5,
                y: EDITOR_PADDING + HEADER_HEIGHT,
            },
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one [two]|"
            );
        });

        cx.simulate_event(MouseDownEvent {
            position: Point {
                x: EDITOR_PADDING + LINE_NUMBER_WIDTH + CONTENT_GAP + DISPLAY_COLUMN_WIDTH,
                y: EDITOR_PADDING + HEADER_HEIGHT + LINE_HEIGHT,
            },
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 3,
            first_mouse: false,
        });
        view.update(cx, |view, _| {
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "[three four]|"
            );
        });

        view.update(cx, |view, _| {
            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one [two]|"
            );

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one t|wo"
            );

            view.dispatch_command(EditorCommand::UndoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "|one two"
            );

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one t|wo"
            );

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[0].text_with_overlays(),
                "one [two]|"
            );

            view.dispatch_command(EditorCommand::RedoSelection).unwrap();
            assert_eq!(
                view.rendered_lines(None)[1].text_with_overlays(),
                "[three four]|"
            );
        });
    }
}
