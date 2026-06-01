use crate::ProjectPath;
use std::fs;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMetadata {
    pub modified: Option<SystemTime>,
    pub len: Option<u64>,
}

impl FileMetadata {
    pub(crate) fn missing() -> Self {
        Self {
            modified: None,
            len: None,
        }
    }
}

pub(crate) fn read_file_metadata(path: &ProjectPath) -> FileMetadata {
    let Ok(metadata) = fs::metadata(path.path.as_ref()) else {
        return FileMetadata::missing();
    };

    FileMetadata {
        modified: metadata.modified().ok(),
        len: Some(metadata.len()),
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
    fn opening_local_file_records_file_metadata() {
        let file_path = test_file_path("open-metadata");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path);
        project.open_local_file(path.clone()).unwrap();

        let metadata = project.file_metadata(&path).unwrap();
        assert!(metadata.modified.is_some());
        assert_eq!(metadata.len, Some("from disk".len() as u64));
    }

    #[test]
    fn saving_and_reloading_refresh_file_metadata() {
        let file_path = test_file_path("refresh-metadata");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "from disk").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        let mut editor = project.open_editor_from_file(path.clone()).unwrap();

        editor.select(0..4);
        editor.insert_text("saved").unwrap();
        project.save_buffer(&path).unwrap();
        assert!(project.file_metadata(&path).unwrap().modified.is_some());
        assert_eq!(
            project.file_metadata(&path).unwrap().len,
            Some("saved disk".len() as u64)
        );

        std::fs::write(&file_path, "changed on disk").unwrap();
        project.reload_buffer(&path).unwrap();

        assert!(project.file_metadata(&path).unwrap().modified.is_some());
        assert_eq!(
            project.file_metadata(&path).unwrap().len,
            Some("changed on disk".len() as u64)
        );
    }

    #[test]
    fn external_change_detection_compares_current_disk_metadata() {
        let file_path = test_file_path("external-change");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "old").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path.clone());
        project.open_local_file(path.clone()).unwrap();

        assert_eq!(project.has_external_change(&path), Some(false));

        std::fs::write(&file_path, "changed length").unwrap();

        assert_eq!(project.has_external_change(&path), Some(true));

        project.reload_buffer(&path).unwrap();

        assert_eq!(project.has_external_change(&path), Some(false));
    }

    #[test]
    fn external_change_detection_is_unknown_after_buffer_closes() {
        let file_path = test_file_path("external-change-closed");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "old").unwrap();

        let mut project = Project::new();
        let path = ProjectPath::new(1, file_path);
        project.open_local_file(path.clone()).unwrap();
        project.close_buffer(&path);

        assert_eq!(project.has_external_change(&path), None);
    }
}
