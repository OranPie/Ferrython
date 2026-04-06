//! Project scaffolding — `ferrython new` and `ferrython init` implementation.

use std::fs;
use std::path::Path;

/// Options for project creation.
pub struct ProjectOptions {
    /// Project name
    pub name: String,
    /// Author name
    pub author: Option<String>,
    /// Author email
    pub email: Option<String>,
    /// Include a test directory
    pub with_tests: bool,
    /// Python version requirement
    pub python_requires: String,
    /// Project description
    pub description: String,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            author: None,
            email: None,
            with_tests: true,
            python_requires: ">=3.8".to_string(),
            description: String::new(),
        }
    }
}

/// Create a new project directory with standard layout.
pub fn create_project(dir: &Path, opts: &ProjectOptions) -> Result<(), String> {
    let package_name = opts.name.replace('-', "_");

    // Create directory structure
    fs::create_dir_all(dir.join("src").join(&package_name))
        .map_err(|e| format!("mkdir: {}", e))?;

    if opts.with_tests {
        fs::create_dir_all(dir.join("tests"))
            .map_err(|e| format!("mkdir tests: {}", e))?;
    }

    // Write pyproject.toml
    let author_line = match (&opts.author, &opts.email) {
        (Some(name), Some(email)) => format!("authors = [{{name = \"{}\", email = \"{}\"}}]\n", name, email),
        (Some(name), None) => format!("authors = [{{name = \"{}\"}}]\n", name),
        _ => String::new(),
    };

    let pyproject = format!(
r#"[build-system]
requires = ["setuptools>=61.0"]
build-backend = "setuptools.build_meta"

[project]
name = "{name}"
version = "0.1.0"
description = "{description}"
{author_line}requires-python = "{python_requires}"
license = {{text = "MIT"}}
readme = "README.md"
dependencies = []

[project.optional-dependencies]
dev = ["pytest"]

[project.scripts]
# {name} = "{package_name}:main"

[tool.setuptools.packages.find]
where = ["src"]
"#,
        name = opts.name,
        description = opts.description,
        author_line = author_line,
        python_requires = opts.python_requires,
        package_name = package_name,
    );

    fs::write(dir.join("pyproject.toml"), pyproject)
        .map_err(|e| format!("Write pyproject.toml: {}", e))?;

    // Write __init__.py
    let init_py = format!(
r#""""{}"""

__version__ = "0.1.0"
"#,
        opts.description
    );
    fs::write(dir.join("src").join(&package_name).join("__init__.py"), init_py)
        .map_err(|e| format!("Write __init__.py: {}", e))?;

    // Write __main__.py
    let main_py = format!(
r#""""Entry point for `ferrython -m {package_name}`."""


def main():
    print("Hello from {name}!")


if __name__ == "__main__":
    main()
"#,
        name = opts.name,
        package_name = package_name,
    );
    fs::write(dir.join("src").join(&package_name).join("__main__.py"), main_py)
        .map_err(|e| format!("Write __main__.py: {}", e))?;

    // Write README.md
    let readme = format!(
        "# {}\n\n{}\n\n## Installation\n\n```bash\nferryip install -e .\n```\n\n## Usage\n\n```python\nimport {}\n```\n",
        opts.name, opts.description, package_name
    );
    fs::write(dir.join("README.md"), readme)
        .map_err(|e| format!("Write README.md: {}", e))?;

    // Write .gitignore
    let gitignore = "__pycache__/\n*.py[cod]\n*$py.class\n*.egg-info/\ndist/\nbuild/\n.venv/\n*.egg\n.pytest_cache/\n";
    fs::write(dir.join(".gitignore"), gitignore)
        .map_err(|e| format!("Write .gitignore: {}", e))?;

    // Write tests/__init__.py and tests/test_basic.py
    if opts.with_tests {
        fs::write(dir.join("tests").join("__init__.py"), "")
            .map_err(|e| format!("Write tests/__init__.py: {}", e))?;

        let test_file = format!(
r#""""Basic tests for {name}."""

import {package_name}


def test_version():
    assert {package_name}.__version__ == "0.1.0"


def test_import():
    assert {package_name} is not None
"#,
            name = opts.name,
            package_name = package_name,
        );
        fs::write(dir.join("tests").join("test_basic.py"), test_file)
            .map_err(|e| format!("Write test_basic.py: {}", e))?;
    }

    Ok(())
}

/// Initialize an existing directory as a project (adds pyproject.toml if missing).
pub fn init_project(dir: &Path, opts: &ProjectOptions) -> Result<(), String> {
    if dir.join("pyproject.toml").exists() {
        return Err("pyproject.toml already exists in this directory".to_string());
    }
    create_project(dir, opts)
}
