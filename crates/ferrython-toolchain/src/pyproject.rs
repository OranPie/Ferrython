//! pyproject.toml parsing — PEP 517/518/621 support.

use serde::Deserialize;
use std::path::Path;

/// Parsed pyproject.toml data.
#[derive(Debug, Clone, Default)]
pub struct PyProject {
    /// [build-system] table
    pub build_system: Option<BuildSystem>,
    /// [project] table (PEP 621)
    pub project: Option<ProjectMetadata>,
    /// [tool] table (opaque, for setuptools/flit/etc.)
    pub tool: Option<toml::Value>,
}

/// [build-system] from PEP 517/518.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BuildSystem {
    pub requires: Option<Vec<String>>,
    pub build_backend: Option<String>,
    pub backend_path: Option<Vec<String>>,
}

/// [project] table from PEP 621.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub readme: Option<toml::Value>,
    pub license: Option<toml::Value>,
    pub requires_python: Option<String>,
    pub authors: Option<Vec<PersonEntry>>,
    pub maintainers: Option<Vec<PersonEntry>>,
    pub keywords: Option<Vec<String>>,
    pub classifiers: Option<Vec<String>>,
    pub urls: Option<toml::value::Table>,
    pub dependencies: Option<Vec<String>>,
    pub optional_dependencies: Option<toml::value::Table>,
    pub scripts: Option<toml::value::Table>,
    pub gui_scripts: Option<toml::value::Table>,
    pub entry_points: Option<toml::value::Table>,
    pub dynamic: Option<Vec<String>>,
}

/// Author/maintainer entry.
#[derive(Debug, Clone, Deserialize)]
pub struct PersonEntry {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// Raw TOML structure for deserialization.
#[derive(Deserialize)]
struct RawPyProject {
    #[serde(rename = "build-system")]
    build_system: Option<BuildSystem>,
    project: Option<ProjectMetadata>,
    tool: Option<toml::Value>,
}

/// Parse a pyproject.toml file.
pub fn parse_pyproject(path: &Path) -> Result<PyProject, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    parse_pyproject_str(&content)
}

/// Parse pyproject.toml from a string.
pub fn parse_pyproject_str(content: &str) -> Result<PyProject, String> {
    let raw: RawPyProject = toml::from_str(content)
        .map_err(|e| format!("TOML parse error: {}", e))?;

    Ok(PyProject {
        build_system: raw.build_system,
        project: raw.project,
        tool: raw.tool,
    })
}

impl PyProject {
    /// Get the project name (normalized).
    pub fn name(&self) -> Option<String> {
        self.project.as_ref()?.name.as_ref().map(|n| {
            n.to_lowercase().replace('-', "_").replace('.', "_")
        })
    }

    /// Get the project version.
    pub fn version(&self) -> Option<&str> {
        self.project.as_ref()?.version.as_deref()
    }

    /// Get the list of dependencies.
    pub fn dependencies(&self) -> Vec<String> {
        self.project.as_ref()
            .and_then(|p| p.dependencies.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Get build-system requirements.
    pub fn build_requires(&self) -> Vec<String> {
        self.build_system.as_ref()
            .and_then(|bs| bs.requires.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Get the build backend (e.g., "setuptools.build_meta").
    pub fn build_backend(&self) -> Option<&str> {
        self.build_system.as_ref()?.build_backend.as_deref()
    }

    /// Get console_scripts entry points.
    pub fn scripts(&self) -> Vec<(String, String)> {
        self.project.as_ref()
            .and_then(|p| p.scripts.as_ref())
            .map(|table| {
                table.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get optional dependency group names.
    pub fn extras(&self) -> Vec<String> {
        self.project.as_ref()
            .and_then(|p| p.optional_dependencies.as_ref())
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get dependencies for a specific optional group.
    pub fn extra_deps(&self, group: &str) -> Vec<String> {
        self.project.as_ref()
            .and_then(|p| p.optional_dependencies.as_ref())
            .and_then(|table| table.get(group))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}
