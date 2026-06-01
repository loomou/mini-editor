use crate::{
    App, Application, Bounds, EditorModel, EditorView, WindowBounds, WindowOptions, px, size,
};
use gpui::AppContext;

pub fn run(editor: EditorModel) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let focus_handle = cx.focus_handle();
                window.focus(&focus_handle);
                cx.new(|_| EditorView::with_focus(editor, focus_handle))
            },
        )
        .expect("open GPUI window");
        cx.activate(true);
    });
}
