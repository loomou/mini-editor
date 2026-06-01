use crate::RopePoint;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSummary {
    pub len: usize,
    pub line_break_count: usize,
    pub extent: RopePoint,
}

impl TextSummary {
    pub fn from_text(text: &str) -> Self {
        let mut summary = Self::default();
        summary.len = text.len();

        for character in text.chars() {
            if character == '\n' {
                summary.line_break_count += 1;
                summary.extent.row += 1;
                summary.extent.column = 0;
            } else {
                summary.extent.column += character.len_utf8();
            }
        }

        summary
    }

    pub(crate) fn append(self, other: Self) -> Self {
        Self {
            len: self.len + other.len,
            line_break_count: self.line_break_count + other.line_break_count,
            extent: self.extent.add(other.extent),
        }
    }
}
