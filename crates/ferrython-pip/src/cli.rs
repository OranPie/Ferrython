use clap::{Parser, Subcommand};
use crate::{pypi, registry, resolver, metadata::PackageMetadata};

#[derive(Parser)]
#[command(name = "ferryip", version = env!("CARGO_PKG_VERSION"), about = "Ferrython package manager (pip-compatible)")]
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

    /// Show information about the ferryip configuration
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
    Remove {
        pattern: String,
    },
}

pub fn run() {
    let cli = Cli::parse();
    let site_packages = cli.target.unwrap_or_else(default_site_packages);
    let quiet = cli.quiet;
    let verbose = cli.verbose;

    let result = match cli.command {
        Commands::Install { packages, requirement, upgrade, editable, no_deps, pre, only_binary: _, install_target, user, no_cache_dir: _, dry_run, force_reinstall, verify } => {
            let effective_site = if user {
                user_site_packages()
            } else {
                install_target.as_deref().unwrap_or(&site_packages).to_string()
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
                let result = install_packages(&reqs, &effective_site, effective_upgrade, no_deps, pre, quiet, verbose);
                if verify && result.is_ok() {
                    verify_all_installed(&effective_site, &reqs, quiet);
                }
                result
            } else if packages.is_empty() {
                eprintln!("Error: You must give at least one requirement to install \
                           (see 'ferryip install --help')");
                std::process::exit(1);
            } else {
                let result = install_packages(&packages, &effective_site, effective_upgrade, no_deps, pre, quiet, verbose);
                if verify && result.is_ok() {
                    verify_all_installed(&effective_site, &packages, quiet);
                }
                result
            }
        }
        Commands::Uninstall { packages, yes, user } => {
            let effective_site = if user { user_site_packages() } else { site_packages.clone() };
            uninstall_packages(&packages, &effective_site, yes, quiet)
        }
        Commands::List { outdated, format, not_required, exclude_editable } => {
            list_packages(&site_packages, outdated, &format, not_required, exclude_editable)
        }
        Commands::Show { packages, files } => {
            if packages.is_empty() {
                eprintln!("Error: Missing required argument <PACKAGE>");
                std::process::exit(1);
            }
            let mut first = true;
            let mut last_err = None;
            for pkg in &packages {
                if !first { println!("---"); }
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
        Commands::Search { query } => {
            search_pypi(&query)
        }
        Commands::Download { packages, dest } => {
            download_packages(&packages, &dest, quiet)
        }
        Commands::Freeze { exclude_editable, local: _ } => {
            freeze_packages(&site_packages, exclude_editable)
        }
        Commands::Check => {
            check_packages(&site_packages)
        }
        Commands::Project { path } => {
            let proj_path = path.unwrap_or_else(|| ".".to_string());
            install_project(&proj_path, &site_packages, quiet)
        }
        Commands::Cache { action } => {
            handle_cache(action, quiet)
        }
        Commands::Hash { files, algorithm } => {
            compute_hashes(&files, &algorithm)
        }
        Commands::Wheel { src, wheel_dir } => {
            build_wheel(&src, &wheel_dir, quiet)
        }
        Commands::Config { list } => {
            show_config(&site_packages, list)
        }
        Commands::Inspect => {
            inspect_packages(&site_packages)
        }
        Commands::Lock { output, requirement } => {
            generate_lock_file(&site_packages, &output, requirement.as_deref())
        }
        Commands::Debug => {
            show_debug(&site_packages)
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

fn user_site_packages() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let site = std::path::Path::new(&home)
        .join(".local")
        .join("lib")
        .join("ferrython")
        .join("site-packages");
    if !site.exists() {
        let _ = std::fs::create_dir_all(&site);
    }
    site.to_string_lossy().to_string()
}

fn show_debug(site_packages: &str) -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_default();
    println!("ferryip {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("  executable: {}", exe.display());
    println!("  site-packages: {}", site_packages);
    println!("  user-site-packages: {}", user_site_packages());
    println!();
    println!("Environment:");
    for var in &["FERRYTHON_COMPAT", "PYTHONPATH", "PYTHONDONTWRITEBYTECODE"] {
        match std::env::var(var) {
            Ok(val) => println!("  {}={}", var, val),
            Err(_)  => println!("  {} (unset)", var),
        }
    }
    Ok(())
}

/// Parse a requirements file supporting:
///  - `-r <file>` recursive includes
///  - `-c <file>` constraints (pinned versions applied as upper bounds)
///  - `-e <path>` editable installs (returned as `editable:<path>`)
///  - `--index-url`, `--extra-index-url`, `--trusted-host` (acknowledged, ignored)
///  - `--hash=sha256:...` inline hashes (preserved for verification)
///  - `--no-deps` (returned as flag prefix `nodeps:`)
///  - environment markers after `;`
///  - line continuations with `\`
fn parse_requirements_file(path: &str) -> Vec<String> {
    parse_requirements_file_inner(path, &mut std::collections::HashSet::new())
}

fn parse_requirements_file_inner(path: &str, seen: &mut std::collections::HashSet<String>) -> Vec<String> {
    let canonical = std::path::Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(path));
    let key = canonical.to_string_lossy().to_string();
    if !seen.insert(key) {
        return vec![]; // avoid infinite recursion
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read {}: {}", path, e);
            std::process::exit(1);
        }
    };

    let base_dir = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));

    // Join continuation lines
    let joined = content.replace("\\\n", "");
    let mut result = Vec::new();

    for raw_line in joined.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle -r / --requirement recursive includes
        if line.starts_with("-r ") || line.starts_with("--requirement ") || line.starts_with("--requirement=") {
            let ref_path = if line.starts_with("--requirement=") {
                line.strip_prefix("--requirement=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !ref_path.is_empty() {
                let full = base_dir.join(ref_path);
                result.extend(parse_requirements_file_inner(&full.to_string_lossy(), seen));
            }
            continue;
        }

        // Handle -c / --constraint (parse as pinned version upper bounds)
        if line.starts_with("-c ") || line.starts_with("--constraint ") || line.starts_with("--constraint=") {
            let ref_path = if line.starts_with("--constraint=") {
                line.strip_prefix("--constraint=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !ref_path.is_empty() {
                let full = base_dir.join(ref_path);
                // Constraints are just version-pinned requirements
                result.extend(parse_requirements_file_inner(&full.to_string_lossy(), seen));
            }
            continue;
        }

        // Handle -e / --editable installs in requirements files
        if line.starts_with("-e ") || line.starts_with("--editable ") || line.starts_with("--editable=") {
            let edit_path = if line.starts_with("--editable=") {
                line.strip_prefix("--editable=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !edit_path.is_empty() {
                let full = base_dir.join(edit_path);
                result.push(format!("editable:{}", full.to_string_lossy()));
            }
            continue;
        }

        // Handle --no-deps as a line-level flag (applies to next package)
        if line == "--no-deps" {
            // Mark next package as no-deps (handled by install pipeline)
            result.push("flag:no-deps".to_string());
            continue;
        }

        // Skip pip option flags (--index-url, --extra-index-url, --trusted-host, etc.)
        if line.starts_with("--") || line.starts_with("-f ") || line.starts_with("-i ") {
            continue;
        }

        // Strip inline comments (after ` #`)
        let spec = if let Some(comment_pos) = line.find(" #") {
            line[..comment_pos].trim()
        } else {
            line
        };

        // Extract and preserve inline --hash options for verification
        let mut hashes: Vec<String> = Vec::new();
        let spec = {
            let mut s = spec;
            while let Some(hash_pos) = s.find(" --hash=") {
                let hash_val = s[hash_pos + 8..].split_whitespace().next().unwrap_or("");
                if !hash_val.is_empty() {
                    hashes.push(hash_val.to_string());
                }
                s = s[..hash_pos].trim();
            }
            s
        };

        // Strip environment markers: handle `; marker` at end
        // Keep the full spec including markers — the resolver's parse_dependency handles them
        let spec = spec.trim();

        if spec.is_empty() {
            continue;
        }

        // If hashes were specified, encode them in the spec for downstream verification
        if !hashes.is_empty() {
            result.push(format!("hash:{}:{}", hashes.join(","), spec));
        } else {
            result.push(spec.to_string());
        }
    }

    result
}

fn install_packages(specs: &[String], site_packages: &str, upgrade: bool, no_deps: bool, _pre: bool, quiet: bool, verbose: bool) -> Result<(), String> {
    let start_time = std::time::Instant::now();
    let mut visited = std::collections::HashSet::new();
    let total = specs.len();
    let mut installed_count = 0;
    let mut next_no_deps = false;

    for (idx, spec) in specs.iter().enumerate() {
        let trimmed = spec.trim();

        // Handle flag:no-deps from requirements files
        if trimmed == "flag:no-deps" {
            next_no_deps = true;
            continue;
        }

        let effective_no_deps = no_deps || next_no_deps;
        next_no_deps = false; // reset for next package

        // Handle hash-verified entries from requirements files: hash:<hashes>:<spec>
        let (trimmed, expected_hashes) = if let Some(rest) = trimmed.strip_prefix("hash:") {
            if let Some(colon) = rest.find(':') {
                let hashes_str = &rest[..colon];
                let actual_spec = &rest[colon + 1..];
                let hashes: Vec<String> = hashes_str.split(',')
                    .map(|h| h.trim().to_string())
                    .filter(|h| !h.is_empty())
                    .collect();
                (actual_spec, hashes)
            } else {
                (trimmed, vec![])
            }
        } else {
            (trimmed, vec![])
        };

        // Handle editable entries from requirements files (editable:<path>)
        if let Some(edit_path) = trimmed.strip_prefix("editable:") {
            if !quiet {
                println!("[{}/{}] Installing {} (editable)", idx + 1, total, edit_path);
            }
            install_editable(edit_path, site_packages, quiet)?;
            installed_count += 1;
            continue;
        }

        // Handle `ferryip install .` or `ferryip install .[dev]` or `ferryip install ./path`
        if trimmed == "." || trimmed.starts_with(".[")
            || trimmed.starts_with("./") || trimmed.starts_with("../")
            || std::path::Path::new(trimmed).join("pyproject.toml").exists()
            || std::path::Path::new(trimmed).join("setup.cfg").exists()
            || std::path::Path::new(trimmed).join("setup.py").exists()
        {
            // Extract extras from ".[dev,test]" syntax
            let (proj_path, proj_extras) = if let Some(bracket_start) = trimmed.find('[') {
                if let Some(bracket_end) = trimmed.find(']') {
                    let extras_str = &trimmed[bracket_start + 1..bracket_end];
                    let extras: Vec<String> = extras_str.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let path = trimmed[..bracket_start].trim();
                    let path = if path.is_empty() { "." } else { path };
                    (path.to_string(), extras)
                } else {
                    (trimmed.to_string(), vec![])
                }
            } else {
                (trimmed.to_string(), vec![])
            };

            install_project_with_extras(&proj_path, &proj_extras, site_packages, quiet)?;
            installed_count += 1;
            continue;
        }

        // Handle local wheel/sdist file paths
        if trimmed.ends_with(".whl") || trimmed.ends_with(".tar.gz") {
            // Verify hash if provided by requirements file
            if !expected_hashes.is_empty() {
                verify_file_hashes(trimmed, &expected_hashes)?;
            }
            install_local_archive(trimmed, site_packages, quiet)?;
            installed_count += 1;
            continue;
        }

        let (name, version_spec, extras) = parse_version_specifier_with_extras(&trimmed.to_string());
        if !quiet && total > 1 {
            let ver_display = version_spec.as_deref().unwrap_or("");
            println!("[{}/{}] Processing {}{}", idx + 1, total, name, ver_display);
        }
        if verbose {
            let extras_display = if extras.is_empty() {
                String::new()
            } else {
                format!("[{}]", extras.join(","))
            };
            println!("  Resolving {}{}{}", name, extras_display,
                     version_spec.as_deref().map(|v| format!(" ({})", v)).unwrap_or_default());
        }
        resolver::install_with_deps(
            &name,
            version_spec.as_deref(),
            site_packages,
            upgrade,
            effective_no_deps,
            quiet,
            &mut visited,
        )?;
        installed_count += 1;

        // Install extras if requested (e.g., package[security,socks])
        if !extras.is_empty() && !effective_no_deps {
            install_extras(&name, &extras, site_packages, quiet, &mut visited)?;
        }
    }

    if !quiet && installed_count > 1 {
        let elapsed = start_time.elapsed();
        println!("\nSuccessfully processed {} package(s) in {:.1}s.", installed_count, elapsed.as_secs_f64());
    }
    Ok(())
}

/// Verify file hashes match expected values (from --hash= in requirements files).
fn verify_file_hashes(path: &str, expected_hashes: &[String]) -> Result<(), String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Cannot read '{}' for hash verification: {}", path, e))?;

    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let actual = format!("{:x}", hasher.finalize());

    for expected in expected_hashes {
        // Support sha256:HASH format
        let hash_val = expected.strip_prefix("sha256:").unwrap_or(expected);
        if actual == hash_val {
            return Ok(());
        }
    }

    Err(format!(
        "Hash verification failed for {}:\n  Expected one of: {}\n  Got: sha256:{}",
        path,
        expected_hashes.join(", "),
        actual,
    ))
}

/// Post-install verification: check RECORD hashes for recently installed packages.
fn verify_all_installed(site_packages: &str, specs: &[String], quiet: bool) {
    for spec in specs {
        let trimmed = spec.trim();
        // Skip flags and special entries
        if trimmed.starts_with("flag:") || trimmed.starts_with("editable:")
            || trimmed.starts_with("hash:") || trimmed == "." || trimmed.starts_with("./")
        {
            continue;
        }
        let (name, _, _) = parse_version_specifier_with_extras(&trimmed.to_string());
        let failures = crate::installer::verify_installed_record(site_packages, &name);
        if failures.is_empty() {
            if !quiet {
                println!("  ✓ {} RECORD verified", name);
            }
        } else {
            eprintln!("  ✗ {} has {} file(s) with mismatched hashes", name, failures.len());
            for f in failures.iter().take(3) {
                eprintln!("      {}", f);
            }
        }
    }
}

/// Install a local .whl or .tar.gz file directly.
fn install_local_archive(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
    let file_path = std::path::Path::new(path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", path));
    }

    let filename = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // For .whl files, prefer reading metadata from inside the wheel
    let (name, version) = if filename.ends_with(".whl") {
        // Check platform compatibility first
        if let Err(e) = crate::installer::check_wheel_compatibility(file_path) {
            return Err(format!("Incompatible wheel: {}", e));
        }
        // Try to read metadata from inside the wheel
        match crate::installer::read_wheel_metadata(file_path) {
            Ok(meta) if !meta.name.is_empty() && !meta.version.is_empty() => {
                (meta.name, meta.version)
            }
            _ => {
                // Fallback: parse from filename
                let stem = filename.strip_suffix(".whl").unwrap_or(filename);
                let parts: Vec<&str> = stem.splitn(3, '-').collect();
                if parts.len() >= 2 {
                    (parts[0].to_string(), parts[1].to_string())
                } else {
                    ("unknown".to_string(), "0.0.0".to_string())
                }
            }
        }
    } else {
        // sdist: {name}-{version}.tar.gz
        let stem = filename.strip_suffix(".tar.gz").unwrap_or(filename);
        let parts: Vec<&str> = stem.rsplitn(2, '-').collect();
        if parts.len() >= 2 {
            (parts[1].to_string(), parts[0].to_string())
        } else {
            ("unknown".to_string(), "0.0.0".to_string())
        }
    };

    if !quiet {
        println!("Installing {} ({}) from local file", name, version);
    }

    crate::installer::install_wheel(file_path, site_packages, &name, &version)?;

    // Verify RECORD hashes after install
    let failures = crate::installer::verify_installed_record(site_packages, &name);
    if !failures.is_empty() {
        eprintln!("WARNING: {} file(s) failed RECORD hash verification:", failures.len());
        for f in failures.iter().take(5) {
            eprintln!("  {}", f);
        }
        if failures.len() > 5 {
            eprintln!("  ... and {} more", failures.len() - 5);
        }
    }

    if !quiet {
        println!("  Successfully installed {}-{}", name, version);
    }
    Ok(())
}

/// Install optional dependency groups (extras) for a package.
fn install_extras(
    pkg_name: &str,
    extras: &[String],
    site_packages: &str,
    quiet: bool,
    visited: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    // Read the installed package's METADATA to find extras dependencies
    if let Some(info) = registry::get_installed(pkg_name, site_packages) {
        if let Some(ref requires) = info.requires {
            for req in requires {
                // Match requirements with extras markers like:
                // PySocks>=1.5.6 ; extra == 'socks'
                if let Some(semicolon) = req.find(';') {
                    let marker = req[semicolon + 1..].trim();
                    for extra in extras {
                        if marker.contains("extra") && marker.contains(extra) {
                            let dep_spec = req[..semicolon].trim();
                            let (dep_name, dep_ver) = parse_version_specifier(dep_spec);
                            resolver::install_with_deps(
                                &dep_name,
                                dep_ver.as_deref(),
                                site_packages,
                                false, false, quiet,
                                visited,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Parse a full version specifier preserving the operator (>=, <=, ~=, !=, ==, etc.)
fn parse_version_specifier(spec: &str) -> (String, Option<String>) {
    let (name, ver, _) = parse_version_specifier_with_extras(spec);
    (name, ver)
}

/// Parse a version specifier extracting name, version spec, and extras.
/// Examples:
///   "requests>=2.28" -> ("requests", Some(">=2.28"), [])
///   "package[security,socks]>=1.0" -> ("package", Some(">=1.0"), ["security", "socks"])
///   "flask" -> ("flask", None, [])
fn parse_version_specifier_with_extras(spec: &str) -> (String, Option<String>, Vec<String>) {
    let spec = spec.trim();

    // Strip environment markers after `;`
    let spec = if let Some(semi) = spec.find(';') {
        spec[..semi].trim()
    } else {
        spec
    };

    // Extract extras from brackets
    let (clean, extras) = if let Some(bracket_start) = spec.find('[') {
        if let Some(bracket_end) = spec.find(']') {
            let extras_str = &spec[bracket_start + 1..bracket_end];
            let extras: Vec<String> = extras_str.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let clean = format!("{}{}", &spec[..bracket_start], &spec[bracket_end + 1..]);
            (clean, extras)
        } else {
            (spec.to_string(), vec![])
        }
    } else {
        (spec.to_string(), vec![])
    };

    for op in &["~=", ">=", "<=", "!=", "==", ">", "<"] {
        if let Some(pos) = clean.find(op) {
            let name = clean[..pos].trim().to_lowercase();
            let version_part = clean[pos..].trim().to_string();
            return (name, Some(version_part), extras);
        }
    }
    (clean.trim().to_lowercase(), None, extras)
}

/// Dry-run mode: show what would be installed without actually installing.
fn dry_run_install(packages: &[String], requirement_files: &[String], quiet: bool) -> Result<(), String> {
    let mut specs: Vec<String> = packages.to_vec();
    for req_file in requirement_files {
        specs.extend(parse_requirements_file(req_file));
    }

    if specs.is_empty() {
        println!("No packages to install.");
        return Ok(());
    }

    println!("Would install:");
    for spec in &specs {
        let (name, version_spec, extras) = parse_version_specifier_with_extras(spec);
        match resolver::resolve_package_info(&name, version_spec.as_deref(), "") {
            Ok((info, transitive_deps)) => {
                let extras_str = if extras.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", extras.join(","))
                };
                let ver_str = version_spec.as_deref().unwrap_or("");
                println!("  {}{} {} (latest: {})", name, extras_str, ver_str, info.version);

                // Show transitive dependencies
                if !transitive_deps.is_empty() {
                    for (dep_name, dep_ver) in &transitive_deps {
                        let dep_ver_str = dep_ver.as_deref().unwrap_or("");
                        match pypi::fetch_package_info(&dep_name, None) {
                            Ok(dep_info) => {
                                println!("    └─ {} {} (latest: {})", dep_name, dep_ver_str, dep_info.version);
                            }
                            Err(_) => {
                                println!("    └─ {} {}", dep_name, dep_ver_str);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if !quiet {
                    println!("  {} — could not resolve: {}", name, e);
                }
            }
        }
    }
    Ok(())
}

/// Install a package in editable mode: writes a .pth file pointing at the source directory.
fn install_editable(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path).canonicalize()
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
    let pyproject_path = proj_dir.join("pyproject.toml");
    let setup_cfg_path = proj_dir.join("setup.cfg");

    let (name, version, pkg_meta) = if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let meta = PackageMetadata::from_pyproject(&pyproj);
        let name = pyproj.name().unwrap_or_else(|| {
            proj_dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let version = pyproj.version().unwrap_or("0.0.0").to_string();
        (name, version, Some(meta))
    } else if setup_cfg_path.exists() {
        let cfg = crate::setup_cfg::parse_setup_cfg(&setup_cfg_path)?;
        let meta = PackageMetadata::from_setup_cfg(&cfg);
        let name = cfg.name.unwrap_or_else(|| {
            proj_dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let version = cfg.version.unwrap_or_else(|| "0.0.0".into());
        (name, version, Some(meta))
    } else {
        let name = proj_dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        (name, "0.0.0".to_string(), None)
    };

    crate::installer::install_editable_with_metadata(
        &proj_dir, site_packages, &name, &version, pkg_meta.as_ref(),
    )?;

    if !quiet {
        let source_root = if proj_dir.join("src").exists() {
            proj_dir.join("src")
        } else {
            proj_dir.clone()
        };
        println!("Successfully installed {} (editable, {})", name, source_root.display());
    }

    // Also install project dependencies
    if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let deps = pyproj.dependencies();
        if !deps.is_empty() {
            if !quiet {
                println!("Installing {} project dependencies...", deps.len());
            }
            install_packages(&deps, site_packages, false, false, false, quiet, false)?;
        }

        // Install build-system requirements too
        let build_reqs = pyproj.build_requires();
        if !build_reqs.is_empty() {
            if !quiet {
                println!("Installing {} build dependencies...", build_reqs.len());
            }
            install_packages(&build_reqs, site_packages, false, false, false, quiet, false)?;
        }
    } else if setup_cfg_path.exists() {
        let cfg = crate::setup_cfg::parse_setup_cfg(&setup_cfg_path)?;
        if !cfg.install_requires.is_empty() {
            if !quiet {
                println!("Installing {} project dependencies...", cfg.install_requires.len());
            }
            install_packages(&cfg.install_requires, site_packages, false, false, false, quiet, false)?;
        }
    }

    Ok(())
}

fn uninstall_packages(names: &[String], site_packages: &str, yes: bool, quiet: bool) -> Result<(), String> {
    if names.is_empty() {
        return Err("You must give at least one package to uninstall (see 'ferryip uninstall --help')".to_string());
    }
    for name in names {
        let installed = registry::get_installed(name, site_packages);
        if installed.is_none() {
            // Try to suggest similar installed packages
            let all = registry::list_installed(site_packages);
            let suggestion = find_closest_name(name, &all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>());
            let hint = if let Some(ref similar) = suggestion {
                format!("\nDid you mean: {}?", similar)
            } else {
                String::new()
            };
            if !quiet {
                println!("WARNING: Skipping {} as it is not installed.{}", name, hint);
            }
            continue;
        }
        let info = installed.unwrap();
        if !yes {
            println!("Found existing installation: {}-{}", info.name, info.version);
            let file_count = info.files.len();
            println!("  Would remove {} file(s):", file_count);
            // Show up to 10 files, then summarize
            for (i, f) in info.files.iter().enumerate() {
                if i >= 10 {
                    println!("    ... and {} more", file_count - 10);
                    break;
                }
                println!("    {}", f);
            }
            print!("Proceed (Y/n)? ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "n" {
                println!("Skipping {}.", name);
                continue;
            }
        }
        registry::uninstall(name, site_packages)
            .map_err(|e| format!("Uninstall failed: {}", e))?;
        if !quiet {
            println!("Successfully uninstalled {}-{}", info.name, info.version);
        }
    }
    Ok(())
}

fn list_packages(site_packages: &str, outdated: bool, format: &str, not_required: bool, exclude_editable: bool) -> Result<(), String> {
    let mut packages = registry::list_installed(site_packages);
    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    // Filter editable packages if requested
    if exclude_editable {
        let site = std::path::Path::new(site_packages);
        packages.retain(|pkg| {
            let normalized = pkg.name.to_lowercase().replace('-', "_").replace('.', "_");
            !site.join(format!("__{}.pth", normalized)).exists()
        });
    }

    // Filter to packages not required by others
    if not_required {
        let all_deps: std::collections::HashSet<String> = packages.iter()
            .filter_map(|p| p.requires.as_ref())
            .flat_map(|reqs| reqs.iter())
            .map(|r| {
                let name = r.split_whitespace().next().unwrap_or(r);
                let name = name.split(&['>', '<', '=', '!', '~', ';', '(', '['][..]).next()
                    .unwrap_or(name);
                name.to_lowercase().replace('-', "_").replace('.', "_")
            })
            .collect();
        packages.retain(|p| {
            let normalized = p.name.to_lowercase().replace('-', "_").replace('.', "_");
            !all_deps.contains(&normalized)
        });
    }

    match format {
        "freeze" => {
            for pkg in &packages {
                println!("{}=={}", pkg.name, pkg.version);
            }
        }
        "json" => {
            println!("[");
            for (i, pkg) in packages.iter().enumerate() {
                let comma = if i + 1 < packages.len() { "," } else { "" };
                println!("  {{\"name\": \"{}\", \"version\": \"{}\"}}{}", pkg.name, pkg.version, comma);
            }
            println!("]");
        }
        _ => { // "columns" (default)
            if outdated {
                // Calculate dynamic column widths
                let name_width = packages.iter().map(|p| p.name.len()).max().unwrap_or(7).max(7);
                let ver_width = packages.iter().map(|p| p.version.len()).max().unwrap_or(7).max(7);
                println!("{:<name_w$} {:<ver_w$} {}", "Package", "Version", "Latest",
                         name_w = name_width, ver_w = ver_width);
                println!("{:<name_w$} {:<ver_w$} {}", "-".repeat(name_width), "-".repeat(ver_width), "------",
                         name_w = name_width, ver_w = ver_width);
                let mut outdated_count = 0;
                for pkg in &packages {
                    match pypi::fetch_package_info(&pkg.name, None) {
                        Ok(latest) => {
                            if latest.version != pkg.version {
                                println!("{:<name_w$} {:<ver_w$} {}", pkg.name, pkg.version, latest.version,
                                         name_w = name_width, ver_w = ver_width);
                                outdated_count += 1;
                            }
                        }
                        Err(_) => {} // skip packages that can't be checked
                    }
                }
                if outdated_count == 0 {
                    println!("All packages are up to date.");
                }
            } else {
                // Calculate dynamic column widths
                let name_width = packages.iter().map(|p| p.name.len()).max().unwrap_or(7).max(7);
                println!("{:<width$} {}", "Package", "Version", width = name_width);
                println!("{:<width$} {}", "-".repeat(name_width), "-------", width = name_width);
                for pkg in &packages {
                    println!("{:<width$} {}", pkg.name, pkg.version, width = name_width);
                }
                println!("\n[{} package(s) installed]", packages.len());
            }
        }
    }
    Ok(())
}

fn show_package(name: &str, site_packages: &str, show_files: bool) -> Result<(), String> {
    // Try installed first
    if let Some(info) = registry::get_installed(name, site_packages) {
        println!("Name: {}", info.name);
        println!("Version: {}", info.version);
        if let Some(ref summary) = info.summary {
            println!("Summary: {}", summary);
        }
        if let Some(ref home_page) = info.home_page {
            println!("Home-page: {}", home_page);
        }
        if let Some(ref author) = info.author {
            println!("Author: {}", author);
        }
        if let Some(ref license) = info.license {
            println!("License: {}", license);
        }
        if let Some(ref requires_python) = info.requires_python {
            println!("Requires-Python: {}", requires_python);
        }
        println!("Location: {}", site_packages);
        if let Some(ref requires) = info.requires {
            // Strip markers for display; show clean dependency names
            let dep_names: Vec<String> = requires.iter()
                .map(|r| {
                    let clean = if let Some(semi) = r.find(';') { &r[..semi] } else { r };
                    clean.trim().to_string()
                })
                .collect();
            println!("Requires: {}", dep_names.join(", "));
        } else {
            println!("Requires: (none)");
        }

        // Compute "Required-by": which installed packages depend on this one
        let normalized_name = info.name.to_lowercase().replace('-', "_").replace('.', "_");
        let all_installed = registry::list_installed(site_packages);
        let required_by: Vec<String> = all_installed.iter()
            .filter(|p| {
                p.requires.as_ref().map_or(false, |reqs| {
                    reqs.iter().any(|r| {
                        let dep = r.split_whitespace().next().unwrap_or(r);
                        let dep = dep.split(&['>', '<', '=', '!', '~', ';', '(', '['][..])
                            .next().unwrap_or(dep);
                        dep.to_lowercase().replace('-', "_").replace('.', "_") == normalized_name
                    })
                })
            })
            .map(|p| p.name.clone())
            .collect();
        if required_by.is_empty() {
            println!("Required-by: (none)");
        } else {
            println!("Required-by: {}", required_by.join(", "));
        }

        println!("Installer: ferryip");

        if show_files {
            println!("Files:");
            for f in &info.files {
                println!("  {}", f);
            }
            println!("  ({} file(s))", info.files.len());
        }
        return Ok(());
    }

    // Not installed — try to fetch from PyPI
    match pypi::fetch_package_info(name, None) {
        Ok(info) => {
            println!("Name: {} (not installed)", info.name);
            println!("Version: {} (latest)", info.version);
            if !info.summary.is_empty() {
                println!("Summary: {}", info.summary);
            }
            if !info.author.is_empty() {
                println!("Author: {}", info.author);
            }
            if !info.license.is_empty() {
                println!("License: {}", info.license);
            }
            if !info.requires_dist.is_empty() {
                println!("Requires: {}", info.requires_dist.join(", "));
            }
            println!("\nTo install: ferryip install {}", name);
            Ok(())
        }
        Err(_e) => {
            let suggestion = suggest_similar_package(name);
            let hint = if let Some(ref similar) = suggestion {
                format!("\nDid you mean: {}?\n", similar)
            } else {
                String::new()
            };
            Err(format!(
                "Package '{}' is not installed and was not found on PyPI.\n\
                 {}Hint: Check the package name spelling or search with: ferryip search {}",
                name, hint, name
            ))
        }
    }
}

fn search_pypi(query: &str) -> Result<(), String> {
    // Try exact match first
    let results = pypi::search(query).map_err(|e| format!("Search failed: {}", e))?;
    if !results.is_empty() {
        for (name, version, summary) in &results {
            println!("{} ({}) - {}", name, version, summary);
        }
        return Ok(());
    }

    // Try common name variations (replace spaces with hyphens, underscores)
    let variations = vec![
        query.replace(' ', "-"),
        query.replace(' ', "_"),
        query.replace('_', "-"),
        query.replace('-', "_"),
        format!("python-{}", query),
        format!("py{}", query),
    ];

    let mut found = false;
    let mut seen = std::collections::HashSet::new();
    seen.insert(query.to_lowercase());

    for variant in &variations {
        let normalized = variant.to_lowercase();
        if !seen.insert(normalized) {
            continue;
        }
        if let Ok(results) = pypi::search(variant) {
            for (name, version, summary) in &results {
                println!("{} ({}) - {}", name, version, summary);
                found = true;
            }
        }
    }

    if !found {
        println!("No packages found matching '{}'.", query);
        println!("Hint: Try browsing https://pypi.org/search/?q={}", query);
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

fn freeze_packages(site_packages: &str, exclude_editable: bool) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    let site = std::path::Path::new(site_packages);
    for pkg in &packages {
        let normalized = pkg.name.to_lowercase().replace('-', "_").replace('.', "_");
        let is_editable = site.join(format!("__{}.pth", normalized)).exists();
        if exclude_editable && is_editable {
            continue;
        }
        if is_editable {
            // Show editable installs in -e format like pip does
            let pth_path = site.join(format!("__{}.pth", normalized));
            if let Ok(content) = std::fs::read_to_string(&pth_path) {
                let source = content.trim();
                println!("-e {}", source);
            } else {
                println!("# Editable install: {}=={}", pkg.name, pkg.version);
            }
        } else {
            println!("{}=={}", pkg.name, pkg.version);
        }
    }
    Ok(())
}

fn check_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    let mut has_errors = false;
    let mut checked = 0;

    for pkg in &packages {
        // Verify RECORD hash integrity
        let hash_failures = crate::installer::verify_installed_record(site_packages, &pkg.name);
        if !hash_failures.is_empty() {
            println!("{} {} has {} file(s) with mismatched RECORD hashes:",
                     pkg.name, pkg.version, hash_failures.len());
            for f in hash_failures.iter().take(3) {
                println!("    {}", f);
            }
            if hash_failures.len() > 3 {
                println!("    ... and {} more", hash_failures.len() - 3);
            }
            has_errors = true;
        }

        if let Some(ref requires) = pkg.requires {
            for req in requires {
                // Strip environment markers for the check
                let req_clean = if let Some(semi) = req.find(';') {
                    req[..semi].trim()
                } else {
                    req.trim()
                };

                let (req_name, req_spec) = parse_version_specifier(req_clean);
                match registry::get_installed(&req_name, site_packages) {
                    None => {
                        println!("{} {} requires {}, which is not installed.",
                                 pkg.name, pkg.version, req);
                        has_errors = true;
                    }
                    Some(installed) => {
                        if let Some(ref spec) = req_spec {
                            if !crate::version::version_matches(&installed.version, spec) {
                                println!(
                                    "{} {} requires {} {}, but {} {} is installed.",
                                    pkg.name, pkg.version, req_name, spec,
                                    installed.name, installed.version
                                );
                                has_errors = true;
                            }
                        }
                    }
                }
                checked += 1;
            }
        }
    }

    if !has_errors {
        println!("No broken requirements found ({} packages checked, {} dependencies verified).",
                 packages.len(), checked);
    }
    Ok(())
}

/// Install dependencies from a project's pyproject.toml or setup.cfg, including optional extras.
fn install_project_with_extras(path: &str, requested_extras: &[String], site_packages: &str, quiet: bool) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path);
    let pyproject_path = proj_dir.join("pyproject.toml");

    if pyproject_path.exists() && !requested_extras.is_empty() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        if !quiet {
            if let Some(name) = pyproj.name() {
                let version = pyproj.version().unwrap_or("0.0.0");
                println!("Installing project: {} ({}) with extras: [{}]",
                         name, version, requested_extras.join(", "));
            }
        }

        // Install base project first
        install_project(path, site_packages, quiet)?;

        // Install requested extras
        let mut visited = std::collections::HashSet::new();
        for extra in requested_extras {
            let extra_deps = pyproj.extra_deps(extra);
            if extra_deps.is_empty() {
                let available = pyproj.extras();
                if available.is_empty() {
                    eprintln!("WARNING: No optional dependencies defined in pyproject.toml");
                } else {
                    eprintln!("WARNING: Extra '{}' not found. Available extras: {}",
                             extra, available.join(", "));
                }
                continue;
            }
            if !quiet {
                println!("Installing extra '{}' ({} dependencies)...", extra, extra_deps.len());
            }
            for dep in &extra_deps {
                let (name, spec) = parse_version_specifier(dep);
                resolver::install_with_deps(&name, spec.as_deref(), site_packages, false, false, quiet, &mut visited)?;
            }
        }
        return Ok(());
    }

    // No extras or no pyproject.toml — fall through to regular install
    install_project(path, site_packages, quiet)
}

/// Install dependencies from a project's pyproject.toml or setup.cfg.
fn install_project(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path);

    // Try pyproject.toml first
    let pyproject_path = proj_dir.join("pyproject.toml");
    if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        if !quiet {
            if let Some(name) = pyproj.name() {
                let version = pyproj.version().unwrap_or("0.0.0");
                println!("Installing project: {} ({})", name, version);
                if let Some(desc) = pyproj.description() {
                    println!("  {}", desc);
                }
            }
        }

        // Install build-system requirements
        let build_reqs = pyproj.build_requires();
        if !build_reqs.is_empty() {
            if !quiet {
                println!("Installing {} build dependencies...", build_reqs.len());
            }
        }
        let mut visited = std::collections::HashSet::new();
        for req in &build_reqs {
            let (name, spec) = parse_version_specifier(req);
            resolver::install_with_deps(&name, spec.as_deref(), site_packages, false, false, quiet, &mut visited)?;
        }

        // Install project dependencies
        let deps = pyproj.dependencies();
        if !deps.is_empty() {
            if !quiet {
                println!("Installing {} project dependencies...", deps.len());
            }
            for dep in &deps {
                let (name, spec) = parse_version_specifier(dep);
                resolver::install_with_deps(&name, spec.as_deref(), site_packages, false, false, quiet, &mut visited)?;
            }
        }

        // Install optional-dependencies if any extras are requested via [tool.setuptools] or similar
        let extras = pyproj.extras();
        if !extras.is_empty() && !quiet {
            println!("  Available extras: {}", extras.join(", "));
        }

        // Check for [tool.setuptools] packages configuration
        if let Some(ref tool) = pyproj.tool {
            if let Some(setuptools) = tool.get("setuptools") {
                if !quiet {
                    if let Some(packages) = setuptools.get("packages") {
                        if let Some(pkgs) = packages.as_array() {
                            let pkg_names: Vec<&str> = pkgs.iter()
                                .filter_map(|v| v.as_str())
                                .collect();
                            if !pkg_names.is_empty() {
                                println!("  Setuptools packages: {}", pkg_names.join(", "));
                            }
                        }
                    }
                    if let Some(pkg_dir) = setuptools.get("package-dir") {
                        if let Some(table) = pkg_dir.as_table() {
                            for (key, val) in table {
                                if let Some(dir) = val.as_str() {
                                    let label = if key.is_empty() { "(root)" } else { key.as_str() };
                                    println!("  Package dir: {} → {}", label, dir);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check python_requires compatibility
        if let Some(requires_python) = pyproj.requires_python() {
            if !crate::version::version_matches("3.12", requires_python) {
                return Err(format!(
                    "This project requires Python {} but Ferrython provides 3.12",
                    requires_python
                ));
            }
        }

        if !quiet {
            println!("Project dependencies installed successfully.");
        }
        return Ok(());
    }

    // Fallback: try setup.cfg
    let setup_cfg_path = proj_dir.join("setup.cfg");
    if setup_cfg_path.exists() {
        return install_from_setup_cfg(&setup_cfg_path, site_packages, quiet);
    }

    // Fallback: try setup.py
    let setup_py_path = proj_dir.join("setup.py");
    if setup_py_path.exists() {
        return install_from_setup_py(&setup_py_path, site_packages, quiet);
    }

    // Fallback: try requirements.txt
    let req_path = proj_dir.join("requirements.txt");
    if req_path.exists() {
        let reqs = parse_requirements_file(&req_path.to_string_lossy());
        return install_packages(&reqs, site_packages, false, false, false, quiet, false);
    }

    Err(format!(
        "No pyproject.toml, setup.cfg, setup.py, or requirements.txt found in {}",
        proj_dir.display()
    ))
}

/// Install dependencies from a setup.cfg file using the structured parser.
fn install_from_setup_cfg(path: &std::path::Path, site_packages: &str, quiet: bool) -> Result<(), String> {
    let cfg = crate::setup_cfg::parse_setup_cfg(path)?;

    if !quiet {
        if let Some(ref name) = cfg.name {
            let version = cfg.version.as_deref().unwrap_or("0.0.0");
            println!("Installing project: {} ({})", name, version);
            if let Some(ref desc) = cfg.description {
                println!("  {}", desc);
            }
        }
    }

    // Check python_requires compatibility
    if let Some(ref requires_python) = cfg.python_requires {
        if !crate::version::version_matches("3.12", requires_python) {
            return Err(format!(
                "This project requires Python {} but Ferrython provides 3.12",
                requires_python
            ));
        }
    }

    if cfg.install_requires.is_empty() {
        if !quiet {
            println!("No dependencies found in setup.cfg");
        }
        return Ok(());
    }

    if !quiet {
        println!("Installing {} dependencies from setup.cfg...", cfg.install_requires.len());
        if !cfg.extras_require.is_empty() {
            let extras: Vec<&String> = cfg.extras_require.keys().collect();
            println!("  Available extras: {}", extras.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(", "));
        }
    }

    install_packages(&cfg.install_requires, site_packages, false, false, false, quiet, false)
}

/// Extract dependencies from a setup.py file using regex-based heuristic parsing.
///
/// This avoids executing the setup.py (which could have side effects) and instead
/// looks for `install_requires=[...]` patterns in the source code.
fn install_from_setup_py(path: &std::path::Path, site_packages: &str, quiet: bool) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

    let deps = extract_setup_py_deps(&content);

    if deps.is_empty() {
        if !quiet {
            println!("No dependencies found in setup.py");
        }
        return Ok(());
    }

    if !quiet {
        println!("Found {} dependencies in setup.py", deps.len());
    }

    install_packages(&deps, site_packages, false, false, false, quiet, false)
}

/// Heuristic parser for install_requires in setup.py.
/// Handles common patterns:
///   install_requires=['dep1', 'dep2>=1.0']
///   install_requires=[
///       'dep1',
///       'dep2>=1.0',
///   ]
///   INSTALL_REQUIRES = ['dep1']
///   setup(..., install_requires=INSTALL_REQUIRES, ...)
fn extract_setup_py_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();

    // Strategy 1: Find install_requires=[...] directly
    if let Some(start) = content.find("install_requires") {
        let after = &content[start..];
        if let Some(eq) = after.find('=') {
            let after_eq = after[eq + 1..].trim_start();
            if after_eq.starts_with('[') {
                deps.extend(extract_string_list(after_eq));
            } else {
                // Might be a variable reference; look for the variable definition
                let var_name = after_eq.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !var_name.is_empty() {
                    // Search for VAR_NAME = [...]
                    let pattern = format!("{} =", var_name);
                    if let Some(var_pos) = content.find(&pattern) {
                        let var_after = &content[var_pos + pattern.len()..];
                        let trimmed = var_after.trim_start();
                        if trimmed.starts_with('[') {
                            deps.extend(extract_string_list(trimmed));
                        }
                    }
                    // Also try without space: VAR_NAME=[...]
                    let pattern2 = format!("{}=", var_name);
                    if deps.is_empty() {
                        if let Some(var_pos) = content.find(&pattern2) {
                            let var_after = &content[var_pos + pattern2.len()..];
                            let trimmed = var_after.trim_start();
                            if trimmed.starts_with('[') {
                                deps.extend(extract_string_list(trimmed));
                            }
                        }
                    }
                }
            }
        }
    }

    deps
}

/// Extract strings from a Python list literal: ['foo', "bar>=1.0", ...]
fn extract_string_list(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = '"';
    let mut current = String::new();
    let mut started = false;

    for ch in s.chars() {
        if !started {
            if ch == '[' {
                started = true;
                depth = 1;
            }
            continue;
        }

        if in_string {
            if ch == string_char {
                in_string = false;
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    result.push(trimmed);
                }
                current.clear();
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth <= 0 {
                    break;
                }
            }
            '\'' | '"' => {
                in_string = true;
                string_char = ch;
                current.clear();
            }
            _ => {}
        }
    }

    result
}

// ── Cache management ─────────────────────────────────────────────────────────

fn cache_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home).join(".cache")
        });
    base.join("ferryip").join("wheels")
}

fn handle_cache(action: CacheAction, _quiet: bool) -> Result<(), String> {
    let dir = cache_dir();
    match action {
        CacheAction::Dir => {
            println!("Package cache directory: {}", dir.display());
            let size = dir_size(&dir);
            println!("Cache size: {}", format_size(size));
        }
        CacheAction::Info => {
            println!("Package cache location: {}", dir.display());
            let count = count_cached(&dir);
            let size = dir_size(&dir);
            println!("Number of cached wheels: {}", count);
            println!("Cache size: {}", format_size(size));
        }
        CacheAction::List => {
            if !dir.exists() {
                println!("Cache is empty.");
                return Ok(());
            }
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| format!("Cannot read cache: {}", e))?;
            let mut found = false;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".whl") || name.ends_with(".tar.gz") {
                    let meta = entry.metadata().ok();
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    println!("  {} ({})", name, format_size(size));
                    found = true;
                }
            }
            if !found {
                println!("Cache is empty.");
            }
        }
        CacheAction::Purge => {
            if dir.exists() {
                let count = count_cached(&dir);
                std::fs::remove_dir_all(&dir)
                    .map_err(|e| format!("Purge failed: {}", e))?;
                println!("Removed {} cached files.", count);
            } else {
                println!("Cache is already empty.");
            }
        }
        CacheAction::Remove { pattern } => {
            if !dir.exists() {
                println!("Cache is empty.");
                return Ok(());
            }
            let pattern_lower = pattern.to_lowercase();
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| format!("Cannot read cache: {}", e))?;
            let mut removed = 0;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.to_lowercase().contains(&pattern_lower) {
                    let _ = std::fs::remove_file(entry.path());
                    removed += 1;
                }
            }
            println!("Removed {} cached file(s) matching '{}'.", removed, pattern);
        }
    }
    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() { return 0; }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn count_cached(path: &std::path::Path) -> usize {
    if !path.exists() { return 0; }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ── Hash computation ─────────────────────────────────────────────────────────

fn compute_hashes(files: &[String], algorithm: &str) -> Result<(), String> {
    use sha2::{Sha256, Digest};

    if files.is_empty() {
        return Err("No files specified".to_string());
    }

    for file_path in files {
        let data = std::fs::read(file_path)
            .map_err(|e| format!("Cannot read '{}': {}", file_path, e))?;

        let hash = match algorithm {
            "sha256" => {
                let mut hasher = Sha256::new();
                hasher.update(&data);
                format!("{:x}", hasher.finalize())
            }
            other => return Err(format!("Unsupported algorithm '{}' (use sha256)", other)),
        };

        println!("{}:", file_path);
        println!("--hash={}:{}", algorithm, hash);
    }
    Ok(())
}

// ── Wheel building ───────────────────────────────────────────────────────────

fn build_wheel(src: &str, wheel_dir: &str, quiet: bool) -> Result<(), String> {
    let src_path = std::path::Path::new(src);
    let pyproject_path = src_path.join("pyproject.toml");

    if !pyproject_path.exists() {
        return Err(format!("No pyproject.toml found in {}", src_path.display()));
    }

    let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
    let name = pyproj.name()
        .ok_or("No project name in pyproject.toml")?
        .to_string();
    let version = pyproj.version()
        .unwrap_or("0.0.0")
        .to_string();

    let normalized_name = name.replace('-', "_").replace('.', "_");
    let wheel_name = format!("{}-{}-py3-none-any.whl", normalized_name, version);

    let out_dir = std::path::Path::new(wheel_dir);
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("Cannot create output dir: {}", e))?;

    let wheel_path = out_dir.join(&wheel_name);

    // Build the wheel (zip archive)
    let file = std::fs::File::create(&wheel_path)
        .map_err(|e| format!("Cannot create wheel: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Collect Python source files from the package directory
    let pkg_dir = src_path.join(&normalized_name);
    let alt_pkg_dir = src_path.join("src").join(&normalized_name);
    let source_dir = if pkg_dir.exists() {
        Some(pkg_dir)
    } else if alt_pkg_dir.exists() {
        Some(alt_pkg_dir)
    } else {
        None
    };

    let mut file_count = 0;
    if let Some(ref pkg) = source_dir {
        add_dir_to_zip(&mut zip, pkg, &normalized_name, &options, &mut file_count)
            .map_err(|e| format!("Failed adding sources: {}", e))?;
    } else {
        // Single-file module: look for <name>.py in src dir
        let single = src_path.join(format!("{}.py", normalized_name));
        if single.exists() {
            let content = std::fs::read_to_string(&single)
                .map_err(|e| format!("Read error: {}", e))?;
            zip.start_file(format!("{}.py", normalized_name), options)
                .map_err(|e| format!("Zip error: {}", e))?;
            use std::io::Write;
            zip.write_all(content.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;
            file_count += 1;
        } else {
            return Err(format!("No package directory '{}' or '{}.py' found", normalized_name, normalized_name));
        }
    }

    // Add dist-info
    let dist_info_prefix = format!("{}-{}.dist-info", normalized_name, version);

    // METADATA — use rich metadata from pyproject.toml
    let pkg_meta = PackageMetadata::from_pyproject(&pyproj);
    let metadata = pkg_meta.render();

    zip.start_file(format!("{}/METADATA", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;
    {
        use std::io::Write;
        zip.write_all(metadata.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    // WHEEL
    let wheel_metadata = format!(
        "Wheel-Version: 1.0\nGenerator: ferryip\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    );
    zip.start_file(format!("{}/WHEEL", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;
    {
        use std::io::Write;
        zip.write_all(wheel_metadata.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    // RECORD (empty, will be filled by installer)
    zip.start_file(format!("{}/RECORD", dist_info_prefix), options)
        .map_err(|e| format!("Zip error: {}", e))?;

    // Add entry points if defined
    if let Some(scripts) = pyproj.scripts() {
        let mut entry_points = String::from("[console_scripts]\n");
        for (name, entry) in scripts {
            entry_points.push_str(&format!("{} = {}\n", name, entry));
        }
        zip.start_file(format!("{}/entry_points.txt", dist_info_prefix), options)
            .map_err(|e| format!("Zip error: {}", e))?;
        use std::io::Write;
        zip.write_all(entry_points.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
    }

    zip.finish().map_err(|e| format!("Zip finalize error: {}", e))?;

    if !quiet {
        println!("Built wheel: {} ({} source files)", wheel_name, file_count);
        println!("  Output: {}", wheel_path.display());
    }
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &std::path::Path,
    prefix: &str,
    options: &zip::write::SimpleFileOptions,
    count: &mut usize,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip __pycache__, .pyc, hidden files
        if name.starts_with('.') || name == "__pycache__" || name.ends_with(".pyc") {
            continue;
        }

        let zip_path = format!("{}/{}", prefix, name);

        if path.is_dir() {
            add_dir_to_zip(zip, &path, &zip_path, options, count)?;
        } else if name.ends_with(".py") || name.ends_with(".pyi") || name.ends_with(".json")
            || name.ends_with(".txt") || name.ends_with(".cfg") || name.ends_with(".toml")
        {
            let content = std::fs::read(&path)
                .map_err(|e| format!("Read {}: {}", path.display(), e))?;
            zip.start_file(&zip_path, *options)
                .map_err(|e| format!("Zip entry {}: {}", zip_path, e))?;
            use std::io::Write;
            zip.write_all(&content)
                .map_err(|e| format!("Write {}: {}", zip_path, e))?;
            *count += 1;
        }
    }
    Ok(())
}

// ── Config / Inspect ─────────────────────────────────────────────────────────

fn show_config(site_packages: &str, _list: bool) -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_default();
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    println!("ferryip version: {}", env!("CARGO_PKG_VERSION"));
    println!("Ferrython compatible: 3.8+");
    println!("Location: {}", exe.display());
    println!("Site-packages: {}", site_packages);
    println!("Cache directory: {}", cache_dir().display());
    println!("Python platform: {}", if os == "linux" { "linux" }
             else if os == "macos" { "darwin" }
             else if os == "windows" { "win32" }
             else { "unknown" });
    println!("Architecture: {}", arch);
    // Show compatible wheel tags
    let mut tags = vec!["py3-none-any".to_string()];
    match os {
        "linux" => {
            tags.push(format!("cp312-cp312-linux_{}", arch));
            tags.push(format!("cp312-abi3-manylinux_2_17_{}", arch));
            tags.push(format!("cp312-cp312-manylinux_2_17_{}", arch));
        }
        "macos" => {
            let mac_arch = if arch == "aarch64" { "arm64" } else { arch };
            tags.push(format!("cp312-cp312-macosx_11_0_{}", mac_arch));
            tags.push(format!("cp312-abi3-macosx_10_9_{}", mac_arch));
        }
        "windows" => {
            let plat = if arch == "x86_64" { "win_amd64" } else { "win32" };
            tags.push(format!("cp312-cp312-{}", plat));
        }
        _ => {}
    }
    println!("Compatible wheel tags:");
    for tag in &tags {
        println!("  {}", tag);
    }
    Ok(())
}

fn inspect_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    println!("{{");
    println!("  \"version\": \"1\",");
    println!("  \"pip_version\": \"ferryip-{}\",", env!("CARGO_PKG_VERSION"));
    println!("  \"installed\": [");
    for (i, pkg) in packages.iter().enumerate() {
        let comma = if i + 1 < packages.len() { "," } else { "" };
        println!("    {{");
        println!("      \"metadata\": {{");
        println!("        \"name\": \"{}\",", pkg.name);
        println!("        \"version\": \"{}\",", pkg.version);
        if let Some(ref summary) = pkg.summary {
            println!("        \"summary\": \"{}\",", summary.replace('"', "\\\""));
        }
        if let Some(ref requires_python) = pkg.requires_python {
            println!("        \"requires_python\": \"{}\",", requires_python);
        }
        if let Some(ref requires) = pkg.requires {
            let req_json: Vec<String> = requires.iter()
                .map(|r| format!("\"{}\"", r.replace('"', "\\\"")))
                .collect();
            println!("        \"requires_dist\": [{}],", req_json.join(", "));
        }
        // Remove trailing comma from last field by always ending with a known field
        println!("        \"installer\": \"ferryip\"");
        println!("      }}");
        println!("    }}{}", comma);
    }
    println!("  ]");
    println!("}}");
    Ok(())
}

/// Generate a lock file with pinned versions and hashes.
fn generate_lock_file(site_packages: &str, output_file: &str, requirement_file: Option<&str>) -> Result<(), String> {
    use std::io::Write;

    let mut locked_packages: Vec<(String, String, Option<String>)> = Vec::new();

    if let Some(req_file) = requirement_file {
        // Resolve from requirements file
        let content = std::fs::read_to_string(req_file)
            .map_err(|e| format!("Cannot read {}: {}", req_file, e))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let (name, _spec) = parse_version_specifier(line);
            // Fetch from PyPI to get latest compatible version
            match pypi::fetch_package_info(&name, None) {
                Ok(info) => {
                    let version = info.version.clone();
                    let hash = info.sha256.clone();
                    locked_packages.push((name.to_string(), version, hash));
                    // Also resolve dependencies
                    for dep in &info.requires_dist {
                        let dep_name = dep.split_whitespace().next().unwrap_or(dep);
                        let dep_name = dep_name.split(&['>', '<', '=', '!', '~', ';'][..]).next()
                            .unwrap_or(dep_name).trim();
                        if !dep_name.is_empty()
                            && !locked_packages.iter().any(|(n, _, _)| n == dep_name)
                        {
                            if let Ok(dep_info) = pypi::fetch_package_info(dep_name, None) {
                                locked_packages.push((
                                    dep_name.to_string(),
                                    dep_info.version,
                                    dep_info.sha256,
                                ));
                            }
                        }
                    }
                }
                Err(e) => eprintln!("Warning: could not resolve {}: {}", name, e),
            }
        }
    } else {
        // Lock from currently installed packages
        let packages = registry::list_installed(site_packages);
        for pkg in &packages {
            // Try to get hash from PyPI
            let hash = match pypi::fetch_package_info(&pkg.name, Some(&pkg.version)) {
                Ok(info) if info.version == pkg.version => info.sha256,
                _ => None,
            };
            locked_packages.push((pkg.name.clone(), pkg.version.clone(), hash));
        }
    }

    // Sort for deterministic output
    locked_packages.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    // Write lock file
    let mut file = std::fs::File::create(output_file)
        .map_err(|e| format!("Cannot create {}: {}", output_file, e))?;

    writeln!(file, "# This file is @generated by ferryip lock.").map_err(|e| e.to_string())?;
    writeln!(file, "# Do not edit manually.").map_err(|e| e.to_string())?;
    writeln!(file, "#").map_err(|e| e.to_string())?;

    for (name, version, hash) in &locked_packages {
        if let Some(h) = hash {
            writeln!(file, "{}=={} --hash=sha256:{}", name, version, h)
                .map_err(|e| e.to_string())?;
        } else {
            writeln!(file, "{}=={}", name, version)
                .map_err(|e| e.to_string())?;
        }
    }

    println!("Locked {} packages to {}", locked_packages.len(), output_file);
    Ok(())
}

// ── Similar package suggestion helpers ───────────────────────────────────────

/// Suggest a similar package name from PyPI using common variations.
fn suggest_similar_package(name: &str) -> Option<String> {
    let variations = vec![
        name.replace('_', "-"),
        name.replace('-', "_"),
        format!("python-{}", name),
        format!("py{}", name),
        format!("{}3", name),
    ];

    for variant in &variations {
        if variant == name { continue; }
        if let Ok(results) = pypi::search(variant) {
            if !results.is_empty() {
                return Some(results[0].0.clone());
            }
        }
    }
    None
}

/// Find the closest matching name from a list using edit distance.
fn find_closest_name(needle: &str, haystack: &[&str]) -> Option<String> {
    let needle_lower = needle.to_lowercase();
    let mut best: Option<(usize, String)> = None;
    for &candidate in haystack {
        let dist = edit_distance(&needle_lower, &candidate.to_lowercase());
        // Only suggest if reasonably close (max 3 edits or half the length)
        let threshold = (needle.len() / 2).max(3);
        if dist <= threshold {
            if best.is_none() || dist < best.as_ref().unwrap().0 {
                best = Some((dist, candidate.to_string()));
            }
        }
    }
    best.map(|(_, name)| name)
}

/// Simple Levenshtein edit distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let m = a_bytes.len();
    let n = b_bytes.len();
    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}
