//! Lambda expression parsing.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::TokenKind;
use ferrython_ast::*;

use super::super::Parser;

impl Parser {
    pub(super) fn parse_lambda(&mut self) -> Result<Expression, ParseError> {
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
}
