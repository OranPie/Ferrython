//! Dependency resolution — recursive install of package requirements.

use crate::{pypi, installer, registry, version};
use std::collections::HashSet;

/// Install a package and all its transitive dependencies.
pub fn install_with_deps(
    name: &str,
    version_req: Option<&str>,
    site_packages: &str,
    upgrade: bool,
    quiet: bool,
    visited: &mut HashSet<String>,
) -> Result<(), String> {
    let normalized = name.to_lowercase().replace('-', "_").replace('.', "_");

    // Avoid cycles
    if !visited.insert(normalized.clone()) {
        return Ok(());
    }

    // Check if already satisfied
    if !upgrade {
        if let Some(installed) = registry::get_installed(name, site_packages) {
            if let Some(spec) = version_req {
                if version::version_matches(&installed.version, spec) {
                    if !quiet {
                        println!("Requirement already satisfied: {} ({})", name, installed.version);
                    }
                    return Ok(());
                }
            } else {
                if !quiet {
                    println!("Requirement already satisfied: {} ({})", name, installed.version);
                }
                return Ok(());
            }
        }
    }

    // Fetch from PyPI
    let exact_version = version_req.and_then(|s| {
        // If it's a simple ==X.Y.Z, pass the exact version
        let s = s.trim();
        if s.starts_with("==") && !s.contains(',') && !s.contains('*') {
            Some(s[2..].trim())
        } else {
            None
        }
    });

    let release = pypi::fetch_package_info(name, exact_version)
        .map_err(|e| format!("Could not find {}: {}", name, e))?;

    // If we have version specifiers, check if this release satisfies them
    if let Some(spec) = version_req {
        if !version::version_matches(&release.version, spec) {
            return Err(format!(
                "No compatible version found for {} (need {}, found {})",
                name, spec, release.version
            ));
        }
    }

    if !quiet {
        println!("Collecting {} ({})", release.name, release.version);
    }

    // Download and install the package
    let wheel_path = pypi::download_wheel(&release)
        .map_err(|e| format!("Download failed for {}: {}", name, e))?;

    installer::install_wheel(&wheel_path, site_packages, &release.name, &release.version)
        .map_err(|e| format!("Install failed for {}: {}", name, e))?;

    if !quiet {
        println!("  Successfully installed {}-{}", release.name, release.version);
    }

    // Process dependencies
    for dep_str in &release.requires_dist {
        if let Some((dep_name, dep_spec)) = parse_dependency(dep_str) {
            install_with_deps(&dep_name, dep_spec.as_deref(), site_packages, false, quiet, visited)?;
        }
    }

    Ok(())
}

/// Parse a Requires-Dist entry like "requests (>=2.20)" or "typing-extensions; python_version < '3.8'".
fn parse_dependency(dep: &str) -> Option<(String, Option<String>)> {
    let dep = dep.trim();

    // Skip environment markers we can't satisfy
    if let Some(semicolon) = dep.find(';') {
        let marker = dep[semicolon + 1..].trim();
        // Simple heuristic: skip extras-only deps like `extra == "test"`
        if marker.contains("extra ==") || marker.contains("extra==") {
            return None;
        }
        // Skip obviously unsatisfiable markers
        if marker.contains("sys_platform == \"win32\"") && !cfg!(windows) {
            return None;
        }
        if marker.contains("os_name == \"nt\"") && !cfg!(windows) {
            return None;
        }
        // Process the part before the semicolon
        return parse_dependency(&dep[..semicolon]);
    }

    // Handle version specifiers in parentheses: "requests (>=2.20,<3.0)"
    if let Some(paren_start) = dep.find('(') {
        if let Some(paren_end) = dep.find(')') {
            let name = dep[..paren_start].trim();
            let spec = dep[paren_start + 1..paren_end].trim();
            return Some((name.to_string(), Some(spec.to_string())));
        }
    }

    // Handle inline specifiers: "requests>=2.20"
    let (name, spec) = pypi::parse_requirement(dep);
    if let Some(v) = spec {
        Some((name, Some(format!("=={}", v))))
    } else {
        // Check if there are operators in the name (parse_requirement strips them)
        for op in &[">=", "<=", "!=", "~=", "==", ">", "<"] {
            if dep.contains(op) {
                let pos = dep.find(op).unwrap();
                let n = dep[..pos].trim().to_string();
                let s = dep[pos..].trim().to_string();
                return Some((n, Some(s)));
            }
        }
        Some((name, None))
    }
}
