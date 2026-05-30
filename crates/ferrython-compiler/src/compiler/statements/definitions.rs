//! Function, class, and annotation statement helpers.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, CodeObject, ConstantValue, Opcode};

use super::super::expressions::body_contains_yield;
use super::super::{CompileUnit, Compiler, Result};
use crate::symbol_table::Scope;

impl Compiler {
    // ── function definition ─────────────────────────────────────────

    pub(super) fn compile_function_def(
        &mut self,
        name: &str,
        args: &Arguments,
        body: &[Statement],
        decorator_list: &[Expression],
        returns: Option<&Expression>,
        is_async: bool,
        location: SourceLocation,
    ) -> Result<()> {
        // Compile decorators first (they are called in reverse order)
        for dec in decorator_list {
            self.compile_expression(dec)?;
        }

        // Compile default argument values in the enclosing scope
        let num_defaults = args.defaults.len();
        if num_defaults > 0 {
            for default in &args.defaults {
                self.compile_expression(default)?;
            }
            self.emit_arg(Opcode::BuildTuple, num_defaults as u32);
        }

        // Compile keyword-only defaults as a dict
        let kw_defaults: Vec<_> = args
            .kw_defaults
            .iter()
            .zip(args.kwonlyargs.iter())
            .filter(|(d, _)| d.is_some())
            .collect();
        let has_kw_defaults = !kw_defaults.is_empty();
        if has_kw_defaults {
            for (default, arg) in &kw_defaults {
                let key_idx = self.add_const(ConstantValue::Str(arg.arg.clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(default.as_ref().unwrap())?;
            }
            self.emit_arg(Opcode::BuildMap, kw_defaults.len() as u32);
        }

        // Build child code object
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        self.push_function_unit(name, child_scope, &qualname)?;

        // Set the first line number from the def statement location
        self.current_unit_mut().code.first_line_number = location.line;

        // Set up argument info on the code object
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            // Add parameters as varnames
            for arg in &args.posonlyargs {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            for arg in &args.args {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref vararg) = args.vararg {
                unit.code.flags |= CodeFlags::VARARGS;
                let name_str = vararg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(vararg.arg.clone());
                }
            }
            for arg in &args.kwonlyargs {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref kwarg) = args.kwarg {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                let name_str = kwarg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(kwarg.arg.clone());
                }
            }

            if is_async {
                unit.code.flags |= CodeFlags::COROUTINE;
            }
        }

        // Compile the function body
        // Extract docstring: if first statement is a string literal, store as first constant
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant {
                    value: Constant::Str(doc),
                } = &value.node
                {
                    // Ensure docstring is the first constant in the code object
                    let unit = self.current_unit_mut();
                    unit.code.docstring = Some(doc.clone());
                    let doc_const = ConstantValue::Str(doc.clone());
                    if unit.code.constants.is_empty() || unit.code.constants[0] != doc_const {
                        unit.code.constants.insert(0, doc_const);
                    }
                }
            }
        }
        self.compile_body(body)?;

        // Check if the function body contains yield — if so, mark as generator
        if body_contains_yield(body) {
            self.current_unit_mut().code.flags |= CodeFlags::GENERATOR;
        }

        // Ensure function returns None if no explicit return
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::ReturnValue);

        let func_code = self.pop_function_unit();

        // Build annotations dict from arg annotations and return type
        let all_args: Vec<&Arg> = args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.vararg.iter())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.iter())
            .collect();
        let mut ann_count: u32 = 0;
        for arg in &all_args {
            if let Some(ref annotation) = arg.annotation {
                let key = CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref());
                let key_idx = self.add_const(ConstantValue::Str(key));
                self.emit_arg(Opcode::LoadConst, key_idx);
                if self.future_annotations {
                    let ann_str = Self::annotation_to_string(annotation);
                    let idx = self.add_const(ConstantValue::Str(CompactString::from(ann_str)));
                    self.emit_arg(Opcode::LoadConst, idx);
                } else {
                    self.compile_expression(annotation)?;
                }
                ann_count += 1;
            }
        }
        if let Some(ret) = returns {
            let key_idx = self.add_const(ConstantValue::Str("return".into()));
            self.emit_arg(Opcode::LoadConst, key_idx);
            if self.future_annotations {
                let ann_str = Self::annotation_to_string(ret);
                let idx = self.add_const(ConstantValue::Str(CompactString::from(ann_str)));
                self.emit_arg(Opcode::LoadConst, idx);
            } else {
                self.compile_expression(ret)?;
            }
            ann_count += 1;
        }
        let has_annotations = ann_count > 0;
        if has_annotations {
            self.emit_arg(Opcode::BuildMap, ann_count);
        }

        // If the function has free variables, emit closure
        let has_closure = !func_code.freevars.is_empty();
        if has_closure {
            for freevar in &func_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = func_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        // Load the code object as a constant
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(func_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        // Load the qualified name
        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        // Determine MAKE_FUNCTION flags
        let mut make_fn_flags: u32 = 0;
        if num_defaults > 0 {
            make_fn_flags |= 0x01;
        }
        if has_kw_defaults {
            make_fn_flags |= 0x02;
        }
        if has_annotations {
            make_fn_flags |= 0x04;
        }
        if has_closure {
            make_fn_flags |= 0x08;
        }
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        // Apply decorators in reverse order
        for _ in decorator_list {
            self.emit_arg(Opcode::CallFunction, 1);
        }

        // Store the function name
        self.store_name(name);

        Ok(())
    }

    pub(in crate::compiler) fn push_function_unit(
        &mut self,
        name: &str,
        scope: Scope,
        qualname: &str,
    ) -> Result<()> {
        let mut unit = CompileUnit::new(name, &self.filename, scope, true, qualname.to_string());
        unit.code.qualname = CompactString::from(qualname);
        self.unit_stack.push(unit);
        Ok(())
    }

    pub(in crate::compiler) fn pop_function_unit(&mut self) -> CodeObject {
        let unit = self.unit_stack.pop().unwrap();
        let mut code = unit.code;
        code.num_locals = code.varnames.len() as u32;
        code
    }

    // ── class definition ────────────────────────────────────────────

    pub(super) fn compile_class_def(
        &mut self,
        name: &str,
        bases: &[Expression],
        keywords: &[Keyword],
        body: &[Statement],
        decorator_list: &[Expression],
        location: SourceLocation,
    ) -> Result<()> {
        // Compile decorators
        for dec in decorator_list {
            self.compile_expression(dec)?;
        }

        // LOAD_BUILD_CLASS
        self.emit_op(Opcode::LoadBuildClass);

        // Compile class body into a sub-CodeObject
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        let mut class_unit =
            CompileUnit::new(name, &self.filename, child_scope, false, qualname.clone());
        class_unit.code.flags = CodeFlags::empty();
        // The class body function takes __locals__ as implicit first arg
        class_unit.code.arg_count = 0;
        class_unit.class_name = Some(name.to_string());
        class_unit.code.first_line_number = location.line;
        self.unit_stack.push(class_unit);

        // __name__ = qualname
        let qname_idx = self.add_const(ConstantValue::Str(qualname.clone().into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);
        self.store_name("__qualname__");

        if Self::has_annotations(body) {
            self.emit_op(Opcode::SetupAnnotations);
        }

        // Extract docstring from first statement if it's a string literal
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant {
                    value: Constant::Str(doc),
                } = &value.node
                {
                    let doc_idx = self.add_const(ConstantValue::Str(doc.clone()));
                    self.emit_arg(Opcode::LoadConst, doc_idx);
                    self.store_name("__doc__");
                }
            }
        }

        // Compile the class body
        self.compile_body(body)?;

        // Return None from the class body
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::ReturnValue);

        let class_code = self.pop_function_unit();

        // Check if the class body needs closure cells
        let has_freevars = !class_code.freevars.is_empty();
        let num_freevars = class_code.freevars.len();

        // Emit closure cells BEFORE loading code/qualname (push order matters)
        if has_freevars {
            // For each freevar in the class code, emit LoadClosure from the parent scope
            for freevar_name in &class_code.freevars.clone() {
                // Find the cell index in the current (parent) scope
                let unit = self.current_unit();
                let cell_idx = unit
                    .code
                    .cellvars
                    .iter()
                    .position(|v| v == freevar_name)
                    .or_else(|| {
                        unit.code
                            .freevars
                            .iter()
                            .position(|v| v == freevar_name)
                            .map(|i| i + unit.code.cellvars.len())
                    });
                if let Some(idx) = cell_idx {
                    self.emit_arg(Opcode::LoadClosure, idx as u32);
                }
            }
            self.emit_arg(Opcode::BuildTuple, num_freevars as u32);
        }

        // Load the class body code object
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(class_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        // Load qualname for MAKE_FUNCTION
        let qname_const = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_const);

        // MAKE_FUNCTION with closure flag if needed
        let make_fn_flags = if has_freevars { 0x08 } else { 0 };
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        // Load class name as string arg
        let name_idx = self.add_const(ConstantValue::Str(name.into()));
        self.emit_arg(Opcode::LoadConst, name_idx);

        // Compile base classes
        for base in bases {
            self.compile_expression(base)?;
        }

        // Compile keyword args (e.g., metaclass=...)
        let num_kw = keywords.iter().filter(|k| k.arg.is_some()).count();
        for kw in keywords {
            if let Some(ref arg_name) = kw.arg {
                self.compile_expression(&kw.value)?;
                let _ = arg_name; // keyword arg names passed via CALL_FUNCTION_KW
            }
        }

        let total_args = 2 + bases.len() as u32; // func + name + bases
        if num_kw > 0 {
            // Build a tuple of keyword argument names
            let kw_names: Vec<ConstantValue> = keywords
                .iter()
                .filter_map(|k| k.arg.as_ref().map(|a| ConstantValue::Str(a.clone())))
                .collect();
            let kw_tuple_idx = self.add_const(ConstantValue::Tuple(kw_names));
            self.emit_arg(Opcode::LoadConst, kw_tuple_idx);
            self.emit_arg(Opcode::CallFunctionKw, total_args + num_kw as u32);
        } else {
            self.emit_arg(Opcode::CallFunction, total_args);
        }

        // Apply decorators in reverse order
        for _ in decorator_list {
            self.emit_arg(Opcode::CallFunction, 1);
        }

        // Store the class
        self.store_name(name);

        Ok(())
    }

    fn constant_to_annotation(value: &Constant) -> String {
        match value {
            Constant::Str(s) => format!("'{}'", s),
            Constant::Int(n) => match n {
                BigInt::Small(v) => v.to_string(),
                BigInt::Big(v) => v.to_string(),
            },
            Constant::Float(f) => f.to_string(),
            Constant::Complex { real, imag } => format!("{}+{}j", real, imag),
            Constant::None => "None".to_string(),
            Constant::Bool(true) => "True".to_string(),
            Constant::Bool(false) => "False".to_string(),
            Constant::Ellipsis => "...".to_string(),
            Constant::Bytes(_) => "b'...'".to_string(),
            Constant::Tuple(items) => {
                let parts: Vec<String> = items.iter().map(Self::constant_to_annotation).collect();
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            Constant::FrozenSet(_) => "frozenset(...)".to_string(),
        }
    }

    /// Convert an annotation expression AST to its source-code string representation.
    /// Used by PEP 563 (`from __future__ import annotations`) to store annotations as strings.
    pub(super) fn annotation_to_string(expr: &Expression) -> String {
        match &expr.node {
            ExpressionKind::Name { id, .. } => id.to_string(),
            ExpressionKind::Attribute { value, attr, .. } => {
                format!("{}.{}", Self::annotation_to_string(value), attr)
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                format!(
                    "{}[{}]",
                    Self::annotation_to_string(value),
                    Self::annotation_to_string(slice)
                )
            }
            ExpressionKind::Tuple { elts, .. } => {
                let parts: Vec<String> = elts.iter().map(Self::annotation_to_string).collect();
                parts.join(", ")
            }
            ExpressionKind::List { elts, .. } => {
                let parts: Vec<String> = elts.iter().map(Self::annotation_to_string).collect();
                format!("[{}]", parts.join(", "))
            }
            ExpressionKind::Constant { value } => Self::constant_to_annotation(value),
            ExpressionKind::BinOp { left, op, right } => {
                let op_str = match op {
                    Operator::BitOr => "|",
                    Operator::Add => "+",
                    Operator::Sub => "-",
                    Operator::Mult => "*",
                    Operator::Div => "/",
                    _ => "|",
                };
                format!(
                    "{} {} {}",
                    Self::annotation_to_string(left),
                    op_str,
                    Self::annotation_to_string(right)
                )
            }
            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                let mut parts: Vec<String> = args.iter().map(Self::annotation_to_string).collect();
                for kw in keywords {
                    if let Some(ref key) = kw.arg {
                        parts.push(format!("{}={}", key, Self::annotation_to_string(&kw.value)));
                    } else {
                        parts.push(format!("**{}", Self::annotation_to_string(&kw.value)));
                    }
                }
                format!("{}({})", Self::annotation_to_string(func), parts.join(", "))
            }
            ExpressionKind::UnaryOp { op, operand } => {
                let op_str = match op {
                    UnaryOperator::Not => "not ",
                    UnaryOperator::USub => "-",
                    UnaryOperator::UAdd => "+",
                    UnaryOperator::Invert => "~",
                };
                format!("{}{}", op_str, Self::annotation_to_string(operand))
            }
            ExpressionKind::BoolOp { op, values } => {
                let op_str = match op {
                    BoolOperator::And => " and ",
                    BoolOperator::Or => " or ",
                };
                let parts: Vec<String> = values.iter().map(Self::annotation_to_string).collect();
                parts.join(op_str)
            }
            ExpressionKind::IfExp { test, body, orelse } => {
                format!(
                    "{} if {} else {}",
                    Self::annotation_to_string(body),
                    Self::annotation_to_string(test),
                    Self::annotation_to_string(orelse)
                )
            }
            ExpressionKind::Starred { value, .. } => {
                format!("*{}", Self::annotation_to_string(value))
            }
            _ => "...".to_string(), // Fallback for unsupported expression types
        }
    }
}
