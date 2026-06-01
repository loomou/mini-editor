use crate::{
    Bounds, CARET_WIDTH, EditorInputElement, EditorView, LINE_HEIGHT, LINE_NUMBER_WIDTH, PaintQuad,
    Pixels, RenderedLine, RenderedLineOverlay, RenderedLineOverlayKind, RenderedScrollbar,
    SCROLLBAR_GAP, SCROLLBAR_WIDTH, ShapedLine, SharedString, TextRun, Window,
    byte_offset_for_display_column_or_end, editor_text_width,
};
use gpui::{IntoElement, MouseButton, Render, canvas, div, fill, point, prelude::*, rgb, size};

pub(crate) struct VisualLinePaintState {
    pub(crate) line: ShapedLine,
    pub(crate) background_quads: Vec<PaintQuad>,
    pub(crate) cursor_quads: Vec<PaintQuad>,
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_cursor_blink(cx);
        let rendered = self.rendered_editor(Some(100));
        let focus_handle = self.render_focus_handle(cx);
        let header_text = rendered.header_text();
        let command_status_text = rendered.command_status_text();
        let is_dirty = rendered.is_dirty;
        let lines = rendered.lines;
        let scrollbar = rendered.scrollbar;
        let cursor_blink_visible = self.cursor_blink_visible;
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
                    .child(
                        div().children(
                            lines
                                .into_iter()
                                .map(move |line| render_editor_row(line, cursor_blink_visible)),
                        ),
                    )
                    .when_some(scrollbar, |this, scrollbar| {
                        this.child(render_scrollbar(scrollbar))
                    }),
            )
    }
}

fn render_editor_row(line: RenderedLine, active_cursor_visible: bool) -> impl IntoElement {
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
        .child(render_visual_line(line, active_cursor_visible))
}

fn render_visual_line(line: RenderedLine, active_cursor_visible: bool) -> impl IntoElement {
    canvas(
        move |bounds, window, _cx| {
            prepaint_visual_line(line, bounds, window, active_cursor_visible)
        },
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

pub(crate) fn prepaint_visual_line(
    rendered_line: RenderedLine,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    active_cursor_visible: bool,
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
                if active && !active_cursor_visible {
                    continue;
                }
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

pub(crate) fn line_x_for_display_column(
    text: &str,
    line: &ShapedLine,
    display_column: usize,
) -> Pixels {
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

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

    #[gpui::test]
    fn cursor_blink_timer_toggles_active_cursor_visibility(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc").into_handle(),
        );
        let (view, cx) = cx.add_window_view(|window, cx| {
            let focus_handle = cx.focus_handle();
            window.focus(&focus_handle);
            EditorView::with_focus(editor, focus_handle)
        });

        view.update(cx, |view, cx| {
            view.restart_cursor_blink(cx);
            assert!(view.cursor_blink_visible);
        });

        cx.executor().advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();

        view.update(cx, |view, _cx| {
            assert!(!view.cursor_blink_visible);
        });

        view.update(cx, |view, cx| {
            view.restart_cursor_blink(cx);
            assert!(view.cursor_blink_visible);
        });
    }

    #[gpui::test]
    fn hidden_active_cursor_leaves_secondary_cursors_visible(cx: &mut gpui::TestAppContext) {
        let editor = EditorModel::for_buffer(
            "scratch",
            Buffer::local(BufferId::new(1).unwrap(), "abc").into_handle(),
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
                    text: "abc".to_string(),
                    continuation: false,
                    cursor_columns: vec![1, 2],
                    active_cursor_columns: vec![1],
                    selection_ranges: Vec::new(),
                    marked_ranges: Vec::new(),
                },
                bounds,
                window,
                false,
            );

            assert_eq!(state.cursor_quads.len(), 1);
            assert_eq!(
                state.cursor_quads[0].bounds.left(),
                line_x_for_display_column("abc", &state.line, 2)
            );
        });
    }
}
