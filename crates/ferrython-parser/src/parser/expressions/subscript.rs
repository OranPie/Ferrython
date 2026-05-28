//! Subscript and slice expression parsing.

use crate::error::ParseError;
use crate::token::TokenKind;
use ferrython_ast::*;

use super::super::Parser;

impl Parser {
    pub(super) fn parse_subscript(&mut self) -> Result<Expression, ParseError> {
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
        } else if self.check(TokenKind::Ellipsis) {
            Some(Box::new(self.parse_atom()?))
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
}
