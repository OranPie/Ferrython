//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

use ferrython_core::object::PyObjectMethods;

/// Unified error for the parse → compile → execute pipeline.
enum PipelineError {
    Parse(ferrython_parser::ParseError),
    Compile(ferrython_compiler::CompileError),
    Runtime(ferrython_core::error::PyException),
}

impl From<ferrython_parser::ParseError> for PipelineError {
    fn from(e: ferrython_parser::ParseError) -> Self { Self::Parse(e) }
}
impl From<ferrython_compiler::CompileError> for PipelineError {
    fn from(e: ferrython_compiler::CompileError) -> Self { Self::Compile(e) }
}
impl From<ferrython_core::error::PyException> for PipelineError {
    fn from(e: ferrython_core::error::PyException) -> Self { Self::Runtime(e) }
}

impl PipelineError {
    fn report(&self, filename: &str) {
        match self {
            Self::Parse(e) => {
                eprintln!("  File \"{}\"", filename);
                eprintln!("SyntaxError: {}", e);
            }
            Self::Compile(e) => {
                eprintln!("  File \"{}\", compilation error", filename);
                eprintln!("CompileError: {}", e);
            }
            Self::Runtime(e) => {
                eprintln!("{}", ferrython_debug::format_traceback(e));
            }
        }
    }
}

fn main() {
    // Spawn main work on a thread with a larger stack (64 MB) to support
    // deep Python recursion without hitting Rust stack overflow.
    let builder = std::thread::Builder::new()
        .name("ferrython-main".into())
        .stack_size(64 * 1024 * 1024);
    let handler = builder.spawn(main_inner).expect("failed to spawn main thread");
    if let Err(e) = handler.join() {
        if let Some(msg) = e.downcast_ref::<&str>() {
            eprintln!("Fatal error: {}", msg);
        } else if let Some(msg) = e.downcast_ref::<String>() {
            eprintln!("Fatal error: {}", msg);
        }
        process::exit(1);
    }
}

