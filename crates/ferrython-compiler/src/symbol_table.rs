//! Simple symbol table for scope analysis.
//!
//! Walks the AST before compilation to determine which names are local,
//! global, nonlocal, free, or cell variables in each scope.

use crate::error::CompileError;
use ferrython_ast::*;
use rustc_hash::FxHashSet;

mod model;
mod resolver;

pub use model::{Scope, ScopeType, Symbol, SymbolScope, SymbolTable};
use resolver::resolve_free_vars;

/// Analyze a module and produce a symbol table.
pub fn analyze(module: &Module) -> Result<SymbolTable, CompileError> {
    let mut analyzer = Analyzer::new();
    analyzer.analyze_module(module);
    if !analyzer.errors.is_empty() {
        return Err(analyzer.errors.remove(0));
    }
    let mut top = analyzer.finish();
    // Post-analysis: resolve cell/free variables by walking scope tree
    resolve_free_vars(&mut top);
    Ok(SymbolTable { top })
}

struct Analyzer {
    scope_stack: Vec<Scope>,
    errors: Vec<CompileError>,
}

fn target_name(expr: &Expression) -> Option<&str> {
    match &expr.node {
        ExpressionKind::Name { id, .. } => Some(id.as_str()),
        _ => None,
    }
}

fn collect_target_names(expr: &Expression, names: &mut FxHashSet<String>) {
    match &expr.node {
        ExpressionKind::Name { id, .. } => {
            names.insert(id.to_string());
        }
        ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
            for elt in elts {
                collect_target_names(elt, names);
            }
        }
        ExpressionKind::Starred { value, .. } => collect_target_names(value, names),
        _ => {}
    }
}

fn collect_named_expr_targets(expr: &Expression, names: &mut FxHashSet<String>) {
    match &expr.node {
        ExpressionKind::NamedExpr { target, value } => {
            if let Some(name) = target_name(target) {
                names.insert(name.to_string());
            }
            collect_named_expr_targets(value, names);
        }
        ExpressionKind::BoolOp { values, .. } => {
            for value in values {
                collect_named_expr_targets(value, names);
            }
        }
        ExpressionKind::BinOp { left, right, .. } => {
            collect_named_expr_targets(left, names);
            collect_named_expr_targets(right, names);
        }
        ExpressionKind::UnaryOp { operand, .. } => collect_named_expr_targets(operand, names),
        ExpressionKind::Lambda { .. } => {}
        ExpressionKind::IfExp { test, body, orelse } => {
            collect_named_expr_targets(test, names);
            collect_named_expr_targets(body, names);
            collect_named_expr_targets(orelse, names);
        }
        ExpressionKind::Dict { keys, values } => {
            for key in keys.iter().flatten() {
                collect_named_expr_targets(key, names);
            }
            for value in values {
                collect_named_expr_targets(value, names);
            }
        }
        ExpressionKind::Set { elts }
        | ExpressionKind::List { elts, .. }
        | ExpressionKind::Tuple { elts, .. } => {
            for elt in elts {
                collect_named_expr_targets(elt, names);
            }
        }
        ExpressionKind::ListComp { elt, generators }
        | ExpressionKind::SetComp { elt, generators }
        | ExpressionKind::GeneratorExp { elt, generators } => {
            collect_named_expr_targets(elt, names);
            for gen in generators {
                collect_named_expr_targets(&gen.iter, names);
                for cond in &gen.ifs {
                    collect_named_expr_targets(cond, names);
                }
            }
        }
        ExpressionKind::DictComp {
            key,
            value,
            generators,
        } => {
            collect_named_expr_targets(key, names);
            collect_named_expr_targets(value, names);
            for gen in generators {
                collect_named_expr_targets(&gen.iter, names);
                for cond in &gen.ifs {
                    collect_named_expr_targets(cond, names);
                }
            }
        }
        ExpressionKind::Await { value }
        | ExpressionKind::YieldFrom { value }
        | ExpressionKind::Starred { value, .. } => collect_named_expr_targets(value, names),
        ExpressionKind::Yield { value } => {
            if let Some(value) = value {
                collect_named_expr_targets(value, names);
            }
        }
        ExpressionKind::Compare {
            left, comparators, ..
        } => {
            collect_named_expr_targets(left, names);
            for comparator in comparators {
                collect_named_expr_targets(comparator, names);
            }
        }
        ExpressionKind::Call {
            func,
            args,
            keywords,
        } => {
            collect_named_expr_targets(func, names);
            for arg in args {
                collect_named_expr_targets(arg, names);
            }
            for keyword in keywords {
                collect_named_expr_targets(&keyword.value, names);
            }
        }
        ExpressionKind::FormattedValue {
            value, format_spec, ..
        } => {
            collect_named_expr_targets(value, names);
            if let Some(format_spec) = format_spec {
                collect_named_expr_targets(format_spec, names);
            }
        }
        ExpressionKind::JoinedStr { values } => {
            for value in values {
                collect_named_expr_targets(value, names);
            }
        }
        ExpressionKind::Attribute { value, .. } | ExpressionKind::Subscript { value, .. } => {
            collect_named_expr_targets(value, names)
        }
        ExpressionKind::Slice { lower, upper, step } => {
            if let Some(expr) = lower {
                collect_named_expr_targets(expr, names);
            }
            if let Some(expr) = upper {
                collect_named_expr_targets(expr, names);
            }
            if let Some(expr) = step {
                collect_named_expr_targets(expr, names);
            }
        }
        ExpressionKind::Constant { .. } | ExpressionKind::Name { .. } => {}
    }
}

