//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

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
    // Initialize GC cycle detection
    ferrython_core::object::init_gc();
    
    let args: Vec<String> = env::args().collect();

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
        run_string(&args[2], "<string>");
        return;
    }

    if args[1] == "-m" {
        if args.len() < 3 {
            eprintln!("No module name specified");
            process::exit(2);
        }
        let module_name = &args[2];
        // Pass remaining args as sys.argv
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
        println!("  -h, --help      Show this help");
        println!();
        println!("Project commands:");
        println!("  new NAME        Create a new project with pyproject.toml");
        println!("  init            Initialize current directory as a project");
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

    let filename = &args[1];
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
                            let source = std::fs::read_to_string(&main_py).unwrap_or_default();
                            run_string(&source, &main_py);
                            return;
                        }
                    }
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
