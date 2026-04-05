//! Argument parsing methods for the Parser.

use crate::error::ParseError;
use crate::token::TokenKind;
use ferrython_ast::*;

use super::Parser;

impl Parser {
    // ─── Argument parsing ───────────────────────────────────────────

    pub(super) fn parse_parameters(&mut self) -> Result<Arguments, ParseError> {
        let mut args = Arguments::empty();
        if self.check(TokenKind::RightParen) || self.check(TokenKind::Colon) {
            return Ok(args);
        }

        let mut seen_star = false;
        let mut _seen_slash = false;

        loop {
            if self.check(TokenKind::RightParen) || self.check(TokenKind::Colon) {
                break;
            }

            if self.check(TokenKind::Star) {
                self.advance();
                seen_star = true;
                if self.check(TokenKind::Comma) || self.check(TokenKind::RightParen) || self.check(TokenKind::Colon) {
                    // bare * separator
                } else {
                    let name = self.expect_name()?;
                    let annotation = self.try_parse_annotation()?;
                    args.vararg = Some(Arg {
                        arg: name,
                        annotation,
                        type_comment: None,
                        location: self.current_location(),
                    });
                }
            } else if self.check(TokenKind::DoubleStar) {
                self.advance();
                let name = self.expect_name()?;
                let annotation = self.try_parse_annotation()?;
                args.kwarg = Some(Arg {
                    arg: name,
                    annotation,
                    type_comment: None,
                    location: self.current_location(),
                });
            } else if self.check(TokenKind::Slash) {
                self.advance();
                _seen_slash = true;
                // Move all args collected so far to posonlyargs
                args.posonlyargs.extend(args.args.drain(..));
            } else {
                let name = self.expect_name()?;
                let annotation = self.try_parse_annotation()?;
                let default = if self.check(TokenKind::Equal) {
                    self.advance();
                    Some(self.parse_test()?)
                } else {
                    None
                };
                let arg = Arg {
                    arg: name,
                    annotation,
                    type_comment: None,
                    location: self.current_location(),
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

        Ok(args)
    }

    fn try_parse_annotation(&mut self) -> Result<Option<Box<Expression>>, ParseError> {
        if self.check(TokenKind::Colon) {
            // Only parse annotation if we're in a function parameter context
            // We need to distinguish from the colon that ends a function signature
            // For now, a simple heuristic: if after colon we see a name or type, parse it
            let saved = self.pos;
            self.advance();
            if self.check(TokenKind::RightParen) || self.check(TokenKind::Equal) || self.check(TokenKind::Comma) {
                self.pos = saved;
                return Ok(None);
            }
            Ok(Some(Box::new(self.parse_test()?)))
        } else {
            Ok(None)
        }
    }

    pub(super) fn parse_call_args(&mut self) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
        let mut args = Vec::new();
        let mut keywords = Vec::new();

        if self.check(TokenKind::RightParen) {
            return Ok((args, keywords));
        }

        loop {
            if self.check(TokenKind::DoubleStar) {
                self.advance();
                let value = self.parse_test()?;
                keywords.push(Keyword {
                    arg: None,
                    value,
                    location: self.current_location(),
                });
            } else if self.check(TokenKind::Star) {
                self.advance();
                let value = self.parse_test()?;
                args.push(Expression::new(
                    ExpressionKind::Starred {
                        value: Box::new(value),
                        ctx: ExprContext::Load,
                    },
                    self.current_location(),
                ));
            } else {
                let expr = self.parse_test()?;
                // Check for generator expression: func(expr for x in iter)
                if self.check(TokenKind::For) && args.is_empty() && keywords.is_empty() {
                    let generators = self.parse_comp_for()?;
                    args.push(Expression::new(
                        ExpressionKind::GeneratorExp {
                            elt: Box::new(expr),
                            generators,
                        },
                        self.current_location(),
                    ));
                    break; // generator expression is always the sole argument
                }
                // Check if this is a keyword argument: name=value
                if self.check(TokenKind::Equal) {
                    if let ExpressionKind::Name { id, .. } = &expr.node {
                        let name = id.clone();
                        self.advance();
                        let value = self.parse_test()?;
                        keywords.push(Keyword {
                            arg: Some(name),
                            value,
                            location: expr.location,
                        });
                    } else {
                        args.push(expr);
                    }
                } else {
                    args.push(expr);
                }
            }

            if !self.check(TokenKind::Comma) {
                break;
            }
            self.advance();
            if self.check(TokenKind::RightParen) {
                break;
            }
        }

        Ok((args, keywords))
    }

    pub(super) fn parse_class_args(&mut self) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
        self.parse_call_args()
    }
}
