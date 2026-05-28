//! Simple symbol table for scope analysis.
//!
//! Walks the AST before compilation to determine which names are local,
//! global, nonlocal, free, or cell variables in each scope.

use indexmap::IndexMap;
use rustc_hash::FxHashSet;

/// The kind of scope a symbol table entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeType {
    Module,
    Function,
    Class,
    Comprehension,
}

/// How a symbol is resolved within its scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolScope {
    /// Bound in the local scope (assigned or parameter).
    Local,
    /// Explicitly declared `global`.
    Global,
    /// Explicitly declared `nonlocal`.
    Nonlocal,
    /// Captured from an enclosing scope (read-only from perspective of this scope).
    Free,
    /// Local variable that is also referenced by a nested scope.
    Cell,
}

/// Information about a single symbol in a scope.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub scope: SymbolScope,
    pub is_assigned: bool,
    pub is_referenced: bool,
    pub is_parameter: bool,
    /// True if `global x` or `nonlocal x` was explicitly declared.
    pub is_explicit_global_or_nonlocal: bool,
}

/// A single scope in the symbol table.
#[derive(Debug, Clone)]
pub struct Scope {
    pub name: String,
    pub scope_type: ScopeType,
    pub symbols: IndexMap<String, Symbol>,
    pub children: Vec<Scope>,
}

impl Scope {
    pub(super) fn new(name: impl Into<String>, scope_type: ScopeType) -> Self {
        Self {
            name: name.into(),
            scope_type,
            symbols: IndexMap::new(),
            children: Vec::new(),
        }
    }

    pub(super) fn add_symbol(&mut self, name: &str, scope: SymbolScope) {
        let entry = self.symbols.entry(name.to_string()).or_insert(Symbol {
            name: name.to_string(),
            scope,
            is_assigned: false,
            is_referenced: false,
            is_parameter: false,
            is_explicit_global_or_nonlocal: false,
        });
        // global/nonlocal declarations override
        if scope == SymbolScope::Global || scope == SymbolScope::Nonlocal {
            entry.scope = scope;
            entry.is_explicit_global_or_nonlocal = true;
        }
    }

    pub(super) fn mark_assigned(&mut self, name: &str) {
        let entry = self.symbols.entry(name.to_string()).or_insert(Symbol {
            name: name.to_string(),
            scope: SymbolScope::Local,
            is_assigned: false,
            is_referenced: false,
            is_parameter: false,
            is_explicit_global_or_nonlocal: false,
        });
        entry.is_assigned = true;
        // Assignment makes a name local unless explicitly declared global/nonlocal.
        if !entry.is_explicit_global_or_nonlocal {
            entry.scope = SymbolScope::Local;
        }
    }

    pub(super) fn mark_referenced(&mut self, name: &str) {
        let entry = self.symbols.entry(name.to_string()).or_insert(Symbol {
            name: name.to_string(),
            // Names only referenced (never assigned) resolve via LOAD_GLOBAL
            // which checks globals dict then builtins dict.
            scope: SymbolScope::Global,
            is_assigned: false,
            is_referenced: false,
            is_parameter: false,
            is_explicit_global_or_nonlocal: false,
        });
        entry.is_referenced = true;
    }

    pub(super) fn mark_parameter(&mut self, name: &str) {
        let entry = self.symbols.entry(name.to_string()).or_insert(Symbol {
            name: name.to_string(),
            scope: SymbolScope::Local,
            is_assigned: false,
            is_referenced: false,
            is_parameter: false,
            is_explicit_global_or_nonlocal: false,
        });
        entry.is_parameter = true;
        entry.is_assigned = true;
    }

    /// Look up how a name should be accessed in this scope.
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }

    /// Returns names that are local (including parameters, excluding global/nonlocal).
    pub fn local_names(&self) -> Vec<&str> {
        self.symbols
            .values()
            .filter(|s| s.scope == SymbolScope::Local || s.scope == SymbolScope::Cell)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// Returns names declared global.
    pub fn global_names(&self) -> FxHashSet<&str> {
        self.symbols
            .values()
            .filter(|s| s.scope == SymbolScope::Global)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// Returns names that are cell variables (local + captured by inner scope).
    pub fn cell_names(&self) -> Vec<&str> {
        self.symbols
            .values()
            .filter(|s| s.scope == SymbolScope::Cell)
            .map(|s| s.name.as_str())
            .collect()
    }

    /// Returns names that are free variables (captured from enclosing scope).
    pub fn free_names(&self) -> Vec<&str> {
        self.symbols
            .values()
            .filter(|s| s.scope == SymbolScope::Free)
            .map(|s| s.name.as_str())
            .collect()
    }
}

/// The complete symbol table for a module.
#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub top: Scope,
}
