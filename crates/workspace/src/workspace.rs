use project::Project;

pub use project::ProjectPath;
pub use ui::{CommandOutcome, EditorCommand, EditorView, RenderedEditor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Editor(EditorCommand),
    SaveAll,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCommandOutcome {
    pub editor: Option<CommandOutcome>,
    pub saved_paths: Vec<ProjectPath>,
}

#[derive(Debug, Default)]
pub struct Workspace {
    project: Project,
    active_editor: Option<EditorView>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_editor(&mut self, path: ProjectPath, text: impl Into<String>) {
        let editor = self.project.open_editor(path, text);
        self.active_editor = Some(EditorView::new(editor));
    }

    pub fn open_editor_from_file(&mut self, path: ProjectPath) -> Result<(), String> {
        let editor = self
            .project
            .open_editor_from_file(path)
            .map_err(|error| error.to_string())?;
        self.active_editor = Some(EditorView::new(editor));
        Ok(())
    }

    pub fn dispatch_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<WorkspaceCommandOutcome, String> {
        match command {
            WorkspaceCommand::Editor(command) => {
                let editor = self
                    .active_editor
                    .as_mut()
                    .ok_or_else(|| "workspace has no active editor".to_string())?;
                Ok(WorkspaceCommandOutcome {
                    editor: Some(editor.dispatch_command(command)?),
                    saved_paths: Vec::new(),
                })
            }
            WorkspaceCommand::SaveAll => Ok(WorkspaceCommandOutcome {
                editor: None,
                saved_paths: self
                    .project
                    .save_dirty_buffers()
                    .map_err(|error| error.to_string())?,
            }),
        }
    }

    pub fn active_rendered_editor(
        &self,
        soft_wrap_column: Option<usize>,
    ) -> Option<RenderedEditor> {
        self.active_editor
            .as_ref()
            .map(|editor| editor.rendered_editor(soft_wrap_column))
    }

    pub fn project(&self) -> &Project {
        &self.project
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
