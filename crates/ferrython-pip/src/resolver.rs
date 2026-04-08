//! Dependency resolution — recursive install of package requirements.

use crate::{pypi, installer, registry, version};
use std::collections::{HashMap, HashSet};

/// Tracks resolved package versions for conflict detection.
struct ResolutionState {
    /// Map from normalized package name to (resolved version, who required it)
    resolved: HashMap<String, (String, String)>,
}

impl ResolutionState {
    fn new() -> Self {
        Self { resolved: HashMap::new() }
    }

    /// Record a resolved version; returns Err if conflicting with a previous resolution.
    fn record(&mut self, name: &str, version: &str, required_by: &str) -> Result<(), String> {
        let key = normalize(name);
        if let Some((prev_ver, prev_by)) = self.resolved.get(&key) {
            if prev_ver != version {
                return Err(format!(
                    "Dependency conflict: {} {} (required by {}) conflicts with {} (required by {})",
                    name, version, required_by, prev_ver, prev_by
                ));
            }
        }
        self.resolved.insert(key, (version.to_string(), required_by.to_string()));
        Ok(())
    }
}

fn normalize(name: &str) -> String {
    name.to_lowercase().replace('-', "_").replace('.', "_")
}

/// Install a package and all its transitive dependencies.
pub fn install_with_deps(
    name: &str,
    version_req: Option<&str>,
    site_packages: &str,
    upgrade: bool,
    no_deps: bool,
    quiet: bool,
    visited: &mut HashSet<String>,
) -> Result<(), String> {
    let mut state = ResolutionState::new();
    install_with_deps_inner(name, version_req, site_packages, upgrade, no_deps, quiet, visited, &mut state, "user")
}

fn install_with_deps_inner(
    name: &str,
    version_req: Option<&str>,
    site_packages: &str,
    upgrade: bool,
    no_deps: bool,
    quiet: bool,
    visited: &mut HashSet<String>,
    state: &mut ResolutionState,
    required_by: &str,
) -> Result<(), String> {
    let normalized = normalize(name);

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
                    state.record(name, &installed.version, required_by).ok();
                    return Ok(());
                }
                // Installed version doesn't match — need to upgrade
            } else {
                if !quiet {
                    println!("Requirement already satisfied: {} ({})", name, installed.version);
                }
                state.record(name, &installed.version, required_by).ok();
                return Ok(());
            }
        }
    }

    // Resolve the best version from PyPI
    let release = resolve_version(name, version_req)?;

    // Double-check that the resolved version satisfies the specs
    if let Some(spec) = version_req {
        if !version::version_matches(&release.version, spec) {
            return Err(format!(
                "ERROR: Could not find a version that satisfies the requirement {} (from versions: {})\n\
                 No matching distribution found for {} {}",
                spec, release.version, name, spec
            ));
        }
    }

    // Conflict detection
    state.record(name, &release.version, required_by)?;

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

    // Process dependencies (unless --no-deps)
    if !no_deps {
        let parent_name = release.name.clone();
        for dep_str in &release.requires_dist {
            if let Some((dep_name, dep_spec, dep_extras)) = parse_dependency(dep_str) {
                install_with_deps_inner(
                    &dep_name, dep_spec.as_deref(), site_packages,
                    false, false, quiet, visited, state, &parent_name,
                )?;
                // If the dependency itself has extras requested, install those too
                if !dep_extras.is_empty() {
                    install_extras_deps(&dep_name, &dep_extras, site_packages, quiet, visited, state, &parent_name)?;
                }
            }
        }
    }

    Ok(())
}

