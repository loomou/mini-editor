use crate::{Capability, Diagnostic, SourceFile};
use text::{Anchor, BufferId, BufferSnapshot as TextSnapshot};

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

    pub fn anchor_before(&self, offset: usize) -> Anchor {
        self.text.anchor_before(offset)
    }

    pub fn anchor_after(&self, offset: usize) -> Anchor {
        self.text.anchor_after(offset)
    }

    pub fn offset_for_anchor(&self, anchor: Anchor) -> Option<usize> {
        self.text.offset_for_anchor(anchor)
    }

    pub fn is_dirty(&self) -> bool {
        self.text.version() != self.saved_version
    }
}
