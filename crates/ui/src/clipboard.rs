use crate::{
    ClipboardItem, CommandOutcome, Context, DEFAULT_SOFT_WRAP_COLUMN, EditorCommand, EditorView,
    Selection, floor_char_boundary, selection_state,
};
use std::ops::Range;

impl EditorView {
    pub(crate) fn dispatch_paste_text(&mut self, text: &str) -> Result<CommandOutcome, String> {
        let before_text_version = self.editor.text_version_key();
        let before_selections = selection_state(&self.editor);
        let selections = self.editor.resolved_selections();
        let is_linewise_paste =
            is_linewise_paste(text, &selections, self.linewise_clipboard_text.as_deref());
        let replacements = if is_linewise_paste {
            let before_text = self.editor.snapshot().text().to_string();
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

        let after_text_version = self.editor.text_version_key();
        let after_selections = selection_state(&self.editor);
        Ok(CommandOutcome {
            changed_text: before_text_version != after_text_version,
            moved_cursor: before_selections != after_selections,
        })
    }

    pub(crate) fn copy_selection_to_clipboard(
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

    pub(crate) fn copy_current_lines_to_clipboard(
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
}

pub(crate) fn current_line_clipboard_text_and_delete_ranges(
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

pub(crate) fn is_linewise_paste(
    text: &str,
    selections: &[Selection],
    linewise_clipboard_text: Option<&str>,
) -> bool {
    linewise_clipboard_text == Some(text)
        && text.ends_with('\n')
        && selections.iter().all(|selection| selection.is_empty())
}

pub(crate) fn distributed_paste_replacements(
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

pub(crate) fn line_start_ranges_for_offsets(
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

pub(crate) fn current_line_ranges(text: &str, offset: usize) -> (Range<usize>, Range<usize>) {
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

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

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
}
