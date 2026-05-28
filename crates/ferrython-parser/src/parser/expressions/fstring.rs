//! F-string expression parsing and source-location remapping.

use crate::error::ParseError;
use compact_str::CompactString;
use ferrython_ast::*;

use super::super::{parse_expression_text, Parser};

impl Parser {
    /// Parse f-string content into a JoinedStr AST node.
    /// Splits on `{expr}` and `{{`/`}}` escapes.
    pub(super) fn parse_fstring_content(
        &mut self,
        raw: &str,
        loc: SourceLocation,
    ) -> Result<Expression, ParseError> {
        let mut values: Vec<Expression> = Vec::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        let mut text_buf = String::new();

        while i < chars.len() {
            if chars[i] == '{' {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    // Escaped {{ → literal {
                    text_buf.push('{');
                    i += 2;
                    continue;
                }
                // Flush text buffer
                if !text_buf.is_empty() {
                    values.push(Expression::constant(
                        Constant::Str(CompactString::from(&text_buf)),
                        loc,
                    ));
                    text_buf.clear();
                }
                // Extract expression text between { and }
                i += 1; // skip {
                let mut depth = 1;
                let mut paren_depth = 0; // track () [] {} to avoid treating : inside them as format spec
                let expr_start_offset = i;
                let mut expr_text = String::new();
                let mut conversion: Option<char> = None;
                let mut format_spec = String::new();
                let mut in_format_spec = false;
                let mut in_string: Option<char> = None; // tracks if inside 'x' or "x"
                let mut in_triple = false; // whether in_string is a triple-quoted string
                while i < chars.len() && depth > 0 {
                    let c = chars[i];

                    // Track string literals — skip everything inside quotes
                    if let Some(quote) = in_string {
                        if c == '\\' && i + 1 < chars.len() {
                            // Escaped character inside string — push both and skip
                            if in_format_spec {
                                format_spec.push(c);
                                format_spec.push(chars[i + 1]);
                            } else {
                                expr_text.push(c);
                                expr_text.push(chars[i + 1]);
                            }
                            i += 2;
                            continue;
                        }
                        if c == quote {
                            if in_triple {
                                // Need three consecutive quotes to end
                                if i + 2 < chars.len()
                                    && chars[i + 1] == quote
                                    && chars[i + 2] == quote
                                {
                                    if in_format_spec {
                                        format_spec.push(c);
                                        format_spec.push(c);
                                        format_spec.push(c);
                                    } else {
                                        expr_text.push(c);
                                        expr_text.push(c);
                                        expr_text.push(c);
                                    }
                                    in_string = None;
                                    in_triple = false;
                                    i += 3;
                                    continue;
                                }
                            } else {
                                in_string = None;
                            }
                        }
                        if in_format_spec {
                            format_spec.push(c);
                        } else {
                            expr_text.push(c);
                        }
                        i += 1;
                        continue;
                    }

                    // Not inside a string — check for quote start
                    if (c == '\'' || c == '"') && !in_format_spec {
                        // Detect triple-quoted string
                        if i + 2 < chars.len() && chars[i + 1] == c && chars[i + 2] == c {
                            in_string = Some(c);
                            in_triple = true;
                            expr_text.push(c);
                            expr_text.push(c);
                            expr_text.push(c);
                            i += 3;
                            continue;
                        }
                        in_string = Some(c);
                        in_triple = false;
                        expr_text.push(c);
                        i += 1;
                        continue;
                    }

                    if c == '{' {
                        depth += 1;
                    }
                    if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    // Track parens/brackets within expression
                    if !in_format_spec {
                        match c {
                            '(' | '[' => paren_depth += 1,
                            ')' | ']' => paren_depth -= 1,
                            _ => {}
                        }
                    }
                    if c == '!' && depth == 1 && paren_depth == 0 && !in_format_spec {
                        // Check for conversion: !s, !r, !a
                        if i + 1 < chars.len()
                            && (chars[i + 1] == 's' || chars[i + 1] == 'r' || chars[i + 1] == 'a')
                        {
                            if i + 2 < chars.len() && (chars[i + 2] == '}' || chars[i + 2] == ':') {
                                conversion = Some(chars[i + 1]);
                                i += 2;
                                continue;
                            }
                        }
                    }
                    if c == ':' && depth == 1 && paren_depth == 0 && !in_format_spec {
                        // Only treat as format spec when not inside parens/brackets/strings
                        in_format_spec = true;
                        i += 1;
                        continue;
                    }
                    if in_format_spec {
                        format_spec.push(c);
                    } else {
                        expr_text.push(c);
                    }
                    i += 1;
                }
                // Handle f-string debug `=` format: f"{x=}" → "x=repr(x)"
                // The `=` may be followed by trailing whitespace, e.g. f"{x=  }"
                let trimmed_end = expr_text.trim_end();
                let debug_eq = trimmed_end.ends_with('=')
                    && !trimmed_end.ends_with("==")
                    && !trimmed_end.ends_with("!=")
                    && !trimmed_end.ends_with("<=")
                    && !trimmed_end.ends_with(">=");
                if debug_eq {
                    // The trailing whitespace (between `=` and `}`) is part of the prefix text.
                    let trailing_ws: String = expr_text[trimmed_end.len()..].to_string();
                    // Remove trailing ws + '=' from expr_text
                    expr_text.truncate(trimmed_end.len());
                    expr_text.pop(); // remove '='
                    let prefix = format!("{}={}", expr_text, trailing_ws);
                    values.push(Expression::constant(
                        Constant::Str(CompactString::from(&prefix)),
                        loc,
                    ));
                    if conversion.is_none() && format_spec.is_empty() {
                        conversion = Some('r');
                    }
                }
                // Parse the expression text
                let leading_ws = expr_text.chars().take_while(|c| c.is_whitespace()).count();
                let expr_loc =
                    self.fstring_content_location(raw, loc, expr_start_offset + leading_ws);
                let mut expr = parse_expression_text(&expr_text, loc)?;
                self.remap_fstring_expression_locations(&mut expr, expr_loc);
                // Parse format spec: it can itself contain {expr} (e.g. f"{x:.{n}f}")
                let fmt = if format_spec.is_empty() {
                    None
                } else {
                    let spec_expr = self.parse_fstring_content(&format_spec, loc)?;
                    Some(Box::new(spec_expr))
                };
                values.push(Expression::new(
                    ExpressionKind::FormattedValue {
                        value: Box::new(expr),
                        conversion,
                        format_spec: fmt,
                    },
                    loc,
                ));
            } else if chars[i] == '}' {
                if i + 1 < chars.len() && chars[i + 1] == '}' {
                    text_buf.push('}');
                    i += 2;
                } else {
                    text_buf.push('}');
                    i += 1;
                }
            } else {
                text_buf.push(chars[i]);
                i += 1;
            }
        }
        // Flush remaining text
        if !text_buf.is_empty() {
            values.push(Expression::constant(
                Constant::Str(CompactString::from(&text_buf)),
                loc,
            ));
        }

