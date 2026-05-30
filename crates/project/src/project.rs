use editor::EditorModel;
use language::{Buffer, BufferHandle, SourceFile};
use std::collections::BTreeMap;
use std::fs;
use std::io;
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

    pub fn open_local_file(&mut self, path: ProjectPath) -> io::Result<BufferHandle> {
        if let Some(buffer) = self.buffer_for_path(&path) {
            return Ok(buffer);
        }

        let text = fs::read_to_string(&path.path)?;
        Ok(self.open_buffer(path, text))
    }

    pub fn save_buffer(&mut self, path: &ProjectPath) -> io::Result<()> {
        let buffer = self.buffer_for_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no open buffer for {}", path.path.display()),
            )
        })?;
        let text = buffer.borrow().snapshot().text.text();

        if let Some(parent) = path.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path.path, text)?;
        buffer.borrow_mut().save();
        Ok(())
    }

    pub fn save_dirty_buffers(&mut self) -> io::Result<Vec<ProjectPath>> {
        let dirty_paths = self.dirty_paths();
        for path in &dirty_paths {
            self.save_buffer(path)?;
        }
        Ok(dirty_paths)
    }

    pub fn revert_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        let buffer = self.buffer_for_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no open buffer for {}", path.path.display()),
            )
        })?;
        buffer
            .borrow_mut()
            .revert_to_saved()
            .map_err(io::Error::other)
    }

    pub fn close_buffer(&mut self, path: &ProjectPath) -> Option<BufferHandle> {
        let buffer_id = self.path_to_buffer_id.remove(path)?;
        self.buffers.remove(&buffer_id)
    }

    pub fn buffer_for_path(&self, path: &ProjectPath) -> Option<BufferHandle> {
        self.path_to_buffer_id
            .get(path)
            .and_then(|buffer_id| self.buffers.get(buffer_id))
            .cloned()
    }

    pub fn dirty_paths(&self) -> Vec<ProjectPath> {
        self.path_to_buffer_id
            .iter()
            .filter_map(|(path, buffer_id)| {
                let buffer = self.buffers.get(buffer_id)?;
                buffer.borrow().snapshot().is_dirty().then(|| path.clone())
            })
            .collect()
    }

    pub fn has_dirty_buffers(&self) -> bool {
        self.path_to_buffer_id.iter().any(|(_, buffer_id)| {
            self.buffers
                .get(buffer_id)
                .is_some_and(|buffer| buffer.borrow().snapshot().is_dirty())
        })
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

    pub fn close_buffer(&mut self, path: &ProjectPath) -> Option<BufferHandle> {
        self.buffer_store.close_buffer(path)
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
    use super::*;
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

    #[test]
    fn saving_open_buffer_writes_disk_and_marks_buffer_clean() {
        let file_path = test_file_path("save-buffer");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let mut editor = project.open_editor_from_file(path.clone()).unwrap();

        editor.select(6..11);
        editor.insert_text("zed").unwrap();
        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert!(buffer.borrow().snapshot().is_dirty());

        project.save_buffer(&path).unwrap();

        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello zed");
        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert!(!buffer.borrow().snapshot().is_dirty());
    }

    #[test]
    fn dirty_buffers_lists_edited_paths_and_clears_after_save() {
        let first_path = test_file_path("dirty-first");
        let second_path = test_file_path("dirty-second");
        std::fs::create_dir_all(first_path.parent().unwrap()).unwrap();
        std::fs::write(&first_path, "first file").unwrap();
        std::fs::write(&second_path, "second file").unwrap();

        let mut project = Project::new();
        let first = ProjectPath::new(1, first_path);
        let second = ProjectPath::new(1, second_path);
        let mut first_editor = project.open_editor_from_file(first.clone()).unwrap();
        let _second_editor = project.open_editor_from_file(second.clone()).unwrap();

        assert!(!project.has_dirty_buffers());
        assert_eq!(project.dirty_buffers(), Vec::<ProjectPath>::new());

        first_editor.select(0..5);
        first_editor.insert_text("changed").unwrap();

        assert!(project.has_dirty_buffers());
        assert_eq!(project.dirty_buffers(), vec![first.clone()]);

        project.save_buffer(&first).unwrap();

        assert!(!project.has_dirty_buffers());
        assert_eq!(project.dirty_buffers(), Vec::<ProjectPath>::new());
    }

    #[test]
    fn saving_dirty_buffers_writes_all_dirty_files_and_returns_saved_paths() {
        let first_path = test_file_path("save-dirty-first");
        let second_path = test_file_path("save-dirty-second");
        let clean_path = test_file_path("save-dirty-clean");
        std::fs::create_dir_all(first_path.parent().unwrap()).unwrap();
        std::fs::write(&first_path, "first file").unwrap();
        std::fs::write(&second_path, "second file").unwrap();
        std::fs::write(&clean_path, "clean file").unwrap();

        let mut project = Project::new();
        let first = ProjectPath::new(1, first_path.clone());
        let second = ProjectPath::new(1, second_path.clone());
        let clean = ProjectPath::new(1, clean_path.clone());
        let mut first_editor = project.open_editor_from_file(first.clone()).unwrap();
        let mut second_editor = project.open_editor_from_file(second.clone()).unwrap();
        let _clean_editor = project.open_editor_from_file(clean.clone()).unwrap();

        first_editor.select(0..5);
        first_editor.insert_text("changed first").unwrap();
        second_editor.select(0..6);
        second_editor.insert_text("changed second").unwrap();

        assert!(project.has_dirty_buffers());
        assert_eq!(project.dirty_buffers(), vec![first.clone(), second.clone()]);

        let saved_paths = project.save_dirty_buffers().unwrap();

        assert_eq!(saved_paths, vec![first.clone(), second.clone()]);
        assert_eq!(
            std::fs::read_to_string(&first_path).unwrap(),
            "changed first file"
        );
        assert_eq!(
            std::fs::read_to_string(&second_path).unwrap(),
            "changed second file"
        );
        assert_eq!(std::fs::read_to_string(&clean_path).unwrap(), "clean file");
        assert!(!project.has_dirty_buffers());
        assert_eq!(project.dirty_buffers(), Vec::<ProjectPath>::new());
    }

    #[test]
    fn reverting_open_buffer_restores_saved_text_without_writing_disk() {
        let file_path = test_file_path("revert-buffer");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let mut editor = project.open_editor_from_file(path.clone()).unwrap();

        editor.select(6..11);
        editor.insert_text("zed").unwrap();
        assert!(project.has_dirty_buffers());

        assert!(project.revert_buffer(&path).unwrap());

        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert_eq!(buffer.borrow().snapshot().text.text(), "hello world");
        assert!(!project.has_dirty_buffers());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[test]
    fn closing_buffer_removes_path_identity_from_project_store() {
        let mut project = Project::new();
        let path = ProjectPath::new(1, "src/main.rs");
        let first = project.open_buffer(path.clone(), "first");

        let closed = project.close_buffer(&path).unwrap();

        assert!(std::rc::Rc::ptr_eq(&first, &closed));
        assert!(project.buffer_store().is_empty());
        assert!(project.buffer_store().buffer_for_path(&path).is_none());

        let reopened = project.open_buffer(path.clone(), "second");

        assert!(!std::rc::Rc::ptr_eq(&first, &reopened));
        assert_eq!(reopened.borrow().snapshot().text.text(), "second");
        assert_eq!(project.buffer_store().len(), 1);
    }
}
