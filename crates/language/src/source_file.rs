use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    path: PathBuf,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}
