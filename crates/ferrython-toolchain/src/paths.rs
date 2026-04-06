//! Ferrython installation paths — runtime discovery of stdlib, site-packages, and prefix.
//!
//! All toolchain components share these path computations to ensure consistency.

use std::path::{Path, PathBuf};

/// Layout of a Ferrython installation (or venv).
#[derive(Debug, Clone)]
pub struct InstallLayout {
    /// Top-level prefix (e.g., `/usr/local` or `/home/user/.venvs/myenv`)
    pub prefix: PathBuf,
    /// The base prefix (real Python install, not the venv)
    pub base_prefix: PathBuf,
    /// Directory containing the `ferrython` binary
    pub bin_dir: PathBuf,
    /// Pure Python library path (purelib)
    pub lib_dir: PathBuf,
    /// Platform-specific library path (platlib, same as lib_dir for us)
    pub plat_lib_dir: PathBuf,
    /// site-packages directory
    pub site_packages: PathBuf,
    /// Include directory
    pub include_dir: PathBuf,
    /// stdlib/Lib directory (pure-Python standard library)
    pub stdlib_dir: Option<PathBuf>,
}

impl InstallLayout {
    /// Discover layout from the running ferrython binary location.
    pub fn discover() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferrython"));
        let bin_dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();

        // Walk up from binary to find the prefix.
        // Typical layouts:
        //   prefix/bin/ferrython        → prefix is 1 up
        //   target/release/ferrython    → prefix is project root (3 up for cargo)
        let prefix = find_prefix(&bin_dir);
        let base_prefix = prefix.clone();

        let lib_dir = prefix.join("lib").join("ferrython");
        let site_packages = lib_dir.join("site-packages");
        let include_dir = prefix.join("include").join("ferrython");

        // Find stdlib/Lib relative to the binary or prefix
        let stdlib_dir = find_stdlib(&bin_dir, &prefix);

        Self {
            prefix,
            base_prefix,
            bin_dir,
            lib_dir: lib_dir.clone(),
            plat_lib_dir: lib_dir,
            site_packages,
            include_dir,
            stdlib_dir,
        }
    }

    /// Create a layout for a virtual environment at `venv_dir`,
    /// inheriting base paths from the host installation.
    pub fn for_venv(venv_dir: &Path, host: &InstallLayout) -> Self {
        let bin_dir = if cfg!(windows) {
            venv_dir.join("Scripts")
        } else {
            venv_dir.join("bin")
        };
        let lib_dir = venv_dir.join("lib").join("ferrython");
        let site_packages = lib_dir.join("site-packages");
        let include_dir = venv_dir.join("include").join("ferrython");

        Self {
            prefix: venv_dir.to_path_buf(),
            base_prefix: host.base_prefix.clone(),
            bin_dir,
            lib_dir: lib_dir.clone(),
            plat_lib_dir: lib_dir,
            site_packages,
            include_dir,
            stdlib_dir: host.stdlib_dir.clone(),
        }
    }

    /// Get a sysconfig-style path by name.
    pub fn get_path(&self, name: &str) -> Option<PathBuf> {
        match name {
            "stdlib" => self.stdlib_dir.clone().or_else(|| Some(self.lib_dir.clone())),
            "purelib" | "platlib" => Some(self.site_packages.clone()),
            "scripts" => Some(self.bin_dir.clone()),
            "include" => Some(self.include_dir.clone()),
            "data" => Some(self.prefix.clone()),
            _ => None,
        }
    }

    /// Get a sysconfig config variable by name.
    pub fn get_config_var(&self, name: &str) -> Option<String> {
        match name {
            "prefix" | "exec_prefix" => Some(self.prefix.to_string_lossy().to_string()),
            "base_prefix" | "base_exec_prefix" => Some(self.base_prefix.to_string_lossy().to_string()),
            "BINDIR" => Some(self.bin_dir.to_string_lossy().to_string()),
            "installed_base" => Some(self.prefix.to_string_lossy().to_string()),
            "py_version_short" => Some("3.11".to_string()),
            "SOABI" => Some(format!("ferrython-{}", env!("CARGO_PKG_VERSION"))),
            "EXT_SUFFIX" => Some(".so".to_string()),
            "SIZEOF_VOID_P" => Some(std::mem::size_of::<usize>().to_string()),
            "Py_ENABLE_SHARED" => Some("0".to_string()),
            _ => None,
        }
    }
}

/// Walk up from `bin_dir` to find a prefix directory.
fn find_prefix(bin_dir: &Path) -> PathBuf {
    // If bin_dir ends with "bin", prefix is the parent
    if bin_dir.ends_with("bin") || bin_dir.ends_with("Scripts") {
        return bin_dir.parent().unwrap_or(bin_dir).to_path_buf();
    }
    // For cargo builds: target/release/ or target/debug/ — go up to project root
    if let Some(parent) = bin_dir.parent() {
        if parent.ends_with("target") {
            return parent.parent().unwrap_or(parent).to_path_buf();
        }
    }
    // Fallback: use the binary directory itself
    bin_dir.to_path_buf()
}

/// Find the stdlib/Lib directory by searching up from binary and prefix.
fn find_stdlib(bin_dir: &Path, prefix: &Path) -> Option<PathBuf> {
    // Check relative to prefix
    let candidates = [
        prefix.join("stdlib/Lib"),
        prefix.join("lib/ferrython/stdlib"),
        bin_dir.join("../stdlib/Lib"),
    ];
    for c in &candidates {
        if let Ok(canon) = c.canonicalize() {
            if canon.is_dir() {
                return Some(canon);
            }
        }
    }
    // Walk up from binary (handles nested cargo target dirs)
    let mut dir = Some(bin_dir.to_path_buf());
    for _ in 0..6 {
        if let Some(ref d) = dir {
            let candidate = d.join("stdlib/Lib");
            if let Ok(canon) = candidate.canonicalize() {
                if canon.is_dir() {
                    return Some(canon);
                }
            }
            dir = d.parent().map(|p| p.to_path_buf());
        } else {
            break;
        }
    }
    None
}
