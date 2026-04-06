//! Wheel and sdist installer — extracts packages into site-packages

use std::path::{Path, PathBuf};
use std::fs;

/// Install a wheel file into site-packages
pub fn install_wheel(wheel_path: &Path, site_packages: &str, name: &str, version: &str) -> Result<(), String> {
    let site = Path::new(site_packages);
    if !site.exists() {
        fs::create_dir_all(site).map_err(|e| format!("mkdir: {}", e))?;
    }

    let ext = wheel_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "whl" => install_from_wheel(wheel_path, site, name, version),
        "gz" | "tar" => install_from_sdist(wheel_path, site, name, version),
        _ => Err(format!("Unknown package format: .{}", ext)),
    }
}

/// Extract a .whl (zip) file into site-packages
fn install_from_wheel(wheel_path: &Path, site: &Path, name: &str, version: &str) -> Result<(), String> {
    let file = fs::File::open(wheel_path)
        .map_err(|e| format!("Open wheel: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Invalid wheel: {}", e))?;

    let mut installed_files = Vec::new();
    let dist_info_dir = format!("{}-{}.dist-info", normalize_name(name), version);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Zip entry error: {}", e))?;
        let entry_name = entry.name().to_string();

        // Skip __pycache__ and .pyc files
        if entry_name.contains("__pycache__") || entry_name.ends_with(".pyc") {
            continue;
        }

        let dest_path = site.join(&entry_name);

        if entry.is_dir() {
            fs::create_dir_all(&dest_path)
                .map_err(|e| format!("mkdir {}: {}", dest_path.display(), e))?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
            }
            let mut outfile = fs::File::create(&dest_path)
                .map_err(|e| format!("create {}: {}", dest_path.display(), e))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("write {}: {}", dest_path.display(), e))?;
            installed_files.push(entry_name.clone());
        }
    }

    // Write RECORD file for tracking
    write_record(site, &dist_info_dir, name, version, &installed_files)?;

    Ok(())
}

/// Install from an sdist (.tar.gz) — extracts Python files only
fn install_from_sdist(sdist_path: &Path, site: &Path, name: &str, version: &str) -> Result<(), String> {
    let file = fs::File::open(sdist_path)
        .map_err(|e| format!("Open sdist: {}", e))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut installed_files = Vec::new();
    let dist_info_dir = format!("{}-{}.dist-info", normalize_name(name), version);

    // Create dist-info directory
    let dist_info_path = site.join(&dist_info_dir);
    fs::create_dir_all(&dist_info_path)
        .map_err(|e| format!("mkdir dist-info: {}", e))?;

    let entries = archive.entries()
        .map_err(|e| format!("Tar error: {}", e))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Tar entry: {}", e))?;
        let path = entry.path()
            .map_err(|e| format!("Path error: {}", e))?
            .to_path_buf();
        let path_str = path.to_string_lossy().to_string();

        // Skip non-Python files and test directories
        if !path_str.ends_with(".py") && !path_str.ends_with(".pyi") {
            continue;
        }
        if path_str.contains("/test/") || path_str.contains("/tests/") {
            continue;
        }

        // Strip the top-level directory (name-version/)
        let components: Vec<_> = path.components().collect();
        if components.len() < 2 { continue; }
        let relative: PathBuf = components[1..].iter().collect();

        // Only install files from the package directory (skip setup.py etc)
        let first_component = components.get(1)
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();

        // Heuristic: install if it looks like a package directory or single-file module
        if first_component == "setup.py" || first_component == "setup.cfg"
            || first_component == "pyproject.toml" || first_component.starts_with("test") {
            continue;
        }

        let dest = site.join(&relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir: {}", e))?;
        }
        entry.unpack(&dest)
            .map_err(|e| format!("Unpack {}: {}", dest.display(), e))?;
        installed_files.push(relative.to_string_lossy().to_string());
    }

    write_record(site, &dist_info_dir, name, version, &installed_files)?;
    Ok(())
}

/// Write dist-info METADATA and RECORD for pip compatibility
fn write_record(site: &Path, dist_info_dir: &str, name: &str, version: &str, files: &[String]) -> Result<(), String> {
    let dist_info_path = site.join(dist_info_dir);
    fs::create_dir_all(&dist_info_path)
        .map_err(|e| format!("mkdir dist-info: {}", e))?;

    // METADATA
    let metadata = format!(
        "Metadata-Version: 2.1\nName: {}\nVersion: {}\nInstaller: ferryip\n",
        name, version
    );
    fs::write(dist_info_path.join("METADATA"), metadata)
        .map_err(|e| format!("Write METADATA: {}", e))?;

    // INSTALLER
    fs::write(dist_info_path.join("INSTALLER"), "ferryip\n")
        .map_err(|e| format!("Write INSTALLER: {}", e))?;

    // RECORD
    let mut record_lines: Vec<String> = files.iter()
        .map(|f| format!("{},", f))
        .collect();
    record_lines.push(format!("{}/METADATA,", dist_info_dir));
    record_lines.push(format!("{}/INSTALLER,", dist_info_dir));
    record_lines.push(format!("{}/RECORD,,", dist_info_dir));

    fs::write(dist_info_path.join("RECORD"), record_lines.join("\n"))
        .map_err(|e| format!("Write RECORD: {}", e))?;

    Ok(())
}

/// Normalize package name for directory naming (PEP 503)
fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace('-', "_").replace('.', "_")
}
