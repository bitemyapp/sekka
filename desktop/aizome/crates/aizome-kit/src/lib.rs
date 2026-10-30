//! Aizome's widget kit, on Sidestep's native toolkit (`sidestep-ui`).
//!
//! Apps are written Elm's way: a [`Program`] holds state, `update`
//! changes it with messages, and `view` describes each window as a tree
//! of [`Element`]s, rebuilt after every change. The kit lays the tree
//! out, diffs it against the last one by key path, and redraws only what
//! changed; widget state that must outlive a rebuild (carets, selections,
//! scroll offsets, hover, open menus) lives under each element's key.
//!
//! ```no_run
//! use aizome_kit::prelude::*;
//!
//! #[derive(Default)]
//! struct Hello { on: bool }
//!
//! #[derive(Clone, Debug)]
//! enum Msg { Toggle(bool) }
//!
//! impl Program for Hello {
//!     type Msg = Msg;
//!     fn info(&self) -> AppInfo { AppInfo::new("org.aizome.Hello", "Hello", "Example") }
//!     fn launched(&mut self, cx: &mut Cx<Msg>) { cx.open_window(WindowSpec::new("Hello").size(360.0, 160.0)); }
//!     fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) { match msg { Msg::Toggle(on) => self.on = on } }
//!     fn view(&self, _w: WindowKey) -> Element<Msg> {
//!         row![text("Wi-Fi"), spacer(), toggle(self.on).on_toggle(Msg::Toggle)].padding(16.0).into()
//!     }
//! }
//!
//! fn main() { aizome_kit::run(Hello::default()) }
//! ```
//!
//! See `docs/kit.md` for the guide.

pub mod a11y;
pub mod ahead;
mod app;
pub mod cx;
pub mod dialogs;
pub mod draw;
pub mod editing;
pub mod editor;
pub mod element;
pub(crate) mod engine;
pub mod fonts;
pub mod hash;
pub mod icons;
pub mod inspect;
pub mod layout;
pub mod menu;
pub mod platform;
pub mod program;
pub(crate) mod shell;
pub mod stats;
pub mod testing;
pub mod text;
pub mod theme;
pub mod tree;
pub mod urls;
pub mod widget;
pub mod widgets;

pub use app::{LINE, run, try_run};
pub use cx::{Cx, MsgSender, TimerHandle, WindowManager};
pub use dialogs::{Alert, FileDialog};
pub use editor::{EditorAction, EditorContent, Granularity, Highlighter, LineStyle, Selection};
pub use element::{Element, Layoutable, children};
pub use icons::{Icon, icon};
pub use layout::{Align, Constraints, Length, Padding, Point, Rect, Size, Vec2};
pub use menu::{Menu, MenuBar, MenuItem, MenuRole, Shortcut, ShortcutKey};
pub use program::{ActivationRequest, Anchors, DragId, KeyboardInteractivity, Layer, LayerOptions, Margins, Output};
pub use program::{AppInfo, Program, WindowKey, WindowKind, WindowSpec, WindowState};
pub use sidestep_ui;
pub use sidestep_ui::{ClipboardRead, PreparedClipboard, PreparedClipboardText};
pub use sidestep_ui::{Color, DragSource, DropAction, Image};
pub use text::{Span, rich_text, text};
pub use theme::{Appearance, FontSpec, Role, Theme};
pub use widgets::controls::{
    Fit, badge, button, checkbox, dropdown, icon_button, image, keycap, meter, progress, progress_indeterminate, radio,
    segmented, segmented_icons, slider, stepper, toggle,
};
pub use widgets::interact::{DragEvent, Dropped, canvas, context_menu, mouse_area, popover, tooltip};
pub use widgets::layout::{Edge, Sides, Tone, column, container, divider, row, space, spacer, stack};
pub use widgets::menus::{MenuPlacement, MenuStyle, menu_view};
pub use widgets::panes::{
    caption, form_row, form_section, sidebar_item, sidebar_layout, split, status_bar, status_text, toolbar,
    toolbar_separator,
};
pub use widgets::scroll::{Direction, list, scroll, virtual_list};
pub use widgets::table::{Column, TreeItem, table, tree};
pub use widgets::text_editor::{FindRequest, text_editor};
pub use widgets::text_field::{search_field, text_field};
pub use widgets::video::{over_video, video_surface};

/// Video in windows: frames from a player's thread, shown by the compositor
/// (see [`widgets::video`]).
pub mod video {
    pub use crate::widgets::video::{VideoEl, VideoId, VideoSpec, fit, over_video};
    pub use sidestep_ui::video::*;
}

/// Everything an app usually needs: `use aizome_kit::prelude::*;`.
pub mod prelude {
    pub use crate::layout::{Fill, Shrink};
    pub use crate::{
        Alert, Align, AppInfo, Color, Column, Cx, DragEvent, DragId, DragSource, DropAction, Dropped, Edge,
        EditorAction, EditorContent, Element, FileDialog, FindRequest, Fit, FontSpec, Icon, Image, Layoutable, Length,
        Menu, MenuBar, MenuItem, MenuPlacement, MenuRole, MenuStyle, Padding, Point, Program, Rect, Role, Shortcut,
        ShortcutKey, Sides, Size, Span, Theme, TimerHandle, Tone, TreeItem, Vec2, WindowKey, WindowKind, WindowSpec,
        badge, button, canvas, caption, checkbox, container, context_menu, divider, dropdown, form_row, form_section,
        icon, icon_button, image, keycap, list, menu_view, meter, mouse_area, popover, progress,
        progress_indeterminate, radio, rich_text, scroll, search_field, segmented, segmented_icons, sidebar_item,
        sidebar_layout, slider, space, spacer, split, status_bar, status_text, stepper, table, text, text_editor,
        text_field, toggle, toolbar, toolbar_separator, tooltip, tree, video_surface, virtual_list,
    };
    pub use crate::{column, row, stack};
}
