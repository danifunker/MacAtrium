//! MacAtrium host build tooling — library crate.
//!
//! Every command's logic lives here so both the CLI (`src/main.rs`) and the GUI
//! (`tools/macatrium-mgmt-ui`) drive the *same* functions: the CLI is the source
//! of truth, the GUI is a thin front-end over it.

pub mod catalog;
pub mod config;
pub mod donors;
pub mod enrich;
pub mod fetch;
pub mod harvest;
pub mod icons;
pub mod image;
pub mod library;
pub mod macroman;
pub mod merge;
pub mod mg;
pub mod mgdb;
pub mod pict;
pub mod preflight;
pub mod rbcli;
pub mod selection;
pub mod settings;
pub mod size_rsrc;
pub mod snd;
pub mod targets;
pub mod templates;
