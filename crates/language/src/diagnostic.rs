#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub range: std::ops::Range<usize>,
}
