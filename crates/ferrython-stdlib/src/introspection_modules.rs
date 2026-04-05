//! Introspection stdlib modules (warnings, traceback, inspect, dis)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── subprocess module (basic) ──


pub fn create_warnings_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicBool, Ordering};
    use parking_lot::RwLock;

    // Global recording state: when catch_warnings(record=True) is active,
    // warn() appends to this list instead of printing to stderr.
    static RECORDING: AtomicBool = AtomicBool::new(false);
    static RECORD_LIST: std::sync::LazyLock<RwLock<Option<PyObjectRef>>> =
        std::sync::LazyLock::new(|| RwLock::new(None));

    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let message = args[0].py_to_string();
        let category = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            let cat = &args[1];
            if let PyObjectPayload::Class(cd) = &cat.payload {
                cd.name.to_string()
            } else {
                cat.py_to_string()
            }
        } else {
            "UserWarning".to_string()
        };

        if RECORDING.load(Ordering::Relaxed) {
            // Recording mode: append a WarningMessage-like object to the list
            let guard = RECORD_LIST.read();
            if let Some(ref list_obj) = *guard {
                let cls = PyObject::class(CompactString::from("WarningMessage"), vec![], IndexMap::new());
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("message"), args[0].clone());
                attrs.insert(CompactString::from("category"), PyObject::str_val(CompactString::from(&category)));
                attrs.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from("<stdin>")));
                attrs.insert(CompactString::from("lineno"), PyObject::int(1));
                let warning_obj = PyObject::instance_with_attrs(cls, attrs);
                if let PyObjectPayload::List(items) = &list_obj.payload {
                    items.write().push(warning_obj);
                }
            }
        } else {
            eprintln!("<stdin>:1: {}: {}", category, message);
        }
        Ok(PyObject::none())
    });

    // filterwarnings / simplefilter stubs
    let filter_warnings_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));
    let simple_filter_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    // catch_warnings(record=False)
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        let record = if !args.is_empty() { args[0].is_truthy() } else { false };

        let cls = PyObject::class(CompactString::from("catch_warnings"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(CompactString::from("_warnings"), warning_list.clone());

        if record {
            let wl = warning_list.clone();
            let enter_list = warning_list.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "catch_warnings.__enter__", move |_args: &[PyObjectRef]| {
                    // Activate recording mode and set the shared list
                    RECORDING.store(true, Ordering::Relaxed);
                    *RECORD_LIST.write() = Some(enter_list.clone());
                    Ok(wl.clone())
                }
            ));
        } else {
            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "catch_warnings.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));
        }

        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "catch_warnings.__exit__", |_args: &[PyObjectRef]| {
                // Deactivate recording mode
                RECORDING.store(false, Ordering::Relaxed);
                *RECORD_LIST.write() = None;
                Ok(PyObject::bool_val(false))
            }
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("warnings", vec![
        ("warn", warn_fn),
        ("filterwarnings", filter_warnings_fn),
        ("simplefilter", simple_filter_fn),
        ("resetwarnings", make_builtin(|_| Ok(PyObject::none()))),
        ("catch_warnings", catch_warnings_fn),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    // format_exc() — return formatted exception string using active exception info
    let format_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        if let Some((kind, msg)) = crate::sys_modules::get_exc_info() {
            let type_name = format!("{:?}", kind);
            Ok(PyObject::str_val(CompactString::from(format!(
                "Traceback (most recent call last):\n  File \"<stdin>\", line 1, in <module>\n{}: {}\n",
                type_name, msg
            ))))
        } else {
            Ok(PyObject::str_val(CompactString::from("NoneType: None\n")))
        }
    });

    // format_exception(etype, value, tb) — format exception into list of strings
    let format_exception_fn = make_builtin(|args: &[PyObjectRef]| {
        let mut lines = Vec::new();
        if args.len() >= 2 {
            let etype = &args[0];
            let value = &args[1];
            let type_name = if let PyObjectPayload::Class(cd) = &etype.payload {
                cd.name.to_string()
            } else if let PyObjectPayload::ExceptionType(kind) = &etype.payload {
                format!("{:?}", kind)
            } else {
                etype.py_to_string()
            };
            let msg = value.py_to_string();
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
                lines.push(PyObject::str_val(CompactString::from("Traceback (most recent call last):\n")));
                lines.push(PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n")));
            }
            lines.push(PyObject::str_val(CompactString::from(
                format!("{}: {}\n", type_name, msg)
            )));
        }
        Ok(PyObject::list(lines))
    });

    // print_exc() — print exception info to stderr
    let print_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("NoneType: None");
        Ok(PyObject::none())
    });

    // format_tb(tb) — format traceback entries as list of strings
    let format_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return a basic traceback entry
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n"))
        ]))
    });

    // extract_tb(tb) — extract FrameSummary-like tuples from traceback
    let extract_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return list of (filename, lineno, name, line) tuples
        Ok(PyObject::list(vec![
            PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("<unknown>")),
                PyObject::int(0),
                PyObject::str_val(CompactString::from("<module>")),
                PyObject::none(),
            ])
        ]))
    });

    make_module("traceback", vec![
        ("format_exc", format_exc_fn),
        ("print_exc", print_exc_fn),
        ("format_exception", format_exception_fn),
        ("print_stack", make_builtin(|_| Ok(PyObject::none()))),
        ("format_tb", format_tb_fn),
        ("extract_tb", extract_tb_fn),
    ])
}

