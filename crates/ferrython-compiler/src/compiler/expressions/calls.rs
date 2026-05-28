//! Function call expression compilation helpers.

use ferrython_ast::*;
use ferrython_bytecode::{ConstantValue, Opcode};

use super::super::{Compiler, Result};

impl Compiler {
    // ── function call ───────────────────────────────────────────────

    pub(super) fn compile_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        keywords: &[Keyword],
    ) -> Result<()> {
        // Check if any arg is starred or any keyword has None arg (** unpacking)
        let has_star_args = args
            .iter()
            .any(|a| matches!(a.node, ExpressionKind::Starred { .. }));
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
        } else if let ExpressionKind::Attribute {
            value,
            attr,
            ctx: ExprContext::Load,
        } = &func.node
        {
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
        let star_count = args
            .iter()
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

        if star_count == 1
            && !args
                .iter()
                .any(|a| !matches!(a.node, ExpressionKind::Starred { .. }))
        {
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
                let key_idx = self.add_const(ConstantValue::Str(kw.arg.as_ref().unwrap().clone()));
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
}
