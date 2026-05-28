//! Statement parsing methods for the Parser.

use crate::error::{ParseError, ParseErrorKind};
use crate::token::{Span, TokenKind};
use compact_str::CompactString;
use ferrython_ast::*;

use super::Parser;

impl Parser {
    // ─── Statement parsing ──────────────────────────────────────────

    pub(super) fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();

        // Detect unexpected indent at the start of a statement.
        if matches!(self.peek().kind, TokenKind::Indent) {
            return Err(ParseError::new(
                crate::error::ParseErrorKind::IndentationError("unexpected indent".into()),
                self.peek().span,
            ));
        }

        match &self.peek().kind {
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::For => self.parse_for_stmt(false),
            TokenKind::Def => self.parse_function_def(false),
            TokenKind::Async => self.parse_async_stmt(),
            TokenKind::Class => self.parse_class_def(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Pass => {
                self.advance();
                self.expect_newline()?;
                Ok(Statement::new(StatementKind::Pass, loc))
            }
            TokenKind::Break => {
                self.advance();
                self.expect_newline()?;
                Ok(Statement::new(StatementKind::Break, loc))
            }
            TokenKind::Continue => {
                self.advance();
                self.expect_newline()?;
                Ok(Statement::new(StatementKind::Continue, loc))
            }
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
        let starts_with_py2_candidate = matches!(&self.peek().kind, TokenKind::Name(name) if name.as_str() == "print" || name.as_str() == "exec");
        let expr = match self.parse_test_list_star_expr() {
            Ok(expr) => expr,
            Err(err) if starts_with_py2_candidate => {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                    err.span,
                ));
            }
            Err(err) => return Err(err),
        };

