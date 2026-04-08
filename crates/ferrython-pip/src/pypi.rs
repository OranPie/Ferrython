//! PyPI API client — fetches package metadata and downloads wheels/sdists

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Parsed release info from PyPI
#[allow(dead_code)]
pub struct ReleaseInfo {
    pub name: String,
    pub version: String,
    pub url: String,
    pub filename: String,
    pub sha256: Option<String>,
    pub requires_dist: Vec<String>,
    pub summary: String,
    pub author: String,
    pub license: String,
    pub yanked: bool,
    pub yanked_reason: Option<String>,
}

/// PyPI JSON API response
#[derive(Deserialize)]
struct PyPIResponse {
    info: PyPIInfo,
    #[allow(dead_code)]
    releases: Option<serde_json::Value>,
    urls: Vec<PyPIUrl>,
}

#[derive(Deserialize)]
struct PyPIInfo {
    name: String,
    version: String,
    summary: Option<String>,
    author: Option<String>,
    license: Option<String>,
    requires_dist: Option<Vec<String>>,
    yanked: Option<bool>,
    yanked_reason: Option<String>,
}

#[derive(Deserialize)]
struct PyPIUrl {
    filename: String,
    url: String,
    packagetype: String,
    digests: Option<PyPIDigests>,
    #[allow(dead_code)]
    requires_python: Option<String>,
}

#[derive(Deserialize)]
struct PyPIDigests {
    sha256: Option<String>,
}

/// Parse a requirement specifier like "requests>=2.28" or "flask==2.0.1"
pub fn parse_requirement(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();
    // Handle extras: package[extra]>=version
    let spec_no_extras = if let Some(bracket) = spec.find('[') {
        if let Some(end) = spec.find(']') {
            format!("{}{}", &spec[..bracket], &spec[end+1..])
        } else {
            spec.to_string()
        }
    } else {
        spec.to_string()
    };

    // Find the earliest version operator to correctly split name from spec
    let mut earliest_pos = None;
    let mut earliest_sep = "";
    for sep in &[">=", "<=", "!=", "~=", "==", ">", "<"] {
        if let Some(pos) = spec_no_extras.find(sep) {
            if earliest_pos.is_none() || pos < earliest_pos.unwrap() {
                earliest_pos = Some(pos);
                earliest_sep = sep;
            }
        }
    }
    if let Some(pos) = earliest_pos {
        let name = spec_no_extras[..pos].trim().to_lowercase();
        if earliest_sep == "==" {
            let version = spec_no_extras[pos + 2..].trim();
            // Strip trailing comma-separated specs for exact pin
            let version = version.split(',').next().unwrap_or(version);
            return (name, Some(version.to_string()));
        } else {
            return (name, None);
        }
    }
    (spec_no_extras.trim().to_lowercase(), None)
}

/// Fetch package metadata from PyPI JSON API.
///
/// When `version` is `Some`, fetches that exact version.
/// Otherwise fetches the project root and picks the best version that satisfies
/// optional version specifiers embedded in the caller's context.
pub fn fetch_package_info(name: &str, version: Option<&str>) -> Result<ReleaseInfo, String> {
    let url = if let Some(v) = version {
        format!("https://pypi.org/pypi/{}/{}/json", name, v)
    } else {
        format!("https://pypi.org/pypi/{}/json", name)
    };

    let client = make_client()?;
    let resp = get_with_retry(&client, &url)?;

    if !resp.status().is_success() {
        return Err(format_http_error(name, resp.status().as_u16()));
    }

    let data: PyPIResponse = resp.json()
        .map_err(|e| format!("JSON parse error: {}", e))?;

    // Find best wheel URL using PEP 425 tag matching; falls back to sdist
    let best_url = find_best_download(&data.urls)
        .ok_or_else(|| format!("No compatible distribution found for {}", name))?;

    Ok(ReleaseInfo {
        name: data.info.name.clone(),
        version: data.info.version.clone(),
        url: best_url.url.clone(),
        filename: best_url.filename.clone(),
        sha256: best_url.digests.as_ref().and_then(|d| d.sha256.clone()),
        requires_dist: data.info.requires_dist.unwrap_or_default(),
        summary: data.info.summary.unwrap_or_default(),
        author: data.info.author.unwrap_or_default(),
        license: data.info.license.unwrap_or_default(),
        yanked: data.info.yanked.unwrap_or(false),
        yanked_reason: data.info.yanked_reason,
    })
}

