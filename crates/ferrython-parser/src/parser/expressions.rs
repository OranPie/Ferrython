//! Expression parsing methods for the Parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::TokenKind;
use compact_str::CompactString;
use ferrython_ast::*;

use super::Parser;

mod fstring;
mod helpers;
mod lambda;
mod subscript;

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
                    self.named_expr_rhs_depth += 1;
                    let value = self.parse_test()?;
                    self.named_expr_rhs_depth -= 1;
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
                let parenthesized_yield = self.check(TokenKind::Yield);
                let mut expr = self.parse_test_list_star_expr()?;
                if self.check(TokenKind::ColonEqual) {
                    let message = match &expr.node {
                        ExpressionKind::Tuple { .. } => {
                            "cannot use assignment expressions with tuple"
                        }
                        ExpressionKind::List { .. } => {
                            "cannot use assignment expressions with list"
                        }
                        _ => "cannot use assignment expressions with expression",
                    };
                    return Err(ParseError::new(
                        ParseErrorKind::SyntaxErrorMessage(message.into()),
                        Self::span_from_location(Self::expression_outer_location(&expr)),
                    ));
                }
                if parenthesized_yield && self.check(TokenKind::Comma) {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                        self.peek().span,
                    ));
                }
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
                if self.check(TokenKind::For) || self.check(TokenKind::Async) {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                        self.peek().span,
                    ));
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
}
