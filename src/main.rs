#![warn(missing_docs)]
//! Installed application browser with uninstall command previews.

mod app;
mod cli_app;
mod cli_group;
mod package_owner;
mod uninstall_command;

use app::main;
