//! Fast name load/store helpers for module-scope bytecode paths.

use crate::frame::{bump_globals_version, globals_version, Frame, ScopeKind};

pub(crate) enum FastNameResult {
    Handled,
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_load_name(frame: &mut Frame, idx: usize) -> FastNameResult {
    let ver = globals_version();
    if frame.exec_locals.is_none() && frame.global_cache_version == ver {
        if let Some(ref cache) = frame.global_cache {
            if let Some(ref value) = unsafe { cache.get_unchecked(idx) } {
                unsafe { frame.push_unchecked(value.clone()) };
                return FastNameResult::Handled;
            }
        }
    }
    FastNameResult::Fallback
}

#[inline(always)]
pub(crate) fn try_fast_store_name(frame: &mut Frame, idx: usize) -> FastNameResult {
    if frame.scope_kind != ScopeKind::Module || frame.exec_locals.is_some() {
        return FastNameResult::Fallback;
    }

    let value = frame.stack.pop().expect("stack underflow");
    if frame.global_cache.is_some() {
        let cur_ver = globals_version();
        let cache = std::rc::Rc::make_mut(frame.global_cache.as_mut().unwrap());
        if frame.global_cache_version != cur_ver {
            for slot in cache.iter_mut() {
                *slot = None;
            }
        }
        if idx < cache.len() {
            cache[idx] = Some(value.clone());
        }
    }

    let name_ref = &frame.code.names[idx];
    let mut globals = frame.globals.write();
    if let Some(slot) = globals.get_mut(name_ref) {
        *slot = value;
    } else {
        globals.insert(name_ref.clone(), value);
    }
    drop(globals);
    bump_globals_version();
    frame.global_cache_version = globals_version();
    FastNameResult::Handled
}
