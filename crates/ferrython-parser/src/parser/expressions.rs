//! Expression parsing methods for the Parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::TokenKind;
use compact_str::CompactString;
use ferrython_ast::*;

use super::{parse_expression_text, Parser};

impl Parser {
    // ─── Expression parsing (precedence climbing) ───────────────────

    pub(super) fn parse_expr(&mut self) -> Result<Expression, ParseError> {
        self.parse_test()
    }

    pub(super) fn parse_test(&mut self) -> Result<Expression, ParseError> {
        // Handle yield expression
        if self.check(TokenKind::Yield) {
            return self.parse_yield_expr();
        }

        // Handle lambda
        if self.check(TokenKind::Lambda) {
            return self.parse_lambda();
        }

        let expr = self.parse_or_test()?;

        // Ternary: expr if test else expr
        if self.check(TokenKind::If) {
            let loc = self.current_location();
            self.advance();
            let test = self.parse_or_test()?;
            self.expect(TokenKind::Else)?;
            let orelse = self.parse_test()?;
            return Ok(Expression::new(
                ExpressionKind::IfExp {
                    test: Box::new(test),
                    body: Box::new(expr),
                    orelse: Box::new(orelse),
                },
                loc,
            ));
        }

        Ok(expr)
    }

    pub(super) fn parse_or_test(&mut self) -> Result<Expression, ParseError> {
        let mut expr = self.parse_and_test()?;
        while self.check(TokenKind::Or) {
            let loc = self.current_location();
            self.advance();
            let right = self.parse_and_test()?;
            expr = Expression::new(
                ExpressionKind::BoolOp {
                    op: BoolOperator::Or,
                    values: vec![expr, right],
                },
                loc,
            );
        }
        Ok(expr)
    }

    fn parse_and_test(&mut self) -> Result<Expression, ParseError> {
        let mut expr = self.parse_not_test()?;
        while self.check(TokenKind::And) {
            let loc = self.current_location();
            self.advance();
            let right = self.parse_not_test()?;
            expr = Expression::new(
                ExpressionKind::BoolOp {
                    op: BoolOperator::And,
                    values: vec![expr, right],
                },
                loc,
            );
        }
        Ok(expr)
    }

