use editor::EditorModel;
use language::Buffer;
use text::BufferId;

fn main() {
    let buffer = Buffer::local(
        BufferId::new(1).expect("buffer id is non-zero"),
        "fn main() {\n    println!(\"hello from mini-zed gpui\");\n}\n",
    );
    let editor = EditorModel::for_buffer("examples/gpui.rs", buffer.into_handle());
    ui::run(editor);
}
