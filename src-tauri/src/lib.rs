pub mod filesystem;
pub mod index;
pub mod search;
pub mod settings;
pub mod update;
pub mod watcher;

#[cfg(feature = "app")]
mod app;
#[cfg(feature = "app")]
pub mod commands;
#[cfg(feature = "app")]
pub use app::{run, spawn_scan};