    fn parse_not_test(&mut self) -> Result<Expression, ParseError> {
        if self.check(TokenKind::Not) {
            let loc = self.current_location();
            self.advance();
            let operand = self.parse_not_test()?;
            return Ok(Expression::new(
                ExpressionKind::UnaryOp {
                    op: UnaryOperator::Not,
                    operand: Box::new(operand),
                },
                loc,
            ));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expression, ParseError> {
        let left = self.parse_or_expr()?;
        let mut ops = Vec::new();
        let mut comparators = Vec::new();

        loop {
            let op = match &self.peek().kind {
                TokenKind::Less => Some(CompareOperator::Lt),
                TokenKind::Greater => Some(CompareOperator::Gt),
                TokenKind::LessEqual => Some(CompareOperator::LtE),
                TokenKind::GreaterEqual => Some(CompareOperator::GtE),
                TokenKind::EqualEqual => Some(CompareOperator::Eq),
                TokenKind::NotEqual => Some(CompareOperator::NotEq),
                TokenKind::In => Some(CompareOperator::In),
                TokenKind::Not => {
                    if self.peek_at(1).map(|t| matches!(t.kind, TokenKind::In)).unwrap_or(false) {
                        self.advance(); // skip 'not'
                        Some(CompareOperator::NotIn)
                    } else {
                        None
                    }
                }
                TokenKind::Is => {
                    if self.peek_at(1).map(|t| matches!(t.kind, TokenKind::Not)).unwrap_or(false) {
                        self.advance(); // skip 'is'
                        Some(CompareOperator::IsNot)
                    } else {
                        Some(CompareOperator::Is)
                    }
                }
                _ => None,
            };

            if let Some(op) = op {
                self.advance();
                ops.push(op);
                comparators.push(self.parse_or_expr()?);
            } else {
                break;
            }
        }

        if ops.is_empty() {
            Ok(left)
        } else {
            let loc = left.location;
            Ok(Expression::new(
                ExpressionKind::Compare {
                    left: Box::new(left),
                    ops,
                    comparators,
                },
                loc,
            ))
        }
    }

    pub(super) fn parse_or_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_xor_expr()?;
        while self.check(TokenKind::Pipe) {
            let loc = left.location;
            self.advance();
            let right = self.parse_xor_expr()?;
            left = Expression::new(
                ExpressionKind::BinOp {
                    left: Box::new(left),
                    op: Operator::BitOr,
                    right: Box::new(right),
                },
                loc,
            );
        }
        Ok(left)
    }

    fn parse_xor_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_and_expr()?;
        while self.check(TokenKind::Caret) {
            let loc = left.location;
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expression::new(
                ExpressionKind::BinOp {
                    left: Box::new(left),
                    op: Operator::BitXor,
                    right: Box::new(right),
                },
                loc,
            );
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_shift_expr()?;
        while self.check(TokenKind::Ampersand) {
            let loc = left.location;
            self.advance();
            let right = self.parse_shift_expr()?;
            left = Expression::new(
                ExpressionKind::BinOp {
                    left: Box::new(left),
                    op: Operator::BitAnd,
                    right: Box::new(right),
                },
                loc,
            );
        }
        Ok(left)
    }

    fn parse_shift_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_arith_expr()?;
        loop {
            let op = match &self.peek().kind {
                TokenKind::LeftShift => Some(Operator::LShift),
                TokenKind::RightShift => Some(Operator::RShift),
                _ => None,
            };
            if let Some(op) = op {
                let loc = left.location;
                self.advance();
                let right = self.parse_arith_expr()?;
                left = Expression::new(
                    ExpressionKind::BinOp {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                    loc,
                );
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_arith_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            let op = match &self.peek().kind {
                TokenKind::Plus => Some(Operator::Add),
                TokenKind::Minus => Some(Operator::Sub),
                _ => None,
            };
            if let Some(op) = op {
                let loc = left.location;
                self.advance();
                let right = self.parse_term()?;
                left = Expression::new(
                    ExpressionKind::BinOp {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                    loc,
                );
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_factor()?;
        loop {
            let op = match &self.peek().kind {
                TokenKind::Star => Some(Operator::Mult),
                TokenKind::Slash => Some(Operator::Div),
                TokenKind::DoubleSlash => Some(Operator::FloorDiv),
                TokenKind::Percent => Some(Operator::Mod),
                TokenKind::At => Some(Operator::MatMult),
                _ => None,
            };
            if let Some(op) = op {
                let loc = left.location;
                self.advance();
                let right = self.parse_factor()?;
                left = Expression::new(
                    ExpressionKind::BinOp {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    },
                    loc,
                );
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        match &self.peek().kind {
            TokenKind::Plus => {
                self.advance();
                let operand = self.parse_factor()?;
                Ok(Expression::new(
                    ExpressionKind::UnaryOp {
                        op: UnaryOperator::UAdd,
                        operand: Box::new(operand),
                    },
                    loc,
                ))
            }
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_factor()?;
                Ok(Expression::new(
                    ExpressionKind::UnaryOp {
                        op: UnaryOperator::USub,
                        operand: Box::new(operand),
                    },
                    loc,
                ))
            }
            TokenKind::Tilde => {
                self.advance();
                let operand = self.parse_factor()?;
                Ok(Expression::new(
                    ExpressionKind::UnaryOp {
                        op: UnaryOperator::Invert,
                        operand: Box::new(operand),
                    },
                    loc,
                ))
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Expression, ParseError> {
        let base = self.parse_atom_expr()?;
        if self.check(TokenKind::DoubleStar) {
            let loc = base.location;
            self.advance();
            let exp = self.parse_factor()?;
            Ok(Expression::new(
                ExpressionKind::BinOp {
                    left: Box::new(base),
                    op: Operator::Pow,
                    right: Box::new(exp),
                },
                loc,
            ))
        } else {
            Ok(base)
        }
    }

    fn parse_atom_expr(&mut self) -> Result<Expression, ParseError> {
        // Handle 'await' prefix
        let is_await = self.check(TokenKind::Await);
        let await_loc = self.current_location();
        if is_await {
            self.advance();
        }

        let mut expr = self.parse_atom()?;

        // Trailers: .attr, [subscript], (call)
        loop {
            match &self.peek().kind {
                TokenKind::LeftParen => {
                    let loc = expr.location;
                    self.advance();
                    let (args, keywords) = self.parse_call_args()?;
                    self.expect(TokenKind::RightParen)?;
                    expr = Expression::new(
                        ExpressionKind::Call {
                            func: Box::new(expr),
                            args,
                            keywords,
                        },
                        loc,
                    );
                }
                TokenKind::LeftBracket => {
                    let loc = expr.location;
                    self.advance();
                    let slice = self.parse_subscript()?;
                    self.expect(TokenKind::RightBracket)?;
                    expr = Expression::new(
                        ExpressionKind::Subscript {
                            value: Box::new(expr),
                            slice: Box::new(slice),
                            ctx: ExprContext::Load,
                        },
                        loc,
                    );
                }
                TokenKind::Dot => {
                    let loc = expr.location;
                    self.advance();
                    let attr = self.expect_name()?;
                    expr = Expression::new(
                        ExpressionKind::Attribute {
                            value: Box::new(expr),
                            attr,
                            ctx: ExprContext::Load,
                        },
                        loc,
                    );
                }
                _ => break,
            }
        }

        if is_await {
            expr = Expression::new(
                ExpressionKind::Await {
                    value: Box::new(expr),
                },
                await_loc,
            );
        }

        Ok(expr)
    }

    pub(super) fn parse_atom(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        let tok = self.peek().clone();

        match &tok.kind {
            TokenKind::Name(name) => {
                let name = name.clone();
                self.advance();
                // Check for walrus operator
                if self.check(TokenKind::ColonEqual) {
                    self.advance();
                    let value = self.parse_test()?;
                    return Ok(Expression::new(
                        ExpressionKind::NamedExpr {
                            target: Box::new(Expression::name(name, ExprContext::Store, loc)),
                            value: Box::new(value),
                        },
                        loc,
                    ));
                }
                Ok(Expression::name(name, ExprContext::Load, loc))
            }
            TokenKind::Int(n) => {
                let n = n.clone();
                self.advance();
                Ok(Expression::constant(Constant::Int(n), loc))
            }
            TokenKind::Float(f) => {
                let f = *f;
                self.advance();
                Ok(Expression::constant(Constant::Float(f), loc))
            }
            TokenKind::Complex(f) => {
                let f = *f;
                self.advance();
                Ok(Expression::constant(Constant::Complex { real: 0.0, imag: f }, loc))
            }
            TokenKind::String(s) => {
                let mut result = s.to_string();
                self.advance();
                // Consume adjacent plain strings
                while let TokenKind::String(s2) = &self.peek().kind {
                    result.push_str(s2.as_str());
                    self.advance();
                }
                // Check if next token is an f-string — need JoinedStr
                if matches!(self.peek().kind, TokenKind::FString(_)) {
                    let mut values: Vec<Expression> = Vec::new();
                    if !result.is_empty() {
                        values.push(Expression::constant(
                            Constant::Str(CompactString::from(&result)), loc,
                        ));
                    }
                    loop {
                        match &self.peek().kind {
                            TokenKind::FString(raw) => {
                                let raw = raw.clone();
                                self.advance();
                                let fexpr = self.parse_fstring_content(&raw, loc)?;
                                self.merge_into_joined_str(&mut values, fexpr);
                            }
                            TokenKind::String(s2) => {
                                let mut plain = s2.to_string();
                                self.advance();
                                while let TokenKind::String(s3) = &self.peek().kind {
                                    plain.push_str(s3.as_str());
                                    self.advance();
                                }
                                values.push(Expression::constant(
                                    Constant::Str(CompactString::from(&plain)), loc,
                                ));
                            }
                            _ => break,
                        }
                    }
                    Ok(Expression::new(ExpressionKind::JoinedStr { values }, loc))
                } else {
                    Ok(Expression::constant(
                        Constant::Str(CompactString::from(result)),
                        loc,
                    ))
                }
            }
            TokenKind::Bytes(b) => {
                let mut result = b.clone();
                self.advance();
                while let TokenKind::Bytes(b2) = &self.peek().kind {
                    result.extend_from_slice(b2);
                    self.advance();
                }
                Ok(Expression::constant(Constant::Bytes(result), loc))
            }
            TokenKind::FString(raw) => {
                let raw = raw.clone();
                self.advance();
                let fexpr = self.parse_fstring_content(&raw, loc)?;
                // Check for adjacent strings/fstrings — concatenate into single JoinedStr
                if matches!(self.peek().kind, TokenKind::String(_) | TokenKind::FString(_)) {
                    let mut values: Vec<Expression> = Vec::new();
                    self.merge_into_joined_str(&mut values, fexpr);
                    loop {
                        match &self.peek().kind {
                            TokenKind::FString(raw2) => {
                                let raw2 = raw2.clone();
                                self.advance();
                                let fexpr2 = self.parse_fstring_content(&raw2, loc)?;
                                self.merge_into_joined_str(&mut values, fexpr2);
                            }
                            TokenKind::String(s) => {
                                let mut plain = s.to_string();
                                self.advance();
                                while let TokenKind::String(s2) = &self.peek().kind {
                                    plain.push_str(s2.as_str());
                                    self.advance();
                                }
                                values.push(Expression::constant(
                                    Constant::Str(CompactString::from(&plain)), loc,
                                ));
                            }
                            _ => break,
                        }
                    }
                    Ok(Expression::new(ExpressionKind::JoinedStr { values }, loc))
                } else {
                    Ok(fexpr)
                }
            }
            TokenKind::True => {
                self.advance();
                Ok(Expression::constant(Constant::Bool(true), loc))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expression::constant(Constant::Bool(false), loc))
            }
            TokenKind::None => {
                self.advance();
                Ok(Expression::constant(Constant::None, loc))
            }
            TokenKind::Ellipsis => {
                self.advance();
                Ok(Expression::constant(Constant::Ellipsis, loc))
            }
            TokenKind::LeftParen => {
                self.advance();
                if self.check(TokenKind::RightParen) {
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                let expr = self.parse_test_list_star_expr()?;
                // Check for generator expression (including async)
                if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                    let generators = self.parse_comp_for()?;
                    self.expect(TokenKind::RightParen)?;
                    return Ok(Expression::new(
                        ExpressionKind::GeneratorExp {
                            elt: Box::new(expr),
                            generators,
                        },
                        loc,
                    ));
                }
                // Check for tuple
                if self.check(TokenKind::Comma) {
                    let mut elts = vec![expr];
                    while self.check(TokenKind::Comma) {
                        self.advance();
                        if self.check(TokenKind::RightParen) {
                            break;
                        }
                        elts.push(self.parse_test_or_star()?);
                    }
                    self.expect(TokenKind::RightParen)?;
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts,
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                self.expect(TokenKind::RightParen)?;
                Ok(expr)
            }
            TokenKind::LeftBracket => {
                self.advance();
                if self.check(TokenKind::RightBracket) {
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::List {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                let first = self.parse_test_or_star()?;
                // List comprehension? (including async)
                if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                    let generators = self.parse_comp_for()?;
                    self.expect(TokenKind::RightBracket)?;
                    return Ok(Expression::new(
                        ExpressionKind::ListComp {
                            elt: Box::new(first),
                            generators,
                        },
                        loc,
                    ));
                }
                let mut elts = vec![first];
                while self.check(TokenKind::Comma) {
                    self.advance();
                    if self.check(TokenKind::RightBracket) {
                        break;
                    }
                    elts.push(self.parse_test_or_star()?);
                }
                self.expect(TokenKind::RightBracket)?;
                Ok(Expression::new(
                    ExpressionKind::List {
                        elts,
                        ctx: ExprContext::Load,
                    },
                    loc,
                ))
            }
            TokenKind::LeftBrace => {
                self.advance();
                if self.check(TokenKind::RightBrace) {
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::Dict {
                            keys: Vec::new(),
                            values: Vec::new(),
                        },
                        loc,
                    ));
                }
                if self.check(TokenKind::DoubleStar) {
                    // Dict starting with **unpacking
                    self.advance();
                    let first_val = self.parse_test()?;
                    let mut keys: Vec<Option<Expression>> = vec![None];
                    let mut values = vec![first_val];
                    while self.check(TokenKind::Comma) {
                        self.advance();
                        if self.check(TokenKind::RightBrace) {
                            break;
                        }
                        if self.check(TokenKind::DoubleStar) {
                            self.advance();
                            keys.push(None);
                            values.push(self.parse_test()?);
                        } else {
                            let k = self.parse_test()?;
                            self.expect(TokenKind::Colon)?;
                            let v = self.parse_test()?;
                            keys.push(Some(k));
                            values.push(v);
                        }
                    }
                    self.expect(TokenKind::RightBrace)?;
                    Ok(Expression::new(
                        ExpressionKind::Dict { keys, values },
                        loc,
                    ))
                } else {
                // Could be dict or set
                let first = self.parse_test_or_star()?;
                if self.check(TokenKind::Colon) {
                    // Dict
                    self.advance();
                    let first_val = self.parse_test()?;
                    // Dict comprehension? (including async)
                    if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                        let generators = self.parse_comp_for()?;
                        self.expect(TokenKind::RightBrace)?;
                        return Ok(Expression::new(
                            ExpressionKind::DictComp {
                                key: Box::new(first),
                                value: Box::new(first_val),
                                generators,
                            },
                            loc,
                        ));
                    }
                    let mut keys = vec![Some(first)];
                    let mut values = vec![first_val];
                    while self.check(TokenKind::Comma) {
                        self.advance();
                        if self.check(TokenKind::RightBrace) {
                            break;
                        }
                        if self.check(TokenKind::DoubleStar) {
                            self.advance();
                            keys.push(None);
                            values.push(self.parse_test()?);
                        } else {
                            let k = self.parse_test()?;
                            self.expect(TokenKind::Colon)?;
                            let v = self.parse_test()?;
                            keys.push(Some(k));
                            values.push(v);
                        }
                    }
                    self.expect(TokenKind::RightBrace)?;
                    Ok(Expression::new(
                        ExpressionKind::Dict { keys, values },
                        loc,
                    ))
                } else {
                    // Set (including async comprehension)
                    if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                        let generators = self.parse_comp_for()?;
                        self.expect(TokenKind::RightBrace)?;
                        return Ok(Expression::new(
                            ExpressionKind::SetComp {
                                elt: Box::new(first),
                                generators,
                            },
                            loc,
                        ));
                    }
                    let mut elts = vec![first];
                    while self.check(TokenKind::Comma) {
                        self.advance();
                        if self.check(TokenKind::RightBrace) {
                            break;
                        }
                        elts.push(self.parse_test_or_star()?);
                    }
                    self.expect(TokenKind::RightBrace)?;
                    Ok(Expression::new(ExpressionKind::Set { elts }, loc))
                }
                } // end else for non-DoubleStar first element
            }
            _ => Err(ParseError::new(
                ParseErrorKind::ExpressionExpected,
                tok.span,
            )),
        }
    }

    fn parse_yield_expr(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Yield)?;

        // Check for `yield from expr`
        if self.check(TokenKind::From) {
            self.advance();
            let value = self.parse_test()?;
            return Ok(Expression::new(
                ExpressionKind::YieldFrom { value: Box::new(value) },
                loc,
            ));
        }

        // Check if there's a value after yield (not at end of statement/expression)
        if self.at_expression_start() {
            let value = self.parse_test_list_star_expr()?;
            Ok(Expression::new(
                ExpressionKind::Yield { value: Some(Box::new(value)) },
                loc,
            ))
        } else {
            Ok(Expression::new(
                ExpressionKind::Yield { value: None },
                loc,
            ))
        }
    }

    /// Check if the current token could start an expression.
    fn at_expression_start(&self) -> bool {
        matches!(self.peek().kind,
            TokenKind::Name(_) | TokenKind::Int(_) | TokenKind::Float(_) |
            TokenKind::String(_) | TokenKind::Bytes(_) | TokenKind::FString(_) |
            TokenKind::True | TokenKind::False | TokenKind::None |
            TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace |
            TokenKind::Minus | TokenKind::Plus | TokenKind::Tilde | TokenKind::Not |
            TokenKind::Lambda | TokenKind::Yield | TokenKind::Ellipsis
        )
    }

    /// Parse f-string content into a JoinedStr AST node.
    /// Splits on `{expr}` and `{{`/`}}` escapes.
    fn parse_fstring_content(
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
                        Constant::Str(CompactString::from(&text_buf)), loc,
                    ));
                    text_buf.clear();
                }
                // Extract expression text between { and }
                i += 1; // skip {
                let mut depth = 1;
                let mut paren_depth = 0; // track () [] {} to avoid treating : inside them as format spec
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
                                if i + 2 < chars.len() && chars[i+1] == quote && chars[i+2] == quote {
                                    if in_format_spec {
                                        format_spec.push(c); format_spec.push(c); format_spec.push(c);
                                    } else {
                                        expr_text.push(c); expr_text.push(c); expr_text.push(c);
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
                        if i + 2 < chars.len() && chars[i+1] == c && chars[i+2] == c {
                            in_string = Some(c);
                            in_triple = true;
                            expr_text.push(c); expr_text.push(c); expr_text.push(c);
                            i += 3;
                            continue;
                        }
                        in_string = Some(c);
                        in_triple = false;
                        expr_text.push(c);
                        i += 1;
                        continue;
                    }

                    if c == '{' { depth += 1; }
                    if c == '}' {
                        depth -= 1;
                        if depth == 0 { i += 1; break; }
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
                        if i + 1 < chars.len() && (chars[i+1] == 's' || chars[i+1] == 'r' || chars[i+1] == 'a') {
                            if i + 2 < chars.len() && (chars[i+2] == '}' || chars[i+2] == ':') {
                                conversion = Some(chars[i+1]);
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
                let debug_eq = trimmed_end.ends_with('=') && !trimmed_end.ends_with("==")
                    && !trimmed_end.ends_with("!=") && !trimmed_end.ends_with("<=")
                    && !trimmed_end.ends_with(">=");
                if debug_eq {
                    // The trailing whitespace (between `=` and `}`) is part of the prefix text.
                    let trailing_ws: String = expr_text[trimmed_end.len()..].to_string();
                    // Remove trailing ws + '=' from expr_text
                    expr_text.truncate(trimmed_end.len());
                    expr_text.pop(); // remove '='
                    let prefix = format!("{}={}", expr_text, trailing_ws);
                    values.push(Expression::constant(
                        Constant::Str(CompactString::from(&prefix)), loc,
                    ));
                    if conversion.is_none() && format_spec.is_empty() {
                        conversion = Some('r');
                    }
                }
                // Parse the expression text
                let expr = parse_expression_text(&expr_text, loc)?;
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
                Constant::Str(CompactString::from(&text_buf)), loc,
            ));
        }

        // If only one element and it's a constant string, just return it
        if values.len() == 1 {
            if let ExpressionKind::Constant { value: Constant::Str(_), .. } = &values[0].node {
                return Ok(values.into_iter().next().unwrap());
            }
        }

        Ok(Expression::new(ExpressionKind::JoinedStr { values }, loc))
    }

    /// Merge an expression (either JoinedStr or plain) into a JoinedStr values list.
    /// Flattens nested JoinedStr nodes so the resulting list is flat.
    fn merge_into_joined_str(&self, values: &mut Vec<Expression>, expr: Expression) {
        match expr.node {
            ExpressionKind::JoinedStr { values: inner } => {
                values.extend(inner);
            }
            _ => {
                values.push(expr);
            }
        }
    }

    fn parse_lambda(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Lambda)?;
        let args = if self.check(TokenKind::Colon) {
            Arguments::empty()
        } else {
            self.parse_lambda_params()?
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_test()?;
        Ok(Expression::new(
            ExpressionKind::Lambda {
                args: Box::new(args),
                body: Box::new(body),
            },
            loc,
        ))
    }

    /// Parse lambda parameters (no annotations, no parens).
    fn parse_lambda_params(&mut self) -> Result<Arguments, ParseError> {
        let mut args = Arguments::empty();
        let mut seen_star = false;

        loop {
            if self.check(TokenKind::Colon) { break; }

            if self.check(TokenKind::Slash) {
                // Positional-only separator: move all args so far to posonlyargs
                self.advance();
                args.posonlyargs.append(&mut args.args);
            } else if self.check(TokenKind::Star) {
                self.advance();
                seen_star = true;
                if self.check(TokenKind::Comma) || self.check(TokenKind::Colon) {
                    // bare * separator
                } else {
                    let name = self.expect_name()?;
                    args.vararg = Some(Arg {
                        arg: name, annotation: None, type_comment: None,
                        location: self.current_location(),
                    });
                }
            } else if self.check(TokenKind::DoubleStar) {
                self.advance();
                let name = self.expect_name()?;
                args.kwarg = Some(Arg {
                    arg: name, annotation: None, type_comment: None,
                    location: self.current_location(),
                });
            } else {
                let name = self.expect_name()?;
                let default = if self.check(TokenKind::Equal) {
                    self.advance();
                    Some(self.parse_test()?)
                } else {
                    None
                };
                let arg = Arg {
                    arg: name, annotation: None, type_comment: None,
                    location: self.current_location(),
                };
                if seen_star {
                    args.kwonlyargs.push(arg);
                    args.kw_defaults.push(default);
                } else {
                    args.args.push(arg);
                    if let Some(d) = default { args.defaults.push(d); }
                }
            }

            if !self.check(TokenKind::Comma) { break; }
            self.advance();
        }
        Ok(args)
    }

    fn parse_subscript(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        let first = self.parse_subscript_element()?;

        // Multi-dimensional subscript: a[1:2, 3:4] → a[(slice(1,2), slice(3,4))]
        if self.check(TokenKind::Comma) {
            let mut elements = vec![first];
            while self.check(TokenKind::Comma) {
                self.advance();
                if self.check(TokenKind::RightBracket) { break; }
                elements.push(self.parse_subscript_element()?);
            }
            return Ok(Expression::new(
                ExpressionKind::Tuple { elts: elements, ctx: ExprContext::Load },
                loc,
            ));
        }
        Ok(first)
    }

    fn parse_subscript_element(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        let lower = if self.check(TokenKind::Colon) {
            None
        } else {
            Some(Box::new(self.parse_test()?))
        };
        if !self.check(TokenKind::Colon) {
            return Ok(*lower.unwrap());
        }
        self.advance(); // skip ':'
        let upper = if !self.check(TokenKind::Colon)
            && !self.check(TokenKind::RightBracket)
            && !self.check(TokenKind::Comma)
        {
            Some(Box::new(self.parse_test()?))
        } else {
            None
        };
        let step = if self.check(TokenKind::Colon) {
            self.advance();
            if !self.check(TokenKind::RightBracket) && !self.check(TokenKind::Comma) {
                Some(Box::new(self.parse_test()?))
            } else {
                None
            }
        } else {
            None
        };
        Ok(Expression::new(
            ExpressionKind::Slice {
                lower,
                upper,
                step,
            },
            loc,
        ))
    }

    // ─── Comprehension parsing ──────────────────────────────────────

    pub(super) fn parse_comp_for(&mut self) -> Result<Vec<Comprehension>, ParseError> {
        let mut generators = Vec::new();
        while self.check(TokenKind::For) || self.check(TokenKind::Async) {
            let is_async = self.check(TokenKind::Async);
            if is_async {
                self.advance();
            }
            self.expect(TokenKind::For)?;
            let target = self.parse_target_list()?;
            self.expect(TokenKind::In)?;
            let iter = self.parse_or_test()?;
            let mut ifs = Vec::new();
            while self.check(TokenKind::If) {
                self.advance();
                ifs.push(self.parse_or_test()?);
            }
            generators.push(Comprehension {
                target,
                iter,
                ifs,
                is_async,
            });
        }
        Ok(generators)
    }

    // ─── Helper expression parsers ──────────────────────────────────

    pub(super) fn parse_test_list(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_test()?;
        if !self.check(TokenKind::Comma) {
            return Ok(first);
        }
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                break;
            }
            elts.push(self.parse_test()?);
        }
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Load,
            },
            loc,
        ))
    }

    pub(super) fn parse_test_list_star_expr(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_test_or_star()?;
        if !self.check(TokenKind::Comma) {
            return Ok(first);
        }
        // Could be a tuple target or value — use Load context here.
        // The compiler's compile_store_target handles Store context separately.
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::Equal)
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                break;
            }
            elts.push(self.parse_test_or_star()?);
        }
        // Note: multiple starred expressions are only invalid in assignment targets,
        // NOT in expression context (PEP 448: [*a, *b] and (*a, *b) are valid).
        // The compiler's compile_store_target handles assignment-target validation.
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Load,
            },
            loc,
        ))
    }

    fn parse_test_or_star(&mut self) -> Result<Expression, ParseError> {
        if self.check(TokenKind::Star) {
            let loc = self.current_location();
            self.advance();
            let expr = self.parse_or_expr()?;
            Ok(Expression::new(
                ExpressionKind::Starred {
                    value: Box::new(expr),
                    ctx: ExprContext::Load,
                },
                loc,
            ))
        } else {
            self.parse_test()
        }
    }

    pub(super) fn parse_target(&mut self) -> Result<Expression, ParseError> {
        // Support starred targets: `for a, *b in items:`
        if self.check(TokenKind::Star) {
            let loc = self.current_location();
            self.advance();
            let expr = self.parse_or_expr()?;
            return Ok(Expression::new(
                ExpressionKind::Starred {
                    value: Box::new(expr),
                    ctx: ExprContext::Store,
                },
                loc,
            ));
        }
        self.parse_or_expr()
    }

    pub(super) fn parse_target_list(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_target()?;
        if !self.check(TokenKind::Comma) || self.check(TokenKind::In) {
            return Ok(first);
        }
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check(TokenKind::In) {
                break;
            }
            elts.push(self.parse_target()?);
        }
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Store,
            },
            loc,
        ))
    }
}
