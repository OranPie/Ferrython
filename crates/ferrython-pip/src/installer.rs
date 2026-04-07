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
    let data_dir = format!("{}-{}.data", normalize_name(name), version);

    // Compute install layout paths for .data directory handling
    let bin_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("bin"))
        .unwrap_or_else(|| site.join("..").join("bin"));
    let include_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("include"))
        .unwrap_or_else(|| site.join("..").join("include"));

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Zip entry error: {}", e))?;
        let entry_name = entry.name().to_string();

        // Skip __pycache__ and .pyc files
        if entry_name.contains("__pycache__") || entry_name.ends_with(".pyc") {
            continue;
        }

        // Handle .data directory: remap to correct install locations
        let dest_path = if entry_name.starts_with(&data_dir) {
            let relative = &entry_name[data_dir.len()..].trim_start_matches('/');
            if relative.starts_with("scripts/") {
                bin_dir.join(&relative["scripts/".len()..])
            } else if relative.starts_with("headers/") {
                include_dir.join(&relative["headers/".len()..])
            } else if relative.starts_with("data/") {
                site.parent()
                    .and_then(|p| p.parent())
                    .unwrap_or(site)
                    .join(&relative["data/".len()..])
            } else if relative.starts_with("purelib/") {
                site.join(&relative["purelib/".len()..])
            } else if relative.starts_with("platlib/") {
                site.join(&relative["platlib/".len()..])
            } else {
                site.join(relative)
            }
        } else {
            site.join(&entry_name)
        };

        // Security: reject paths that escape site-packages via traversal
        let canonical_site = site.canonicalize().unwrap_or_else(|_| site.to_path_buf());
        let canonical_dest = if dest_path.exists() {
            dest_path.canonicalize().unwrap_or_else(|_| dest_path.to_path_buf())
        } else {
            // For new files, canonicalize the existing parent and append the rest
            let mut base = dest_path.clone();
            while !base.exists() {
                if !base.pop() { break; }
            }
            let base_canon = base.canonicalize().unwrap_or(base);
            base_canon.join(dest_path.strip_prefix(&base_canon).unwrap_or(&dest_path))
        };
        if !canonical_dest.starts_with(&canonical_site) && !canonical_dest.starts_with(&bin_dir) && !canonical_dest.starts_with(&include_dir) {
            return Err(format!(
                "Wheel contains path traversal: {} escapes {}", entry_name, site.display()
            ));
        }

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

            // Make scripts executable
            #[cfg(unix)]
            if entry_name.starts_with(&data_dir) && entry_name.contains("scripts/") {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&dest_path, fs::Permissions::from_mode(0o755));
            }

            installed_files.push(entry_name.clone());
        }
    }

    // Write RECORD file for tracking
    write_record(site, &dist_info_dir, name, version, &installed_files)?;

    // Generate console_scripts from entry_points.txt if present
    generate_console_scripts(site, &dist_info_dir)?;

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

/// Write dist-info METADATA, WHEEL, INSTALLER, RECORD, and top_level.txt for pip compatibility
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

    // WHEEL (PEP 427)
    let wheel_meta = "Wheel-Version: 1.0\nGenerator: ferryip 0.1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n";
    fs::write(dist_info_path.join("WHEEL"), wheel_meta)
        .map_err(|e| format!("Write WHEEL: {}", e))?;

    // INSTALLER
    fs::write(dist_info_path.join("INSTALLER"), "ferryip\n")
        .map_err(|e| format!("Write INSTALLER: {}", e))?;

    // top_level.txt — infer top-level package names from installed files
    let mut top_level = std::collections::BTreeSet::new();
    for f in files {
        // A top-level module is either `foo/__init__.py` → "foo" or `bar.py` → "bar"
        let components: Vec<&str> = f.split('/').collect();
        if components.len() >= 2 && !components[0].contains('.') && !components[0].ends_with(".dist-info") && !components[0].ends_with(".data") {
            top_level.insert(components[0].to_string());
        } else if components.len() == 1 && f.ends_with(".py") {
            if let Some(stem) = f.strip_suffix(".py") {
                if stem != "__init__" {
                    top_level.insert(stem.to_string());
                }
            }
        }
    }
    if !top_level.is_empty() {
        let content = top_level.into_iter().collect::<Vec<_>>().join("\n") + "\n";
        fs::write(dist_info_path.join("top_level.txt"), content)
            .map_err(|e| format!("Write top_level.txt: {}", e))?;
    }

    // RECORD (with SHA256 hashes)
    let mut record_lines: Vec<String> = Vec::new();
    for f in files {
        let file_path = site.join(f);
        let hash_entry = if file_path.exists() {
            if let Ok(data) = fs::read(&file_path) {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(&data);
                let hash = format!("{:x}", hasher.finalize());
                format!("{},sha256={},{}", f, hash, data.len())
            } else {
                format!("{},", f)
            }
        } else {
            format!("{},", f)
        };
        record_lines.push(hash_entry);
    }
    record_lines.push(format!("{}/METADATA,", dist_info_dir));
    record_lines.push(format!("{}/WHEEL,", dist_info_dir));
    record_lines.push(format!("{}/INSTALLER,", dist_info_dir));
    if site.join(dist_info_dir).join("top_level.txt").exists() {
        record_lines.push(format!("{}/top_level.txt,", dist_info_dir));
    }
    record_lines.push(format!("{}/RECORD,,", dist_info_dir));

    fs::write(dist_info_path.join("RECORD"), record_lines.join("\n") + "\n")
        .map_err(|e| format!("Write RECORD: {}", e))?;

    Ok(())
}

/// Normalize package name for directory naming (PEP 503)
fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace('-', "_").replace('.', "_")
}

/// Generate console_scripts from entry_points.txt in a dist-info directory.
fn generate_console_scripts(site: &Path, dist_info_dir: &str) -> Result<(), String> {
    let entry_points_path = site.join(dist_info_dir).join("entry_points.txt");
    if !entry_points_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&entry_points_path)
        .map_err(|e| format!("Read entry_points.txt: {}", e))?;

    let mut in_console_scripts = false;
    let bin_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("bin"))
        .unwrap_or_else(|| site.join("../bin"));

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[console_scripts]" {
            in_console_scripts = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_console_scripts = false;
            continue;
        }
        if !in_console_scripts || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse: script_name = module:function
        if let Some((script_name, entry)) = trimmed.split_once('=') {
            let script_name = script_name.trim();
            let entry = entry.trim();

            if let Some((module, func)) = entry.split_once(':') {
                let module = module.trim();
                let func = func.trim();

                let script_content = format!(
                    "#!/usr/bin/env ferrython\nimport sys\nfrom {} import {}\nsys.exit({}())\n",
                    module, func, func
                );

                let _ = fs::create_dir_all(&bin_dir);
                let script_path = bin_dir.join(script_name);
                fs::write(&script_path, &script_content)
                    .map_err(|e| format!("Write script {}: {}", script_name, e))?;

                // Make executable on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(
                        &script_path,
                        fs::Permissions::from_mode(0o755),
                    );
                }
            }
        }
    }

    Ok(())
}
