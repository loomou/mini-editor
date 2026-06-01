mod ui;

mod app;
mod clipboard;
mod command;
mod input;
mod layout;
mod mouse;
mod render;
mod render_model;
mod scroll;
mod view;

#[cfg(test)]
pub(crate) use clipboard::*;
pub(crate) use command::*;
pub(crate) use input::*;
pub(crate) use layout::*;
#[cfg(test)]
pub(crate) use render::*;
pub(crate) use render_model::*;
pub(crate) use ui::*;
pub(crate) use view::*;

pub use app::run;
pub use command::{CommandOutcome, EditorCommand};
pub use render_model::{RenderedEditor, RenderedLine, RenderedLineFragment, RenderedScrollbar};
pub use view::EditorView;
