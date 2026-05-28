use clap::{Parser, Subcommand};

mod cache;
mod commands;
mod hash;
mod info;
mod install;
mod output;
mod paths;
mod project;
mod requirements;
mod search;
mod wheel;

use cache::handle_cache;
use commands::{
    check_packages, download_packages, freeze_packages, list_packages, uninstall_packages,
};
use hash::compute_hashes;
use info::{generate_lock_file, inspect_packages, show_config};
use install::{dry_run_install, install_packages, verify_all_installed};
use paths::{default_site_packages, show_debug, user_site_packages};
use project::{install_editable, install_project};
use requirements::parse_requirements_file;
use search::{search_pypi, show_package};
use wheel::build_wheel;

#[derive(Parser)]
#[command(
    name = "ferrypip",
    version = env!("CARGO_PKG_VERSION"),
    about = "Ferrython package manager (pip-compatible)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Package install directory (default: site-packages)
    #[arg(long, global = true)]
    target: Option<String>,

    /// Quiet mode — minimal output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Verbose mode — show detailed output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Base URL of the Python Package Index (default: https://pypi.org/simple)
    #[arg(long, global = true)]
    index_url: Option<String>,

    /// Extra URLs of package indexes to use in addition to --index-url
    #[arg(long, global = true)]
    extra_index_url: Vec<String>,

    /// Mark a host as trusted (TLS verification skipped, accepted, no-op)
    #[arg(long, global = true)]
    trusted_host: Vec<String>,

    /// Disable the cache
    #[arg(long, global = true)]
    no_cache_dir: bool,

    /// Base timeout in seconds for network operations (default: 15)
    #[arg(long, global = true, default_value = "15")]
    timeout: u64,
}

#[derive(Subcommand)]
enum Commands {
    /// Install packages from PyPI or requirements file
    Install {
        /// Package names (with optional version specifiers)
        packages: Vec<String>,

        /// Install from requirements files (may be specified multiple times)
        #[arg(short, long)]
        requirement: Vec<String>,

        /// Upgrade already-installed packages
        #[arg(short = 'U', long)]
        upgrade: bool,

        /// Install the current project (reads pyproject.toml or setup.cfg)
        #[arg(short, long)]
        editable: Option<Option<String>>,

        /// Don't install package dependencies
        #[arg(long)]
        no_deps: bool,

        /// Include pre-release and development versions
        #[arg(long)]
        pre: bool,

        /// Only install binary (wheel) packages
        #[arg(long)]
        only_binary: bool,

        /// Install packages into <dir>
        #[arg(short = 't', long = "install-target")]
        install_target: Option<String>,

        /// Install to user site-packages (~/.local/lib/ferrython/site-packages)
        #[arg(long)]
        user: bool,

        /// Don't use the wheel cache
        #[arg(long)]
        no_cache_dir: bool,

        /// Show what would be installed without installing
        #[arg(long)]
        dry_run: bool,

        /// Force reinstallation of packages
        #[arg(long)]
        force_reinstall: bool,

        /// Verify RECORD hashes after installation
        #[arg(long)]
        verify: bool,
    },

    /// Uninstall packages
    Uninstall {
        packages: Vec<String>,

        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,

        /// Uninstall from user site-packages instead of system site-packages
        #[arg(long)]
        user: bool,
    },

    /// List installed packages
    List {
        /// Show outdated packages
        #[arg(short, long)]
        outdated: bool,

        /// Output format: columns (default), freeze, json
        #[arg(long, default_value = "columns")]
        format: String,

        /// Only show packages not required by other packages
        #[arg(long)]
        not_required: bool,

        /// Exclude editable packages
        #[arg(long)]
        exclude_editable: bool,
    },

    /// Show package information
    Show {
        /// Package name(s) to show
        packages: Vec<String>,

        /// Show installed files
        #[arg(short, long)]
        files: bool,
    },

    /// Search PyPI for packages
    Search { query: String },

    /// Download packages without installing
    Download {
        packages: Vec<String>,

        /// Destination directory
        #[arg(short, long, default_value = ".")]
        dest: String,
    },

    /// Freeze installed packages into requirements format
    Freeze {
        /// Exclude editable packages from output
        #[arg(long)]
        exclude_editable: bool,

        /// Only output packages matching these names
        #[arg(short, long)]
        local: bool,
    },

    /// Verify installed packages have compatible dependencies
    Check,

    /// Install from pyproject.toml in current or given directory
    Project {
        /// Path to project directory (default: current dir)
        path: Option<String>,
    },

    /// Manage the local package cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Compute hash digests for files (for requirements --hash)
    Hash {
        /// Files to hash
        files: Vec<String>,

        /// Hash algorithm
        #[arg(short, long, default_value = "sha256")]
        algorithm: String,
    },

    /// Build wheel from project source
    Wheel {
        /// Source directory (default: current dir)
        #[arg(default_value = ".")]
        src: String,

        /// Output directory for the wheel
        #[arg(short, long, default_value = "dist")]
        wheel_dir: String,
    },

