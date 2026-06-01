use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectPath {
    pub worktree_id: u64,
    pub path: Rc<PathBuf>,
}

impl ProjectPath {
    pub fn new(worktree_id: u64, path: impl Into<PathBuf>) -> Self {
        Self {
            worktree_id,
            path: Rc::new(path.into()),
        }
    }

    pub fn path_key(&self) -> String {
        self.path.as_path().display().to_string()
    }
}