fn expr_has_named_expr(expr: &Expression) -> bool {
    let mut names = FxHashSet::default();
    collect_named_expr_targets(expr, &mut names);
    !names.is_empty()
}

fn expr_contains_named_expr(expr: &Expression) -> bool {
    match &expr.node {
        ExpressionKind::NamedExpr { .. } => true,
        ExpressionKind::BoolOp { values, .. } => values.iter().any(expr_contains_named_expr),
        ExpressionKind::BinOp { left, right, .. } => {
            expr_contains_named_expr(left) || expr_contains_named_expr(right)
        }
        ExpressionKind::UnaryOp { operand, .. } => expr_contains_named_expr(operand),
        ExpressionKind::Lambda { args, body } => {
            arguments_contain_named_expr(args) || expr_contains_named_expr(body)
        }
        ExpressionKind::IfExp { test, body, orelse } => {
            expr_contains_named_expr(test)
                || expr_contains_named_expr(body)
                || expr_contains_named_expr(orelse)
        }
        ExpressionKind::Dict { keys, values } => {
            keys.iter().flatten().any(expr_contains_named_expr)
                || values.iter().any(expr_contains_named_expr)
        }
        ExpressionKind::Set { elts }
        | ExpressionKind::List { elts, .. }
        | ExpressionKind::Tuple { elts, .. } => elts.iter().any(expr_contains_named_expr),
        ExpressionKind::ListComp { elt, generators }
        | ExpressionKind::SetComp { elt, generators }
        | ExpressionKind::GeneratorExp { elt, generators } => {
            expr_contains_named_expr(elt) || comprehensions_contain_named_expr(generators)
        }
        ExpressionKind::DictComp {
            key,
            value,
            generators,
        } => {
            expr_contains_named_expr(key)
                || expr_contains_named_expr(value)
                || comprehensions_contain_named_expr(generators)
        }
        ExpressionKind::Await { value }
        | ExpressionKind::YieldFrom { value }
        | ExpressionKind::Starred { value, .. }
        | ExpressionKind::Attribute { value, .. } => expr_contains_named_expr(value),
        ExpressionKind::Yield { value } => value.as_deref().is_some_and(expr_contains_named_expr),
        ExpressionKind::Compare {
            left, comparators, ..
        } => expr_contains_named_expr(left) || comparators.iter().any(expr_contains_named_expr),
        ExpressionKind::Call {
            func,
            args,
            keywords,
        } => {
            expr_contains_named_expr(func)
                || args.iter().any(expr_contains_named_expr)
                || keywords
                    .iter()
                    .any(|keyword| expr_contains_named_expr(&keyword.value))
        }
        ExpressionKind::FormattedValue {
            value, format_spec, ..
        } => {
            expr_contains_named_expr(value)
                || format_spec.as_deref().is_some_and(expr_contains_named_expr)
        }
        ExpressionKind::JoinedStr { values } => values.iter().any(expr_contains_named_expr),
        ExpressionKind::Subscript { value, slice, .. } => {
            expr_contains_named_expr(value) || expr_contains_named_expr(slice)
        }
        ExpressionKind::Slice { lower, upper, step } => {
            lower.as_deref().is_some_and(expr_contains_named_expr)
                || upper.as_deref().is_some_and(expr_contains_named_expr)
                || step.as_deref().is_some_and(expr_contains_named_expr)
        }
        ExpressionKind::Constant { .. } | ExpressionKind::Name { .. } => false,
    }
}

