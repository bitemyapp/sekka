//! Protocols smithay doesn't provide (wlr-screencopy, ext-image-copy-capture
//! with its capture sources, wlr foreign toplevel management, wlr output
//! management, wlr gamma control) and one it gets wrong
//! (xdg-toplevel-icon).

pub mod capture;
pub mod foreign_toplevel;
pub mod gamma_control;
pub mod image_copy_capture;
pub mod output_management;
pub mod screencopy;
pub mod toplevel_icon;
