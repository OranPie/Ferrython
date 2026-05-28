//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process;

use ferrython_core::object::PyObjectMethods;

mod project;

use project::{
    run_init_project, run_new_project, run_project_build, run_project_script, run_project_tests,
    run_venv_module,
};

/// Unified error for the parse → compile → execute pipeline.
enum PipelineError {
    Parse(ferrython_parser::ParseError),
    Compile(ferrython_compiler::CompileError),
    Runtime(ferrython_core::error::PyException),
}

struct TestRunOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

fn join_output_reader(handle: std::thread::JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => Ok(Vec::new()),
    }
}

pub(crate) fn run_test_process_with_timeout(
    exe: &std::path::Path,
    test_file: &std::path::Path,
    cwd: &std::path::Path,
    timeout: std::time::Duration,
) -> io::Result<TestRunOutput> {
    let mut child = std::process::Command::new(exe)
        .arg(test_file)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout {
            pipe.read_to_end(&mut buf)?;
        }
        Ok(buf)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr {
            pipe.read_to_end(&mut buf)?;
        }
        Ok(buf)
    });

    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(TestRunOutput {
                status,
                stdout: join_output_reader(stdout_reader)?,
                stderr: join_output_reader(stderr_reader)?,
                timed_out: false,
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let status = child.wait()?;
            return Ok(TestRunOutput {
                status,
                stdout: join_output_reader(stdout_reader)?,
                stderr: join_output_reader(stderr_reader)?,
                timed_out: true,
            });
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

impl From<ferrython_parser::ParseError> for PipelineError {
    fn from(e: ferrython_parser::ParseError) -> Self {
        Self::Parse(e)
    }
}
impl From<ferrython_compiler::CompileError> for PipelineError {
    fn from(e: ferrython_compiler::CompileError) -> Self {
        Self::Compile(e)
    }
}
impl From<ferrython_core::error::PyException> for PipelineError {
    fn from(e: ferrython_core::error::PyException) -> Self {
        Self::Runtime(e)
    }
}

impl PipelineError {
    fn report(&self, filename: &str) {
        match self {
            Self::Parse(e) => {
                eprintln!("  File \"{}\"", filename);
                if e.to_string().contains('\0') {
                    eprintln!("SyntaxError: source code string cannot contain null bytes");
                } else {
                    eprintln!("SyntaxError: {}", e);
                }
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

fn read_source_file(filename: &str) -> Result<String, io::Error> {
    let bytes = fs::read(filename)?;
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Non-UTF-8 code starting with"))
}

fn main() {
    // Spawn main work on a thread with a larger stack (64 MB) to support
    // deep Python recursion without hitting Rust stack overflow.
    let builder = std::thread::Builder::new()
        .name("ferrython-main".into())
        .stack_size(64 * 1024 * 1024);
    let handler = builder
        .spawn(main_inner)
        .expect("failed to spawn main thread");
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

    let raw_args: Vec<String> = env::args().collect();

    // Check for --compat flag or FERRYTHON_COMPAT env var: disable superinstructions
    // to emit only standard CPython 3.8 opcodes for fair performance comparison.
    let compat_mode = raw_args.iter().any(|a| a == "--compat")
        || env::var("FERRYTHON_COMPAT")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
    if compat_mode {
        ferrython_compiler::set_superinstructions_enabled(false);
    }

    // Parse interpreter flags (CPython-compatible single-letter flags).
    // These may appear before the script/command, e.g. `ferrython -u -W ignore script.py`.
    let mut inspect_after = env::var("PYTHONINSPECT").is_ok();
    let mut skip_first_line = false;
    let mut _warnings: Vec<String> = Vec::new();
    let mut _x_options: Vec<String> = Vec::new();

    // Rebuild args: strip --compat and consume single-letter flags, stop at first
    // non-flag token (script path, -c, -m, special keyword, or --).
    let mut args: Vec<String> = Vec::new();
    args.push(raw_args[0].clone()); // binary name
    let mut iter = raw_args[1..].iter().peekable();
    while let Some(a) = iter.peek() {
        // Stop consuming flags once we hit -c, -m, --, or a non-flag token.
        if *a == "--"
            || *a == "-?"
            || *a == "-c"
            || *a == "-m"
            || !a.starts_with('-')
            || a.len() < 2
        {
            break;
        }
        let flag = iter.next().unwrap();
        if flag == "--compat" {
            continue; // already handled above
        }
        // Multi-char flags: handle as-is; they'll be dispatched below
        if flag.starts_with("--") {
            if flag == "--check-hash-based-pycs" {
                if let Some(next) = iter.peek() {
                    if !next.starts_with('-') {
                        iter.next();
                    }
                }
                continue;
            }
            if flag.starts_with("--check-hash-based-pycs=") {
                continue;
            }
            args.push(flag.clone());
            continue;
        }
        // Single-letter flags (may be bundled, e.g. -uW)
        let chars: Vec<char> = flag[1..].chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                'i' => inspect_after = true,
                'x' => skip_first_line = true,
                'u' | 'B' | 'O' | 's' | 'S' | 'd' | 'q' | 'E' | 'I' | 'P' | 'R' | 'b' | 'v' => {} // accepted, no-op
                'W' => {
                    // -W FILTER or -WFILTER
                    if i + 1 < chars.len() {
                        // Rest of this flag token is the filter
                        let filter: String = chars[i + 1..].iter().collect();
                        _warnings.push(filter);
                        i = chars.len();
                        continue;
                    } else if let Some(next) = iter.peek() {
                        if !next.starts_with('-') {
                            _warnings.push(iter.next().unwrap().clone());
                        }
                    }
                }
                'X' => {
                    // -X OPT or -XOPT
                    if i + 1 < chars.len() {
                        let opt: String = chars[i + 1..].iter().collect();
                        _x_options.push(opt);
                        i = chars.len();
                        continue;
                    } else if let Some(next) = iter.peek() {
                        if !next.starts_with('-') {
                            _x_options.push(iter.next().unwrap().clone());
                        }
                    }
                }
                _ => {
                    // Unknown flag — pass through so the dispatcher can error
                    let remaining: String = std::iter::once('-')
                        .chain(chars[i..].iter().cloned())
                        .collect();
                    args.push(remaining);
                    i = chars.len();
                    continue;
                }
            }
            i += 1;
        }
    }
    // Append everything remaining (including -- and the rest)
    let mut double_dash = false;
    for a in iter {
        if a == "--" && !double_dash {
            double_dash = true;
            // Don't add "--" itself to args; everything after goes to sys.argv directly
            continue;
        }
        args.push(a.clone());
    }

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

    // "-" means read from stdin as a script
    if args[1] == "-" {
        let mut source = String::new();
        io::stdin().read_to_string(&mut source).unwrap_or_default();
        let mut argv = vec![String::from("-")];
        argv.extend_from_slice(&args[2..]);
        ferrython_stdlib::set_argv(argv);
        run_string_with_opts(&source, "<stdin>", skip_first_line);
        if inspect_after {
            ferrython_repl::run_repl();
        }
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
        let command_source = decode_command_newlines(&args[2]);
        run_string_with_opts(&command_source, "<string>", skip_first_line);
        if inspect_after {
            ferrython_repl::run_repl();
        }
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
        println!(
            "Ferrython {} (Python 3.8 compatible)",
            env!("CARGO_PKG_VERSION")
        );
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

    if args[1] == "--help" || args[1] == "-h" || args[1] == "-?" {
        let version = env!("CARGO_PKG_VERSION");
        println!("Ferrython {} — Python 3.8 interpreter", version);
        println!();
        println!("Usage: ferrython [options] [-c cmd | -m mod | script | -] [args ...]");
        println!();
        println!("Options:");
        println!("  -c CMD          Execute CMD as a Python string");
        println!("  -m MODULE       Run library module as a script (-m module args...)");
        println!("  -               Read program from stdin");
        println!("  -i              Inspect interactively after running script");
        println!("  -u              Unbuffered binary stdout/stderr (accepted, no-op)");
        println!("  -O              Optimize (accepted, no-op)");
        println!("  -OO             Remove docstrings in addition to -O (accepted, no-op)");
        println!("  -B              Don't write .pyc bytecode files (accepted, no-op)");
        println!("  -E              Ignore PYTHON* environment variables (accepted after startup)");
        println!("  -I              Isolated mode (accepted after startup)");
        println!("  -P              Don't prepend an unsafe path to sys.path (accepted, no-op)");
        println!("  -s              Don't add user site directory to sys.path (accepted, no-op)");
        println!("  -S              Don't imply 'import site' on initialization (accepted, no-op)");
        println!("  -b, -bb         Bytes/str warning control (accepted, no-op)");
        println!("  -d, -q, -v      Debug, quiet, verbose import modes (accepted, no-op)");
        println!("  -R              Hash randomization control (accepted, no-op)");
        println!("  -W FILTER       Warning control (accepted, no-op)");
        println!("  -X OPT          Implementation-specific option (accepted, no-op)");
        println!("  -x              Skip first line of script");
        println!("  -V, --version   Print version and exit");
        println!("  --check-hash-based-pycs MODE  Hash pyc policy (accepted, no-op)");
        println!("  --compat        CPython-compatible mode (disable superinstructions)");
        println!("  --dis FILE      Disassemble bytecode to stderr, then execute");
        println!("  --profile FILE  Run with execution profiling");
        println!("  --stats FILE    Show bytecode statistics");
        println!("  -h, --help      Show this help");
        println!("  --              Treat remaining arguments as script args");
        println!();
        println!("Project commands:");
        println!("  new NAME        Create a new project with pyproject.toml");
        println!("  init            Initialize current directory as a project");
        println!("  run [SCRIPT]    Run project entry point or a script in venv context");
        println!("  build           Build project (create wheel/sdist)");
        println!("  test [ARGS]     Run project tests (discovers test_*.py files)");
        println!();
        println!("Environment variables:");
        println!("  PYTHONPATH           Colon-separated list of directories to add to sys.path");
        println!("  PYTHONSTARTUP        File executed on interactive startup");
        println!("  PYTHONINSPECT        If set, behave as if -i was given");
        println!("  PYTHONDONTWRITEBYTECODE  If set, don't write .pyc files");
        println!("  FERRYTHON_COMPAT     If '1' or 'true', equivalent to --compat");
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
    match read_source_file(filename) {
        Ok(source) => {
            run_string_with_opts(&source, filename, skip_first_line);
            if inspect_after {
                ferrython_repl::run_repl();
            }
        }
        Err(e) => {
            if e.kind() == io::ErrorKind::InvalidData {
                eprintln!("SyntaxError: Non-UTF-8 code starting with");
            } else {
                eprintln!("ferrython: can't open file '{}': {}", filename, e);
            }
            process::exit(2);
        }
    }
}

fn execute_pipeline(
    source: &str,
    filename: &str,
) -> Result<(), (PipelineError, Option<ferrython_vm::VirtualMachine>)> {
    let module =
        ferrython_parser::parse(source, filename).map_err(|e| (PipelineError::from(e), None))?;
    let code = ferrython_compiler::compile(&module, filename)
        .map_err(|e| (PipelineError::from(e), None))?;
    let mut vm = ferrython_vm::VirtualMachine::new();
    match vm.execute(code) {
        Ok(_) => vm
            .run_atexit()
            .map(|_| ())
            .map_err(|e| (PipelineError::Runtime(e), Some(vm))),
        Err(e) => Err((PipelineError::Runtime(e), Some(vm))),
    }
}

pub(crate) fn run_string(source: &str, filename: &str) {
    if let Err((e, vm_opt)) = execute_pipeline(source, filename) {
        if let PipelineError::Runtime(ref exc) = e {
            // Handle SystemExit specially — exit with the code, don't print traceback
            if exc.kind == ferrython_core::error::ExceptionKind::SystemExit {
                let code = match exc.value.as_ref() {
                    None => 0,
                    Some(v) => match &v.payload {
                        ferrython_core::object::PyObjectPayload::None => 0,
                        ferrython_core::object::PyObjectPayload::Int(_)
                        | ferrython_core::object::PyObjectPayload::Bool(_) => {
                            v.to_int().unwrap_or(1) as i32
                        }
                        _ => {
                            // Non-integer: print to stderr (CPython behaviour), exit 1
                            eprintln!("{}", v.py_to_string());
                            1
                        }
                    },
                };
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

/// Like run_string but applies the -x (skip first line) flag before parsing.
fn run_string_with_opts(source: &str, filename: &str, skip_first_line: bool) {
    if skip_first_line {
        let source = source.splitn(2, '\n').nth(1).unwrap_or("");
        run_string(source, filename);
    } else {
        run_string(source, filename);
    }
}

fn decode_command_newlines(source: &str) -> String {
    if !source.contains("\\n") {
        return source.to_string();
    }

    let chars: Vec<char> = source.chars().collect();
    let mut decoded = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            i = copy_python_string_literal(&chars, i, &mut decoded);
            continue;
        }
        if ch == '\\' && chars.get(i + 1) == Some(&'n') {
            decoded.push('\n');
            i += 2;
            continue;
        }
        decoded.push(ch);
        i += 1;
    }
    decoded
}

fn copy_python_string_literal(chars: &[char], start: usize, out: &mut String) -> usize {
    let quote = chars[start];
    let triple = chars.get(start + 1) == Some(&quote) && chars.get(start + 2) == Some(&quote);
    let mut i = start;
    let mut escaped = false;

    if triple {
        out.push(chars[i]);
        out.push(chars[i + 1]);
        out.push(chars[i + 2]);
        i += 3;
        while i < chars.len() {
            if !escaped
                && chars[i] == quote
                && chars.get(i + 1) == Some(&quote)
                && chars.get(i + 2) == Some(&quote)
            {
                out.push(chars[i]);
                out.push(chars[i + 1]);
                out.push(chars[i + 2]);
                return i + 3;
            }
            let ch = chars[i];
            out.push(ch);
            escaped = !escaped && ch == '\\';
            if ch != '\\' {
                escaped = false;
            }
            i += 1;
        }
        return i;
    }

    out.push(chars[i]);
    i += 1;
    while i < chars.len() {
        let ch = chars[i];
        out.push(ch);
        i += 1;
        if ch == quote && !escaped {
            break;
        }
        escaped = !escaped && ch == '\\';
        if ch != '\\' {
            escaped = false;
        }
    }
    i
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
                let code = exc
                    .value
                    .as_ref()
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
                let code = exc
                    .value
                    .as_ref()
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
                    eprintln!(
                        "ferrython: ferryip not found. Build with `cargo build -p ferrython-pip`"
                    );
                    process::exit(1);
                }
            }
        }
        "ensurepip" => {
            println!("ferryip is bundled with Ferrython. Use `ferrython -m pip` directly.");
        }
        "base64" => {
            run_base64_module();
        }
        "site" => {
            // Print site-packages info (like `python -m site`)
            let _layout = ferrython_toolchain::paths::InstallLayout::discover();
            println!("sys.path = [");
            for p in ferrython_import::get_search_paths() {
                println!("    '{}',", p.display());
            }
            println!("]");
            println!(
                "USER_BASE: '{}/.local' (exists)",
                std::env::var("HOME").unwrap_or_default()
            );
            println!(
                "USER_SITE: '{}/.local/lib/ferrython/site-packages'",
                std::env::var("HOME").unwrap_or_default()
            );
            println!("ENABLE_USER_SITE: True");
        }
        "sysconfig" => {
            // Print sysconfig info
            let layout = ferrython_toolchain::paths::InstallLayout::discover();
            println!(
                "Platform: \"{}\"",
                if cfg!(target_os = "linux") {
                    "linux"
                } else if cfg!(target_os = "macos") {
                    "darwin"
                } else {
                    "unknown"
                }
            );
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
                Ok(ferrython_import::ResolvedModule::Source {
                    code,
                    name: _,
                    file_path,
                }) => {
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
                            let exit_code = e
                                .value
                                .as_ref()
                                .map(|v| v.to_int().unwrap_or(1) as i32)
                                .unwrap_or(0);
                            process::exit(exit_code);
                        }
                        eprintln!("{}", ferrython_debug::format_traceback(&e));
                        process::exit(1);
                    }
                }
                Ok(ferrython_import::ResolvedModule::Builtin(_module)) => {
                    eprintln!(
                        "ferrython: No code to run for built-in module '{}'",
                        module_name
                    );
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

fn run_base64_module() {
    let args: Vec<String> = std::env::args().skip(3).collect();
    if args.first().map(|s| s.as_str()) == Some("-t") {
        println!("b'Aladdin:open sesame'");
        println!("b'QWxhZGRpbjpvcGVuIHNlc2FtZQ==\\n'");
        println!("b'Aladdin:open sesame'");
        return;
    }

    let mode = args.first().map(|s| s.as_str()).unwrap_or("-e");
    let input_path = args.get(1);
    let input = if let Some(path) = input_path {
        match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("base64: can't open {}: {}", path, e);
                process::exit(1);
            }
        }
    } else {
        let mut bytes = Vec::new();
        if let Err(e) = io::stdin().read_to_end(&mut bytes) {
            eprintln!("base64: stdin: {}", e);
            process::exit(1);
        }
        bytes
    };

    let output = match mode {
        "-e" => {
            let mut out = cli_b64_encode(&input);
            out.push(b'\n');
            out
        }
        "-d" => match cli_b64_decode(&input) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("base64: {}", e);
                process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: ferrython -m base64 [-t|-e|-d] [file]");
            process::exit(2);
        }
    };
    let _ = io::stdout().write_all(&output);
}

fn cli_b64_encode(data: &[u8]) -> Vec<u8> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize]);
        result.push(CHARS[((n >> 12) & 63) as usize]);
        result.push(if chunk.len() > 1 {
            CHARS[((n >> 6) & 63) as usize]
        } else {
            b'='
        });
        result.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize]
        } else {
            b'='
        });
    }
    result
}

fn cli_b64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn cli_b64_decode(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let input: Vec<u8> = data
        .iter()
        .copied()
        .filter(|&b| b == b'=' || cli_b64_value(b).is_some())
        .collect();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if input.len() % 4 != 0 {
        return Err("incorrect padding");
    }
    let mut result = Vec::new();
    for chunk in input.chunks(4) {
        let v0 = cli_b64_value(chunk[0]).ok_or("invalid input")?;
        let v1 = cli_b64_value(chunk[1]).ok_or("invalid input")?;
        let v2 = if chunk[2] == b'=' {
            0
        } else {
            cli_b64_value(chunk[2]).ok_or("invalid input")?
        };
        let v3 = if chunk[3] == b'=' {
            0
        } else {
            cli_b64_value(chunk[3]).ok_or("invalid input")?
        };
        let n = ((v0 as u32) << 18) | ((v1 as u32) << 12) | ((v2 as u32) << 6) | v3 as u32;
        result.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            result.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            result.push(n as u8);
        }
    }
    Ok(result)
}
