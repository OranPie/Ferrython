//! AST visitor and transformer traits.

use crate::nodes::*;

/// Immutable AST visitor. Implement this trait to walk an AST without modifying it.
pub trait Visitor {
    type Result: Default;

    fn visit_module(&mut self, module: &Module) -> Self::Result {
        match module {
            Module::Module { body, .. } | Module::Interactive { body } => {
                for stmt in body {
                    self.visit_statement(stmt);
                }
            }
            Module::Expression { body } => {
                self.visit_expression(body);
            }
        }
        Self::Result::default()
    }

    fn visit_statement(&mut self, stmt: &Statement) -> Self::Result {
        self.visit_statement_kind(&stmt.node)
    }

    fn visit_statement_kind(&mut self, node: &StatementKind) -> Self::Result {
        match node {
            StatementKind::FunctionDef {
                body,
                decorator_list,
                args,
                returns,
                ..
            } => {
                for dec in decorator_list {
                    self.visit_expression(dec);
                }
                self.visit_arguments(args);
                if let Some(ret) = returns {
                    self.visit_expression(ret);
                }
                for stmt in body {
                    self.visit_statement(stmt);
                }
            }
            StatementKind::ClassDef {
                bases,
                keywords,
                body,
                decorator_list,
                ..
            } => {
                for dec in decorator_list {
                    self.visit_expression(dec);
                }
                for base in bases {
                    self.visit_expression(base);
                }
                for kw in keywords {
                    self.visit_expression(&kw.value);
                }
                for stmt in body {
                    self.visit_statement(stmt);
                }
            }
            StatementKind::Return { value } => {
                if let Some(v) = value {
                    self.visit_expression(v);
                }
            }
            StatementKind::Delete { targets } => {
                for t in targets {
                    self.visit_expression(t);
                }
            }
            StatementKind::Assign { targets, value, .. } => {
                for t in targets {
                    self.visit_expression(t);
                }
                self.visit_expression(value);
            }
            StatementKind::AugAssign {
                target, value, ..
            } => {
                self.visit_expression(target);
                self.visit_expression(value);
            }
            StatementKind::AnnAssign {
                target,
                annotation,
                value,
                ..
            } => {
                self.visit_expression(target);
                self.visit_expression(annotation);
                if let Some(v) = value {
                    self.visit_expression(v);
                }
            }
            StatementKind::For {
                target,
                iter,
                body,
                orelse,
                ..
            } => {
                self.visit_expression(target);
                self.visit_expression(iter);
                for s in body {
                    self.visit_statement(s);
                }
                for s in orelse {
                    self.visit_statement(s);
                }
            }
            StatementKind::While {
                test,
                body,
                orelse,
            } => {
                self.visit_expression(test);
                for s in body {
                    self.visit_statement(s);
                }
                for s in orelse {
                    self.visit_statement(s);
                }
            }
            StatementKind::If {
                test,
                body,
                orelse,
            } => {
                self.visit_expression(test);
                for s in body {
                    self.visit_statement(s);
                }
                for s in orelse {
                    self.visit_statement(s);
                }
            }
            StatementKind::With { items, body, .. } => {
                for item in items {
                    self.visit_expression(&item.context_expr);
                    if let Some(vars) = &item.optional_vars {
                        self.visit_expression(vars);
                    }
                }
                for s in body {
                    self.visit_statement(s);
                }
            }
            StatementKind::Raise { exc, cause } => {
                if let Some(e) = exc {
                    self.visit_expression(e);
                }
                if let Some(c) = cause {
                    self.visit_expression(c);
                }
            }
            StatementKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                for s in body {
                    self.visit_statement(s);
                }
                for handler in handlers {
                    if let Some(t) = &handler.typ {
                        self.visit_expression(t);
                    }
                    for s in &handler.body {
                        self.visit_statement(s);
                    }
                }
                for s in orelse {
                    self.visit_statement(s);
                }
                for s in finalbody {
                    self.visit_statement(s);
                }
            }
            StatementKind::Assert { test, msg } => {
                self.visit_expression(test);
                if let Some(m) = msg {
                    self.visit_expression(m);
                }
            }
            StatementKind::Import { .. } => {}
            StatementKind::ImportFrom { .. } => {}
            StatementKind::Global { .. } | StatementKind::Nonlocal { .. } => {}
            StatementKind::Expr { value } => {
                self.visit_expression(value);
            }
            StatementKind::Pass | StatementKind::Break | StatementKind::Continue => {}
            StatementKind::Match { subject, cases } => {
                self.visit_expression(subject);
                for case in cases {
                    self.visit_pattern(&case.pattern);
                    if let Some(guard) = &case.guard {
                        self.visit_expression(guard);
                    }
                    for s in &case.body {
                        self.visit_statement(s);
                    }
                }
            }
        }
        Self::Result::default()
    }

    fn visit_expression(&mut self, expr: &Expression) -> Self::Result {
        self.visit_expression_kind(&expr.node)
    }

    fn visit_expression_kind(&mut self, node: &ExpressionKind) -> Self::Result {
        match node {
            ExpressionKind::BoolOp { values, .. } => {
                for v in values {
                    self.visit_expression(v);
                }
            }
            ExpressionKind::NamedExpr { target, value } => {
                self.visit_expression(target);
                self.visit_expression(value);
            }
            ExpressionKind::BinOp { left, right, .. } => {
                self.visit_expression(left);
                self.visit_expression(right);
            }
            ExpressionKind::UnaryOp { operand, .. } => {
                self.visit_expression(operand);
            }
            ExpressionKind::Lambda { body, args } => {
                self.visit_arguments(args);
                self.visit_expression(body);
            }
            ExpressionKind::IfExp {
                test,
                body,
                orelse,
            } => {
                self.visit_expression(test);
                self.visit_expression(body);
                self.visit_expression(orelse);
            }
            ExpressionKind::Dict { keys, values } => {
                for k in keys.iter().flatten() {
                    self.visit_expression(k);
                }
                for v in values {
                    self.visit_expression(v);
                }
            }
            ExpressionKind::Set { elts }
            | ExpressionKind::List { elts, .. }
            | ExpressionKind::Tuple { elts, .. } => {
                for e in elts {
                    self.visit_expression(e);
                }
            }
            ExpressionKind::ListComp { elt, generators }
            | ExpressionKind::SetComp { elt, generators }
            | ExpressionKind::GeneratorExp { elt, generators } => {
                self.visit_expression(elt);
                for gen in generators {
                    self.visit_comprehension(gen);
                }
            }
            ExpressionKind::DictComp {
                key,
                value,
                generators,
            } => {
                self.visit_expression(key);
                self.visit_expression(value);
                for gen in generators {
                    self.visit_comprehension(gen);
                }
            }
            ExpressionKind::Await { value }
            | ExpressionKind::YieldFrom { value }
            | ExpressionKind::Starred { value, .. } => {
                self.visit_expression(value);
            }
            ExpressionKind::Yield { value } => {
                if let Some(v) = value {
                    self.visit_expression(v);
                }
            }
            ExpressionKind::Compare {
                left, comparators, ..
            } => {
                self.visit_expression(left);
                for c in comparators {
                    self.visit_expression(c);
                }
            }
            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                self.visit_expression(func);
                for a in args {
                    self.visit_expression(a);
                }
                for kw in keywords {
                    self.visit_expression(&kw.value);
                }
            }
            ExpressionKind::FormattedValue {
                value, format_spec, ..
            } => {
                self.visit_expression(value);
                if let Some(spec) = format_spec {
                    self.visit_expression(spec);
                }
            }
            ExpressionKind::JoinedStr { values } => {
                for v in values {
                    self.visit_expression(v);
                }
            }
            ExpressionKind::Constant { .. } | ExpressionKind::Name { .. } => {}
            ExpressionKind::Attribute { value, .. } | ExpressionKind::Subscript { value, .. } => {
                self.visit_expression(value);
            }
            ExpressionKind::Slice {
                lower,
                upper,
                step,
            } => {
                if let Some(l) = lower {
                    self.visit_expression(l);
                }
                if let Some(u) = upper {
                    self.visit_expression(u);
                }
                if let Some(s) = step {
                    self.visit_expression(s);
                }
            }
        }
        Self::Result::default()
    }

    fn visit_comprehension(&mut self, comp: &Comprehension) {
        self.visit_expression(&comp.target);
        self.visit_expression(&comp.iter);
        for cond in &comp.ifs {
            self.visit_expression(cond);
        }
    }

    fn visit_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::MatchWildcard | Pattern::MatchCapture { .. } => {}
            Pattern::MatchValue { value } | Pattern::MatchLiteral { value } => {
                self.visit_expression(value);
            }
            Pattern::MatchSequence { patterns } | Pattern::MatchOr { patterns } => {
                for p in patterns {
                    self.visit_pattern(p);
                }
            }
            Pattern::MatchMapping { keys, patterns, .. } => {
                for k in keys {
                    self.visit_expression(k);
                }
                for p in patterns {
                    self.visit_pattern(p);
                }
            }
            Pattern::MatchClass {
                cls,
                patterns,
                kwd_patterns,
                ..
            } => {
                self.visit_expression(cls);
                for p in patterns {
                    self.visit_pattern(p);
                }
                for p in kwd_patterns {
                    self.visit_pattern(p);
                }
            }
            Pattern::MatchAs { pattern, .. } => {
                if let Some(p) = pattern {
                    self.visit_pattern(p);
                }
            }
            Pattern::MatchStar { .. } => {}
        }
    }

    fn visit_arguments(&mut self, args: &Arguments) {
        for arg in &args.posonlyargs {
            if let Some(ann) = &arg.annotation {
                self.visit_expression(ann);
            }
        }
        for arg in &args.args {
            if let Some(ann) = &arg.annotation {
                self.visit_expression(ann);
            }
        }
        if let Some(vararg) = &args.vararg {
            if let Some(ann) = &vararg.annotation {
                self.visit_expression(ann);
            }
        }
        for arg in &args.kwonlyargs {
            if let Some(ann) = &arg.annotation {
                self.visit_expression(ann);
            }
        }
        if let Some(kwarg) = &args.kwarg {
            if let Some(ann) = &kwarg.annotation {
                self.visit_expression(ann);
            }
        }
        for default in &args.defaults {
            self.visit_expression(default);
        }
        for default in args.kw_defaults.iter().flatten() {
            self.visit_expression(default);
        }
    }
}

/// Mutable AST visitor (transformer). Returns new nodes to replace old ones.
pub trait VisitorMut {
    fn visit_statement(&mut self, stmt: Statement) -> Statement {
        stmt
    }

    fn visit_expression(&mut self, expr: Expression) -> Expression {
        expr
    }
}
