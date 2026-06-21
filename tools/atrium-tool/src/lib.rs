//! MacAtrium host build tooling — library crate.
//!
//! Every command's logic lives here so both the CLI (`src/main.rs`) and the GUI
//! (`tools/macatrium-mgmt-ui`) drive the *same* functions: the CLI is the source
//! of truth, the GUI is a thin front-end over it.

pub mod catalog;
pub mod enrich;
pub mod harvest;
pub mod image;
pub mod macroman;
pub mod merge;
pub mod pict;
pub mod rbcli;
