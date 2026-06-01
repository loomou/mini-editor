use crate::{
    Context, DEFAULT_SOFT_WRAP_COLUMN, DRAG_AUTOSCROLL_REPEAT_INTERVAL, DragAutoscroll,
    EDITOR_PADDING, EditorView, HEADER_HEIGHT, LINE_HEIGHT, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollbarDrag, Window, scrollbar_track_top, selection_state,
    visible_display_point_for_mouse_position,
};

impl EditorView {
    pub(crate) fn handle_mouse_down(
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
                self.restart_cursor_blink(cx);
                cx.notify();
                cx.stop_propagation();
            }
            Err(error) => eprintln!("mini_ui mouse selection failed: {error}"),
        }
    }

    pub(crate) fn handle_mouse_move(
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
                self.restart_cursor_blink(cx);
                cx.notify();
                cx.stop_propagation();
            }
            Err(error) => eprintln!("mini_ui mouse drag selection failed: {error}"),
        }
    }

    pub(crate) fn handle_mouse_up(
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

    pub(crate) fn update_drag_autoscroll(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
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

    pub(crate) fn repeat_drag_autoscroll(&mut self, generation: u64) -> Result<bool, String> {
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

    pub(crate) fn display_point_for_mouse_position(
        &self,
        position: Point<Pixels>,
    ) -> (usize, usize) {
        let (visible_row, display_column) = visible_display_point_for_mouse_position(position);
        (self.viewport_start_row + visible_row, display_column)
    }

    pub(crate) fn utf8_offset_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (row, display_column) = self.display_point_for_mouse_position(position);
        self.editor.source_offset_for_display_point(
            row,
            display_column,
            Some(DEFAULT_SOFT_WRAP_COLUMN),
        )
    }

    pub(crate) fn extend_selection_for_drag_position(
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

    pub(crate) fn extend_rectangular_selection_for_drag_position(
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

    pub(crate) fn display_point_for_drag_position(
        &mut self,
        position: Point<Pixels>,
    ) -> (usize, usize) {
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

    pub(crate) fn drag_autoscroll_direction_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<isize> {
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
}

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

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
