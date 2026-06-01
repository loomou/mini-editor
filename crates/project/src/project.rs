use crate::{BufferStore, FileMetadata, ProjectPath};
use editor::EditorModel;
use language::BufferHandle;
use std::io;

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

    pub fn open_local_file(&mut self, path: ProjectPath) -> io::Result<BufferHandle> {
        self.buffer_store.open_local_file(path)
    }

    pub fn open_editor(&mut self, path: ProjectPath, text: impl Into<String>) -> EditorModel {
        let path_key = path.path_key();
        let buffer = self.open_buffer(path, text);
        EditorModel::for_buffer(path_key, buffer)
    }

    pub fn open_editor_from_file(&mut self, path: ProjectPath) -> io::Result<EditorModel> {
        let path_key = path.path_key();
        let buffer = self.open_local_file(path)?;
        Ok(EditorModel::for_buffer(path_key, buffer))
    }

    pub fn save_buffer(&mut self, path: &ProjectPath) -> io::Result<()> {
        self.buffer_store.save_buffer(path)
    }

    pub fn save_dirty_buffers(&mut self) -> io::Result<Vec<ProjectPath>> {
        self.buffer_store.save_dirty_buffers()
    }

    pub fn revert_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        self.buffer_store.revert_buffer(path)
    }

    pub fn reload_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        self.buffer_store.reload_buffer(path)
    }

    pub fn force_reload_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        self.buffer_store.force_reload_buffer(path)
    }

    pub fn close_buffer(&mut self, path: &ProjectPath) -> Option<BufferHandle> {
        self.buffer_store.close_buffer(path)
    }

    pub fn file_metadata(&self, path: &ProjectPath) -> Option<&FileMetadata> {
        self.buffer_store.file_metadata(path)
    }

    pub fn has_external_change(&self, path: &ProjectPath) -> Option<bool> {
        self.buffer_store.has_external_change(path)
    }

    pub fn dirty_buffers(&self) -> Vec<ProjectPath> {
        self.buffer_store.dirty_paths()
    }

    pub fn has_dirty_buffers(&self) -> bool {
        self.buffer_store.has_dirty_buffers()
    }

    pub fn buffer_store(&self) -> &BufferStore {
        &self.buffer_store
    }
}

#[cfg(test)]
mod tests {
    use crate::{Project, ProjectPath};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_file_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("mini_project_tests")
            .join(format!("{name}-{unique}.txt"))
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

    #[test]
    fn opening_local_file_reads_disk_and_reuses_open_buffer() {
        let file_path = test_file_path("open-local-file");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let first = project.open_local_file(path.clone()).unwrap();

        std::fs::write(&file_path, "changed on disk").unwrap();
        let second = project.open_local_file(path).unwrap();

        assert!(std::rc::Rc::ptr_eq(&first, &second));
        assert_eq!(second.borrow().snapshot().text.text(), "from disk");
    }
}
