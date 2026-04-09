//! Simple symbol table for scope analysis.
//!
//! Walks the AST before compilation to determine which names are local,
//! global, nonlocal, free, or cell variables in each scope.

use ferrython_ast::*;
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
    fn new(name: impl Into<String>, scope_type: ScopeType) -> Self {
        Self {
            name: name.into(),
            scope_type,
            symbols: IndexMap::new(),
            children: Vec::new(),
        }
    }

    fn add_symbol(&mut self, name: &str, scope: SymbolScope) {
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

    fn mark_assigned(&mut self, name: &str) {
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

    fn mark_referenced(&mut self, name: &str) {
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

    fn mark_parameter(&mut self, name: &str) {
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

/// Analyze a module and produce a symbol table.
pub fn analyze(module: &Module) -> SymbolTable {
    let mut analyzer = Analyzer::new();
    analyzer.analyze_module(module);
    let mut top = analyzer.finish();
    // Post-analysis: resolve cell/free variables by walking scope tree
    resolve_free_vars(&mut top);
    SymbolTable { top }
}

/// Resolve cell/free variables by walking the scope tree bottom-up.
/// A variable is "free" in a child scope if it's referenced there but not local/global,
/// and it exists as local in some enclosing scope.
/// When a variable is free in a child, it becomes "cell" in the enclosing scope.
fn resolve_free_vars(scope: &mut Scope) {
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
    if scope.scope_type == ScopeType::Function || scope.scope_type == ScopeType::Comprehension
       || scope.scope_type == ScopeType::Class {
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
        } else if scope.scope_type == ScopeType::Function || scope.scope_type == ScopeType::Comprehension
                  || scope.scope_type == ScopeType::Class {
            // Not in our scope — add as Free so our parent provides it
            scope.symbols.insert(name.clone(), Symbol {
                name: name.clone(),
                scope: SymbolScope::Free,
                is_assigned: false,
                is_referenced: true,
                is_parameter: false,
                is_explicit_global_or_nonlocal: false,
            });
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

struct Analyzer {
    scope_stack: Vec<Scope>,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            scope_stack: Vec::new(),
        }
    }

    fn current_scope(&mut self) -> &mut Scope {
        self.scope_stack.last_mut().expect("no scope on stack")
    }

    /// True if the current scope is a function directly inside a class scope.
    fn is_inside_class_method(&self) -> bool {
        let len = self.scope_stack.len();
        if len < 2 { return false; }
        let current = &self.scope_stack[len - 1];
        if current.scope_type != ScopeType::Function { return false; }
        // Walk up: skip comprehension scopes, look for class
        for i in (0..len - 1).rev() {
            match self.scope_stack[i].scope_type {
                ScopeType::Class => return true,
                ScopeType::Comprehension => continue,
                _ => return false,
            }
        }
        false
    }

    fn push_scope(&mut self, name: impl Into<String>, scope_type: ScopeType) {
        self.scope_stack.push(Scope::new(name, scope_type));
    }

    fn pop_scope(&mut self) -> Scope {
        self.scope_stack.pop().expect("scope stack underflow")
    }

    fn finish(mut self) -> Scope {
        assert_eq!(self.scope_stack.len(), 1, "scope stack not balanced");
        self.scope_stack.pop().unwrap()
    }

    fn analyze_module(&mut self, module: &Module) {
        self.push_scope("<module>", ScopeType::Module);
        match module {
            Module::Module { body, .. } | Module::Interactive { body } => {
                for stmt in body {
                    self.analyze_statement(stmt);
                }
            }
            Module::Expression { body } => {
                self.analyze_expression(body);
            }
        }
    }

    fn analyze_statement(&mut self, stmt: &Statement) {
        match &stmt.node {
            StatementKind::FunctionDef {
                name,
                args,
                body,
                decorator_list,
                returns,
                is_async: _,
                ..
            } => {
                // The function name is assigned in the enclosing scope
                self.current_scope().mark_assigned(name);
                for dec in decorator_list {
                    self.analyze_expression(dec);
                }
                if let Some(ret) = returns {
                    self.analyze_expression(ret);
                }
                // Analyze default values in the enclosing scope
                for default in &args.defaults {
                    self.analyze_expression(default);
                }
                for default in args.kw_defaults.iter().flatten() {
                    self.analyze_expression(default);
                }
                // Analyze parameter annotations in the enclosing scope
                // (annotations are evaluated where the def statement appears)
                for arg in args.posonlyargs.iter()
                    .chain(args.args.iter())
                    .chain(args.vararg.iter())
                    .chain(args.kwonlyargs.iter())
                    .chain(args.kwarg.iter())
                {
                    if let Some(ref ann) = arg.annotation {
                        self.analyze_expression(ann);
                    }
                }
                // Push function scope
                self.push_scope(name.as_str(), ScopeType::Function);
                self.analyze_arguments(args);
                for s in body {
                    self.analyze_statement(s);
                }
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            StatementKind::ClassDef {
                name,
                bases,
                keywords,
                body,
                decorator_list,
            } => {
                self.current_scope().mark_assigned(name);
                for dec in decorator_list {
                    self.analyze_expression(dec);
                }
                for base in bases {
                    self.analyze_expression(base);
                }
                for kw in keywords {
                    self.analyze_expression(&kw.value);
                }
                self.push_scope(name.as_str(), ScopeType::Class);
                // Implicitly bind __class__ so methods can use super() without args
                self.current_scope().mark_assigned("__class__");
                for s in body {
                    self.analyze_statement(s);
                }
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            StatementKind::Assign { targets, value, .. } => {
                self.analyze_expression(value);
                for target in targets {
                    self.analyze_target(target);
                }
            }

            StatementKind::AugAssign { target, value, .. } => {
                self.analyze_expression(target);
                self.analyze_expression(value);
                self.analyze_target(target);
            }

            StatementKind::AnnAssign {
                target,
                annotation,
                value,
                ..
            } => {
                self.analyze_expression(annotation);
                if let Some(val) = value {
                    self.analyze_expression(val);
                }
                self.analyze_target(target);
            }

            StatementKind::Return { value } => {
                if let Some(val) = value {
                    self.analyze_expression(val);
                }
            }

            StatementKind::Delete { targets } => {
                for t in targets {
                    self.analyze_expression(t);
                }
            }

            StatementKind::For {
                target,
                iter,
                body,
                orelse,
                ..
            } => {
                self.analyze_expression(iter);
                self.analyze_target(target);
                for s in body {
                    self.analyze_statement(s);
                }
                for s in orelse {
                    self.analyze_statement(s);
                }
            }

            StatementKind::While {
                test,
                body,
                orelse,
            } => {
                self.analyze_expression(test);
                for s in body {
                    self.analyze_statement(s);
                }
                for s in orelse {
                    self.analyze_statement(s);
                }
            }

            StatementKind::If {
                test,
                body,
                orelse,
            } => {
                self.analyze_expression(test);
                for s in body {
                    self.analyze_statement(s);
                }
                for s in orelse {
                    self.analyze_statement(s);
                }
            }

            StatementKind::With { items, body, .. } => {
                for item in items {
                    self.analyze_expression(&item.context_expr);
                    if let Some(vars) = &item.optional_vars {
                        self.analyze_target(vars);
                    }
                }
                for s in body {
                    self.analyze_statement(s);
                }
            }

            StatementKind::Raise { exc, cause } => {
                if let Some(e) = exc {
                    self.analyze_expression(e);
                }
                if let Some(c) = cause {
                    self.analyze_expression(c);
                }
            }

            StatementKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                for s in body {
                    self.analyze_statement(s);
                }
                // Visit orelse BEFORE handlers to match compiler visitation order
                // (compiler emits: body → orelse → jump → handlers → finally)
                for s in orelse {
                    self.analyze_statement(s);
                }
                for handler in handlers {
                    if let Some(typ) = &handler.typ {
                        self.analyze_expression(typ);
                    }
                    if let Some(name) = &handler.name {
                        self.current_scope().mark_assigned(name);
                    }
                    for s in &handler.body {
                        self.analyze_statement(s);
                    }
                }
                for s in finalbody {
                    self.analyze_statement(s);
                }
            }

            StatementKind::Assert { test, msg } => {
                self.analyze_expression(test);
                if let Some(m) = msg {
                    self.analyze_expression(m);
                }
            }

            StatementKind::Import { names } => {
                for alias in names {
                    let store_name = alias
                        .asname
                        .as_deref()
                        .unwrap_or_else(|| {
                            // For `import a.b.c`, we store `a`
                            alias.name.split('.').next().unwrap_or(&alias.name)
                        });
                    self.current_scope().mark_assigned(store_name);
                }
            }

            StatementKind::ImportFrom { names, .. } => {
                for alias in names {
                    if alias.name.as_str() == "*" {
                        continue;
                    }
                    let store_name = alias.asname.as_deref().unwrap_or(&alias.name);
                    self.current_scope().mark_assigned(store_name);
                }
            }

            StatementKind::Global { names } => {
                for name in names {
                    self.current_scope()
                        .add_symbol(name, SymbolScope::Global);
                }
            }

            StatementKind::Nonlocal { names } => {
                for name in names {
                    self.current_scope()
                        .add_symbol(name, SymbolScope::Nonlocal);
                }
            }

            StatementKind::Expr { value } => {
                self.analyze_expression(value);
            }

            StatementKind::Pass | StatementKind::Break | StatementKind::Continue => {}

            StatementKind::Match { subject, cases } => {
                self.analyze_expression(subject);
                for case in cases {
                    self.analyze_pattern(&case.pattern);
                    if let Some(guard) = &case.guard {
                        self.analyze_expression(guard);
                    }
                    for stmt in &case.body {
                        self.analyze_statement(stmt);
                    }
                }
            }
        }
    }

    fn analyze_expression(&mut self, expr: &Expression) {
        match &expr.node {
            ExpressionKind::Name { id, .. } => {
                self.current_scope().mark_referenced(id);
                // PEP 3135: __class__ is an implicit cell variable in class bodies.
                // When a method (function inside class) references __class__ directly
                // or via super(), capture it from the enclosing class scope.
                if id.as_str() == "super" && self.is_inside_class_method() {
                    self.current_scope().mark_referenced("__class__");
                }
                if id.as_str() == "__class__" && self.is_inside_class_method() {
                    // Force scope to Free (override the default Global from mark_referenced)
                    if let Some(sym) = self.current_scope().symbols.get_mut("__class__") {
                        sym.scope = SymbolScope::Free;
                    }
                }
            }

            ExpressionKind::BoolOp { values, .. } => {
                for v in values {
                    self.analyze_expression(v);
                }
            }

            ExpressionKind::NamedExpr { target, value } => {
                self.analyze_expression(value);
                // PEP 572: In comprehensions, walrus target leaks to enclosing scope
                if self.current_scope().scope_type == ScopeType::Comprehension {
                    if let ExpressionKind::Name { id, .. } = &target.node {
                        // Mark target as Free in comprehension (will use STORE_DEREF)
                        self.current_scope().add_symbol(id, SymbolScope::Free);
                        // Mark as assigned in the enclosing non-comprehension scope
                        // (resolve_bottom_up will promote it to Cell)
                        let len = self.scope_stack.len();
                        for i in (0..len - 1).rev() {
                            if self.scope_stack[i].scope_type != ScopeType::Comprehension {
                                self.scope_stack[i].mark_assigned(id);
                                break;
                            }
                            // Intermediate comprehension scopes also need Free
                            self.scope_stack[i].add_symbol(id, SymbolScope::Free);
                        }
                    }
                } else {
                    self.analyze_target(target);
                }
            }

            ExpressionKind::BinOp { left, right, .. } => {
                self.analyze_expression(left);
                self.analyze_expression(right);
            }

            ExpressionKind::UnaryOp { operand, .. } => {
                self.analyze_expression(operand);
            }

            ExpressionKind::Lambda { args, body } => {
                for default in &args.defaults {
                    self.analyze_expression(default);
                }
                for default in args.kw_defaults.iter().flatten() {
                    self.analyze_expression(default);
                }
                self.push_scope("<lambda>", ScopeType::Function);
                self.analyze_arguments(args);
                self.analyze_expression(body);
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            ExpressionKind::IfExp {
                test,
                body,
                orelse,
            } => {
                self.analyze_expression(test);
                self.analyze_expression(body);
                self.analyze_expression(orelse);
            }

            ExpressionKind::Dict { keys, values } => {
                for k in keys.iter().flatten() {
                    self.analyze_expression(k);
                }
                for v in values {
                    self.analyze_expression(v);
                }
            }

            ExpressionKind::Set { elts }
            | ExpressionKind::List { elts, .. }
            | ExpressionKind::Tuple { elts, .. } => {
                for e in elts {
                    self.analyze_expression(e);
                }
            }

            ExpressionKind::ListComp { elt, generators }
            | ExpressionKind::SetComp { elt, generators } => {
                // First generator's iter is evaluated in enclosing scope (CPython semantics)
                if let Some(first) = generators.first() {
                    self.analyze_expression(&first.iter);
                }
                self.push_scope("<comprehension>", ScopeType::Comprehension);
                // First generator: only target + conditions (iter already analyzed above)
                if let Some(first) = generators.first() {
                    self.analyze_target(&first.target);
                    for cond in &first.ifs {
                        self.analyze_expression(cond);
                    }
                }
                // Remaining generators are fully inside comprehension scope
                for gen in generators.iter().skip(1) {
                    self.analyze_comprehension(gen);
                }
                self.analyze_expression(elt);
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            ExpressionKind::DictComp {
                key,
                value,
                generators,
            } => {
                if let Some(first) = generators.first() {
                    self.analyze_expression(&first.iter);
                }
                self.push_scope("<comprehension>", ScopeType::Comprehension);
                if let Some(first) = generators.first() {
                    self.analyze_target(&first.target);
                    for cond in &first.ifs {
                        self.analyze_expression(cond);
                    }
                }
                for gen in generators.iter().skip(1) {
                    self.analyze_comprehension(gen);
                }
                self.analyze_expression(key);
                self.analyze_expression(value);
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            ExpressionKind::GeneratorExp { elt, generators } => {
                if let Some(first) = generators.first() {
                    self.analyze_expression(&first.iter);
                }
                self.push_scope("<genexpr>", ScopeType::Comprehension);
                if let Some(first) = generators.first() {
                    self.analyze_target(&first.target);
                    for cond in &first.ifs {
                        self.analyze_expression(cond);
                    }
                }
                for gen in generators.iter().skip(1) {
                    self.analyze_comprehension(gen);
                }
                self.analyze_expression(elt);
                let child = self.pop_scope();
                self.current_scope().children.push(child);
            }

            ExpressionKind::Await { value } => {
                self.analyze_expression(value);
            }

            ExpressionKind::Yield { value } => {
                if let Some(v) = value {
                    self.analyze_expression(v);
                }
            }

            ExpressionKind::YieldFrom { value } => {
                self.analyze_expression(value);
            }

            ExpressionKind::Compare {
                left, comparators, ..
            } => {
                self.analyze_expression(left);
                for c in comparators {
                    self.analyze_expression(c);
                }
            }

            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                self.analyze_expression(func);
                for a in args {
                    self.analyze_expression(a);
                }
                for kw in keywords {
                    self.analyze_expression(&kw.value);
                }
            }

            ExpressionKind::FormattedValue {
                value, format_spec, ..
            } => {
                self.analyze_expression(value);
                if let Some(spec) = format_spec {
                    self.analyze_expression(spec);
                }
            }

            ExpressionKind::JoinedStr { values } => {
                for v in values {
                    self.analyze_expression(v);
                }
            }

            ExpressionKind::Constant { .. } => {}

            ExpressionKind::Attribute { value, .. } => {
                self.analyze_expression(value);
            }

            ExpressionKind::Subscript { value, slice, .. } => {
                self.analyze_expression(value);
                self.analyze_expression(slice);
            }

            ExpressionKind::Starred { value, .. } => {
                self.analyze_expression(value);
            }

            ExpressionKind::Slice {
                lower,
                upper,
                step,
            } => {
                if let Some(l) = lower {
                    self.analyze_expression(l);
                }
                if let Some(u) = upper {
                    self.analyze_expression(u);
                }
                if let Some(s) = step {
                    self.analyze_expression(s);
                }
            }
        }
    }

    fn analyze_target(&mut self, expr: &Expression) {
        match &expr.node {
            ExpressionKind::Name { id, .. } => {
                self.current_scope().mark_assigned(id);
            }
            ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
                for e in elts {
                    self.analyze_target(e);
                }
            }
            ExpressionKind::Starred { value, .. } => {
                self.analyze_target(value);
            }
            // Attribute and subscript targets don't introduce local bindings
            _ => {
                self.analyze_expression(expr);
            }
        }
    }

    fn analyze_arguments(&mut self, args: &Arguments) {
        for arg in &args.posonlyargs {
            self.current_scope().mark_parameter(&arg.arg);
        }
        for arg in &args.args {
            self.current_scope().mark_parameter(&arg.arg);
        }
        if let Some(ref vararg) = args.vararg {
            self.current_scope().mark_parameter(&vararg.arg);
        }
        for arg in &args.kwonlyargs {
            self.current_scope().mark_parameter(&arg.arg);
        }
        if let Some(ref kwarg) = args.kwarg {
            self.current_scope().mark_parameter(&kwarg.arg);
        }
    }

    fn analyze_comprehension(&mut self, comp: &Comprehension) {
        self.analyze_expression(&comp.iter);
        self.analyze_target(&comp.target);
        for cond in &comp.ifs {
            self.analyze_expression(cond);
        }
    }

    fn analyze_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::MatchWildcard | Pattern::MatchStar { name: None } => {}
            Pattern::MatchCapture { name } | Pattern::MatchStar { name: Some(name) } => {
                self.current_scope().mark_assigned(name);
            }
            Pattern::MatchLiteral { value } | Pattern::MatchValue { value } => {
                self.analyze_expression(value);
            }
            Pattern::MatchSequence { patterns } | Pattern::MatchOr { patterns } => {
                for p in patterns {
                    self.analyze_pattern(p);
                }
            }
            Pattern::MatchMapping { keys, patterns, rest } => {
                for k in keys {
                    self.analyze_expression(k);
                }
                for p in patterns {
                    self.analyze_pattern(p);
                }
                if let Some(rest_name) = rest {
                    self.current_scope().mark_assigned(rest_name);
                }
            }
            Pattern::MatchClass { cls, patterns, kwd_patterns, .. } => {
                self.analyze_expression(cls);
                for p in patterns {
                    self.analyze_pattern(p);
                }
                for p in kwd_patterns {
                    self.analyze_pattern(p);
                }
            }
            Pattern::MatchAs { pattern, name } => {
                if let Some(inner) = pattern {
                    self.analyze_pattern(inner);
                }
                if let Some(name) = name {
                    self.current_scope().mark_assigned(name);
                }
            }
        }
    }
}
