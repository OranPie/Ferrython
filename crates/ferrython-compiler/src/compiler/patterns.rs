//! Match/case pattern compilation methods for the Compiler.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{ConstantValue, Opcode};

use super::{Compiler, Result};

impl Compiler {
    // ── match/case compilation (Python 3.10+) ───────────────────────
    //
    // Strategy: compile match/case into equivalent if-elif chain.
    // The subject is evaluated once and stored in a temp variable.
    // Each case pattern is compiled to a test expression.

    pub(super) fn compile_match(
        &mut self,
        subject: &Expression,
        cases: &[MatchCase],
    ) -> Result<()> {
        // Evaluate subject and store in a temp variable
        let temp = CompactString::from("$match_subject$");
        let temp_idx = self.varname_index(&temp);
        self.compile_expression(subject)?;
        self.emit_arg(Opcode::StoreFast, temp_idx);

        let mut end_labels = Vec::new();

        for case in cases {
            // Compile pattern test
            let is_wildcard = self.compile_pattern_test(&case.pattern, temp_idx)?;

            let skip_label = if !is_wildcard {
                Some(self.emit_jump(Opcode::PopJumpIfFalse))
            } else {
                None
            };

            // Bind pattern captures BEFORE guard (guard may reference captured names)
            self.compile_pattern_bindings(&case.pattern, temp_idx)?;

            // Compile guard if present
            let guard_label = if let Some(guard) = &case.guard {
                self.compile_expression(guard)?;
                Some(self.emit_jump(Opcode::PopJumpIfFalse))
            } else {
                None
            };

            // Compile body
            self.compile_body(&case.body)?;

            // Jump to end of match
            end_labels.push(self.emit_jump(Opcode::JumpForward));

            // Patch skip labels
            if let Some(label) = guard_label {
                self.patch_jump_here(label);
            }
            if let Some(label) = skip_label {
                self.patch_jump_here(label);
            }
        }

        // Patch all end labels
        for label in end_labels {
            self.patch_jump_here(label);
        }

        Ok(())
    }

