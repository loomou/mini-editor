use language::{BufferHandle, BufferSnapshot, Capability};
use std::collections::BTreeMap;
use std::ops::Range;
use text::{Anchor, BufferId, TextEdit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcerptRange {
    pub context: Range<usize>,
    pub primary: Range<usize>,
}

impl ExcerptRange {
    pub fn new(context: Range<usize>) -> Self {
        Self {
            primary: context.clone(),
            context,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Excerpt {
    pub path_key: String,
    pub buffer_id: BufferId,
    pub range: ExcerptRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiBufferAnchor {
    buffer_id: BufferId,
    anchor_index: usize,
}

impl MultiBufferAnchor {
    pub fn buffer_id(self) -> BufferId {
        self.buffer_id
    }

    pub fn anchor_index(self) -> usize {
        self.anchor_index
    }
}

#[derive(Clone, Debug)]
pub struct MultiBufferSnapshot {
    excerpts: Vec<Excerpt>,
    buffers: BTreeMap<BufferId, BufferSnapshot>,
    text: String,
    capability: Capability,
}

impl MultiBufferSnapshot {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn excerpts(&self) -> &[Excerpt] {
        &self.excerpts
    }

    pub fn buffer(&self, id: BufferId) -> Option<&BufferSnapshot> {
        self.buffers.get(&id)
    }

    pub fn capability(&self) -> Capability {
        self.capability
    }

    pub fn is_dirty(&self) -> bool {
        self.buffers.values().any(BufferSnapshot::is_dirty)
    }

    pub fn anchor_before(&self, offset: usize) -> Option<Anchor> {
        let (buffer_id, buffer_offset) = self.locate(offset)?;
        self.buffers
            .get(&buffer_id)
            .map(|buffer| buffer.anchor_before(buffer_offset))
    }

    pub fn anchor_after(&self, offset: usize) -> Option<Anchor> {
        let (buffer_id, buffer_offset) = self.locate(offset)?;
        self.buffers
            .get(&buffer_id)
            .map(|buffer| buffer.anchor_after(buffer_offset))
    }

    pub fn offset_for_anchor(&self, anchor: Anchor) -> Option<usize> {
        let buffer = self.buffers.get(&anchor.buffer_id())?;
        let buffer_offset = buffer.offset_for_anchor(anchor)?;
        let mut cursor = 0;

        for excerpt in &self.excerpts {
            let len = excerpt.range.context.end - excerpt.range.context.start;
            if excerpt.buffer_id == anchor.buffer_id()
                && buffer_offset >= excerpt.range.context.start
                && buffer_offset <= excerpt.range.context.end
            {
                return Some(cursor + buffer_offset - excerpt.range.context.start);
            }

            cursor += len;
            if cursor < self.text.len() {
                cursor += 1;
            }
        }

        None
    }

    fn locate(&self, offset: usize) -> Option<(BufferId, usize)> {
        let mut cursor = 0;
        for excerpt in &self.excerpts {
            let len = excerpt.range.context.end - excerpt.range.context.start;
            if offset <= cursor + len {
                return Some((
                    excerpt.buffer_id,
                    excerpt.range.context.start + offset - cursor,
                ));
            }
            cursor += len;
            if cursor < self.text.len() {
                cursor += 1;
            }
        }
        None
    }
}

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
        let text = self
            .excerpts
            .iter()
            .filter_map(|excerpt| {
                let buffer = buffers.get(&excerpt.buffer_id)?;
                Some(buffer.text.text_slice(excerpt.range.context.clone()))
            })
            .collect::<Vec<_>>()
            .join("\n");

        MultiBufferSnapshot {
            excerpts: self.excerpts.clone(),
            buffers,
            text,
            capability: self.capability,
        }
    }

    pub fn track_anchor_before(&mut self, offset: usize) -> Option<MultiBufferAnchor> {
        let anchor = self.snapshot().anchor_before(offset)?;
        self.track_anchor(anchor)
    }

    pub fn track_anchor_after(&mut self, offset: usize) -> Option<MultiBufferAnchor> {
        let anchor = self.snapshot().anchor_after(offset)?;
        self.track_anchor(anchor)
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
        if !self.capability.editable() {
            return Err("multibuffer is read-only".to_string());
        }

        let snapshot = self.snapshot();
        let start = snapshot
            .locate(range.start)
            .ok_or_else(|| "edit start is outside the multibuffer".to_string())?;
        let end = snapshot
            .locate(range.end)
            .ok_or_else(|| "edit end is outside the multibuffer".to_string())?;
        if start.0 != end.0 {
            return Err("this teaching step only supports edits inside one excerpt".to_string());
        }

        let replacement = replacement.into();
        let replacement_len = replacement.len();
        let deleted_len = end.1 - start.1;

        let buffer = self
            .buffers
            .get_mut(&start.0)
            .ok_or_else(|| "buffer disappeared".to_string())?;
        buffer.borrow_mut().edit(TextEdit {
            range: start.1..end.1,
            replacement,
        })?;

        if replacement_len >= deleted_len {
            let delta = replacement_len - deleted_len;
            for excerpt in &mut self.excerpts {
                if excerpt.buffer_id == start.0
                    && excerpt.range.context.start <= start.1
                    && excerpt.range.context.end >= end.1
                {
                    excerpt.range.context.end += delta;
                    excerpt.range.primary.end += delta;
                }
            }
        } else {
            let delta = deleted_len - replacement_len;
            for excerpt in &mut self.excerpts {
                if excerpt.buffer_id == start.0
                    && excerpt.range.context.start <= start.1
                    && excerpt.range.context.end >= end.1
                {
                    excerpt.range.context.end -= delta;
                    excerpt.range.primary.end -= delta;
                }
            }
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
        Some(MultiBufferAnchor {
            buffer_id,
            anchor_index,
        })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use language::{Buffer, SourceFile};

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

        multibuffer.edit(6..11, "zed").unwrap();
        assert_eq!(multibuffer.snapshot().text(), "hello zed");

        assert!(multibuffer.undo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "hello world");

        assert!(multibuffer.redo().unwrap());
        assert_eq!(multibuffer.snapshot().text(), "hello zed");
    }

    #[test]
    fn snapshot_reports_dirty_when_any_backing_buffer_is_dirty() {
        let buffer = Buffer::from_file(
            BufferId::new(1).unwrap(),
            SourceFile::new("src/main.rs"),
            "hello world",
        );
        let mut multibuffer = MultiBuffer::singleton("src/main.rs", buffer.into_handle());

        assert!(!multibuffer.snapshot().is_dirty());

        multibuffer.edit(6..11, "zed").unwrap();

        assert!(multibuffer.snapshot().is_dirty());
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

    #[test]
    fn snapshot_creates_and_resolves_anchors_in_singleton_excerpt() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world").into_handle();
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer.clone());
        let anchor = multibuffer.snapshot().anchor_after(6).unwrap();
        let tracked_anchor = buffer.borrow_mut().track_anchor(anchor);

        multibuffer.edit(6..11, "zed").unwrap();

        let anchor = buffer.borrow().tracked_anchor(tracked_anchor).unwrap();
        assert_eq!(multibuffer.snapshot().offset_for_anchor(anchor), Some(9));
    }

    #[test]
    fn snapshot_resolves_anchors_inside_later_excerpts() {
        let first = Buffer::local(BufferId::new(1).unwrap(), "alpha").into_handle();
        let second = Buffer::local(BufferId::new(2).unwrap(), "beta").into_handle();
        let mut multibuffer = MultiBuffer::new(Capability::ReadWrite);
        multibuffer.add_excerpt("first", first, ExcerptRange::new(0..5));
        multibuffer.add_excerpt("second", second.clone(), ExcerptRange::new(0..4));

        let snapshot = multibuffer.snapshot();
        assert_eq!(snapshot.text(), "alpha\nbeta");

        let anchor = snapshot.anchor_before(8).unwrap();

        assert_eq!(anchor.buffer_id(), BufferId::new(2).unwrap());
        assert_eq!(
            second.borrow().snapshot().offset_for_anchor(anchor),
            Some(2)
        );
        assert_eq!(snapshot.offset_for_anchor(anchor), Some(8));
    }

    #[test]
    fn snapshot_does_not_resolve_anchor_outside_any_excerpt() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world").into_handle();
        let mut multibuffer = MultiBuffer::new(Capability::ReadWrite);
        multibuffer.add_excerpt("scratch", buffer.clone(), ExcerptRange::new(0..5));

        let anchor = buffer.borrow().snapshot().anchor_after(8);

        assert_eq!(multibuffer.snapshot().offset_for_anchor(anchor), None);
    }

    #[test]
    fn tracked_anchor_moves_through_singleton_edits() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world").into_handle();
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer);
        let anchor = multibuffer.track_anchor_after(6).unwrap();

        multibuffer.edit(6..11, "zed").unwrap();

        assert_eq!(multibuffer.offset_for_tracked_anchor(anchor), Some(9));
    }

    #[test]
    fn tracked_anchor_can_start_in_later_excerpt() {
        let first = Buffer::local(BufferId::new(1).unwrap(), "alpha").into_handle();
        let second = Buffer::local(BufferId::new(2).unwrap(), "beta").into_handle();
        let mut multibuffer = MultiBuffer::new(Capability::ReadWrite);
        multibuffer.add_excerpt("first", first, ExcerptRange::new(0..5));
        multibuffer.add_excerpt("second", second, ExcerptRange::new(0..4));

        let anchor = multibuffer.track_anchor_before(8).unwrap();

        assert_eq!(anchor.buffer_id(), BufferId::new(2).unwrap());
        assert_eq!(anchor.anchor_index(), 0);
        assert_eq!(multibuffer.offset_for_tracked_anchor(anchor), Some(8));
    }

    #[test]
    fn tracking_anchor_outside_multibuffer_returns_none() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello").into_handle();
        let mut multibuffer = MultiBuffer::singleton("scratch", buffer);

        assert_eq!(multibuffer.track_anchor_after(99), None);
    }
}
