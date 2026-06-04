use crate::{Excerpt, ExcerptRange, MultiBufferAnchor, MultiBufferEdit, MultiBufferSnapshot};
use language::{BufferHandle, Capability};
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;
use text::{Anchor, BufferId, TextEdit};

#[derive(Debug)]
pub struct MultiBuffer {
    buffers: BTreeMap<BufferId, BufferHandle>,
    excerpts: Vec<Excerpt>,
    capability: Capability,
}

impl MultiBuffer {
    pub fn new(capability: Capability) -> Self {
        Self {
            buffers: BTreeMap::new(),
            excerpts: Vec::new(),
            capability,
        }
    }

    pub fn singleton(path_key: impl Into<String>, buffer: BufferHandle) -> Self {
        let snapshot = buffer.borrow().snapshot();
        let len = snapshot.text.len();
        let mut multibuffer = Self::new(snapshot.capability);
        multibuffer.add_excerpt(path_key, buffer, ExcerptRange::new(0..len));
        multibuffer
    }

    pub fn add_excerpt(
        &mut self,
        path_key: impl Into<String>,
        buffer: BufferHandle,
        range: ExcerptRange,
    ) {
        let buffer_id = buffer.borrow().id();
        self.buffers.insert(buffer_id, buffer);
        self.excerpts.push(Excerpt {
            path_key: path_key.into(),
            buffer_id,
            range,
        });
    }

    pub fn snapshot(&self) -> MultiBufferSnapshot {
        let buffers = self
            .buffers
            .iter()
            .map(|(id, buffer)| (*id, buffer.borrow().snapshot()))
            .collect::<BTreeMap<_, _>>();

        MultiBufferSnapshot::new(self.excerpts.clone(), buffers, self.capability)
    }

    pub fn text_version_key(&self) -> Vec<(u64, u64)> {
        self.buffers
            .iter()
            .map(|(id, buffer)| (id.get(), buffer.borrow().version()))
            .collect()
    }

    pub fn excerpt_version_key(&self) -> Vec<(u64, usize, usize, usize, usize)> {
        self.excerpts
            .iter()
            .map(|excerpt| {
                (
                    excerpt.buffer_id.get(),
                    excerpt.range.context.start,
                    excerpt.range.context.end,
                    excerpt.range.primary.start,
                    excerpt.range.primary.end,
                )
            })
            .collect()
    }

    pub fn track_anchor_before(&mut self, offset: usize) -> Option<MultiBufferAnchor> {
        let anchor = self.snapshot().anchor_before(offset)?;
        self.track_anchor(anchor)
    }

    pub fn track_anchor_after(&mut self, offset: usize) -> Option<MultiBufferAnchor> {
        let anchor = self.snapshot().anchor_after(offset)?;
        self.track_anchor(anchor)
    }

    pub fn update_anchor_before(
        &mut self,
        handle: MultiBufferAnchor,
        offset: usize,
    ) -> Option<MultiBufferAnchor> {
        self.update_anchor(handle, self.snapshot().anchor_before(offset)?)
    }

    pub fn update_anchor_after(
        &mut self,
        handle: MultiBufferAnchor,
        offset: usize,
    ) -> Option<MultiBufferAnchor> {
        self.update_anchor(handle, self.snapshot().anchor_after(offset)?)
    }

    pub fn anchor_for_handle(&self, anchor: MultiBufferAnchor) -> Option<Anchor> {
        self.buffers
            .get(&anchor.buffer_id)?
            .borrow()
            .tracked_anchor(anchor.anchor_index)
    }

    pub fn offset_for_tracked_anchor(&self, anchor: MultiBufferAnchor) -> Option<usize> {
        let anchor = self.anchor_for_handle(anchor)?;
        self.snapshot().offset_for_anchor(anchor)
    }

    pub fn edit(
        &mut self,
        range: Range<usize>,
        replacement: impl Into<String>,
    ) -> Result<(), String> {
        self.edit_group(vec![MultiBufferEdit {
            range,
            replacement: Rc::<str>::from(replacement.into()),
        }])
    }

