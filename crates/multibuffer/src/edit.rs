use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiBufferEdit {
    pub range: Range<usize>,
    pub replacement: Rc<str>,
}