    /// Show information about the ferrypip configuration
    Config {
        /// Show all configuration values
        #[arg(short, long)]
        list: bool,
    },

    /// Output installed packages in pip-compatible format
    Inspect,

    /// Generate a lock file with exact versions of all dependencies
    Lock {
        /// Output file (default: requirements.lock)
        #[arg(short, long, default_value = "requirements.lock")]
        output: String,

        /// Input requirements file
        #[arg(short, long)]
        requirement: Option<String>,
    },

    /// Show environment and configuration debug information
    Debug,
}

#[derive(Subcommand)]
enum CacheAction {
    /// Show cache directory and size
    Dir,
    /// Show cached package info
    Info,
    /// List cached packages
    List,
    /// Remove all cached packages
    Purge,
    /// Remove a specific package from cache
    Remove { pattern: String },
}

pub fn run() {
    let cli = Cli::parse();
    let site_packages = cli.target.unwrap_or_else(default_site_packages);
    let quiet = cli.quiet;
    let verbose = cli.verbose;

    let result = match cli.command {
        Commands::Install {
            packages,
            requirement,
            upgrade,
            editable,
            no_deps,
            pre,
            only_binary: _,
            install_target,
            user,
            no_cache_dir: _,
            dry_run,
            force_reinstall,
            verify,
        } => {
            let effective_site = if user {
                user_site_packages()
            } else {
                install_target
                    .as_deref()
                    .unwrap_or(&site_packages)
                    .to_string()
            };
            let effective_upgrade = upgrade || force_reinstall;
            if dry_run {
                dry_run_install(&packages, &requirement, quiet)
            } else if let Some(editable_path) = editable {
                let proj_path = editable_path.unwrap_or_else(|| ".".to_string());
                install_editable(&proj_path, &effective_site, quiet)
            } else if !requirement.is_empty() {
                // Collect all requirements from all -r files, then add CLI packages
                let mut reqs: Vec<String> = Vec::new();
                for req_file in &requirement {
                    reqs.extend(parse_requirements_file(req_file));
                }
                reqs.extend(packages.iter().cloned());
                let result = install_packages(
                    &reqs,
                    &effective_site,
                    effective_upgrade,
                    no_deps,
                    pre,
                    quiet,
                    verbose,
                );
                if verify && result.is_ok() {
                    verify_all_installed(&effective_site, &reqs, quiet);
                }
                result
            } else if packages.is_empty() {
                eprintln!(
                    "Error: You must give at least one requirement to install \
                           (see 'ferrypip install --help')"
                );
                std::process::exit(1);
            } else {
                let result = install_packages(
                    &packages,
                    &effective_site,
                    effective_upgrade,
                    no_deps,
                    pre,
                    quiet,
                    verbose,
                );
                if verify && result.is_ok() {
                    verify_all_installed(&effective_site, &packages, quiet);
                }
                result
            }
        }
        Commands::Uninstall {
            packages,
            yes,
            user,
        } => {
            let effective_site = if user {
                user_site_packages()
            } else {
                site_packages.clone()
            };
            uninstall_packages(&packages, &effective_site, yes, quiet)
        }
        Commands::List {
            outdated,
            format,
            not_required,
            exclude_editable,
        } => list_packages(
            &site_packages,
            outdated,
            &format,
            not_required,
            exclude_editable,
        ),
        Commands::Show { packages, files } => {
            if packages.is_empty() {
                eprintln!("Error: Missing required argument <PACKAGE>");
                std::process::exit(1);
            }
            let mut first = true;
            let mut last_err = None;
            for pkg in &packages {
                if !first {
                    println!("---");
                }
                if let Err(e) = show_package(pkg, &site_packages, files) {
                    last_err = Some(e);
                }
                first = false;
            }
            match last_err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        Commands::Search { query } => search_pypi(&query),
        Commands::Download { packages, dest } => download_packages(&packages, &dest, quiet),
        Commands::Freeze {
            exclude_editable,
            local: _,
        } => freeze_packages(&site_packages, exclude_editable),
        Commands::Check => check_packages(&site_packages),
        Commands::Project { path } => {
            let proj_path = path.unwrap_or_else(|| ".".to_string());
            install_project(&proj_path, &site_packages, quiet)
        }
        Commands::Cache { action } => handle_cache(action, quiet),
        Commands::Hash { files, algorithm } => compute_hashes(&files, &algorithm),
        Commands::Wheel { src, wheel_dir } => build_wheel(&src, &wheel_dir, quiet),
        Commands::Config { list } => show_config(&site_packages, list),
        Commands::Inspect => inspect_packages(&site_packages),
        Commands::Lock {
            output,
            requirement,
        } => generate_lock_file(&site_packages, &output, requirement.as_deref()),
        Commands::Debug => show_debug(&site_packages),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
