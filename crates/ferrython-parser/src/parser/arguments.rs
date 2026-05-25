//! Argument parsing methods for the Parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::{Span, TokenKind};
use compact_str::CompactString;
use ferrython_ast::*;
use std::collections::HashSet;

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
        let mut seen_default = false;

        loop {
            if self.check(TokenKind::RightParen) || self.check(TokenKind::Colon) {
                break;
            }

            if self.check(TokenKind::Star) {
                self.advance();
                seen_star = true;
                if self.check(TokenKind::Comma)
                    || self.check(TokenKind::RightParen)
                    || self.check(TokenKind::Colon)
                {
                    // bare * separator
                } else {
                    let location = self.current_location();
                    let name = self.expect_name()?;
                    let annotation = self.try_parse_annotation()?;
                    let location = annotation
                        .as_ref()
                        .map(|ann| {
                            Self::with_end_location(location, Self::expression_outer_location(ann))
                        })
                        .unwrap_or(location);
                    args.vararg = Some(Arg {
                        arg: name,
                        annotation,
                        type_comment: None,
                        location,
                    });
                }
            } else if self.check(TokenKind::DoubleStar) {
                self.advance();
                let location = self.current_location();
                let name = self.expect_name()?;
                let annotation = self.try_parse_annotation()?;
                let location = annotation
                    .as_ref()
                    .map(|ann| {
                        Self::with_end_location(location, Self::expression_outer_location(ann))
                    })
                    .unwrap_or(location);
                args.kwarg = Some(Arg {
                    arg: name,
                    annotation,
                    type_comment: None,
                    location,
                });
            } else if self.check(TokenKind::Slash) {
                self.advance();
                _seen_slash = true;
                // Move all args collected so far to posonlyargs
                args.posonlyargs.extend(args.args.drain(..));
            } else {
                let location = self.current_location();
                let name = self.expect_name()?;
                let annotation = self.try_parse_annotation()?;
                let location = annotation
                    .as_ref()
                    .map(|ann| {
                        Self::with_end_location(location, Self::expression_outer_location(ann))
                    })
                    .unwrap_or(location);
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
                    annotation,
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

    pub(super) fn validate_unique_arguments(args: &Arguments) -> Result<(), ParseError> {
        let mut seen = Vec::new();
        for arg in args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.vararg.iter())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.iter())
        {
            let name = arg.arg.as_str();
            if seen.iter().any(|existing| *existing == name) {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidSyntax(format!(
                        "duplicate argument '{}' in function definition",
                        name
                    )),
                    Self::span_from_location(arg.location),
                ));
            }
            seen.push(name);
        }
        Ok(())
    }

    pub(super) fn span_from_location(location: SourceLocation) -> Span {
        Span::new(
            location.line,
            location.column,
            location.end_line.unwrap_or(location.line),
            location.end_column.unwrap_or(location.column),
        )
    }

    fn try_parse_annotation(&mut self) -> Result<Option<Box<Expression>>, ParseError> {
        if self.check(TokenKind::Colon) {
            // Only parse annotation if we're in a function parameter context
            // We need to distinguish from the colon that ends a function signature
            // For now, a simple heuristic: if after colon we see a name or type, parse it
            let saved = self.pos;
            self.advance();
            if self.check(TokenKind::RightParen)
                || self.check(TokenKind::Equal)
                || self.check(TokenKind::Comma)
            {
                self.pos = saved;
                return Ok(None);
            }
            Ok(Some(Box::new(self.parse_test()?)))
        } else {
            Ok(None)
        }
    }

    pub(super) fn parse_call_args(
        &mut self,
        open_location: SourceLocation,
    ) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
        let mut args = Vec::new();
        let mut keywords = Vec::new();
        let mut has_keyword = false;
        let mut has_kwarg_unpacking = false;
        let mut seen_keyword_names = HashSet::<CompactString>::new();

        if self.check(TokenKind::RightParen) {
            return Ok((args, keywords));
        }

        loop {
            if self.check(TokenKind::DoubleStar) {
                let star_span = self.peek().span;
                self.advance();
                let value = self.parse_test()?;
                let location = Self::with_end_location(
                    Self::location_from_span(star_span),
                    Self::expression_outer_location(&value),
                );
                keywords.push(Keyword {
                    arg: None,
                    value,
                    location,
                });
                has_keyword = true;
                has_kwarg_unpacking = true;
            } else if self.check(TokenKind::Star) {
                let span = self.peek().span;
                self.advance();
                let value = self.parse_test()?;
                if has_kwarg_unpacking {
                    return Err(ParseError::new(
                        ParseErrorKind::InvalidSyntax(
                            "iterable argument unpacking follows keyword argument unpacking".into(),
                        ),
                        span,
                    ));
                }
                let loc = Self::with_end_location(
                    Self::location_from_span(span),
                    Self::expression_outer_location(&value),
                );
                args.push(Expression::new(
                    ExpressionKind::Starred {
                        value: Box::new(value),
                        ctx: ExprContext::Load,
                    },
                    loc,
                ));
            } else {
                let expr = self.parse_test()?;
                // Check for generator expression: func(expr for x in iter)
                if self.check(TokenKind::For) && args.is_empty() && keywords.is_empty() {
                    let generators = self.parse_comp_for()?;
                    let loc = Self::with_end_location(
                        open_location,
                        generators
                            .last()
                            .and_then(|gen| {
                                gen.ifs
                                    .last()
                                    .map(Self::expression_outer_location)
                                    .or(Some(Self::expression_outer_location(&gen.iter)))
                            })
                            .unwrap_or_else(|| Self::expression_outer_location(&expr)),
                    );
                    args.push(Expression::new(
                        ExpressionKind::GeneratorExp {
                            elt: Box::new(expr),
                            generators,
                        },
                        loc,
                    ));
                    break; // generator expression is always the sole argument
                }
                // Check if this is a keyword argument: name=value
                if self.check(TokenKind::Equal) {
                    if let ExpressionKind::Name { id, .. } = &expr.node {
                        let name = id.clone();
                        self.advance();
                        let value = self.parse_test()?;
                        let location = Self::with_end_location(
                            Self::expression_outer_location(&expr),
                            Self::expression_outer_location(&value),
                        );
                        if !seen_keyword_names.insert(name.clone()) {
                            return Err(ParseError::new(
                                ParseErrorKind::SyntaxErrorMessage(format!(
                                    "keyword argument repeated: {}",
                                    name
                                )),
                                Span {
                                    start_line: expr.location.line,
                                    start_col: expr.location.column,
                                    end_line: expr.location.line,
                                    end_col: expr.location.column,
                                },
                            ));
                        }
                        keywords.push(Keyword {
                            arg: Some(name),
                            value,
                            location,
                        });
                        has_keyword = true;
                    } else {
                        args.push(expr);
                    }
                } else {
                    if has_kwarg_unpacking {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidSyntax(
                                "positional argument follows keyword argument unpacking".into(),
                            ),
                            crate::token::Span {
                                start_line: expr.location.line,
                                start_col: expr.location.column,
                                end_line: expr.location.line,
                                end_col: expr.location.column,
                            },
                        ));
                    }
                    if has_keyword {
                        return Err(ParseError::new(
                            ParseErrorKind::InvalidSyntax(
                                "positional argument follows keyword argument".into(),
                            ),
                            crate::token::Span {
                                start_line: expr.location.line,
                                start_col: expr.location.column,
                                end_line: expr.location.line,
                                end_col: expr.location.column,
                            },
                        ));
                    }
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

    pub(super) fn parse_class_args(
        &mut self,
        open_location: SourceLocation,
    ) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
        self.parse_call_args(open_location)
    }
}