// ── warnings module (stub) ──


pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Class(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("isgenerator", make_builtin(|args| {
            check_args("inspect.isgenerator", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Generator(_))))
        })),
        ("isgeneratorfunction", make_builtin(|args| {
            check_args("inspect.isgeneratorfunction", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                Ok(PyObject::bool_val(f.code.flags.contains(ferrython_bytecode::code::CodeFlags::GENERATOR)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        })),
        ("iscoroutine", make_builtin(|args| {
            check_args("inspect.iscoroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Coroutine(_))))
        })),
        ("iscoroutinefunction", make_builtin(|args| {
            check_args("inspect.iscoroutinefunction", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                Ok(PyObject::bool_val(f.code.flags.contains(ferrython_bytecode::code::CodeFlags::COROUTINE)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        })),
        ("isroutine", make_builtin(|args| {
            check_args("inspect.isroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Function(_) | PyObjectPayload::BoundMethod { .. } |
                PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } |
                PyObjectPayload::BuiltinBoundMethod { .. })))
        })),
        ("isabstract", make_builtin(|args| {
            check_args("inspect.isabstract", args, 1)?;
            Ok(PyObject::bool_val(args[0].get_attr("__abstractmethods__").is_some()))
        })),
        ("getmembers", make_builtin(|args| {
            check_args_min("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let mut result = Vec::new();
            for n in &dir_names {
                if let Some(val) = args[0].get_attr(n.as_str()) {
                    // If predicate provided (args[1]), filter
                    if args.len() > 1 {
                        // Can't call VM functions from native — skip predicate filter
                    }
                    result.push(PyObject::tuple(vec![PyObject::str_val(n.clone()), val]));
                }
            }
            Ok(PyObject::list(result))
        })),
        ("getdoc", make_builtin(|args| {
            check_args("inspect.getdoc", args, 1)?;
            Ok(args[0].get_attr("__doc__").unwrap_or_else(PyObject::none))
        })),
        ("getfile", make_builtin(|args| {
            check_args("inspect.getfile", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                return Ok(PyObject::str_val(f.code.filename.clone()));
            }
            if let PyObjectPayload::Module(m) = &args[0].payload {
                if let Some(file) = m.attrs.read().get("__file__").cloned() {
                    return Ok(file);
                }
            }
            Err(PyException::type_error("could not get file for object"))
        })),
        ("getmodule", make_builtin(|args| {
            check_args("inspect.getmodule", args, 1)?;
            Ok(args[0].get_attr("__module__").unwrap_or_else(PyObject::none))
        })),
        ("signature", make_builtin(|args| {
            check_args("inspect.signature", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                let total = (f.code.arg_count + f.code.kwonlyarg_count) as usize;
                let params: Vec<String> = f.code.varnames.iter()
                    .take(total)
                    .map(|v| v.to_string())
                    .collect();
                let sig_str = format!("({})", params.join(", "));
                Ok(PyObject::str_val(CompactString::from(sig_str)))
            } else {
                Ok(PyObject::str_val(CompactString::from("(*args, **kwargs)")))
            }
        })),
        ("getfullargspec", make_builtin(|args| {
            check_args("inspect.getfullargspec", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                let ac = f.code.arg_count as usize;
                let kwc = f.code.kwonlyarg_count as usize;
                let arg_names: Vec<PyObjectRef> = f.code.varnames.iter()
                    .take(ac)
                    .map(|v| PyObject::str_val(v.clone()))
                    .collect();
                let kwonly_names: Vec<PyObjectRef> = f.code.varnames.iter()
                    .skip(ac)
                    .take(kwc)
                    .map(|v| PyObject::str_val(v.clone()))
                    .collect();
                // Return a FullArgSpec-like namedtuple as dict for simplicity
                let mut map = IndexMap::new();
                map.insert(HashableKey::Str(CompactString::from("args")), PyObject::list(arg_names));
                map.insert(HashableKey::Str(CompactString::from("varargs")), PyObject::none());
                map.insert(HashableKey::Str(CompactString::from("varkw")), PyObject::none());
                map.insert(HashableKey::Str(CompactString::from("defaults")),
                    if f.defaults.is_empty() { PyObject::none() } else { PyObject::tuple(f.defaults.clone()) });
                map.insert(HashableKey::Str(CompactString::from("kwonlyargs")), PyObject::list(kwonly_names));
                map.insert(HashableKey::Str(CompactString::from("kwonlydefaults")),
                    if f.kw_defaults.is_empty() { PyObject::none() }
                    else {
                        let mut kw_map = IndexMap::new();
                        for (k, v) in &f.kw_defaults {
                            kw_map.insert(HashableKey::Str(k.clone()), v.clone());
                        }
                        PyObject::dict(kw_map)
                    });
                map.insert(HashableKey::Str(CompactString::from("annotations")), PyObject::dict(IndexMap::new()));
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("unsupported callable"))
            }
        })),
        // Parameter and Signature classes (simplified placeholders for compatibility)
        ("Parameter", PyObject::class(CompactString::from("Parameter"), vec![], IndexMap::new())),
        ("Signature", PyObject::class(CompactString::from("Signature"), vec![], IndexMap::new())),
    ])
}

// ── dis module ──

pub fn create_dis_module() -> PyObjectRef {
    use ferrython_bytecode::code::ConstantValue;
    use ferrython_bytecode::opcode::Opcode;

    fn dis_dis(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("dis() requires a function argument"));
        }
        let obj = &args[0];
        let code: std::sync::Arc<ferrython_bytecode::CodeObject> = match &obj.payload {
            PyObjectPayload::Function(pf) => std::sync::Arc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::sync::Arc::clone(c),
            _ => return Err(PyException::type_error(
                format!("don't know how to disassemble {} objects", obj.type_name())
            )),
        };
        disassemble_code(&code, 0);
        Ok(PyObject::none())
    }

    fn disassemble_code(code: &ferrython_bytecode::CodeObject, indent: usize) {
        let pad = " ".repeat(indent);
        // Find line number for each instruction using lnotab
        let last_lineno = code.first_line_number;
        let mut line_for_offset: Vec<u32> = Vec::with_capacity(code.instructions.len());
        {
            let mut line = code.first_line_number;
            let mut lnotab_idx = 0;
            for i in 0..code.instructions.len() {
                while lnotab_idx + 1 < code.line_number_table.len() {
                    let (off, ln) = code.line_number_table[lnotab_idx];
                    if i >= off as usize {
                        line = ln;
                        lnotab_idx += 1;
                    } else {
                        break;
                    }
                }
                line_for_offset.push(line);
            }
        }

        let mut prev_line = 0u32;
        for (i, instr) in code.instructions.iter().enumerate() {
            let lineno = if i < line_for_offset.len() { line_for_offset[i] } else { last_lineno };
            let line_str = if lineno != prev_line {
                prev_line = lineno;
                format!("{:>4}", lineno)
            } else {
                "    ".to_string()
            };

            let arg_desc = format_dis_arg(code, instr.op, instr.arg);
            println!("{}{} {:>6} {:<24} {}", pad, line_str, i * 2, format!("{:?}", instr.op), arg_desc);
        }

        // Recurse into nested code objects
        for c in &code.constants {
            if let ConstantValue::Code(nested) = c {
                println!();
                println!("{}Disassembly of <code object {} at ...>:", pad, nested.name);
                disassemble_code(nested, indent + 2);
            }
        }
    }

    fn format_dis_arg(code: &ferrython_bytecode::CodeObject, op: Opcode, arg: u32) -> String {
        match op {
            Opcode::LoadConst => {
                if let Some(c) = code.constants.get(arg as usize) {
                    match c {
                        ConstantValue::Str(s) => format!("{:<4} ('{}')", arg, if s.len() > 30 { &s[..27] } else { s }),
                        ConstantValue::Integer(n) => format!("{:<4} ({})", arg, n),
                        ConstantValue::Float(f) => format!("{:<4} ({})", arg, f),
                        ConstantValue::None => format!("{:<4} (None)", arg),
                        ConstantValue::Bool(b) => format!("{:<4} ({})", arg, b),
                        ConstantValue::Code(c) => format!("{:<4} (<code object {}>)", arg, c.name),
                        ConstantValue::Tuple(t) => format!("{:<4} (tuple/{})", arg, t.len()),
                        _ => format!("{}", arg),
                    }
                } else { format!("{}", arg) }
            }
            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
            | Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
            | Opcode::ImportName | Opcode::ImportFrom => {
                if let Some(n) = code.names.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else { format!("{}", arg) }
            }
            Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast => {
                if let Some(n) = code.varnames.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else { format!("{}", arg) }
            }
            Opcode::LoadDeref | Opcode::StoreDeref | Opcode::LoadClosure => {
                let nc = code.cellvars.len();
                let idx = arg as usize;
                if idx < nc {
                    code.cellvars.get(idx).map_or(format!("{}", arg), |n| format!("{:<4} (cell: {})", arg, n))
                } else {
                    code.freevars.get(idx - nc).map_or(format!("{}", arg), |n| format!("{:<4} (free: {})", arg, n))
                }
            }
            Opcode::CompareOp => {
                let op_name = match arg {
                    0 => "<", 1 => "<=", 2 => "==", 3 => "!=", 4 => ">", 5 => ">=",
                    6 => "in", 7 => "not in", 8 => "is", 9 => "is not",
                    10 => "exception match", _ => "?",
                };
                format!("{:<4} ({})", arg, op_name)
            }
            Opcode::JumpAbsolute | Opcode::JumpForward
            | Opcode::PopJumpIfTrue | Opcode::PopJumpIfFalse
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::SetupExcept | Opcode::SetupFinally
            | Opcode::ForIter => {
                format!("{:<4} (to {})", arg, arg)
            }
            _ => {
                if arg != 0 { format!("{}", arg) } else { String::new() }
            }
        }
    }

    make_module("dis", vec![
        ("dis", make_builtin(dis_dis)),
        ("disassemble", make_builtin(dis_dis)),
    ])
}

