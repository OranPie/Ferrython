//! Ferrypip — pip-compatible package manager for Ferrython
//!
//! Supports:
//! - `ferrypip install <package>` — download from PyPI (pure-python wheels)
//! - `ferrypip install -r requirements.txt` — batch install
//! - `ferrypip install -e .` — install from pyproject.toml
//! - `ferrypip project .` — install project dependencies
//! - `ferrypip list` — list installed packages
//! - `ferrypip uninstall <package>` — remove packages
//! - `ferrypip show <package>` — package metadata
//! - Recursive dependency resolution with version specifiers

mod cli;
mod installer;
pub mod metadata;
mod pypi;
mod registry;
mod resolver;
pub mod setup_cfg;
pub mod version;

pub fn run() {
    cli::run();
}