    pub fn edit_group(&mut self, edits: Vec<MultiBufferEdit>) -> Result<(), String> {
        if !self.capability.editable() {
            return Err("multibuffer is read-only".to_string());
        }
        if edits.is_empty() {
            return Ok(());
        }

        let snapshot = self.snapshot();
        let mut buffer_id = None;
        let mut text_edits = Vec::new();
        let mut excerpt_updates = Vec::new();

        for edit in edits {
            let start = snapshot
                .locate(edit.range.start)
                .ok_or_else(|| "edit start is outside the multibuffer".to_string())?;
            let end = snapshot
                .locate(edit.range.end)
                .ok_or_else(|| "edit end is outside the multibuffer".to_string())?;
            if start.0 != end.0 {
                return Err("this teaching step only supports edits inside one excerpt".to_string());
            }
            if let Some(buffer_id) = buffer_id {
                if buffer_id != start.0 {
                    return Err(
                        "this teaching step only supports grouped edits in one buffer".to_string(),
                    );
                }
            } else {
                buffer_id = Some(start.0);
            }

            let replacement_len = edit.replacement.len();
            let deleted_len = end.1 - start.1;
            text_edits.push(TextEdit {
                range: start.1..end.1,
                replacement: edit.replacement,
            });
            excerpt_updates.push((start.0, start.1..end.1, replacement_len, deleted_len));
        }

        let buffer_id = buffer_id.ok_or_else(|| "edit group has no edits".to_string())?;

        let buffer = self
            .buffers
            .get_mut(&buffer_id)
            .ok_or_else(|| "buffer disappeared".to_string())?;
        buffer.borrow_mut().edit_group(text_edits)?;

        for (buffer_id, range, replacement_len, deleted_len) in excerpt_updates {
            self.adjust_excerpts_after_edit(buffer_id, range, replacement_len, deleted_len);
        }

        Ok(())
    }

    fn track_anchor(&mut self, anchor: Anchor) -> Option<MultiBufferAnchor> {
        let buffer_id = anchor.buffer_id();
        let anchor_index = self
            .buffers
            .get(&buffer_id)?
            .borrow_mut()
            .track_anchor(anchor);
        Some(MultiBufferAnchor::new(buffer_id, anchor_index))
    }

    fn update_anchor(
        &mut self,
        handle: MultiBufferAnchor,
        anchor: Anchor,
    ) -> Option<MultiBufferAnchor> {
        if handle.buffer_id == anchor.buffer_id()
            && self
                .buffers
                .get(&handle.buffer_id)?
                .borrow_mut()
                .update_tracked_anchor(handle.anchor_index, anchor)
        {
            Some(handle)
        } else {
            self.track_anchor(anchor)
        }
    }