// ── ast module ──

pub fn create_ast_module() -> PyObjectRef {
    // Basic AST module — provides parse() and dump() for introspection
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.parse() requires source code argument"));
        }
        let _source = args[0].py_to_string();
        // Create a Module AST node (simplified)
        let cls = PyObject::class(CompactString::from("Module"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("body"), PyObject::list(vec![]));
            w.insert(CompactString::from("type_ignores"), PyObject::list(vec![]));
            w.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("body")),
                PyObject::str_val(CompactString::from("type_ignores")),
            ]));
        }
        Ok(inst)
    });

    let dump_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.dump() requires node argument"));
        }
        // Simple dump — show the type name and fields
        let type_name = args[0].type_name();
        Ok(PyObject::str_val(CompactString::from(format!("{}()", type_name))))
    });

    let literal_eval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.literal_eval() requires string argument"));
        }
        let s = args[0].py_to_string();
        let trimmed = s.trim();
        // Handle basic literals
        if trimmed == "None" { return Ok(PyObject::none()); }
        if trimmed == "True" { return Ok(PyObject::bool_val(true)); }
        if trimmed == "False" { return Ok(PyObject::bool_val(false)); }
        if let Ok(n) = trimmed.parse::<i64>() { return Ok(PyObject::int(n)); }
        if let Ok(f) = trimmed.parse::<f64>() { return Ok(PyObject::float(f)); }
        if (trimmed.starts_with('"') && trimmed.ends_with('"')) || (trimmed.starts_with('\'') && trimmed.ends_with('\'')) {
            return Ok(PyObject::str_val(CompactString::from(&trimmed[1..trimmed.len()-1])));
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Simple list literal parsing
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let items: Vec<PyObjectRef> = inner.split(',')
                .map(|s| {
                    let s = s.trim();
                    if let Ok(n) = s.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = s.parse::<f64>() { PyObject::float(f) }
                    else if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
                        PyObject::str_val(CompactString::from(&s[1..s.len()-1]))
                    } else {
                        PyObject::str_val(CompactString::from(s))
                    }
                }).collect();
            return Ok(PyObject::list(items));
        }
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::tuple(vec![]));
            }
            let items: Vec<PyObjectRef> = inner.split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    let s = s.trim();
                    if let Ok(n) = s.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = s.parse::<f64>() { PyObject::float(f) }
                    else { PyObject::str_val(CompactString::from(s)) }
                }).collect();
            return Ok(PyObject::tuple(items));
        }
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::dict(IndexMap::new()));
            }
            // Basic dict literal — only handles simple key:value pairs
            let mut map = IndexMap::new();
            for pair in inner.split(',') {
                if let Some((k, v)) = pair.split_once(':') {
                    let k = k.trim().trim_matches(|c| c == '\'' || c == '"');
                    let v = v.trim();
                    let val = if let Ok(n) = v.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = v.parse::<f64>() { PyObject::float(f) }
                    else { PyObject::str_val(CompactString::from(v.trim_matches(|c| c == '\'' || c == '"'))) };
                    map.insert(ferrython_core::types::HashableKey::Str(CompactString::from(k)), val);
                }
            }
            return Ok(PyObject::dict(map));
        }
        Err(PyException::value_error(format!("malformed node or string: {}", trimmed)))
    });

    // AST node type constructors (stubs)
    let make_node_type = |name: &str| -> PyObjectRef {
        let n = name.to_string();
        PyObject::native_closure(&format!("ast.{}", n), move |_args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from(&n), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })
    };

    make_module("ast", vec![
        ("parse", parse_fn),
        ("dump", dump_fn),
        ("literal_eval", literal_eval_fn),
        // Node types
        ("Module", make_node_type("Module")),
        ("Expression", make_node_type("Expression")),
        ("Interactive", make_node_type("Interactive")),
        ("FunctionDef", make_node_type("FunctionDef")),
        ("AsyncFunctionDef", make_node_type("AsyncFunctionDef")),
        ("ClassDef", make_node_type("ClassDef")),
        ("Return", make_node_type("Return")),
        ("Assign", make_node_type("Assign")),
        ("AugAssign", make_node_type("AugAssign")),
        ("AnnAssign", make_node_type("AnnAssign")),
        ("For", make_node_type("For")),
        ("While", make_node_type("While")),
        ("If", make_node_type("If")),
        ("With", make_node_type("With")),
        ("Raise", make_node_type("Raise")),
        ("Try", make_node_type("Try")),
        ("Import", make_node_type("Import")),
        ("ImportFrom", make_node_type("ImportFrom")),
        ("Expr", make_node_type("Expr")),
        ("Name", make_node_type("Name")),
        ("Constant", make_node_type("Constant")),
        ("BinOp", make_node_type("BinOp")),
        ("UnaryOp", make_node_type("UnaryOp")),
        ("BoolOp", make_node_type("BoolOp")),
        ("Compare", make_node_type("Compare")),
        ("Call", make_node_type("Call")),
        ("Attribute", make_node_type("Attribute")),
        ("Subscript", make_node_type("Subscript")),
        ("Starred", make_node_type("Starred")),
        ("List", make_node_type("List")),
        ("Tuple", make_node_type("Tuple")),
        ("Dict", make_node_type("Dict")),
        ("Set", make_node_type("Set")),
        ("Lambda", make_node_type("Lambda")),
        ("IfExp", make_node_type("IfExp")),
        ("ListComp", make_node_type("ListComp")),
        ("SetComp", make_node_type("SetComp")),
        ("DictComp", make_node_type("DictComp")),
        ("GeneratorExp", make_node_type("GeneratorExp")),
        ("Yield", make_node_type("Yield")),
        ("YieldFrom", make_node_type("YieldFrom")),
        ("Await", make_node_type("Await")),
        ("Pass", make_node_type("Pass")),
        ("Break", make_node_type("Break")),
        ("Continue", make_node_type("Continue")),
        // Load/Store/Del contexts
        ("Load", make_node_type("Load")),
        ("Store", make_node_type("Store")),
        ("Del", make_node_type("Del")),
        // PyCF compile flags
        ("PyCF_ONLY_AST", PyObject::int(1024)),
    ])
}

