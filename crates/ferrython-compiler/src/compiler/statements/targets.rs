//! Augmented assignment, store target, and delete target compilation.

use ferrython_ast::*;
use ferrython_bytecode::Opcode;

use super::super::{Compiler, Result};
use crate::error::CompileError;

impl Compiler {
    // ── augmented assignment ────────────────────────────────────────

    pub(super) fn compile_aug_assign(
        &mut self,
        target: &Expression,
        op: Operator,
        value: &Expression,
    ) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                if id.as_str() == "__debug__" {
                    return Err(CompileError::syntax(
                        "cannot assign to __debug__",
                        target.location,
                    ));
                }
                self.load_name(id);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.store_name(id);
            }
            ExpressionKind::Attribute {
                value: obj, attr, ..
            } => {
                self.compile_expression(obj)?;
                self.emit_op(Opcode::DupTop);
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
                self.emit_arg(Opcode::LoadAttr, attr_idx);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.emit_op(Opcode::RotTwo);
                self.emit_arg(Opcode::StoreAttr, attr_idx);
            }
            ExpressionKind::Subscript {
                value: obj, slice, ..
            } => {
                self.compile_expression(obj)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::DupTopTwo);
                self.emit_op(Opcode::BinarySubscr);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.emit_op(Opcode::RotThree);
                self.emit_op(Opcode::StoreSubscr);
            }
            _ => {
                return Err(CompileError::InvalidAssignTarget {
                    location: target.outer_location,
                });
            }
        }
        Ok(())
    }

    pub(super) fn emit_inplace_op(&mut self, op: Operator) {
        let opcode = match op {
            Operator::Add => Opcode::InplaceAdd,
            Operator::Sub => Opcode::InplaceSubtract,
            Operator::Mult => Opcode::InplaceMultiply,
            Operator::MatMult => Opcode::InplaceMatrixMultiply,
            Operator::Div => Opcode::InplaceTrueDivide,
            Operator::FloorDiv => Opcode::InplaceFloorDivide,
            Operator::Mod => Opcode::InplaceModulo,
            Operator::Pow => Opcode::InplacePower,
            Operator::LShift => Opcode::InplaceLshift,
            Operator::RShift => Opcode::InplaceRshift,
            Operator::BitOr => Opcode::InplaceOr,
            Operator::BitXor => Opcode::InplaceXor,
            Operator::BitAnd => Opcode::InplaceAnd,
        };
        self.emit_op(opcode);
    }

    // ── store / delete targets ──────────────────────────────────────

    pub(in crate::compiler) fn compile_store_target(&mut self, target: &Expression) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                if id.as_str() == "__debug__" {
                    return Err(CompileError::syntax(
                        "cannot assign to __debug__",
                        target.location,
                    ));
                }
                self.store_name(id);
            }
            ExpressionKind::Attribute { value, attr, .. } => {
                self.compile_expression(value)?;
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
                self.emit_arg(Opcode::StoreAttr, attr_idx);
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                self.compile_expression(value)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::StoreSubscr);
            }
            ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
                // Check for starred element
                let star_count = elts
                    .iter()
                    .filter(|e| matches!(e.node, ExpressionKind::Starred { .. }))
                    .count();

                if star_count > 1 {
                    return Err(CompileError::syntax(
                        "multiple starred expressions in assignment",
                        target.location,
                    ));
                }

                let star_idx = elts
                    .iter()
                    .position(|e| matches!(e.node, ExpressionKind::Starred { .. }));

                if let Some(star) = star_idx {
                    let before = star as u32;
                    let after = (elts.len() - star - 1) as u32;
                    self.emit_arg(Opcode::UnpackEx, before | (after << 8));
                } else {
                    self.emit_arg(Opcode::UnpackSequence, elts.len() as u32);
                }

                for elt in elts {
                    if let ExpressionKind::Starred { value, .. } = &elt.node {
                        self.compile_store_target(value)?;
                    } else {
                        self.compile_store_target(elt)?;
                    }
                }
            }
            ExpressionKind::Starred { value, .. } => {
                self.compile_store_target(value)?;
            }
            _ => {
                return Err(CompileError::InvalidAssignTarget {
                    location: target.outer_location,
                });
            }
        }
        Ok(())
    }

    pub(super) fn compile_delete_target(&mut self, target: &Expression) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                self.delete_name(id);
            }
            ExpressionKind::Attribute { value, attr, .. } => {
                self.compile_expression(value)?;
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
                self.emit_arg(Opcode::DeleteAttr, attr_idx);
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                self.compile_expression(value)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::DeleteSubscr);
            }
            ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
                for elt in elts {
                    self.compile_delete_target(elt)?;
                }
            }
            ExpressionKind::Call { .. } => {
                return Err(CompileError::CannotDeleteCall {
                    location: target.location,
                });
            }
            ExpressionKind::Constant { .. } => {
                return Err(CompileError::CannotDeleteLiteral {
                    location: target.location,
                });
            }
            _ => {
                return Err(CompileError::CannotDeleteExpression {
                    location: target.location,
                });
            }
        }
        Ok(())
    }
}
