use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    pub row: usize,
    pub text: String,
    pub source_range: Range<usize>,
    pub continuation: bool,
}
