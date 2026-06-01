use project::ProjectPath;
use ui::EditorView;

#[derive(Debug)]
pub(crate) struct WorkspaceEditor {
    pub(crate) path: ProjectPath,
    pub(crate) view: EditorView,
}
