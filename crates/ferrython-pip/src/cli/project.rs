use crate::{metadata::PackageMetadata, resolver};

use super::install::{install_packages, parse_version_specifier};
use super::output::status;
use super::requirements::parse_requirements_file;

/// Install a package in editable mode: writes a .pth file pointing at the source directory.
pub(super) fn install_editable(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
    let pyproject_path = proj_dir.join("pyproject.toml");
    let setup_cfg_path = proj_dir.join("setup.cfg");

    let (name, version, pkg_meta) = if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let meta = PackageMetadata::from_pyproject(&pyproj);
        let name = pyproj.name().unwrap_or_else(|| {
            proj_dir
                .file_name()
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
            proj_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let version = cfg.version.unwrap_or_else(|| "0.0.0".into());
        (name, version, Some(meta))
    } else {
        let name = proj_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        (name, "0.0.0".to_string(), None)
    };

    crate::installer::install_editable_with_metadata(
        &proj_dir,
        site_packages,
        &name,
        &version,
        pkg_meta.as_ref(),
    )?;

    if !quiet {
        let source_root = if proj_dir.join("src").exists() {
            proj_dir.join("src")
        } else {
            proj_dir.clone()
        };
        status(
            "Installed",
            format!("{} (editable, {})", name, source_root.display()),
            quiet,
        );
    }

    // Also install project dependencies
    if pyproject_path.exists() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        let deps = pyproj.dependencies();
        if !deps.is_empty() {
            if !quiet {
                status(
                    "Resolving",
                    format!(
                        "{} project dependenc{}",
                        deps.len(),
                        if deps.len() == 1 { "y" } else { "ies" }
                    ),
                    quiet,
                );
            }
            install_packages(&deps, site_packages, false, false, false, quiet, false)?;
        }

        // Install build-system requirements too
        let build_reqs = pyproj.build_requires();
        if !build_reqs.is_empty() {
            if !quiet {
                status(
                    "Resolving",
                    format!(
                        "{} build dependenc{}",
                        build_reqs.len(),
                        if build_reqs.len() == 1 { "y" } else { "ies" }
                    ),
                    quiet,
                );
            }
            install_packages(
                &build_reqs,
                site_packages,
                false,
                false,
                false,
                quiet,
                false,
            )?;
        }
    } else if setup_cfg_path.exists() {
        let cfg = crate::setup_cfg::parse_setup_cfg(&setup_cfg_path)?;
        if !cfg.install_requires.is_empty() {
            if !quiet {
                status(
                    "Resolving",
                    format!(
                        "{} project dependenc{}",
                        cfg.install_requires.len(),
                        if cfg.install_requires.len() == 1 {
                            "y"
                        } else {
                            "ies"
                        }
                    ),
                    quiet,
                );
            }
            install_packages(
                &cfg.install_requires,
                site_packages,
                false,
                false,
                false,
                quiet,
                false,
            )?;
        }
    }

    Ok(())
}

/// Install dependencies from a project's pyproject.toml or setup.cfg, including optional extras.
pub(super) fn install_project_with_extras(
    path: &str,
    requested_extras: &[String],
    site_packages: &str,
    quiet: bool,
) -> Result<(), String> {
    let proj_dir = std::path::Path::new(path);
    let pyproject_path = proj_dir.join("pyproject.toml");

    if pyproject_path.exists() && !requested_extras.is_empty() {
        let pyproj = ferrython_toolchain::pyproject::parse_pyproject(&pyproject_path)?;
        if !quiet {
            if let Some(name) = pyproj.name() {
                let version = pyproj.version().unwrap_or("0.0.0");
                println!(
                    "Installing project: {} ({}) with extras: [{}]",
                    name,
                    version,
                    requested_extras.join(", ")
                );
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
                    eprintln!(
                        "WARNING: Extra '{}' not found. Available extras: {}",
                        extra,
                        available.join(", ")
                    );
                }
                continue;
            }
            if !quiet {
                println!(
                    "Installing extra '{}' ({} dependencies)...",
                    extra,
                    extra_deps.len()
                );
            }
            for dep in &extra_deps {
                let (name, spec) = parse_version_specifier(dep);
                resolver::install_with_deps(
                    &name,
                    spec.as_deref(),
                    site_packages,
                    false,
                    false,
                    quiet,
                    &mut visited,
                )?;
            }
        }
        return Ok(());
    }

    // No extras or no pyproject.toml - fall through to regular install
    install_project(path, site_packages, quiet)
}

/// Install dependencies from a project's pyproject.toml or setup.cfg.
pub(super) fn install_project(path: &str, site_packages: &str, quiet: bool) -> Result<(), String> {
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
            resolver::install_with_deps(
                &name,
                spec.as_deref(),
                site_packages,
                false,
                false,
                quiet,
                &mut visited,
            )?;
        }

        // Install project dependencies
        let deps = pyproj.dependencies();
        if !deps.is_empty() {
            if !quiet {
                println!("Installing {} project dependencies...", deps.len());
            }
            for dep in &deps {
                let (name, spec) = parse_version_specifier(dep);
                resolver::install_with_deps(
                    &name,
                    spec.as_deref(),
                    site_packages,
                    false,
                    false,
                    quiet,
                    &mut visited,
                )?;
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
                            let pkg_names: Vec<&str> =
                                pkgs.iter().filter_map(|v| v.as_str()).collect();
                            if !pkg_names.is_empty() {
                                println!("  Setuptools packages: {}", pkg_names.join(", "));
                            }
                        }
                    }
                    if let Some(pkg_dir) = setuptools.get("package-dir") {
                        if let Some(table) = pkg_dir.as_table() {
                            for (key, val) in table {
                                if let Some(dir) = val.as_str() {
                                    let label = if key.is_empty() {
                                        "(root)"
                                    } else {
                                        key.as_str()
                                    };
                                    println!("  Package dir: {} -> {}", label, dir);
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
fn install_from_setup_cfg(
    path: &std::path::Path,
    site_packages: &str,
    quiet: bool,
) -> Result<(), String> {
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
        println!(
            "Installing {} dependencies from setup.cfg...",
            cfg.install_requires.len()
        );
        if !cfg.extras_require.is_empty() {
            let extras: Vec<&String> = cfg.extras_require.keys().collect();
            println!(
                "  Available extras: {}",
                extras
                    .iter()
                    .map(|e| e.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    install_packages(
        &cfg.install_requires,
        site_packages,
        false,
        false,
        false,
        quiet,
        false,
    )
}

/// Extract dependencies from a setup.py file using regex-based heuristic parsing.
///
/// This avoids executing the setup.py (which could have side effects) and instead
/// looks for `install_requires=[...]` patterns in the source code.
fn install_from_setup_py(
    path: &std::path::Path,
    site_packages: &str,
    quiet: bool,
) -> Result<(), String> {
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
                let var_name = after_eq
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
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
