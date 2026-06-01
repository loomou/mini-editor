#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RopePoint {
    pub row: usize,
    pub column: usize,
}

impl RopePoint {
    pub(crate) fn add(self, other: Self) -> Self {
        if other.row == 0 {
            Self {
                row: self.row,
                column: self.column + other.column,
            }
        } else {
            Self {
                row: self.row + other.row,
                column: other.column,
            }
        }
    }

    pub(crate) fn subtract(self, other: Self) -> Self {
        debug_assert!(other <= self);
        if self.row == other.row {
            Self {
                row: 0,
                column: self.column - other.column,
            }
        } else {
            Self {
                row: self.row - other.row,
                column: self.column,
            }
        }
    }
}

impl PartialOrd for RopePoint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RopePoint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.row
            .cmp(&other.row)
            .then_with(|| self.column.cmp(&other.column))
    }
}