// ── linecache module ──

pub fn create_linecache_module() -> PyObjectRef {
    let getline_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("linecache.getline requires filename and lineno"));
        }
        let filename = args[0].py_to_string();
        let lineno = match &args[1].payload {
            PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as usize,
            _ => 0,
        };
        // Try to read the file and get the line
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                if lineno > 0 && lineno <= lines.len() {
                    Ok(PyObject::str_val(CompactString::from(format!("{}\n", lines[lineno - 1]))))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }
            Err(_) => Ok(PyObject::str_val(CompactString::from(""))),
        }
    });

    let getlines_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("linecache.getlines requires filename"));
        }
        let filename = args[0].py_to_string();
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<PyObjectRef> = content.lines()
                    .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                    .collect();
                Ok(PyObject::list(lines))
            }
            Err(_) => Ok(PyObject::list(vec![])),
        }
    });

    let clearcache_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    let checkcache_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    make_module("linecache", vec![
        ("getline", getline_fn),
        ("getlines", getlines_fn),
        ("clearcache", clearcache_fn),
        ("checkcache", checkcache_fn),
    ])
}

// ── token module ──

pub fn create_token_module() -> PyObjectRef {
    make_module("token", vec![
        ("ENDMARKER", PyObject::int(0)),
        ("NAME", PyObject::int(1)),
        ("NUMBER", PyObject::int(2)),
        ("STRING", PyObject::int(3)),
        ("NEWLINE", PyObject::int(4)),
        ("INDENT", PyObject::int(5)),
        ("DEDENT", PyObject::int(6)),
        ("LPAR", PyObject::int(7)),
        ("RPAR", PyObject::int(8)),
        ("LSQB", PyObject::int(9)),
        ("RSQB", PyObject::int(10)),
        ("COLON", PyObject::int(11)),
        ("COMMA", PyObject::int(12)),
        ("SEMI", PyObject::int(13)),
        ("PLUS", PyObject::int(14)),
        ("MINUS", PyObject::int(15)),
        ("STAR", PyObject::int(16)),
        ("SLASH", PyObject::int(17)),
        ("VBAR", PyObject::int(18)),
        ("AMPER", PyObject::int(19)),
        ("LESS", PyObject::int(20)),
        ("GREATER", PyObject::int(21)),
        ("EQUAL", PyObject::int(22)),
        ("DOT", PyObject::int(23)),
        ("PERCENT", PyObject::int(24)),
        ("LBRACE", PyObject::int(25)),
        ("RBRACE", PyObject::int(26)),
        ("EQEQUAL", PyObject::int(27)),
        ("NOTEQUAL", PyObject::int(28)),
        ("LESSEQUAL", PyObject::int(29)),
        ("GREATEREQUAL", PyObject::int(30)),
        ("TILDE", PyObject::int(31)),
        ("CIRCUMFLEX", PyObject::int(32)),
        ("LEFTSHIFT", PyObject::int(33)),
        ("RIGHTSHIFT", PyObject::int(34)),
        ("DOUBLESTAR", PyObject::int(35)),
        ("PLUSEQUAL", PyObject::int(36)),
        ("MINEQUAL", PyObject::int(37)),
        ("STAREQUAL", PyObject::int(38)),
        ("SLASHEQUAL", PyObject::int(39)),
        ("PERCENTEQUAL", PyObject::int(40)),
        ("AMPEREQUAL", PyObject::int(41)),
        ("VBAREQUAL", PyObject::int(42)),
        ("CIRCUMFLEXEQUAL", PyObject::int(43)),
        ("LEFTSHIFTEQUAL", PyObject::int(44)),
        ("RIGHTSHIFTEQUAL", PyObject::int(45)),
        ("DOUBLESTAREQUAL", PyObject::int(46)),
        ("DOUBLESLASH", PyObject::int(47)),
        ("DOUBLESLASHEQUAL", PyObject::int(48)),
        ("AT", PyObject::int(49)),
        ("ATEQUAL", PyObject::int(50)),
        ("RARROW", PyObject::int(51)),
        ("ELLIPSIS", PyObject::int(52)),
        ("COLONEQUAL", PyObject::int(53)),
        ("OP", PyObject::int(54)),
        ("COMMENT", PyObject::int(55)),
        ("NL", PyObject::int(56)),
        ("ERRORTOKEN", PyObject::int(57)),
        ("ENCODING", PyObject::int(62)),
        ("NT_OFFSET", PyObject::int(256)),
        ("tok_name", {
            let mut map = IndexMap::new();
            for (i, name) in [(0,"ENDMARKER"),(1,"NAME"),(2,"NUMBER"),(3,"STRING"),(4,"NEWLINE"),
                (5,"INDENT"),(6,"DEDENT"),(54,"OP"),(55,"COMMENT"),(56,"NL"),(57,"ERRORTOKEN"),(62,"ENCODING")].iter() {
                map.insert(ferrython_core::types::HashableKey::Int(ferrython_core::types::PyInt::Small(*i)), 
                    PyObject::str_val(CompactString::from(*name)));
            }
            PyObject::dict(map)
        }),
    ])
}
