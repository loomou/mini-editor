use crate::{
    Context, DEFAULT_SOFT_WRAP_COLUMN, EditorModel, EditorView, KeyDownEvent, Keystroke, Modifiers,
    Window,
};

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

impl EditorView {
    pub(crate) fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = command_for_keystroke(&event.keystroke) {
            match self.dispatch_context_command(command, cx) {
                Ok(_) => {
                    self.restart_cursor_blink(cx);
                    cx.notify();
                    cx.stop_propagation();
                }
                Err(error) => eprintln!("mini_ui command failed: {error}"),
            }
        }
    }

    pub fn dispatch_command(&mut self, command: EditorCommand) -> Result<CommandOutcome, String> {
        let before_text_version = self.editor.text_version_key();
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
        let after_text_version = self.editor.text_version_key();
        let after_selections = selection_state(&self.editor);
        Ok(CommandOutcome {
            changed_text: before_text_version != after_text_version,
            moved_cursor: before_selections != after_selections,
        })
    }

    pub(crate) fn dispatch_context_command(
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

    pub(crate) fn move_page(&mut self, direction: isize, extend: bool) -> Result<(), String> {
        let page_rows = isize::try_from(self.viewport_rows.max(1)).unwrap_or(isize::MAX);
        let row_delta = if direction < 0 { -page_rows } else { page_rows };
        self.editor
            .move_display_rows(row_delta, extend, Some(DEFAULT_SOFT_WRAP_COLUMN))?;
        self.scroll_viewport(direction);
        Ok(())
    }
}

pub(crate) fn selection_state(editor: &EditorModel) -> Vec<(usize, usize, usize, bool)> {
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

pub(crate) fn command_for_keystroke(keystroke: &Keystroke) -> Option<EditorCommand> {
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

pub(crate) fn navigation_modifiers(modifiers: Modifiers) -> bool {
    !modifiers.control && !modifiers.alt && !modifiers.platform && !modifiers.function
}

pub(crate) fn add_caret_modifiers(modifiers: Modifiers) -> bool {
    modifiers.alt
        && !modifiers.shift
        && !modifiers.control
        && !modifiers.platform
        && !modifiers.function
}

pub(crate) fn word_modifiers(modifiers: Modifiers) -> bool {
    shortcut_navigation_modifiers(modifiers)
}

pub(crate) fn document_modifiers(modifiers: Modifiers) -> bool {
    shortcut_navigation_modifiers(modifiers)
}

pub(crate) fn shortcut_navigation_modifiers(modifiers: Modifiers) -> bool {
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

pub(crate) fn shortcut_modifiers(modifiers: Modifiers, shift: bool) -> bool {
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

#[cfg(test)]
mod tests {
    use crate::*;
    use language::Buffer;
    use text::BufferId;

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
                true,
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
}
