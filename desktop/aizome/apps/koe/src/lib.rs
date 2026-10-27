//! Koe 声, Aizome's dictation: the session service that listens and
//! recognises (`daemon`), its versioned protocol, the window that sets it
//! up (`app`), and the vocabulary packs that steer recognition.
pub mod app;
pub mod audio;
pub mod config;
pub mod daemon;
pub mod desktop;
pub mod dictation;
pub mod engine;
pub mod instance;
pub mod ipc;
pub mod models;
pub mod protocol;
pub mod service;
pub mod text;
pub mod tray;
pub mod vocab;
pub mod vocabulary;
