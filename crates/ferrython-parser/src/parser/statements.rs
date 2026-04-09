//! Statement parsing methods for the Parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::TokenKind;
use compact_str::CompactString;
use ferrython_ast::*;

use super::Parser;

impl Parser {
    // ─── Statement parsing ──────────────────────────────────────────

    pub(super) fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();

        match &self.peek().kind {
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::For => self.parse_for_stmt(false),
            TokenKind::Def => self.parse_function_def(false),
            TokenKind::Async => self.parse_async_stmt(),
            TokenKind::Class => self.parse_class_def(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Pass => { self.advance(); self.expect_newline()?; Ok(Statement::new(StatementKind::Pass, loc)) }
            TokenKind::Break => { self.advance(); self.expect_newline()?; Ok(Statement::new(StatementKind::Break, loc)) }
            TokenKind::Continue => { self.advance(); self.expect_newline()?; Ok(Statement::new(StatementKind::Continue, loc)) }
            TokenKind::Import => self.parse_import_stmt(),
            TokenKind::From => self.parse_from_import_stmt(),
            TokenKind::Raise => self.parse_raise_stmt(),
            TokenKind::Try => self.parse_try_stmt(),
            TokenKind::With => self.parse_with_stmt(false),
            TokenKind::Assert => self.parse_assert_stmt(),
            TokenKind::Del => self.parse_del_stmt(),
            TokenKind::Global => self.parse_global_stmt(),
            TokenKind::Nonlocal => self.parse_nonlocal_stmt(),
            TokenKind::At => self.parse_decorated(),
            TokenKind::Name(ref name) if name.as_str() == "match" => {
                // Soft keyword: try match/case first, fall back to expression
                let saved = self.pos;
                match self.parse_match_stmt() {
                    Ok(stmt) => Ok(stmt),
                    Err(_) => {
                        self.pos = saved;
                        self.parse_expression_or_assignment()
                    }
                }
            }
            _ => self.parse_expression_or_assignment(),
        }
    }

    fn parse_expression_or_assignment(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        let expr = self.parse_test_list_star_expr()?;

        // Check for augmented assignment
        if let Some(op) = self.try_parse_aug_assign_op() {
            let value = self.parse_test_list()?;
            self.expect_newline()?;
            return Ok(Statement::new(
                StatementKind::AugAssign {
                    target: Box::new(expr),
                    op,
                    value: Box::new(value),
                },
                loc,
            ));
        }

        // Check for annotation
        if self.check(TokenKind::Colon) {
            self.advance();
            let annotation = self.parse_expr()?;
            let value = if self.check(TokenKind::Equal) {
                self.advance();
                Some(Box::new(self.parse_test_list()?))
            } else {
                None
            };
            self.expect_newline()?;
            return Ok(Statement::new(
                StatementKind::AnnAssign {
                    target: Box::new(expr),
                    annotation: Box::new(annotation),
                    value,
                    simple: true,
                },
                loc,
            ));
        }

        // Check for assignment (=)
        if self.check(TokenKind::Equal) {
            let mut targets = vec![expr];
            while self.check(TokenKind::Equal) {
                self.advance();
                let next = self.parse_test_list_star_expr()?;
                targets.push(next);
            }
            let value = targets.pop().unwrap();
            self.expect_newline()?;
            return Ok(Statement::new(
                StatementKind::Assign {
                    targets,
                    value: Box::new(value),
                    type_comment: None,
                },
                loc,
            ));
        }

        // Expression statement
        self.expect_newline()?;
        Ok(Statement::new(
            StatementKind::Expr {
                value: Box::new(expr),
            },
            loc,
        ))
    }

