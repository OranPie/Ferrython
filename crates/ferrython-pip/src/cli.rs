use clap::{Parser, Subcommand};
use crate::{pypi, installer, registry};

#[derive(Parser)]
#[command(name = "ferryip", about = "Ferrython package manager (pip-compatible)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Package install directory (default: site-packages)
    #[arg(long, global = true)]
    target: Option<String>,

    /// Quiet mode — minimal output
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Install packages from PyPI or requirements file
    Install {
        /// Package names (with optional version specifiers)
        packages: Vec<String>,

        /// Install from requirements file
        #[arg(short, long)]
        requirement: Option<String>,

        /// Upgrade already-installed packages
        #[arg(short = 'U', long)]
        upgrade: bool,
    },

    /// Uninstall packages
    Uninstall {
        packages: Vec<String>,

        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// List installed packages
    List {
        /// Show outdated packages
        #[arg(short, long)]
        outdated: bool,
    },

    /// Show package information
    Show {
        package: String,
    },

    /// Search PyPI for packages
    Search {
        query: String,
    },

    /// Download packages without installing
    Download {
        packages: Vec<String>,

        /// Destination directory
        #[arg(short, long, default_value = ".")]
        dest: String,
    },

    /// Freeze installed packages into requirements format
    Freeze,

    /// Verify installed packages have compatible dependencies
    Check,
}

pub fn run() {
    let cli = Cli::parse();
    let site_packages = cli.target.unwrap_or_else(default_site_packages);
    let quiet = cli.quiet;

    let result = match cli.command {
        Commands::Install { packages, requirement, upgrade } => {
            if let Some(req_file) = requirement {
                let reqs = parse_requirements_file(&req_file);
                install_packages(&reqs, &site_packages, upgrade, quiet)
            } else if packages.is_empty() {
                eprintln!("Error: no packages specified");
                std::process::exit(1);
            } else {
                install_packages(&packages, &site_packages, upgrade, quiet)
            }
        }
        Commands::Uninstall { packages, yes } => {
            uninstall_packages(&packages, &site_packages, yes, quiet)
        }
        Commands::List { outdated } => {
            list_packages(&site_packages, outdated)
        }
        Commands::Show { package } => {
            show_package(&package, &site_packages)
        }
        Commands::Search { query } => {
            search_pypi(&query)
        }
        Commands::Download { packages, dest } => {
            download_packages(&packages, &dest, quiet)
        }
        Commands::Freeze => {
            freeze_packages(&site_packages)
        }
        Commands::Check => {
            check_packages(&site_packages)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn default_site_packages() -> String {
    // Look for ferrython's site-packages relative to the binary
    let exe = std::env::current_exe().unwrap_or_default();
    let base = exe.parent().unwrap_or(std::path::Path::new("."));
    let site = base.join("lib").join("ferrython").join("site-packages");
    if !site.exists() {
        let _ = std::fs::create_dir_all(&site);
    }
    site.to_string_lossy().to_string()
}

fn parse_requirements_file(path: &str) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read {}: {}", path, e);
            std::process::exit(1);
        }
    };
    content.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'))
        .map(String::from)
        .collect()
}

fn install_packages(specs: &[String], site_packages: &str, upgrade: bool, quiet: bool) -> Result<(), String> {
    for spec in specs {
        let (name, version_req) = pypi::parse_requirement(spec);
        if !quiet {
            if let Some(ref v) = version_req {
                println!("Collecting {}=={}", name, v);
            } else {
                println!("Collecting {}", name);
            }
        }

        // Check if already installed
        if !upgrade {
            if let Some(installed) = registry::get_installed(&name, site_packages) {
                if version_req.as_ref().map_or(true, |v| v == &installed.version) {
                    if !quiet {
                        println!("Requirement already satisfied: {} ({})", name, installed.version);
                    }
                    continue;
                }
            }
        }

        // Fetch from PyPI
        let release = pypi::fetch_package_info(&name, version_req.as_deref())
            .map_err(|e| format!("Could not find {}: {}", name, e))?;

        if !quiet {
            println!("  Downloading {}-{}", release.name, release.version);
        }

        // Download and install
        let wheel_path = pypi::download_wheel(&release)
            .map_err(|e| format!("Download failed: {}", e))?;

        installer::install_wheel(&wheel_path, site_packages, &release.name, &release.version)
            .map_err(|e| format!("Install failed: {}", e))?;

        if !quiet {
            println!("Successfully installed {}-{}", release.name, release.version);
        }
    }
    Ok(())
}