        // If only one element and it's a constant string, just return it
        if values.len() == 1 {
            if let ExpressionKind::Constant {
                value: Constant::Str(_),
                ..
            } = &values[0].node
            {
                return Ok(values.into_iter().next().unwrap());
            }
        }

        Ok(Expression::new(ExpressionKind::JoinedStr { values }, loc))
    }

    /// Merge an expression (either JoinedStr or plain) into a JoinedStr values list.
    /// Flattens nested JoinedStr nodes so the resulting list is flat.
    pub(super) fn merge_into_joined_str(&self, values: &mut Vec<Expression>, expr: Expression) {
        match expr.node {
            ExpressionKind::JoinedStr { values: inner } => {
                values.extend(inner);
            }
            _ => {
                values.push(expr);
            }
        }
    }

    fn fstring_content_location(
        &self,
        raw: &str,
        string_loc: SourceLocation,
        offset: usize,
    ) -> SourceLocation {
        let quote_len = if string_loc.end_line.unwrap_or(string_loc.line) > string_loc.line {
            3
        } else {
            1
        };
        let mut line = string_loc.line;
        let mut column = string_loc.column + 1 + quote_len;
        for c in raw.chars().take(offset) {
            if c == '\n' {
                line += 1;
                column = 0;
            } else {
                column += c.len_utf8() as u32;
            }
        }
        SourceLocation::new(line, column)
    }

    fn remap_fstring_expression_locations(&self, expr: &mut Expression, start: SourceLocation) {
        fn remap_loc(loc: SourceLocation, start: SourceLocation) -> SourceLocation {
            let map_pos = |line: u32, col: u32| {
                if line <= 1 {
                    (start.line, start.column + col.saturating_sub(1))
                } else {
                    (start.line + line - 1, col)
                }
            };
            let (line, column) = map_pos(loc.line, loc.column);
            let mut out = SourceLocation::new(line, column);
            if let (Some(end_line), Some(end_col)) = (loc.end_line, loc.end_column) {
                let (end_line, end_col) = map_pos(end_line, end_col);
                out = out.with_end(end_line, end_col);
            }
            out
        }

        fn remap_arg(arg: &mut Arg, start: SourceLocation) {
            arg.location = remap_loc(arg.location, start);
            if let Some(annotation) = &mut arg.annotation {
                remap_expr(annotation, start);
            }
        }

        fn remap_arguments(args: &mut Arguments, start: SourceLocation) {
            for arg in &mut args.posonlyargs {
                remap_arg(arg, start);
            }
            for arg in &mut args.args {
                remap_arg(arg, start);
            }
            if let Some(arg) = &mut args.vararg {
                remap_arg(arg, start);
            }
            for arg in &mut args.kwonlyargs {
                remap_arg(arg, start);
            }
            if let Some(arg) = &mut args.kwarg {
                remap_arg(arg, start);
            }
            for default in &mut args.defaults {
                remap_expr(default, start);
            }
            for default in &mut args.kw_defaults {
                if let Some(default) = default {
                    remap_expr(default, start);
                }
            }
        }

        fn remap_keyword(keyword: &mut Keyword, start: SourceLocation) {
            keyword.location = remap_loc(keyword.location, start);
            remap_expr(&mut keyword.value, start);
        }

        fn remap_comprehension(comp: &mut Comprehension, start: SourceLocation) {
            remap_expr(&mut comp.target, start);
            remap_expr(&mut comp.iter, start);
            for if_expr in &mut comp.ifs {
                remap_expr(if_expr, start);
            }
        }

        fn remap_expr(expr: &mut Expression, start: SourceLocation) {
            expr.location = remap_loc(expr.location, start);
            expr.outer_location = remap_loc(expr.outer_location, start);
            match &mut expr.node {
                ExpressionKind::BoolOp { values, .. } => {
                    for value in values {
                        remap_expr(value, start);
                    }
                }
                ExpressionKind::NamedExpr { target, value } => {
                    remap_expr(target, start);
                    remap_expr(value, start);
                }
                ExpressionKind::BinOp { left, right, .. } => {
                    remap_expr(left, start);
                    remap_expr(right, start);
                }
                ExpressionKind::UnaryOp { operand, .. } => remap_expr(operand, start),
                ExpressionKind::Lambda { args, body } => {
                    remap_arguments(args, start);
                    remap_expr(body, start);
                }
                ExpressionKind::IfExp { test, body, orelse } => {
                    remap_expr(test, start);
                    remap_expr(body, start);
                    remap_expr(orelse, start);
                }
                ExpressionKind::Dict { keys, values } => {
                    for key in keys.iter_mut().flatten() {
                        remap_expr(key, start);
                    }
                    for value in values {
                        remap_expr(value, start);
                    }
                }
                ExpressionKind::Set { elts }
                | ExpressionKind::List { elts, .. }
                | ExpressionKind::Tuple { elts, .. } => {
                    for elt in elts {
                        remap_expr(elt, start);
                    }
                }
                ExpressionKind::ListComp { elt, generators }
                | ExpressionKind::SetComp { elt, generators }
                | ExpressionKind::GeneratorExp { elt, generators } => {
                    remap_expr(elt, start);
                    for gen in generators {
                        remap_comprehension(gen, start);
                    }
                }
                ExpressionKind::DictComp {
                    key,
                    value,
                    generators,
                } => {
                    remap_expr(key, start);
                    remap_expr(value, start);
                    for gen in generators {
                        remap_comprehension(gen, start);
                    }
                }
                ExpressionKind::Await { value }
                | ExpressionKind::YieldFrom { value }
                | ExpressionKind::Starred { value, .. } => remap_expr(value, start),
                ExpressionKind::Yield { value } => {
                    if let Some(value) = value {
                        remap_expr(value, start);
                    }
                }
                ExpressionKind::Compare {
                    left, comparators, ..
                } => {
                    remap_expr(left, start);
                    for comparator in comparators {
                        remap_expr(comparator, start);
                    }
                }
                ExpressionKind::Call {
                    func,
                    args,
                    keywords,
                } => {
                    remap_expr(func, start);
                    for arg in args {
                        remap_expr(arg, start);
                    }
                    for keyword in keywords {
                        remap_keyword(keyword, start);
                    }
                }
                ExpressionKind::FormattedValue {
                    value, format_spec, ..
                } => {
                    remap_expr(value, start);
                    if let Some(format_spec) = format_spec {
                        remap_expr(format_spec, start);
                    }
                }
                ExpressionKind::JoinedStr { values } => {
                    for value in values {
                        remap_expr(value, start);
                    }
                }
                ExpressionKind::Attribute { value, .. } => remap_expr(value, start),
                ExpressionKind::Subscript { value, slice, .. } => {
                    remap_expr(value, start);
                    remap_expr(slice, start);
                }
                ExpressionKind::Slice { lower, upper, step } => {
                    if let Some(lower) = lower {
                        remap_expr(lower, start);
                    }
                    if let Some(upper) = upper {
                        remap_expr(upper, start);
                    }
                    if let Some(step) = step {
                        remap_expr(step, start);
                    }
                }
                ExpressionKind::Constant { .. } | ExpressionKind::Name { .. } => {}
            }
        }

        remap_expr(expr, start);
    }
}