/// Fetch the best version of a package that satisfies version specifiers.
///
/// This fetches the project page (all releases), filters by the given specs,
/// and returns the info for the highest compatible version.
/// Skips yanked versions unless the spec is an exact pin (==X.Y.Z).
pub fn fetch_best_version(name: &str, specs: &str) -> Result<ReleaseInfo, String> {
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let client = make_client()?;
    let resp = get_with_retry(&client, &url)?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(format!("HTTP {} error fetching package '{}'", status.as_u16(), name));
    }

    let data: PyPIResponse = resp.json()
        .map_err(|e| format!("JSON parse error for '{}': {}", name, e))?;

    // If latest version satisfies and is not yanked, use it directly (common fast path)
    let is_yanked = data.info.yanked.unwrap_or(false);
    if crate::version::version_matches(&data.info.version, specs) && !is_yanked {
        let best_url = find_best_download(&data.urls)
            .ok_or_else(|| format!("No compatible distribution found for {}", name))?;
        return Ok(ReleaseInfo {
            name: data.info.name.clone(),
            version: data.info.version.clone(),
            url: best_url.url.clone(),
            filename: best_url.filename.clone(),
            sha256: best_url.digests.as_ref().and_then(|d| d.sha256.clone()),
            requires_dist: data.info.requires_dist.unwrap_or_default(),
            summary: data.info.summary.unwrap_or_default(),
            author: data.info.author.unwrap_or_default(),
            license: data.info.license.unwrap_or_default(),
            yanked: false,
            yanked_reason: None,
        });
    }

    // Latest doesn't match or is yanked — scan all releases for the best compatible version
    if let Some(ref releases) = data.releases {
        if let Some(releases_map) = releases.as_object() {
            // Filter out yanked versions: a release is yanked if all its files are yanked
            // or the release array is empty. We check for the "yanked" field on each file entry.
            let versions: Vec<&str> = releases_map.iter()
                .filter(|(_, files)| {
                    if let Some(files_arr) = files.as_array() {
                        if files_arr.is_empty() { return false; }
                        // Keep if at least one file is not yanked
                        files_arr.iter().any(|f| {
                            !f.get("yanked").and_then(|y| y.as_bool()).unwrap_or(false)
                        })
                    } else {
                        true
                    }
                })
                .map(|(ver, _)| ver.as_str())
                .collect();
            // Find best version matching specs
            if let Some(best_ver) = crate::version::find_best_version(&versions, specs) {
                return fetch_package_info(name, Some(best_ver));
            }
        }
    }

    Err(format!(
        "No version of {} satisfies {} (latest is {})",
        name, specs, data.info.version
    ))
}

fn make_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("ferryip/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))
}

/// Format a descriptive error message for PyPI HTTP errors.
fn format_http_error(name: &str, status: u16) -> String {
    match status {
        404 => {
            // Try common name variations to suggest alternatives
            let suggestions = suggest_pypi_alternatives(name);
            let mut msg = format!(
                "Package '{}' not found on PyPI (HTTP 404).\n\
                 Check the package name spelling.",
                name
            );
            if !suggestions.is_empty() {
                msg.push_str(&format!(
                    "\nDid you mean one of these?\n  {}",
                    suggestions.join("\n  ")
                ));
            }
            msg
        }
        403 => format!(
            "Access denied for '{}' on PyPI (HTTP 403). The package may be private or restricted.",
            name
        ),
        429 => "Rate limited by PyPI (HTTP 429). Please wait a moment and try again.".to_string(),
        500..=599 => format!(
            "PyPI server error (HTTP {}). Try again later.\n\
             Hint: Check https://status.python.org/ for PyPI service status.",
            status
        ),
        _ => format!("Failed to fetch '{}' from PyPI (HTTP {})", name, status),
    }
}

/// Try common name variations to suggest alternatives on PyPI.
fn suggest_pypi_alternatives(name: &str) -> Vec<String> {
    let variations: Vec<String> = vec![
        name.replace('_', "-"),
        name.replace('-', "_"),
        format!("python-{}", name),
        format!("py{}", name),
    ]
    .into_iter()
    .filter(|v| v.to_lowercase() != name.to_lowercase())
    .collect();

    let client = match make_client() {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut found = Vec::new();
    for variant in &variations {
        let url = format!("https://pypi.org/pypi/{}/json", variant);
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<PyPIResponse>() {
                    found.push(format!("{} ({})", data.info.name, data.info.version));
                    if found.len() >= 3 { break; }
                }
            }
        }
    }
    found
}

