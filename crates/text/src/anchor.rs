use std::{fmt, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(u64);

impl BufferId {
    pub fn new(id: u64) -> Option<Self> {
        (id != 0).then_some(Self(id))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    buffer_id: BufferId,
    offset: usize,
    bias: Bias,
}

impl fmt::Debug for Anchor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Anchor")
            .field("buffer_id", &self.buffer_id)
            .field("offset", &self.offset)
            .field("bias", &self.bias)
            .finish()
    }
}

impl Anchor {
    pub fn new(buffer_id: BufferId, offset: usize, bias: Bias) -> Self {
        Self {
            buffer_id,
            offset,
            bias,
        }
    }

    pub fn buffer_id(self) -> BufferId {
        self.buffer_id
    }

    pub fn offset(self) -> usize {
        self.offset
    }

    pub fn bias(self) -> Bias {
        self.bias
    }

    pub fn transform(self, edited_range: Range<usize>, inserted_len: usize) -> Self {
        let deleted_len = edited_range.end - edited_range.start;
        let offset = if self.offset < edited_range.start {
            self.offset
        } else if self.offset > edited_range.end {
            self.offset + inserted_len - deleted_len
        } else {
            match self.bias {
                Bias::Left => edited_range.start,
                Bias::Right => edited_range.start + inserted_len,
            }
        };

        Self { offset, ..self }
    }
}
