use std::cell::Cell;
use std::rc::Rc;

pub(super) const FRAMELESS_CALL_RECURSION_LIMIT: usize = 128;

pub(super) struct CallObjectDepthGuard {
    pub(super) depth: Rc<Cell<usize>>,
}

impl Drop for CallObjectDepthGuard {
    fn drop(&mut self) {
        self.depth.set(self.depth.get().saturating_sub(1));
    }
}
