//! Tauri IPC command layer: thin, testable command functions that delegate to
//! the core modules. Registration and app state live in `crate::run`.

pub(crate) mod dialogs;
pub(crate) mod meetings;
pub(crate) mod models;
pub(crate) mod notes;
pub(crate) mod settings;
pub(crate) mod streaming;
pub(crate) mod transcription;
