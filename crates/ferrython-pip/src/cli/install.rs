use crate::{pypi, registry, resolver};

use super::output::{detail, status};
use super::project::{install_editable, install_project_with_extras};
use super::requirements::parse_requirements_file;

pub(super) fn install_packages(
    specs: &[String],
    site_packages: &str,
    upgrade: bool,
    no_deps: bool,
    _pre: bool,
    quiet: bool,
    verbose: bool,
) -> Result<(), String> {
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
                let hashes: Vec<String> = hashes_str
                    .split(',')
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
            status(
                "Installing",
                format!("[{}/{}] {} (editable)", idx + 1, total, edit_path),
                quiet,
            );
            install_editable(edit_path, site_packages, quiet)?;
            installed_count += 1;
            continue;
        }

        // Handle `ferrypip install .` or `ferrypip install .[dev]` or `ferrypip install ./path`
        if trimmed == "."
            || trimmed.starts_with(".[")
            || trimmed.starts_with("./")
            || trimmed.starts_with("../")
            || std::path::Path::new(trimmed)
                .join("pyproject.toml")
                .exists()
            || std::path::Path::new(trimmed).join("setup.cfg").exists()
            || std::path::Path::new(trimmed).join("setup.py").exists()
        {
            // Extract extras from ".[dev,test]" syntax
            let (proj_path, proj_extras) = if let Some(bracket_start) = trimmed.find('[') {
                if let Some(bracket_end) = trimmed.find(']') {
                    let extras_str = &trimmed[bracket_start + 1..bracket_end];
                    let extras: Vec<String> = extras_str
                        .split(',')
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

        let (name, version_spec, extras) =
            parse_version_specifier_with_extras(&trimmed.to_string());
        if !quiet && total > 1 {
            let ver_display = version_spec.as_deref().unwrap_or("");
            status(
                "Processing",
                format!("[{}/{}] {}{}", idx + 1, total, name, ver_display),
                quiet,
            );
        }
        if verbose {
            let extras_display = if extras.is_empty() {
                String::new()
            } else {
                format!("[{}]", extras.join(","))
            };
            detail(
                format!(
                    "Resolving {}{}{}",
                    name,
                    extras_display,
                    version_spec
                        .as_deref()
                        .map(|v| format!(" ({})", v))
                        .unwrap_or_default()
                ),
                quiet,
            );
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
        println!();
        status(
            "Complete",
            format!(
                "processed {} package(s) in {:.1}s",
                installed_count,
                elapsed.as_secs_f64()
            ),
            quiet,
        );
    }
    Ok(())
}

/// Verify file hashes match expected values (from --hash= in requirements files).
fn verify_file_hashes(path: &str, expected_hashes: &[String]) -> Result<(), String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("Cannot read '{}' for hash verification: {}", path, e))?;

    use sha2::{Digest, Sha256};
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
pub(super) fn verify_all_installed(site_packages: &str, specs: &[String], quiet: bool) {
    for spec in specs {
        let trimmed = spec.trim();
        // Skip flags and special entries
        if trimmed.starts_with("flag:")
            || trimmed.starts_with("editable:")
            || trimmed.starts_with("hash:")
            || trimmed == "."
            || trimmed.starts_with("./")
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
            eprintln!(
                "  ✗ {} has {} file(s) with mismatched hashes",
                name,
                failures.len()
            );
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

    let filename = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

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

    status(
        "Installing",
        format!("{} ({}) from local file", name, version),
        quiet,
    );

    crate::installer::install_wheel(file_path, site_packages, &name, &version)?;

    // Verify RECORD hashes after install
    let failures = crate::installer::verify_installed_record(site_packages, &name);
    if !failures.is_empty() {
        eprintln!(
            "WARNING: {} file(s) failed RECORD hash verification:",
            failures.len()
        );
        for f in failures.iter().take(5) {
            eprintln!("  {}", f);
        }
        if failures.len() > 5 {
            eprintln!("  ... and {} more", failures.len() - 5);
        }
    }

    status("Installed", format!("{}-{}", name, version), quiet);
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
                                false,
                                false,
                                quiet,
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
pub(super) fn parse_version_specifier(spec: &str) -> (String, Option<String>) {
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
            let extras: Vec<String> = extras_str
                .split(',')
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
pub(super) fn dry_run_install(
    packages: &[String],
    requirement_files: &[String],
    quiet: bool,
) -> Result<(), String> {
    let mut specs: Vec<String> = packages.to_vec();
    for req_file in requirement_files {
        specs.extend(parse_requirements_file(req_file));
    }

    if specs.is_empty() {
        println!("No packages to install.");
        return Ok(());
    }

    println!("Dry run");
    println!("-------");
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
                println!(
                    "{:<10} {}{} {} (latest: {})",
                    "Would add", name, extras_str, ver_str, info.version
                );

                // Show transitive dependencies
                if !transitive_deps.is_empty() {
                    for (dep_name, dep_ver) in &transitive_deps {
                        let dep_ver_str = dep_ver.as_deref().unwrap_or("");
                        match pypi::fetch_package_info(&dep_name, None) {
                            Ok(dep_info) => {
                                println!(
                                    "  {:<8} {} {} (latest: {})",
                                    "requires", dep_name, dep_ver_str, dep_info.version
                                );
                            }
                            Err(_) => {
                                println!("  {:<8} {} {}", "requires", dep_name, dep_ver_str);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if !quiet {
                    println!("{:<10} {} - could not resolve: {}", "Skipped", name, e);
                }
            }
        }
    }
    Ok(())
}
