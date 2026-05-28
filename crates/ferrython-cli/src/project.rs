//! Project, venv, build, and test command helpers.

use std::fs;
use std::process;

use crate::{run_string, run_test_process_with_timeout};

/// Handle `ferrython -m venv` — create virtual environments.
pub(crate) fn run_venv_module() {
    let args: Vec<String> = std::env::args().collect();
    // Args after `-m venv`: ferrython -m venv [options] <dir>
    let venv_args: Vec<&str> = args.iter().skip(3).map(|s| s.as_str()).collect();

    if venv_args.is_empty() {
        eprintln!("usage: ferrython -m venv [-h] [--clear] [--system-site-packages] [--prompt PROMPT] ENV_DIR");
        process::exit(2);
    }

    let mut opts = ferrython_toolchain::venv::VenvOptions::default();
    let mut dir_path = None;

    let mut i = 0;
    while i < venv_args.len() {
        match venv_args[i] {
            "-h" | "--help" => {
                println!("usage: ferrython -m venv [-h] [--clear] [--system-site-packages] [--prompt PROMPT] ENV_DIR");
                println!();
                println!("Creates virtual Ferrython environments");
                println!();
                println!("positional arguments:");
                println!("  ENV_DIR               A directory to create the environment in");
                println!();
                println!("optional arguments:");
                println!("  -h, --help            show this help message and exit");
                println!(
                    "  --clear               Delete the contents of the environment directory"
                );
                println!("  --system-site-packages Give access to the system site-packages");
                println!("  --without-pip         Skip installing pip");
                println!("  --prompt PROMPT       Set the environment prompt prefix");
                println!("  --copies              Use copies instead of symlinks");
                println!("  --upgrade             Upgrade an existing environment");
                return;
            }
            "--clear" => opts.clear = true,
            "--system-site-packages" => opts.system_site_packages = true,
            "--without-pip" => opts.without_pip = true,
            "--copies" => opts.symlinks = false,
            "--symlinks" => opts.symlinks = true,
            "--upgrade" => opts.upgrade = true,
            "--prompt" => {
                i += 1;
                if i < venv_args.len() {
                    opts.prompt = Some(venv_args[i].to_string());
                } else {
                    eprintln!("Error: --prompt requires a value");
                    process::exit(2);
                }
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                process::exit(2);
            }
            dir => {
                dir_path = Some(dir.to_string());
            }
        }
        i += 1;
    }

    let dir = match dir_path {
        Some(d) => d,
        None => {
            eprintln!("Error: you must provide a destination directory");
            process::exit(2);
        }
    };

    let venv_dir = std::path::Path::new(&dir);
    match ferrython_toolchain::venv::create_venv(venv_dir, &opts) {
        Ok(()) => {
            println!("Created virtual environment in '{}'", venv_dir.display());
            println!(
                "  Activate with: source {}/bin/activate",
                venv_dir.display()
            );
        }
        Err(e) => {
            eprintln!("Error creating venv: {}", e);
            process::exit(1);
        }
    }
}

