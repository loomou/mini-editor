use std::ops::Range;
use text::BufferId;

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
