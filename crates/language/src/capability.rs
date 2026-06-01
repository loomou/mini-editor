#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    ReadWrite,
    ReadOnly,
}

impl Capability {
    pub fn editable(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}
