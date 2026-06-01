use crate::Excerpt;
use language::{BufferSnapshot, Capability};
use std::collections::BTreeMap;
use text::{Anchor, BufferId};

#[derive(Clone, Debug)]
pub struct MultiBufferSnapshot {
    pub(crate) excerpts: Vec<Excerpt>,
    pub(crate) buffers: BTreeMap<BufferId, BufferSnapshot>,
    pub(crate) text: String,
    pub(crate) capability: Capability,
}

impl MultiBufferSnapshot {
    pub(crate) fn new(
        excerpts: Vec<Excerpt>,
        buffers: BTreeMap<BufferId, BufferSnapshot>,
        text: String,
        capability: Capability,
    ) -> Self {
        Self {
            excerpts,
            buffers,
            text,
            capability,
        }
    }

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

    pub(crate) fn locate(&self, offset: usize) -> Option<(BufferId, usize)> {
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

#[cfg(test)]
mod tests {
    use crate::{ExcerptRange, MultiBuffer};
    use language::{Buffer, Capability, SourceFile};
    use text::BufferId;

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
}
