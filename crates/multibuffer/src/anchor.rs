use text::BufferId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiBufferAnchor {
    pub(crate) buffer_id: BufferId,
    pub(crate) anchor_index: usize,
}

impl MultiBufferAnchor {
    pub(crate) fn new(buffer_id: BufferId, anchor_index: usize) -> Self {
        Self {
            buffer_id,
            anchor_index,
        }
    }

    pub fn buffer_id(self) -> BufferId {
        self.buffer_id
    }

    pub fn anchor_index(self) -> usize {
        self.anchor_index
    }
}

#[cfg(test)]
mod tests {
    use crate::{ExcerptRange, MultiBuffer};
    use language::{Buffer, Capability};
    use text::BufferId;

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
