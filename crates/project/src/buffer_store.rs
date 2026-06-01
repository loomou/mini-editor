use crate::{FileMetadata, ProjectPath};
use language::{Buffer, BufferHandle, SourceFile};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use text::BufferId;

#[derive(Debug, Default)]
pub struct BufferStore {
    next_buffer_id: u64,
    buffers: BTreeMap<BufferId, BufferHandle>,
    path_to_buffer_id: BTreeMap<ProjectPath, BufferId>,
    file_metadata: BTreeMap<ProjectPath, FileMetadata>,
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
        let buffer =
            Buffer::from_file(buffer_id, SourceFile::new(path.path.as_ref().clone()), text);
        let buffer = buffer.into_handle();
        self.buffers.insert(buffer_id, buffer.clone());
        self.file_metadata
            .insert(path.clone(), FileMetadata::missing());
        self.path_to_buffer_id.insert(path, buffer_id);
        buffer
    }

    pub fn open_local_file(&mut self, path: ProjectPath) -> io::Result<BufferHandle> {
        if let Some(buffer) = self.buffer_for_path(&path) {
            return Ok(buffer);
        }

        let text = fs::read_to_string(path.path.as_ref())?;
        let metadata = crate::file_metadata::read_file_metadata(&path);
        let buffer = self.open_buffer(path.clone(), text);
        self.file_metadata.insert(path, metadata);
        Ok(buffer)
    }

    pub fn save_buffer(&mut self, path: &ProjectPath) -> io::Result<()> {
        let buffer = self.buffer_for_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no open buffer for {}", path.path.as_path().display()),
            )
        })?;
        let text = buffer.borrow().snapshot().text.text();

        if let Some(parent) = path.path.as_path().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path.path.as_ref(), text)?;
        buffer.borrow_mut().save();
        self.file_metadata
            .insert(path.clone(), crate::file_metadata::read_file_metadata(path));
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
                format!("no open buffer for {}", path.path.as_path().display()),
            )
        })?;
        buffer
            .borrow_mut()
            .revert_to_saved()
            .map_err(io::Error::other)
    }

    pub fn reload_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        let buffer = self.buffer_for_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no open buffer for {}", path.path.as_path().display()),
            )
        })?;
        if buffer.borrow().snapshot().is_dirty() {
            return Err(io::Error::other(format!(
                "cannot reload dirty buffer {}",
                path.path.as_path().display()
            )));
        }

        let text = fs::read_to_string(path.path.as_ref())?;
        let changed = buffer.borrow_mut().reload_saved_text(text);
        self.file_metadata
            .insert(path.clone(), crate::file_metadata::read_file_metadata(path));
        Ok(changed)
    }

    pub fn force_reload_buffer(&mut self, path: &ProjectPath) -> io::Result<bool> {
        let buffer = self.buffer_for_path(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no open buffer for {}", path.path.as_path().display()),
            )
        })?;
        let text = fs::read_to_string(path.path.as_ref())?;
        let changed = buffer.borrow_mut().reload_saved_text(text);
        self.file_metadata
            .insert(path.clone(), crate::file_metadata::read_file_metadata(path));
        Ok(changed)
    }

    pub fn close_buffer(&mut self, path: &ProjectPath) -> Option<BufferHandle> {
        let buffer_id = self.path_to_buffer_id.remove(path)?;
        self.file_metadata.remove(path);
        self.buffers.remove(&buffer_id)
    }

    pub fn file_metadata(&self, path: &ProjectPath) -> Option<&FileMetadata> {
        self.file_metadata.get(path)
    }

    pub fn has_external_change(&self, path: &ProjectPath) -> Option<bool> {
        let known = self.file_metadata.get(path)?;
        if known.modified.is_none() && known.len.is_none() {
            return Some(false);
        }
        Some(&crate::file_metadata::read_file_metadata(path) != known)
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
        assert!(project.file_metadata(&path).is_none());

        let reopened = project.open_buffer(path.clone(), "second");

        assert!(!std::rc::Rc::ptr_eq(&first, &reopened));
        assert_eq!(reopened.borrow().snapshot().text.text(), "second");
        assert_eq!(project.buffer_store().len(), 1);
    }

    #[test]
    fn reloading_clean_open_buffer_reads_new_disk_contents() {
        let file_path = test_file_path("reload-clean");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        project.open_local_file(path.clone()).unwrap();

        std::fs::write(&file_path, "changed on disk").unwrap();

        assert!(project.reload_buffer(&path).unwrap());

        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert_eq!(buffer.borrow().snapshot().text.text(), "changed on disk");
        assert!(!buffer.borrow().snapshot().is_dirty());
    }

    #[test]
    fn reloading_dirty_open_buffer_is_rejected() {
        let file_path = test_file_path("reload-dirty");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let mut editor = project.open_editor_from_file(path.clone()).unwrap();
        editor.select(0..4);
        editor.insert_text("buffer").unwrap();

        std::fs::write(&file_path, "changed on disk").unwrap();

        let error = project.reload_buffer(&path).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("cannot reload dirty buffer {}", file_path.display())
        );
        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert_eq!(buffer.borrow().snapshot().text.text(), "buffer disk");
        assert!(buffer.borrow().snapshot().is_dirty());
    }

    #[test]
    fn force_reloading_dirty_open_buffer_replaces_unsaved_text() {
        let file_path = test_file_path("force-reload-dirty");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let mut editor = project.open_editor_from_file(path.clone()).unwrap();
        editor.select(0..4);
        editor.insert_text("buffer").unwrap();

        std::fs::write(&file_path, "changed on disk").unwrap();

        assert!(project.force_reload_buffer(&path).unwrap());

        let buffer = project.buffer_store().buffer_for_path(&path).unwrap();
        assert_eq!(buffer.borrow().snapshot().text.text(), "changed on disk");
        assert!(!buffer.borrow().snapshot().is_dirty());
        assert_eq!(project.has_external_change(&path), Some(false));
    }
}
