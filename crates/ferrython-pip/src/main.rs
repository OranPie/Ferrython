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

mod pypi;
mod installer;
mod registry;
mod resolver;
pub mod version;
mod cli;

fn main() {
    cli::run();
}
