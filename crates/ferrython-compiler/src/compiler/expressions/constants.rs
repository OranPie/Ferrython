//! Constant folding and constant conversion helpers.

use ferrython_ast::*;
use ferrython_bytecode::{get_int_max_str_digits, ConstantValue};

use super::super::{Compiler, Result};
use super::comprehensions::{expr_contains_yield, ComprehensionKind};
use crate::error::CompileError;

impl Compiler {
    // ── constant folding ──────────────────────────────────────────────

    /// Try to evaluate a constant expression at compile time.
    /// Returns Some(value) if folded, None if not foldable.
    pub(in crate::compiler::expressions) fn try_fold_constant(
        &self,
        expr: &Expression,
    ) -> Option<ConstantValue> {
        match &expr.node {
            ExpressionKind::Constant { value } => Some(self.constant_to_value(value)),
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Load,
            } => {
                let items = elts
                    .iter()
                    .map(|elt| self.try_fold_constant(elt))
                    .collect::<Option<Vec<_>>>()?;
                Some(ConstantValue::Tuple(items))
            }
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
            ExpressionKind::UnaryOp {
                op: UnaryOperator::USub,
                operand,
            } => {
                if let ExpressionKind::Constant { value } = &operand.node {
                    match value {
                        Constant::Int(BigInt::Small(i)) => Some(Constant::Int(BigInt::Small(-i))),
                        Constant::Float(f) => Some(Constant::Float(-f)),
                        Constant::Complex { real, imag } => Some(Constant::Complex {
                            real: -*real,
                            imag: -*imag,
                        }),
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
            (UnaryOperator::USub, Constant::Float(f)) => Some(ConstantValue::Float(-f)),
            (UnaryOperator::USub, Constant::Complex { real, imag }) => {
                Some(ConstantValue::Complex {
                    real: -*real,
                    imag: -*imag,
                })
            }
            (UnaryOperator::UAdd, Constant::Int(BigInt::Small(i))) => {
                Some(ConstantValue::Integer(*i))
            }
            (UnaryOperator::UAdd, Constant::Float(f)) => Some(ConstantValue::Float(*f)),
            (UnaryOperator::UAdd, Constant::Complex { real, imag }) => {
                Some(ConstantValue::Complex {
                    real: *real,
                    imag: *imag,
                })
            }
            (UnaryOperator::Not, Constant::Bool(b)) => Some(ConstantValue::Bool(!b)),
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
            (Constant::Float(a), Constant::Float(b)) => Self::fold_float_binop(*a, op, *b),
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
                if b == 0 {
                    return None;
                }
                let (q, r) = (a / b, a % b);
                if (r != 0) && ((r ^ b) < 0) {
                    Some(q - 1)
                } else {
                    Some(q)
                }
            }?,
            Operator::Mod => {
                if b == 0 {
                    return None;
                }
                let r = a % b;
                if (r != 0) && ((r ^ b) < 0) {
                    Some(r + b)
                } else {
                    Some(r)
                }
            }?,
            Operator::Pow => {
                if b < 0 {
                    return None;
                } // negative power → float
                if b > 63 {
                    return None;
                } // prevent huge results
                a.checked_pow(b as u32)?
            }
            Operator::LShift => {
                if b < 0 {
                    return None;
                }
                if b <= 62 {
                    a.checked_mul(1_i64 << (b as u32))?
                } else if b == 63 {
                    match a {
                        0 => 0,
                        -1 => i64::MIN,
                        _ => return None,
                    }
                } else {
                    return None;
                }
            }
            Operator::RShift => {
                if b < 0 || b > 63 {
                    return None;
                }
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
                if b == 0.0 {
                    return None;
                }
                a / b
            }
            Operator::FloorDiv => {
                if b == 0.0 {
                    return None;
                }
                (a / b).floor()
            }
            Operator::Mod => {
                if b == 0.0 {
                    return None;
                }
                a % b
            }
            Operator::Pow => a.powf(b),
            _ => return None,
        };
        if result.is_nan() || result.is_infinite() {
            return None;
        }
        Some(ConstantValue::Float(result))
    }

    // ── constant conversion ─────────────────────────────────────────

    pub(in crate::compiler::expressions) fn validate_int_literal_limit(
        &self,
        expr: &Expression,
    ) -> Result<()> {
        let ExpressionKind::Constant {
            value: Constant::Int(value),
        } = &expr.node
        else {
            return Ok(());
        };
        let limit = get_int_max_str_digits();
        if limit <= 0 {
            return Ok(());
        }
        let digits = match value {
            BigInt::Small(i) => i.to_string().trim_start_matches('-').len(),
            BigInt::Big(i) => i.to_str_radix(10).trim_start_matches('-').len(),
        };
        if digits as i64 > limit {
            return Err(CompileError::syntax(
                format!(
                    "Exceeds the limit ({} digits) for integer string conversion: value has {} digits; Consider hexadecimal for huge integer literals",
                    limit, digits
                ),
                expr.location,
            ));
        }
        Ok(())
    }

    pub(in crate::compiler::expressions) fn validate_comprehension_yields(
        kind: ComprehensionKind,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        location: SourceLocation,
    ) -> Result<()> {
        let has_yield = expr_contains_yield(elt)
            || value.map_or(false, expr_contains_yield)
            || generators.iter().enumerate().any(|(idx, gen)| {
                (idx > 0 && expr_contains_yield(&gen.iter))
                    || gen.ifs.iter().any(expr_contains_yield)
            });

        if has_yield {
            return Err(CompileError::syntax(
                Self::comprehension_yield_message(kind),
                location,
            ));
        }
        Ok(())
    }

    fn comprehension_yield_message(kind: ComprehensionKind) -> &'static str {
        match kind {
            ComprehensionKind::List => "'yield' inside list comprehension",
            ComprehensionKind::Set => "'yield' inside set comprehension",
            ComprehensionKind::Dict => "'yield' inside dict comprehension",
            ComprehensionKind::Generator => "'yield' inside generator expression",
        }
    }

    pub(super) fn constant_to_value(&self, constant: &Constant) -> ConstantValue {
        match constant {
            Constant::None => ConstantValue::None,
            Constant::Bool(b) => ConstantValue::Bool(*b),
            Constant::Int(BigInt::Small(i)) => ConstantValue::Integer(*i),
            Constant::Int(BigInt::Big(b)) => ConstantValue::BigInteger(b.clone()),
            Constant::Float(f) => ConstantValue::Float(*f),
            Constant::Complex { real, imag } => ConstantValue::Complex {
                real: *real,
                imag: *imag,
            },
            Constant::Str(s) => ConstantValue::Str(s.clone()),
            Constant::Bytes(b) => ConstantValue::Bytes(b.clone()),
            Constant::Ellipsis => ConstantValue::Ellipsis,
            Constant::Tuple(items) => ConstantValue::Tuple(
                items
                    .iter()
                    .map(|item| self.constant_to_value(item))
                    .collect(),
            ),
            Constant::FrozenSet(items) => ConstantValue::FrozenSet(
                items
                    .iter()
                    .map(|item| self.constant_to_value(item))
                    .collect(),
            ),
        }
    }
}
