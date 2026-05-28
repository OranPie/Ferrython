//! Expression compilation methods for the Compiler, plus utility functions.

use ferrython_ast::*;
use ferrython_bytecode::{ConstantValue, Opcode};

use super::{Compiler, Result};
use crate::error::CompileError;

mod calls;
mod comprehensions;
mod constants;

pub(super) use comprehensions::{body_contains_yield, compare_op_arg, ComprehensionKind};

impl Compiler {
    // ── expression compilation ──────────────────────────────────────

    pub(super) fn compile_expression(&mut self, expr: &Expression) -> Result<()> {
        self.validate_int_literal_limit(expr)?;

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

            ExpressionKind::Name { id, ctx } => match ctx {
                ExprContext::Load if id.as_str() == "__debug__" => {
                    let idx = self.add_const(ConstantValue::Bool(true));
                    self.emit_arg(Opcode::LoadConst, idx);
                }
                ExprContext::Load => self.load_name(id),
                ExprContext::Store => self.store_name(id),
                ExprContext::Del => self.delete_name(id),
            },

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

            ExpressionKind::Subscript { value, slice, ctx } => match ctx {
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
            },

            ExpressionKind::List { elts, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        let has_star = elts
                            .iter()
                            .any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
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
                        let has_star = elts
                            .iter()
                            .any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
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
                let has_star = elts
                    .iter()
                    .any(|e| matches!(e.node, ExpressionKind::Starred { .. }));
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

            ExpressionKind::IfExp { test, body, orelse } => {
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
                Self::validate_comprehension_yields(
                    ComprehensionKind::List,
                    elt,
                    None,
                    generators,
                    expr.location,
                )?;
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(
                        elt,
                        None,
                        generators,
                        ComprehensionKind::List,
                    )?;
                } else {
                    self.compile_comprehension(
                        "<listcomp>",
                        elt,
                        None,
                        generators,
                        ComprehensionKind::List,
                    )?;
                }
            }

            ExpressionKind::SetComp { elt, generators } => {
                Self::validate_comprehension_yields(
                    ComprehensionKind::Set,
                    elt,
                    None,
                    generators,
                    expr.location,
                )?;
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(
                        elt,
                        None,
                        generators,
                        ComprehensionKind::Set,
                    )?;
                } else {
                    self.compile_comprehension(
                        "<setcomp>",
                        elt,
                        None,
                        generators,
                        ComprehensionKind::Set,
                    )?;
                }
            }

            ExpressionKind::DictComp {
                key,
                value,
                generators,
            } => {
                Self::validate_comprehension_yields(
                    ComprehensionKind::Dict,
                    key,
                    Some(value),
                    generators,
                    expr.location,
                )?;
                if generators.iter().any(|g| g.is_async) {
                    self.compile_async_comprehension_inline(
                        key,
                        Some(value),
                        generators,
                        ComprehensionKind::Dict,
                    )?;
                } else {
                    self.compile_comprehension(
                        "<dictcomp>",
                        key,
                        Some(value),
                        generators,
                        ComprehensionKind::Dict,
                    )?;
                }
            }

            ExpressionKind::GeneratorExp { elt, generators } => {
                Self::validate_comprehension_yields(
                    ComprehensionKind::Generator,
                    elt,
                    None,
                    generators,
                    expr.location,
                )?;
                self.compile_comprehension(
                    "<genexpr>",
                    elt,
                    None,
                    generators,
                    ComprehensionKind::Generator,
                )?;
            }

            ExpressionKind::Yield { value } => {
                if !self.current_unit().is_function {
                    return Err(CompileError::YieldOutsideFunction {
                        location: expr.location,
                    });
                }
                if let Some(val) = value {
                    self.compile_expression(val)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                self.emit_op(Opcode::YieldValue);
            }

            ExpressionKind::YieldFrom { value } => {
                if !self.current_unit().is_function {
                    return Err(CompileError::YieldOutsideFunction {
                        location: expr.location,
                    });
                }
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

            ExpressionKind::Slice { lower, upper, step } => {
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
        if comparators.is_empty() {
            return Err(CompileError::InvalidAst {
                message: "no comparators".to_string(),
            });
        }
        if ops.len() != comparators.len() {
            return Err(CompileError::InvalidAst {
                message: "different number of comparators and operands".to_string(),
            });
        }

        self.compile_expression(left)?;

        if ops.len() == 1 {
            // Simple comparison: left op right
            self.compile_compare_operand(ops[0], &comparators[0])?;
            let cmp_arg = compare_op_arg(ops[0]);
            self.emit_arg(Opcode::CompareOp, cmp_arg);
        } else {
            // Chained: a < b < c → (a < b) and (b < c)
            let mut cleanup_labels = Vec::new();
            for (i, (op, comp)) in ops.iter().zip(comparators.iter()).enumerate() {
                self.compile_compare_operand(*op, comp)?;
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

    fn compile_compare_operand(&mut self, op: CompareOperator, operand: &Expression) -> Result<()> {
        if matches!(op, CompareOperator::In | CompareOperator::NotIn) {
            if let Some(value) = self.constant_set_literal_to_frozenset(operand) {
                let idx = self.add_const(value);
                self.emit_arg(Opcode::LoadConst, idx);
                return Ok(());
            }
        }
        self.compile_expression(operand)
    }

    fn constant_set_literal_to_frozenset(&self, expr: &Expression) -> Option<ConstantValue> {
        if let ExpressionKind::Set { elts } = &expr.node {
            let items = elts
                .iter()
                .map(|elt| self.try_fold_constant(elt))
                .collect::<Option<Vec<_>>>()?;
            Some(ConstantValue::FrozenSet(items))
        } else {
            None
        }
    }
}
