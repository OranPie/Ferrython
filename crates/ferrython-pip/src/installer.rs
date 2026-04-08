//! Wheel and sdist installer — extracts packages into site-packages

use std::path::{Path, PathBuf};
use std::fs;

use crate::metadata::PackageMetadata;

/// Install a wheel file into site-packages
pub fn install_wheel(wheel_path: &Path, site_packages: &str, name: &str, version: &str) -> Result<(), String> {
    install_wheel_with_metadata(wheel_path, site_packages, name, version, None)
}

/// Install a wheel file into site-packages, optionally using rich metadata for non-wheel sources.
pub fn install_wheel_with_metadata(
    wheel_path: &Path,
    site_packages: &str,
    name: &str,
    version: &str,
    pkg_meta: Option<&PackageMetadata>,
) -> Result<(), String> {
    let site = Path::new(site_packages);
    if !site.exists() {
        fs::create_dir_all(site).map_err(|e| format!("mkdir: {}", e))?;
    }

    let ext = wheel_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "whl" => install_from_wheel(wheel_path, site, name, version),
        "gz" | "tar" => install_from_sdist(wheel_path, site, name, version, pkg_meta),
        _ => Err(format!("Unknown package format: .{}", ext)),
    }
}

/// Install a package in editable mode by writing a .pth file.
#[allow(dead_code)]
pub fn install_editable(source_dir: &Path, site_packages: &str, name: &str, version: &str) -> Result<(), String> {
    install_editable_with_metadata(source_dir, site_packages, name, version, None)
}

/// Install a package in editable mode with optional rich metadata.
pub fn install_editable_with_metadata(
    source_dir: &Path,
    site_packages: &str,
    name: &str,
    version: &str,
    pkg_meta: Option<&PackageMetadata>,
) -> Result<(), String> {
    let site = Path::new(site_packages);
    fs::create_dir_all(site).map_err(|e| format!("mkdir: {}", e))?;

    let package_name = normalize_name(name);
    let source_dir = source_dir.canonicalize()
        .map_err(|e| format!("Cannot resolve path '{}': {}", source_dir.display(), e))?;

    // Determine source root: prefer src/ layout, then top-level
    let source_root = if source_dir.join("src").exists() {
        source_dir.join("src")
    } else {
        source_dir.clone()
    };

    // Write .pth file — each line is a path added to sys.path
    let pth_file = site.join(format!("__{}.pth", package_name));
    fs::write(&pth_file, format!("{}\n", source_root.display()))
        .map_err(|e| format!("Write .pth file: {}", e))?;

    // Write dist-info for pip/ferryip compatibility
    let dist_info_name = format!("{}-{}.dist-info", package_name, version);
    let dist_info_path = site.join(&dist_info_name);
    fs::create_dir_all(&dist_info_path)
        .map_err(|e| format!("mkdir dist-info: {}", e))?;

    // METADATA
    let metadata = if let Some(meta) = pkg_meta {
        meta.render()
    } else {
        format!(
            "Metadata-Version: 2.1\nName: {}\nVersion: {}\nInstaller: ferryip\n",
            name, version
        )
    };
    fs::write(dist_info_path.join("METADATA"), &metadata)
        .map_err(|e| format!("Write METADATA: {}", e))?;

    // INSTALLER
    fs::write(dist_info_path.join("INSTALLER"), "ferryip\n")
        .map_err(|e| format!("Write INSTALLER: {}", e))?;

    // PEP 610 direct_url.json
    let direct_url = format!(
        "{{\"url\": \"file://{}\", \"dir_info\": {{\"editable\": true}}}}",
        source_dir.display()
    );
    fs::write(dist_info_path.join("direct_url.json"), &direct_url)
        .map_err(|e| format!("Write direct_url.json: {}", e))?;

    // top_level.txt
    fs::write(dist_info_path.join("top_level.txt"), format!("{}\n", package_name))
        .map_err(|e| format!("Write top_level.txt: {}", e))?;

    // RECORD
    let record = format!(
        "{pth},\n{di}/METADATA,\n{di}/INSTALLER,\n{di}/direct_url.json,\n{di}/top_level.txt,\n{di}/RECORD,,\n",
        pth = pth_file.file_name().unwrap().to_string_lossy(),
        di = dist_info_name,
    );
    fs::write(dist_info_path.join("RECORD"), &record)
        .map_err(|e| format!("Write RECORD: {}", e))?;

    Ok(())
}