    pub fn undo(&mut self) -> Result<bool, String> {
        if !self.capability.editable() {
            return Err("multibuffer is read-only".to_string());
        }
        let (buffer_id, buffer) = self.singleton_buffer()?;
        let changed = buffer.borrow_mut().undo()?;
        if changed {
            self.refresh_singleton_excerpt(buffer_id);
        }
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, String> {
        if !self.capability.editable() {
            return Err("multibuffer is read-only".to_string());
        }
        let (buffer_id, buffer) = self.singleton_buffer()?;
        let changed = buffer.borrow_mut().redo()?;
        if changed {
            self.refresh_singleton_excerpt(buffer_id);
        }
        Ok(changed)
    }

    pub fn can_undo(&self) -> bool {
        self.capability.editable()
            && self
                .singleton_buffer()
                .map(|(_, buffer)| buffer.borrow().can_undo())
                .unwrap_or(false)
    }

    pub fn can_redo(&self) -> bool {
        self.capability.editable()
            && self
                .singleton_buffer()
                .map(|(_, buffer)| buffer.borrow().can_redo())
                .unwrap_or(false)
    }

    pub fn refresh(&mut self) {
        if self.buffers.len() == 1 && self.excerpts.len() == 1 {
            if let Some(buffer_id) = self.excerpts.first().map(|excerpt| excerpt.buffer_id) {
                self.refresh_singleton_excerpt(buffer_id);
            }
        }
    }

    fn singleton_buffer(&self) -> Result<(BufferId, BufferHandle), String> {
        if self.buffers.len() != 1 || self.excerpts.len() != 1 {
            return Err("this teaching step only supports singleton undo/redo".to_string());
        }
        let (buffer_id, buffer) = self
            .buffers
            .iter()
            .next()
            .ok_or_else(|| "multibuffer has no buffers".to_string())?;
        Ok((*buffer_id, buffer.clone()))
    }

    fn refresh_singleton_excerpt(&mut self, buffer_id: BufferId) {
        let Some(excerpt) = self.excerpts.first_mut() else {
            return;
        };
        if excerpt.buffer_id != buffer_id {
            return;
        }
        let Some(buffer) = self.buffers.get(&buffer_id) else {
            return;
        };
        let len = buffer.borrow().snapshot().text.len();
        excerpt.range = ExcerptRange::new(0..len);
    }

    fn adjust_excerpts_after_edit(
        &mut self,
        buffer_id: BufferId,
        range: Range<usize>,
        replacement_len: usize,
        deleted_len: usize,
    ) {
        if replacement_len >= deleted_len {
            let delta = replacement_len - deleted_len;
            for excerpt in &mut self.excerpts {
                if excerpt.buffer_id == buffer_id
                    && excerpt.range.context.start <= range.start
                    && excerpt.range.context.end >= range.end
                {
                    excerpt.range.context.end += delta;
                    excerpt.range.primary.end += delta;
                }
            }
        } else {
            let delta = deleted_len - replacement_len;
            for excerpt in &mut self.excerpts {
                if excerpt.buffer_id == buffer_id
                    && excerpt.range.context.start <= range.start
                    && excerpt.range.context.end >= range.end
                {
                    excerpt.range.context.end -= delta;
                    excerpt.range.primary.end -= delta;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{MultiBuffer, MultiBufferEdit};
    use language::{Buffer, SourceFile};
    use text::BufferId;

    #[test]
    fn singleton_exposes_buffer_text() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "fn main() {}",
        );
        let multibuffer = MultiBuffer::singleton("src/main.rs", buffer.into_handle());

        assert_eq!(multibuffer.snapshot().text(), "fn main() {}");
    }

    #[test]
    fn edits_route_back_to_underlying_buffer() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());

        multibuffer.edit(6..11, "zed").unwrap();

        assert_eq!(multibuffer.snapshot().text(), "hello zed");
    }

    #[test]
    fn undo_and_redo_route_to_singleton_buffer() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());

        assert!(!multibuffer.can_undo());
        assert!(!multibuffer.can_redo());
        multibuffer.edit(6..11, "zed").unwrap();
        assert_eq!(multibuffer.snapshot().text(), "hello zed");
        assert!(multibuffer.can_undo());
        assert!(!multibuffer.can_redo());

        assert!(multibuffer.undo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "hello world");
        assert!(!multibuffer.can_undo());
        assert!(multibuffer.can_redo());

        assert!(multibuffer.redo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "hello zed");
        assert!(multibuffer.can_undo());
        assert!(!multibuffer.can_redo());
    }

    #[test]
    fn grouped_edits_undo_and_redo_as_one_entry() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());

        multibuffer
            .edit_group(vec![
                MultiBufferEdit {
                    range: 8..13,
                    replacement: "3".to_string().into(),
                },
                MultiBufferEdit {
                    range: 0..3,
                    replacement: "1".to_string().into(),
                },
            ])
            .unwrap();

        assert_eq!(multibuffer.snapshot().text(), "1 two 3");
        assert!(multibuffer.undo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "one two three");
        assert!(multibuffer.redo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "1 two 3");
    }

    #[test]
    fn refresh_updates_singleton_excerpt_after_external_buffer_change() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "old",
        )
        .into_handle();
        let mut multibuffer = MultiBuffer::singleton("src/main.rs", buffer.clone());

        buffer.borrow_mut().reload_saved_text("new longer text");
        multibuffer.refresh();

        assert_eq!(multibuffer.snapshot().text(), "new longer text");
    }
}
