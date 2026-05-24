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
            self.advance();
            let test = self.parse_or_test()?;
            self.expect(TokenKind::Else)?;
            let orelse = self.parse_test()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&expr),
                Self::expression_outer_location(&orelse),
            );
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
            self.advance();
            let right = self.parse_and_test()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&expr),
                Self::expression_outer_location(&right),
            );
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
            self.advance();
            let right = self.parse_not_test()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&expr),
                Self::expression_outer_location(&right),
            );
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
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&operand));
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
                    if self
                        .peek_at(1)
                        .map(|t| matches!(t.kind, TokenKind::In))
                        .unwrap_or(false)
                    {
                        self.advance(); // skip 'not'
                        Some(CompareOperator::NotIn)
                    } else {
                        None
                    }
                }
                TokenKind::Is => {
                    if self
                        .peek_at(1)
                        .map(|t| matches!(t.kind, TokenKind::Not))
                        .unwrap_or(false)
                    {
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
            let end = comparators
                .last()
                .map(Self::expression_outer_location)
                .unwrap_or(left.location);
            let loc = Self::with_end_location(Self::expression_outer_location(&left), end);
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
            self.advance();
            let right = self.parse_xor_expr()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&left),
                Self::expression_outer_location(&right),
            );
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
            self.advance();
            let right = self.parse_and_expr()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&left),
                Self::expression_outer_location(&right),
            );
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
            self.advance();
            let right = self.parse_shift_expr()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&left),
                Self::expression_outer_location(&right),
            );
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
                self.advance();
                let right = self.parse_arith_expr()?;
                let loc = Self::with_end_location(
                    Self::expression_outer_location(&left),
                    Self::expression_outer_location(&right),
                );
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
                self.advance();
                let right = self.parse_term()?;
                let loc = Self::with_end_location(
                    Self::expression_outer_location(&left),
                    Self::expression_outer_location(&right),
                );
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
                self.advance();
                let right = self.parse_factor()?;
                let loc = Self::with_end_location(
                    Self::expression_outer_location(&left),
                    Self::expression_outer_location(&right),
                );
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
                let loc = Self::with_end_location(loc, Self::expression_outer_location(&operand));
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
                let loc = Self::with_end_location(loc, Self::expression_outer_location(&operand));
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
                let loc = Self::with_end_location(loc, Self::expression_outer_location(&operand));
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
            self.advance();
            let exp = self.parse_factor()?;
            let loc = Self::with_end_location(
                Self::expression_outer_location(&base),
                Self::expression_outer_location(&exp),
            );
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

        let trailer_start = if self.check(TokenKind::LeftParen) {
            Some(self.current_location())
        } else {
            None
        };
        let mut expr = self.parse_atom()?;

        // Trailers: .attr, [subscript], (call)
        loop {
            match &self.peek().kind {
                TokenKind::LeftParen => {
                    let open_location = self.current_location();
                    let loc =
                        trailer_start.unwrap_or_else(|| Self::expression_outer_location(&expr));
                    self.advance();
                    let (mut args, keywords) = self.parse_call_args(open_location)?;
                    let rparen_span = self.expect(TokenKind::RightParen)?.span;
                    if keywords.is_empty()
                        && args.len() == 1
                        && args[0].location.line == open_location.line
                        && args[0].location.column == open_location.column
                        && matches!(args[0].node, ExpressionKind::GeneratorExp { .. })
                    {
                        let gen_loc = Self::with_end_span(args[0].location, rparen_span);
                        args[0].location = gen_loc;
                        args[0].outer_location = gen_loc;
                    }
                    let loc = Self::with_end_span(loc, rparen_span);
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
                    let loc =
                        trailer_start.unwrap_or_else(|| Self::expression_outer_location(&expr));
                    self.advance();
                    let slice = self.parse_subscript()?;
                    let rbracket_span = self.expect(TokenKind::RightBracket)?.span;
                    let loc = Self::with_end_span(loc, rbracket_span);
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
                    let loc =
                        trailer_start.unwrap_or_else(|| Self::expression_outer_location(&expr));
                    self.advance();
                    let attr_span = self.peek().span;
                    let attr = self.expect_name()?;
                    let loc = Self::with_end_span(loc, attr_span);
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
            let await_loc =
                Self::with_end_location(await_loc, Self::expression_outer_location(&expr));
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
                    let named_loc =
                        Self::with_end_location(loc, Self::expression_outer_location(&value));
                    return Ok(Expression::new(
                        ExpressionKind::NamedExpr {
                            target: Box::new(Expression::name(name, ExprContext::Store, loc)),
                            value: Box::new(value),
                        },
                        named_loc,
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
                Ok(Expression::constant(
                    Constant::Complex { real: 0.0, imag: f },
                    loc,
                ))
            }
            TokenKind::String(s) => {
                let mut result = s.to_string();
                let mut end_span = tok.span;
                self.advance();
                // Consume adjacent plain strings
                while let TokenKind::String(s2) = &self.peek().kind {
                    end_span = self.peek().span;
                    result.push_str(s2.as_str());
                    self.advance();
                }
                let string_loc = Self::with_end_span(loc, end_span);
                // Check if next token is an f-string — need JoinedStr
                if matches!(self.peek().kind, TokenKind::FString(_)) {
                    let mut values: Vec<Expression> = Vec::new();
                    if !result.is_empty() {
                        values.push(Expression::constant(
                            Constant::Str(CompactString::from(&result)),
                            string_loc,
                        ));
                    }
                    loop {
                        match &self.peek().kind {
                            TokenKind::FString(raw) => {
                                let f_loc = self.current_location();
                                end_span = self.peek().span;
                                let raw = raw.clone();
                                self.advance();
                                let fexpr = self.parse_fstring_content(&raw, f_loc)?;
                                self.merge_into_joined_str(&mut values, fexpr);
                            }
                            TokenKind::String(s2) => {
                                end_span = self.peek().span;
                                let plain_start = self.current_location();
                                let mut plain = s2.to_string();
                                self.advance();
                                while let TokenKind::String(s3) = &self.peek().kind {
                                    end_span = self.peek().span;
                                    plain.push_str(s3.as_str());
                                    self.advance();
                                }
                                let plain_loc = Self::with_end_span(plain_start, end_span);
                                values.push(Expression::constant(
                                    Constant::Str(CompactString::from(&plain)),
                                    plain_loc,
                                ));
                            }
                            _ => break,
                        }
                    }
                    let joined_loc = Self::with_end_span(loc, end_span);
                    Ok(Expression::new(
                        ExpressionKind::JoinedStr { values },
                        joined_loc,
                    ))
                } else {
                    Ok(Expression::constant(
                        Constant::Str(CompactString::from(result)),
                        string_loc,
                    ))
                }
            }
            TokenKind::Bytes(b) => {
                let mut result = b.clone();
                let mut end_span = tok.span;
                self.advance();
                while let TokenKind::Bytes(b2) = &self.peek().kind {
                    end_span = self.peek().span;
                    result.extend_from_slice(b2);
                    self.advance();
                }
                let bytes_loc = Self::with_end_span(loc, end_span);
                Ok(Expression::constant(Constant::Bytes(result), bytes_loc))
            }
            TokenKind::FString(raw) => {
                let mut end_span = tok.span;
                let raw = raw.clone();
                self.advance();
                let fexpr = self.parse_fstring_content(&raw, loc)?;
                // Check for adjacent strings/fstrings — concatenate into single JoinedStr
                if matches!(
                    self.peek().kind,
                    TokenKind::String(_) | TokenKind::FString(_)
                ) {
                    let mut values: Vec<Expression> = Vec::new();
                    self.merge_into_joined_str(&mut values, fexpr);
                    loop {
                        match &self.peek().kind {
                            TokenKind::FString(raw2) => {
                                let f_loc = self.current_location();
                                end_span = self.peek().span;
                                let raw2 = raw2.clone();
                                self.advance();
                                let fexpr2 = self.parse_fstring_content(&raw2, f_loc)?;
                                self.merge_into_joined_str(&mut values, fexpr2);
                            }
                            TokenKind::String(s) => {
                                end_span = self.peek().span;
                                let plain_start = self.current_location();
                                let mut plain = s.to_string();
                                self.advance();
                                while let TokenKind::String(s2) = &self.peek().kind {
                                    end_span = self.peek().span;
                                    plain.push_str(s2.as_str());
                                    self.advance();
                                }
                                let plain_loc = Self::with_end_span(plain_start, end_span);
                                values.push(Expression::constant(
                                    Constant::Str(CompactString::from(&plain)),
                                    plain_loc,
                                ));
                            }
                            _ => break,
                        }
                    }
                    let joined_loc = Self::with_end_span(loc, end_span);
                    Ok(Expression::new(
                        ExpressionKind::JoinedStr { values },
                        joined_loc,
                    ))
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
                    let rparen_span = self.expect(TokenKind::RightParen)?.span;
                    let tuple_loc = Self::with_end_span(loc, rparen_span);
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        tuple_loc,
                    ));
                }
                let mut expr = self.parse_test_list_star_expr()?;
                // Check for generator expression (including async)
                if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                    let generators = self.parse_comp_for()?;
                    let rparen_span = self.expect(TokenKind::RightParen)?.span;
                    let gen_loc = Self::with_end_span(loc, rparen_span);
                    return Ok(Expression::new(
                        ExpressionKind::GeneratorExp {
                            elt: Box::new(expr),
                            generators,
                        },
                        gen_loc,
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
                    let rparen_span = self.expect(TokenKind::RightParen)?.span;
                    let tuple_loc = Self::with_end_span(loc, rparen_span);
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts,
                            ctx: ExprContext::Load,
                        },
                        tuple_loc,
                    ));
                }
                let rparen_span = self.expect(TokenKind::RightParen)?.span;
                let outer_loc = Self::with_end_span(loc, rparen_span);
                if matches!(expr.node, ExpressionKind::Tuple { .. }) {
                    expr.location = outer_loc;
                    expr.outer_location = outer_loc;
                    Ok(expr)
                } else {
                    Ok(expr.with_outer_location(outer_loc))
                }
            }
            TokenKind::LeftBracket => {
                self.advance();
                if self.check(TokenKind::RightBracket) {
                    let rbracket_span = self.expect(TokenKind::RightBracket)?.span;
                    let list_loc = Self::with_end_span(loc, rbracket_span);
                    return Ok(Expression::new(
                        ExpressionKind::List {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        list_loc,
                    ));
                }
                let first = self.parse_test_or_star()?;
                // List comprehension? (including async)
                if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                    let generators = self.parse_comp_for()?;
                    let rbracket_span = self.expect(TokenKind::RightBracket)?.span;
                    let list_loc = Self::with_end_span(loc, rbracket_span);
                    return Ok(Expression::new(
                        ExpressionKind::ListComp {
                            elt: Box::new(first),
                            generators,
                        },
                        list_loc,
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
                let rbracket_span = self.expect(TokenKind::RightBracket)?.span;
                let list_loc = Self::with_end_span(loc, rbracket_span);
                Ok(Expression::new(
                    ExpressionKind::List {
                        elts,
                        ctx: ExprContext::Load,
                    },
                    list_loc,
                ))
            }
            TokenKind::LeftBrace => {
                self.advance();
                if self.check(TokenKind::RightBrace) {
                    let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                    let dict_loc = Self::with_end_span(loc, rbrace_span);
                    return Ok(Expression::new(
                        ExpressionKind::Dict {
                            keys: Vec::new(),
                            values: Vec::new(),
                        },
                        dict_loc,
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
                    let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                    let dict_loc = Self::with_end_span(loc, rbrace_span);
                    Ok(Expression::new(
                        ExpressionKind::Dict { keys, values },
                        dict_loc,
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
                            let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                            let dict_loc = Self::with_end_span(loc, rbrace_span);
                            return Ok(Expression::new(
                                ExpressionKind::DictComp {
                                    key: Box::new(first),
                                    value: Box::new(first_val),
                                    generators,
                                },
                                dict_loc,
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
                        let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                        let dict_loc = Self::with_end_span(loc, rbrace_span);
                        Ok(Expression::new(
                            ExpressionKind::Dict { keys, values },
                            dict_loc,
                        ))
                    } else {
                        // Set (including async comprehension)
                        if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                            let generators = self.parse_comp_for()?;
                            let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                            let set_loc = Self::with_end_span(loc, rbrace_span);
                            return Ok(Expression::new(
                                ExpressionKind::SetComp {
                                    elt: Box::new(first),
                                    generators,
                                },
                                set_loc,
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
                        let rbrace_span = self.expect(TokenKind::RightBrace)?.span;
                        let set_loc = Self::with_end_span(loc, rbrace_span);
                        Ok(Expression::new(ExpressionKind::Set { elts }, set_loc))
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
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&value));
            return Ok(Expression::new(
                ExpressionKind::YieldFrom {
                    value: Box::new(value),
                },
                loc,
            ));
        }

        // Check if there's a value after yield (not at end of statement/expression)
        if self.at_expression_start() {
            let value = self.parse_test_list_star_expr()?;
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&value));
            Ok(Expression::new(
                ExpressionKind::Yield {
                    value: Some(Box::new(value)),
                },
                loc,
            ))
        } else {
            Ok(Expression::new(ExpressionKind::Yield { value: None }, loc))
        }
    }

    /// Check if the current token could start an expression.
    fn at_expression_start(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Name(_)
                | TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::String(_)
                | TokenKind::Bytes(_)
                | TokenKind::FString(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::None
                | TokenKind::LeftParen
                | TokenKind::LeftBracket
                | TokenKind::LeftBrace
                | TokenKind::Minus
                | TokenKind::Plus
                | TokenKind::Tilde
                | TokenKind::Not
                | TokenKind::Lambda
                | TokenKind::Yield
                | TokenKind::Ellipsis
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
        let loc = Self::with_end_location(loc, Self::expression_outer_location(&body));
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
        let mut seen_default = false;

        loop {
            if self.check(TokenKind::Colon) {
                break;
            }

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
                    let location = self.current_location();
                    let name = self.expect_name()?;
                    args.vararg = Some(Arg {
                        arg: name,
                        annotation: None,
                        type_comment: None,
                        location,
                    });
                }
            } else if self.check(TokenKind::DoubleStar) {
                self.advance();
                let location = self.current_location();
                let name = self.expect_name()?;
                args.kwarg = Some(Arg {
                    arg: name,
                    annotation: None,
                    type_comment: None,
                    location,
                });
            } else {
                let location = self.current_location();
                let name = self.expect_name()?;
                let default = if self.check(TokenKind::Equal) {
                    self.advance();
                    Some(self.parse_test()?)
                } else {
                    None
                };
                if !seen_star {
                    if default.is_some() {
                        seen_default = true;
                    } else if seen_default {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidSyntax(
                                "non-default argument follows default argument".into(),
                            ),
                            Self::span_from_location(location),
                        ));
                    }
                }
                let arg = Arg {
                    arg: name,
                    annotation: None,
                    type_comment: None,
                    location,
                };
                if seen_star {
                    args.kwonlyargs.push(arg);
                    args.kw_defaults.push(default);
                } else {
                    args.args.push(arg);
                    if let Some(d) = default {
                        args.defaults.push(d);
                    }
                }
            }

            if !self.check(TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        Self::validate_unique_arguments(&args)?;
        Ok(args)
    }

    fn parse_subscript(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_subscript_element()?;

        // Multi-dimensional subscript: a[1:2, 3:4] → a[(slice(1,2), slice(3,4))]
        if self.check(TokenKind::Comma) {
            let loc = Self::expression_outer_location(&first);
            let mut end = Self::expression_outer_location(&first);
            let mut trailing_comma = None;
            let mut elements = vec![first];
            while self.check(TokenKind::Comma) {
                let comma_span = self.peek().span;
                self.advance();
                if self.check(TokenKind::RightBracket) {
                    trailing_comma = Some(comma_span);
                    break;
                }
                let element = self.parse_subscript_element()?;
                end = Self::expression_outer_location(&element);
                trailing_comma = None;
                elements.push(element);
            }
            let loc = trailing_comma
                .map(|span| Self::with_end_span(loc, span))
                .unwrap_or_else(|| Self::with_end_location(loc, end));
            return Ok(Expression::new(
                ExpressionKind::Tuple {
                    elts: elements,
                    ctx: ExprContext::Load,
                },
                loc,
            ));
        }
        Ok(first)
    }

    fn parse_subscript_element(&mut self) -> Result<Expression, ParseError> {
        let lower = if self.check(TokenKind::Colon) {
            None
        } else {
            Some(Box::new(self.parse_test()?))
        };
        if !self.check(TokenKind::Colon) {
            return Ok(*lower.unwrap());
        }
        let colon_span = self.expect(TokenKind::Colon)?.span;
        let mut loc = lower
            .as_ref()
            .map(|expr| Self::expression_outer_location(expr))
            .unwrap_or_else(|| Self::location_from_span(colon_span));
        loc = Self::with_end_span(loc, colon_span);
        let upper = if !self.check(TokenKind::Colon)
            && !self.check(TokenKind::RightBracket)
            && !self.check(TokenKind::Comma)
        {
            let upper = self.parse_test()?;
            loc = Self::with_end_location(loc, Self::expression_outer_location(&upper));
            Some(Box::new(upper))
        } else {
            None
        };
        let step = if self.check(TokenKind::Colon) {
            let colon_span = self.expect(TokenKind::Colon)?.span;
            loc = Self::with_end_span(loc, colon_span);
            if !self.check(TokenKind::RightBracket) && !self.check(TokenKind::Comma) {
                let step = self.parse_test()?;
                loc = Self::with_end_location(loc, Self::expression_outer_location(&step));
                Some(Box::new(step))
            } else {
                None
            }
        } else {
            None
        };
        Ok(Expression::new(
            ExpressionKind::Slice { lower, upper, step },
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
        let loc = Self::expression_outer_location(&first);
        let mut end = Self::expression_outer_location(&first);
        let mut trailing_comma = None;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            let comma_span = self.peek().span;
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                trailing_comma = Some(comma_span);
                break;
            }
            let elt = self.parse_test()?;
            end = Self::expression_outer_location(&elt);
            trailing_comma = None;
            elts.push(elt);
        }
        let loc = trailing_comma
            .map(|span| Self::with_end_span(loc, span))
            .unwrap_or_else(|| Self::with_end_location(loc, end));
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
        let loc = Self::expression_outer_location(&first);
        let mut end = Self::expression_outer_location(&first);
        let mut trailing_comma = None;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            let comma_span = self.peek().span;
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::Equal)
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                trailing_comma = Some(comma_span);
                break;
            }
            let elt = self.parse_test_or_star()?;
            end = Self::expression_outer_location(&elt);
            trailing_comma = None;
            elts.push(elt);
        }
        // Note: multiple starred expressions are only invalid in assignment targets,
        // NOT in expression context (PEP 448: [*a, *b] and (*a, *b) are valid).
        // The compiler's compile_store_target handles assignment-target validation.
        let loc = trailing_comma
            .map(|span| Self::with_end_span(loc, span))
            .unwrap_or_else(|| Self::with_end_location(loc, end));
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
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&expr));
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
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&expr));
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
        let loc = Self::expression_outer_location(&first);
        let mut end = Self::expression_outer_location(&first);
        let mut trailing_comma = None;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            let comma_span = self.peek().span;
            self.advance();
            if self.check(TokenKind::In) {
                trailing_comma = Some(comma_span);
                break;
            }
            let elt = self.parse_target()?;
            end = Self::expression_outer_location(&elt);
            trailing_comma = None;
            elts.push(elt);
        }
        let loc = trailing_comma
            .map(|span| Self::with_end_span(loc, span))
            .unwrap_or_else(|| Self::with_end_location(loc, end));
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Store,
            },
            loc,
        ))
    }
}
