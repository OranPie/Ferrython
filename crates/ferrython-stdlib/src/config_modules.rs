//! Configuration and argument parsing stdlib modules.

#[allow(dead_code)]
mod argparse;
mod configparser;

pub use configparser::create_configparser_module;
