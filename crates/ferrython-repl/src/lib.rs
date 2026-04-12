//! Ferrython interactive REPL.
//!
//! Features:
//! - Persistent command history (~/.ferrython_history)
//! - Arrow key navigation, Ctrl+R reverse search
//! - Multi-line input (blocks, parentheses, brackets)
//! - Syntax-aware prompt (>>> / ...)
//! - Tab completion for builtins and globals
//! - ANSI color output for errors and prompts
//! - Special commands: exit(), quit(), help(), clear

use std::borrow::Cow;
use std::rc::Rc;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Config, Editor, Helper};

use compact_str::CompactString;
use ferrython_core::object::{PyObject, PyObjectRef};
use ferrython_core::types::SharedGlobals;

/// Python keyword and builtin lists for completion.
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await",
    "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try",
    "while", "with", "yield",
];

const PYTHON_BUILTINS: &[&str] = &[
    "abs", "all", "any", "ascii", "bin", "bool", "breakpoint", "bytearray",
    "bytes", "callable", "chr", "classmethod", "compile", "complex",
    "delattr", "dict", "dir", "divmod", "enumerate", "eval", "exec",
    "filter", "float", "format", "frozenset", "getattr", "globals",
    "hasattr", "hash", "help", "hex", "id", "input", "int", "isinstance",
    "issubclass", "iter", "len", "list", "locals", "map", "max",
    "memoryview", "min", "next", "object", "oct", "open", "ord", "pow",
    "print", "property", "range", "repr", "reversed", "round", "set",
    "setattr", "slice", "sorted", "staticmethod", "str", "sum", "super",
    "tuple", "type", "vars", "zip", "__import__",
];

/// REPL helper providing completion, validation, and highlighting.
struct FerryHelper {
    globals: SharedGlobals,
}

impl Completer for FerryHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Find the start of the word being completed
        let start = line[..pos]
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .map(|i| i + 1)
            .unwrap_or(0);
        let prefix = &line[start..pos];

        if prefix.is_empty() {
            return Ok((start, Vec::new()));
        }

        let mut candidates = Vec::new();

        // Check if completing an attribute (obj.attr)
        if let Some(dot_pos) = prefix.rfind('.') {
            let _obj_name = &prefix[..dot_pos];
            let attr_prefix = &prefix[dot_pos + 1..];
            // Try to get attributes from the object in globals
            let globals = self.globals.read();
            if let Some(obj) = globals.get(_obj_name) {
                // Get dir() of the object
                if let Some(dir_list) = get_object_dir(obj) {
                    for attr in dir_list {
                        if attr.starts_with(attr_prefix) {
                            candidates.push(Pair {
                                display: attr.clone(),
                                replacement: format!("{}.{}", _obj_name, attr),
                            });
                        }
                    }
                }
            }
            // Also try module attributes
            return Ok((start, candidates));
        }

        // Complete keywords
        for kw in PYTHON_KEYWORDS {
            if kw.starts_with(prefix) {
                candidates.push(Pair {
                    display: kw.to_string(),
                    replacement: kw.to_string(),
                });
            }
        }

        // Complete builtins
        for bi in PYTHON_BUILTINS {
            if bi.starts_with(prefix) {
                candidates.push(Pair {
                    display: bi.to_string(),
                    replacement: bi.to_string(),
                });
            }
        }

        // Complete from current globals
        let globals = self.globals.read();
        for key in globals.keys() {
            if key.starts_with(prefix) && !candidates.iter().any(|c| c.display == key.as_str()) {
                candidates.push(Pair {
                    display: key.to_string(),
                    replacement: key.to_string(),
                });
            }
        }

        Ok((start, candidates))
    }
}

impl Highlighter for FerryHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if prompt.starts_with(">>>") {
            Cow::Owned(format!("\x1b[1;32m{}\x1b[0m", prompt))
        } else {
            Cow::Owned(format!("\x1b[1;33m{}\x1b[0m", prompt))
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint))
    }
}

impl Hinter for FerryHelper {
    type Hint = String;
}

impl Validator for FerryHelper {
    fn validate(&self, ctx: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();

        // Check for unclosed brackets/parens/strings
        let mut parens = 0i32;
        let mut brackets = 0i32;
        let mut braces = 0i32;
        let mut in_string = false;
        let mut string_char = ' ';
        let mut escape = false;
        let mut in_triple = false;

        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if escape {
                escape = false;
                i += 1;
                continue;
            }
            if c == '\\' && in_string {
                escape = true;
                i += 1;
                continue;
            }
            if !in_string {
                if (c == '"' || c == '\'') && i + 2 < chars.len() && chars[i+1] == c && chars[i+2] == c {
                    in_string = true;
                    string_char = c;
                    in_triple = true;
                    i += 3;
                    continue;
                }
                if c == '"' || c == '\'' {
                    in_string = true;
                    string_char = c;
                    in_triple = false;
                    i += 1;
                    continue;
                }
                if c == '#' {
                    // Skip to end of line
                    while i < chars.len() && chars[i] != '\n' { i += 1; }
                    continue;
                }
                match c {
                    '(' => parens += 1,
                    ')' => parens -= 1,
                    '[' => brackets += 1,
                    ']' => brackets -= 1,
                    '{' => braces += 1,
                    '}' => braces -= 1,
                    _ => {}
                }
            } else {
                if in_triple {
                    if c == string_char && i + 2 < chars.len() && chars[i+1] == string_char && chars[i+2] == string_char {
                        in_string = false;
                        i += 3;
                        continue;
                    }
                } else if c == string_char {
                    in_string = false;
                }
            }
            i += 1;
        }