/// Extract a .whl (zip) file into site-packages
fn install_from_wheel(wheel_path: &Path, site: &Path, name: &str, version: &str) -> Result<(), String> {
    let file = fs::File::open(wheel_path)
        .map_err(|e| format!("Open wheel: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Invalid wheel: {}", e))?;

    let mut installed_files = Vec::new();
    let norm_name = normalize_name(name);
    let dist_info_dir = format!("{}-{}.dist-info", norm_name, version);
    let data_dir = format!("{}-{}.data", norm_name, version);

    // Detect the actual dist-info directory name from the wheel (may differ in casing)
    let mut actual_dist_info_dir = dist_info_dir.clone();
    let mut actual_data_dir = data_dir.clone();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let ename = entry.name().to_string();
            if ename.ends_with(".dist-info/") || ename.contains(".dist-info/") {
                let prefix = ename.split(".dist-info/").next().unwrap_or("");
                if !prefix.is_empty() && !prefix.contains('/') {
                    actual_dist_info_dir = format!("{}.dist-info", prefix);
                    break;
                }
            }
        }
    }
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let ename = entry.name().to_string();
            if ename.ends_with(".data/") || ename.contains(".data/") {
                let prefix = ename.split(".data/").next().unwrap_or("");
                if !prefix.is_empty() && !prefix.contains('/') {
                    actual_data_dir = format!("{}.data", prefix);
                    break;
                }
            }
        }
    }

    // Compute install layout paths for .data directory handling
    let bin_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("bin"))
        .unwrap_or_else(|| site.join("..").join("bin"));
    let include_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("include"))
        .unwrap_or_else(|| site.join("..").join("include"));

    // Track which files came from the wheel's RECORD (if present)
    let mut wheel_record_entries: Vec<String> = Vec::new();
    let mut has_wheel_entry_points = false;

    // First pass: check for entry_points.txt and existing RECORD
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let ename = entry.name().to_string();
            if ename.ends_with("/entry_points.txt") && ename.contains(".dist-info") {
                has_wheel_entry_points = true;
            }
            if ename.ends_with("/RECORD") && ename.contains(".dist-info") {
                // Read the wheel's RECORD to preserve it
                let content = std::io::read_to_string(entry).unwrap_or_default();
                wheel_record_entries = content.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect();
            }
        }
    }

    // Re-open archive for extraction
    let file = fs::File::open(wheel_path)
        .map_err(|e| format!("Open wheel: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Invalid wheel: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Zip entry error: {}", e))?;
        let entry_name = entry.name().to_string();

        // Skip __pycache__ and .pyc files
        if entry_name.contains("__pycache__") || entry_name.ends_with(".pyc") {
            continue;
        }

        // Handle .data directory: remap to correct install locations
        let dest_path = if entry_name.starts_with(&actual_data_dir) {
            let relative = &entry_name[actual_data_dir.len()..].trim_start_matches('/');
            if relative.starts_with("scripts/") {
                bin_dir.join(&relative["scripts/".len()..])
            } else if relative.starts_with("headers/") || relative.starts_with("include/") {
                let prefix_len = relative.find('/').map(|p| p + 1).unwrap_or(0);
                include_dir.join(&relative[prefix_len..])
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
            if entry_name.starts_with(&actual_data_dir) && entry_name.contains("scripts/") {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&dest_path, fs::Permissions::from_mode(0o755));
            }

            installed_files.push(entry_name.clone());
        }
    }

    // Write RECORD and metadata files, preserving original wheel RECORD entries when available
    write_record(site, &actual_dist_info_dir, name, version, &installed_files, &wheel_record_entries, None)?;

    // Generate console_scripts and gui_scripts from entry_points.txt if present
    if has_wheel_entry_points {
        generate_console_scripts(site, &actual_dist_info_dir)?;
        generate_gui_scripts(site, &actual_dist_info_dir)?;
    }

    Ok(())
}

