use ferrython_bytecode::code::{CodeObject, ConstantValue};
use ferrython_bytecode::Opcode;

/// Analyze a __init__ function's bytecode to check if it is trivially inlinable.
/// Returns `Some(slots)` where each slot is `(arg_local_index, name_index)` for
/// a LOAD_FAST+LOAD_FAST(self)+STORE_ATTR triple. Returns `None` if the body
/// contains anything beyond simple `self.attr = arg` assignments + `return None`.
pub(super) fn analyze_trivial_init(code: &CodeObject) -> Option<Vec<(usize, usize)>> {
    let instrs = &code.instructions;
    let len = instrs.len();
    if len < 2 {
        return None;
    }

    let mut i = 0;
    let mut slots = Vec::new();

    while i + 3 <= len {
        if instrs[i].op == Opcode::LoadFast
            && instrs[i + 1].op == Opcode::LoadFast
            && instrs[i + 1].arg == 0
            && instrs[i + 2].op == Opcode::StoreAttr
        {
            let arg_idx = instrs[i].arg as usize;
            if arg_idx == 0 {
                return None;
            }
            slots.push((arg_idx, instrs[i + 2].arg as usize));
            i += 3;
        } else {
            break;
        }
    }

    if slots.is_empty() || i + 2 != len {
        return None;
    }
    if instrs[i].op != Opcode::LoadConst || instrs[i + 1].op != Opcode::ReturnValue {
        return None;
    }
    let const_idx = instrs[i].arg as usize;
    if const_idx < code.constants.len() && !matches!(code.constants[const_idx], ConstantValue::None)
    {
        return None;
    }
    Some(slots)
}
