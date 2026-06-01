use crate::{
    App, Bounds, Context, DEFAULT_SOFT_WRAP_COLUMN, EditorView, Pixels, UTF16Selection, Window,
    bounds_for_visible_display_range, display_column_for_byte_offset,
};
use gpui::{
    Element, ElementId, ElementInputHandler, Entity, FocusHandle, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Style,
};
use std::ops::Range;

impl EditorView {
    pub(crate) fn bounds_for_utf16_range(
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

    pub(crate) fn active_input_range(&self) -> Range<usize> {
        self.editor
            .resolved_selections()
            .get(self.editor.active_selection_index())
            .map(|selection| selection.range())
            .unwrap_or(0..0)
    }

    pub(crate) fn replace_input_text(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
    ) -> Result<(), String> {
        let range = self.input_range_from_utf16(range_utf16);
        self.marked_range = None;
        self.editor.select(range);
        self.editor.insert_text(new_text.to_string())
    }

    pub(crate) fn replace_and_mark_input_text(
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

    pub(crate) fn input_range_from_utf16(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| utf16_range_to_utf8(self.editor.snapshot().text(), range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.active_input_range())
    }
}

pub(crate) struct EditorInputElement {
    pub(crate) view: Entity<EditorView>,
    pub(crate) focus_handle: FocusHandle,
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

pub(crate) fn utf16_range_to_utf8(text: &str, range_utf16: Range<usize>) -> Range<usize> {
    let start = utf16_offset_to_utf8(text, range_utf16.start);
    let end = utf16_offset_to_utf8(text, range_utf16.end);
    start.min(end)..start.max(end)
}

pub(crate) fn utf16_offset_to_utf8(text: &str, offset_utf16: usize) -> usize {
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

pub(crate) fn utf8_range_to_utf16(text: &str, range_utf8: Range<usize>) -> Range<usize> {
    utf8_offset_to_utf16(text, range_utf8.start)..utf8_offset_to_utf16(text, range_utf8.end)
}

pub(crate) fn utf8_offset_to_utf16(text: &str, offset_utf8: usize) -> usize {
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
            Ok(()) => {
                self.restart_cursor_blink(cx);
                cx.notify();
            }
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
            Ok(()) => {
                self.restart_cursor_blink(cx);
                cx.notify();
            }
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

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

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