/// Install from an sdist (.tar.gz) — extracts Python files only
fn install_from_sdist(sdist_path: &Path, site: &Path, name: &str, version: &str, pkg_meta: Option<&PackageMetadata>) -> Result<(), String> {
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

    // Collect pyproject.toml and setup.cfg content from the sdist for metadata
    let mut sdist_pyproject: Option<String> = None;
    let mut sdist_setup_cfg: Option<String> = None;
    {
        let file2 = fs::File::open(sdist_path)
            .map_err(|e| format!("Open sdist: {}", e))?;
        let gz2 = flate2::read::GzDecoder::new(file2);
        let mut archive2 = tar::Archive::new(gz2);
        let entries2 = archive2.entries().map_err(|e| format!("Tar error: {}", e))?;
        for entry in entries2 {
            let mut entry = entry.map_err(|e| format!("Tar entry: {}", e))?;
            let path = entry.path().map_err(|e| format!("Path error: {}", e))?.to_path_buf();
            let path_str = path.to_string_lossy().to_string();
            let components: Vec<_> = path.components().collect();
            if components.len() == 2 {
                let filename = components[1].as_os_str().to_string_lossy();
                if filename == "pyproject.toml" {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut entry, &mut buf).ok();
                    sdist_pyproject = Some(buf);
                } else if filename == "setup.cfg" {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut entry, &mut buf).ok();
                    sdist_setup_cfg = Some(buf);
                }
            }
            let _ = path_str; // suppress unused warning
        }
    }

    // Derive rich metadata from the sdist contents
    let derived_meta: Option<PackageMetadata> = if pkg_meta.is_some() {
        None // caller provided metadata, use that
    } else if let Some(ref pyproject_content) = sdist_pyproject {
        ferrython_toolchain::pyproject::parse_pyproject_str(pyproject_content)
            .ok()
            .map(|pp| PackageMetadata::from_pyproject(&pp))
    } else if let Some(ref setup_cfg_content) = sdist_setup_cfg {
        crate::setup_cfg::parse_setup_cfg_str(setup_cfg_content)
            .ok()
            .map(|sc| PackageMetadata::from_setup_cfg(&sc))
    } else {
        None
    };

    let effective_meta = pkg_meta.or(derived_meta.as_ref());

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

    write_record(site, &dist_info_dir, name, version, &installed_files, &[], effective_meta)?;
    Ok(())
}

