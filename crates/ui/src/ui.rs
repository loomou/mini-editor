pub(crate) use editor::{EditorModel, Selection, SelectionHistoryCheckpoint};
pub(crate) use std::time::Duration;

#[cfg(test)]
pub(crate) use gpui::MouseButton;
pub(crate) use gpui::{
    App, Application, Bounds, ClipboardItem, Context, FocusHandle, KeyDownEvent, Keystroke,
    Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, ShapedLine, SharedString, TextRun, UTF16Selection, Window, WindowBounds,
    WindowOptions, px, size,
};

pub(crate) const DEFAULT_SOFT_WRAP_COLUMN: usize = 100;
pub(crate) const EDITOR_PADDING: Pixels = px(16.0);
pub(crate) const HEADER_HEIGHT: Pixels = px(28.0);
pub(crate) const LINE_HEIGHT: Pixels = px(24.0);
pub(crate) const LINE_NUMBER_WIDTH: Pixels = px(48.0);
pub(crate) const CONTENT_GAP: Pixels = px(12.0);
pub(crate) const DISPLAY_COLUMN_WIDTH: Pixels = px(8.0);
pub(crate) const CARET_WIDTH: Pixels = px(2.0);
pub(crate) const SCROLLBAR_WIDTH: Pixels = px(6.0);
pub(crate) const SCROLLBAR_GAP: Pixels = px(8.0);
pub(crate) const DEFAULT_VIEWPORT_ROWS: usize = 20;
pub(crate) const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);
pub(crate) const SCROLLBAR_TRACK_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(350);
pub(crate) const SCROLLBAR_TRACK_REPEAT_INTERVAL: Duration = Duration::from_millis(75);
pub(crate) const DRAG_AUTOSCROLL_REPEAT_INTERVAL: Duration = Duration::from_millis(75);
