//! Expression compilation methods for the Compiler, plus utility functions.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, ConstantValue, Opcode};

use super::{Compiler, Result};

impl Compiler {
    // ── expression compilation ──────────────────────────────────────

    pub(super) fn compile_expression(&mut self, expr: &Expression) -> Result<()> {
        // Constant folding: evaluate constant expressions at compile time
        if let Some(folded) = self.try_fold_constant(expr) {
            let idx = self.add_const(folded);
            self.emit_arg(Opcode::LoadConst, idx);
            return Ok(());
        }

        match &expr.node {
            ExpressionKind::Constant { value } => {
                let cv = self.constant_to_value(value);
                let idx = self.add_const(cv);
                self.emit_arg(Opcode::LoadConst, idx);
            }

            ExpressionKind::Name { id, ctx } => {
                match ctx {
                    ExprContext::Load => self.load_name(id),
                    ExprContext::Store => self.store_name(id),
                    ExprContext::Del => self.delete_name(id),
                }
            }

            ExpressionKind::BinOp { left, op, right } => {
                self.compile_expression(left)?;
                self.compile_expression(right)?;
                self.emit_binary_op(*op);
            }

            ExpressionKind::UnaryOp { op, operand } => {
                self.compile_expression(operand)?;
                let opcode = match op {
                    UnaryOperator::Invert => Opcode::UnaryInvert,
                    UnaryOperator::Not => Opcode::UnaryNot,
                    UnaryOperator::UAdd => Opcode::UnaryPositive,
                    UnaryOperator::USub => Opcode::UnaryNegative,
                };
                self.emit_op(opcode);
            }

            ExpressionKind::BoolOp { op, values } => {
                self.compile_bool_op(*op, values)?;
            }

            ExpressionKind::Compare {
                left,
                ops,
                comparators,
            } => {
                self.compile_compare(left, ops, comparators)?;
            }

            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                self.compile_call(func, args, keywords)?;
            }

            ExpressionKind::Attribute { value, attr, ctx } => {
                let mangled = self.mangle_name(attr);
                match ctx {
                    ExprContext::Load => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(&mangled);
                        self.emit_arg(Opcode::LoadAttr, attr_idx);
                    }
                    ExprContext::Store => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(&mangled);
                        self.emit_arg(Opcode::StoreAttr, attr_idx);
                    }
                    ExprContext::Del => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(&mangled);
                        self.emit_arg(Opcode::DeleteAttr, attr_idx);
                    }
                }
            }

            ExpressionKind::Subscript { value, slice, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::BinarySubscr);
                    }
                    ExprContext::Store => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ExprContext::Del => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::DeleteSubscr);
                    }
                }
            }

            ExpressionKind::List { elts, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        let has_star = elts.iter().any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
                        if has_star {
                            // Build list then extend with starred elements
                            let mut regular_count = 0u32;
                            let mut started = false;
                            for elt in elts {
                                if let ExpressionKind::Starred { value, .. } = &elt.node {
                                    if !started {
                                        self.emit_arg(Opcode::BuildList, regular_count);
                                        started = true;
                                        regular_count = 0;
                                    } else if regular_count > 0 {
                                        self.emit_arg(Opcode::BuildList, regular_count);
                                        self.emit_arg(Opcode::ListExtend, 1);
                                        regular_count = 0;
                                    }
                                    self.compile_expression(value)?;
                                    self.emit_arg(Opcode::ListExtend, 1);
                                } else {
                                    self.compile_expression(elt)?;
                                    regular_count += 1;
                                    if !started {
                                        // still collecting initial elements
                                    }
                                }
                            }
                            if !started {
                                self.emit_arg(Opcode::BuildList, regular_count);
                            } else if regular_count > 0 {
                                self.emit_arg(Opcode::BuildList, regular_count);
                                self.emit_arg(Opcode::ListExtend, 1);
                            }
                        } else {
                            for elt in elts {
                                self.compile_expression(elt)?;
                            }
                            self.emit_arg(Opcode::BuildList, elts.len() as u32);
                        }
                    }
                    _ => {
                        // Store/Del contexts handled by compile_store_target
                    }
                }
            }

            ExpressionKind::Tuple { elts, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        let has_star = elts.iter().any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
                        if has_star {
                            // Build list, extend, convert to tuple
                            let mut regular_count = 0u32;
                            let mut started = false;
                            for elt in elts {
                                if let ExpressionKind::Starred { value, .. } = &elt.node {
                                    if !started {
                                        self.emit_arg(Opcode::BuildList, regular_count);
                                        started = true;
                                        regular_count = 0;
                                    } else if regular_count > 0 {
                                        self.emit_arg(Opcode::BuildList, regular_count);
                                        self.emit_arg(Opcode::ListExtend, 1);
                                        regular_count = 0;
                                    }
                                    self.compile_expression(value)?;
                                    self.emit_arg(Opcode::ListExtend, 1);
                                } else {
                                    self.compile_expression(elt)?;
                                    regular_count += 1;
                                }
                            }
                            if !started {
                                self.emit_arg(Opcode::BuildList, regular_count);
                            } else if regular_count > 0 {
                                self.emit_arg(Opcode::BuildList, regular_count);
                                self.emit_arg(Opcode::ListExtend, 1);
                            }
                            // Convert list to tuple
                            self.emit_arg(Opcode::ListToTuple, 0);
                        } else {
                            for elt in elts {
                                self.compile_expression(elt)?;
                            }
                            self.emit_arg(Opcode::BuildTuple, elts.len() as u32);
                        }
                    }
                    _ => {
                        // Store/Del contexts handled by compile_store_target
                    }
                }
            }

            ExpressionKind::Set { elts } => {
                let has_star = elts.iter().any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
                if has_star {
                    // Build empty set, then add regular elements and update with starred
                    self.emit_arg(Opcode::BuildSet, 0);
                    let mut n_regular = 0u32;
                    for elt in elts {
                        if let ExpressionKind::Starred { value, .. } = &elt.node {
                            // Flush accumulated regular elements
                            if n_regular > 0 {
                                self.emit_arg(Opcode::BuildSet, n_regular);
                                self.emit_arg(Opcode::SetUpdate, 1);
                                n_regular = 0;
                            }
                            self.compile_expression(value)?;
                            self.emit_arg(Opcode::SetUpdate, 1);
                        } else {
                            self.compile_expression(elt)?;
                            n_regular += 1;
                        }
                    }
                    if n_regular > 0 {
                        self.emit_arg(Opcode::BuildSet, n_regular);
                        self.emit_arg(Opcode::SetUpdate, 1);
                    }
                } else {
                    for elt in elts {
                        self.compile_expression(elt)?;
                    }
                    self.emit_arg(Opcode::BuildSet, elts.len() as u32);
                }
            }

            ExpressionKind::Dict { keys, values } => {
                // Check for dictionary unpacking (None keys indicate **)
                let has_unpacking = keys.iter().any(|k| k.is_none());
                if has_unpacking {
                    // Start with empty dict, then update with each segment
                    self.emit_arg(Opcode::BuildMap, 0); // empty base dict
                    let mut n_regular = 0u32;
                    for (key, val) in keys.iter().zip(values.iter()) {
                        if let Some(k) = key {
                            // Accumulate regular key-value pairs
                            self.compile_expression(k)?;
                            self.compile_expression(val)?;
                            n_regular += 1;
                        } else {
                            // Flush accumulated regular pairs first
                            if n_regular > 0 {
                                self.emit_arg(Opcode::BuildMap, n_regular);
                                self.emit_arg(Opcode::DictUpdate, 1);
                                n_regular = 0;
                            }
                            // Compile and merge the unpacked dict
                            self.compile_expression(val)?;
                            self.emit_arg(Opcode::DictUpdate, 1);
                        }
                    }
                    // Flush any remaining regular pairs
                    if n_regular > 0 {
                        self.emit_arg(Opcode::BuildMap, n_regular);
                        self.emit_arg(Opcode::DictUpdate, 1);
                    }
                } else {
                    for (key, val) in keys.iter().zip(values.iter()) {
                        if let Some(k) = key {
                            self.compile_expression(k)?;
                        }
                        self.compile_expression(val)?;
                    }
                    self.emit_arg(Opcode::BuildMap, keys.len() as u32);
                }
            }

            ExpressionKind::IfExp {
                test,
                body,
                orelse,
            } => {
                self.compile_expression(test)?;
                let else_label = self.emit_jump(Opcode::PopJumpIfFalse);
                self.compile_expression(body)?;
                let end_label = self.emit_jump(Opcode::JumpForward);
                self.patch_jump_here(else_label);
                self.compile_expression(orelse)?;
                self.patch_jump_here(end_label);
            }

            ExpressionKind::Lambda { args, body } => {
                self.compile_lambda(args, body, expr.location)?;
            }

            ExpressionKind::ListComp { elt, generators } => {
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(elt, None, generators, ComprehensionKind::List)?;
                } else {
                    self.compile_comprehension("<listcomp>", elt, None, generators, ComprehensionKind::List)?;
                }
            }

            ExpressionKind::SetComp { elt, generators } => {
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(elt, None, generators, ComprehensionKind::Set)?;
                } else {
                    self.compile_comprehension("<setcomp>", elt, None, generators, ComprehensionKind::Set)?;
                }
            }

            ExpressionKind::DictComp {
                key,
                value,
                generators,
            } => {
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(key, Some(value), generators, ComprehensionKind::Dict)?;
                } else {
                    self.compile_comprehension("<dictcomp>", key, Some(value), generators, ComprehensionKind::Dict)?;
                }
            }

            ExpressionKind::GeneratorExp { elt, generators } => {
                self.compile_comprehension("<genexpr>", elt, None, generators, ComprehensionKind::Generator)?;
            }

            ExpressionKind::Yield { value } => {
                if let Some(val) = value {
                    self.compile_expression(val)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                self.emit_op(Opcode::YieldValue);
            }

            ExpressionKind::YieldFrom { value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::GetIter);
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_op(Opcode::YieldFrom);
            }

            ExpressionKind::Await { value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::GetAwaitable);
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_op(Opcode::YieldFrom);
            }

            ExpressionKind::Starred { value, .. } => {
                // In expression context (e.g., [*a, b]), starred should be handled
                // by the parent (List, Call, etc.). If we reach here, just compile normally.
                self.compile_expression(value)?;
            }

            ExpressionKind::Slice {
                lower,
                upper,
                step,
            } => {
                let argc = if step.is_some() { 3 } else { 2 };
                if let Some(l) = lower {
                    self.compile_expression(l)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                if let Some(u) = upper {
                    self.compile_expression(u)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                if let Some(s) = step {
                    self.compile_expression(s)?;
                }
                self.emit_arg(Opcode::BuildSlice, argc);
            }

            ExpressionKind::NamedExpr { target, value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::DupTop);
                self.compile_store_target(target)?;
            }

            ExpressionKind::FormattedValue {
                value,
                conversion,
                format_spec,
            } => {
                self.compile_expression(value)?;
                let mut flags = 0u32;
                // Conversion: -1 = no conversion, 's' = str, 'r' = repr, 'a' = ascii
                match conversion {
                    Some('s') => flags |= 0x01,
                    Some('r') => flags |= 0x02,
                    Some('a') => flags |= 0x03,
                    _ => {}
                }
                if let Some(spec) = format_spec {
                    self.compile_expression(spec)?;
                    flags |= 0x04;
                }
                self.emit_arg(Opcode::FormatValue, flags);
            }

            ExpressionKind::JoinedStr { values } => {
                for v in values {
                    self.compile_expression(v)?;
                }
                self.emit_arg(Opcode::BuildString, values.len() as u32);
            }
        }
        Ok(())
    }

    // ── binary operator emission ────────────────────────────────────

    pub(super) fn emit_binary_op(&mut self, op: Operator) {
        let opcode = match op {
            Operator::Add => Opcode::BinaryAdd,
            Operator::Sub => Opcode::BinarySubtract,
            Operator::Mult => Opcode::BinaryMultiply,
            Operator::MatMult => Opcode::BinaryMatrixMultiply,
            Operator::Div => Opcode::BinaryTrueDivide,
            Operator::FloorDiv => Opcode::BinaryFloorDivide,
            Operator::Mod => Opcode::BinaryModulo,
            Operator::Pow => Opcode::BinaryPower,
            Operator::LShift => Opcode::BinaryLshift,
            Operator::RShift => Opcode::BinaryRshift,
            Operator::BitOr => Opcode::BinaryOr,
            Operator::BitXor => Opcode::BinaryXor,
            Operator::BitAnd => Opcode::BinaryAnd,
        };
        self.emit_op(opcode);
    }

    // ── boolean operator (short-circuit) ────────────────────────────

    pub(super) fn compile_bool_op(
        &mut self,
        op: BoolOperator,
        values: &[Expression],
    ) -> Result<()> {
        let jump_op = match op {
            BoolOperator::And => Opcode::JumpIfFalseOrPop,
            BoolOperator::Or => Opcode::JumpIfTrueOrPop,
        };

        let mut labels = Vec::new();
        for (i, val) in values.iter().enumerate() {
            self.compile_expression(val)?;
            if i < values.len() - 1 {
                labels.push(self.emit_jump(jump_op));
            }
        }
        let end = self.current_offset();
        for label in labels {
            self.patch_jump(label, end);
        }
        Ok(())
    }

    // ── comparison (chained) ────────────────────────────────────────

    pub(super) fn compile_compare(
        &mut self,
        left: &Expression,
        ops: &[CompareOperator],
        comparators: &[Expression],
    ) -> Result<()> {
        self.compile_expression(left)?;

        if ops.len() == 1 {
            // Simple comparison: left op right
            self.compile_expression(&comparators[0])?;
            let cmp_arg = compare_op_arg(ops[0]);
            self.emit_arg(Opcode::CompareOp, cmp_arg);
        } else {
            // Chained: a < b < c → (a < b) and (b < c)
            let mut cleanup_labels = Vec::new();
            for (i, (op, comp)) in ops.iter().zip(comparators.iter()).enumerate() {
                self.compile_expression(comp)?;
                if i < ops.len() - 1 {
                    // Need to keep the intermediate value
                    self.emit_op(Opcode::DupTop);
                    self.emit_op(Opcode::RotThree);
                    let cmp_arg = compare_op_arg(*op);
                    self.emit_arg(Opcode::CompareOp, cmp_arg);
                    cleanup_labels.push(self.emit_jump(Opcode::JumpIfFalseOrPop));
                } else {
                    let cmp_arg = compare_op_arg(*op);
                    self.emit_arg(Opcode::CompareOp, cmp_arg);
                }
            }
            let end = self.current_offset();
            for label in &cleanup_labels {
                // When short-circuiting, we need to clean up the extra value
                self.patch_jump(*label, end);
            }
            // If we short-circuited, RotTwo to get rid of the extra copy
            if !cleanup_labels.is_empty() {
                let ok_label = self.emit_jump(Opcode::JumpForward);
                let cleanup_target = self.current_offset();
                self.emit_op(Opcode::RotTwo);
                self.emit_op(Opcode::PopTop);
                self.patch_jump_here(ok_label);
                // Re-patch cleanup labels to the cleanup target
                for label in cleanup_labels {
                    self.patch_jump(label, cleanup_target);
                }
            }
        }
        Ok(())
    }

    // ── function call ───────────────────────────────────────────────

    pub(super) fn compile_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        keywords: &[Keyword],
    ) -> Result<()> {
        // Check if any arg is starred or any keyword has None arg (** unpacking)
        let has_star_args = args.iter().any(|a| matches!(a.node, ExpressionKind::Starred { .. }));
        let has_double_star = keywords.iter().any(|k| k.arg.is_none());

        if has_star_args || has_double_star {
            // Use CALL_FUNCTION_EX
            self.compile_expression(func)?;
            self.compile_star_args(args)?;
            if keywords.is_empty() {
                self.emit_arg(Opcode::CallFunctionEx, 0);
            } else {
                self.compile_star_kwargs(keywords)?;
                self.emit_arg(Opcode::CallFunctionEx, 1);
            }
        } else if !keywords.is_empty() {
            // Use CALL_FUNCTION_KW
            self.compile_expression(func)?;
            for arg in args {
                self.compile_expression(arg)?;
            }
            for kw in keywords {
                self.compile_expression(&kw.value)?;
            }
            let kw_names: Vec<ConstantValue> = keywords
                .iter()
                .map(|k| ConstantValue::Str(k.arg.as_ref().unwrap().clone()))
                .collect();
            let tuple_idx = self.add_const(ConstantValue::Tuple(kw_names));
            self.emit_arg(Opcode::LoadConst, tuple_idx);
            let total = (args.len() + keywords.len()) as u32;
            self.emit_arg(Opcode::CallFunctionKw, total);
        } else if let ExpressionKind::Attribute { value, attr, ctx: ExprContext::Load } = &func.node {
            // Optimization: obj.method(args) → LoadMethod + CallMethod
            // Avoids creating a BoundMethod wrapper on every call
            self.compile_expression(value)?;
            let mangled = self.mangle_name(attr);
            let attr_idx = self.add_name(&mangled);
            self.emit_arg(Opcode::LoadMethod, attr_idx);
            for arg in args {
                self.compile_expression(arg)?;
            }
            self.emit_arg(Opcode::CallMethod, args.len() as u32);
        } else {
            // Simple CALL_FUNCTION
            self.compile_expression(func)?;
            for arg in args {
                self.compile_expression(arg)?;
            }
            self.emit_arg(Opcode::CallFunction, args.len() as u32);
        }
        Ok(())
    }

    pub(super) fn compile_star_args(&mut self, args: &[Expression]) -> Result<()> {
        // Count segments: contiguous regular args form one tuple segment,
        // each starred arg is its own segment.
        let star_count = args.iter()
            .filter(|a| matches!(a.node, ExpressionKind::Starred { .. }))
            .count();

        if star_count == 0 {
            // No star args — simple tuple
            for arg in args {
                self.compile_expression(arg)?;
            }
            self.emit_arg(Opcode::BuildTuple, args.len() as u32);
            return Ok(());
        }

        if star_count == 1 && !args.iter().any(|a| !matches!(a.node, ExpressionKind::Starred { .. })) {
            // Single starred arg, no regular args: just compile the value
            if let ExpressionKind::Starred { value, .. } = &args[0].node {
                self.compile_expression(value)?;
            }
            return Ok(());
        }

        // Multiple segments: build a list incrementally, then use it directly.
        // CallFunctionEx calls collect_iterable() which handles lists.
        //
        // Strategy: push an empty list, then for each segment:
        //   - regular args: BuildTuple(n) + ListExtend(1)
        //   - starred arg: compile value + ListExtend(1)
        self.emit_arg(Opcode::BuildList, 0); // accumulator list

        let mut regular_start = None;
        let mut n_regular = 0u32;

        for (i, arg) in args.iter().enumerate() {
            if let ExpressionKind::Starred { value, .. } = &arg.node {
                // Flush pending regular args
                if n_regular > 0 {
                    // We already compiled the regular args but they're on top of the list.
                    // BuildTuple collects from stack, then ListExtend merges into the list below.
                    self.emit_arg(Opcode::BuildTuple, n_regular);
                    self.emit_arg(Opcode::ListExtend, 1);
                    n_regular = 0;
                    regular_start = None;
                }
                self.compile_expression(value)?;
                self.emit_arg(Opcode::ListExtend, 1);
            } else {
                if regular_start.is_none() {
                    regular_start = Some(i);
                }
                self.compile_expression(arg)?;
                n_regular += 1;
            }
        }

        // Flush any remaining regular args
        if n_regular > 0 {
            self.emit_arg(Opcode::BuildTuple, n_regular);
            self.emit_arg(Opcode::ListExtend, 1);
        }

        // The accumulator list now contains all args merged.
        // CallFunctionEx's collect_iterable handles lists.
        Ok(())
    }

    pub(super) fn compile_star_kwargs(&mut self, keywords: &[Keyword]) -> Result<()> {
        let mut n_regular = 0u32;
        let mut segments = 0u32;

        for kw in keywords {
            if kw.arg.is_none() {
                // ** unpacking
                if n_regular > 0 {
                    self.emit_arg(Opcode::BuildMap, n_regular);
                    segments += 1;
                    n_regular = 0;
                }
                self.compile_expression(&kw.value)?;
                segments += 1;
            } else {
                let key_idx =
                    self.add_const(ConstantValue::Str(kw.arg.as_ref().unwrap().clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(&kw.value)?;
                n_regular += 1;
            }
        }
        if n_regular > 0 {
            self.emit_arg(Opcode::BuildMap, n_regular);
            segments += 1;
        }

        if segments == 0 {
            self.emit_arg(Opcode::BuildMap, 0);
        } else if segments > 1 {
            // Merge dicts
            let first = segments;
            self.emit_arg(Opcode::BuildMap, 0);
            for _ in 0..first {
                self.emit_arg(Opcode::DictMerge, 1);
            }
        }
        Ok(())
    }

    // ── lambda ──────────────────────────────────────────────────────

    pub(super) fn compile_lambda(
        &mut self,
        args: &Arguments,
        body: &Expression,
        location: SourceLocation,
    ) -> Result<()> {
        // Compile defaults in enclosing scope
        let num_defaults = args.defaults.len();
        if num_defaults > 0 {
            for default in &args.defaults {
                self.compile_expression(default)?;
            }
            self.emit_arg(Opcode::BuildTuple, num_defaults as u32);
        }

        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            "<lambda>".to_string()
        } else {
            format!("{}.<lambda>", qualname_prefix)
        };

        self.push_function_unit("<lambda>", child_scope, &qualname)?;

        // Set the first line number from the lambda location
        self.current_unit_mut().code.first_line_number = location.line;

        // Set up argument info
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count =
                (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            for arg in &args.posonlyargs {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            for arg in &args.args {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref vararg) = args.vararg {
                unit.code.flags |= CodeFlags::VARARGS;
                if !unit.code.varnames.iter().any(|v| v.as_str() == vararg.arg.as_str()) {
                    unit.code.varnames.push(vararg.arg.clone());
                }
            }
            for arg in &args.kwonlyargs {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref kwarg) = args.kwarg {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                if !unit.code.varnames.iter().any(|v| v.as_str() == kwarg.arg.as_str()) {
                    unit.code.varnames.push(kwarg.arg.clone());
                }
            }
        }

        // Compile the lambda body and return it
        self.compile_expression(body)?;
        self.emit_op(Opcode::ReturnValue);

        let func_code = self.pop_function_unit();

        // If the lambda has free variables, emit closure
        let has_closure = !func_code.freevars.is_empty();
        if has_closure {
            for freevar in &func_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = func_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(func_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        let mut make_fn_flags: u32 = 0;
        if num_defaults > 0 {
            make_fn_flags |= 0x01;
        }
        if has_closure {
            make_fn_flags |= 0x08;
        }
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        Ok(())
    }

    // ── comprehensions ──────────────────────────────────────────────

    pub(super) fn compile_comprehension(
        &mut self,
        name: &str,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        kind: ComprehensionKind,
    ) -> Result<()> {
        // CPython semantics: the first generator's iterable is evaluated in
        // the enclosing scope BEFORE the comprehension scope is entered.
        // We must compile it first to match symbol table child ordering.
        self.compile_expression(&generators[0].iter)?;
        self.emit_op(Opcode::GetIter);
        // Stack: [iter]

        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        // Compile the comprehension function body
        self.push_function_unit(name, child_scope, &qualname)?;

        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = 1;
            unit.code.varnames.push(".0".into());
            if matches!(kind, ComprehensionKind::Generator) {
                unit.code.flags |= CodeFlags::GENERATOR;
            }
        }

        match kind {
            ComprehensionKind::List => {
                self.emit_arg(Opcode::BuildList, 0);
            }
            ComprehensionKind::Set => {
                self.emit_arg(Opcode::BuildSet, 0);
            }
            ComprehensionKind::Dict => {
                self.emit_arg(Opcode::BuildMap, 0);
            }
            ComprehensionKind::Generator => {}
        }

        self.compile_comprehension_generators(generators, 0, elt, value, kind)?;

        match kind {
            ComprehensionKind::Generator => {
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
            }
            _ => {}
        }
        self.emit_op(Opcode::ReturnValue);

        let comp_code = self.pop_function_unit();

        // If the comprehension has free variables, emit closure
        let has_closure = !comp_code.freevars.is_empty();
        if has_closure {
            for freevar in &comp_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = comp_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        // Back in the enclosing scope — emit function creation
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(comp_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        self.emit_arg(Opcode::MakeFunction, if has_closure { 0x08 } else { 0 });

        // Stack: [iter, fn] — need [fn, iter] for CallFunction
        self.emit_op(Opcode::RotTwo);

        // Call fn(iter)
        self.emit_arg(Opcode::CallFunction, 1);

        Ok(())
    }

    pub(super) fn compile_comprehension_generators(
        &mut self,
        generators: &[Comprehension],
        idx: usize,
        elt: &Expression,
        value: Option<&Expression>,
        kind: ComprehensionKind,
    ) -> Result<()> {
        let gen = &generators[idx];

        // Load the iterator (first one comes from argument, rest are created here)
        if idx == 0 {
            let iter_idx = self.varname_index(".0");
            self.emit_arg(Opcode::LoadFast, iter_idx);
        } else {
            self.compile_expression(&gen.iter)?;
            self.emit_op(Opcode::GetIter);
        }

        let loop_start = self.current_offset();
        let done_label = self.emit_jump(Opcode::ForIter);

        // Store the iteration variable
        self.compile_store_target(&gen.target)?;

        // Compile filter conditions
        let mut skip_labels = Vec::new();
        for cond in &gen.ifs {
            self.compile_expression(cond)?;
            skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
        }

        if idx + 1 < generators.len() {
            self.compile_comprehension_generators(generators, idx + 1, elt, value, kind)?;
        } else {
            match kind {
                ComprehensionKind::List => {
                    self.compile_expression(elt)?;
                    self.emit_arg(Opcode::ListAppend, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Set => {
                    self.compile_expression(elt)?;
                    self.emit_arg(Opcode::SetAdd, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Dict => {
                    self.compile_expression(elt)?;
                    self.compile_expression(value.unwrap())?;
                    self.emit_arg(Opcode::MapAdd, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Generator => {
                    self.compile_expression(elt)?;
                    self.emit_op(Opcode::YieldValue);
                    self.emit_op(Opcode::PopTop);
                }
            }
        }

        let cont_target = self.current_offset();
        for label in skip_labels {
            self.patch_jump(label, cont_target);
        }

        self.emit_arg(Opcode::JumpAbsolute, loop_start);
        self.patch_jump_here(done_label);

        Ok(())
    }

    /// Compile an async comprehension inline (without a separate coroutine function).
    /// Uses a temp variable for the result container to avoid stack corruption
    /// from SetupExcept/EndAsyncFor.
    pub(super) fn compile_async_comprehension_inline(
        &mut self,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        kind: ComprehensionKind,
    ) -> Result<()> {
        // Build the empty result container and store in temp var
        let result_temp = CompactString::from("$async_comp_result$");
        let result_idx = self.varname_index(&result_temp);
        match kind {
            ComprehensionKind::List => self.emit_arg(Opcode::BuildList, 0),
            ComprehensionKind::Set => self.emit_arg(Opcode::BuildSet, 0),
            ComprehensionKind::Dict => self.emit_arg(Opcode::BuildMap, 0),
            ComprehensionKind::Generator => 0,
        };
        self.emit_arg(Opcode::StoreFast, result_idx);

        self.compile_async_comp_generator(generators, 0, elt, value, kind, result_idx)?;

        // Load result back onto stack
        self.emit_arg(Opcode::LoadFast, result_idx);
        Ok(())
    }

    fn compile_async_comp_generator(
        &mut self,
        generators: &[Comprehension],
        idx: usize,
        elt: &Expression,
        value: Option<&Expression>,
        kind: ComprehensionKind,
        result_idx: u32,
    ) -> Result<()> {
        let gen = &generators[idx];

        self.compile_expression(&gen.iter)?;
        if gen.is_async {
            self.emit_op(Opcode::GetAiter);
        } else {
            self.emit_op(Opcode::GetIter);
        }

        let loop_start = self.current_offset();

        if gen.is_async {
            let except_label = self.emit_jump(Opcode::SetupExcept);
            self.emit_op(Opcode::GetAnext);
            self.emit_op(Opcode::GetAwaitable);
            let none_idx = self.add_const(ConstantValue::None);
            self.emit_arg(Opcode::LoadConst, none_idx);
            self.emit_op(Opcode::YieldFrom);
            self.compile_store_target(&gen.target)?;
            self.emit_op(Opcode::PopBlock);

            let mut skip_labels = Vec::new();
            for cond in &gen.ifs {
                self.compile_expression(cond)?;
                skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
            }

            if idx + 1 < generators.len() {
                self.compile_async_comp_generator(generators, idx + 1, elt, value, kind, result_idx)?;
            } else {
                // Innermost: load result, append element, store back
                match kind {
                    ComprehensionKind::List => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let append_name = self.add_name("append");
                        self.emit_arg(Opcode::LoadAttr, append_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Set => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let add_name = self.add_name("add");
                        self.emit_arg(Opcode::LoadAttr, add_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Dict => {
                        // StoreSubscr stack: TOS=key, TOS1=obj, TOS2=value
                        self.compile_expression(value.unwrap())?; // value
                        self.emit_arg(Opcode::LoadFast, result_idx); // dict
                        self.compile_expression(elt)?;   // key
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ComprehensionKind::Generator => {
                        self.compile_expression(elt)?;
                        self.emit_op(Opcode::YieldValue);
                        self.emit_op(Opcode::PopTop);
                    }
                }
            }

            let cont_target = self.current_offset();
            for label in skip_labels {
                self.patch_jump(label, cont_target);
            }
            self.emit_arg(Opcode::JumpAbsolute, loop_start);
            self.patch_jump_here(except_label);
            self.emit_op(Opcode::EndAsyncFor);
        } else {
            let done_label = self.emit_jump(Opcode::ForIter);
            self.compile_store_target(&gen.target)?;

            let mut skip_labels = Vec::new();
            for cond in &gen.ifs {
                self.compile_expression(cond)?;
                skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
            }

            if idx + 1 < generators.len() {
                self.compile_async_comp_generator(generators, idx + 1, elt, value, kind, result_idx)?;
            } else {
                match kind {
                    ComprehensionKind::List => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let append_name = self.add_name("append");
                        self.emit_arg(Opcode::LoadAttr, append_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Set => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let add_name = self.add_name("add");
                        self.emit_arg(Opcode::LoadAttr, add_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Dict => {
                        self.compile_expression(value.unwrap())?; // value
                        self.emit_arg(Opcode::LoadFast, result_idx); // dict
                        self.compile_expression(elt)?;   // key
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ComprehensionKind::Generator => {
                        self.compile_expression(elt)?;
                        self.emit_op(Opcode::YieldValue);
                        self.emit_op(Opcode::PopTop);
                    }
                }
            }

            let cont_target = self.current_offset();
            for label in skip_labels {
                self.patch_jump(label, cont_target);
            }
            self.emit_arg(Opcode::JumpAbsolute, loop_start);
            self.patch_jump_here(done_label);
        }

        Ok(())
    }

    // ── constant folding ──────────────────────────────────────────────

    /// Try to evaluate a constant expression at compile time.
    /// Returns Some(value) if folded, None if not foldable.
    fn try_fold_constant(&self, expr: &Expression) -> Option<ConstantValue> {
        match &expr.node {
            ExpressionKind::UnaryOp { op, operand } => {
                let c = Self::extract_constant(operand)?;
                Self::fold_unary(*op, &c)
            }
            ExpressionKind::BinOp { left, op, right } => {
                let lc = Self::extract_constant(left)?;
                let rc = Self::extract_constant(right)?;
                Self::fold_binop(&lc, *op, &rc)
            }
            _ => None,
        }
    }

    /// Extract a constant value from an expression (including nested folding).
    fn extract_constant(expr: &Expression) -> Option<Constant> {
        match &expr.node {
            ExpressionKind::Constant { value } => Some(value.clone()),
            // Fold through unary minus on literals: e.g., -1 in `(-1) + 2`
            ExpressionKind::UnaryOp { op: UnaryOperator::USub, operand } => {
                if let ExpressionKind::Constant { value } = &operand.node {
                    match value {
                        Constant::Int(BigInt::Small(i)) => Some(Constant::Int(BigInt::Small(-i))),
                        Constant::Float(f) => Some(Constant::Float(-f)),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn fold_unary(op: UnaryOperator, c: &Constant) -> Option<ConstantValue> {
        match (op, c) {
            (UnaryOperator::USub, Constant::Int(BigInt::Small(i))) => {
                Some(ConstantValue::Integer(-i))
            }
            (UnaryOperator::USub, Constant::Float(f)) => {
                Some(ConstantValue::Float(-f))
            }
            (UnaryOperator::UAdd, Constant::Int(BigInt::Small(i))) => {
                Some(ConstantValue::Integer(*i))
            }
            (UnaryOperator::UAdd, Constant::Float(f)) => {
                Some(ConstantValue::Float(*f))
            }
            (UnaryOperator::Not, Constant::Bool(b)) => {
                Some(ConstantValue::Bool(!b))
            }
            (UnaryOperator::Invert, Constant::Int(BigInt::Small(i))) => {
                Some(ConstantValue::Integer(!i))
            }
            _ => None,
        }
    }

    fn fold_binop(left: &Constant, op: Operator, right: &Constant) -> Option<ConstantValue> {
        match (left, right) {
            (Constant::Int(BigInt::Small(a)), Constant::Int(BigInt::Small(b))) => {
                Self::fold_int_binop(*a, op, *b)
            }
            (Constant::Float(a), Constant::Float(b)) => {
                Self::fold_float_binop(*a, op, *b)
            }
            (Constant::Int(BigInt::Small(a)), Constant::Float(b)) => {
                Self::fold_float_binop(*a as f64, op, *b)
            }
            (Constant::Float(a), Constant::Int(BigInt::Small(b))) => {
                Self::fold_float_binop(*a, op, *b as f64)
            }
            (Constant::Str(a), Constant::Str(b)) if op == Operator::Add => {
                let mut s = a.clone();
                s.push_str(b);
                Some(ConstantValue::Str(s))
            }
            (Constant::Str(a), Constant::Int(BigInt::Small(n)))
                if op == Operator::Mult && *n >= 0 && *n <= 4096 =>
            {
                Some(ConstantValue::Str(a.repeat(*n as usize)))
            }
            _ => None,
        }
    }

    fn fold_int_binop(a: i64, op: Operator, b: i64) -> Option<ConstantValue> {
        let result = match op {
            Operator::Add => a.checked_add(b)?,
            Operator::Sub => a.checked_sub(b)?,
            Operator::Mult => a.checked_mul(b)?,
            Operator::FloorDiv => {
                if b == 0 { return None; }
                let (q, r) = (a / b, a % b);
                if (r != 0) && ((r ^ b) < 0) { Some(q - 1) } else { Some(q) }
            }?,
            Operator::Mod => {
                if b == 0 { return None; }
                let r = a % b;
                if (r != 0) && ((r ^ b) < 0) { Some(r + b) } else { Some(r) }
            }?,
            Operator::Pow => {
                if b < 0 { return None; } // negative power → float
                if b > 63 { return None; } // prevent huge results
                a.checked_pow(b as u32)?
            }
            Operator::LShift => {
                if b < 0 || b > 63 { return None; }
                a.checked_shl(b as u32)?
            }
            Operator::RShift => {
                if b < 0 || b > 63 { return None; }
                Some(a >> b as u32)
            }?,
            Operator::BitOr => a | b,
            Operator::BitXor => a ^ b,
            Operator::BitAnd => a & b,
            _ => return None, // Div, MatMult → not foldable to int
        };
        Some(ConstantValue::Integer(result))
    }

    fn fold_float_binop(a: f64, op: Operator, b: f64) -> Option<ConstantValue> {
        let result = match op {
            Operator::Add => a + b,
            Operator::Sub => a - b,
            Operator::Mult => a * b,
            Operator::Div => {
                if b == 0.0 { return None; }
                a / b
            }
            Operator::FloorDiv => {
                if b == 0.0 { return None; }
                (a / b).floor()
            }
            Operator::Mod => {
                if b == 0.0 { return None; }
                a % b
            }
            Operator::Pow => a.powf(b),
            _ => return None,
        };
        if result.is_nan() || result.is_infinite() { return None; }
        Some(ConstantValue::Float(result))
    }

    // ── constant conversion ─────────────────────────────────────────

    pub(super) fn constant_to_value(&self, constant: &Constant) -> ConstantValue {
        match constant {
            Constant::None => ConstantValue::None,
            Constant::Bool(b) => ConstantValue::Bool(*b),
            Constant::Int(BigInt::Small(i)) => ConstantValue::Integer(*i),
            Constant::Int(BigInt::Big(b)) => {
                ConstantValue::BigInteger(b.clone())
            }
            Constant::Float(f) => ConstantValue::Float(*f),
            Constant::Complex { real, imag } => ConstantValue::Complex {
                real: *real,
                imag: *imag,
            },
            Constant::Str(s) => ConstantValue::Str(s.clone()),
            Constant::Bytes(b) => ConstantValue::Bytes(b.clone()),
            Constant::Ellipsis => ConstantValue::Ellipsis,
        }
    }
}

/// What kind of comprehension we're compiling.
#[derive(Debug, Clone, Copy)]
pub(super) enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}

/// Map a comparison operator to the CPython compare op argument.

pub(super) fn compare_op_arg(op: CompareOperator) -> u32 {
    match op {
        CompareOperator::Lt => 0,
        CompareOperator::LtE => 1,
        CompareOperator::Eq => 2,
        CompareOperator::NotEq => 3,
        CompareOperator::Gt => 4,
        CompareOperator::GtE => 5,
        CompareOperator::In => 6,
        CompareOperator::NotIn => 7,
        CompareOperator::Is => 8,
        CompareOperator::IsNot => 9,
    }
}

/// Check if a function body (list of statements) contains any `yield` or `yield from` expressions.
/// Only checks the direct body — does NOT recurse into nested function/class defs.
pub(super) fn body_contains_yield(stmts: &[Statement]) -> bool {
    for stmt in stmts {
        if stmt_contains_yield(stmt) { return true; }
    }
    false
}

pub(super) fn stmt_contains_yield(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Expr { value } => expr_contains_yield(value),
        StatementKind::Return { value } => value.as_ref().map_or(false, |v| expr_contains_yield(v)),
        StatementKind::Assign { value, .. } => expr_contains_yield(value),
        StatementKind::AugAssign { value, .. } => expr_contains_yield(value),
        StatementKind::If { test, body, orelse } => {
            expr_contains_yield(test) || body_contains_yield(body) || body_contains_yield(orelse)
        }
        StatementKind::While { test, body, orelse } => {
            expr_contains_yield(test) || body_contains_yield(body) || body_contains_yield(orelse)
        }
        StatementKind::For { body, orelse, iter, .. } => {
            expr_contains_yield(iter) || body_contains_yield(body) || body_contains_yield(orelse)
        }
        StatementKind::Try { body, handlers, orelse, finalbody } => {
            body_contains_yield(body) || body_contains_yield(orelse) || body_contains_yield(finalbody)
            || handlers.iter().any(|h| body_contains_yield(&h.body))
        }
        StatementKind::With { body, .. } => body_contains_yield(body),
        // Do NOT recurse into nested function/class definitions
        StatementKind::FunctionDef { .. } | StatementKind::ClassDef { .. } => false,
        _ => false,
    }
}

pub(super) fn expr_contains_yield(expr: &Expression) -> bool {
    match &expr.node {
        ExpressionKind::Yield { .. } | ExpressionKind::YieldFrom { .. } => true,
        ExpressionKind::BinOp { left, right, .. } => {
            expr_contains_yield(left) || expr_contains_yield(right)
        }
        ExpressionKind::UnaryOp { operand, .. } => expr_contains_yield(operand),
        ExpressionKind::BoolOp { values, .. } => values.iter().any(|v| expr_contains_yield(v)),
        ExpressionKind::Call { func, args, keywords } => {
            expr_contains_yield(func) || args.iter().any(|a| expr_contains_yield(a))
            || keywords.iter().any(|k| expr_contains_yield(&k.value))
        }
        ExpressionKind::IfExp { test, body, orelse } => {
            expr_contains_yield(test) || expr_contains_yield(body) || expr_contains_yield(orelse)
        }
        ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } | ExpressionKind::Set { elts } => {
            elts.iter().any(|e| expr_contains_yield(e))
        }
        ExpressionKind::Dict { keys, values } => {
            keys.iter().any(|k| k.as_ref().map_or(false, |k| expr_contains_yield(k))) || values.iter().any(|v| expr_contains_yield(v))
        }
        ExpressionKind::Attribute { value, .. } => expr_contains_yield(value),
        ExpressionKind::Subscript { value, slice, .. } => {
            expr_contains_yield(value) || expr_contains_yield(slice)
        }
        ExpressionKind::Compare { left, comparators, .. } => {
            expr_contains_yield(left) || comparators.iter().any(|c| expr_contains_yield(c))
        }
        _ => false,
    }
}