/// Install extra dependency groups for an already-installed package.
fn install_extras_deps(
    pkg_name: &str,
    extras: &[String],
    site_packages: &str,
    quiet: bool,
    visited: &mut HashSet<String>,
    state: &mut ResolutionState,
    required_by: &str,
) -> Result<(), String> {
    if let Some(info) = registry::get_installed(pkg_name, site_packages) {
        if let Some(ref requires) = info.requires {
            for req in requires {
                if let Some(semicolon) = req.find(';') {
                    let marker = req[semicolon + 1..].trim();
                    for extra in extras {
                        if marker.contains("extra") && marker.contains(extra) {
                            let dep_spec = req[..semicolon].trim();
                            if let Some((dep_name, dep_ver, _)) = parse_dependency_raw(dep_spec) {
                                install_with_deps_inner(
                                    &dep_name, dep_ver.as_deref(), site_packages,
                                    false, false, quiet, visited, state, required_by,
                                )?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Resolve the best version of a package from PyPI.
///
/// If an exact version is specified (==X.Y.Z), fetch it directly.
/// For range specifiers (>=, <, ~=, etc.), use fetch_best_version to scan all releases.
fn resolve_version(name: &str, version_req: Option<&str>) -> Result<pypi::ReleaseInfo, String> {
    match version_req {
        Some(spec) => {
            let trimmed = spec.trim();
            // Exact version pin: ==X.Y.Z (no wildcard, no comma)
            if trimmed.starts_with("==") && !trimmed.contains(',') && !trimmed.contains('*') {
                let exact = trimmed[2..].trim();
                pypi::fetch_package_info(name, Some(exact))
                    .map_err(|e| format!("Could not find {}=={}: {}", name, exact, e))
            } else {
                // Range specifier — try latest first (fast path), then scan all releases
                pypi::fetch_best_version(name, trimmed)
                    .map_err(|e| format!("Could not resolve {} {}: {}", name, trimmed, e))
            }
        }
        None => {
            // No version constraint — fetch latest
            pypi::fetch_package_info(name, None)
                .map_err(|e| format!("Could not find {}: {}", name, e))
        }
    }
}

/// Parse a Requires-Dist entry like "requests (>=2.20)" or "typing-extensions; python_version < '3.8'".
/// Returns (name, version_spec, extras).
fn parse_dependency(dep: &str) -> Option<(String, Option<String>, Vec<String>)> {
    let dep = dep.trim();

    // Evaluate environment markers (PEP 508)
    if let Some(semicolon) = dep.find(';') {
        let marker = dep[semicolon + 1..].trim();
        if !evaluate_marker(marker) {
            return None;
        }
        return parse_dependency(&dep[..semicolon]);
    }

    parse_dependency_raw(dep)
}

/// Parse a dependency spec without evaluating markers.
/// Returns (name, version_spec, extras).
fn parse_dependency_raw(dep: &str) -> Option<(String, Option<String>, Vec<String>)> {
    let dep = dep.trim();

    // Extract extras: name[extra1,extra2]
    let (dep_clean, extras) = extract_extras(dep);
    let dep = &dep_clean;

    // Handle version specifiers in parentheses: "requests (>=2.20,<3.0)"
    if let Some(paren_start) = dep.find('(') {
        if let Some(paren_end) = dep.find(')') {
            let name = dep[..paren_start].trim();
            let spec = dep[paren_start + 1..paren_end].trim();
            return Some((normalize(name), Some(spec.to_string()), extras));
        }
    }

    // Handle inline specifiers: "requests>=2.20" or "charset_normalizer<4,>=2"
    let mut earliest_pos = None;
    for op in &[">=", "<=", "!=", "~=", "==", ">", "<"] {
        if let Some(pos) = dep.find(op) {
            if earliest_pos.is_none() || pos < earliest_pos.unwrap() {
                earliest_pos = Some(pos);
            }
        }
    }
    if let Some(pos) = earliest_pos {
        let name = normalize(&dep[..pos]);
        let spec = dep[pos..].trim().to_string();
        return Some((name, Some(spec), extras));
    }
    // No version constraint — just a bare name
    Some((normalize(dep), None, extras))
}

/// Extract extras from a dependency name, e.g. "requests[security,socks]" → ("requests", ["security", "socks"])
fn extract_extras(dep: &str) -> (String, Vec<String>) {
    if let Some(bracket_start) = dep.find('[') {
        if let Some(bracket_end) = dep.find(']') {
            let extras_str = &dep[bracket_start + 1..bracket_end];
            let extras: Vec<String> = extras_str.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let clean = format!("{}{}", &dep[..bracket_start], &dep[bracket_end + 1..]);
            return (clean, extras);
        }
    }
    (dep.to_string(), vec![])
}

/// Evaluate a PEP 508 environment marker expression.
///
/// Supports `and`, `or`, comparison operators, and common marker variables:
/// `sys_platform`, `os_name`, `platform_system`, `platform_machine`,
/// `python_version`, `python_full_version`, `implementation_name`, `extra`.
fn evaluate_marker(marker: &str) -> bool {
    let marker = marker.trim();
    if marker.is_empty() { return true; }

    // Handle `or` (lowest precedence, split first)
    // Be careful not to split inside strings
    if let Some(parts) = split_marker_logic(marker, " or ") {
        return parts.iter().any(|p| evaluate_marker(p));
    }

    // Handle `and`
    if let Some(parts) = split_marker_logic(marker, " and ") {
        return parts.iter().all(|p| evaluate_marker(p));
    }

    // Handle parentheses
    let trimmed = marker.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        return evaluate_marker(&trimmed[1..trimmed.len()-1]);
    }

    // Single comparison: variable op value
    evaluate_marker_comparison(trimmed)
}

fn split_marker_logic<'a>(expr: &'a str, sep: &str) -> Option<Vec<&'a str>> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut in_string = false;
    let mut string_char = '"';
    let mut last_split = 0;
    let bytes = expr.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if in_string {
            if ch == string_char {
                in_string = false;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = true;
            string_char = ch;
        } else if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && expr[i..].starts_with(sep) {
            parts.push(expr[last_split..i].trim());
            last_split = i + sep.len();
            i = last_split;
            continue;
        }
        i += 1;
    }

    if parts.is_empty() {
        None
    } else {
        parts.push(expr[last_split..].trim());
        Some(parts)
    }
}

fn evaluate_marker_comparison(expr: &str) -> bool {
    // Try each comparison operator
    for (op_str, cmp_fn) in &[
        ("not in", marker_not_in as fn(&str, &str) -> bool),
        (" in ", marker_in as fn(&str, &str) -> bool),
        ("!=", marker_ne as fn(&str, &str) -> bool),
        ("==", marker_eq as fn(&str, &str) -> bool),
        (">=", marker_ge as fn(&str, &str) -> bool),
        ("<=", marker_le as fn(&str, &str) -> bool),
        (">", marker_gt as fn(&str, &str) -> bool),
        ("<", marker_lt as fn(&str, &str) -> bool),
        ("~=", marker_compat as fn(&str, &str) -> bool),
    ] {
        if let Some(pos) = expr.find(op_str) {
            let lhs = resolve_marker_var(expr[..pos].trim());
            let rhs = resolve_marker_var(expr[pos + op_str.len()..].trim());
            return cmp_fn(&lhs, &rhs);
        }
    }

    // Unknown expression — conservatively include the dependency
    true
}

fn resolve_marker_var(s: &str) -> String {
    let s = s.trim();
    // Strip quotes
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return s[1..s.len()-1].to_string();
    }
    // Resolve known environment variables
    match s {
        "sys_platform" => {
            if cfg!(target_os = "linux") { "linux".to_string() }
            else if cfg!(target_os = "macos") { "darwin".to_string() }
            else if cfg!(target_os = "windows") { "win32".to_string() }
            else { "unknown".to_string() }
        }
        "os_name" | "os.name" => {
            if cfg!(windows) { "nt".to_string() } else { "posix".to_string() }
        }
        "platform_system" => {
            if cfg!(target_os = "linux") { "Linux".to_string() }
            else if cfg!(target_os = "macos") { "Darwin".to_string() }
            else if cfg!(target_os = "windows") { "Windows".to_string() }
            else { "Unknown".to_string() }
        }
        "platform_machine" => {
            if cfg!(target_arch = "x86_64") { "x86_64".to_string() }
            else if cfg!(target_arch = "aarch64") { "aarch64".to_string() }
            else if cfg!(target_arch = "x86") { "i686".to_string() }
            else { "unknown".to_string() }
        }
        "platform_release" | "platform_version" => "".to_string(),
        "python_version" => "3.12".to_string(),
        "python_full_version" => "3.12.0".to_string(),
        "implementation_name" => "ferrython".to_string(),
        "implementation_version" => "0.1.0".to_string(),
        "extra" => "".to_string(),
        _ => s.to_string(),
    }
}

fn marker_eq(a: &str, b: &str) -> bool { a == b }
fn marker_ne(a: &str, b: &str) -> bool { a != b }
fn marker_in(a: &str, b: &str) -> bool { b.contains(a) }
fn marker_not_in(a: &str, b: &str) -> bool { !b.contains(a) }

fn marker_ge(a: &str, b: &str) -> bool {
    match (version::Version::parse(a), version::Version::parse(b)) {
        (Some(va), Some(vb)) => va >= vb,
        _ => a >= b,
    }
}
fn marker_le(a: &str, b: &str) -> bool {
    match (version::Version::parse(a), version::Version::parse(b)) {
        (Some(va), Some(vb)) => va <= vb,
        _ => a <= b,
    }
}
fn marker_gt(a: &str, b: &str) -> bool {
    match (version::Version::parse(a), version::Version::parse(b)) {
        (Some(va), Some(vb)) => va > vb,
        _ => a > b,
    }
}
fn marker_lt(a: &str, b: &str) -> bool {
    match (version::Version::parse(a), version::Version::parse(b)) {
        (Some(va), Some(vb)) => va < vb,
        _ => a < b,
    }
}
fn marker_compat(a: &str, b: &str) -> bool {
    // ~= for markers: treat as >= with prefix match
    marker_ge(a, b)
}
