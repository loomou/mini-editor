use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use text::{Buffer as TextBuffer, BufferId, BufferSnapshot as TextSnapshot, TextEdit};

pub type BufferHandle = Rc<RefCell<Buffer>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    ReadWrite,
    ReadOnly,
}

impl Capability {
    pub fn editable(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    path: PathBuf,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub range: std::ops::Range<usize>,
}

#[derive(Clone, Debug)]
pub struct BufferSnapshot {
    pub text: TextSnapshot,
    pub file: Option<SourceFile>,
    pub language_name: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub capability: Capability,
    pub saved_version: u64,
}

impl BufferSnapshot {
    pub fn id(&self) -> BufferId {
        self.text.id()
    }

    pub fn is_dirty(&self) -> bool {
        self.text.version() != self.saved_version
    }
}

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
            replacement: self.saved_text.clone(),
        });
        self.saved_version = self.text.snapshot().version();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                replacement: "// hi\n".to_string(),
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

        buffer
            .edit(TextEdit {
                range: 5..5,
                replacement: " zed".to_string(),
            })
            .unwrap();
        assert!(buffer.snapshot().is_dirty());

        assert!(buffer.undo().unwrap());
        assert_eq!(buffer.snapshot().text.text(), "hello");
        assert!(buffer.snapshot().is_dirty());
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
                replacement: "zed".to_string(),
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
}
