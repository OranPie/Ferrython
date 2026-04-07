use clap::{Parser, Subcommand};
use crate::{pypi, registry, resolver};

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

        /// Don't use the wheel cache
        #[arg(long)]
        no_cache_dir: bool,
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

    let result = match cli.command {
        Commands::Install { packages, requirement, upgrade, editable, no_deps, pre, only_binary: _, install_target, no_cache_dir: _ } => {
            let effective_site = install_target.as_deref().unwrap_or(&site_packages);
            if let Some(editable_path) = editable {
                let proj_path = editable_path.unwrap_or_else(|| ".".to_string());
                install_editable(&proj_path, effective_site, quiet)
            } else if let Some(req_file) = requirement {
                let reqs = parse_requirements_file(&req_file);
                install_packages(&reqs, effective_site, upgrade, no_deps, pre, quiet)
            } else if packages.is_empty() {
                eprintln!("Error: no packages specified");
                std::process::exit(1);
            } else {
                install_packages(&packages, effective_site, upgrade, no_deps, pre, quiet)
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

fn install_packages(specs: &[String], site_packages: &str, upgrade: bool, no_deps: bool, _pre: bool, quiet: bool) -> Result<(), String> {
    let mut visited = std::collections::HashSet::new();
    for spec in specs {
        let (name, version_spec) = parse_version_specifier(spec);
        resolver::install_with_deps(
            &name,
            version_spec.as_deref(),
            site_packages,
            upgrade,
            no_deps,
            quiet,
            &mut visited,
        )?;
    }
    Ok(())
}

/// Parse a full version specifier preserving the operator (>=, <=, ~=, !=, ==, etc.)
fn parse_version_specifier(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();
    // Handle extras: package[extra]>=version
    let clean = if let Some(bracket) = spec.find('[') {
        if let Some(end) = spec.find(']') {
            format!("{}{}", &spec[..bracket], &spec[end+1..])
        } else {
            spec.to_string()
        }
    } else {
        spec.to_string()
    };

    for op in &["~=", ">=", "<=", "!=", "==", ">", "<"] {
        if let Some(pos) = clean.find(op) {
            let name = clean[..pos].trim().to_lowercase();
            let version_part = clean[pos..].trim().to_string();
            return (name, Some(version_part));
        }
    }
    (clean.trim().to_lowercase(), None)
}

/// Install a package in editable mode: writes a .pth file pointing at the source directory.
fn install_editable(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path).canonicalize()
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
    let pyproject_path = proj_dir.join("pyproject.toml");

    let (name, version) = if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let name = pyproj.name().unwrap_or_else(|| {
            proj_dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let version = pyproj.version().unwrap_or("0.0.0").to_string();
        (name, version)
    } else {
        let name = proj_dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        (name, "0.0.0".to_string())
    };

    let site = std::path::Path::new(site_packages);
    std::fs::create_dir_all(site)
        .map_err(|e| format!("Cannot create site-packages: {}", e))?;

    // Determine the source root: prefer src/<package> layout, then top-level
    let package_name = name.replace('-', "_");
    let source_root = if proj_dir.join("src").exists() {
        proj_dir.join("src")
    } else {
        proj_dir.clone()
    };

    // Write .pth file — each line is a path added to sys.path
    let pth_file = site.join(format!("__{}.pth", package_name));
    std::fs::write(&pth_file, format!("{}\n", source_root.display()))
        .map_err(|e| format!("Write .pth file: {}", e))?;

    // Write a minimal .dist-info for pip/ferryip compatibility
    let dist_info_name = format!("{}-{}.dist-info", package_name, version);
    let dist_info_path = site.join(&dist_info_name);
    std::fs::create_dir_all(&dist_info_path)
        .map_err(|e| format!("mkdir dist-info: {}", e))?;

    let metadata = format!(
        "Metadata-Version: 2.1\nName: {}\nVersion: {}\nInstaller: ferryip\n",
        name, version
    );
    std::fs::write(dist_info_path.join("METADATA"), &metadata)
        .map_err(|e| format!("Write METADATA: {}", e))?;
    std::fs::write(dist_info_path.join("INSTALLER"), "ferryip\n")
        .map_err(|e| format!("Write INSTALLER: {}", e))?;

    // Mark as direct_url.json for PEP 610 compliance
    let direct_url = format!(
        "{{\"url\": \"file://{}\", \"dir_info\": {{\"editable\": true}}}}",
        proj_dir.display()
    );
    std::fs::write(dist_info_path.join("direct_url.json"), &direct_url)
        .map_err(|e| format!("Write direct_url.json: {}", e))?;

    // RECORD
    let record = format!(
        "{pth},\n{di}/METADATA,\n{di}/INSTALLER,\n{di}/direct_url.json,\n{di}/RECORD,,\n",
        pth = pth_file.file_name().unwrap().to_string_lossy(),
        di = dist_info_name,
    );
    std::fs::write(dist_info_path.join("RECORD"), &record)
        .map_err(|e| format!("Write RECORD: {}", e))?;

    if !quiet {
        println!("Successfully installed {} (editable, {})", name, source_root.display());
    }

    // Also install project dependencies
    if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let deps = pyproj.dependencies();
        if !deps.is_empty() {
            if !quiet {
                println!("Installing project dependencies...");
            }
            install_packages(&deps, site_packages, false, false, false, quiet)?;
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

fn list_packages(site_packages: &str, outdated: bool) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    if outdated {
        println!("{:<30} {:<15} {}", "Package", "Version", "Latest");
        println!("{:<30} {:<15} {}", "-------", "-------", "------");
        for pkg in &packages {
            match pypi::fetch_package_info(&pkg.name, None) {
                Ok(latest) => {
                    if latest.version != pkg.version {
                        println!("{:<30} {:<15} {}", pkg.name, pkg.version, latest.version);
                    }
                }
                Err(_) => {} // skip packages that can't be checked
            }
        }
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
            }
        }

        // Install build-system requirements
        let build_reqs = pyproj.build_requires();
        if !build_reqs.is_empty() && !quiet {
            println!("Installing build dependencies...");
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
                println!("Installing project dependencies...");
            }
            for dep in &deps {
                let (name, spec) = parse_version_specifier(dep);
                resolver::install_with_deps(&name, spec.as_deref(), site_packages, false, false, quiet, &mut visited)?;
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

    // Fallback: try requirements.txt
    let req_path = proj_dir.join("requirements.txt");
    if req_path.exists() {
        let reqs = parse_requirements_file(&req_path.to_string_lossy());
        return install_packages(&reqs, site_packages, false, false, false, quiet);
    }

    Err(format!(
        "No pyproject.toml, setup.cfg, or requirements.txt found in {}",
        proj_dir.display()
    ))
}

/// Install dependencies from a setup.cfg file.
fn install_from_setup_cfg(path: &std::path::Path, site_packages: &str, quiet: bool) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

    let mut deps = Vec::new();
    let mut in_install_requires = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[options]" || trimmed.starts_with("[options]") {
            continue;
        }
        if trimmed.starts_with("install_requires") {
            in_install_requires = true;
            // Handle inline: install_requires = package1
            if let Some(eq_pos) = trimmed.find('=') {
                let val = trimmed[eq_pos + 1..].trim();
                if !val.is_empty() {
                    deps.push(val.to_string());
                }
            }
            continue;
        }
        if in_install_requires {
            if trimmed.is_empty() || (!trimmed.starts_with(' ') && !trimmed.starts_with('\t') && trimmed.contains('=')) {
                in_install_requires = false;
                continue;
            }
            if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with('[') {
                deps.push(trimmed.to_string());
            }
            if trimmed.starts_with('[') {
                in_install_requires = false;
            }
        }
    }

    if deps.is_empty() {
        if !quiet {
            println!("No dependencies found in setup.cfg");
        }
        return Ok(());
    }

    install_packages(&deps, site_packages, false, false, false, quiet)
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

    // METADATA
    let mut metadata = format!(
        "Metadata-Version: 2.1\nName: {}\nVersion: {}\n",
        name, version
    );
    if let Some(desc) = pyproj.description() {
        metadata.push_str(&format!("Summary: {}\n", desc));
    }
    for dep in pyproj.dependencies() {
        metadata.push_str(&format!("Requires-Dist: {}\n", dep));
    }

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
    println!("ferryip version: 0.1.0");
    println!("Ferrython compatible: 3.8+");
    println!("Location: {}", exe.display());
    println!("Site-packages: {}", site_packages);
    println!("Cache directory: {}", cache_dir().display());
    println!("Python platform: {}", if cfg!(target_os = "linux") { "linux" }
             else if cfg!(target_os = "macos") { "darwin" }
             else if cfg!(target_os = "windows") { "win32" }
             else { "unknown" });
    println!("Wheel tags: py3-none-any");
    Ok(())
}

fn inspect_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    // Output in JSON-like format (pip inspect compatibility)
    println!("{{");
    println!("  \"version\": \"1\",");
    println!("  \"pip_version\": \"ferryip-0.1.0\",");
    println!("  \"installed\": [");
    for (i, pkg) in packages.iter().enumerate() {
        let comma = if i + 1 < packages.len() { "," } else { "" };
        println!("    {{");
        println!("      \"metadata\": {{");
        println!("        \"name\": \"{}\",", pkg.name);
        println!("        \"version\": \"{}\"", pkg.version);
        println!("      }},");
        println!("      \"installer\": \"ferryip\"");
        println!("    }}{}", comma);
    }
    println!("  ]");
    println!("}}");
    Ok(())
}
