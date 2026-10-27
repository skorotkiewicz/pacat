#![warn(missing_docs)]
//! Browse installed packages without executing uninstall commands.

mod app;
mod catalog;
mod package;

use app::main;
