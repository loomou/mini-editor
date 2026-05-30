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
}

impl RenderedTab {
    pub fn label_text(&self) -> String {
        let active_marker = if self.is_active { '>' } else { ' ' };
        let dirty_marker = if self.is_dirty { '*' } else { ' ' };
        format!("{active_marker}{dirty_marker} {}", self.title)
    }
}

#[derive(Debug, Default)]
pub struct Workspace {
    project: Project,
    open_editors: Vec<WorkspaceEditor>,
    active_index: Option<usize>,
}

#[derive(Debug)]
struct WorkspaceEditor {
    path: ProjectPath,
    view: EditorView,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_editor(&mut self, path: ProjectPath, text: impl Into<String>) {
        if self.activate_existing_editor(&path) {
            return;
        }

        let editor = self.project.open_editor(path.clone(), text);
        self.open_editors.push(WorkspaceEditor {
            path,
            view: EditorView::new(editor),
        });
        self.active_index = Some(self.open_editors.len() - 1);
    }

    pub fn open_editor_from_file(&mut self, path: ProjectPath) -> Result<(), String> {
        if self.activate_existing_editor(&path) {
            return Ok(());
        }

        let editor = self
            .project
            .open_editor_from_file(path.clone())
            .map_err(|error| error.to_string())?;
        self.open_editors.push(WorkspaceEditor {
            path,
            view: EditorView::new(editor),
        });
        self.active_index = Some(self.open_editors.len() - 1);
        Ok(())
    }

    pub fn dispatch_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<WorkspaceCommandOutcome, String> {
        match command {
            WorkspaceCommand::Editor(command) => {
                let editor = self.active_editor_mut()?;
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
        self.active_editor()
            .as_ref()
            .map(|editor| editor.rendered_editor(soft_wrap_column))
    }

    pub fn rendered_workspace(&self, soft_wrap_column: Option<usize>) -> RenderedWorkspace {
        RenderedWorkspace {
            tabs: self
                .open_editors
                .iter()
                .enumerate()
                .map(|(index, editor)| {
                    let rendered = editor.view.rendered_editor(None);
                    RenderedTab {
                        path: editor.path.clone(),
                        title: rendered.title,
                        is_active: self.active_index == Some(index),
                        is_dirty: rendered.is_dirty,
                    }
                })
                .collect(),
            active_editor: self.active_rendered_editor(soft_wrap_column),
        }
    }

    pub fn open_paths(&self) -> Vec<ProjectPath> {
        self.open_editors
            .iter()
            .map(|editor| editor.path.clone())
            .collect()
    }

    pub fn active_path(&self) -> Option<&ProjectPath> {
        self.active_index
            .and_then(|index| self.open_editors.get(index))
            .map(|editor| &editor.path)
    }

    pub fn switch_to_editor(&mut self, path: &ProjectPath) -> Result<(), String> {
        self.activate_existing_editor(path)
            .then_some(())
            .ok_or_else(|| format!("workspace has no open editor for {}", path.path.display()))
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    fn activate_existing_editor(&mut self, path: &ProjectPath) -> bool {
        if let Some(index) = self
            .open_editors
            .iter()
            .position(|editor| editor.path == *path)
        {
            self.active_index = Some(index);
            true
        } else {
            false
        }
    }

    fn active_editor(&self) -> Option<&EditorView> {
        self.active_index
            .and_then(|index| self.open_editors.get(index))
            .map(|editor| &editor.view)
    }

    fn active_editor_mut(&mut self) -> Result<&mut EditorView, String> {
        let index = self
            .active_index
            .ok_or_else(|| "workspace has no active editor".to_string())?;
        self.open_editors
            .get_mut(index)
            .map(|editor| &mut editor.view)
            .ok_or_else(|| "workspace active editor is missing".to_string())
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

    #[test]
    fn workspace_tracks_open_editors_and_switches_the_active_editor() {
        let first = ProjectPath::new(1, "src/first.rs");
        let second = ProjectPath::new(1, "src/second.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(first.clone(), "first");
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();

        workspace.open_editor(second.clone(), "second");
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('*')))
            .unwrap();

        assert_eq!(workspace.open_paths(), vec![first.clone(), second.clone()]);
        assert_eq!(workspace.active_path(), Some(&second));
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "*second"
        );

        workspace.switch_to_editor(&first).unwrap();

        assert_eq!(workspace.active_path(), Some(&first));
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "/first"
        );
    }

    #[test]
    fn opening_an_already_open_path_switches_to_that_editor() {
        let first = ProjectPath::new(1, "src/first.rs");
        let second = ProjectPath::new(1, "src/second.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(first.clone(), "first");
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();
        workspace.open_editor(second.clone(), "second");

        workspace.open_editor(first.clone(), "ignored replacement");

        assert_eq!(workspace.open_paths(), vec![first.clone(), second]);
        assert_eq!(workspace.active_path(), Some(&first));
        assert_eq!(
            workspace.active_rendered_editor(None).unwrap().lines[0].text,
            "/first"
        );
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
        assert_eq!(rendered.tabs[0].label_text(), " * src/first.rs");
        assert_eq!(rendered.tabs[1].path, second);
        assert_eq!(rendered.tabs[1].label_text(), ">  src/second.rs");
        assert_eq!(rendered.active_editor.unwrap().title, "src/second.rs");
    }
}