        // Check for augmented assignment
        if let Some(op) = self.try_parse_aug_assign_op() {
            let value = self.parse_test_list()?;
            self.expect_newline()?;
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&value));
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
            let simple = matches!(&expr.node, ExpressionKind::Name { .. })
                && expr.location == expr.outer_location;
            self.validate_annotation_target(&expr)?;
            self.advance();
            let annotation = self.parse_expr()?;
            let value = if self.check(TokenKind::Equal) {
                self.advance();
                Some(Box::new(self.parse_test_list_star_expr()?))
            } else {
                None
            };
            self.expect_newline()?;
            let end = value
                .as_ref()
                .map(|expr| Self::expression_outer_location(expr))
                .unwrap_or_else(|| Self::expression_outer_location(&annotation));
            let loc = Self::with_end_location(loc, end);
            return Ok(Statement::new(
                StatementKind::AnnAssign {
                    target: Box::new(expr),
                    annotation: Box::new(annotation),
                    value,
                    simple,
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
            let loc = Self::with_end_location(loc, Self::expression_outer_location(&value));
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
        if !self.check_newline_or_eof() {
            let py2_candidate_has_invalid_expr = self.is_py2_missing_parens_candidate(&expr)
                && self.py2_print_candidate_is_invalid_expression();
            if py2_candidate_has_invalid_expr {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidSyntax("invalid syntax".into()),
                    self.peek().span,
                ));
            }
            if let Some(message) = self.py2_missing_parens_hint(&expr) {
                return Err(ParseError::new(
                    ParseErrorKind::SyntaxErrorMessage(message),
                    self.peek().span,
                ));
            }
            return Err(ParseError::new(
                ParseErrorKind::InvalidSyntax("unexpected token after expression".into()),
                self.peek().span,
            ));
        }
        self.expect_newline()?;
        let loc = Self::expression_outer_location(&expr);
        Ok(Statement::new(
            StatementKind::Expr {
                value: Box::new(expr),
            },
            loc,
        ))
    }

    fn validate_annotation_target(&self, expr: &Expression) -> Result<(), ParseError> {
        match &expr.node {
            ExpressionKind::Name { .. }
            | ExpressionKind::Attribute { .. }
            | ExpressionKind::Subscript { .. } => Ok(()),
            ExpressionKind::List { .. } => Err(ParseError::new(
                ParseErrorKind::SyntaxErrorMessage(
                    "only single target (not list) can be annotated".into(),
                ),
                Self::span_from_location(Self::expression_outer_location(expr)),
            )),
            ExpressionKind::Tuple { .. } => Err(ParseError::new(
                ParseErrorKind::SyntaxErrorMessage(
                    "only single target (not tuple) can be annotated".into(),
                ),
                Self::span_from_location(Self::expression_outer_location(expr)),
            )),
            _ => Err(ParseError::new(
                ParseErrorKind::SyntaxErrorMessage("illegal target for annotation".into()),
                Self::span_from_location(Self::expression_outer_location(expr)),
            )),
        }
    }

    fn is_py2_missing_parens_candidate(&self, expr: &Expression) -> bool {
        matches!(&expr.node, ExpressionKind::Name { id, .. } if id.as_str() == "print" || id.as_str() == "exec")
    }

    fn py2_missing_parens_hint(&self, expr: &Expression) -> Option<String> {
        let ExpressionKind::Name { id, .. } = &expr.node else {
            return None;
        };
        if self.py2_print_candidate_is_invalid_expression() {
            return None;
        }
        match id.as_str() {
            "print" => {
                let (args, soft_space) = self.py2_print_suggestion_args();
                let suggestion = if soft_space {
                    format!("{}, end=\" \"", args)
                } else {
                    args
                };
                Some(format!(
                    "Missing parentheses in call to 'print'. Did you mean print({})?",
                    suggestion
                ))
            }
            "exec" => Some("Missing parentheses in call to 'exec'".to_string()),
            _ => None,
        }
    }

    fn py2_print_candidate_is_invalid_expression(&self) -> bool {
        let mut parser = Parser {
            tokens: self.tokens.clone(),
            pos: self.pos,
            _filename: self._filename.clone(),
        };
        parser.parse_test_list_star_expr().is_err()
    }

    fn py2_print_suggestion_args(&self) -> (String, bool) {
        let mut end = self.pos;
        while end < self.tokens.len()
            && !matches!(
                self.tokens[end].kind,
                TokenKind::Newline | TokenKind::Semicolon | TokenKind::Eof
            )
        {
            end += 1;
        }

        let mut soft_space = false;
        if end > self.pos && matches!(self.tokens[end - 1].kind, TokenKind::Comma) {
            soft_space = true;
            end -= 1;
        }

        (self.py2_print_tokens_to_source(self.pos, end), soft_space)
    }

    fn py2_print_tokens_to_source(&self, start: usize, end: usize) -> String {
        let mut out = String::new();
        for token in &self.tokens[start..end] {
            match &token.kind {
                TokenKind::Comma => {
                    if out.ends_with(' ') {
                        out.pop();
                    }
                    out.push_str(", ");
                }
                TokenKind::Dot => {
                    if out.ends_with(' ') {
                        out.pop();
                    }
                    out.push('.');
                }
                TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => {
                    out.push_str(Self::py2_print_token_source(&token.kind).as_str());
                }
                TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                    if out.ends_with(' ') {
                        out.pop();
                    }
                    out.push_str(Self::py2_print_token_source(&token.kind).as_str());
                }
                _ => {
                    let piece = Self::py2_print_token_source(&token.kind);
                    if piece.is_empty() {
                        continue;
                    }
                    if !out.is_empty()
                        && !out
                            .chars()
                            .last()
                            .is_some_and(|ch| matches!(ch, ' ' | '.' | '(' | '[' | '{'))
                        && Self::py2_print_needs_space(&piece)
                    {
                        out.push(' ');
                    }
                    out.push_str(&piece);
                }
            }
        }
        out.trim().to_string()
    }

    fn py2_print_needs_space(piece: &str) -> bool {
        piece
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '"' || ch == '\'')
    }

    fn py2_print_token_source(kind: &TokenKind) -> String {
        match kind {
            TokenKind::Name(name) => name.to_string(),
            TokenKind::String(s) => format!("{:?}", s.as_str()),
            TokenKind::Bytes(bytes) => format!("b{:?}", String::from_utf8_lossy(bytes)),
            TokenKind::Int(n) => match n {
                BigInt::Small(value) => value.to_string(),
                BigInt::Big(value) => value.to_string(),
            },
            TokenKind::Float(f) => f.to_string(),
            TokenKind::Complex(f) => format!("{}j", f),
            TokenKind::False => "False".to_string(),
            TokenKind::True => "True".to_string(),
            TokenKind::None => "None".to_string(),
            TokenKind::Plus => " + ".to_string(),
            TokenKind::Minus => " - ".to_string(),
            TokenKind::Star => "*".to_string(),
            TokenKind::DoubleStar => "**".to_string(),
            TokenKind::Slash => " / ".to_string(),
            TokenKind::DoubleSlash => " // ".to_string(),
            TokenKind::Percent => " % ".to_string(),
            TokenKind::At => " @ ".to_string(),
            TokenKind::LeftShift => " << ".to_string(),
            TokenKind::RightShift => " >> ".to_string(),
            TokenKind::Ampersand => " & ".to_string(),
            TokenKind::Pipe => " | ".to_string(),
            TokenKind::Caret => " ^ ".to_string(),
            TokenKind::Tilde => "~".to_string(),
            TokenKind::ColonEqual => " := ".to_string(),
            TokenKind::Less => " < ".to_string(),
            TokenKind::Greater => " > ".to_string(),
            TokenKind::LessEqual => " <= ".to_string(),
            TokenKind::GreaterEqual => " >= ".to_string(),
            TokenKind::EqualEqual => " == ".to_string(),
            TokenKind::NotEqual => " != ".to_string(),
            TokenKind::LeftParen => "(".to_string(),
            TokenKind::RightParen => ")".to_string(),
            TokenKind::LeftBracket => "[".to_string(),
            TokenKind::RightBracket => "]".to_string(),
            TokenKind::LeftBrace => "{".to_string(),
            TokenKind::RightBrace => "}".to_string(),
            TokenKind::Colon => ": ".to_string(),
            _ => String::new(),
        }
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

        let end = Self::suite_end_location([body.as_slice(), orelse.as_slice()])
            .unwrap_or_else(|| Self::expression_outer_location(&test));
        let loc = Self::with_end_location(loc, end);
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

        let end = Self::suite_end_location([body.as_slice(), orelse.as_slice()])
            .unwrap_or_else(|| Self::expression_outer_location(&test));
        let loc = Self::with_end_location(loc, end);
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
        let end = Self::suite_end_location([body.as_slice(), orelse.as_slice()])
            .unwrap_or_else(|| Self::expression_outer_location(&test));
        let loc = Self::with_end_location(loc, end);
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
        let end = Self::suite_end_location([body.as_slice(), orelse.as_slice()])
            .unwrap_or_else(|| Self::expression_outer_location(&iter));
        let loc = Self::with_end_location(loc, end);
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
        let end = Self::last_statement_location(&body)
            .or_else(|| {
                returns
                    .as_ref()
                    .map(|expr| Self::expression_outer_location(expr))
            })
            .unwrap_or(loc);
        let loc = Self::with_end_location(loc, end);
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
            let open_location = self.current_location();
            self.advance();
            let (b, k) = self.parse_class_args(open_location)?;
            self.expect(TokenKind::RightParen)?;
            (b, k)
        } else {
            (Vec::new(), Vec::new())
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_block()?;
        let end = Self::last_statement_location(&body)
            .or_else(|| keywords.last().map(|kw| kw.location))
            .or_else(|| bases.last().map(Self::expression_outer_location))
            .unwrap_or(loc);
        let loc = Self::with_end_location(loc, end);
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
        let loc = value
            .as_ref()
            .map(|expr| Self::with_end_location(loc, Self::expression_outer_location(expr)))
            .unwrap_or(loc);
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
        let end = cause
            .as_ref()
            .map(|expr| Self::expression_outer_location(expr))
            .unwrap_or_else(|| Self::expression_outer_location(&exc));
        let loc = Self::with_end_location(loc, end);
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
        if !self.check_newline_or_eof() {
            return Err(self.unexpected_token("newline"));
        }
        self.expect_newline()?;
        let loc = names
            .last()
            .map(|alias| Self::with_end_location(loc, alias.location))
            .unwrap_or(loc);
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
        let mut end = loc;
        let names = if self.check(TokenKind::Star) {
            let star_span = self.peek().span;
            self.advance();
            let star_loc = Self::location_from_span(star_span);
            end = star_loc;
            vec![Alias {
                name: CompactString::from("*"),
                asname: None,
                location: star_loc,
            }]
        } else {
            let open_paren = self.check(TokenKind::LeftParen);
            if open_paren {
                self.advance();
            }
            let mut names = vec![self.parse_import_as_name()?];
            end = names.last().map(|alias| alias.location).unwrap_or(end);
            while self.check(TokenKind::Comma) {
                self.advance();
                if open_paren && self.check(TokenKind::RightParen) {
                    break;
                }
                let alias = self.parse_import_as_name()?;
                end = alias.location;
                names.push(alias);
            }
            if open_paren {
                let rparen_span = self.expect(TokenKind::RightParen)?.span;
                end = Self::location_from_span(rparen_span);
            }
            names
        };
        if !self.check_newline_or_eof() {
            return Err(self.unexpected_token("newline"));
        }
        self.expect_newline()?;
        let loc = Self::with_end_location(loc, end);
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
        let end = Self::last_statement_location(&finalbody)
            .or_else(|| Self::last_statement_location(&orelse))
            .or_else(|| handlers.last().map(|handler| handler.location))
            .or_else(|| Self::last_statement_location(&body))
            .unwrap_or(loc);
        let loc = Self::with_end_location(loc, end);
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
        // Check for except* (PEP 654)
        let is_star = if self.check(TokenKind::Star) {
            self.advance();
            true
        } else {
            false
        };
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
        let end = Self::last_statement_location(&body)
            .or_else(|| {
                typ.as_ref()
                    .map(|expr| Self::expression_outer_location(expr))
            })
            .unwrap_or(loc);
        let loc = Self::with_end_location(loc, end);
        Ok(ExceptHandler {
            typ,
            name,
            body,
            location: loc,
            is_star,
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
        let end = Self::last_statement_location(&body)
            .or_else(|| {
                items.last().map(|item| {
                    item.optional_vars
                        .as_ref()
                        .map(|expr| Self::expression_outer_location(expr))
                        .unwrap_or_else(|| Self::expression_outer_location(&item.context_expr))
                })
            })
            .unwrap_or(loc);
        let loc = Self::with_end_location(loc, end);
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
        let end = msg
            .as_ref()
            .map(|expr| Self::expression_outer_location(expr))
            .unwrap_or_else(|| Self::expression_outer_location(&test));
        let loc = Self::with_end_location(loc, end);
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
        let loc = targets
            .last()
            .map(|target| Self::with_end_location(loc, Self::expression_outer_location(target)))
            .unwrap_or(loc);
        Ok(Statement::new(StatementKind::Delete { targets }, loc))
    }

    fn parse_global_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Global)?;
        let mut last_span = self.peek().span;
        let mut names = vec![self.expect_name()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            last_span = self.peek().span;
            names.push(self.expect_name()?);
        }
        self.expect_newline()?;
        let loc = Self::with_end_span(loc, last_span);
        Ok(Statement::new(StatementKind::Global { names }, loc))
    }

    fn parse_nonlocal_stmt(&mut self) -> Result<Statement, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Nonlocal)?;
        let mut last_span = self.peek().span;
        let mut names = vec![self.expect_name()?];
        while self.check(TokenKind::Comma) {
            self.advance();
            last_span = self.peek().span;
            names.push(self.expect_name()?);
        }
        self.expect_newline()?;
        let loc = Self::with_end_span(loc, last_span);
        Ok(Statement::new(StatementKind::Nonlocal { names }, loc))
    }

    fn parse_async_stmt(&mut self) -> Result<Statement, ParseError> {
        let async_loc = self.current_location();
        self.expect(TokenKind::Async)?;
        let mut stmt = match &self.peek().kind {
            TokenKind::Def => self.parse_function_def(true),
            TokenKind::For => self.parse_for_stmt(true),
            TokenKind::With => self.parse_with_stmt(true),
            _ => Err(ParseError::new(
                ParseErrorKind::InvalidSyntax(
                    "expected 'def', 'for', or 'with' after 'async'".into(),
                ),
                self.peek().span,
            )),
        }?;
        stmt.location = Self::with_end_location(async_loc, stmt.location);
        Ok(stmt)
    }

    fn parse_decorated(&mut self) -> Result<Statement, ParseError> {
        let mut decorators = Vec::new();
        while self.check(TokenKind::At) {
            self.advance();
            decorators.push(self.parse_decorator_expr()?);
            self.expect_newline()?;
        }
        let mut stmt = match &self.peek().kind {
            TokenKind::Def => self.parse_function_def(false)?,
            TokenKind::Async => {
                let async_loc = self.current_location();
                self.advance();
                let mut stmt = self.parse_function_def(true)?;
                stmt.location = Self::with_end_location(async_loc, stmt.location);
                stmt
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
            StatementKind::FunctionDef { decorator_list, .. }
            | StatementKind::ClassDef { decorator_list, .. } => {
                *decorator_list = decorators;
            }
            _ => unreachable!(),
        }
        Ok(stmt)
    }

    fn parse_decorator_expr(&mut self) -> Result<Expression, ParseError> {
        let start = self.current_location();
        let mut expr = Expression::name(self.expect_name()?, ExprContext::Load, start);

        while self.check(TokenKind::Dot) {
            self.advance();
            let attr_span = self.peek().span;
            let attr = self.expect_name()?;
            let loc = Self::with_end_span(Self::expression_outer_location(&expr), attr_span);
            expr = Expression::new(
                ExpressionKind::Attribute {
                    value: Box::new(expr),
                    attr,
                    ctx: ExprContext::Load,
                },
                loc,
            );
        }

        if self.check(TokenKind::LeftParen) {
            let open_location = self.current_location();
            self.advance();
            let (args, keywords) = self.parse_call_args(open_location)?;
            let rparen_span = self.expect(TokenKind::RightParen)?.span;
            let loc = Self::with_end_span(Self::expression_outer_location(&expr), rparen_span);
            expr = Expression::new(
                ExpressionKind::Call {
                    func: Box::new(expr),
                    args,
                    keywords,
                },
                loc,
            );
        }

        if !self.check_newline_or_eof() {
            return Err(ParseError::new(
                ParseErrorKind::InvalidSyntax("invalid decorator".into()),
                Span::new(
                    expr.location.line,
                    expr.location.column,
                    expr.location.end_line.unwrap_or(expr.location.line),
                    expr.location.end_column.unwrap_or(expr.location.column),
                ),
            ));
        }

        Ok(expr)
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
        let mut name = self.expect_name()?.to_string();
        let mut end_span = self.tokens[self.pos.saturating_sub(1)].span;
        while self.check(TokenKind::Dot) {
            self.advance();
            name.push('.');
            end_span = self.peek().span;
            name.push_str(self.expect_name()?.as_str());
        }
        let asname = if self.check(TokenKind::As) {
            self.advance();
            end_span = self.peek().span;
            Some(self.expect_name()?)
        } else {
            None
        };
        let loc = Self::with_end_span(loc, end_span);
        Ok(Alias {
            name: CompactString::from(name),
            asname,
            location: loc,
        })
    }

    fn parse_import_as_name(&mut self) -> Result<Alias, ParseError> {
        let loc = self.current_location();
        let mut end_span = self.peek().span;
        let name = self.expect_name()?;
        let asname = if self.check(TokenKind::As) {
            self.advance();
            end_span = self.peek().span;
            Some(self.expect_name()?)
        } else {
            None
        };
        let loc = Self::with_end_span(loc, end_span);
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

    pub(super) fn unexpected_token(&self, expected: &str) -> ParseError {
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
