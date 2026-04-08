//! setup.cfg INI parser — reads [metadata], [options], and [options.extras_require].

use std::collections::HashMap;
use std::path::Path;

/// Parsed contents of a setup.cfg file.
#[derive(Debug, Clone, Default)]
pub struct SetupCfg {
    // [metadata]
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub long_description: Option<String>,
    pub long_description_content_type: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub maintainer: Option<String>,
    pub maintainer_email: Option<String>,
    pub license: Option<String>,
    pub url: Option<String>,
    pub classifiers: Vec<String>,
    pub project_urls: Vec<(String, String)>,

    // [options]
    pub install_requires: Vec<String>,
    pub python_requires: Option<String>,
    pub packages: Vec<String>,

    // [options.extras_require]
    pub extras_require: HashMap<String, Vec<String>>,
}

/// Parse a setup.cfg file from disk.
pub fn parse_setup_cfg(path: &Path) -> Result<SetupCfg, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    parse_setup_cfg_str(&content)
}

/// Parse setup.cfg from a string.
pub fn parse_setup_cfg_str(content: &str) -> Result<SetupCfg, String> {
    let mut cfg = SetupCfg::default();
    let mut current_section = String::new();
    // For multi-line values: (section, key) -> accumulated lines
    let mut current_key: Option<(String, String)> = None;
    // Temporary storage for multi-line values
    let mut multiline_values: HashMap<(String, String), Vec<String>> = HashMap::new();

    for line in content.lines() {
        // Blank line may terminate continuation in some contexts but we keep
        // collecting until a new key or section header appears.
        if line.trim().is_empty() {
            continue;
        }

        // Section header
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                current_section = line[1..end].trim().to_string();
                current_key = None;
                continue;
            }
        }

        // Continuation line (starts with whitespace)
        if (line.starts_with(' ') || line.starts_with('\t')) && current_key.is_some() {
            let val = line.trim();
            if !val.is_empty() && !val.starts_with('#') {
                let key = current_key.as_ref().unwrap().clone();
                multiline_values
                    .entry(key)
                    .or_default()
                    .push(val.to_string());
            }
            continue;
        }

        // Comment line
        if line.trim().starts_with('#') || line.trim().starts_with(';') {
            current_key = None;
            continue;
        }

        // Key = value
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let val = line[eq_pos + 1..].trim().to_string();
            let section_key = (current_section.clone(), key.clone());
            current_key = Some(section_key.clone());

            if !val.is_empty() {
                multiline_values
                    .entry(section_key)
                    .or_default()
                    .push(val);
            }
        }
    }

    // Process accumulated values into the struct
    for ((section, key), values) in &multiline_values {
        match section.as_str() {
            "metadata" => apply_metadata_field(&mut cfg, key, values),
            "options" => apply_options_field(&mut cfg, key, values),
            s if s.starts_with("options.extras_require") => {
                // Each key in this section is an extra name
                cfg.extras_require.insert(key.clone(), values.clone());
            }
            _ => {} // ignore unknown sections
        }
    }

    Ok(cfg)
}

fn apply_metadata_field(cfg: &mut SetupCfg, key: &str, values: &[String]) {
    let single = values.first().map(|s| s.as_str()).unwrap_or("");
    match key {
        "name" => cfg.name = Some(single.to_string()),
        "version" => cfg.version = Some(single.to_string()),
        "description" | "summary" => cfg.description = Some(single.to_string()),
        "long_description" | "long-description" => {
            // May reference a file: `file: README.md` — store as-is
            cfg.long_description = Some(values.join("\n"));
        }
        "long_description_content_type" | "long-description-content-type" => {
            cfg.long_description_content_type = Some(single.to_string());
        }
        "author" => cfg.author = Some(single.to_string()),
        "author_email" | "author-email" => cfg.author_email = Some(single.to_string()),
        "maintainer" => cfg.maintainer = Some(single.to_string()),
        "maintainer_email" | "maintainer-email" => cfg.maintainer_email = Some(single.to_string()),
        "license" => cfg.license = Some(single.to_string()),
        "url" | "home-page" | "home_page" => cfg.url = Some(single.to_string()),
        "classifiers" | "classifier" => {
            cfg.classifiers.extend(values.iter().cloned());
        }
        "project_urls" | "project-urls" => {
            for val in values {
                if let Some(eq) = val.find('=') {
                    let label = val[..eq].trim().to_string();
                    let url = val[eq + 1..].trim().to_string();
                    cfg.project_urls.push((label, url));
                }
            }
        }
        _ => {}
    }
}

fn apply_options_field(cfg: &mut SetupCfg, key: &str, values: &[String]) {
    match key {
        "install_requires" | "install-requires" => {
            cfg.install_requires.extend(values.iter().cloned());
        }
        "python_requires" | "python-requires" => {
            cfg.python_requires = values.first().cloned();
        }
        "packages" => {
            cfg.packages.extend(values.iter().cloned());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_setup_cfg() {
        let content = r#"
[metadata]
name = my-package
version = 1.2.3
author = Alice
author_email = alice@example.com
license = MIT
description = A cool package
classifiers =
    Programming Language :: Python :: 3
    License :: OSI Approved :: MIT License
project_urls =
    Homepage = https://example.com
    Bug Tracker = https://example.com/bugs

[options]
python_requires = >=3.8
install_requires =
    requests>=2.20
    click

[options.extras_require]
dev =
    pytest
    black
"#;
        let cfg = parse_setup_cfg_str(content).unwrap();
        assert_eq!(cfg.name.as_deref(), Some("my-package"));
        assert_eq!(cfg.version.as_deref(), Some("1.2.3"));
        assert_eq!(cfg.author.as_deref(), Some("Alice"));
        assert_eq!(cfg.author_email.as_deref(), Some("alice@example.com"));
        assert_eq!(cfg.license.as_deref(), Some("MIT"));
        assert_eq!(cfg.python_requires.as_deref(), Some(">=3.8"));
        assert_eq!(cfg.install_requires.len(), 2);
        assert!(cfg.install_requires.contains(&"requests>=2.20".to_string()));
        assert!(cfg.install_requires.contains(&"click".to_string()));
        assert_eq!(cfg.classifiers.len(), 2);
        assert_eq!(cfg.project_urls.len(), 2);
        assert!(cfg.extras_require.contains_key("dev"));
        assert_eq!(cfg.extras_require["dev"].len(), 2);
    }
}
