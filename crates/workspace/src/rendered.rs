use project::ProjectPath;
use ui::RenderedEditor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedWorkspace {
    pub tabs: Vec<RenderedTab>,
    pub active_editor: Option<RenderedEditor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedTab {
    pub path: ProjectPath,
    pub title: String,
    pub is_active: bool,
    pub is_dirty: bool,
    pub has_external_change: bool,
}

impl RenderedTab {
    pub fn label_text(&self) -> String {
        let active_marker = if self.is_active { '>' } else { ' ' };
        let dirty_marker = if self.is_dirty { '*' } else { ' ' };
        let external_marker = if self.has_external_change { '!' } else { ' ' };
        format!(
            "{active_marker}{dirty_marker}{external_marker} {}",
            self.title
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{EditorCommand, Workspace, WorkspaceCommand};
    use project::ProjectPath;
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
            .join("mini_workspace_tests")
            .join(format!("{name}-{unique}.txt"))
    }

    #[test]
    fn rendered_workspace_lists_tabs_with_active_and_dirty_state() {
        let first = ProjectPath::new(1, "src/first.rs");
        let second = ProjectPath::new(1, "src/second.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(first.clone(), "first");
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();
        workspace.open_editor(second.clone(), "second");

        let rendered = workspace.rendered_workspace(None);

        assert_eq!(rendered.tabs.len(), 2);
        assert_eq!(rendered.tabs[0].path, first);
        assert_eq!(rendered.tabs[0].label_text(), " *  src/first.rs");
        assert_eq!(rendered.tabs[1].path, second);
        assert_eq!(rendered.tabs[1].label_text(), ">   src/second.rs");
        assert_eq!(rendered.active_editor.unwrap().title, "src/second.rs");
    }

    #[test]
    fn rendered_workspace_marks_tabs_with_external_file_changes() {
        let file_path = test_file_path("external-change-tab");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "old").unwrap();

        let path = ProjectPath::new(1, file_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(path.clone()).unwrap();

        assert_eq!(
            workspace.rendered_workspace(None).tabs[0].label_text(),
            format!(">   {}", file_path.display())
        );

        std::fs::write(&file_path, "changed length").unwrap();

        let rendered = workspace.rendered_workspace(None);

        assert!(rendered.tabs[0].has_external_change);
        assert_eq!(
            rendered.tabs[0].label_text(),
            format!("> ! {}", file_path.display())
        );

        workspace.reload_active_editor().unwrap();

        let rendered = workspace.rendered_workspace(None);
        assert!(!rendered.tabs[0].has_external_change);
        assert_eq!(
            rendered.tabs[0].label_text(),
            format!(">   {}", file_path.display())
        );
    }
}
