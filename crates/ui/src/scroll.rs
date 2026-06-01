use crate::{
    Context, DEFAULT_SOFT_WRAP_COLUMN, EDITOR_PADDING, EditorView, HEADER_HEIGHT, LINE_HEIGHT,
    Pixels, Point, RenderedScrollbar, SCROLLBAR_TRACK_REPEAT_INITIAL_DELAY,
    SCROLLBAR_TRACK_REPEAT_INTERVAL, SCROLLBAR_WIDTH, ScrollWheelEvent, ScrollbarHit,
    ScrollbarTrackPress, Window, scroll_rows_for_delta, scrollbar_track_left, scrollbar_track_top,
};

impl EditorView {
    pub(crate) fn rendered_scrollbar_for_row_count(
        &self,
        row_count: usize,
    ) -> Option<RenderedScrollbar> {
        let visible_rows = self.viewport_rows.max(1);
        (row_count > visible_rows).then_some(RenderedScrollbar {
            first_visible_row: self.viewport_start_row,
            visible_rows,
            total_rows: row_count,
            hovered_row: self.hovered_scrollbar_row.filter(|row| *row < visible_rows),
            pressed: self.scrollbar_drag.is_some() || self.scrollbar_track_press.is_some(),
        })
    }

    pub(crate) fn handle_scroll_wheel(
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

    pub(crate) fn scroll_viewport(&mut self, direction: isize) {
        let page_rows = self.viewport_rows.max(1);
        self.scroll_viewport_by_rows(direction.saturating_mul(page_rows as isize));
    }

    pub(crate) fn scroll_viewport_by_rows(&mut self, rows: isize) {
        self.viewport_start_row = self.viewport_start_row.saturating_add_signed(rows);
        self.clamp_viewport(Some(DEFAULT_SOFT_WRAP_COLUMN));
    }

    pub(crate) fn start_scrollbar_track_repeat(
        &mut self,
        direction: isize,
        cx: &mut Context<Self>,
    ) {
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

    pub(crate) fn repeat_scrollbar_track_press(&mut self, press: ScrollbarTrackPress) -> bool {
        if self.scrollbar_track_press != Some(press) {
            return false;
        }

        let before = self.viewport_start_row;
        self.scroll_viewport(press.direction);
        self.viewport_start_row != before
    }

    #[cfg(test)]
    pub(crate) fn scroll_viewport_to_scrollbar_row(
        &mut self,
        visible_row: usize,
        thumb_grab_row: usize,
    ) {
        self.scroll_viewport_to_scrollbar_y(
            scrollbar_track_top() + LINE_HEIGHT * visible_row,
            LINE_HEIGHT * thumb_grab_row,
        );
    }

    pub(crate) fn scroll_viewport_to_scrollbar_y(&mut self, y: Pixels, thumb_grab_offset: Pixels) {
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

    pub(crate) fn reveal_active_cursor(&mut self, soft_wrap_column: Option<usize>) {
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

    pub(crate) fn clamp_viewport(&mut self, soft_wrap_column: Option<usize>) {
        let row_count = self.editor.display_snapshot(soft_wrap_column).rows().len();
        let max_start = row_count.saturating_sub(self.viewport_rows.max(1));
        self.viewport_start_row = self.viewport_start_row.min(max_start);
    }

    pub(crate) fn rendered_scrollbar_for_current_view(&self) -> Option<RenderedScrollbar> {
        let display = self.editor.display_snapshot(Some(DEFAULT_SOFT_WRAP_COLUMN));
        self.rendered_scrollbar_for_row_count(display.rows().len())
    }

    pub(crate) fn scrollbar_hit_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<ScrollbarHit> {
        let visible_row = self.scrollbar_visible_row_for_position(position)?;
        let scrollbar = self.rendered_scrollbar_for_current_view()?;
        let thumb_rows = scrollbar.thumb_rows();
        Some(ScrollbarHit {
            visible_row,
            thumb_rows,
        })
    }

    pub(crate) fn scrollbar_visible_row_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let track_left = scrollbar_track_left();
        if position.x < track_left || position.x > track_left + SCROLLBAR_WIDTH {
            return None;
        }

        self.scrollbar_visible_row_for_y(position.y)
    }

    pub(crate) fn scrollbar_visible_row_for_y(&self, y: Pixels) -> Option<usize> {
        let row_origin = EDITOR_PADDING + HEADER_HEIGHT;
        if y < row_origin {
            return None;
        }

        let visible_row = ((y - row_origin) / LINE_HEIGHT).floor() as usize;
        (visible_row < self.viewport_rows.max(1)).then_some(visible_row)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

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
}
