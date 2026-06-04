use crate::{BufferSnapshot, Capability, Diagnostic, SourceFile};
use std::cell::RefCell;
use std::rc::Rc;
use text::{Anchor, Buffer as TextBuffer, BufferId, TextEdit};

pub type BufferHandle = Rc<RefCell<Buffer>>;

#[derive(Debug)]
pub struct Buffer {
    text: TextBuffer,
    file: Option<SourceFile>,
    language_name: Option<String>,
    diagnostics: Vec<Diagnostic>,
    capability: Capability,
    saved_version: u64,
    saved_text: String,
}

impl Buffer {
    pub fn local(id: BufferId, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            text: TextBuffer::new(id, text.clone()),
            file: None,
            language_name: None,
            diagnostics: Vec::new(),
            capability: Capability::ReadWrite,
            saved_version: 0,
            saved_text: text,
        }
    }

    pub fn from_file(id: BufferId, file: SourceFile, text: impl Into<String>) -> Self {
        Self {
            file: Some(file),
            ..Self::local(id, text)
        }
    }

    pub fn into_handle(self) -> BufferHandle {
        Rc::new(RefCell::new(self))
    }

    pub fn id(&self) -> BufferId {
        self.text.id()
    }

    pub fn version(&self) -> u64 {
        self.text.version()
    }

    pub fn track_anchor(&mut self, anchor: Anchor) -> usize {
        self.text.track_anchor(anchor)
    }

    pub fn tracked_anchor(&self, index: usize) -> Option<Anchor> {
        self.text.tracked_anchor(index)
    }

    pub fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot {
            text: self.text.snapshot(),
            file: self.file.clone(),
            language_name: self.language_name.clone(),
            diagnostics: self.diagnostics.clone(),
            capability: self.capability,
            saved_version: self.saved_version,
        }
    }

    pub fn set_language(&mut self, language_name: impl Into<String>) {
        self.language_name = Some(language_name.into());
    }

    pub fn set_capability(&mut self, capability: Capability) {
        self.capability = capability;
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
    }

    pub fn edit(&mut self, edit: TextEdit) -> Result<(), String> {
        if !self.capability.editable() {
            return Err("buffer is read-only".to_string());
        }
        self.text.edit(edit);
        Ok(())
    }

    pub fn edit_group(&mut self, edits: Vec<TextEdit>) -> Result<(), String> {
        if !self.capability.editable() {
            return Err("buffer is read-only".to_string());
        }
        self.text.edit_group(edits);
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, String> {
        if !self.capability.editable() {
            return Err("buffer is read-only".to_string());
        }
        Ok(self.text.undo())
    }

    pub fn redo(&mut self) -> Result<bool, String> {
        if !self.capability.editable() {
            return Err("buffer is read-only".to_string());
        }
        Ok(self.text.redo())
    }

    pub fn can_undo(&self) -> bool {
        self.capability.editable() && self.text.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.capability.editable() && self.text.can_redo()
    }

    pub fn save(&mut self) {
        let snapshot = self.text.snapshot();
        self.saved_text = snapshot.text();
        self.saved_version = snapshot.version();
    }

    pub fn revert_to_saved(&mut self) -> Result<bool, String> {
        if !self.capability.editable() {
            return Err("buffer is read-only".to_string());
        }

        let snapshot = self.text.snapshot();
        if snapshot.text() == self.saved_text {
            return Ok(false);
        }

        self.text.edit(TextEdit {
            range: 0..snapshot.len(),
            replacement: self.saved_text.clone().into(),
        });
        self.saved_version = self.text.snapshot().version();
        Ok(true)
    }

    pub fn reload_saved_text(&mut self, text: impl Into<String>) -> bool {
        let text = text.into();
        let snapshot = self.text.snapshot();

        if snapshot.text() == text {
            self.saved_text = text;
            self.saved_version = snapshot.version();
            return false;
        }

        self.text.edit(TextEdit {
            range: 0..snapshot.len(),
            replacement: text.clone().into(),
        });
        self.saved_text = text;
        self.saved_version = self.text.snapshot().version();
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{Buffer, SourceFile};
    use text::{Anchor, Bias, BufferId, TextEdit};

    #[test]
    fn buffer_tracks_dirty_state_against_saved_version() {
        let mut buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "fn main() {}",
        );

        assert!(!buffer.snapshot().is_dirty());
        buffer
            .edit(TextEdit {
                range: 0..0,
                replacement: "// hi\n".to_string().into(),
            })
            .unwrap();
        assert!(buffer.snapshot().is_dirty());

        buffer.save();
        assert!(!buffer.snapshot().is_dirty());
    }

    #[test]
    fn undo_keeps_dirty_state_relative_to_saved_version() {
        let mut buffer = Buffer::local(BufferId::new(1).unwrap(), "hello");
        buffer.save();

        assert!(!buffer.can_undo());
        assert!(!buffer.can_redo());
        buffer
            .edit(TextEdit {
                range: 5..5,
                replacement: " zed".to_string().into(),
            })
            .unwrap();
        assert!(buffer.snapshot().is_dirty());
        assert!(buffer.can_undo());
        assert!(!buffer.can_redo());

        assert!(buffer.undo().unwrap());
        assert_eq!(buffer.snapshot().text.text(), "hello");
        assert!(buffer.snapshot().is_dirty());
        assert!(!buffer.can_undo());
        assert!(buffer.can_redo());
    }

    #[test]
    fn reverting_to_saved_text_restores_clean_buffer_contents() {
        let mut buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "hello world",
        );

        buffer
            .edit(TextEdit {
                range: 6..11,
                replacement: "zed".to_string().into(),
            })
            .unwrap();
        assert_eq!(buffer.snapshot().text.text(), "hello zed");
        assert!(buffer.snapshot().is_dirty());

        assert!(buffer.revert_to_saved().unwrap());

        assert_eq!(buffer.snapshot().text.text(), "hello world");
        assert!(!buffer.snapshot().is_dirty());
    }

    #[test]
    fn reverting_unchanged_buffer_is_a_noop() {
        let mut buffer = Buffer::local(BufferId::new(1).unwrap(), "hello");

        assert!(!buffer.revert_to_saved().unwrap());
        assert_eq!(buffer.snapshot().text.text(), "hello");
        assert!(!buffer.snapshot().is_dirty());
    }

    #[test]
    fn reloading_saved_text_replaces_contents_and_marks_buffer_clean() {
        let mut buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "hello world",
        );

        assert!(buffer.reload_saved_text("hello zed"));

        assert_eq!(buffer.snapshot().text.text(), "hello zed");
        assert!(!buffer.snapshot().is_dirty());
    }

    #[test]
    fn reloading_same_saved_text_is_a_noop() {
        let mut buffer = Buffer::local(BufferId::new(1).unwrap(), "hello");

        assert!(!buffer.reload_saved_text("hello"));

        assert_eq!(buffer.snapshot().text.text(), "hello");
        assert!(!buffer.snapshot().is_dirty());
    }

    #[test]
    fn snapshot_creates_and_resolves_text_anchors() {
        let mut buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let anchor = buffer.snapshot().anchor_after(6);
        let tracked_anchor = buffer.track_anchor(anchor);

        buffer
            .edit(TextEdit {
                range: 6..11,
                replacement: "zed".to_string().into(),
            })
            .unwrap();

        let snapshot = buffer.snapshot();
        assert_eq!(
            snapshot.offset_for_anchor(buffer.tracked_anchor(tracked_anchor).unwrap()),
            Some(9)
        );
    }

    #[test]
    fn snapshot_rejects_anchors_from_another_buffer() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello");
        let other_anchor = Anchor::new(BufferId::new(2).unwrap(), 1, Bias::Left);

        assert_eq!(buffer.snapshot().offset_for_anchor(other_anchor), None);
    }
}