/// Write dist-info METADATA, WHEEL, INSTALLER, RECORD, and top_level.txt for pip compatibility.
///
/// If `wheel_record` is non-empty, it contains the original RECORD entries from the wheel
/// and we merge them instead of recomputing hashes for every file.
///
/// If `pkg_meta` is provided, it is used to write a complete METADATA file when one does
/// not already exist (i.e. for sdist / editable installs).
fn write_record(
    site: &Path,
    dist_info_dir: &str,
    name: &str,
    version: &str,
    files: &[String],
    wheel_record: &[String],
    pkg_meta: Option<&PackageMetadata>,
) -> Result<(), String> {
    let dist_info_path = site.join(dist_info_dir);
    fs::create_dir_all(&dist_info_path)
        .map_err(|e| format!("mkdir dist-info: {}", e))?;

    // Only write METADATA if it doesn't already exist (wheel may have provided a richer one)
    let metadata_path = dist_info_path.join("METADATA");
    if !metadata_path.exists() {
        let metadata = if let Some(meta) = pkg_meta {
            meta.render()
        } else {
            format!(
                "Metadata-Version: 2.1\nName: {}\nVersion: {}\nInstaller: ferryip\n",
                name, version
            )
        };
        fs::write(&metadata_path, metadata)
            .map_err(|e| format!("Write METADATA: {}", e))?;
    }

    // Only write WHEEL if it doesn't already exist
    let wheel_path = dist_info_path.join("WHEEL");
    if !wheel_path.exists() {
        let wheel_meta = "Wheel-Version: 1.0\nGenerator: ferryip 0.1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n";
        fs::write(&wheel_path, wheel_meta)
            .map_err(|e| format!("Write WHEEL: {}", e))?;
    }

    // INSTALLER (always overwrite — we installed it)
    fs::write(dist_info_path.join("INSTALLER"), "ferryip\n")
        .map_err(|e| format!("Write INSTALLER: {}", e))?;

    // top_level.txt — infer top-level package names if not already present
    let top_level_path = dist_info_path.join("top_level.txt");
    if !top_level_path.exists() {
        let mut top_level = std::collections::BTreeSet::new();
        for f in files {
            let components: Vec<&str> = f.split('/').collect();
            if components.len() >= 2 && !components[0].contains('.')
                && !components[0].ends_with("dist-info") && !components[0].ends_with("data")
            {
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
            fs::write(&top_level_path, content)
                .map_err(|e| format!("Write top_level.txt: {}", e))?;
        }
    }

    // Build RECORD: prefer original wheel record entries, supplement with our own
    let mut record_lines: Vec<String> = Vec::new();
    let mut seen_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Use original wheel RECORD entries when available
    if !wheel_record.is_empty() {
        for entry in wheel_record {
            let file_name = entry.split(',').next().unwrap_or("").to_string();
            if !file_name.is_empty() && !file_name.ends_with("/RECORD") {
                seen_files.insert(file_name);
                record_lines.push(entry.clone());
            }
        }
    }

    // Add entries for files not in the original RECORD
    for f in files {
        if seen_files.contains(f.as_str()) {
            continue;
        }
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

    // Ensure dist-info metadata files are tracked
    for meta_file in &["METADATA", "WHEEL", "INSTALLER", "top_level.txt", "entry_points.txt"] {
        let entry_path = format!("{}/{}", dist_info_dir, meta_file);
        if dist_info_path.join(meta_file).exists() && !seen_files.contains(&entry_path) {
            record_lines.push(format!("{},", entry_path));
        }
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
    generate_scripts_from_section(site, dist_info_dir, "[console_scripts]")
}

/// Generate gui_scripts from entry_points.txt in a dist-info directory.
fn generate_gui_scripts(site: &Path, dist_info_dir: &str) -> Result<(), String> {
    generate_scripts_from_section(site, dist_info_dir, "[gui_scripts]")
}

/// Generate executable scripts from a named section in entry_points.txt.
fn generate_scripts_from_section(site: &Path, dist_info_dir: &str, section: &str) -> Result<(), String> {
    let entry_points_path = site.join(dist_info_dir).join("entry_points.txt");
    if !entry_points_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&entry_points_path)
        .map_err(|e| format!("Read entry_points.txt: {}", e))?;

    let mut in_target_section = false;
    let bin_dir = site.parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("bin"))
        .unwrap_or_else(|| site.join("../bin"));

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == section {
            in_target_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_target_section = false;
            continue;
        }
        if !in_target_section || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse: script_name = module:function [extras]
        if let Some((script_name, entry)) = trimmed.split_once('=') {
            let script_name = script_name.trim();
            let entry = entry.trim();
            // Strip optional extras specifier like [extra1,extra2]
            let entry_clean = if let Some(bracket) = entry.find('[') {
                entry[..bracket].trim()
            } else {
                entry
            };

            if let Some((module, func)) = entry_clean.split_once(':') {
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

/// Read Requires-Dist from an installed package's METADATA file.
///
/// Returns the list of dependency specifiers found in the installed metadata.
/// This is used by the resolver to get accurate dependency info from the wheel
/// (which may differ from what the PyPI JSON API reports).
pub fn read_requires_dist_from_installed(site_packages: &str, name: &str) -> Vec<String> {
    let site = Path::new(site_packages);
    let normalized = normalize_name(name);

    // Search for the dist-info directory
    let entries = match fs::read_dir(site) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.ends_with(".dist-info") {
            let pkg_part = match fname.strip_suffix(".dist-info") {
                Some(p) => p,
                None => continue,
            };
            let pkg_name = pkg_part.split('-').next().unwrap_or("");
            if normalize_name(pkg_name) == normalized {
                let metadata_path = entry.path().join("METADATA");
                if let Ok(content) = fs::read_to_string(&metadata_path) {
                    return content.lines()
                        .filter_map(|line| line.strip_prefix("Requires-Dist: "))
                        .map(|s| s.trim().to_string())
                        .collect();
                }
            }
        }
    }

    vec![]
}
