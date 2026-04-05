//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, FramePool, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_bytecode::opcode::Opcode;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{PyInt, SharedGlobals};
use ferrython_debug::{ExecutionProfiler, BreakpointManager};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    pub(crate) call_stack: Vec<Frame>,
    pub(crate) builtins: SharedBuiltins,
    pub(crate) modules: IndexMap<CompactString, PyObjectRef>,
    /// Currently active exception being handled (for bare `raise` re-raise).
    pub(crate) active_exception: Option<PyException>,
    /// Reference to the sys.modules dict for synchronization.
    pub(crate) sys_modules_dict: Option<PyObjectRef>,
    /// Execution profiler (disabled by default — zero overhead when off).
    pub profiler: ExecutionProfiler,
    /// Breakpoint manager for debugger support.
    pub breakpoints: BreakpointManager,
    /// Pool of reusable frame vectors to reduce allocation.
    pub(crate) frame_pool: FramePool,
}

impl VirtualMachine {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            builtins: Arc::new(builtins::init_builtins()),
            modules: IndexMap::new(),
            active_exception: None,
            sys_modules_dict: None,
            profiler: ExecutionProfiler::new(),
            breakpoints: BreakpointManager::new(),
            frame_pool: FramePool::new(),
        }
    }

    /// Create a new empty shared globals map.
    pub fn new_globals() -> SharedGlobals {
        Arc::new(RwLock::new(IndexMap::new()))
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let globals = Arc::new(RwLock::new(IndexMap::new()));
        // Set __name__ = "__main__" for top-level scripts
        globals.write().insert(
            CompactString::from("__name__"),
            PyObject::str_val(CompactString::from("__main__")),
        );
        self.execute_with_globals(Arc::new(code), globals)
    }

    /// Execute a code object with shared globals (for REPL).
    pub fn execute_with_globals(&mut self, code: Arc<CodeObject>, globals: SharedGlobals) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let frame = Frame::new(code, globals, Arc::clone(&self.builtins));
        self.call_stack.push(frame);
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
    }

    /// Execute a code object as a function call with arguments.
    pub(crate) fn run_frame(&mut self) -> PyResult<PyObjectRef> {
        let profiling = self.profiler.is_enabled();
        loop {
            let frame = self.call_stack.last().unwrap();
            if frame.ip >= frame.code.instructions.len() {
                return Ok(PyObject::none());
            }

            let instr = frame.code.instructions[frame.ip];
            let frame = self.call_stack.last_mut().unwrap();
            frame.ip += 1;

            if profiling { self.profiler.start_instruction(instr.op); }

            // Inline the hottest opcodes to avoid execute_one dispatch overhead
            let result = match instr.op {
                Opcode::LoadFast => {
                    let idx = instr.arg as usize;
                    match frame.locals.get(idx).and_then(|v| v.as_ref()) {
                        Some(val) => { frame.stack.push(val.clone()); Ok(None) }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::StoreFast => {
                    let val = frame.stack.pop().expect("stack underflow");
                    frame.locals[instr.arg as usize] = Some(val);
                    Ok(None)
                }
                Opcode::LoadConst => {
                    let obj = frame.constant_cache[instr.arg as usize].clone();
                    frame.stack.push(obj);
                    Ok(None)
                }
                Opcode::PopTop => {
                    frame.stack.pop();
                    Ok(None)
                }
                // Inline ReturnValue: fast path when no finally blocks are active
                Opcode::ReturnValue => {
                    if frame.block_stack.iter().any(|b| b.kind == BlockKind::Finally) {
                        // Must go through full handler for finally unwinding
                        self.execute_one(instr)
                    } else {
                        let val = frame.stack.pop().expect("stack underflow");
                        Ok(Some(val))
                    }
                }

                // Inline int+int for BinaryAdd (hot in arithmetic loops)
                Opcode::BinaryAdd | Opcode::InplaceAdd => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                let result = match x.checked_add(*y) {
                                    Some(r) => PyObject::int(r),
                                    None => {
                                        use num_bigint::BigInt;
                                        PyObject::big_int(BigInt::from(*x) + BigInt::from(*y))
                                    }
                                };
                                frame.stack.truncate(len - 2);
                                frame.stack.push(result);
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline int comparisons (hot in for-loop range iteration)
                Opcode::CompareOp if instr.arg <= 5 => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                let result = match instr.arg {
                                    0 => x < y,  // Lt
                                    1 => x <= y, // Le
                                    2 => x == y, // Eq
                                    3 => x != y, // Ne
                                    4 => x > y,  // Gt
                                    _ => x >= y, // Ge (5)
                                };
                                frame.stack.truncate(len - 2);
                                frame.stack.push(PyObject::bool_val(result));
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                _ => self.execute_one(instr),
            };

            match result {
                Ok(Some(ret)) => {
                    if profiling { self.profiler.end_instruction(instr.op); }
                    return Ok(ret);
                }
                Ok(None) => {
                    if profiling { self.profiler.end_instruction(instr.op); }
                }
                Err(mut exc) => {
                    // Always attach traceback from the call stack
                    if exc.traceback.is_empty() {
                        self.attach_traceback(&mut exc);
                    }
                    if let Some(handler_ip) = self.unwind_except() {
                        // Store active exception for bare `raise` re-raise
                        self.active_exception = Some(exc.clone());
                        // Update thread-local for sys.exc_info()
                        ferrython_stdlib::set_exc_info(
                            exc.kind.clone(),
                            exc.message.clone(),
                            exc.original.clone(),
                        );
                        let frame = self.call_stack.last_mut().unwrap();
                        // CPython pushes (traceback, value, type) — 3 items
                        let (exc_value, exc_type) = if let Some(orig) = &exc.original {
                            let cls = if let PyObjectPayload::Instance(inst) = &orig.payload {
                                inst.class.clone()
                            } else {
                                PyObject::exception_type(exc.kind.clone())
                            };
                            (orig.clone(), cls)
                        } else {
                            let inst = if let Some(val) = &exc.value {
                                PyObject::exception_instance_with_args(exc.kind.clone(), exc.message.clone(), vec![val.clone()])
                            } else {
                                PyObject::exception_instance(exc.kind.clone(), exc.message.clone())
                            };
                            (
                                inst,
                                PyObject::exception_type(exc.kind.clone()),
                            )
                        };
                        // Store StopIteration.value if present
                        if let Some(val) = &exc.value {
                            Self::store_exc_attr(&exc_value, "value", val.clone());
                        }
                        // Attach __cause__ from exception chaining (raise X from Y)
                        if let Some(cause) = &exc.cause {
                            let cause_obj = if let Some(corig) = &cause.original {
                                corig.clone()
                            } else {
                                PyObject::exception_instance(cause.kind.clone(), cause.message.clone())
                            };
                            Self::store_exc_attr(&exc_value, "__cause__", cause_obj);
                        } else {
                            Self::store_exc_attr(&exc_value, "__cause__", PyObject::none());
                        }
                        // Attach __context__ from implicit exception chaining
                        if let Some(ctx) = &exc.context {
                            let ctx_obj = if let Some(corig) = &ctx.original {
                                corig.clone()
                            } else {
                                PyObject::exception_instance(ctx.kind.clone(), ctx.message.clone())
                            };
                            Self::store_exc_attr(&exc_value, "__context__", ctx_obj);
                        } else {
                            Self::store_exc_attr(&exc_value, "__context__", PyObject::none());
                        }
                        // Store __traceback__ on the exception value
                        let tb_obj = Self::build_traceback_object(&exc.traceback);
                        Self::store_exc_attr(&exc_value, "__traceback__", tb_obj.clone());
                        frame.push(tb_obj);               // traceback
                        frame.push(exc_value);            // value
                        frame.push(exc_type);             // type
                        frame.ip = handler_ip;
                    } else {
                        return Err(exc);
                    }
                }
            }
        }
    }

    /// Attach traceback entries from the current call stack to an exception.
    fn attach_traceback(&self, exc: &mut PyException) {
        use ferrython_core::error::TracebackEntry;
        for frame in &self.call_stack {
            let lineno = ferrython_debug::resolve_lineno(
                &frame.code,
                frame.ip.saturating_sub(1),
            );
            exc.traceback.push(TracebackEntry {
                filename: frame.code.filename.to_string(),
                function: frame.code.name.to_string(),
                lineno,
            });
        }
    }

    /// Build a Python-level traceback object (list of (filename, lineno, funcname) tuples).
    fn build_traceback_object(entries: &[ferrython_core::error::TracebackEntry]) -> PyObjectRef {
        if entries.is_empty() {
            return PyObject::none();
        }
        // Build a traceback chain: last entry is the innermost frame.
        // Each traceback object has tb_lineno, tb_frame (None), and tb_next.
        let tb_class = PyObject::builtin_type(CompactString::from("traceback"));
        let mut tb_next = PyObject::none();
        for entry in entries {
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("tb_lineno"), PyObject::int(entry.lineno as i64));
            attrs.insert(CompactString::from("tb_frame"), PyObject::none());
            attrs.insert(CompactString::from("tb_next"), tb_next);
            attrs.insert(CompactString::from("tb_filename"),
                PyObject::str_val(CompactString::from(&entry.filename)));
            attrs.insert(CompactString::from("tb_name"),
                PyObject::str_val(CompactString::from(&entry.function)));
            tb_next = PyObject::instance_with_attrs(tb_class.clone(), attrs);
        }
        tb_next
    }

    /// Store an attribute on an exception value object (works for both Instance and ExceptionInstance).
    fn store_exc_attr(exc_value: &PyObjectRef, name: &str, value: PyObjectRef) {
        match &exc_value.payload {
            PyObjectPayload::Instance(inst) => {
                inst.attrs.write().insert(CompactString::from(name), value);
            }
            PyObjectPayload::ExceptionInstance { attrs, .. } => {
                attrs.write().insert(CompactString::from(name), value);
            }
            _ => {}
        }
    }

    /// Handle a breakpoint hit — print location info and current stack state.
    pub(crate) fn handle_breakpoint_hit(&self) {
        if let Some(frame) = self.call_stack.last() {
            let lineno = ferrython_debug::resolve_lineno(
                &frame.code,
                frame.ip.saturating_sub(1),
            );
            eprintln!(
                "*** Breakpoint hit: File \"{}\", line {}, in {} ***",
                frame.code.filename, lineno, frame.code.name
            );
            // Print local variables if in a function scope
            if frame.scope_kind == crate::frame::ScopeKind::Function {
                let mut locals_info = Vec::new();
                for (i, name) in frame.code.varnames.iter().enumerate() {
                    if let Some(val) = frame.locals.get(i).and_then(|v| v.as_ref()) {
                        locals_info.push(format!("  {} = {}", name, val.py_to_string()));
                    }
                }
                if !locals_info.is_empty() {
                    eprintln!("  Locals:");
                    for info in &locals_info {
                        eprintln!("{}", info);
                    }
                }
            }
        }
    }

    /// Find an exception handler on the block stack. Returns handler IP if found.
    pub(crate) fn unwind_except(&mut self) -> Option<usize> {
        let frame = self.call_stack.last_mut()?;
        while let Some(block) = frame.pop_block() {
            match block.kind {
                BlockKind::Except | BlockKind::Finally => {
                    // Unwind value stack to block level
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    // Push an ExceptHandler block so PopExcept can find it
                    frame.push_block(BlockKind::ExceptHandler, 0);
                    return Some(block.handler);
                }
                BlockKind::ExceptHandler => {
                    // Clean up a previous except handler (exception in except body)
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::Loop => {
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::With => {
                    // With block exception — jump to cleanup handler which will
                    // call __exit__ with exception info
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    return Some(block.handler);
                }
            }
        }
        None
    }

    fn execute_one(&mut self, instr: ferrython_bytecode::Instruction) -> Result<Option<PyObjectRef>, PyException> {
        use ferrython_bytecode::opcode::Opcode;
        match instr.op {
            Opcode::Nop | Opcode::PopTop | Opcode::RotTwo | Opcode::RotThree
            | Opcode::RotFour | Opcode::DupTop | Opcode::DupTopTwo | Opcode::LoadConst
                => self.exec_stack_ops(instr),

            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast
            | Opcode::LoadDeref | Opcode::StoreDeref | Opcode::DeleteDeref
            | Opcode::LoadClosure | Opcode::LoadClassderef
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
                => self.exec_name_ops(instr),

            Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
                => self.exec_attr_ops(instr),

            Opcode::UnaryPositive | Opcode::UnaryNegative
            | Opcode::UnaryNot | Opcode::UnaryInvert
                => self.exec_unary_ops(instr),

            Opcode::BinaryAdd | Opcode::InplaceAdd
            | Opcode::BinarySubtract | Opcode::InplaceSubtract
            | Opcode::BinaryMultiply | Opcode::InplaceMultiply
            | Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide
            | Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide
            | Opcode::BinaryModulo | Opcode::InplaceModulo
            | Opcode::BinaryPower | Opcode::InplacePower
            | Opcode::BinaryLshift | Opcode::InplaceLshift
            | Opcode::BinaryRshift | Opcode::InplaceRshift
            | Opcode::BinaryAnd | Opcode::InplaceAnd
            | Opcode::BinaryOr | Opcode::InplaceOr
            | Opcode::BinaryXor | Opcode::InplaceXor
            | Opcode::BinaryMatrixMultiply | Opcode::InplaceMatrixMultiply
                => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr
                => self.exec_subscript_ops(instr),

            Opcode::CompareOp => self.exec_compare_ops(instr),

            Opcode::JumpForward | Opcode::JumpAbsolute
            | Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter | Opcode::GetYieldFromIter | Opcode::ForIter
            | Opcode::EndForLoop
                => self.exec_jump_ops(instr),

            Opcode::BuildTuple | Opcode::BuildList | Opcode::BuildSet
            | Opcode::BuildMap | Opcode::BuildConstKeyMap | Opcode::BuildString
            | Opcode::ListAppend | Opcode::SetAdd | Opcode::MapAdd
            | Opcode::DictUpdate | Opcode::DictMerge | Opcode::ListExtend
            | Opcode::SetUpdate | Opcode::ListToTuple | Opcode::BuildSlice
            | Opcode::UnpackSequence | Opcode::UnpackEx
                => self.exec_build_ops(instr),

            Opcode::CallFunction | Opcode::CallFunctionKw | Opcode::CallMethod
            | Opcode::CallFunctionEx | Opcode::LoadMethod | Opcode::MakeFunction
                => self.exec_call_ops(instr),

            Opcode::ReturnValue | Opcode::ImportName | Opcode::ImportFrom
            | Opcode::ImportStar
                => self.exec_return_import(instr),

            Opcode::SetupFinally | Opcode::SetupExcept | Opcode::PopBlock
            | Opcode::PopExcept | Opcode::EndFinally | Opcode::BeginFinally
            | Opcode::RaiseVarargs | Opcode::SetupWith | Opcode::SetupAsyncWith
            | Opcode::WithCleanupStart | Opcode::WithCleanupFinish
                => self.exec_exception_ops(instr),

            Opcode::PrintExpr | Opcode::LoadBuildClass | Opcode::SetupAnnotations
            | Opcode::FormatValue | Opcode::ExtendedArg
            | Opcode::YieldValue | Opcode::YieldFrom
            | Opcode::GetAwaitable | Opcode::GetAiter | Opcode::GetAnext
            | Opcode::BeforeAsyncWith | Opcode::EndAsyncFor
                => self.exec_misc_ops(instr),

            #[allow(unreachable_patterns)]
            _ => Err(PyException::runtime_error(format!(
                "unimplemented opcode: {:?}", instr.op
            ))),
        }
    }


    /// Truthiness test that dispatches __bool__/__len__ on instances.
    /// Walk a class hierarchy to find if it inherits from an ExceptionType
    pub(crate) fn find_exception_kind(cls: &PyObjectRef) -> ExceptionKind {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => kind.clone(),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
                ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
            }
            PyObjectPayload::Class(cd) => {
                for base in &cd.bases {
                    let kind = Self::find_exception_kind(base);
                    if !matches!(kind, ExceptionKind::RuntimeError) {
                        return kind;
                    }
                    // Also check if base IS the exception type
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                // Check MRO
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                ExceptionKind::RuntimeError
            }
            _ => ExceptionKind::RuntimeError,
        }
    }

    pub(crate) fn vm_is_truthy(&mut self, obj: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_) = &obj.payload {
            if let Some(bool_method) = obj.get_attr("__bool__") {
                let result = self.call_object(bool_method, vec![])?;
                return Ok(result.is_truthy());
            }
            if let Some(len_method) = obj.get_attr("__len__") {
                let result = self.call_object(len_method, vec![])?;
                return Ok(result.is_truthy());
            }
        }
        Ok(obj.is_truthy())
    }

    /// Try to call a dunder method on an instance. Returns None if the object
    /// is not an Instance or doesn't have the named dunder.
    pub(crate) fn try_call_dunder(
        &mut self, obj: &PyObjectRef, dunder: &str, args: Vec<PyObjectRef>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match &obj.payload {
            PyObjectPayload::Instance(_) => {
                if let Some(method) = obj.get_attr(dunder) {
                    return Ok(Some(self.call_object(method, args)?));
                }
            }
            PyObjectPayload::Module { .. } => {
                if let Some(method) = obj.get_attr(dunder) {
                    // Module methods expect self as first arg (like file objects with _bind_methods)
                    let mut method_args = vec![obj.clone()];
                    method_args.extend(args);
                    return Ok(Some(self.call_object(method, method_args)?));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if `actual` exception kind matches `expected` (including inheritance).
pub(crate) fn exception_kind_matches(actual: &ExceptionKind, expected: &ExceptionKind) -> bool {
    if std::mem::discriminant(actual) == std::mem::discriminant(expected) {
        return true;
    }
    // Walk the exception hierarchy
    match expected {
        ExceptionKind::BaseException => true, // catches everything
        ExceptionKind::Exception => !matches!(actual,
            ExceptionKind::SystemExit | ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit
        ),
        ExceptionKind::ArithmeticError => matches!(actual,
            ExceptionKind::ArithmeticError | ExceptionKind::FloatingPointError |
            ExceptionKind::OverflowError | ExceptionKind::ZeroDivisionError
        ),
        ExceptionKind::LookupError => matches!(actual,
            ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
        ),
        ExceptionKind::OSError => matches!(actual,
            ExceptionKind::OSError | ExceptionKind::BlockingIOError |
            ExceptionKind::BrokenPipeError | ExceptionKind::FileExistsError |
            ExceptionKind::FileNotFoundError | ExceptionKind::PermissionError
        ),
        ExceptionKind::UnicodeError => matches!(actual,
            ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
            ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::ValueError => matches!(actual,
            ExceptionKind::ValueError | ExceptionKind::UnicodeError |
            ExceptionKind::UnicodeDecodeError | ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::Warning => matches!(actual,
            ExceptionKind::Warning | ExceptionKind::DeprecationWarning |
            ExceptionKind::RuntimeWarning | ExceptionKind::UserWarning
        ),
        ExceptionKind::ImportError => matches!(actual,
            ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
        ),
        _ => false,
    }
}
