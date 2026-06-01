use project::ProjectPath;
use ui::{CommandOutcome, EditorCommand};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Editor(EditorCommand),
    SaveAll,
    CloseActiveEditor,
    SaveAndCloseActiveEditor,
    DiscardAndCloseActiveEditor,
    ReloadActiveEditor,
    ForceReloadActiveEditor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCommandOutcome {
    pub editor: Option<CommandOutcome>,
    pub saved_paths: Vec<ProjectPath>,
    pub closed_path: Option<ProjectPath>,
    pub reloaded_path: Option<ProjectPath>,
    pub reload_changed_text: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReloadOutcome {
    pub path: ProjectPath,
    pub changed_text: bool,
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
    fn save_all_clears_active_editor_dirty_marker() {
        let file_path = test_file_path("save-all");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let path = ProjectPath::new(1, file_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(path.clone()).unwrap();

        assert_eq!(
            workspace
                .active_rendered_editor(None)
                .unwrap()
                .header_text(),
            format!("  {}", file_path.display())
        );

        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();

        assert_eq!(
            workspace
                .active_rendered_editor(None)
                .unwrap()
                .header_text(),
            format!("* {}", file_path.display())
        );
        assert!(workspace.project().has_dirty_buffers());

        let outcome = workspace
            .dispatch_command(WorkspaceCommand::SaveAll)
            .unwrap();

        assert_eq!(outcome.saved_paths, vec![path]);
        assert_eq!(outcome.closed_path, None);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "/hello world");
        assert_eq!(
            workspace
                .active_rendered_editor(None)
                .unwrap()
                .header_text(),
            format!("  {}", file_path.display())
        );
        assert!(!workspace.project().has_dirty_buffers());
    }

    #[test]
    fn editor_command_requires_an_active_editor() {
        let mut workspace = Workspace::new();

        let error = workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::Delete))
            .unwrap_err();

        assert_eq!(error, "workspace has no active editor");
    }

    #[test]
    fn workspace_dispatches_editor_undo_and_redo_commands() {
        let path = ProjectPath::new(1, "src/main.rs");
        let mut workspace = Workspace::new();
        workspace.open_editor(path, "hello");

        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "/hello"
        );

        let undo = workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::Undo))
            .unwrap()
            .editor
            .unwrap();
        assert!(undo.changed_text);
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "hello"
        );

        let redo = workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::Redo))
            .unwrap()
            .editor
            .unwrap();
        assert!(redo.changed_text);
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "/hello"
        );
    }
}
