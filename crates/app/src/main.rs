#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use editor::EditorModel;
use language::Buffer;
use text::BufferId;

fn main() {
    let buffer = Buffer::local(BufferId::new(1).expect("buffer id is non-zero"), "");
    let editor = EditorModel::for_buffer("examples/gpui.rs", buffer.into_handle());
    ui::run(editor);
}
