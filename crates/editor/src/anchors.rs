use crate::selection::Selection;
use multibuffer::MultiBuffer;

pub(crate) fn resolve_selection_from_anchors(
    buffer: &MultiBuffer,
    selection: &Selection,
) -> Selection {
    let Some(tail_anchor) = selection.tail_anchor else {
        return selection.clone();
    };
    let Some(head_anchor) = selection.head_anchor else {
        return selection.clone();
    };
    let Some(tail) = buffer.offset_for_tracked_anchor(tail_anchor) else {
        return selection.clone();
    };
    let Some(head) = buffer.offset_for_tracked_anchor(head_anchor) else {
        return selection.clone();
    };

    let mut resolved = Selection::from_anchor_head(selection.id, tail, head);
    resolved.goal = selection.goal;
    resolved.set_anchor_handles(tail_anchor, head_anchor);
    resolved
}

pub(crate) fn attach_selection_anchors(buffer: &mut MultiBuffer, selection: &mut Selection) {
    let tail = selection.tail();
    let head = selection.head();
    let tail_anchor = if selection.is_empty() {
        update_or_track_anchor_after(buffer, selection.tail_anchor, tail)
    } else if selection.reversed {
        update_or_track_anchor_after(buffer, selection.tail_anchor, tail)
    } else {
        update_or_track_anchor_before(buffer, selection.tail_anchor, tail)
    };
    let head_anchor = if selection.is_empty() || !selection.reversed {
        update_or_track_anchor_after(buffer, selection.head_anchor, head)
    } else {
        update_or_track_anchor_before(buffer, selection.head_anchor, head)
    };

    if let (Some(tail_anchor), Some(head_anchor)) = (tail_anchor, head_anchor) {
        selection.set_anchor_handles(tail_anchor, head_anchor);
    }
}

fn update_or_track_anchor_before(
    buffer: &mut MultiBuffer,
    handle: Option<multibuffer::MultiBufferAnchor>,
    offset: usize,
) -> Option<multibuffer::MultiBufferAnchor> {
    if let Some(handle) = handle {
        buffer.update_anchor_before(handle, offset)
    } else {
        buffer.track_anchor_before(offset)
    }
}

fn update_or_track_anchor_after(
    buffer: &mut MultiBuffer,
    handle: Option<multibuffer::MultiBufferAnchor>,
    offset: usize,
) -> Option<multibuffer::MultiBufferAnchor> {
    if let Some(handle) = handle {
        buffer.update_anchor_after(handle, offset)
    } else {
        buffer.track_anchor_after(offset)
    }
}

#[cfg(test)]
mod tests {
    use crate::EditorModel;
    use language::Buffer;
    use text::BufferId;

    #[test]
    fn select_anchor_heads_preserves_multiple_selection_directions() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one two three");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_heads(vec![(3, 0), (4, 7)]);

        let selections = editor.selections();
        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].range(), 0..3);
        assert_eq!(selections[0].head(), 0);
        assert_eq!(selections[0].tail(), 3);
        assert!(selections[0].reversed);
        assert_eq!(selections[1].range(), 4..7);
        assert_eq!(selections[1].head(), 7);
        assert_eq!(selections[1].tail(), 4);
        assert!(!selections[1].reversed);
    }

    #[test]
    fn select_anchor_heads_normalizes_overlaps_to_forward_selection() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdefghi");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_heads(vec![(5, 1), (3, 8)]);

        let selections = editor.selections();
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].range(), 1..8);
        assert_eq!(selections[0].head(), 8);
        assert_eq!(selections[0].tail(), 1);
        assert!(!selections[0].reversed);
    }

    #[test]
    fn cursor_queries_resolve_selection_anchors_without_mutating_cached_offsets() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        editor.buffer.edit(0..0, "say ").unwrap();

        assert_eq!(editor.snapshot().text(), "say hello world");
        assert_eq!(editor.selections()[0].head(), 6);
        assert_eq!(editor.cursor_offset().unwrap(), 10);
        assert_eq!(editor.resolved_selections()[0].head(), 10);
    }

    #[test]
    fn reversed_selection_syncs_head_and_tail_from_tracked_anchors() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select_anchor_head(8, 2);

        editor.buffer.edit(0..0, "say ").unwrap();
        editor.sync_selections_to_anchors();

        let selection = &editor.selections()[0];
        assert_eq!(selection.range(), 6..12);
        assert_eq!(selection.head(), 6);
        assert_eq!(selection.tail(), 12);
        assert!(selection.reversed);
    }

    #[test]
    fn reattaching_selection_anchors_reuses_existing_handles() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "hello world");
        let mut editor = EditorModel::for_buffer("scratch", buffer.into_handle());
        editor.select(6..6);

        let tail_anchor = editor.selections()[0].tail_anchor;
        let head_anchor = editor.selections()[0].head_anchor;

        editor.reattach_selection_anchors();
        editor.reattach_selection_anchors();

        assert_eq!(editor.selections()[0].tail_anchor, tail_anchor);
        assert_eq!(editor.selections()[0].head_anchor, head_anchor);
    }
}
