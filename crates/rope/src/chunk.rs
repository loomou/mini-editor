use crate::{RopePoint, TextSummary};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RopeChunk {
    text: Arc<str>,
    line_breaks: Arc<[usize]>,
    summary: TextSummary,
}

impl RopeChunk {
    pub(crate) fn new(text: String) -> Self {
        let line_breaks = text
            .bytes()
            .enumerate()
            .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset))
            .collect::<Vec<_>>();
        let summary = TextSummary::from_text(&text);

        Self {
            text: Arc::from(text),
            line_breaks: Arc::from(line_breaks),
            summary,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_break_count(&self) -> usize {
        self.line_breaks.len()
    }

    pub fn summary(&self) -> TextSummary {
        self.summary
    }

    pub(crate) fn point_for_offset(&self, offset: usize) -> RopePoint {
        let clipped = offset.min(self.len());
        let mut row = 0;
        let mut line_start = 0;

        for line_break in self.line_breaks.iter() {
            if *line_break >= clipped {
                break;
            }
            row += 1;
            line_start = *line_break + 1;
        }

        RopePoint {
            row,
            column: clipped - line_start,
        }
    }

    pub(crate) fn offset_for_point(&self, point: RopePoint) -> usize {
        let mut current_row = 0;
        let mut current_line_start = 0;

        for line_break in self.line_breaks.iter() {
            if current_row == point.row {
                return (current_line_start + point.column).min(*line_break);
            }
            current_row += 1;
            current_line_start = *line_break + 1;
        }

        if current_row == point.row {
            (current_line_start + point.column).min(self.len())
        } else {
            self.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Rope, RopePoint, TextSummary};

    #[test]
    fn stores_line_summaries_per_chunk() {
        let rope = Rope::from_text_with_chunk_size("a\nb\ncd".to_string(), 3);

        assert_eq!(rope.line_count(), 3);
        assert_eq!(
            rope.summary(),
            TextSummary {
                len: 6,
                line_break_count: 2,
                extent: RopePoint { row: 2, column: 2 },
            }
        );
        assert_eq!(rope.chunks()[0].line_break_count(), 1);
        assert_eq!(rope.chunks()[1].line_break_count(), 1);
    }
}
