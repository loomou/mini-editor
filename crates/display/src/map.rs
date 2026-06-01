use crate::wrap::next_wrap_boundary;
use crate::{DisplayRow, DisplaySnapshot};
use multibuffer::MultiBufferSnapshot;

#[derive(Clone, Debug)]
pub struct DisplayMap {
    soft_wrap_column: Option<usize>,
}

impl DisplayMap {
    pub fn new(soft_wrap_column: Option<usize>) -> Self {
        Self { soft_wrap_column }
    }

    pub fn snapshot(&self, buffer: &MultiBufferSnapshot) -> DisplaySnapshot {
        let source = buffer.text();
        let mut rows = Vec::new();
        let mut source_line_start = 0;

        for source_line in source.split_inclusive('\n') {
            let has_newline = source_line.ends_with('\n');
            let visible_line = if has_newline {
                &source_line[..source_line.len() - 1]
            } else {
                source_line
            };
            self.push_wrapped_rows(&mut rows, visible_line, source_line_start);
            source_line_start += source_line.len();
        }

        if source.is_empty() || source.ends_with('\n') {
            rows.push(DisplayRow {
                row: rows.len(),
                text: String::new(),
                source_range: source.len()..source.len(),
                continuation: false,
            });
        }

        DisplaySnapshot::new(rows, source.len())
    }

    fn push_wrapped_rows(&self, rows: &mut Vec<DisplayRow>, line: &str, line_start: usize) {
        let Some(wrap_column) = self.soft_wrap_column.filter(|column| *column > 0) else {
            rows.push(DisplayRow {
                row: rows.len(),
                text: line.to_string(),
                source_range: line_start..line_start + line.len(),
                continuation: false,
            });
            return;
        };

        if line.is_empty() {
            rows.push(DisplayRow {
                row: rows.len(),
                text: String::new(),
                source_range: line_start..line_start,
                continuation: false,
            });
            return;
        }

        let mut segment_start = 0;
        while segment_start < line.len() {
            let segment_end = next_wrap_boundary(line, segment_start, wrap_column);
            rows.push(DisplayRow {
                row: rows.len(),
                text: line[segment_start..segment_end].to_string(),
                source_range: line_start + segment_start..line_start + segment_end,
                continuation: segment_start > 0,
            });
            segment_start = segment_end;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{DisplayMap, DisplayPoint};
    use language::Buffer;
    use multibuffer::MultiBuffer;
    use text::BufferId;

    #[test]
    fn creates_display_rows_from_multibuffer_text() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "one\ntwo");
        let multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());
        let display = DisplayMap::new(None).snapshot(&multibuffer.snapshot());

        assert_eq!(display.rows().len(), 2);
        assert_eq!(display.rows()[0].text, "one");
        assert_eq!(display.rows()[1].source_range, 4..7);
    }

    #[test]
    fn wraps_long_lines_without_changing_source_offsets() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "abcdef");
        let multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());
        let display = DisplayMap::new(Some(3)).snapshot(&multibuffer.snapshot());

        assert_eq!(display.rows()[0].text, "abc");
        assert_eq!(display.rows()[1].text, "def");
        assert!(display.rows()[1].continuation);
        assert_eq!(
            display.source_offset_for_display_point(DisplayPoint { row: 1, column: 1 }),
            4
        );
        assert_eq!(
            display.display_point_for_source_offset(4),
            DisplayPoint { row: 1, column: 1 }
        );
    }

    #[test]
    fn maps_vertical_movement_by_display_column() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "ab\ncdef");
        let multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());
        let display = DisplayMap::new(None).snapshot(&multibuffer.snapshot());

        assert_eq!(display.source_offset_for_vertical_movement(1, 1, 1), 4);
        assert_eq!(display.source_offset_for_vertical_movement(4, -1, 1), 1);
        assert_eq!(display.source_offset_for_vertical_movement(1, 10, 5), 7);
    }

    #[test]
    fn display_columns_map_to_utf8_source_offsets() {
        let buffer = Buffer::local(BufferId::new(1).unwrap(), "a😀c");
        let multibuffer = MultiBuffer::singleton("scratch", buffer.into_handle());
        let display = DisplayMap::new(None).snapshot(&multibuffer.snapshot());

        assert_eq!(
            display.source_offset_for_display_point(DisplayPoint { row: 0, column: 0 }),
            0
        );
        assert_eq!(
            display.source_offset_for_display_point(DisplayPoint { row: 0, column: 1 }),
            1
        );
        assert_eq!(
            display.source_offset_for_display_point(DisplayPoint { row: 0, column: 2 }),
            5
        );
        assert_eq!(
            display.source_offset_for_display_point(DisplayPoint { row: 0, column: 3 }),
            6
        );
        assert_eq!(
            display.display_point_for_source_offset(0),
            DisplayPoint { row: 0, column: 0 }
        );
        assert_eq!(
            display.display_point_for_source_offset(1),
            DisplayPoint { row: 0, column: 1 }
        );
        assert_eq!(
            display.display_point_for_source_offset(5),
            DisplayPoint { row: 0, column: 2 }
        );
        assert_eq!(
            display.display_point_for_source_offset(6),
            DisplayPoint { row: 0, column: 3 }
        );
    }
}