/// GET request with retry logic (exponential backoff, 3 attempts).
fn get_with_retry(client: &reqwest::blocking::Client, url: &str) -> Result<reqwest::blocking::Response, String> {
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1)));
        }
        match client.get(url).send() {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if e.is_timeout() {
                    last_err = format!(
                        "Connection timed out after 30s.\n\
                         URL: {}\n\
                         Hint: Check if you're behind a slow proxy or firewall.",
                        url
                    );
                    continue;
                }
                if e.is_connect() {
                    last_err = format!(
                        "Connection failed: {}\n\
                         Hint: Check your internet connection and DNS settings.",
                        e
                    );
                    continue;
                }
                return Err(format!(
                    "Network error: {}\n\
                     Hint: Check your internet connection and proxy settings.\n\
                     URL: {}",
                    e, url
                ));
            }
        }
    }
    Err(format!(
        "Network error after 3 retries: {}\n\
         Hint: Check your internet connection, DNS, or try again later.\n\
         If behind a corporate firewall, ensure HTTPS traffic to pypi.org is allowed.",
        last_err
    ))
}

// ---------------------------------------------------------------------------
// PEP 425 wheel tag matching
// ---------------------------------------------------------------------------

/// Tags parsed from a wheel filename: {name}-{ver}(-{build})?-{py}-{abi}-{plat}.whl
/// Each component may contain multiple tags separated by `.` (e.g. `py2.py3`).
#[derive(Debug)]
struct WheelTags {
    python: Vec<String>,
    abi: Vec<String>,
    platform: Vec<String>,
}

/// Parse the tag triple from a wheel filename.
/// Returns `None` for filenames that don't match the wheel naming convention.
fn parse_wheel_tags(filename: &str) -> Option<WheelTags> {
    let stem = filename.strip_suffix(".whl")?;
    // Format: {name}-{version}(-{build})?-{python}-{abi}-{platform}
    // Split from the right: the last three dash-separated segments are the tag triple.
    let parts: Vec<&str> = stem.rsplitn(4, '-').collect();
    if parts.len() < 3 {
        return None;
    }
    let platform_str = parts[0];
    let abi_str = parts[1];
    let python_str = parts[2];

    Some(WheelTags {
        python: python_str.split('.').map(|s| s.to_lowercase()).collect(),
        abi: abi_str.split('.').map(|s| s.to_lowercase()).collect(),
        platform: platform_str.split('.').map(|s| s.to_lowercase()).collect(),
    })
}

/// Build the set of compatible tags for the current platform.
fn compatible_tags() -> (Vec<String>, Vec<String>, Vec<String>) {
    let python_tags = vec![
        "py3".to_string(),
        "cp312".to_string(), "py312".to_string(),
        "cp311".to_string(), "py311".to_string(),
        "cp310".to_string(), "py310".to_string(),
        "cp39".to_string(), "py39".to_string(),
        "cp38".to_string(), "py38".to_string(),
        "py2.py3".to_string(),
    ];
    let abi_tags = vec![
        "none".to_string(),
        "abi3".to_string(),
        "cp312".to_string(),
        "cp311".to_string(),
        "cp310".to_string(),
    ];

    let mut platform_tags = vec!["any".to_string()];

    let arch = std::env::consts::ARCH; // e.g. "x86_64", "aarch64"
    let os = std::env::consts::OS;     // e.g. "linux", "macos", "windows"

    match os {
        "linux" => {
            platform_tags.push(format!("linux_{}", arch));
            // Common manylinux generations
            for ml in &[
                "manylinux_2_17", "manylinux_2_28", "manylinux_2_34",
                "manylinux2014", "manylinux2010", "manylinux1",
            ] {
                platform_tags.push(format!("{}_{}", ml, arch));
            }
        }
        "macos" => {
            let mac_arch = if arch == "aarch64" { "arm64" } else { arch };
            platform_tags.push(format!("macosx_10_9_{}", mac_arch));
            platform_tags.push(format!("macosx_11_0_{}", mac_arch));
            platform_tags.push("macosx_10_9_universal2".to_string());
            platform_tags.push("macosx_11_0_universal2".to_string());
        }
        "windows" => {
            if arch == "x86_64" {
                platform_tags.push("win_amd64".to_string());
            } else if arch == "x86" {
                platform_tags.push("win32".to_string());
            } else {
                platform_tags.push(format!("win_{}", arch));
            }
        }
        _ => {}
    }

    (python_tags, abi_tags, platform_tags)
}