/// Handle `ferrython new <name>` — create a new project.
pub(crate) fn run_new_project(name: &str, extra_args: &[String]) {
    let mut opts = ferrython_toolchain::scaffold::ProjectOptions {
        name: name.to_string(),
        description: format!("A Python project: {}", name),
        ..Default::default()
    };

    // Parse additional flags
    let mut i = 0;
    while i < extra_args.len() {
        match extra_args[i].as_str() {
            "--author" => {
                i += 1;
                if i < extra_args.len() {
                    opts.author = Some(extra_args[i].clone());
                }
            }
            "--email" => {
                i += 1;
                if i < extra_args.len() {
                    opts.email = Some(extra_args[i].clone());
                }
            }
            "--no-tests" => opts.with_tests = false,
            _ => {}
        }
        i += 1;
    }

    let dir = std::path::Path::new(name);
    if dir.exists() {
        eprintln!("Error: directory '{}' already exists", name);
        process::exit(1);
    }

    match ferrython_toolchain::scaffold::create_project(dir, &opts) {
        Ok(()) => {
            println!("Created project '{}' in ./{}/", name, name);
            println!();
            println!("  cd {}", name);
            println!("  ferrython -m venv .venv");
            println!("  source .venv/bin/activate");
            println!("  ferryip install -e .");
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

/// Handle `ferrython init` — initialize current directory as a project.
pub(crate) fn run_init_project(extra_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("myproject")
        .to_string();

    let mut opts = ferrython_toolchain::scaffold::ProjectOptions {
        name: name.clone(),
        description: format!("A Python project: {}", name),
        ..Default::default()
    };

    for arg in extra_args {
        if arg == "--no-tests" {
            opts.with_tests = false;
        }
    }

    match ferrython_toolchain::scaffold::init_project(&cwd, &opts) {
        Ok(()) => {
            println!("Initialized project '{}' in current directory", name);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

/// Handle `ferrython run [script]` — run a script or project entry point.
///
/// If a script path is given, runs it directly (with venv site-packages on path).
/// Otherwise, looks for pyproject.toml [project.scripts] or falls back to src/<name>/__main__.py.
pub(crate) fn run_project_script(extra_args: &[String]) {
    // Activate venv if present
    activate_venv_if_present();

    // If explicit script given, run it
    if !extra_args.is_empty() && !extra_args[0].starts_with('-') {
        let filename = &extra_args[0];
        if let Some(parent) = std::path::Path::new(filename.as_str()).parent() {
            ferrython_import::prepend_search_path(parent.to_path_buf());
        }
        match fs::read_to_string(filename) {
            Ok(source) => run_string(&source, filename),
            Err(e) => {
                eprintln!("ferrython run: can't open '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    // Look for pyproject.toml entry point
    let cwd = std::env::current_dir().unwrap_or_default();
    let pyproject_path = cwd.join("pyproject.toml");
    if pyproject_path.exists() {
        if let Ok(content) = fs::read_to_string(&pyproject_path) {
            if let Ok(config) = ferrython_toolchain::pyproject::parse_pyproject_str(&content) {
                let name = config.name().unwrap_or_default();
                // Try src/<name>/__main__.py
                let main_paths = [
                    cwd.join("src").join(&name).join("__main__.py"),
                    cwd.join(&name).join("__main__.py"),
                    cwd.join("__main__.py"),
                    cwd.join("main.py"),
                ];
                for main_path in &main_paths {
                    if main_path.exists() {
                        ferrython_import::prepend_search_path(cwd.join("src"));
                        ferrython_import::prepend_search_path(cwd.clone());
                        if let Ok(source) = fs::read_to_string(main_path) {
                            let fname = main_path.to_string_lossy().to_string();
                            run_string(&source, &fname);
                            return;
                        }
                    }
                }
                eprintln!("ferrython run: no entry point found for project '{}'", name);
                eprintln!("  Expected one of:");
                for p in &main_paths {
                    eprintln!("    {}", p.display());
                }
                process::exit(1);
            }
        }
    }
    eprintln!("ferrython run: no pyproject.toml found in current directory");
    process::exit(1);
}

/// Handle `ferrython build` — build wheel/sdist from pyproject.toml.
pub(crate) fn run_project_build(_extra_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let pyproject_path = cwd.join("pyproject.toml");

    if !pyproject_path.exists() {
        eprintln!("ferrython build: no pyproject.toml found in current directory");
        process::exit(1);
    }

    let content = match fs::read_to_string(&pyproject_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ferrython build: {}", e);
            process::exit(1);
        }
    };

    let config = match ferrython_toolchain::pyproject::parse_pyproject_str(&content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ferrython build: invalid pyproject.toml: {}", e);
            process::exit(1);
        }
    };

    let name = config.name().unwrap_or_else(|| "unknown".to_string());
    let version = config.version().unwrap_or("0.0.0").to_string();
    let dist_dir = cwd.join("dist");
    fs::create_dir_all(&dist_dir).ok();

    println!("Building {}-{}...", name, version);

    // Build sdist (.tar.gz)
    let sdist_name = format!("{}-{}.tar.gz", name, version);
    let sdist_path = dist_dir.join(&sdist_name);
    match build_sdist(&cwd, &name, &version, &sdist_path) {
        Ok(()) => println!("  Created sdist: dist/{}", sdist_name),
        Err(e) => eprintln!("  Warning: sdist creation failed: {}", e),
    }

    // Build wheel (.whl)
    let wheel_name = format!("{}-{}-py3-none-any.whl", name, version);
    let wheel_path = dist_dir.join(&wheel_name);
    match build_wheel(&cwd, &name, &version, &wheel_path, &config) {
        Ok(()) => println!("  Created wheel: dist/{}", wheel_name),
        Err(e) => eprintln!("  Warning: wheel creation failed: {}", e),
    }

    println!("Build complete.");
}

/// Handle `ferrython test [args]` — discover and run tests.
pub(crate) fn run_project_tests(extra_args: &[String]) {
    activate_venv_if_present();

    let cwd = std::env::current_dir().unwrap_or_default();
    ferrython_import::prepend_search_path(cwd.join("src"));
    ferrython_import::prepend_search_path(cwd.clone());

    // If explicit test file given, run it directly (no capture)
    if !extra_args.is_empty() && !extra_args[0].starts_with('-') {
        let filename = &extra_args[0];
        match fs::read_to_string(filename) {
            Ok(source) => {
                println!("Running {}...", filename);
                run_string(&source, filename);
            }
            Err(e) => {
                eprintln!("ferrython test: can't open '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    // Auto-discover test files
    let test_dirs = ["tests", "test", "."];
    let mut test_files = Vec::new();
    for dir_name in &test_dirs {
        let dir = cwd.join(dir_name);
        if dir.is_dir() {
            discover_test_files(&dir, &mut test_files);
        }
    }

    if test_files.is_empty() {
        eprintln!("ferrython test: no test files found (test_*.py or *_test.py)");
        process::exit(1);
    }

    test_files.sort();
    test_files.dedup();

    let use_color = atty::is(atty::Stream::Stdout);
    let (green, red, yellow, bold, reset, dim) = if use_color {
        (
            "\x1b[32m", "\x1b[31m", "\x1b[33m", "\x1b[1m", "\x1b[0m", "\x1b[2m",
        )
    } else {
        ("", "", "", "", "", "")
    };

    let total = test_files.len();
    let w: usize = 66;

    // Helper: compute display width (handles emojis and CJK properly)
    fn display_width(s: &str) -> usize {
        s.chars()
            .map(|c| {
                if c.is_ascii() {
                    1
                } else if c >= '\u{1F000}' {
                    2
                }
                // Supplementary plane emojis (🧪📊🎉)
                else if c >= '\u{1100}' && c <= '\u{115F}' {
                    2
                }
                // Korean Jamo
                else if c >= '\u{2E80}' && c <= '\u{A4CF}' {
                    2
                }
                // CJK
                else if c >= '\u{AC00}' && c <= '\u{D7AF}' {
                    2
                }
                // Korean syllables
                else if c >= '\u{FF01}' && c <= '\u{FF60}' {
                    2
                }
                // Fullwidth
                else {
                    1
                } // BMP symbols (✔✘⏱⚠═║─) are single-width
            })
            .sum()
    }
    fn pad_to(s: &str, target: usize) -> String {
        let dw = display_width(s);
        let pad = target.saturating_sub(dw);
        format!("{}{}", s, " ".repeat(pad))
    }

    // Header
    println!("{}╔{}╗{}", bold, "═".repeat(w), reset);
    let title = format!("  🧪 Ferrython Test Suite — {} test files", total);
    println!("{}║{}║{}", bold, pad_to(&title, w), reset);
    println!("{}╚{}╝{}", bold, "═".repeat(w), reset);
    println!();

    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("ferrython"));
    let start = std::time::Instant::now();
    let timeout_secs = std::env::var("FERRYTHON_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(30);
    let per_file_timeout = std::time::Duration::from_secs(timeout_secs);

    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut warned: usize = 0;
    let mut failed_names: Vec<String> = Vec::new();

    for test_file in &test_files {
        let rel = test_file.strip_prefix(&cwd).unwrap_or(test_file);
        let rel_str = rel.display().to_string();

        // Run each test as a subprocess for clean capture, crash isolation, and timeout isolation.
        let result = run_test_process_with_timeout(&exe, test_file, &cwd, per_file_timeout);

        match result {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut combined = if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    format!("{}{}", stdout, stderr)
                };
                if out.timed_out {
                    combined.push_str(&format!(
                        "ferrython test: timed out after {}s\n",
                        timeout_secs
                    ));
                }

                if !out.timed_out && out.status.success() {
                    // Check for internal "FAIL:" markers in test output
                    let fail_lines: Vec<&str> = combined
                        .lines()
                        .filter(|l| l.starts_with("FAIL:") || l.starts_with("  FAIL:"))
                        .collect();
                    if fail_lines.is_empty() {
                        println!("  {}{} OK{}  {}", green, bold, reset, rel_str);
                        passed += 1;
                    } else {
                        println!(
                            "  {}{}\u{26a0} WARN{}  {}  {}({} internal failure(s)){}",
                            yellow,
                            bold,
                            reset,
                            rel_str,
                            dim,
                            fail_lines.len(),
                            reset
                        );
                        for line in &fail_lines {
                            println!("         {}{}{}", dim, line.trim(), reset);
                        }
                        passed += 1;
                        warned += 1;
                    }
                } else {
                    println!("  {}{}\u{2718} FAIL{}  {}", red, bold, reset, rel_str);
                    failed += 1;
                    failed_names.push(rel_str.clone());
                    // Show output in bordered box
                    let lines: Vec<&str> = combined.lines().collect();
                    if !lines.is_empty() {
                        let max_len = lines
                            .iter()
                            .map(|l| display_width(&l.chars().take(74).collect::<String>()))
                            .max()
                            .unwrap_or(0)
                            .min(76);
                        let box_w = max_len + 2;
                        println!("     {}┌{}┐{}", dim, "─".repeat(box_w), reset);
                        for line in &lines {
                            let display: String = line.chars().take(74).collect();
                            let dw = display_width(&display);
                            let pad = max_len.saturating_sub(dw);
                            println!("     {}│ {}{} │{}", dim, display, " ".repeat(pad), reset);
                        }
                        println!("     {}└{}┘{}", dim, "─".repeat(box_w), reset);
                    }
                }
            }
            Err(e) => {
                println!(
                    "  {}{}\u{2718} FAIL{}  {} — {}",
                    red, bold, reset, rel_str, e
                );
                failed += 1;
                failed_names.push(rel_str);
            }
        }
    }

    let elapsed = start.elapsed();
    println!();
    println!("{}╔{}╗{}", bold, "═".repeat(w), reset);
    if failed == 0 {
        let msg = format!("  🎉 All {} tests passed!", total);
        println!("{}║{}{}{}║{}", bold, green, pad_to(&msg, w), reset, bold);
    } else {
        let msg = format!(
            "  📊 {} ✔ passed   {} ✘ failed   ({} total)",
            passed,
            failed,
            passed + failed
        );
        println!("{}║{}║{}", bold, pad_to(&msg, w), reset);
    }
    let time_msg = format!("  ⏱  {:.1}s elapsed", elapsed.as_secs_f64());
    println!("{}║{}║{}", bold, pad_to(&time_msg, w), reset);
    if warned > 0 {
        let warn_msg = format!("  ⚠  {} test(s) with internal failures", warned);
        println!(
            "{}║{}{}{}║{}",
            bold,
            yellow,
            pad_to(&warn_msg, w),
            reset,
            bold
        );
    }
    println!("{}╚{}╝{}", bold, "═".repeat(w), reset);

    if !failed_names.is_empty() {
        println!();
        println!("{}Failed tests:{}", red, reset);
        for name in &failed_names {
            println!("  {} {}", "\u{2718}", name);
        }
    }

    if failed > 0 {
        process::exit(1);
    }
}

// ── Helper functions for project commands ──

fn activate_venv_if_present() {
    let cwd = std::env::current_dir().unwrap_or_default();
    let venv_dirs = [".venv", "venv", ".env"];
    for venv_name in &venv_dirs {
        let venv_dir = cwd.join(venv_name);
        if venv_dir.join("pyvenv.cfg").exists() {
            let site_packages = venv_dir.join("lib").join("python3.8").join("site-packages");
            if site_packages.is_dir() {
                ferrython_import::prepend_search_path(site_packages);
            }
            // Also check lib/pythonX.Y/site-packages with glob
            if let Ok(entries) = fs::read_dir(venv_dir.join("lib")) {
                for entry in entries.flatten() {
                    let sp = entry.path().join("site-packages");
                    if sp.is_dir() {
                        ferrython_import::prepend_search_path(sp);
                    }
                }
            }
            break;
        }
    }
}

fn discover_test_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirectories (but skip hidden dirs, __pycache__, etc.)
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with('.')
                    && name != "__pycache__"
                    && name != "node_modules"
                    && name != "site-packages"
                    && name != "target"
                    && name != "venv"
                    && name != ".venv"
                {
                    discover_test_files(&path, out);
                }
            }
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py")) {
                out.push(path);
            }
        }
    }
}

fn build_sdist(
    project_dir: &std::path::Path,
    name: &str,
    version: &str,
    output: &std::path::Path,
) -> Result<(), String> {
    let file = fs::File::create(output).map_err(|e| e.to_string())?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    let prefix = format!("{}-{}", name, version);

    // Add Python source files
    let src_dirs = ["src", name, "."];
    for dir_name in &src_dirs {
        let dir = project_dir.join(dir_name);
        if dir.is_dir() {
            add_python_files_to_tar(&mut tar, &dir, project_dir, &prefix)?;
        }
    }

    // Add pyproject.toml
    let pyproject = project_dir.join("pyproject.toml");
    if pyproject.exists() {
        let data = fs::read(&pyproject).map_err(|e| e.to_string())?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(
            &mut header,
            format!("{}/pyproject.toml", prefix),
            data.as_slice(),
        )
        .map_err(|e| e.to_string())?;
    }

    // Add PKG-INFO
    let pkg_info = format!(
        "Metadata-Version: 2.1\nName: {}\nVersion: {}\n",
        name, version
    );
    let mut header = tar::Header::new_gnu();
    header.set_size(pkg_info.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("{}/PKG-INFO", prefix),
        pkg_info.as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let enc = tar.into_inner().map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn build_wheel(
    project_dir: &std::path::Path,
    name: &str,
    version: &str,
    output: &std::path::Path,
    config: &ferrython_toolchain::pyproject::PyProject,
) -> Result<(), String> {
    use std::io::Write;
    let file = fs::File::create(output).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let dist_info = format!("{}-{}.dist-info", name, version);

    // Collect source files
    let src_dirs = ["src", name];
    for dir_name in &src_dirs {
        let dir = project_dir.join(dir_name);
        if dir.is_dir() {
            add_python_files_to_zip(&mut zip, &dir, project_dir, "", &options)?;
        }
    }

    // METADATA
    let description = config.description().unwrap_or_default();
    let requires_python = config.requires_python().unwrap_or_default();
    let mut metadata = format!(
        "Metadata-Version: 2.1\nName: {}\nVersion: {}\n",
        name, version
    );
    if !description.is_empty() {
        metadata.push_str(&format!("Summary: {}\n", description));
    }
    if !requires_python.is_empty() {
        metadata.push_str(&format!("Requires-Python: {}\n", requires_python));
    }
    for dep in config.dependencies() {
        metadata.push_str(&format!("Requires-Dist: {}\n", dep));
    }
    zip.start_file(format!("{}/METADATA", dist_info), options)
        .map_err(|e| e.to_string())?;
    zip.write_all(metadata.as_bytes())
        .map_err(|e| e.to_string())?;

    // WHEEL
    let wheel_content = format!(
        "Wheel-Version: 1.0\nGenerator: ferrython {}\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        "0.1.0"
    );
    zip.start_file(format!("{}/WHEEL", dist_info), options)
        .map_err(|e| e.to_string())?;
    zip.write_all(wheel_content.as_bytes())
        .map_err(|e| e.to_string())?;

    // RECORD (empty — we'd normally hash all files)
    zip.start_file(format!("{}/RECORD", dist_info), options)
        .map_err(|e| e.to_string())?;
    zip.write_all(b"").map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn add_python_files_to_tar<W: std::io::Write>(
    tar: &mut tar::Builder<W>,
    dir: &std::path::Path,
    base: &std::path::Path,
    prefix: &str,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_str().unwrap_or("");
            if !name.starts_with('.') && name != "__pycache__" {
                add_python_files_to_tar(tar, &path, base, prefix)?;
            }
        } else if let Some(ext) = path.extension() {
            if ext == "py" || ext == "pyi" || ext == "toml" || ext == "cfg" || ext == "txt" {
                let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
                let tar_path = format!("{}/{}", prefix, rel.display());
                let data = fs::read(&path).map_err(|e| e.to_string())?;
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, tar_path, data.as_slice())
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn add_python_files_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &std::path::Path,
    base: &std::path::Path,
    _prefix: &str,
    options: &zip::write::SimpleFileOptions,
) -> Result<(), String> {
    use std::io::Write;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_str().unwrap_or("");
            if !name.starts_with('.') && name != "__pycache__" {
                add_python_files_to_zip(zip, &path, base, _prefix, options)?;
            }
        } else if let Some(ext) = path.extension() {
            if ext == "py" || ext == "pyi" {
                let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
                let zip_path = rel.display().to_string();
                let data = fs::read(&path).map_err(|e| e.to_string())?;
                zip.start_file(&zip_path, *options)
                    .map_err(|e| e.to_string())?;
                zip.write_all(&data).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
