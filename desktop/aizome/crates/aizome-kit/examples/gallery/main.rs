//! Every widget of the kit, in Night, Day and High Contrast: controls in
//! their states, text and fields, lists, a Settings-like pane and an
//! editor.
//!
//! `AIZOME_FONT_DIR=~/work/.aizome-fonts cargo run -p aizome-kit --example gallery`
//! (under a headless compositor, or a session of your own).

mod views;

fn main() {
    aizome_kit::run(views::Gallery::default());
}
