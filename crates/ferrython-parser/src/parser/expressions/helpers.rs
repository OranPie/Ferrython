//! Shared expression-list, target-list, and comprehension parsing helpers.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::TokenKind;
use ferrython_ast::*;

use super::super::Parser;

impl Parser {
    // ─── Comprehension parsing ──────────────────────────────────────

    pub(in crate::parser) fn parse_comp_for(&mut self) -> Result<Vec<Comprehension>, ParseError> {
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
            if Self::is_unparenthesized_named_expr(&iter) {
                return Err(Self::invalid_unparenthesized_named_expr(&iter));
            }
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

    pub(in crate::parser) fn is_unparenthesized_named_expr(expr: &Expression) -> bool {
        matches!(&expr.node, ExpressionKind::NamedExpr { .. })
            && expr.location == expr.outer_location
    }

    pub(in crate::parser) fn invalid_unparenthesized_named_expr(expr: &Expression) -> ParseError {
        ParseError::new(
            ParseErrorKind::InvalidSyntax("invalid syntax".into()),
            Self::span_from_location(expr.location),
        )
    }

    pub(in crate::parser) fn parse_test_list(&mut self) -> Result<Expression, ParseError> {
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

    pub(in crate::parser) fn parse_test_list_star_expr(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let starts_with_yield = self.check(TokenKind::Yield);
        let first = self.parse_test_or_star()?;
        if !self.check(TokenKind::Comma) {
            return Ok(first);
        }
        if starts_with_yield {
            return Err(ParseError::new(
                ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                self.peek().span,
            ));
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
            if self.check(TokenKind::Yield) {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                    self.peek().span,
                ));
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

    pub(super) fn parse_test_or_star(&mut self) -> Result<Expression, ParseError> {
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

    pub(in crate::parser) fn parse_target(&mut self) -> Result<Expression, ParseError> {
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

    pub(in crate::parser) fn parse_target_list(&mut self) -> Result<Expression, ParseError> {
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
