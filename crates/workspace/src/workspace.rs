use project::Project;

pub use project::ProjectPath;
pub use ui::{CommandOutcome, EditorCommand, EditorView, RenderedEditor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCommand {
    Editor(EditorCommand),
    SaveAll,
    CloseActiveEditor,
    SaveAndCloseActiveEditor,
    DiscardAndCloseActiveEditor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCommandOutcome {
    pub editor: Option<CommandOutcome>,
    pub saved_paths: Vec<ProjectPath>,
    pub closed_path: Option<ProjectPath>,
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
                    closed_path: None,
                })
            }
            WorkspaceCommand::SaveAll => Ok(WorkspaceCommandOutcome {
                editor: None,
                saved_paths: self
                    .project
                    .save_dirty_buffers()
                    .map_err(|error| error.to_string())?,
                closed_path: None,
            }),
            WorkspaceCommand::CloseActiveEditor => Ok(WorkspaceCommandOutcome {
                editor: None,
                saved_paths: Vec::new(),
                closed_path: Some(self.close_active_editor()?),
            }),
            WorkspaceCommand::SaveAndCloseActiveEditor => Ok(WorkspaceCommandOutcome {
                editor: None,
                saved_paths: Vec::new(),
                closed_path: Some(self.save_and_close_active_editor()?),
            }),
            WorkspaceCommand::DiscardAndCloseActiveEditor => Ok(WorkspaceCommandOutcome {
                editor: None,
                saved_paths: Vec::new(),
                closed_path: Some(self.discard_and_close_active_editor()?),
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

    pub fn close_editor(&mut self, path: &ProjectPath) -> Result<ProjectPath, String> {
        let index = self
            .open_editors
            .iter()
            .position(|editor| editor.path == *path)
            .ok_or_else(|| format!("workspace has no open editor for {}", path.path.display()))?;
        self.close_editor_at(index, DirtyClosePolicy::Reject)
    }

    pub fn close_active_editor(&mut self) -> Result<ProjectPath, String> {
        let index = self
            .active_index
            .ok_or_else(|| "workspace has no active editor".to_string())?;
        self.close_editor_at(index, DirtyClosePolicy::Reject)
    }

    pub fn save_and_close_editor(&mut self, path: &ProjectPath) -> Result<ProjectPath, String> {
        let index = self
            .open_editors
            .iter()
            .position(|editor| editor.path == *path)
            .ok_or_else(|| format!("workspace has no open editor for {}", path.path.display()))?;
        self.close_editor_at(index, DirtyClosePolicy::Save)
    }

    pub fn save_and_close_active_editor(&mut self) -> Result<ProjectPath, String> {
        let index = self
            .active_index
            .ok_or_else(|| "workspace has no active editor".to_string())?;
        self.close_editor_at(index, DirtyClosePolicy::Save)
    }

    pub fn discard_and_close_editor(&mut self, path: &ProjectPath) -> Result<ProjectPath, String> {
        let index = self
            .open_editors
            .iter()
            .position(|editor| editor.path == *path)
            .ok_or_else(|| format!("workspace has no open editor for {}", path.path.display()))?;
        self.close_editor_at(index, DirtyClosePolicy::Discard)
    }

    pub fn discard_and_close_active_editor(&mut self) -> Result<ProjectPath, String> {
        let index = self
            .active_index
            .ok_or_else(|| "workspace has no active editor".to_string())?;
        self.close_editor_at(index, DirtyClosePolicy::Discard)
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

    fn close_editor_at(
        &mut self,
        index: usize,
        dirty_close_policy: DirtyClosePolicy,
    ) -> Result<ProjectPath, String> {
        let path = self.open_editors[index].path.clone();
        let is_dirty = self.open_editors[index].view.rendered_editor(None).is_dirty;

        if is_dirty {
            match dirty_close_policy {
                DirtyClosePolicy::Reject => {
                    return Err(format!(
                        "cannot close dirty editor without saving {}",
                        path.path.display()
                    ));
                }
                DirtyClosePolicy::Save => self
                    .project
                    .save_buffer(&path)
                    .map_err(|error| error.to_string())?,
                DirtyClosePolicy::Discard => {
                    self.project
                        .revert_buffer(&path)
                        .map_err(|error| error.to_string())?;
                }
            }
        }

        Ok(self.remove_editor_at(index))
    }

    fn remove_editor_at(&mut self, index: usize) -> ProjectPath {
        let removed = self.open_editors.remove(index);
        self.active_index = match self.active_index {
            None => None,
            Some(_) if self.open_editors.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.open_editors.len() - 1)),
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };
        self.project.close_buffer(&removed.path);
        removed.path
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirtyClosePolicy {
    Reject,
    Save,
    Discard,
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

    #[test]
    fn closing_active_editor_activates_the_next_available_editor() {
        let first = ProjectPath::new(1, "src/first.rs");
        let second = ProjectPath::new(1, "src/second.rs");
        let third = ProjectPath::new(1, "src/third.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(first.clone(), "first");
        workspace.open_editor(second.clone(), "second");
        workspace.open_editor(third.clone(), "third");
        workspace.switch_to_editor(&second).unwrap();

        let outcome = workspace
            .dispatch_command(WorkspaceCommand::CloseActiveEditor)
            .unwrap();

        assert_eq!(outcome.closed_path, Some(second.clone()));
        assert_eq!(workspace.open_paths(), vec![first.clone(), third.clone()]);
        assert_eq!(workspace.active_path(), Some(&third));
    }

    #[test]
    fn closing_inactive_editor_preserves_the_active_editor() {
        let first = ProjectPath::new(1, "src/first.rs");
        let second = ProjectPath::new(1, "src/second.rs");
        let third = ProjectPath::new(1, "src/third.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(first.clone(), "first");
        workspace.open_editor(second.clone(), "second");
        workspace.open_editor(third.clone(), "third");

        let closed_path = workspace.close_editor(&first).unwrap();

        assert_eq!(closed_path, first);
        assert_eq!(workspace.open_paths(), vec![second, third.clone()]);
        assert_eq!(workspace.active_path(), Some(&third));
    }

    #[test]
    fn closing_the_last_editor_clears_the_active_editor() {
        let path = ProjectPath::new(1, "src/main.rs");
        let mut workspace = Workspace::new();

        workspace.open_editor(path.clone(), "main");

        assert_eq!(workspace.close_active_editor().unwrap(), path);
        assert_eq!(workspace.open_paths(), Vec::<ProjectPath>::new());
        assert_eq!(workspace.active_path(), None);
        assert!(workspace.active_rendered_editor(None).is_none());
        assert!(workspace.rendered_workspace(None).active_editor.is_none());
        assert!(workspace.project().buffer_store().is_empty());
    }

    #[test]
    fn closing_dirty_editor_without_saving_is_rejected() {
        let file_path = test_file_path("close-dirty-reject");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let path = ProjectPath::new(1, file_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(path.clone()).unwrap();
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();

        let error = workspace.close_active_editor().unwrap_err();

        assert_eq!(
            error,
            format!(
                "cannot close dirty editor without saving {}",
                file_path.display()
            )
        );
        assert_eq!(workspace.open_paths(), vec![path]);
        assert!(workspace.project().has_dirty_buffers());
    }

    #[test]
    fn save_and_close_active_editor_writes_dirty_file_before_closing() {
        let file_path = test_file_path("save-and-close");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let path = ProjectPath::new(1, file_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(path.clone()).unwrap();
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();

        let outcome = workspace
            .dispatch_command(WorkspaceCommand::SaveAndCloseActiveEditor)
            .unwrap();

        assert_eq!(outcome.closed_path, Some(path));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "/hello world");
        assert_eq!(workspace.open_paths(), Vec::<ProjectPath>::new());
        assert!(!workspace.project().has_dirty_buffers());
    }

    #[test]
    fn discard_and_close_active_editor_reverts_dirty_file_before_closing() {
        let file_path = test_file_path("discard-and-close");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "hello world").unwrap();

        let path = ProjectPath::new(1, file_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(path.clone()).unwrap();
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();

        let outcome = workspace
            .dispatch_command(WorkspaceCommand::DiscardAndCloseActiveEditor)
            .unwrap();

        assert_eq!(outcome.closed_path, Some(path));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
        assert_eq!(workspace.open_paths(), Vec::<ProjectPath>::new());
        assert!(!workspace.project().has_dirty_buffers());
    }

    #[test]
    fn discard_and_close_by_path_keeps_other_editors_open() {
        let first_path = test_file_path("discard-by-path-first");
        let second_path = test_file_path("discard-by-path-second");
        std::fs::create_dir_all(first_path.parent().unwrap()).unwrap();
        std::fs::write(&first_path, "first").unwrap();
        std::fs::write(&second_path, "second").unwrap();

        let first = ProjectPath::new(1, first_path.clone());
        let second = ProjectPath::new(1, second_path.clone());
        let mut workspace = Workspace::new();
        workspace.open_editor_from_file(first.clone()).unwrap();
        workspace
            .dispatch_command(WorkspaceCommand::Editor(EditorCommand::InsertChar('/')))
            .unwrap();
        workspace.open_editor_from_file(second.clone()).unwrap();

        let closed_path = workspace.discard_and_close_editor(&first).unwrap();

        assert_eq!(closed_path, first);
        assert_eq!(std::fs::read_to_string(&first_path).unwrap(), "first");
        assert_eq!(workspace.open_paths(), vec![second.clone()]);
        assert_eq!(workspace.active_path(), Some(&second));
        assert!(!workspace.project().has_dirty_buffers());
        assert_eq!(workspace.project().buffer_store().len(), 1);
        assert!(
            workspace
                .project()
                .buffer_store()
                .buffer_for_path(&second)
                .is_some()
        );
    }
}
