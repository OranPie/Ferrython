use super::*;

// ── dis module ──

pub fn create_dis_module() -> PyObjectRef {
    use ferrython_bytecode::code::ConstantValue;
    use ferrython_bytecode::opcode::Opcode;

    fn dis_dis(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "dis() requires a function argument",
            ));
        }
        let obj = &args[0];
        let code: std::rc::Rc<ferrython_bytecode::CodeObject> = match &obj.payload {
            PyObjectPayload::Function(pf) => std::rc::Rc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::rc::Rc::clone(c),
            PyObjectPayload::Str(s) => {
                // Auto-compile string source code, like CPython
                let source = s.as_str();
                match ferrython_parser::parse(source, "<dis>") {
                    Ok(module) => match ferrython_compiler::compile(&module, "<dis>") {
                        Ok(c) => std::rc::Rc::new(c),
                        Err(e) => {
                            return Err(PyException::type_error(format!(
                                "could not compile source: {}",
                                e
                            )))
                        }
                    },
                    Err(e) => {
                        return Err(PyException::type_error(format!(
                            "could not parse source: {:?}",
                            e
                        )))
                    }
                }
            }
            _ => {
                return Err(PyException::type_error(format!(
                    "don't know how to disassemble {} objects",
                    obj.type_name()
                )))
            }
        };
        let output = disassemble_code_to_string(&code, 0);
        // Resolve file= keyword argument from trailing kwargs dict or positional arg
        let mut file_obj: Option<PyObjectRef> = None;
        if args.len() >= 2 {
            let last = &args[args.len() - 1];
            // kwargs packed as trailing dict by VM
            if let PyObjectPayload::Dict(map) = &last.payload {
                let r = map.read();
                if let Some(f) = r.get(&HashableKey::str_key(CompactString::from("file"))) {
                    file_obj = Some(f.clone());
                }
            }
            // Also accept positional file-like object
            if file_obj.is_none() {
                if let PyObjectPayload::Instance(_) = &last.payload {
                    file_obj = Some(last.clone());
                }
            }
        }
        let mut written = false;
        if let Some(ref fobj) = file_obj {
            if let PyObjectPayload::Instance(ref inst) = fobj.payload {
                if let Some(write_fn) = inst.attrs.read().get("write").cloned() {
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            (nf.func)(&[PyObject::str_val(CompactString::from(output.as_str()))])?;
                            written = true;
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            (nc.func)(&[PyObject::str_val(CompactString::from(output.as_str()))])?;
                            written = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if !written {
            print!("{}", output);
        }
        Ok(PyObject::none())
    }

    fn disassemble_code_to_string(code: &ferrython_bytecode::CodeObject, indent: usize) -> String {
        let mut output = String::new();
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
            let lineno = if i < line_for_offset.len() {
                line_for_offset[i]
            } else {
                last_lineno
            };
            let line_str = if lineno != prev_line {
                prev_line = lineno;
                format!("{:>4}", lineno)
            } else {
                "    ".to_string()
            };

            let arg_desc = format_dis_arg(code, instr.op, instr.arg);
            use std::fmt::Write;
            let _ = writeln!(
                output,
                "{}{} {:>6} {:<24} {}",
                pad,
                line_str,
                i * 2,
                format!("{:?}", instr.op),
                arg_desc
            );
        }

        // Recurse into nested code objects
        for c in &code.constants {
            if let ConstantValue::Code(nested) = c {
                output.push('\n');
                use std::fmt::Write;
                let _ = writeln!(
                    output,
                    "{}Disassembly of <code object {} at ...>:",
                    pad, nested.name
                );
                output.push_str(&disassemble_code_to_string(nested, indent + 2));
            }
        }
        output
    }

    fn format_dis_arg(code: &ferrython_bytecode::CodeObject, op: Opcode, arg: u32) -> String {
        match op {
            Opcode::LoadConst => {
                if let Some(c) = code.constants.get(arg as usize) {
                    match c {
                        ConstantValue::Str(s) => {
                            format!("{:<4} ('{}')", arg, if s.len() > 30 { &s[..27] } else { s })
                        }
                        ConstantValue::Integer(n) => format!("{:<4} ({})", arg, n),
                        ConstantValue::Float(f) => format!("{:<4} ({})", arg, f),
                        ConstantValue::None => format!("{:<4} (None)", arg),
                        ConstantValue::Bool(b) => format!("{:<4} ({})", arg, b),
                        ConstantValue::Code(c) => format!("{:<4} (<code object {}>)", arg, c.name),
                        ConstantValue::Tuple(t) => format!("{:<4} (tuple/{})", arg, t.len()),
                        _ => format!("{}", arg),
                    }
                } else {
                    format!("{}", arg)
                }
            }
            Opcode::LoadName
            | Opcode::StoreName
            | Opcode::DeleteName
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal
            | Opcode::DeleteGlobal
            | Opcode::LoadAttr
            | Opcode::StoreAttr
            | Opcode::DeleteAttr
            | Opcode::ImportName
            | Opcode::ImportFrom => {
                if let Some(n) = code.names.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else {
                    format!("{}", arg)
                }
            }
            Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast => {
                if let Some(n) = code.varnames.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else {
                    format!("{}", arg)
                }
            }
            Opcode::LoadDeref | Opcode::StoreDeref | Opcode::LoadClosure => {
                let nc = code.cellvars.len();
                let idx = arg as usize;
                if idx < nc {
                    code.cellvars
                        .get(idx)
                        .map_or(format!("{}", arg), |n| format!("{:<4} (cell: {})", arg, n))
                } else {
                    code.freevars
                        .get(idx - nc)
                        .map_or(format!("{}", arg), |n| format!("{:<4} (free: {})", arg, n))
                }
            }
            Opcode::CompareOp => {
                let op_name = match arg {
                    0 => "<",
                    1 => "<=",
                    2 => "==",
                    3 => "!=",
                    4 => ">",
                    5 => ">=",
                    6 => "in",
                    7 => "not in",
                    8 => "is",
                    9 => "is not",
                    10 => "exception match",
                    _ => "?",
                };
                format!("{:<4} ({})", arg, op_name)
            }
            Opcode::JumpAbsolute
            | Opcode::JumpForward
            | Opcode::JumpFinally
            | Opcode::PopJumpIfTrue
            | Opcode::PopJumpIfFalse
            | Opcode::JumpIfTrueOrPop
            | Opcode::JumpIfFalseOrPop
            | Opcode::SetupExcept
            | Opcode::SetupFinally
            | Opcode::ForIter => {
                format!("{:<4} (to {})", arg, arg)
            }
            _ => {
                if arg != 0 {
                    format!("{}", arg)
                } else {
                    String::new()
                }
            }
        }
    }

    fn constant_to_pyobject(c: &ConstantValue) -> PyObjectRef {
        match c {
            ConstantValue::None => PyObject::none(),
            ConstantValue::Bool(b) => PyObject::bool_val(*b),
            ConstantValue::Integer(n) => PyObject::int(*n),
            ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
            ConstantValue::Float(f) => PyObject::float(*f),
            ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
            ConstantValue::Str(s) => PyObject::str_val(s.clone()),
            ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
            ConstantValue::Ellipsis => PyObject::ellipsis(),
            ConstantValue::Code(co) => {
                PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::clone(co)))
            }
            ConstantValue::Tuple(items) => {
                PyObject::tuple(items.iter().map(constant_to_pyobject).collect())
            }
            ConstantValue::FrozenSet(items) => {
                let mut set = new_fx_hashkey_map();
                for item in items {
                    let obj = constant_to_pyobject(item);
                    if let Ok(key) = obj.to_hashable_key() {
                        set.insert(key, obj);
                    }
                }
                PyObject::frozenset(set)
            }
        }
    }

    fn opcode_display_name(op: Opcode) -> CompactString {
        let raw = format!("{:?}", op);
        let mut out = String::with_capacity(raw.len() + 8);
        for (i, ch) in raw.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() {
                out.push('_');
            }
            out.push(ch.to_ascii_uppercase());
        }
        CompactString::from(out)
    }

    fn instruction_argval(
        code: &ferrython_bytecode::CodeObject,
        op: Opcode,
        arg: u32,
    ) -> PyObjectRef {
        match op {
            Opcode::LoadConst => code
                .constants
                .get(arg as usize)
                .map(constant_to_pyobject)
                .unwrap_or_else(PyObject::none),
            Opcode::LoadName
            | Opcode::StoreName
            | Opcode::DeleteName
            | Opcode::LoadGlobal
            | Opcode::StoreGlobal
            | Opcode::DeleteGlobal
            | Opcode::LoadAttr
            | Opcode::StoreAttr
            | Opcode::DeleteAttr
            | Opcode::ImportName
            | Opcode::ImportFrom => code
                .names
                .get(arg as usize)
                .map(|n| PyObject::str_val(n.clone()))
                .unwrap_or_else(PyObject::none),
            Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast => code
                .varnames
                .get(arg as usize)
                .map(|n| PyObject::str_val(n.clone()))
                .unwrap_or_else(PyObject::none),
            Opcode::LoadDeref | Opcode::StoreDeref | Opcode::DeleteDeref | Opcode::LoadClosure => {
                let idx = arg as usize;
                let ncells = code.cellvars.len();
                if idx < ncells {
                    code.cellvars
                        .get(idx)
                        .map(|n| PyObject::str_val(n.clone()))
                        .unwrap_or_else(PyObject::none)
                } else {
                    code.freevars
                        .get(idx - ncells)
                        .map(|n| PyObject::str_val(n.clone()))
                        .unwrap_or_else(PyObject::none)
                }
            }
            Opcode::CompareOp => {
                let op_name = match arg {
                    0 => "<",
                    1 => "<=",
                    2 => "==",
                    3 => "!=",
                    4 => ">",
                    5 => ">=",
                    6 => "in",
                    7 => "not in",
                    8 => "is",
                    9 => "is not",
                    10 => "exception match",
                    _ => "?",
                };
                PyObject::str_val(CompactString::from(op_name))
            }
            Opcode::JumpAbsolute
            | Opcode::JumpForward
            | Opcode::JumpFinally
            | Opcode::PopJumpIfTrue
            | Opcode::PopJumpIfFalse
            | Opcode::JumpIfTrueOrPop
            | Opcode::JumpIfFalseOrPop
            | Opcode::SetupExcept
            | Opcode::SetupFinally
            | Opcode::ForIter => PyObject::int((arg * 2) as i64),
            _ if op.has_arg() => PyObject::int(arg as i64),
            _ => PyObject::none(),
        }
    }

    fn push_instruction(
        code: &ferrython_bytecode::CodeObject,
        out: &mut Vec<PyObjectRef>,
        offset: &mut usize,
        op: Opcode,
        arg: u32,
    ) {
        let inst_cls = PyObject::class(CompactString::from("Instruction"), vec![], IndexMap::new());
        let inst = PyObject::instance(inst_cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(
                CompactString::from("opname"),
                PyObject::str_val(opcode_display_name(op)),
            );
            attrs.insert(CompactString::from("opcode"), PyObject::int(op as i64));
            attrs.insert(
                CompactString::from("arg"),
                if op.has_arg() {
                    PyObject::int(arg as i64)
                } else {
                    PyObject::none()
                },
            );
            attrs.insert(
                CompactString::from("argval"),
                instruction_argval(code, op, arg),
            );
            attrs.insert(CompactString::from("offset"), PyObject::int(*offset as i64));
            attrs.insert(CompactString::from("starts_line"), PyObject::none());
            attrs.insert(
                CompactString::from("is_jump_target"),
                PyObject::bool_val(false),
            );
        }
        out.push(inst);
        *offset += 2;
    }

    fn instruction_list(code: &ferrython_bytecode::CodeObject) -> Vec<PyObjectRef> {
        let mut instructions = Vec::new();
        let mut offset = 0usize;
        for instr in &code.instructions {
            match instr.op {
                Opcode::LoadConstReturnValue => {
                    push_instruction(
                        code,
                        &mut instructions,
                        &mut offset,
                        Opcode::LoadConst,
                        instr.arg,
                    );
                    push_instruction(code, &mut instructions, &mut offset, Opcode::ReturnValue, 0);
                }
                Opcode::LoadFastReturnValue => {
                    push_instruction(
                        code,
                        &mut instructions,
                        &mut offset,
                        Opcode::LoadFast,
                        instr.arg,
                    );
                    push_instruction(code, &mut instructions, &mut offset, Opcode::ReturnValue, 0);
                }
                _ => push_instruction(code, &mut instructions, &mut offset, instr.op, instr.arg),
            }
        }
        instructions
    }

    // code_info(x) — return formatted information about a code object
    fn dis_code_info(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("code_info() requires argument"));
        }
        let code: std::rc::Rc<ferrython_bytecode::CodeObject> = match &args[0].payload {
            PyObjectPayload::Function(pf) => std::rc::Rc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::rc::Rc::clone(c),
            _ => return Err(PyException::type_error("don't know how to get code info")),
        };
        let mut info = String::new();
        info.push_str(&format!("Name:              {}\n", code.name));
        info.push_str(&format!("Filename:          {}\n", code.filename));
        info.push_str(&format!("Argument count:    {}\n", code.arg_count));
        info.push_str(&format!("Kw-only arguments: {}\n", code.kwonlyarg_count));
        info.push_str(&format!("Number of locals:  {}\n", code.varnames.len()));
        info.push_str(&format!("Stack size:        {}\n", code.instructions.len()));
        info.push_str(&format!("Flags:             0x{:04x}\n", code.flags));
        if !code.constants.is_empty() {
            info.push_str("Constants:\n");
            for (i, c) in code.constants.iter().enumerate() {
                let repr = match c {
                    ConstantValue::Str(s) => format!("'{}'", s),
                    ConstantValue::Integer(n) => format!("{}", n),
                    ConstantValue::Float(f) => format!("{}", f),
                    ConstantValue::None => "None".to_string(),
                    ConstantValue::Bool(b) => format!("{}", b),
                    ConstantValue::Code(c) => format!("<code object {}>", c.name),
                    _ => "...".to_string(),
                };
                info.push_str(&format!("   {}: {}\n", i, repr));
            }
        }
        if !code.names.is_empty() {
            info.push_str("Names:\n");
            for (i, n) in code.names.iter().enumerate() {
                info.push_str(&format!("   {}: {}\n", i, n));
            }
        }
        if !code.varnames.is_empty() {
            info.push_str("Variable names:\n");
            for (i, v) in code.varnames.iter().enumerate() {
                info.push_str(&format!("   {}: {}\n", i, v));
            }
        }
        Ok(PyObject::str_val(CompactString::from(info)))
    }

    // Instruction namedtuple-like class
    let instruction_cls = {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__init__"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        PyObject::class(CompactString::from("Instruction"), vec![], ns)
    };

    // Bytecode(x) / get_instructions(x) — iterable of Instruction objects
    let bytecode_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("Bytecode() requires argument"));
        }
        let code: std::rc::Rc<ferrython_bytecode::CodeObject> = match &args[0].payload {
            PyObjectPayload::Function(pf) => std::rc::Rc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::rc::Rc::clone(c),
            _ => return Err(PyException::type_error("don't know how to disassemble")),
        };
        Ok(PyObject::list(instruction_list(&code)))
    });

    // show_code(x) — print code_info to stdout
    let show_code_fn = make_builtin(|args: &[PyObjectRef]| {
        let info = dis_code_info(args)?;
        println!("{}", info.py_to_string());
        Ok(PyObject::none())
    });

    make_module(
        "dis",
        vec![
            ("dis", make_builtin(dis_dis)),
            ("disassemble", make_builtin(dis_dis)),
            ("code_info", make_builtin(dis_code_info)),
            ("show_code", show_code_fn),
            ("Bytecode", bytecode_fn.clone()),
            ("get_instructions", bytecode_fn),
            ("Instruction", instruction_cls),
        ],
    )
}