        // Unclosed string or brackets
        if in_string || parens > 0 || brackets > 0 || braces > 0 {
            return Ok(ValidationResult::Incomplete);
        }

        // Backslash continuation
        if input.trim_end().ends_with('\\') {
            return Ok(ValidationResult::Incomplete);
        }

        // Block detection: if any line ends with ':', we're in a block.
        // Block is complete when the last line is empty (user pressed Enter at ... prompt).
        let lines: Vec<&str> = input.split('\n').collect();
        let has_block_starter = lines.iter().any(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && t.ends_with(':')
        });
        let has_decorator = lines.first().map_or(false, |l| l.trim().starts_with('@'));

        if has_block_starter || has_decorator {
            // Block is open — need empty line to terminate
            if let Some(last) = lines.last() {
                if !last.trim().is_empty() {
                    return Ok(ValidationResult::Incomplete);
                }
            } else {
                return Ok(ValidationResult::Incomplete);
            }
        }

        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for FerryHelper {}

/// Get directory listing of an object (simplified).
fn get_object_dir(obj: &PyObjectRef) -> Option<Vec<String>> {
    use ferrython_core::object::PyObjectPayload;
    match &obj.payload {
        PyObjectPayload::Module(md) => {
            let attrs = md.attrs.read();
            Some(attrs.keys().map(|k: &CompactString| k.to_string()).collect())
        }
        PyObjectPayload::Instance(inst) => {
            let mut names: Vec<String> = inst.attrs.read().keys().map(|k: &CompactString| k.to_string()).collect();
            // Add class methods
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                for key in cd.namespace.read().keys() {
                    let s = key.to_string();
                    if !names.contains(&s) {
                        names.push(s);
                    }
                }
            }
            Some(names)
        }
        PyObjectPayload::Class(cd) => {
            Some(cd.namespace.read().keys().map(|k: &CompactString| k.to_string()).collect())
        }
        _ => None,
    }
}

/// Run the interactive REPL.
pub fn run_repl() {
    let version = env!("CARGO_PKG_VERSION");
    println!("Ferrython {} (Python 3.12 compatible)", version);
    println!("Type \"help()\" for help, \"exit()\" or Ctrl+D to exit.");

    let mut vm = ferrython_vm::VirtualMachine::new();
    let globals = ferrython_vm::VirtualMachine::new_globals();

    // Initialize _ = None in globals
    globals.write().insert(
        CompactString::from("_"),
        PyObject::none(),
    );

    // Configure rustyline
    let config = Config::builder()
        .max_history_size(10_000)
        .expect("valid history size")
        .auto_add_history(true)
        .tab_stop(4)
        .build();

    let helper = FerryHelper {
        globals: globals.clone(),
    };

    let mut rl: Editor<FerryHelper, DefaultHistory> = Editor::with_config(config)
        .expect("Failed to create line editor");
    rl.set_helper(Some(helper));

    // Load history
    let history_path = dirs_history_path();
    let _ = rl.load_history(&history_path);

    loop {
        let prompt = ">>> ";

        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    continue;
                }
                if trimmed == "exit()" || trimmed == "quit()" {
                    break;
                }
                if trimmed == "clear" || trimmed == "clear()" {
                    print!("\x1b[2J\x1b[H");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    continue;
                }

                // Rustyline Validator handles multi-line blocks:
                // it returns the full accumulated input (with newlines) only when complete.
                execute_source(&mut vm, &globals, &line);
            }
            Err(ReadlineError::Interrupted) => {
                eprintln!("KeyboardInterrupt");
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    // Save history
    let _ = rl.save_history(&history_path);
    println!();
}

/// Execute a source string in the VM.
fn execute_source(
    vm: &mut ferrython_vm::VirtualMachine,
    globals: &SharedGlobals,
    source: &str,
) {
    match ferrython_parser::parse(source, "<stdin>") {
        Ok(module) => {
            match ferrython_compiler::compile_interactive(&module, "<stdin>") {
                Ok(code) => {
                    match vm.execute_with_globals(Rc::new(code), globals.clone()) {
                        Ok(_) => {}
                        Err(e) => {
                            let tb = ferrython_debug::format_traceback(&e);
                            eprintln!("\x1b[31m{}\x1b[0m", tb);
                        }
                    }
                    ferrython_core::error::clear_thread_exc_info();
                }
                Err(e) => eprintln!("\x1b[31mCompileError: {}\x1b[0m", e),
            }
        }
        Err(e) => eprintln!("\x1b[31mSyntaxError: {}\x1b[0m", e),
    }
}

/// Get the history file path.
fn dirs_history_path() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let mut path = std::path::PathBuf::from(home);
        path.push(".ferrython_history");
        path.to_string_lossy().to_string()
    } else {
        ".ferrython_history".to_string()
    }
}
