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

    for sep in &["==", ">=", "<=", "!=", "~=", ">", "<"] {
        if let Some(pos) = spec_no_extras.find(sep) {
            let name = spec_no_extras[..pos].trim().to_lowercase();
            let version = spec_no_extras[pos + sep.len()..].trim();
            if *sep == "==" {
                return (name, Some(version.to_string()));
            } else {
                // For non-exact specifiers, we'll fetch the latest compatible version
                return (name, None);
            }
        }
    }
    (spec_no_extras.trim().to_lowercase(), None)
}

/// Fetch package metadata from PyPI JSON API
pub fn fetch_package_info(name: &str, version: Option<&str>) -> Result<ReleaseInfo, String> {
    let url = if let Some(v) = version {
        format!("https://pypi.org/pypi/{}/{}/json", name, v)
    } else {
        format!("https://pypi.org/pypi/{}/json", name)
    };

    let client = reqwest::blocking::Client::builder()
        .user_agent("ferryip/0.1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client.get(&url).send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Package '{}' not found on PyPI (HTTP {})", name, resp.status()));
    }

    let data: PyPIResponse = resp.json()
        .map_err(|e| format!("JSON parse error: {}", e))?;

    // Find best wheel URL: prefer pure-python wheel (py3-none-any), then any wheel, then sdist
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
    })
}

/// Select the best download URL: pure-python wheel > any wheel > sdist
fn find_best_download(urls: &[PyPIUrl]) -> Option<&PyPIUrl> {
    // First: pure-python wheel (py3-none-any or py2.py3-none-any)
    let pure_wheel = urls.iter().find(|u| {
        u.packagetype == "bdist_wheel" &&
        (u.filename.contains("-py3-none-any") || u.filename.contains("-py2.py3-none-any"))
    });
    if pure_wheel.is_some() { return pure_wheel; }

    // Second: any wheel
    let any_wheel = urls.iter().find(|u| u.packagetype == "bdist_wheel");
    if any_wheel.is_some() { return any_wheel; }

    // Third: sdist (.tar.gz)
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

    let client = reqwest::blocking::Client::builder()
        .user_agent("ferryip/0.1.0")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client.get(&release.url).send()
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
    // PyPI disabled XMLRPC search. Use simple approach: fetch JSON for exact name
    let url = format!("https://pypi.org/pypi/{}/json", query);
    let client = reqwest::blocking::Client::builder()
        .user_agent("ferryip/0.1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

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
