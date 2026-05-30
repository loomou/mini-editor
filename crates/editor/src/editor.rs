use language::Buffer;
use multibuffer::{MultiBuffer, MultiBufferSnapshot};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub range: Range<usize>,
}

#[derive(Debug)]
pub struct EditorModel {
    buffer: MultiBuffer,
    selections: Vec<Selection>,
}

impl EditorModel {
    pub fn for_buffer(path_key: impl Into<String>, buffer: Buffer) -> Self {
        Self {
            buffer: MultiBuffer::singleton(path_key, buffer),
            selections: vec![Selection { range: 0..0 }],
        }
    }

    pub fn snapshot(&self) -> MultiBufferSnapshot {
        self.buffer.snapshot()
    }

    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    pub fn select(&mut self, range: Range<usize>) {
        self.selections = vec![Selection { range }];
    }

    pub fn insert_text(&mut self, text: impl Into<String>) -> Result<(), String> {
        let selection = self
            .selections
            .first()
            .ok_or_else(|| "editor has no active selection".to_string())?
            .clone();
        let replacement = text.into();
        let cursor = selection.range.start + replacement.len();
        self.buffer.edit(selection.range, replacement)?;
        self.selections = vec![Selection {
            range: cursor..cursor,
        }];
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text::BufferId;

    #[test]
    fn insertion_replaces_active_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer);

        editor.select(6..11);
        editor.insert_text("zed").unwrap();

        assert_eq!(editor.snapshot().text(), "hello zed");
        assert_eq!(editor.selections()[0].range, 9..9);
    }
}
