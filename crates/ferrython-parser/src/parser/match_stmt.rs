//! Match/case statement parsing methods for the Parser.

use crate::error::ParseError;
use crate::token::TokenKind;
use ferrython_ast::*;

use super::Parser;

impl Parser {
    fn check_soft_keyword(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Name(n) if n.as_str() == name)
    }

    pub(super) fn parse_match_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        if !self.check_soft_keyword("match") {
            return Err(self.unexpected_token("match"));
        }
        self.advance();

        let subject = self.parse_test_list_star_expr()?;

        self.expect(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect(TokenKind::Indent)?;

        let mut cases = Vec::new();
        while self.check_soft_keyword("case") {
            cases.push(self.parse_match_case()?);
            self.skip_newlines();
        }

        if cases.is_empty() {
            return Err(self.unexpected_token("case"));
        }

        if self.check(TokenKind::Dedent) {
            self.advance();
        }

        let end = cases
            .last()
            .and_then(|case| Self::last_statement_location(&case.body))
            .unwrap_or_else(|| Self::expression_outer_location(&subject));
        let loc = Self::with_end_location(loc, end);
        Ok(Statement::new(
            StatementKind::Match {
                subject: Box::new(subject),
                cases,
            },
            loc,
        ))
    }

    fn parse_match_case(&mut self) -> Result<MatchCase, ParseError> {
        if !self.check_soft_keyword("case") {
            return Err(self.unexpected_token("case"));
        }
        self.advance();

        let pattern = self.parse_pattern()?;

        let guard = if self.check(TokenKind::If) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;

        Ok(MatchCase {
            pattern,
            guard,
            body,
        })
    }

    /// Parse a pattern (top-level: handles OR patterns and AS patterns).
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let first = self.parse_closed_pattern()?;

        if self.check(TokenKind::As) {
            self.advance();
            let name = self.expect_name()?;
            return Ok(Pattern::MatchAs {
                pattern: Some(Box::new(first)),
                name: Some(name),
            });
        }

        if self.check(TokenKind::Pipe) {
            let mut patterns = vec![first];
            while self.check(TokenKind::Pipe) {
                self.advance();
                patterns.push(self.parse_closed_pattern()?);
            }
            if self.check(TokenKind::As) {
                self.advance();
                let name = self.expect_name()?;
                return Ok(Pattern::MatchAs {
                    pattern: Some(Box::new(Pattern::MatchOr { patterns })),
                    name: Some(name),
                });
            }
            return Ok(Pattern::MatchOr { patterns });
        }

        Ok(first)
    }

    /// Parse a "closed" pattern (no top-level OR or AS).
    fn parse_closed_pattern(&mut self) -> Result<Pattern, ParseError> {
        match &self.peek().kind {
            TokenKind::Name(n) if n.as_str() == "_" => {
                self.advance();
                Ok(Pattern::MatchWildcard)
            }
            TokenKind::None => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::None, loc),
                })
            }
            TokenKind::True => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::Bool(true), loc),
                })
            }
            TokenKind::False => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::Bool(false), loc),
                })
            }
            TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Complex(_) => {
                self.parse_literal_pattern()
            }
            TokenKind::Minus => {
                let loc = self.current_location();
                self.advance();
                let lit_pat = self.parse_literal_pattern()?;
                match lit_pat {
                    Pattern::MatchLiteral { value } => {
                        let loc =
                            Self::with_end_location(loc, Self::expression_outer_location(&value));
                        Ok(Pattern::MatchLiteral {
                            value: Expression::new(
                                ExpressionKind::UnaryOp {
                                    op: UnaryOperator::USub,
                                    operand: Box::new(value),
                                },
                                loc,
                            ),
                        })
                    }
                    _ => Err(self.unexpected_token("numeric literal")),
                }
            }
            TokenKind::String(_) | TokenKind::Bytes(_) | TokenKind::FString(_) => {
                let value = self.parse_atom()?;
                Ok(Pattern::MatchLiteral { value })
            }
            TokenKind::LeftBracket => {
                self.advance();
                let patterns = self.parse_pattern_list(TokenKind::RightBracket)?;
                self.expect(TokenKind::RightBracket)?;
                Ok(Pattern::MatchSequence { patterns })
            }
            TokenKind::LeftParen => {
                self.advance();
                if self.check(TokenKind::RightParen) {
                    self.advance();
                    return Ok(Pattern::MatchSequence { patterns: vec![] });
                }
                let first = self.parse_pattern()?;
                if self.check(TokenKind::Comma) {
                    let mut patterns = vec![first];
                    while self.check(TokenKind::Comma) {
                        self.advance();
                        if self.check(TokenKind::RightParen) {
                            break;
                        }
                        patterns.push(self.parse_pattern()?);
                    }
                    self.expect(TokenKind::RightParen)?;
                    Ok(Pattern::MatchSequence { patterns })
                } else {
                    self.expect(TokenKind::RightParen)?;
                    Ok(first)
                }
            }
            TokenKind::LeftBrace => self.parse_mapping_pattern(),
            TokenKind::Star => {
                self.advance();
                if self.check_soft_keyword("_") {
                    self.advance();
                    Ok(Pattern::MatchStar { name: None })
                } else {
                    let name = self.expect_name()?;
                    Ok(Pattern::MatchStar { name: Some(name) })
                }
            }
            TokenKind::Name(_) => self.parse_name_or_class_pattern(),
            _ => Err(self.unexpected_token("pattern")),
        }
    }

    fn parse_literal_pattern(&mut self) -> Result<Pattern, ParseError> {
        let loc = self.current_location();
        let value = match &self.peek().kind {
            TokenKind::Int(n) => {
                let n = n.clone();
                self.advance();
                Expression::constant(Constant::Int(n), loc)
            }
            TokenKind::Float(f) => {
                let f = *f;
                self.advance();
                Expression::constant(Constant::Float(f), loc)
            }
            TokenKind::Complex(c) => {
                let c = *c;
                self.advance();
                Expression::constant(Constant::Complex { real: 0.0, imag: c }, loc)
            }
            _ => return Err(self.unexpected_token("numeric literal")),
        };
        Ok(Pattern::MatchLiteral { value })
    }

    fn parse_name_or_class_pattern(&mut self) -> Result<Pattern, ParseError> {
        let loc = self.current_location();
        let name = self.expect_name()?;

        if self.check(TokenKind::Dot) {
            let mut expr = Expression::name(name, ExprContext::Load, loc);
            while self.check(TokenKind::Dot) {
                self.advance();
                let attr_span = self.peek().span;
                let attr = self.expect_name()?;
                let attr_loc =
                    Self::with_end_span(Self::expression_outer_location(&expr), attr_span);
                expr = Expression::new(
                    ExpressionKind::Attribute {
                        value: Box::new(expr),
                        attr,
                        ctx: ExprContext::Load,
                    },
                    attr_loc,
                );
            }
            if self.check(TokenKind::LeftParen) {
                return self.parse_class_pattern_args(expr);
            }
            return Ok(Pattern::MatchValue { value: expr });
        }

        if self.check(TokenKind::LeftParen) {
            let cls_expr = Expression::name(name, ExprContext::Load, loc);
            return self.parse_class_pattern_args(cls_expr);
        }

        Ok(Pattern::MatchCapture { name })
    }

    fn parse_class_pattern_args(&mut self, cls: Expression) -> Result<Pattern, ParseError> {
        self.expect(TokenKind::LeftParen)?;
        let mut patterns = Vec::new();
        let mut kwd_attrs = Vec::new();
        let mut kwd_patterns = Vec::new();
        let mut seen_keyword = false;

        while !self.check(TokenKind::RightParen) && !self.is_at_end() {
            if !patterns.is_empty() || !kwd_attrs.is_empty() {
                self.expect(TokenKind::Comma)?;
                if self.check(TokenKind::RightParen) {
                    break;
                }
            }
            let saved = self.pos;
            if let TokenKind::Name(n) = &self.peek().kind {
                let n = n.clone();
                self.advance();
                if self.check(TokenKind::Equal) {
                    self.advance();
                    let pat = self.parse_pattern()?;
                    kwd_attrs.push(n);
                    kwd_patterns.push(pat);
                    seen_keyword = true;
                    continue;
                }
                self.pos = saved;
            }
            if seen_keyword {
                return Err(self.unexpected_token("keyword pattern"));
            }
            patterns.push(self.parse_pattern()?);
        }
        self.expect(TokenKind::RightParen)?;

        Ok(Pattern::MatchClass {
            cls,
            patterns,
            kwd_attrs,
            kwd_patterns,
        })
    }

    fn parse_mapping_pattern(&mut self) -> Result<Pattern, ParseError> {
        self.expect(TokenKind::LeftBrace)?;
        let mut keys = Vec::new();
        let mut patterns = Vec::new();
        let mut rest = None;

        while !self.check(TokenKind::RightBrace) && !self.is_at_end() {
            if !keys.is_empty() || rest.is_some() {
                self.expect(TokenKind::Comma)?;
                if self.check(TokenKind::RightBrace) {
                    break;
                }
            }
            if self.check(TokenKind::DoubleStar) {
                self.advance();
                rest = Some(self.expect_name()?);
                continue;
            }
            let key = self.parse_expr()?;
            self.expect(TokenKind::Colon)?;
            let pat = self.parse_pattern()?;
            keys.push(key);
            patterns.push(pat);
        }
        self.expect(TokenKind::RightBrace)?;

        Ok(Pattern::MatchMapping {
            keys,
            patterns,
            rest,
        })
    }

    fn parse_pattern_list(&mut self, end: TokenKind) -> Result<Vec<Pattern>, ParseError> {
        let mut patterns = Vec::new();
        while !self.check(end.clone()) && !self.is_at_end() {
            if !patterns.is_empty() {
                self.expect(TokenKind::Comma)?;
                if self.check(end.clone()) {
                    break;
                }
            }
            patterns.push(self.parse_pattern()?);
        }
        Ok(patterns)
    }
}