    /// Compile a pattern test — pushes True/False on stack.
    /// Returns true if the pattern always matches (wildcard/capture).
    fn compile_pattern_test(&mut self, pattern: &Pattern, subject_idx: u32) -> Result<bool> {
        match pattern {
            Pattern::MatchWildcard => {
                // Always matches — push True
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
            Pattern::MatchCapture { .. } => {
                // Always matches — push True (binding happens later)
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
            Pattern::MatchLiteral { value } | Pattern::MatchValue { value } => {
                // subject == value
                self.emit_arg(Opcode::LoadFast, subject_idx);
                self.compile_expression(value)?;
                self.emit_arg(Opcode::CompareOp, 2); // == operator
                Ok(false)
            }
            Pattern::MatchSequence { patterns } => {
                self.compile_sequence_pattern_test(patterns, subject_idx)
            }
            Pattern::MatchMapping { keys, patterns, .. } => {
                self.compile_mapping_pattern_test(keys, patterns, subject_idx)
            }
            Pattern::MatchClass {
                cls,
                patterns,
                kwd_attrs,
                kwd_patterns,
            } => {
                self.compile_class_pattern_test(cls, patterns, kwd_attrs, kwd_patterns, subject_idx)
            }
            Pattern::MatchOr { patterns } => {
                // Any pattern can match
                let mut end_labels = Vec::new();
                for (i, pat) in patterns.iter().enumerate() {
                    let is_wc = self.compile_pattern_test(pat, subject_idx)?;
                    if is_wc {
                        // Always true — skip the rest
                        if i < patterns.len() - 1 {
                            // Pop remaining patterns
                        }
                        return Ok(true);
                    }
                    if i < patterns.len() - 1 {
                        // If this one matched, jump to end with True
                        let dup_label = self.emit_jump(Opcode::PopJumpIfTrue);
                        end_labels.push(dup_label);
                    }
                }
                // Last pattern result is on stack
                let done = self.emit_jump(Opcode::JumpForward);
                // Patch PopJumpIfTrue labels — they jump here with True already popped
                // Need to push True
                for label in &end_labels {
                    self.patch_jump_here(*label);
                }
                if !end_labels.is_empty() {
                    let idx = self.add_const(ConstantValue::Bool(true));
                    self.emit_arg(Opcode::LoadConst, idx);
                }
                self.patch_jump_here(done);
                Ok(false)
            }
            Pattern::MatchAs { pattern, .. } => {
                // Test the inner pattern (binding happens in compile_pattern_bindings)
                if let Some(inner) = pattern {
                    self.compile_pattern_test(inner, subject_idx)
                } else {
                    // `as name` with no inner pattern = wildcard
                    let idx = self.add_const(ConstantValue::Bool(true));
                    self.emit_arg(Opcode::LoadConst, idx);
                    Ok(true)
                }
            }
            Pattern::MatchStar { .. } => {
                // Star patterns only valid inside sequences — shouldn't hit here at top level
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
        }
    }

    fn compile_sequence_pattern_test(
        &mut self,
        patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, (list, tuple))
        // Build: isinstance(subject, (list, tuple))
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.load_name("list");
        self.load_name("tuple");
        self.emit_arg(Opcode::BuildTuple, 2);
        self.emit_arg(Opcode::CallFunction, 2);

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check len
        let has_star = patterns
            .iter()
            .any(|p| matches!(p, Pattern::MatchStar { .. }));
        let fixed_count = patterns
            .iter()
            .filter(|p| !matches!(p, Pattern::MatchStar { .. }))
            .count();

        self.load_name("len");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.emit_arg(Opcode::CallFunction, 1);

        let count_idx = self.add_const(ConstantValue::Integer(fixed_count as i64));
        self.emit_arg(Opcode::LoadConst, count_idx);
        if has_star {
            self.emit_arg(Opcode::CompareOp, 5); // >=
        } else {
            self.emit_arg(Opcode::CompareOp, 2); // ==
        }

        let len_fail = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check each element pattern, using negative indices for elements after the star
        let mut elem_fails = Vec::new();
        let star_pos = patterns
            .iter()
            .position(|p| matches!(p, Pattern::MatchStar { .. }));
        let post_star_count = star_pos.map_or(0, |sp| patterns.len() - sp - 1);
        let mut elem_idx = 0u32;
        let mut past_star = false;
        let mut after_star_i = 0usize;
        for pat in patterns {
            if matches!(pat, Pattern::MatchStar { .. }) {
                past_star = true;
                continue;
            }
            let elem_temp = CompactString::from(format!("$match_elem_{}$", elem_idx));
            let elem_temp_idx = self.varname_index(&elem_temp);
            self.emit_arg(Opcode::LoadFast, subject_idx);
            if past_star {
                // Use negative index: subject[-(post_star_count - after_star_i)]
                let neg_idx = -((post_star_count - after_star_i) as i64);
                let idx_const = self.add_const(ConstantValue::Integer(neg_idx));
                self.emit_arg(Opcode::LoadConst, idx_const);
                after_star_i += 1;
            } else {
                let idx_const = self.add_const(ConstantValue::Integer(elem_idx as i64));
                self.emit_arg(Opcode::LoadConst, idx_const);
            }
            self.emit_op(Opcode::BinarySubscr);
            self.emit_arg(Opcode::StoreFast, elem_temp_idx);

            let is_wc = self.compile_pattern_test(pat, elem_temp_idx)?;
            if !is_wc {
                elem_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
            elem_idx += 1;
        }

        // All passed — push True
        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        // Failure paths
        self.patch_jump_here(fail_label);
        self.patch_jump_here(len_fail);
        for label in elem_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    fn compile_mapping_pattern_test(
        &mut self,
        keys: &[Expression],
        patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, dict)
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.load_name("dict");
        self.emit_arg(Opcode::CallFunction, 2);

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check each key exists and value matches
        let mut kv_fails = Vec::new();
        for (i, (key, pat)) in keys.iter().zip(patterns.iter()).enumerate() {
            // Check key in subject
            self.compile_expression(key)?;
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.emit_arg(Opcode::CompareOp, 6); // in

            kv_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));

            // Get value, store temp, test pattern
            let val_temp = CompactString::from(format!("$match_val_{}$", i));
            let val_temp_idx = self.varname_index(&val_temp);
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.compile_expression(key)?;
            self.emit_op(Opcode::BinarySubscr);
            self.emit_arg(Opcode::StoreFast, val_temp_idx);

            let is_wc = self.compile_pattern_test(pat, val_temp_idx)?;
            if !is_wc {
                kv_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        self.patch_jump_here(fail_label);
        for label in kv_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    fn compile_class_pattern_test(
        &mut self,
        cls: &Expression,
        patterns: &[Pattern],
        kwd_attrs: &[CompactString],
        kwd_patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, cls)
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.compile_expression(cls)?;
        self.emit_arg(Opcode::CallFunction, 2);

        if patterns.is_empty() && kwd_attrs.is_empty() {
            return Ok(false);
        }

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        let mut attr_fails = Vec::new();

        // Positional patterns: for builtin types (int, str, float, bytes, bool),
        // the single positional arg captures the subject value itself.
        // For user classes, positional args map via __match_args__.
        for (i, pat) in patterns.iter().enumerate() {
            if matches!(pat, Pattern::MatchWildcard) {
                continue;
            }
            let pos_temp = CompactString::from(format!("$match_pos_{}$", i));
            let pos_temp_idx = self.varname_index(&pos_temp);
            // For single positional arg on builtin types, bind to subject directly
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.emit_arg(Opcode::StoreFast, pos_temp_idx);

            let is_wc = self.compile_pattern_test(pat, pos_temp_idx)?;
            if !is_wc {
                attr_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        // Keyword attribute patterns
        for (attr, pat) in kwd_attrs.iter().zip(kwd_patterns.iter()) {
            let attr_temp = CompactString::from(format!("$match_attr_{}$", attr));
            let attr_temp_idx = self.varname_index(&attr_temp);

            self.emit_arg(Opcode::LoadFast, subject_idx);
            let attr_name = self.add_name(attr);
            self.emit_arg(Opcode::LoadAttr, attr_name);
            self.emit_arg(Opcode::StoreFast, attr_temp_idx);

            let is_wc = self.compile_pattern_test(pat, attr_temp_idx)?;
            if !is_wc {
                attr_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        self.patch_jump_here(fail_label);
        for label in attr_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    /// Compile pattern bindings — store captured names after a successful match.
    fn compile_pattern_bindings(&mut self, pattern: &Pattern, subject_idx: u32) -> Result<()> {
        match pattern {
            Pattern::MatchCapture { name } => {
                self.emit_arg(Opcode::LoadFast, subject_idx);
                self.store_name(name);
            }
            Pattern::MatchAs { pattern, name } => {
                if let Some(inner) = pattern {
                    self.compile_pattern_bindings(inner, subject_idx)?;
                }
                if let Some(name) = name {
                    self.emit_arg(Opcode::LoadFast, subject_idx);
                    self.store_name(name);
                }
            }
            Pattern::MatchOr { patterns } => {
                // Bind from the first pattern that could match (simplified)
                if let Some(first) = patterns.first() {
                    self.compile_pattern_bindings(first, subject_idx)?;
                }
            }
            Pattern::MatchSequence { patterns } => {
                let star_pos = patterns
                    .iter()
                    .position(|p| matches!(p, Pattern::MatchStar { .. }));
                let post_star_count = star_pos.map_or(0, |sp| patterns.len() - sp - 1);
                let mut elem_idx = 0u32;
                let mut past_star = false;
                for pat in patterns {
                    if let Pattern::MatchStar { name } = pat {
                        // Bind star capture: rest = subject[pre_count : len(subject) - post_count]
                        if let Some(star_name) = name {
                            let pre_count = elem_idx as i64;
                            let post_count = post_star_count as i64;
                            // subject[pre_count:]  or  subject[pre_count:-post_count]
                            self.emit_arg(Opcode::LoadFast, subject_idx);
                            // Build slice(pre_count, -post_count or None)
                            let start_c = self.add_const(ConstantValue::Integer(pre_count));
                            self.emit_arg(Opcode::LoadConst, start_c);
                            if post_count > 0 {
                                let end_c = self.add_const(ConstantValue::Integer(-post_count));
                                self.emit_arg(Opcode::LoadConst, end_c);
                            } else {
                                let none_c = self.add_const(ConstantValue::None);
                                self.emit_arg(Opcode::LoadConst, none_c);
                            }
                            self.emit_arg(Opcode::BuildSlice, 2);
                            self.emit_op(Opcode::BinarySubscr);
                            // Convert to list (subject slice may be tuple)
                            self.load_name("list");
                            self.emit_op(Opcode::RotTwo);
                            self.emit_arg(Opcode::CallFunction, 1);
                            self.store_name(star_name);
                        }
                        past_star = true;
                        continue;
                    }
                    let elem_temp = CompactString::from(format!("$match_elem_{}$", elem_idx));
                    let elem_temp_idx = self.varname_index(&elem_temp);
                    self.compile_pattern_bindings(pat, elem_temp_idx)?;
                    elem_idx += 1;
                    let _ = past_star;
                }
            }
            Pattern::MatchMapping {
                keys: _,
                patterns,
                rest,
            } => {
                for (i, pat) in patterns.iter().enumerate() {
                    let val_temp = CompactString::from(format!("$match_val_{}$", i));
                    let val_temp_idx = self.varname_index(&val_temp);
                    self.compile_pattern_bindings(pat, val_temp_idx)?;
                }
                if let Some(rest_name) = rest {
                    // Bind remaining dict items — simplified: bind entire subject
                    self.emit_arg(Opcode::LoadFast, subject_idx);
                    self.store_name(rest_name);
                }
            }
            Pattern::MatchClass {
                kwd_attrs,
                kwd_patterns,
                patterns,
                ..
            } => {
                // Positional pattern bindings: bind subject to captured names
                for (i, pat) in patterns.iter().enumerate() {
                    let pos_temp = CompactString::from(format!("$match_pos_{}$", i));
                    let pos_temp_idx = self.varname_index(&pos_temp);
                    self.compile_pattern_bindings(pat, pos_temp_idx)?;
                }
                // Keyword attribute bindings
                for (attr, pat) in kwd_attrs.iter().zip(kwd_patterns.iter()) {
                    let attr_temp = CompactString::from(format!("$match_attr_{}$", attr));
                    let attr_temp_idx = self.varname_index(&attr_temp);
                    self.compile_pattern_bindings(pat, attr_temp_idx)?;
                }
            }
            Pattern::MatchWildcard
            | Pattern::MatchLiteral { .. }
            | Pattern::MatchValue { .. }
            | Pattern::MatchStar { .. } => {}
        }
        Ok(())
    }
}
