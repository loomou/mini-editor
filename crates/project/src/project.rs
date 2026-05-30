use editor::EditorModel;
use language::{Buffer, BufferHandle, SourceFile};
use std::collections::BTreeMap;
use std::path::PathBuf;
use text::BufferId;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectPath {
    pub worktree_id: u64,
    pub path: PathBuf,
}

impl ProjectPath {
    pub fn new(worktree_id: u64, path: impl Into<PathBuf>) -> Self {
        Self {
            worktree_id,
            path: path.into(),
        }
    }

    pub fn path_key(&self) -> String {
        self.path.display().to_string()
    }
}

#[derive(Debug, Default)]
pub struct BufferStore {
    next_buffer_id: u64,
    buffers: BTreeMap<BufferId, BufferHandle>,
    path_to_buffer_id: BTreeMap<ProjectPath, BufferId>,
}

impl BufferStore {
    pub fn open_buffer(&mut self, path: ProjectPath, text: impl Into<String>) -> BufferHandle {
        if let Some(buffer_id) = self.path_to_buffer_id.get(&path) {
            return self
                .buffers
                .get(buffer_id)
                .expect("path map should point at an open buffer")
                .clone();
        }

        self.next_buffer_id += 1;
        let buffer_id = BufferId::new(self.next_buffer_id).expect("buffer ids start at one");
        let buffer = Buffer::from_file(buffer_id, SourceFile::new(path.path.clone()), text);
        let buffer = buffer.into_handle();
        self.buffers.insert(buffer_id, buffer.clone());
        self.path_to_buffer_id.insert(path, buffer_id);
        buffer
    }

    pub fn buffer_for_path(&self, path: &ProjectPath) -> Option<BufferHandle> {
        self.path_to_buffer_id
            .get(path)
            .and_then(|buffer_id| self.buffers.get(buffer_id))
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct Project {
    buffer_store: BufferStore,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_buffer(&mut self, path: ProjectPath, text: impl Into<String>) -> BufferHandle {
        self.buffer_store.open_buffer(path, text)
    }

    pub fn open_editor(&mut self, path: ProjectPath, text: impl Into<String>) -> EditorModel {
        let path_key = path.path_key();
        let buffer = self.open_buffer(path, text);
        EditorModel::for_buffer(path_key, buffer)
    }

    pub fn buffer_store(&self) -> &BufferStore {
        &self.buffer_store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_the_same_path_reuses_the_same_buffer_handle() {
        let mut project = Project::new();
        let path = ProjectPath::new(1, "src/main.rs");

        let first = project.open_buffer(path.clone(), "first");
        let second = project.open_buffer(path, "second");

        assert_eq!(project.buffer_store().len(), 1);
        assert!(std::rc::Rc::ptr_eq(&first, &second));
        assert_eq!(second.borrow().snapshot().text.text(), "first");
    }

    #[test]
    fn editor_edits_are_visible_through_the_project_buffer_store() {
        let mut project = Project::new();
        let path = ProjectPath::new(1, "src/main.rs");
        let mut editor = project.open_editor(path.clone(), "hello world");

        editor.select(6..11);
        editor.insert_text("zed").unwrap();

        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert_eq!(buffer.borrow().snapshot().text.text(), "hello zed");
    }
}