fn uninstall_packages(names: &[String], site_packages: &str, yes: bool, quiet: bool) -> Result<(), String> {
    for name in names {
        let installed = registry::get_installed(name, site_packages);
        if installed.is_none() {
            if !quiet {
                println!("WARNING: Skipping {} as it is not installed.", name);
            }
            continue;
        }
        let info = installed.unwrap();
        if !yes {
            println!("Found existing installation: {}-{}", name, info.version);
            println!("  Would remove:");
            for f in &info.files {
                println!("    {}", f);
            }
            print!("Proceed (Y/n)? ");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "n" {
                continue;
            }
        }
        registry::uninstall(name, site_packages)
            .map_err(|e| format!("Uninstall failed: {}", e))?;
        if !quiet {
            println!("Successfully uninstalled {}-{}", name, info.version);
        }
    }
    Ok(())
}

fn list_packages(site_packages: &str, _outdated: bool) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    if packages.is_empty() {
        println!("No packages installed.");
    } else {
        println!("{:<30} {}", "Package", "Version");
        println!("{:<30} {}", "-------", "-------");
        for pkg in &packages {
            println!("{:<30} {}", pkg.name, pkg.version);
        }
    }
    Ok(())
}

fn show_package(name: &str, site_packages: &str) -> Result<(), String> {
    let info = registry::get_installed(name, site_packages)
        .ok_or_else(|| format!("Package '{}' is not installed", name))?;
    println!("Name: {}", info.name);
    println!("Version: {}", info.version);
    if let Some(ref summary) = info.summary {
        println!("Summary: {}", summary);
    }
    if let Some(ref author) = info.author {
        println!("Author: {}", author);
    }
    if let Some(ref license) = info.license {
        println!("License: {}", license);
    }
    if let Some(ref requires) = info.requires {
        println!("Requires: {}", requires.join(", "));
    }
    println!("Location: {}", site_packages);
    println!("Files:");
    for f in &info.files {
        println!("  {}", f);
    }
    Ok(())
}

fn search_pypi(query: &str) -> Result<(), String> {
    let results = pypi::search(query).map_err(|e| format!("Search failed: {}", e))?;
    if results.is_empty() {
        println!("No packages found for '{}'", query);
    } else {
        for (name, version, summary) in &results {
            println!("{} ({}) - {}", name, version, summary);
        }
    }
    Ok(())
}

fn download_packages(specs: &[String], dest: &str, quiet: bool) -> Result<(), String> {
    for spec in specs {
        let (name, version_req) = pypi::parse_requirement(spec);
        let release = pypi::fetch_package_info(&name, version_req.as_deref())
            .map_err(|e| format!("Could not find {}: {}", name, e))?;
        if !quiet {
            println!("Downloading {}-{}", release.name, release.version);
        }
        let wheel_path = pypi::download_wheel(&release)
            .map_err(|e| format!("Download failed: {}", e))?;
        let dest_path = std::path::Path::new(dest).join(
            wheel_path.file_name().unwrap_or_default()
        );
        std::fs::copy(&wheel_path, &dest_path)
            .map_err(|e| format!("Copy failed: {}", e))?;
        if !quiet {
            println!("  Saved {}", dest_path.display());
        }
    }
    Ok(())
}

fn freeze_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    for pkg in &packages {
        println!("{}=={}", pkg.name, pkg.version);
    }
    Ok(())
}

fn check_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    let mut has_errors = false;
    for pkg in &packages {
        if let Some(ref requires) = pkg.requires {
            for req in requires {
                let (req_name, _) = pypi::parse_requirement(req);
                if registry::get_installed(&req_name, site_packages).is_none() {
                    println!("{} {} requires {}, which is not installed.", pkg.name, pkg.version, req);
                    has_errors = true;
                }
            }
        }
    }
    if !has_errors {
        println!("No broken requirements found.");
    }
    Ok(())
}