/// Check whether every tag in `wheel_values` has at least one match in `compatible`.
fn tags_intersect(wheel_values: &[String], compatible: &[String]) -> bool {
    wheel_values.iter().any(|wt| compatible.contains(wt))
}

/// Score a wheel for sorting: lower is better.
///   0 = pure-python `py3-none-any`
///   1 = `py2.py3-none-any` (still pure-python)
///   2 = platform-specific compatible wheel
fn wheel_priority(tags: &WheelTags) -> u8 {
    let is_any_platform = tags.platform.iter().any(|p| p == "any");
    let is_none_abi = tags.abi.iter().any(|a| a == "none");
    let has_py3 = tags.python.iter().any(|p| p == "py3");

    if is_any_platform && is_none_abi && has_py3 {
        0 // pure-python py3
    } else if is_any_platform && is_none_abi {
        1 // pure-python py2.py3
    } else {
        2 // platform-specific
    }
}

/// Select the best compatible download URL using PEP 425 tag matching.
/// Priority: compatible wheel (pure-python first, then platform-specific) > sdist.
fn find_best_download(urls: &[PyPIUrl]) -> Option<&PyPIUrl> {
    let (compat_py, compat_abi, compat_plat) = compatible_tags();

    // Collect compatible wheels with their priority
    let mut candidates: Vec<(u8, usize)> = Vec::new();
    for (idx, u) in urls.iter().enumerate() {
        if u.packagetype != "bdist_wheel" {
            continue;
        }
        if let Some(tags) = parse_wheel_tags(&u.filename) {
            if tags_intersect(&tags.python, &compat_py)
                && tags_intersect(&tags.abi, &compat_abi)
                && tags_intersect(&tags.platform, &compat_plat)
            {
                candidates.push((wheel_priority(&tags), idx));
            }
        }
    }

    // Sort by priority (lowest first), stable in original order for ties
    candidates.sort_by_key(|&(prio, idx)| (prio, idx));

    if let Some(&(_prio, idx)) = candidates.first() {
        return Some(&urls[idx]);
    }

    // Fallback: sdist (.tar.gz)
    urls.iter().find(|u| u.packagetype == "sdist")
}

/// Download a wheel/sdist, using local cache when available, verify SHA256
pub fn download_wheel(release: &ReleaseInfo) -> Result<PathBuf, String> {
    // Check cache first
    let cache_dir = wheel_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let cached = cache_dir.join(&release.filename);
    if cached.exists() {
        // Verify cached file integrity if hash is known
        if let Some(ref expected_hash) = release.sha256 {
            if verify_sha256(&cached, expected_hash) {
                return Ok(cached);
            }
            // Hash mismatch — re-download
            let _ = std::fs::remove_file(&cached);
        } else {
            return Ok(cached);
        }
    }

    let client = make_client()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = get_with_retry(&client, &release.url)
        .map_err(|e| format!("Download error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed (HTTP {})", resp.status()));
    }

    let bytes = resp.bytes()
        .map_err(|e| format!("Read error: {}", e))?;

    // Verify SHA256 if available
    if let Some(ref expected_hash) = release.sha256 {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual_hash = format!("{:x}", hasher.finalize());
        if &actual_hash != expected_hash {
            return Err(format!(
                "SHA256 mismatch for {}: expected {}, got {}",
                release.filename, expected_hash, actual_hash
            ));
        }
    }

    // Save to cache directory
    std::fs::write(&cached, &bytes)
        .map_err(|e| format!("Cache write error: {}", e))?;

    Ok(cached)
}

fn wheel_cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".cache")
        });
    base.join("ferryip").join("wheels")
}

fn verify_sha256(path: &Path, expected: &str) -> bool {
    use sha2::{Sha256, Digest};
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let actual = format!("{:x}", hasher.finalize());
    actual == expected
}

/// Search PyPI (uses simple API since XMLRPC search was disabled)
pub fn search(query: &str) -> Result<Vec<(String, String, String)>, String> {
    let url = format!("https://pypi.org/pypi/{}/json", query);
    let client = make_client()?;

    match client.get(&url).send() {
        Ok(resp) if resp.status().is_success() => {
            let data: PyPIResponse = resp.json()
                .map_err(|e| format!("JSON parse error: {}", e))?;
            Ok(vec![(
                data.info.name,
                data.info.version,
                data.info.summary.unwrap_or_default(),
            )])
        }
        _ => Ok(vec![]),
    }
}
