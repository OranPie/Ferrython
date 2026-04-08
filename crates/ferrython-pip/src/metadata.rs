//! Package metadata — rich METADATA (PEP 566/643) generation from multiple sources.
//!
//! Builds complete `METADATA` content from pyproject.toml, setup.cfg, or PyPI data,
//! including Requires-Dist, Provides-Extra, classifiers, author fields, and project URLs.

/// All metadata fields that can appear in a PEP 566 / PEP 643 METADATA file.
#[derive(Debug, Clone, Default)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub description_content_type: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub maintainer: Option<String>,
    pub maintainer_email: Option<String>,
    pub license: Option<String>,
    pub home_page: Option<String>,
    pub requires_python: Option<String>,
    pub classifiers: Vec<String>,
    pub project_urls: Vec<(String, String)>,
    pub requires_dist: Vec<String>,
    pub provides_extra: Vec<String>,
}

impl PackageMetadata {
    /// Build metadata from a parsed pyproject.toml.
    pub fn from_pyproject(pyproj: &ferrython_toolchain::pyproject::PyProject) -> Self {
        let name = pyproj.name().unwrap_or_default();
        let version = pyproj.version().unwrap_or("0.0.0").to_string();

        let authors = pyproj.authors();
        let (author, author_email) = Self::merge_persons(&authors);

        let maintainers = pyproj
            .project
            .as_ref()
            .and_then(|p| p.maintainers.as_ref())
            .map(|ms| {
                ms.iter()
                    .map(|m| (m.name.clone(), m.email.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let (maintainer, maintainer_email) = Self::merge_persons(&maintainers);

        let license = pyproj
            .project
            .as_ref()
            .and_then(|p| p.license.as_ref())
            .and_then(|v| {
                // PEP 639: license can be a string or a table with "text" key
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.get("text").and_then(|t| t.as_str()).map(String::from))
            });

        let project_urls = pyproj.urls();

        let home_page = project_urls
            .iter()
            .find(|(k, _)| {
                let k = k.to_lowercase();
                k == "homepage" || k == "home-page" || k == "home"
            })
            .map(|(_, v)| v.clone());

        let extras = pyproj.extras();
        let mut requires_dist = pyproj.dependencies();
        // Append extras dependencies with environment markers
        for extra in &extras {
            for dep in pyproj.extra_deps(extra) {
                requires_dist.push(format!("{} ; extra == \"{}\"", dep, extra));
            }
        }

        Self {
            name,
            version,
            summary: pyproj.description().map(String::from),
            description: None,
            description_content_type: None,
            author,
            author_email,
            maintainer,
            maintainer_email,
            license,
            home_page,
            requires_python: pyproj.requires_python().map(String::from),
            classifiers: pyproj.classifiers(),
            project_urls,
            requires_dist,
            provides_extra: extras,
        }
    }

    /// Build metadata from a parsed setup.cfg.
    pub fn from_setup_cfg(cfg: &crate::setup_cfg::SetupCfg) -> Self {
        let mut requires_dist: Vec<String> = cfg.install_requires.clone();
        let mut provides_extra = Vec::new();

        for (extra, deps) in &cfg.extras_require {
            provides_extra.push(extra.clone());
            for dep in deps {
                requires_dist.push(format!("{} ; extra == \"{}\"", dep, extra));
            }
        }

        Self {
            name: cfg.name.clone().unwrap_or_default(),
            version: cfg.version.clone().unwrap_or_else(|| "0.0.0".into()),
            summary: cfg.description.clone(),
            description: cfg.long_description.clone(),
            description_content_type: cfg.long_description_content_type.clone(),
            author: cfg.author.clone(),
            author_email: cfg.author_email.clone(),
            maintainer: cfg.maintainer.clone(),
            maintainer_email: cfg.maintainer_email.clone(),
            license: cfg.license.clone(),
            home_page: cfg.url.clone(),
            requires_python: cfg.python_requires.clone(),
            classifiers: cfg.classifiers.clone(),
            project_urls: cfg.project_urls.clone(),
            requires_dist,
            provides_extra,
        }
    }

    /// Render into a PEP 566 METADATA string.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("Metadata-Version: 2.1\n");
        out.push_str(&format!("Name: {}\n", self.name));
        out.push_str(&format!("Version: {}\n", self.version));

        if let Some(ref s) = self.summary {
            out.push_str(&format!("Summary: {}\n", s));
        }
        if let Some(ref hp) = self.home_page {
            out.push_str(&format!("Home-page: {}\n", hp));
        }
        if let Some(ref a) = self.author {
            out.push_str(&format!("Author: {}\n", a));
        }
        if let Some(ref ae) = self.author_email {
            out.push_str(&format!("Author-email: {}\n", ae));
        }
        if let Some(ref m) = self.maintainer {
            out.push_str(&format!("Maintainer: {}\n", m));
        }
        if let Some(ref me) = self.maintainer_email {
            out.push_str(&format!("Maintainer-email: {}\n", me));
        }
        if let Some(ref lic) = self.license {
            out.push_str(&format!("License: {}\n", lic));
        }
        for (label, url) in &self.project_urls {
            out.push_str(&format!("Project-URL: {}, {}\n", label, url));
        }
        for cls in &self.classifiers {
            out.push_str(&format!("Classifier: {}\n", cls));
        }
        if let Some(ref rp) = self.requires_python {
            out.push_str(&format!("Requires-Python: {}\n", rp));
        }
        if let Some(ref ct) = self.description_content_type {
            out.push_str(&format!("Description-Content-Type: {}\n", ct));
        }
        for extra in &self.provides_extra {
            out.push_str(&format!("Provides-Extra: {}\n", extra));
        }
        for dep in &self.requires_dist {
            out.push_str(&format!("Requires-Dist: {}\n", dep));
        }

        // Installer tag
        out.push_str("Installer: ferryip\n");

        // Long description separated by blank line
        if let Some(ref desc) = self.description {
            out.push('\n');
            out.push_str(desc);
            if !desc.ends_with('\n') {
                out.push('\n');
            }
        }

        out
    }

    /// Merge a list of (name, email) pairs into single Author / Author-email strings.
    fn merge_persons(persons: &[(Option<String>, Option<String>)]) -> (Option<String>, Option<String>) {
        let names: Vec<&str> = persons
            .iter()
            .filter_map(|(n, _)| n.as_deref())
            .collect();
        let emails: Vec<&str> = persons
            .iter()
            .filter_map(|(_, e)| e.as_deref())
            .collect();
        let name = if names.is_empty() {
            None
        } else {
            Some(names.join(", "))
        };
        let email = if emails.is_empty() {
            None
        } else {
            Some(emails.join(", "))
        };
        (name, email)
    }
}
