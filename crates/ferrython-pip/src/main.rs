//! Ferryip — pip-compatible package manager for Ferrython
//!
//! Supports:
//! - `ferryip install <package>` — download from PyPI (pure-python wheels)
//! - `ferryip install -r requirements.txt` — batch install
//! - `ferryip install -e .` — install from pyproject.toml
//! - `ferryip project .` — install project dependencies
//! - `ferryip list` — list installed packages
//! - `ferryip uninstall <package>` — remove packages
//! - `ferryip show <package>` — package metadata
//! - Recursive dependency resolution with version specifiers

mod cli;
mod installer;
pub mod metadata;
mod pypi;
mod registry;
mod resolver;
pub mod setup_cfg;
pub mod version;

fn main() {
    cli::run();
}