fn arguments_contain_named_expr(args: &Arguments) -> bool {
    args.defaults.iter().any(expr_contains_named_expr)
        || args
            .kw_defaults
            .iter()
            .flatten()
            .any(expr_contains_named_expr)
        || args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.vararg.iter())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.iter())
            .any(|arg| {
                arg.annotation
                    .as_deref()
                    .is_some_and(expr_contains_named_expr)
            })
}

fn comprehensions_contain_named_expr(generators: &[Comprehension]) -> bool {
    generators.iter().any(|gen| {
        expr_contains_named_expr(&gen.iter) || gen.ifs.iter().any(expr_contains_named_expr)
    })
}

impl Analyzer {
    fn new() -> Self {
        Self {
            scope_stack: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn current_scope(&mut self) -> &mut Scope {
        self.scope_stack.last_mut().expect("no scope on stack")
    }

    /// True if the current scope is a function directly inside a class scope.
    fn is_inside_class_method(&self) -> bool {
        let len = self.scope_stack.len();
        if len < 2 {
            return false;
        }
        let current = &self.scope_stack[len - 1];
        if current.scope_type != ScopeType::Function {
            return false;
        }
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

    fn syntax_error(&mut self, message: impl Into<String>, location: SourceLocation) {
        self.errors.push(CompileError::syntax(message, location));
    }

    fn validate_named_expr_target(&mut self, expr: &Expression) {
        if let ExpressionKind::NamedExpr { target, .. } = &expr.node {
            if !matches!(target.node, ExpressionKind::Name { .. }) {
                let kind = match &target.node {
                    ExpressionKind::Tuple { .. } => "tuple",
                    ExpressionKind::List { .. } => "list",
                    _ => "expression",
                };
                self.syntax_error(
                    format!("cannot use assignment expressions with {}", kind),
                    target.location,
                );
            }
        }
    }

    fn validate_comprehension_named_exprs(
        &mut self,
        expr: &Expression,
        generators: &[Comprehension],
    ) {
        for gen in generators {
            if expr_contains_named_expr(&gen.iter) {
                self.syntax_error(
                    "assignment expression cannot be used in a comprehension iterable expression",
                    gen.iter.location,
                );
            }
        }

        let mut prior_loop_targets = FxHashSet::default();
        let mut prior_named_targets = FxHashSet::default();
        for gen in generators {
            let mut cond_named_targets = FxHashSet::default();
            for cond in &gen.ifs {
                collect_named_expr_targets(cond, &mut cond_named_targets);
            }

            let mut targets = FxHashSet::default();
            collect_target_names(&gen.target, &mut targets);
            for name in &targets {
                if prior_named_targets.contains(name) {
                    self.syntax_error(
                        format!(
                            "comprehension inner loop cannot rebind assignment expression target '{}'",
                            name
                        ),
                        gen.target.location,
                    );
                }
            }
            for name in &cond_named_targets {
                if prior_loop_targets.contains(name) || targets.contains(name) {
                    self.syntax_error(
                        format!(
                            "assignment expression cannot rebind comprehension iteration variable '{}'",
                            name
                        ),
                        expr.location,
                    );
                }
            }

            prior_loop_targets.extend(targets);
            prior_named_targets.extend(cond_named_targets);
        }

        let mut elt_named_targets = FxHashSet::default();
        match &expr.node {
            ExpressionKind::ListComp { elt, .. }
            | ExpressionKind::SetComp { elt, .. }
            | ExpressionKind::GeneratorExp { elt, .. } => {
                collect_named_expr_targets(elt, &mut elt_named_targets);
            }
            ExpressionKind::DictComp { key, value, .. } => {
                collect_named_expr_targets(key, &mut elt_named_targets);
                collect_named_expr_targets(value, &mut elt_named_targets);
            }
            _ => {}
        }
        for name in &elt_named_targets {
            if prior_loop_targets.contains(name) {
                self.syntax_error(
                    format!(
                        "assignment expression cannot rebind comprehension iteration variable '{}'",
                        name
                    ),
                    expr.location,
                );
            }
        }

        if self.current_scope().scope_type == ScopeType::Class {
            let mut targets = FxHashSet::default();
            match &expr.node {
                ExpressionKind::ListComp { elt, .. }
                | ExpressionKind::SetComp { elt, .. }
                | ExpressionKind::GeneratorExp { elt, .. } => {
                    collect_named_expr_targets(elt, &mut targets);
                }
                ExpressionKind::DictComp { key, value, .. } => {
                    collect_named_expr_targets(key, &mut targets);
                    collect_named_expr_targets(value, &mut targets);
                }
                _ => {}
            }
            for gen in generators {
                for cond in &gen.ifs {
                    collect_named_expr_targets(cond, &mut targets);
                }
            }
            if !targets.is_empty() {
                self.syntax_error(
                    "assignment expression within a comprehension cannot be used in a class body",
                    expr.location,
                );
            }
        }
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
                for arg in args
                    .posonlyargs
                    .iter()
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
                simple,
            } => {
                let in_function_scope = self.current_scope().scope_type == ScopeType::Function;
                if *simple {
                    if let ExpressionKind::Name { id, .. } = &target.node {
                        if let Some(sym) = self.current_scope().symbols.get(id.as_str()) {
                            if sym.scope == SymbolScope::Global
                                && sym.is_explicit_global_or_nonlocal
                            {
                                self.errors.push(CompileError::syntax(
                                    format!("annotated name '{}' can't be global", id),
                                    target.location,
                                ));
                            }
                        }
                    }
                }
                if !in_function_scope {
                    self.analyze_expression(annotation);
                }
                if let Some(val) = value {
                    self.analyze_expression(val);
                    self.analyze_target(target);
                } else if *simple {
                    self.analyze_target(target);
                } else if !in_function_scope {
                    self.analyze_expression(target);
                }
            }

            StatementKind::Return { value } => {
                if let Some(val) = value {
                    self.analyze_expression(val);
                }
            }

            StatementKind::Delete { targets } => {
                for t in targets {
                    // `del x` counts as binding x in the current scope (CPython semantics)
                    if let ExpressionKind::Name { id, .. } = &t.node {
                        self.current_scope().mark_assigned(id);
                    }
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

            StatementKind::While { test, body, orelse } => {
                self.analyze_expression(test);
                for s in body {
                    self.analyze_statement(s);
                }
                for s in orelse {
                    self.analyze_statement(s);
                }
            }

            StatementKind::If { test, body, orelse } => {
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
                    let store_name = alias.asname.as_deref().unwrap_or_else(|| {
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
                    // Check if name was already used before the global declaration
                    if self.current_scope().scope_type == ScopeType::Function {
                        if let Some(sym) = self.current_scope().symbols.get(name.as_str()) {
                            if sym.is_parameter {
                                self.errors.push(CompileError::syntax(
                                    format!("name '{}' is parameter and global", name),
                                    stmt.location,
                                ));
                            } else if sym.is_assigned || sym.is_referenced {
                                self.errors.push(CompileError::syntax(
                                    format!("name '{}' is used prior to global declaration", name),
                                    stmt.location,
                                ));
                            }
                        }
                    }
                    self.current_scope().add_symbol(name, SymbolScope::Global);
                }
            }

            StatementKind::Nonlocal { names } => {
                for name in names {
                    if self.current_scope().scope_type == ScopeType::Function {
                        if let Some(sym) = self.current_scope().symbols.get(name.as_str()) {
                            if sym.is_parameter {
                                self.errors.push(CompileError::syntax(
                                    format!("name '{}' is parameter and nonlocal", name),
                                    stmt.location,
                                ));
                            } else if sym.is_assigned || sym.is_referenced {
                                self.errors.push(CompileError::syntax(
                                    format!(
                                        "name '{}' is used prior to nonlocal declaration",
                                        name
                                    ),
                                    stmt.location,
                                ));
                            }
                        }
                    }
                    self.current_scope().add_symbol(name, SymbolScope::Nonlocal);
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
                    if let Some(sym) = self.current_scope().symbols.get_mut("__class__") {
                        sym.scope = SymbolScope::Free;
                    }
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
                self.validate_named_expr_target(expr);
                self.analyze_expression(value);
                // PEP 572: In comprehensions, walrus target leaks to enclosing scope
                if self.current_scope().scope_type == ScopeType::Comprehension {
                    if let ExpressionKind::Name { id, .. } = &target.node {
                        // Check if enclosing non-comprehension scope has declared this
                        // name as explicitly global or nonlocal — if so, the walrus
                        // must respect that declaration (no Cell/Free promotion).
                        let len = self.scope_stack.len();
                        let mut enclosing_decl: Option<SymbolScope> = None;
                        for i in (0..len - 1).rev() {
                            if self.scope_stack[i].scope_type != ScopeType::Comprehension {
                                if let Some(sym) = self.scope_stack[i].symbols.get(id.as_str()) {
                                    if sym.is_explicit_global_or_nonlocal {
                                        enclosing_decl = Some(sym.scope);
                                    }
                                }
                                break;
                            }
                        }
                        if let Some(decl) = enclosing_decl {
                            // Propagate the same declaration into comprehension scopes
                            // so STORE_GLOBAL / STORE_DEREF is emitted correctly.
                            self.current_scope().add_symbol(id, decl);
                            for i in (0..len - 1).rev() {
                                if self.scope_stack[i].scope_type != ScopeType::Comprehension {
                                    self.scope_stack[i].mark_assigned(id);
                                    break;
                                }
                                self.scope_stack[i].add_symbol(id, decl);
                            }
                        } else {
                            // Mark target as Free in comprehension (will use STORE_DEREF)
                            self.current_scope().add_symbol(id, SymbolScope::Free);
                            // Mark as assigned in the enclosing non-comprehension scope
                            // (resolve_bottom_up will promote it to Cell)
                            for i in (0..len - 1).rev() {
                                if self.scope_stack[i].scope_type != ScopeType::Comprehension {
                                    self.scope_stack[i].mark_assigned(id);
                                    break;
                                }
                                // Intermediate comprehension scopes also need Free
                                self.scope_stack[i].add_symbol(id, SymbolScope::Free);
                            }
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

            ExpressionKind::IfExp { test, body, orelse } => {
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
                self.validate_comprehension_named_exprs(expr, generators);
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
                self.validate_comprehension_named_exprs(expr, generators);
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
                self.validate_comprehension_named_exprs(expr, generators);
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
                    if kw.arg.is_some()
                        && expr_has_named_expr(&kw.value)
                        && kw.value.location == kw.value.outer_location
                    {
                        self.syntax_error("invalid syntax", kw.value.location);
                    }
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

            ExpressionKind::Slice { lower, upper, step } => {
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
            Pattern::MatchMapping {
                keys,
                patterns,
                rest,
            } => {
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
            Pattern::MatchClass {
                cls,
                patterns,
                kwd_patterns,
                ..
            } => {
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
