//! Ferryip — pip-compatible package manager for Ferrython
//!
//! Supports:
//! - `ferryip install <package>` — download from PyPI (pure-python wheels)
//! - `ferryip install -r requirements.txt` — batch install
//! - `ferryip list` — list installed packages
//! - `ferryip uninstall <package>` — remove packages
//! - `ferryip show <package>` — package metadata
//! - pyproject.toml and setup.cfg parsing

mod pypi;
mod installer;
mod registry;
mod cli;

fn main() {
    cli::run();
}
