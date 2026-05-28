use super::*;

// ── importlib.metadata ──────────────────────────────────────────────────
// Provides metadata about installed packages (version, name, etc.)

pub fn create_importlib_metadata_module() -> PyObjectRef {
    make_module(
        "importlib.metadata",
        vec![
            ("version", make_builtin(metadata_version)),
            ("metadata", make_builtin(metadata_metadata)),
            (
                "packages_distributions",
                make_builtin(metadata_packages_distributions),
            ),
            ("requires", make_builtin(metadata_requires)),
            ("distributions", make_builtin(metadata_distributions)),
            ("entry_points", make_builtin(metadata_entry_points)),
            ("files", make_builtin(metadata_files)),
            (
                "PackageNotFoundError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::ModuleNotFoundError),
            ),
        ],
    )
}

/// Read installed package metadata from dist-info directories.
/// Searches site-packages using the toolchain's discovered layout and binary-relative paths.
fn find_dist_info(package_name: &str) -> Option<std::path::PathBuf> {
    let normalized = package_name.to_lowercase().replace('-', "_");
    let layout = ferrython_toolchain::paths::InstallLayout::discover();

    let home = std::env::var("HOME").unwrap_or_default();
    let mut search_paths = vec![
        layout.site_packages.clone(),
        std::path::PathBuf::from(format!("{}/.local/lib/ferrython/site-packages", home)),
    ];

    // Search relative to the binary (target/release/lib/ferrython/site-packages)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            search_paths.push(exe_dir.join("lib").join("ferrython").join("site-packages"));
        }
    }

    // Also check cwd-relative site-packages for development
    if let Ok(cwd) = std::env::current_dir() {
        let local_site = cwd.join("lib").join("ferrython").join("site-packages");
        if local_site.is_dir() {
            search_paths.push(local_site);
        }
    }

    // Add system Python dist-packages as fallback
    search_paths.push(std::path::PathBuf::from("/usr/lib/python3/dist-packages"));

    for base in &search_paths {
        if !base.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".dist-info") {
                    let dist_name = name
                        .trim_end_matches(".dist-info")
                        .split('-')
                        .next()
                        .unwrap_or("")
                        .to_lowercase()
                        .replace('-', "_");
                    if dist_name == normalized {
                        return Some(entry.path());
                    }
                }
            }
        }
    }
    None
}

fn parse_metadata_file(path: &std::path::Path) -> IndexMap<CompactString, CompactString> {
    let mut result = IndexMap::new();
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            if line.is_empty() {
                break;
            } // stop at body separator
            if let Some((key, value)) = line.split_once(": ") {
                let k = CompactString::from(key.trim());
                let v = CompactString::from(value.trim());
                // For multi-value keys, join with newline
                if let Some(existing) = result.get(&k) {
                    let joined = CompactString::from(format!("{}\n{}", existing, v));
                    result.insert(k, joined);
                } else {
                    result.insert(k, v);
                }
            }
        }
    }
    result
}

fn metadata_version(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.version", args, 1)?;
    let name = args[0]
        .as_str()
        .ok_or_else(|| PyException::type_error("version() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let meta = parse_metadata_file(&metadata_path);
        if let Some(version) = meta.get("Version") {
            return Ok(PyObject::str_val(version.clone()));
        }
    }
    Err(PyException::runtime_error(format!(
        "No package metadata found for '{}'",
        name
    )))
}

fn metadata_metadata(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.metadata", args, 1)?;
    let name = args[0]
        .as_str()
        .ok_or_else(|| PyException::type_error("metadata() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let meta = parse_metadata_file(&metadata_path);
        let mut dict_map = IndexMap::new();
        for (k, v) in &meta {
            dict_map.insert(
                HashableKey::str_key(k.clone()),
                PyObject::str_val(v.clone()),
            );
            // Also store lowercase key for case-insensitive access
            // (CPython's metadata returns email.Message which is case-insensitive)
            let lower = CompactString::from(k.to_lowercase());
            if lower != *k {
                dict_map.insert(HashableKey::str_key(lower), PyObject::str_val(v.clone()));
            }
        }
        return Ok(PyObject::dict(dict_map));
    }
    Err(PyException::runtime_error(format!(
        "No package metadata found for '{}'",
        name
    )))
}

fn metadata_packages_distributions(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict(IndexMap::new()))
}

fn metadata_requires(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.requires", args, 1)?;
    let name = args[0]
        .as_str()
        .ok_or_else(|| PyException::type_error("requires() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let _meta = parse_metadata_file(&metadata_path);
        let mut requires = Vec::new();
        // Collect all "Requires-Dist" entries
        if let Ok(content) = std::fs::read_to_string(&metadata_path) {
            for line in content.lines() {
                if line.starts_with("Requires-Dist: ") {
                    let req = line.trim_start_matches("Requires-Dist: ");
                    requires.push(PyObject::str_val(CompactString::from(req)));
                }
            }
        }
        if requires.is_empty() {
            return Ok(PyObject::none());
        }
        return Ok(PyObject::list(requires));
    }
    Ok(PyObject::none())
}

/// List all installed distributions (packages) by scanning site-packages.
fn metadata_distributions(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();
    let home = std::env::var("HOME").unwrap_or_default();
    let mut search_paths = vec![
        layout.site_packages.clone(),
        std::path::PathBuf::from(format!("{}/.local/lib/ferrython/site-packages", home)),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let local_site = cwd.join("lib").join("ferrython").join("site-packages");
        if local_site.is_dir() {
            search_paths.push(local_site);
        }
    }

    let mut distributions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for base in &search_paths {
        if !base.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".dist-info") {
                    let dist_name = name
                        .trim_end_matches(".dist-info")
                        .split('-')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !dist_name.is_empty() && seen.insert(dist_name.clone()) {
                        let metadata_path = entry.path().join("METADATA");
                        let meta = parse_metadata_file(&metadata_path);
                        let mut attrs = IndexMap::new();
                        attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(
                                meta.get("Name").map(|s| s.as_str()).unwrap_or(&dist_name),
                            )),
                        );
                        attrs.insert(
                            CompactString::from("version"),
                            PyObject::str_val(CompactString::from(
                                meta.get("Version").map(|s| s.as_str()).unwrap_or("0.0.0"),
                            )),
                        );
                        attrs.insert(
                            CompactString::from("_path"),
                            PyObject::str_val(CompactString::from(
                                entry.path().to_string_lossy().as_ref(),
                            )),
                        );
                        let cls = PyObject::class(
                            CompactString::from("Distribution"),
                            vec![],
                            IndexMap::new(),
                        );
                        distributions.push(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
            }
        }
    }
    Ok(PyObject::list(distributions))
}

fn metadata_entry_points(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::list(vec![]))
}

fn metadata_files(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.files", args, 1)?;
    let name = args[0]
        .as_str()
        .ok_or_else(|| PyException::type_error("files() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let record_path = dist_info.join("RECORD");
        if let Ok(content) = std::fs::read_to_string(&record_path) {
            let files: Vec<PyObjectRef> = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let file_path = l.split(',').next().unwrap_or(l);
                    PyObject::str_val(CompactString::from(file_path.trim()))
                })
                .collect();
            return Ok(PyObject::list(files));
        }
    }
    Ok(PyObject::none())
}
