//! Package registry — tracks installed packages via dist-info directories

use std::path::Path;
use std::fs;

/// Information about an installed package
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub requires: Option<Vec<String>>,
    pub files: Vec<String>,
}

/// List all installed packages by scanning site-packages for .dist-info dirs
pub fn list_installed(site_packages: &str) -> Vec<InstalledPackage> {
    let site = Path::new(site_packages);
    if !site.exists() { return vec![]; }

    let mut packages = Vec::new();
    let entries = match fs::read_dir(site) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".dist-info") && entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if let Some(pkg) = read_dist_info(&entry.path()) {
                packages.push(pkg);
            }
        }
    }

    packages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packages
}

/// Get info about a specific installed package
pub fn get_installed(name: &str, site_packages: &str) -> Option<InstalledPackage> {
    let normalized = normalize(name);
    list_installed(site_packages)
        .into_iter()
        .find(|p| normalize(&p.name) == normalized)
}

/// Uninstall a package by removing its files and dist-info
pub fn uninstall(name: &str, site_packages: &str) -> Result<(), String> {
    let site = Path::new(site_packages);
    let pkg = get_installed(name, site_packages)
        .ok_or_else(|| format!("Package '{}' is not installed", name))?;

    // Remove files listed in RECORD
    for file in &pkg.files {
        let path = site.join(file);
        if path.exists() {
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }

    // Remove dist-info directory
    let dist_info = find_dist_info_dir(site, name);
    if let Some(dir) = dist_info {
        let _ = fs::remove_dir_all(dir);
    }

    // Clean up empty package directories
    cleanup_empty_dirs(site);

    Ok(())
}

/// Read METADATA from a .dist-info directory
fn read_dist_info(dist_info_path: &Path) -> Option<InstalledPackage> {
    let metadata_path = dist_info_path.join("METADATA");
    let metadata_content = fs::read_to_string(&metadata_path).ok()?;

    let mut name = String::new();
    let mut version = String::new();
    let mut summary = None;
    let mut author = None;
    let mut license = None;
    let mut requires = Vec::new();

    for line in metadata_content.lines() {
        if let Some(val) = line.strip_prefix("Name: ") {
            name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Version: ") {
            version = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("Summary: ") {
            summary = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Author: ") {
            author = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("License: ") {
            license = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Requires-Dist: ") {
            requires.push(val.trim().to_string());
        }
    }

    if name.is_empty() {
        // Try to extract from directory name
        let dir_name = dist_info_path.file_name()?.to_str()?;
        let without_suffix = dir_name.strip_suffix(".dist-info")?;
        let parts: Vec<&str> = without_suffix.splitn(2, '-').collect();
        name = parts.first()?.to_string();
        if parts.len() > 1 {
            version = parts[1].to_string();
        }
    }

    // Read RECORD for file list
    let record_path = dist_info_path.join("RECORD");
    let files = if let Ok(content) = fs::read_to_string(&record_path) {
        content.lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split(',').next().unwrap_or("").to_string())
            .filter(|f| !f.is_empty())
            .collect()
    } else {
        vec![]
    };

    Some(InstalledPackage {
        name,
        version,
        summary,
        author,
        license,
        requires: if requires.is_empty() { None } else { Some(requires) },
        files,
    })
}

fn find_dist_info_dir(site: &Path, name: &str) -> Option<std::path::PathBuf> {
    let normalized = normalize(name);
    let entries = fs::read_dir(site).ok()?;
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.ends_with(".dist-info") {
            let pkg_part = fname.strip_suffix(".dist-info")?;
            let pkg_name = pkg_part.split('-').next()?;
            if normalize(pkg_name) == normalized {
                return Some(entry.path());
            }
        }
    }
    None
}

fn normalize(name: &str) -> String {
    name.to_lowercase().replace('-', "_").replace('.', "_")
}

fn cleanup_empty_dirs(site: &Path) {
    if let Ok(entries) = fs::read_dir(site) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let path = entry.path();
                if is_empty_dir(&path) {
                    let _ = fs::remove_dir_all(&path);
                }
            }
        }
    }
}

fn is_empty_dir(path: &Path) -> bool {
    match fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}
