use std::ops::Range;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: Rc<str>,
}
