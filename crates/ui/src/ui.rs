use editor::EditorModel;
use std::ops::Range;

#[derive(Debug)]
pub struct EditorView {
    editor: EditorModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertChar(char),
    Backspace,
    Delete,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedLine {
    pub line_number: usize,
    pub text: String,
    pub continuation: bool,
    pub cursor_columns: Vec<usize>,
    pub selection_ranges: Vec<Range<usize>>,
}

impl EditorView {
    pub fn new(editor: EditorModel) -> Self {
        Self { editor }
    }

    pub fn dispatch_command(&mut self, command: EditorCommand) -> Result<CommandOutcome, String> {
        let before_text = self.editor.snapshot().text().to_string();
        let before_cursor = self.editor.cursor_offset().ok();

        match command {
            EditorCommand::InsertChar(character) => self.editor.insert_char(character)?,
            EditorCommand::Backspace => {
                self.editor.backspace()?;
            }
            EditorCommand::Delete => {
                self.editor.delete()?;
            }
            EditorCommand::MoveLeft { extend } => self.editor.move_left(extend)?,
            EditorCommand::MoveRight { extend } => self.editor.move_right(extend)?,
        }

        let after_text = self.editor.snapshot().text().to_string();
        let after_cursor = self.editor.cursor_offset().ok();
        Ok(CommandOutcome {
            changed_text: before_text != after_text,
            moved_cursor: before_cursor != after_cursor,
        })
    }

    pub fn rendered_editor(&self, soft_wrap_column: Option<usize>) -> RenderedEditor {
        RenderedEditor {
            title: self.editor.title(),
            is_dirty: self.editor.is_dirty(),
            lines: self.rendered_lines(soft_wrap_column),
        }
    }

    pub fn rendered_lines(&self, soft_wrap_column: Option<usize>) -> Vec<RenderedLine> {
        let display = self.editor.display_snapshot(soft_wrap_column);
        let cursor = self.editor.cursor_display_point(soft_wrap_column).ok();

        display
            .rows()
            .iter()
            .map(|row| RenderedLine {
                line_number: row.row + 1,
                text: row.text.clone(),
                continuation: row.continuation,
                cursor_columns: cursor
                    .filter(|cursor| cursor.row == row.row)
                    .map(|cursor| vec![cursor.column])
                    .unwrap_or_default(),
                selection_ranges: self
                    .editor
                    .selections()
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
