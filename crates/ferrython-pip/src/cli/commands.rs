use crate::{pypi, registry};

use super::install::parse_version_specifier;
use super::output::{detail, status};
use super::search::find_closest_name;

pub(super) fn uninstall_packages(
    names: &[String],
    site_packages: &str,
    yes: bool,
    quiet: bool,
) -> Result<(), String> {
    if names.is_empty() {
        return Err(
            "You must give at least one package to uninstall (see 'ferrypip uninstall --help')"
                .to_string(),
        );
    }
    for name in names {
        let installed = registry::get_installed(name, site_packages);
        if installed.is_none() {
            // Try to suggest similar installed packages
            let all = registry::list_installed(site_packages);
            let suggestion = find_closest_name(
                name,
                &all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            );
            let hint = if let Some(ref similar) = suggestion {
                format!("\nDid you mean: {}?", similar)
            } else {
                String::new()
            };
            if !quiet {
                status(
                    "Skipped",
                    format!("{} is not installed.{}", name, hint),
                    quiet,
                );
            }
            continue;
        }
        let info = installed.unwrap();
        if !yes {
            status(
                "Found",
                format!("existing installation: {}-{}", info.name, info.version),
                quiet,
            );
            let file_count = info.files.len();
            detail(format!("Would remove {} file(s):", file_count), quiet);
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
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "n"
            {
                status("Skipped", name, quiet);
                continue;
            }
        }
        registry::uninstall(name, site_packages).map_err(|e| format!("Uninstall failed: {}", e))?;
        if !quiet {
            status("Removed", format!("{}-{}", info.name, info.version), quiet);
        }
    }
    Ok(())
}

pub(super) fn list_packages(
    site_packages: &str,
    outdated: bool,
    format: &str,
    not_required: bool,
    exclude_editable: bool,
) -> Result<(), String> {
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
        let all_deps: std::collections::HashSet<String> = packages
            .iter()
            .filter_map(|p| p.requires.as_ref())
            .flat_map(|reqs| reqs.iter())
            .map(|r| {
                let name = r.split_whitespace().next().unwrap_or(r);
                let name = name
                    .split(&['>', '<', '=', '!', '~', ';', '(', '['][..])
                    .next()
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
                println!(
                    "  {{\"name\": \"{}\", \"version\": \"{}\"}}{}",
                    pkg.name, pkg.version, comma
                );
            }
            println!("]");
        }
        _ => {
            // "columns" (default)
            if outdated {
                // Calculate dynamic column widths
                let name_width = packages
                    .iter()
                    .map(|p| p.name.len())
                    .max()
                    .unwrap_or(7)
                    .max(7);
                let ver_width = packages
                    .iter()
                    .map(|p| p.version.len())
                    .max()
                    .unwrap_or(7)
                    .max(7);
                println!(
                    "{:<name_w$} {:<ver_w$} {}",
                    "Package",
                    "Version",
                    "Latest",
                    name_w = name_width,
                    ver_w = ver_width
                );
                println!(
                    "{:<name_w$} {:<ver_w$} {}",
                    "-".repeat(name_width),
                    "-".repeat(ver_width),
                    "------",
                    name_w = name_width,
                    ver_w = ver_width
                );
                let mut outdated_count = 0;
                for pkg in &packages {
                    match pypi::fetch_package_info(&pkg.name, None) {
                        Ok(latest) => {
                            if latest.version != pkg.version {
                                println!(
                                    "{:<name_w$} {:<ver_w$} {}",
                                    pkg.name,
                                    pkg.version,
                                    latest.version,
                                    name_w = name_width,
                                    ver_w = ver_width
                                );
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
                let name_width = packages
                    .iter()
                    .map(|p| p.name.len())
                    .max()
                    .unwrap_or(7)
                    .max(7);
                println!("{:<width$} {}", "Package", "Version", width = name_width);
                println!(
                    "{:<width$} {}",
                    "-".repeat(name_width),
                    "-------",
                    width = name_width
                );
                for pkg in &packages {
                    println!("{:<width$} {}", pkg.name, pkg.version, width = name_width);
                }
                println!("\n[{} package(s) installed]", packages.len());
            }
        }
    }
    Ok(())
}

pub(super) fn download_packages(specs: &[String], dest: &str, quiet: bool) -> Result<(), String> {
    for spec in specs {
        let (name, version_req) = pypi::parse_requirement(spec);
        let release = pypi::fetch_package_info(&name, version_req.as_deref())
            .map_err(|e| format!("Could not find {}: {}", name, e))?;
        if !quiet {
            println!("Downloading {}-{}", release.name, release.version);
        }
        let wheel_path =
            pypi::download_wheel(&release).map_err(|e| format!("Download failed: {}", e))?;
        let dest_path = std::path::Path::new(dest).join(wheel_path.file_name().unwrap_or_default());
        std::fs::copy(&wheel_path, &dest_path).map_err(|e| format!("Copy failed: {}", e))?;
        if !quiet {
            println!("  Saved {}", dest_path.display());
        }
    }
    Ok(())
}

pub(super) fn freeze_packages(site_packages: &str, exclude_editable: bool) -> Result<(), String> {
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

pub(super) fn check_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    let mut has_errors = false;
    let mut checked = 0;

    for pkg in &packages {
        // Verify RECORD hash integrity
        let hash_failures = crate::installer::verify_installed_record(site_packages, &pkg.name);
        if !hash_failures.is_empty() {
            println!(
                "{} {} has {} file(s) with mismatched RECORD hashes:",
                pkg.name,
                pkg.version,
                hash_failures.len()
            );
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
                        println!(
                            "{} {} requires {}, which is not installed.",
                            pkg.name, pkg.version, req
                        );
                        has_errors = true;
                    }
                    Some(installed) => {
                        if let Some(ref spec) = req_spec {
                            if !crate::version::version_matches(&installed.version, spec) {
                                println!(
                                    "{} {} requires {} {}, but {} {} is installed.",
                                    pkg.name,
                                    pkg.version,
                                    req_name,
                                    spec,
                                    installed.name,
                                    installed.version
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
        println!(
            "No broken requirements found ({} packages checked, {} dependencies verified).",
            packages.len(),
            checked
        );
    }
    Ok(())
}