    fn parse_if_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::If)?;
        let test = self.parse_expr()?;
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;

        let mut orelse = Vec::new();
        if self.check(TokenKind::Elif) {
            orelse.push(self.parse_elif_stmt()?);
        } else if self.check(TokenKind::Else) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            orelse = self.parse_block()?;
        }

        Ok(Statement::new(
            StatementKind::If {
                test: Box::new(test),
                body,
                orelse,
            },
            loc,
        ))
    }

    fn parse_elif_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Elif)?;
        let test = self.parse_expr()?;
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;

        let mut orelse = Vec::new();
        if self.check(TokenKind::Elif) {
            orelse.push(self.parse_elif_stmt()?);
        } else if self.check(TokenKind::Else) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            orelse = self.parse_block()?;
        }

        Ok(Statement::new(
            StatementKind::If {
                test: Box::new(test),
                body,
                orelse,
            },
            loc,
        ))
    }

    fn parse_while_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::While)?;
        let test = self.parse_expr()?;
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        let orelse = if self.check(TokenKind::Else) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            self.parse_block()?
        } else {
            Vec::new()
        };
        Ok(Statement::new(
            StatementKind::While {
                test: Box::new(test),
                body,
                orelse,
            },
            loc,
        ))
    }

    fn parse_for_stmt(&mut self, is_async: bool) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::For)?;
        let target = self.parse_target_list()?;
        self.expect(TokenKind::In)?;
        let iter = self.parse_test_list()?;
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        let orelse = if self.check(TokenKind::Else) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            self.parse_block()?
        } else {
            Vec::new()
        };
        Ok(Statement::new(
            StatementKind::For {
                target: Box::new(target),
                iter: Box::new(iter),
                body,
                orelse,
                type_comment: None,
                is_async,
            },
            loc,
        ))
    }

    pub(super) fn parse_function_def(&mut self, is_async: bool) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Def)?;
        let name = self.expect_name()?;
        self.expect(TokenKind::LeftParen)?;
        let args = self.parse_parameters()?;
        self.expect(TokenKind::RightParen)?;
        let returns = if self.check(TokenKind::Arrow) {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Statement::new(
            StatementKind::FunctionDef {
                name,
                args: Box::new(args),
                body,
                decorator_list: Vec::new(),
                returns,
                type_comment: None,
                is_async,
            },
            loc,
        ))
    }

    pub(super) fn parse_class_def(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Class)?;
        let name = self.expect_name()?;
        let (bases, keywords) = if self.check(TokenKind::LeftParen) {
            self.advance();
            let (b, k) = self.parse_class_args()?;
            self.expect(TokenKind::RightParen)?;
            (b, k)
        } else {
            (Vec::new(), Vec::new())
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Statement::new(
            StatementKind::ClassDef {
                name,
                bases,
                keywords,
                body,
                decorator_list: Vec::new(),
            },
            loc,
        ))
    }

    fn parse_return_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Return)?;
        let value = if !self.check_newline_or_eof() {
            Some(Box::new(self.parse_test_list_star_expr()?))
        } else {
            None
        };
        self.expect_newline()?;
        Ok(Statement::new(StatementKind::Return { value }, loc))
    }

    fn parse_raise_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Raise)?;
        if self.check_newline_or_eof() {
            self.expect_newline()?;
            return Ok(Statement::new(
                StatementKind::Raise {
                    exc: None,
                    cause: None,
                },
                loc,
            ));
        }
        let exc = self.parse_expr()?;
        let cause = if self.check(TokenKind::From) {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect_newline()?;
        Ok(Statement::new(
            StatementKind::Raise {
                exc: Some(Box::new(exc)),
                cause,
            },
            loc,
        ))
    }

    fn parse_import_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Import)?;
        let mut names = vec![self.parse_dotted_as_name()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            names.push(self.parse_dotted_as_name()?);
        }
        self.expect_newline()?;
        Ok(Statement::new(StatementKind::Import { names }, loc))
    }

    fn parse_from_import_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::From)?;
        let mut level = 0u32;
        while self.check(TokenKind::Dot) {
            self.advance();
            level += 1;
        }
        let module = if !self.check(TokenKind::Import) {
            Some(self.parse_dotted_name()?)
        } else {
            None
        };
        self.expect(TokenKind::Import)?;
        let names = if self.check(TokenKind::Star) {
            self.advance();
            vec![Alias {
                name: CompactString::from("*"),
                asname: None,
                location: self.current_location(),
            }]
        } else {
            let open_paren = self.check(TokenKind::LeftParen);
            if open_paren {
                self.advance();
            }
            let mut names = vec![self.parse_import_as_name()?];
            while self.check(TokenKind::Comma) {
                self.advance();
                if open_paren && self.check(TokenKind::RightParen) {
                    break;
                }
                names.push(self.parse_import_as_name()?);
            }
            if open_paren {
                self.expect(TokenKind::RightParen)?;
            }
            names
        };
        self.expect_newline()?;
        Ok(Statement::new(
            StatementKind::ImportFrom {
                module,
                names,
                level,
            },
            loc,
        ))
    }

    fn parse_try_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Try)?;
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        let mut handlers = Vec::new();
        while self.check(TokenKind::Except) {
            handlers.push(self.parse_except_handler()?);
        }
        let orelse = if self.check(TokenKind::Else) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            self.parse_block()?
        } else {
            Vec::new()
        };
        let finalbody = if self.check(TokenKind::Finally) {
            self.advance();
            self.expect(TokenKind::Colon)?;
            self.parse_block()?
        } else {
            Vec::new()
        };
        Ok(Statement::new(
            StatementKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            },
            loc,
        ))
    }

    fn parse_except_handler(&mut self) -> Result<ExceptHandler, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Except)?;
        let (typ, name) = if !self.check(TokenKind::Colon) {
            let t = self.parse_expr()?;
            let n = if self.check(TokenKind::As) {
                self.advance();
                Some(self.expect_name()?)
            } else {
                None
            };
            (Some(Box::new(t)), n)
        } else {
            (None, None)
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(ExceptHandler {
            typ,
            name,
            body,
            location: loc,
        })
    }

    fn parse_with_stmt(&mut self, is_async: bool) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::With)?;
        let mut items = vec![self.parse_with_item()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            items.push(self.parse_with_item()?);
        }
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        Ok(Statement::new(
            StatementKind::With {
                items,
                body,
                type_comment: None,
                is_async,
            },
            loc,
        ))
    }

    fn parse_with_item(&mut self) -> Result<WithItem, ParseError> {
        let context_expr = self.parse_expr()?;
        let optional_vars = if self.check(TokenKind::As) {
            self.advance();
            Some(Box::new(self.parse_target()?))
        } else {
            None
        };
        Ok(WithItem {
            context_expr,
            optional_vars,
        })
    }

    fn parse_assert_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Assert)?;
        let test = self.parse_expr()?;
        let msg = if self.check(TokenKind::Comma) {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect_newline()?;
        Ok(Statement::new(
            StatementKind::Assert {
                test: Box::new(test),
                msg,
            },
            loc,
        ))
    }

    fn parse_del_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Del)?;
        let mut targets = vec![self.parse_expr()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check_newline_or_eof() {
                break;
            }
            targets.push(self.parse_expr()?);
        }
        self.expect_newline()?;
        Ok(Statement::new(StatementKind::Delete { targets }, loc))
    }

    fn parse_global_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Global)?;
        let mut names = vec![self.expect_name()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            names.push(self.expect_name()?);
        }
        self.expect_newline()?;
        Ok(Statement::new(StatementKind::Global { names }, loc))
    }

    fn parse_nonlocal_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Nonlocal)?;
        let mut names = vec![self.expect_name()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            names.push(self.expect_name()?);
        }
        self.expect_newline()?;
        Ok(Statement::new(StatementKind::Nonlocal { names }, loc))
    }

    fn parse_async_stmt(&mut self) -> Result<Statement, ParseError> {
        self.expect(TokenKind::Async)?;
        match &self.peek().kind {
            TokenKind::Def => self.parse_function_def(true),
            TokenKind::For => self.parse_for_stmt(true),
            TokenKind::With => self.parse_with_stmt(true),
            _ => Err(ParseError::new(
                ParseErrorKind::InvalidSyntax("expected 'def', 'for', or 'with' after 'async'".into()),
                self.peek().span,
            )),
        }
    }

    fn parse_decorated(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        let mut decorators = Vec::new();
        while self.check(TokenKind::At) {
            self.advance();
            decorators.push(self.parse_expr()?);
            self.expect_newline()?;
        }
        let mut stmt = match &self.peek().kind {
            TokenKind::Def => self.parse_function_def(false)?,
            TokenKind::Async => {
                self.advance();
                self.parse_function_def(true)?
            }
            TokenKind::Class => self.parse_class_def()?,
            _ => {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidSyntax(
                        "expected function or class definition after decorator".into(),
                    ),
                    self.peek().span,
                ));
            }
        };
        // Attach decorators
        match &mut stmt.node {
            StatementKind::FunctionDef {
                decorator_list, ..
            }
            | StatementKind::ClassDef {
                decorator_list, ..
            } => {
                *decorator_list = decorators;
            }
            _ => unreachable!(),
        }
        stmt.location = loc;
        Ok(stmt)
    }

    // ─── Import helpers ─────────────────────────────────────────────

    fn parse_dotted_name(&mut self) -> Result<CompactString, ParseError> {
        let mut name = self.expect_name()?.to_string();
        while self.check(TokenKind::Dot) {
            self.advance();
            name.push('.');
            name.push_str(self.expect_name()?.as_str());
        }
        Ok(CompactString::from(name))
    }

    fn parse_dotted_as_name(&mut self) -> Result<Alias, ParseError> {
        let loc = self.current_location();
        let name = self.parse_dotted_name()?;
        let asname = if self.check(TokenKind::As) {
            self.advance();
            Some(self.expect_name()?)
        } else {
            None
        };
        Ok(Alias {
            name,
            asname,
            location: loc,
        })
    }

    fn parse_import_as_name(&mut self) -> Result<Alias, ParseError> {
        let loc = self.current_location();
        let name = self.expect_name()?;
        let asname = if self.check(TokenKind::As) {
            self.advance();
            Some(self.expect_name()?)
        } else {
            None
        };
        Ok(Alias {
            name,
            asname,
            location: loc,
        })
    }

    fn try_parse_aug_assign_op(&mut self) -> Option<Operator> {
        let op = match &self.peek().kind {
            TokenKind::PlusEqual => Some(Operator::Add),
            TokenKind::MinusEqual => Some(Operator::Sub),
            TokenKind::StarEqual => Some(Operator::Mult),
            TokenKind::SlashEqual => Some(Operator::Div),
            TokenKind::DoubleSlashEqual => Some(Operator::FloorDiv),
            TokenKind::PercentEqual => Some(Operator::Mod),
            TokenKind::DoubleStarEqual => Some(Operator::Pow),
            TokenKind::LeftShiftEqual => Some(Operator::LShift),
            TokenKind::RightShiftEqual => Some(Operator::RShift),
            TokenKind::AmpersandEqual => Some(Operator::BitAnd),
            TokenKind::PipeEqual => Some(Operator::BitOr),
            TokenKind::CaretEqual => Some(Operator::BitXor),
            TokenKind::AtEqual => Some(Operator::MatMult),
            _ => None,
        };
        if op.is_some() {
            self.advance();
        }
        op
    }

    // ─── Match/case statement (Python 3.10+) ────────────────────────

    fn check_soft_keyword(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Name(n) if n.as_str() == name)
    }

    fn parse_match_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        // Consume `match` soft keyword
        if !self.check_soft_keyword("match") {
            return Err(self.unexpected_token("match"));
        }
        self.advance();

        // Parse subject expression (must not be empty)
        let subject = self.parse_test_list_star_expr()?;

        // Expect `:` after subject
        self.expect(TokenKind::Colon)?;
        // Expect newline then indent (block of cases)
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

        Ok(Statement::new(
            StatementKind::Match {
                subject: Box::new(subject),
                cases,
            },
            loc,
        ))
    }

    fn parse_match_case(&mut self) -> Result<MatchCase, ParseError> {
        // Consume `case` soft keyword
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

        // Check for `as` binding
        if self.check(TokenKind::As) {
            self.advance();
            let name = self.expect_name()?;
            return Ok(Pattern::MatchAs {
                pattern: Some(Box::new(first)),
                name: Some(name),
            });
        }

        // Check for OR pattern: `pat1 | pat2 | ...`
        if self.check(TokenKind::Pipe) {
            let mut patterns = vec![first];
            while self.check(TokenKind::Pipe) {
                self.advance();
                patterns.push(self.parse_closed_pattern()?);
            }
            // Check for trailing `as` on the whole OR pattern
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
            // Wildcard `_`
            TokenKind::Name(n) if n.as_str() == "_" => {
                self.advance();
                Ok(Pattern::MatchWildcard)
            }
            // `None` literal
            TokenKind::None => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::None, loc),
                })
            }
            // `True` literal
            TokenKind::True => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::Bool(true), loc),
                })
            }
            // `False` literal
            TokenKind::False => {
                let loc = self.current_location();
                self.advance();
                Ok(Pattern::MatchLiteral {
                    value: Expression::constant(Constant::Bool(false), loc),
                })
            }
            // Numeric literals (possibly negated)
            TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Complex(_) => {
                self.parse_literal_pattern()
            }
            TokenKind::Minus => {
                // Negative number literal
                let loc = self.current_location();
                self.advance();
                let lit_pat = self.parse_literal_pattern()?;
                match lit_pat {
                    Pattern::MatchLiteral { value } => {
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
            // String literal
            TokenKind::String(_) | TokenKind::Bytes(_) | TokenKind::FString(_) => {
                let loc = self.current_location();
                let value = self.parse_atom()?;
                Ok(Pattern::MatchLiteral { value })
            }
            // Sequence pattern with brackets `[p1, p2, ...]`
            TokenKind::LeftBracket => {
                self.advance();
                let patterns = self.parse_pattern_list(TokenKind::RightBracket)?;
                self.expect(TokenKind::RightBracket)?;
                Ok(Pattern::MatchSequence { patterns })
            }
            // Tuple-like / group pattern with parens `(p1, p2, ...)`
            TokenKind::LeftParen => {
                self.advance();
                if self.check(TokenKind::RightParen) {
                    self.advance();
                    return Ok(Pattern::MatchSequence { patterns: vec![] });
                }
                let first = self.parse_pattern()?;
                if self.check(TokenKind::Comma) {
                    // Sequence pattern
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
                    // Group pattern (single pattern in parens)
                    self.expect(TokenKind::RightParen)?;
                    Ok(first)
                }
            }
            // Mapping pattern `{key: pat, ...}`
            TokenKind::LeftBrace => {
                self.parse_mapping_pattern()
            }
            // Star pattern `*name` or `*_`
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
            // Name: could be capture, value (dotted), or class pattern
            TokenKind::Name(_) => {
                self.parse_name_or_class_pattern()
            }
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

        // Dotted name: `module.attr` or `module.attr.subattr`
        if self.check(TokenKind::Dot) {
            let mut expr = Expression::name(name, ExprContext::Load, loc);
            while self.check(TokenKind::Dot) {
                self.advance();
                let attr = self.expect_name()?;
                let attr_loc = self.current_location();
                expr = Expression::new(
                    ExpressionKind::Attribute {
                        value: Box::new(expr),
                        attr,
                        ctx: ExprContext::Load,
                    },
                    attr_loc,
                );
            }
            // Class pattern with args: `ClassName(p1, kw=p2)`
            if self.check(TokenKind::LeftParen) {
                return self.parse_class_pattern_args(expr);
            }
            return Ok(Pattern::MatchValue { value: expr });
        }

        // Class pattern: `ClassName(p1, kw=p2)`
        if self.check(TokenKind::LeftParen) {
            let cls_expr = Expression::name(name, ExprContext::Load, loc);
            return self.parse_class_pattern_args(cls_expr);
        }

        // Simple capture name
        Ok(Pattern::MatchCapture { name })
    }

    fn parse_class_pattern_args(
        &mut self,
        cls: Expression,
    ) -> Result<Pattern, ParseError> {
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
            // Check if this is keyword=pattern
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
            // `**rest` capture
            if self.check(TokenKind::DoubleStar) {
                self.advance();
                rest = Some(self.expect_name()?);
                continue;
            }
            // key: pattern
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

    fn unexpected_token(&self, expected: &str) -> ParseError {
        ParseError::new(
            ParseErrorKind::UnexpectedToken(format!(
                "expected {}, got {:?}",
                expected,
                self.peek().kind
            )),
            self.peek().span,
        )
    }
}
