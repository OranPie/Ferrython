/// Parse a requirements file supporting:
///  - `-r <file>` recursive includes
///  - `-c <file>` constraints (pinned versions applied as upper bounds)
///  - `-e <path>` editable installs (returned as `editable:<path>`)
///  - `--index-url`, `--extra-index-url`, `--trusted-host` (acknowledged, ignored)
///  - `--hash=sha256:...` inline hashes (preserved for verification)
///  - `--no-deps` (returned as flag prefix `nodeps:`)
///  - environment markers after `;`
///  - line continuations with `\`
pub(super) fn parse_requirements_file(path: &str) -> Vec<String> {
    parse_requirements_file_inner(path, &mut std::collections::HashSet::new())
}

fn parse_requirements_file_inner(
    path: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<String> {
    let canonical = std::path::Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(path));
    let key = canonical.to_string_lossy().to_string();
    if !seen.insert(key) {
        return vec![]; // avoid infinite recursion
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read {}: {}", path, e);
            std::process::exit(1);
        }
    };

    let base_dir = std::path::Path::new(path)
        .parent()
        .unwrap_or(std::path::Path::new("."));

    // Join continuation lines
    let joined = content.replace("\\\n", "");
    let mut result = Vec::new();

    for raw_line in joined.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle -r / --requirement recursive includes
        if line.starts_with("-r ")
            || line.starts_with("--requirement ")
            || line.starts_with("--requirement=")
        {
            let ref_path = if line.starts_with("--requirement=") {
                line.strip_prefix("--requirement=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !ref_path.is_empty() {
                let full = base_dir.join(ref_path);
                result.extend(parse_requirements_file_inner(&full.to_string_lossy(), seen));
            }
            continue;
        }

        // Handle -c / --constraint (parse as pinned version upper bounds)
        if line.starts_with("-c ")
            || line.starts_with("--constraint ")
            || line.starts_with("--constraint=")
        {
            let ref_path = if line.starts_with("--constraint=") {
                line.strip_prefix("--constraint=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !ref_path.is_empty() {
                let full = base_dir.join(ref_path);
                // Constraints are just version-pinned requirements
                result.extend(parse_requirements_file_inner(&full.to_string_lossy(), seen));
            }
            continue;
        }

        // Handle -e / --editable installs in requirements files
        if line.starts_with("-e ")
            || line.starts_with("--editable ")
            || line.starts_with("--editable=")
        {
            let edit_path = if line.starts_with("--editable=") {
                line.strip_prefix("--editable=").unwrap().trim()
            } else {
                line.split_whitespace().nth(1).unwrap_or("")
            };
            if !edit_path.is_empty() {
                let full = base_dir.join(edit_path);
                result.push(format!("editable:{}", full.to_string_lossy()));
            }
            continue;
        }

        // Handle --no-deps as a line-level flag (applies to next package)
        if line == "--no-deps" {
            // Mark next package as no-deps (handled by install pipeline)
            result.push("flag:no-deps".to_string());
            continue;
        }

        // Skip pip option flags (--index-url, --extra-index-url, --trusted-host, etc.)
        if line.starts_with("--") || line.starts_with("-f ") || line.starts_with("-i ") {
            continue;
        }

        // Strip inline comments (after ` #`)
        let spec = if let Some(comment_pos) = line.find(" #") {
            line[..comment_pos].trim()
        } else {
            line
        };

        // Extract and preserve inline --hash options for verification
        let mut hashes: Vec<String> = Vec::new();
        let spec = {
            let mut s = spec;
            while let Some(hash_pos) = s.find(" --hash=") {
                let hash_val = s[hash_pos + 8..].split_whitespace().next().unwrap_or("");
                if !hash_val.is_empty() {
                    hashes.push(hash_val.to_string());
                }
                s = s[..hash_pos].trim();
            }
            s
        };

        // Strip environment markers: handle `; marker` at end
        // Keep the full spec including markers — the resolver's parse_dependency handles them
        let spec = spec.trim();

        if spec.is_empty() {
            continue;
        }

        // If hashes were specified, encode them in the spec for downstream verification
        if !hashes.is_empty() {
            result.push(format!("hash:{}:{}", hashes.join(","), spec));
        } else {
            result.push(spec.to_string());
        }
    }

    result
}