fn main_inner() {
    // Initialize GC cycle detection
    ferrython_core::object::init_gc();
    // Initialize import search paths (discovers stdlib, site-packages)
    // Must happen before any module is imported so sys.path reflects them.
    ferrython_import::init();
    
    let args: Vec<String> = env::args().collect();

    // Check for --compat flag or FERRYTHON_COMPAT env var: disable superinstructions
    // to emit only standard CPython 3.8 opcodes for fair performance comparison.
    let compat_mode = args.iter().any(|a| a == "--compat")
        || env::var("FERRYTHON_COMPAT").map(|v| v == "1" || v == "true").unwrap_or(false);
    if compat_mode {
        ferrython_compiler::set_superinstructions_enabled(false);
    }
    // Filter out --compat from args for downstream processing
    let args: Vec<String> = args.into_iter().filter(|a| a != "--compat").collect();

    if args.len() < 2 {
        // If stdin is not a terminal, read from stdin as a script
        if atty::isnt(atty::Stream::Stdin) {
            let mut source = String::new();
            io::stdin().read_to_string(&mut source).unwrap_or_default();
            if !source.trim().is_empty() {
                run_string(&source, "<stdin>");
                return;
            }
        }
        ferrython_repl::run_repl();
        return;
    }

    if args[1] == "-c" {
        if args.len() < 3 {
            eprintln!("Argument expected for the -c option");
            process::exit(2);
        }
        // sys.argv[0] = "-c", remaining args follow the code string
        let mut argv = vec![String::from("-c")];
        argv.extend_from_slice(&args[3..]);
        ferrython_stdlib::set_argv(argv);
        run_string(&args[2], "<string>");
        return;
    }

    if args[1] == "-m" {
        if args.len() < 3 {
            eprintln!("No module name specified");
            process::exit(2);
        }
        let module_name = &args[2];
        // sys.argv is set inside run_module once the module file path is known
        let module_args: Vec<String> = args[2..].to_vec();
        run_module(module_name, &module_args);
        return;
    }

    if args[1] == "--version" || args[1] == "-V" {
        println!("Ferrython 0.1.0 (Python 3.8 compatible)");
        return;
    }

    if args[1] == "--dis" {
        if args.len() < 3 {
            eprintln!("Usage: ferrython --dis <script.py>");
            process::exit(2);
        }
        let filename = &args[2];
        if let Some(parent) = std::path::Path::new(filename).parent() {
            ferrython_import::prepend_search_path(parent.to_path_buf());
        }
        match fs::read_to_string(filename) {
            Ok(source) => dis_and_run_string(&source, filename),
            Err(e) => {
                eprintln!("ferrython: can't open file '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    if args[1] == "--profile" {
        if args.len() < 3 {
            eprintln!("Usage: ferrython --profile <script.py>");
            process::exit(2);
        }
        let filename = &args[2];
        if let Some(parent) = std::path::Path::new(filename).parent() {
            ferrython_import::prepend_search_path(parent.to_path_buf());
        }
        match fs::read_to_string(filename) {
            Ok(source) => run_profiled(&source, filename),
            Err(e) => {
                eprintln!("ferrython: can't open file '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    if args[1] == "--stats" {
        if args.len() < 3 {
            eprintln!("Usage: ferrython --stats <script.py>");
            process::exit(2);
        }
        let filename = &args[2];
        match fs::read_to_string(filename) {
            Ok(source) => stats_string(&source, filename),
            Err(e) => {
                eprintln!("ferrython: can't open file '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    if args[1] == "--help" || args[1] == "-h" {
        println!("Usage: ferrython [options] [script.py]");
        println!();
        println!("Options:");
        println!("  -c CMD          Execute CMD as a string");
        println!("  -m MODULE       Run library module as a script");
        println!("  -V, --version   Show version");
        println!("  --dis FILE      Disassemble bytecode to stderr, then execute");
        println!("  --profile FILE  Run with execution profiling");
        println!("  --stats FILE    Show bytecode statistics");
        println!("  --compat        CPython-compatible mode (no superinstructions)");
        println!("  -h, --help      Show this help");
        println!();
        println!("Project commands:");
        println!("  new NAME        Create a new project with pyproject.toml");
        println!("  init            Initialize current directory as a project");
        println!("  run [SCRIPT]    Run project entry point or a script in venv context");
        println!("  build           Build project (create wheel/sdist)");
        println!("  test [ARGS]     Run project tests (discovers test_*.py files)");
        return;
    }

    // Project management commands
    if args[1] == "new" {
        if args.len() < 3 {
            eprintln!("Usage: ferrython new <project-name>");
            process::exit(2);
        }
        run_new_project(&args[2], &args[3..]);
        return;
    }

    if args[1] == "init" {
        run_init_project(&args[2..]);
        return;
    }

    if args[1] == "run" {
        run_project_script(&args[2..]);
        return;
    }

    if args[1] == "build" {
        run_project_build(&args[2..]);
        return;
    }

    if args[1] == "test" {
        run_project_tests(&args[2..]);
        return;
    }

    let filename = &args[1];
    // sys.argv[0] = script path, remaining args follow
    let mut argv = vec![filename.clone()];
    argv.extend_from_slice(&args[2..]);
    ferrython_stdlib::set_argv(argv);
    if let Some(parent) = std::path::Path::new(filename).parent() {
        ferrython_import::prepend_search_path(parent.to_path_buf());
    }
    match fs::read_to_string(filename) {
        Ok(source) => run_string(&source, filename),
        Err(e) => {
            eprintln!("ferrython: can't open file '{}': {}", filename, e);
            process::exit(2);
        }
    }
}

fn execute_pipeline(source: &str, filename: &str) -> Result<(), (PipelineError, Option<ferrython_vm::VirtualMachine>)> {
    let module = ferrython_parser::parse(source, filename)
        .map_err(|e| (PipelineError::from(e), None))?;
    let code = ferrython_compiler::compile(&module, filename)
        .map_err(|e| (PipelineError::from(e), None))?;
    let mut vm = ferrython_vm::VirtualMachine::new();
    match vm.execute(code) {
        Ok(_) => Ok(()),
        Err(e) => Err((PipelineError::Runtime(e), Some(vm))),
    }
}

fn run_string(source: &str, filename: &str) {
    if let Err((e, vm_opt)) = execute_pipeline(source, filename) {
        if let PipelineError::Runtime(ref exc) = e {
            // Handle SystemExit specially — exit with the code, don't print traceback
            if exc.kind == ferrython_core::error::ExceptionKind::SystemExit {
                let code = exc.value.as_ref()
                    .map(|v| v.to_int().unwrap_or(1) as i32)
                    .unwrap_or(0);
                process::exit(code);
            }
            // Try sys.excepthook before default traceback display
            if let Some(mut vm) = vm_opt {
                if vm.invoke_excepthook(exc) {
                    process::exit(1);
                }
            }
        }
        e.report(filename);
        process::exit(1);
    }
}

fn dis_and_run_pipeline(source: &str, filename: &str) -> Result<(), PipelineError> {
    let module = ferrython_parser::parse(source, filename)?;
    let code = ferrython_compiler::compile(&module, filename)?;
    ferrython_debug::dis_code_stderr(&code, 0);
    let mut vm = ferrython_vm::VirtualMachine::new();
    vm.execute(code)?;
    Ok(())
}

fn dis_and_run_string(source: &str, filename: &str) {
    if let Err(e) = dis_and_run_pipeline(source, filename) {
        if let PipelineError::Runtime(ref exc) = e {
            if exc.kind == ferrython_core::error::ExceptionKind::SystemExit {
                let code = exc.value.as_ref()
                    .map(|v| v.to_int().unwrap_or(1) as i32)
                    .unwrap_or(0);
                process::exit(code);
            }
        }
        e.report(filename);
        process::exit(1);
    }
}

fn profiled_pipeline(source: &str, filename: &str) -> Result<(), PipelineError> {
    let module = ferrython_parser::parse(source, filename)?;
    let code = ferrython_compiler::compile(&module, filename)?;
    let mut vm = ferrython_vm::VirtualMachine::new();
    vm.profiler.set_enabled(true);
    let result = vm.execute(code);
    eprintln!();
    vm.profiler.report();
    result?;
    Ok(())
}

fn run_profiled(source: &str, filename: &str) {
    if let Err(e) = profiled_pipeline(source, filename) {
        if let PipelineError::Runtime(ref exc) = e {
            if exc.kind == ferrython_core::error::ExceptionKind::SystemExit {
                let code = exc.value.as_ref()
                    .map(|v| v.to_int().unwrap_or(1) as i32)
                    .unwrap_or(0);
                process::exit(code);
            }
        }
        e.report(filename);
        process::exit(1);
    }
}

fn stats_pipeline(source: &str, filename: &str) -> Result<(), PipelineError> {
    let module = ferrython_parser::parse(source, filename)?;
    let code = ferrython_compiler::compile(&module, filename)?;
    let stats = ferrython_debug::code_stats(&code);
    ferrython_debug::stats::print_stats_report(&stats);
    Ok(())
}

fn stats_string(source: &str, filename: &str) {
    if let Err(e) = stats_pipeline(source, filename) {
        e.report(filename);
        process::exit(1);
    }
}

/// Run a module using `-m module_name` semantics.
///
/// For known built-in modules (venv, pip, etc.), dispatch directly.
/// For Python modules, find and execute their `__main__.py` or the module file.
fn run_module(module_name: &str, _module_args: &[String]) {
    match module_name {
        "venv" => {
            run_venv_module();
        }
        "pip" | "ferryip" => {
            // Delegate to ferryip by running the binary
            let ferryip = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.join("ferryip")))
                .unwrap_or_else(|| std::path::PathBuf::from("ferryip"));

            let pip_args: Vec<String> = std::env::args().skip(3).collect();
            let status = std::process::Command::new(&ferryip)
                .args(&pip_args)
                .status();

            match status {
                Ok(s) => process::exit(s.code().unwrap_or(1)),
                Err(_) => {
                    eprintln!("ferrython: ferryip not found. Build with `cargo build -p ferrython-pip`");
                    process::exit(1);
                }
            }
        }
        "ensurepip" => {
            println!("ferryip is bundled with Ferrython. Use `ferrython -m pip` directly.");
        }
        "site" => {
            // Print site-packages info (like `python -m site`)
            let _layout = ferrython_toolchain::paths::InstallLayout::discover();
            println!("sys.path = [");
            for p in ferrython_import::get_search_paths() {
                println!("    '{}',", p.display());
            }
            println!("]");
            println!("USER_BASE: '{}/.local' (exists)", std::env::var("HOME").unwrap_or_default());
            println!("USER_SITE: '{}/.local/lib/ferrython/site-packages'", std::env::var("HOME").unwrap_or_default());
            println!("ENABLE_USER_SITE: True");
        }
        "sysconfig" => {
            // Print sysconfig info
            let layout = ferrython_toolchain::paths::InstallLayout::discover();
            println!("Platform: \"{}\"", if cfg!(target_os = "linux") { "linux" } else if cfg!(target_os = "macos") { "darwin" } else { "unknown" });
            println!("Python version: \"3.11\"");
            println!("Paths:");
            for name in &["stdlib", "purelib", "platlib", "include", "scripts", "data"] {
                if let Some(p) = layout.get_path(name) {
                    println!("  {}: \"{}\"", name, p.display());
                }
            }
        }
        _ => {
            // Try to find and execute the module as Python code
            // Look for module/__main__.py or module.py
            match ferrython_import::resolve_module(module_name, "<cli>") {
                Ok(ferrython_import::ResolvedModule::Source { code, name: _, file_path }) => {
                    // Check for __main__.py in package
                    let file = file_path.as_deref().unwrap_or("<module>");
                    if file.ends_with("__init__.py") {
                        // It's a package — look for __main__.py
                        let main_py = file.replace("__init__.py", "__main__.py");
                        if std::path::Path::new(&main_py).exists() {
                            // sys.argv[0] = path to __main__.py, rest = module args[1..]
                            let mut argv = vec![main_py.clone()];
                            argv.extend(_module_args.iter().skip(1).cloned());
                            ferrython_stdlib::set_argv(argv);
                            let source = std::fs::read_to_string(&main_py).unwrap_or_default();
                            run_string(&source, &main_py);
                            return;
                        }
                    }
                    // sys.argv[0] = module file path, rest = module args[1..]
                    let mut argv = vec![file.to_string()];
                    argv.extend(_module_args.iter().skip(1).cloned());
                    ferrython_stdlib::set_argv(argv);
                    // Execute the module directly
                    let mut vm = ferrython_vm::VirtualMachine::new();
                    if let Err(e) = vm.execute((*code).clone()) {
                        if e.kind == ferrython_core::error::ExceptionKind::SystemExit {
                            let exit_code = e.value.as_ref()
                                .map(|v| v.to_int().unwrap_or(1) as i32)
                                .unwrap_or(0);
                            process::exit(exit_code);
                        }
                        eprintln!("{}", ferrython_debug::format_traceback(&e));
                        process::exit(1);
                    }
                }
                Ok(ferrython_import::ResolvedModule::Builtin(_module)) => {
                    eprintln!("ferrython: No code to run for built-in module '{}'", module_name);
                    process::exit(1);
                }
                Err(e) => {
                    eprintln!("ferrython: No module named '{}'", module_name);
                    eprintln!("  {}", e.message);
                    process::exit(1);
                }
            }
        }
    }
}

/// Handle `ferrython -m venv` — create virtual environments.
fn run_venv_module() {
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
                println!("  --clear               Delete the contents of the environment directory");
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
            println!("  Activate with: source {}/bin/activate", venv_dir.display());
        }
        Err(e) => {
            eprintln!("Error creating venv: {}", e);
            process::exit(1);
        }
    }
}

/// Handle `ferrython new <name>` — create a new project.
fn run_new_project(name: &str, extra_args: &[String]) {
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
fn run_init_project(extra_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let name = cwd.file_name()
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
fn run_project_script(extra_args: &[String]) {
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
fn run_project_build(_extra_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let pyproject_path = cwd.join("pyproject.toml");

    if !pyproject_path.exists() {
        eprintln!("ferrython build: no pyproject.toml found in current directory");
        process::exit(1);
    }

    let content = match fs::read_to_string(&pyproject_path) {
        Ok(c) => c,
        Err(e) => { eprintln!("ferrython build: {}", e); process::exit(1); }
    };

    let config = match ferrython_toolchain::pyproject::parse_pyproject_str(&content) {
        Ok(c) => c,
        Err(e) => { eprintln!("ferrython build: invalid pyproject.toml: {}", e); process::exit(1); }
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
fn run_project_tests(extra_args: &[String]) {
    activate_venv_if_present();

    let cwd = std::env::current_dir().unwrap_or_default();
    ferrython_import::prepend_search_path(cwd.join("src"));
    ferrython_import::prepend_search_path(cwd.clone());

    // If explicit test file given, run it
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
    println!("Discovered {} test file(s):", test_files.len());

    let mut passed = 0;
    let mut failed = 0;
    for test_file in &test_files {
        let rel = test_file.strip_prefix(&cwd).unwrap_or(test_file);
        print!("  {} ... ", rel.display());
        match fs::read_to_string(test_file) {
            Ok(source) => {
                let fname = test_file.to_string_lossy().to_string();
                match execute_pipeline(&source, &fname) {
                    Ok(()) => { println!("ok"); passed += 1; }
                    Err((e, _vm)) => {
                        println!("FAIL");
                        e.report(&fname);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                println!("ERROR ({})", e);
                failed += 1;
            }
        }
    }

    println!();
    println!("{} passed, {} failed", passed, failed);
    if failed > 0 { process::exit(1); }
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
                if !name.starts_with('.') && name != "__pycache__" && name != "node_modules" {
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
        ).map_err(|e| e.to_string())?;
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
    ).map_err(|e| e.to_string())?;

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
    zip.start_file(format!("{}/METADATA", dist_info), options).map_err(|e| e.to_string())?;
    zip.write_all(metadata.as_bytes()).map_err(|e| e.to_string())?;

    // WHEEL
    let wheel_content = format!(
        "Wheel-Version: 1.0\nGenerator: ferrython {}\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        "0.1.0"
    );
    zip.start_file(format!("{}/WHEEL", dist_info), options).map_err(|e| e.to_string())?;
    zip.write_all(wheel_content.as_bytes()).map_err(|e| e.to_string())?;

    // RECORD (empty — we'd normally hash all files)
    zip.start_file(format!("{}/RECORD", dist_info), options).map_err(|e| e.to_string())?;
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
                zip.start_file(&zip_path, *options).map_err(|e| e.to_string())?;
                zip.write_all(&data).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(()
    )
}
