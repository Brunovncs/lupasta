//! The core of lupasta: filesystem access, the search index, the watcher and settings, plus the
//! pure scene logic (tree model, layout, connector routing, animation) the GPUI shell draws.

pub mod animator;
pub mod controller;
pub mod filesystem;
pub mod fixture;
pub mod index;
pub mod layout;
pub mod line_edit;
pub mod metrics;
pub mod palette;
pub mod prefs;
pub mod router;
pub mod search;
pub mod session;
pub mod settings;
pub mod tree;
pub mod update;
pub mod watcher;
