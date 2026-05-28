use crate::{pypi, registry};

use super::cache::cache_dir;
use super::install::parse_version_specifier;

pub(super) fn show_config(site_packages: &str, _list: bool) -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_default();
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    println!("ferrypip version: {}", env!("CARGO_PKG_VERSION"));
    println!("Ferrython compatible: 3.8+");
    println!("Location: {}", exe.display());
    println!("Site-packages: {}", site_packages);
    println!("Cache directory: {}", cache_dir().display());
    println!(
        "Python platform: {}",
        if os == "linux" {
            "linux"
        } else if os == "macos" {
            "darwin"
        } else if os == "windows" {
            "win32"
        } else {
            "unknown"
        }
    );
    println!("Architecture: {}", arch);

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
            let plat = if arch == "x86_64" {
                "win_amd64"
            } else {
                "win32"
            };
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

pub(super) fn inspect_packages(site_packages: &str) -> Result<(), String> {
    let packages = registry::list_installed(site_packages);
    println!("{{");
    println!("  \"version\": \"1\",");
    println!(
        "  \"pip_version\": \"ferrypip-{}\",",
        env!("CARGO_PKG_VERSION")
    );
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
            let req_json: Vec<String> = requires
                .iter()
                .map(|r| format!("\"{}\"", r.replace('"', "\\\"")))
                .collect();
            println!("        \"requires_dist\": [{}],", req_json.join(", "));
        }
        println!("        \"installer\": \"ferrypip\"");
        println!("      }}");
        println!("    }}{}", comma);
    }
    println!("  ]");
    println!("}}");
    Ok(())
}

pub(super) fn generate_lock_file(
    site_packages: &str,
    output_file: &str,
    requirement_file: Option<&str>,
) -> Result<(), String> {
    use std::io::Write;

    let mut locked_packages: Vec<(String, String, Option<String>)> = Vec::new();

    if let Some(req_file) = requirement_file {
        let content = std::fs::read_to_string(req_file)
            .map_err(|e| format!("Cannot read {}: {}", req_file, e))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, _spec) = parse_version_specifier(line);
            match pypi::fetch_package_info(&name, None) {
                Ok(info) => {
                    let version = info.version.clone();
                    let hash = info.sha256.clone();
                    locked_packages.push((name.to_string(), version, hash));
                    for dep in &info.requires_dist {
                        let dep_name = dep.split_whitespace().next().unwrap_or(dep);
                        let dep_name = dep_name
                            .split(&['>', '<', '=', '!', '~', ';'][..])
                            .next()
                            .unwrap_or(dep_name)
                            .trim();
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
        let packages = registry::list_installed(site_packages);
        for pkg in &packages {
            let hash = match pypi::fetch_package_info(&pkg.name, Some(&pkg.version)) {
                Ok(info) if info.version == pkg.version => info.sha256,
                _ => None,
            };
            locked_packages.push((pkg.name.clone(), pkg.version.clone(), hash));
        }
    }

    locked_packages.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut file = std::fs::File::create(output_file)
        .map_err(|e| format!("Cannot create {}: {}", output_file, e))?;

    writeln!(file, "# This file is @generated by ferrypip lock.").map_err(|e| e.to_string())?;
    writeln!(file, "# Do not edit manually.").map_err(|e| e.to_string())?;
    writeln!(file, "#").map_err(|e| e.to_string())?;

    for (name, version, hash) in &locked_packages {
        if let Some(h) = hash {
            writeln!(file, "{}=={} --hash=sha256:{}", name, version, h)
                .map_err(|e| e.to_string())?;
        } else {
            writeln!(file, "{}=={}", name, version).map_err(|e| e.to_string())?;
        }
    }

    println!(
        "Locked {} packages to {}",
        locked_packages.len(),
        output_file
    );
    Ok(())
}
