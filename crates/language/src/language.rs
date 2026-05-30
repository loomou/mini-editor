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
}

impl Buffer {
    pub fn local(id: BufferId, text: impl Into<String>) -> Self {
        Self {
            text: TextBuffer::new(id, text),
            file: None,
            language_name: None,
            diagnostics: Vec::new(),
            capability: Capability::ReadWrite,
            saved_version: 0,
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

    pub fn save(&mut self) {
        self.saved_version = self.text.snapshot().version();
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
}
