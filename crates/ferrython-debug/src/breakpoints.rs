//! Breakpoint management for the Ferrython debugger.

use std::collections::HashMap;

/// What to do when a breakpoint is hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointAction {
    /// Pause execution and enter interactive debug mode (future).
    Break,
    /// Print the current source location and continue.
    Log,
    /// Evaluate and print an expression, then continue.
    Eval(String),
}

/// A single breakpoint.
#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: u32,
    pub filename: String,
    pub lineno: u32,
    pub function: Option<String>,
    pub condition: Option<String>,
    pub action: BreakpointAction,
    pub hit_count: u64,
    pub enabled: bool,
}

/// Manages all active breakpoints.
///
/// The VM calls `check_breakpoint` at each instruction. When profiling is
/// disabled the check is a simple bool test, so overhead is negligible.
pub struct BreakpointManager {
    breakpoints: HashMap<u32, Breakpoint>,
    next_id: u32,
    /// Quick-test flag: true if any breakpoint is enabled.
    any_enabled: bool,
    /// `breakpoint()` builtin was invoked — treated as a one-shot break.
    pub builtin_breakpoint_pending: bool,
}

impl BreakpointManager {
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            next_id: 1,
            any_enabled: false,
            builtin_breakpoint_pending: false,
        }
    }

    /// Add a breakpoint and return its ID.
    pub fn add(&mut self, filename: &str, lineno: u32, action: BreakpointAction) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.breakpoints.insert(id, Breakpoint {
            id,
            filename: filename.to_string(),
            lineno,
            function: None,
            condition: None,
            action,
            hit_count: 0,
            enabled: true,
        });
        self.any_enabled = true;
        id
    }

    /// Add a conditional breakpoint.
    pub fn add_conditional(
        &mut self, filename: &str, lineno: u32,
        condition: &str, action: BreakpointAction,
    ) -> u32 {
        let id = self.add(filename, lineno, action);
        if let Some(bp) = self.breakpoints.get_mut(&id) {
            bp.condition = Some(condition.to_string());
        }
        id
    }

    /// Remove a breakpoint by ID.
    pub fn remove(&mut self, id: u32) -> bool {
        let removed = self.breakpoints.remove(&id).is_some();
        self.update_any_enabled();
        removed
    }

    /// Enable or disable a breakpoint.
    pub fn set_enabled(&mut self, id: u32, enabled: bool) {
        if let Some(bp) = self.breakpoints.get_mut(&id) {
            bp.enabled = enabled;
        }
        self.update_any_enabled();
    }

    /// Fast check: are any breakpoints active?
    #[inline]
    pub fn has_active(&self) -> bool {
        self.any_enabled || self.builtin_breakpoint_pending
    }

    /// Check if a breakpoint matches the current execution location.
    /// Returns the action to take, or None if no breakpoint matches.
    pub fn check(&mut self, filename: &str, lineno: u32, function: &str) -> Option<BreakpointAction> {
        // Handle builtin breakpoint() call
        if self.builtin_breakpoint_pending {
            self.builtin_breakpoint_pending = false;
            return Some(BreakpointAction::Break);
        }

        if !self.any_enabled { return None; }

        for bp in self.breakpoints.values_mut() {
            if !bp.enabled { continue; }
            if bp.lineno != lineno { continue; }
            if bp.filename != filename { continue; }
            if let Some(ref func) = bp.function {
                if func != function { continue; }
            }
            bp.hit_count += 1;
            return Some(bp.action.clone());
        }
        None
    }

    /// List all breakpoints.
    pub fn list(&self) -> Vec<&Breakpoint> {
        let mut bps: Vec<_> = self.breakpoints.values().collect();
        bps.sort_by_key(|b| b.id);
        bps
    }

    /// Clear all breakpoints.
    pub fn clear(&mut self) {
        self.breakpoints.clear();
        self.any_enabled = false;
        self.builtin_breakpoint_pending = false;
    }

    fn update_any_enabled(&mut self) {
        self.any_enabled = self.breakpoints.values().any(|bp| bp.enabled);
    }
}

impl Default for BreakpointManager {
    fn default() -> Self { Self::new() }
}
