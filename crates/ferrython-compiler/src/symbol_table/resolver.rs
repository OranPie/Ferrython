use super::{Scope, ScopeType, Symbol, SymbolScope};
use rustc_hash::FxHashSet;

/// Resolve cell/free variables by walking the scope tree bottom-up.
/// A variable is "free" in a child scope if it's referenced there but not local/global,
/// and it exists as local in some enclosing scope.
/// When a variable is free in a child, it becomes "cell" in the enclosing scope.
pub(super) fn resolve_free_vars(scope: &mut Scope) {
    // We use a two-pass approach:
    // Pass 1 (top-down): Propagate available closure names downward and mark
    //   implicit globals as Free when an enclosing function scope has them.
    // Pass 2 (bottom-up): Mark parent variables as Cell when children capture them.

    // Collect names available from enclosing function scopes
    let available: FxHashSet<String> = FxHashSet::default();
    resolve_top_down(scope, &available);

    // Now bottom-up: mark Cell vars and propagate Nonlocal → Free
    resolve_bottom_up(scope);
}

/// Top-down pass: propagate available closure names from enclosing scopes.
/// `available` = names defined in enclosing function scopes that can be captured.
fn resolve_top_down(scope: &mut Scope, available: &FxHashSet<String>) {
    // For each implicit-Global symbol in this scope, check if it's available
    // from an enclosing function scope
    if scope.scope_type == ScopeType::Function
        || scope.scope_type == ScopeType::Comprehension
        || scope.scope_type == ScopeType::Class
    {
        for (_name, sym) in &mut scope.symbols {
            if sym.scope == SymbolScope::Global && !sym.is_explicit_global_or_nonlocal {
                if available.contains(&sym.name) {
                    sym.scope = SymbolScope::Free;
                }
            }
        }
    }

    // Build the set of names available to our children
    let mut child_available = available.clone();
    if scope.scope_type == ScopeType::Function || scope.scope_type == ScopeType::Comprehension {
        for (name, sym) in &scope.symbols {
            if sym.scope == SymbolScope::Local || sym.scope == SymbolScope::Free {
                child_available.insert(name.clone());
            }
        }
    } else if scope.scope_type == ScopeType::Class {
        // Class scopes pass through free variables from enclosing scopes
        for (name, sym) in &scope.symbols {
            if sym.scope == SymbolScope::Free {
                child_available.insert(name.clone());
            }
        }
    }

    // Recurse into children
    for child in &mut scope.children {
        resolve_top_down(child, &child_available);
    }
}

/// Bottom-up pass: mark Cell vars where children capture, propagate Free upward.
fn resolve_bottom_up(scope: &mut Scope) {
    // Process children first
    for child in &mut scope.children {
        resolve_bottom_up(child);
    }

    // Collect names that children need as Free or Nonlocal
    let mut names_needed: FxHashSet<String> = FxHashSet::default();
    for child in &scope.children {
        for (name, sym) in &child.symbols {
            if sym.scope == SymbolScope::Free || sym.scope == SymbolScope::Nonlocal {
                names_needed.insert(name.clone());
            }
        }
    }

    for name in &names_needed {
        if let Some(sym) = scope.symbols.get_mut(name.as_str()) {
            if sym.scope == SymbolScope::Local {
                sym.scope = SymbolScope::Cell;
            }
        } else if scope.scope_type == ScopeType::Function
            || scope.scope_type == ScopeType::Comprehension
            || scope.scope_type == ScopeType::Class
        {
            // Not in our scope — add as Free so our parent provides it
            scope.symbols.insert(
                name.clone(),
                Symbol {
                    name: name.clone(),
                    scope: SymbolScope::Free,
                    is_assigned: false,
                    is_referenced: true,
                    is_parameter: false,
                    is_explicit_global_or_nonlocal: false,
                },
            );
        }
    }

    // Nonlocal → Free for runtime access
    for child in &mut scope.children {
        for (_name, sym) in &mut child.symbols {
            if sym.scope == SymbolScope::Nonlocal {
                sym.scope = SymbolScope::Free;
            }
        }
    }
}
