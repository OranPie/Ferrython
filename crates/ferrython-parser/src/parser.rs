//! Recursive-descent parser for Python 3.8.
//!
//! Parses a token stream into a Python AST.

use crate::error::{ParseError, ParseErrorKind};
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};
use compact_str::CompactString;
use ferrython_ast::*;

/// Parse a Python source string into a Module AST.
pub fn parse(source: &str, filename: &str) -> Result<Module, ParseError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens, filename);
    parser.parse_module()
}

/// Parse a single expression.
pub fn parse_expression(source: &str, filename: &str) -> Result<Expression, ParseError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens, filename);
    parser.parse_expr()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    #[allow(dead_code)]
    filename: CompactString,
}

impl Parser {
    fn new(tokens: Vec<Token>, filename: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            filename: CompactString::from(filename),
        }
    }

    // ─── Module parsing ─────────────────────────────────────────────

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.is_at_end() {
            let stmt = self.parse_statement()?;
            body.push(stmt);
            self.skip_newlines();
        }
        Ok(Module::Module {
            body,
            type_ignores: Vec::new(),
        })
    }

    // ─── Statement parsing ──────────────────────────────────────────

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
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

    fn parse_function_def(&mut self, is_async: bool) -> Result<Statement, ParseError> {
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

    fn parse_class_def(&mut self) -> Result<Statement, ParseError> {
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
            Some(Box::new(self.parse_test_list()?))
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

    // ─── Expression parsing (precedence climbing) ───────────────────

    fn parse_expr(&mut self) -> Result<Expression, ParseError> {
        self.parse_test()
    }

    fn parse_test(&mut self) -> Result<Expression, ParseError> {
        // Handle lambda
        if self.check(TokenKind::Lambda) {
            return self.parse_lambda();
        }

        let expr = self.parse_or_test()?;

        // Ternary: expr if test else expr
        if self.check(TokenKind::If) {
            let loc = self.current_location();
            self.advance();
            let test = self.parse_or_test()?;
            self.expect(TokenKind::Else)?;
            let orelse = self.parse_test()?;
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

    fn parse_or_test(&mut self) -> Result<Expression, ParseError> {
        let mut expr = self.parse_and_test()?;
        while self.check(TokenKind::Or) {
            let loc = self.current_location();
            self.advance();
            let right = self.parse_and_test()?;
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
            let loc = self.current_location();
            self.advance();
            let right = self.parse_not_test()?;
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
                    if self.peek_at(1).map(|t| matches!(t.kind, TokenKind::In)).unwrap_or(false) {
                        self.advance(); // skip 'not'
                        Some(CompareOperator::NotIn)
                    } else {
                        None
                    }
                }
                TokenKind::Is => {
                    if self.peek_at(1).map(|t| matches!(t.kind, TokenKind::Not)).unwrap_or(false) {
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
            let loc = left.location;
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

    fn parse_or_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_xor_expr()?;
        while self.check(TokenKind::Pipe) {
            let loc = left.location;
            self.advance();
            let right = self.parse_xor_expr()?;
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
            let loc = left.location;
            self.advance();
            let right = self.parse_and_expr()?;
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
            let loc = left.location;
            self.advance();
            let right = self.parse_shift_expr()?;
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
                let loc = left.location;
                self.advance();
                let right = self.parse_arith_expr()?;
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
                let loc = left.location;
                self.advance();
                let right = self.parse_term()?;
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
                let loc = left.location;
                self.advance();
                let right = self.parse_factor()?;
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
            let loc = base.location;
            self.advance();
            let exp = self.parse_factor()?;
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

        let mut expr = self.parse_atom()?;

        // Trailers: .attr, [subscript], (call)
        loop {
            match &self.peek().kind {
                TokenKind::LeftParen => {
                    let loc = expr.location;
                    self.advance();
                    let (args, keywords) = self.parse_call_args()?;
                    self.expect(TokenKind::RightParen)?;
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
                    let loc = expr.location;
                    self.advance();
                    let slice = self.parse_subscript()?;
                    self.expect(TokenKind::RightBracket)?;
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
                    let loc = expr.location;
                    self.advance();
                    let attr = self.expect_name()?;
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
            expr = Expression::new(
                ExpressionKind::Await {
                    value: Box::new(expr),
                },
                await_loc,
            );
        }

        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expression, ParseError> {
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
                    return Ok(Expression::new(
                        ExpressionKind::NamedExpr {
                            target: Box::new(Expression::name(name, ExprContext::Store, loc)),
                            value: Box::new(value),
                        },
                        loc,
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
                Ok(Expression::constant(Constant::Complex { real: 0.0, imag: f }, loc))
            }
            TokenKind::String(s) => {
                let mut result = s.to_string();
                self.advance();
                // Concatenate adjacent strings
                while let TokenKind::String(s2) = &self.peek().kind {
                    result.push_str(s2.as_str());
                    self.advance();
                }
                Ok(Expression::constant(
                    Constant::Str(CompactString::from(result)),
                    loc,
                ))
            }
            TokenKind::Bytes(b) => {
                let mut result = b.clone();
                self.advance();
                while let TokenKind::Bytes(b2) = &self.peek().kind {
                    result.extend_from_slice(b2);
                    self.advance();
                }
                Ok(Expression::constant(Constant::Bytes(result), loc))
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
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                let expr = self.parse_test_list_star_expr()?;
                // Check for generator expression
                if self.check(TokenKind::For) {
                    let generators = self.parse_comp_for()?;
                    self.expect(TokenKind::RightParen)?;
                    return Ok(Expression::new(
                        ExpressionKind::GeneratorExp {
                            elt: Box::new(expr),
                            generators,
                        },
                        loc,
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
                    self.expect(TokenKind::RightParen)?;
                    return Ok(Expression::new(
                        ExpressionKind::Tuple {
                            elts,
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                self.expect(TokenKind::RightParen)?;
                Ok(expr)
            }
            TokenKind::LeftBracket => {
                self.advance();
                if self.check(TokenKind::RightBracket) {
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::List {
                            elts: Vec::new(),
                            ctx: ExprContext::Load,
                        },
                        loc,
                    ));
                }
                let first = self.parse_test_or_star()?;
                // List comprehension?
                if self.check(TokenKind::For) {
                    let generators = self.parse_comp_for()?;
                    self.expect(TokenKind::RightBracket)?;
                    return Ok(Expression::new(
                        ExpressionKind::ListComp {
                            elt: Box::new(first),
                            generators,
                        },
                        loc,
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
                self.expect(TokenKind::RightBracket)?;
                Ok(Expression::new(
                    ExpressionKind::List {
                        elts,
                        ctx: ExprContext::Load,
                    },
                    loc,
                ))
            }
            TokenKind::LeftBrace => {
                self.advance();
                if self.check(TokenKind::RightBrace) {
                    self.advance();
                    return Ok(Expression::new(
                        ExpressionKind::Dict {
                            keys: Vec::new(),
                            values: Vec::new(),
                        },
                        loc,
                    ));
                }
                // Could be dict or set
                let first = self.parse_test_or_star()?;
                if self.check(TokenKind::Colon) {
                    // Dict
                    self.advance();
                    let first_val = self.parse_test()?;
                    // Dict comprehension?
                    if self.check(TokenKind::For) {
                        let generators = self.parse_comp_for()?;
                        self.expect(TokenKind::RightBrace)?;
                        return Ok(Expression::new(
                            ExpressionKind::DictComp {
                                key: Box::new(first),
                                value: Box::new(first_val),
                                generators,
                            },
                            loc,
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
                    self.expect(TokenKind::RightBrace)?;
                    Ok(Expression::new(
                        ExpressionKind::Dict { keys, values },
                        loc,
                    ))
                } else {
                    // Set
                    if self.check(TokenKind::For) {
                        let generators = self.parse_comp_for()?;
                        self.expect(TokenKind::RightBrace)?;
                        return Ok(Expression::new(
                            ExpressionKind::SetComp {
                                elt: Box::new(first),
                                generators,
                            },
                            loc,
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
                    self.expect(TokenKind::RightBrace)?;
                    Ok(Expression::new(ExpressionKind::Set { elts }, loc))
                }
            }
            _ => Err(ParseError::new(
                ParseErrorKind::ExpressionExpected,
                tok.span,
            )),
        }
    }

    fn parse_lambda(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        self.expect(TokenKind::Lambda)?;
        let args = if self.check(TokenKind::Colon) {
            Arguments::empty()
        } else {
            self.parse_parameters()?
        };
        self.expect(TokenKind::Colon)?;
        let body = self.parse_test()?;
        Ok(Expression::new(
            ExpressionKind::Lambda {
                args: Box::new(args),
                body: Box::new(body),
            },
            loc,
        ))
    }

    fn parse_subscript(&mut self) -> Result<Expression, ParseError> {
        let loc = self.current_location();
        let lower = if self.check(TokenKind::Colon) {
            None
        } else {
            Some(Box::new(self.parse_test()?))
        };
        if !self.check(TokenKind::Colon) {
            return Ok(*lower.unwrap());
        }
        self.advance(); // skip ':'
        let upper = if !self.check(TokenKind::Colon) && !self.check(TokenKind::RightBracket) {
            Some(Box::new(self.parse_test()?))
        } else {
            None
        };
        let step = if self.check(TokenKind::Colon) {
            self.advance();
            if !self.check(TokenKind::RightBracket) {
                Some(Box::new(self.parse_test()?))
            } else {
                None
            }
        } else {
            None
        };
        Ok(Expression::new(
            ExpressionKind::Slice {
                lower,
                upper,
                step,
            },
            loc,
        ))
    }

    // ─── Comprehension parsing ──────────────────────────────────────

    fn parse_comp_for(&mut self) -> Result<Vec<Comprehension>, ParseError> {
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

    // ─── Argument parsing ───────────────────────────────────────────

    fn parse_parameters(&mut self) -> Result<Arguments, ParseError> {
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

    fn parse_call_args(&mut self) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
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

    fn parse_class_args(&mut self) -> Result<(Vec<Expression>, Vec<Keyword>), ParseError> {
        self.parse_call_args()
    }

    // ─── Helper expression parsers ──────────────────────────────────

    fn parse_test_list(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_test()?;
        if !self.check(TokenKind::Comma) {
            return Ok(first);
        }
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                break;
            }
            elts.push(self.parse_test()?);
        }
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Load,
            },
            loc,
        ))
    }

    fn parse_test_list_star_expr(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_test_or_star()?;
        if !self.check(TokenKind::Comma) {
            return Ok(first);
        }
        // Could be a tuple target or value — use Load context here.
        // The compiler's compile_store_target handles Store context separately.
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check_newline_or_eof()
                || self.check(TokenKind::Equal)
                || self.check(TokenKind::RightParen)
                || self.check(TokenKind::RightBracket)
                || self.check(TokenKind::RightBrace)
            {
                break;
            }
            elts.push(self.parse_test_or_star()?);
        }
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

    fn parse_target(&mut self) -> Result<Expression, ParseError> {
        // Use parse_or_expr to stop before 'in' (which is a comparison op)
        self.parse_or_expr()
    }

    fn parse_target_list(&mut self) -> Result<Expression, ParseError> {
        let first = self.parse_target()?;
        if !self.check(TokenKind::Comma) || self.check(TokenKind::In) {
            return Ok(first);
        }
        let loc = first.location;
        let mut elts = vec![first];
        while self.check(TokenKind::Comma) {
            self.advance();
            if self.check(TokenKind::In) {
                break;
            }
            elts.push(self.parse_target()?);
        }
        Ok(Expression::new(
            ExpressionKind::Tuple {
                elts,
                ctx: ExprContext::Store,
            },
            loc,
        ))
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

    // ─── Block parsing ──────────────────────────────────────────────

    fn parse_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        self.expect_newline()?;
        self.expect(TokenKind::Indent)?;
        let mut stmts = Vec::new();
        while !self.check(TokenKind::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(TokenKind::Dedent) || self.is_at_end() {
                break;
            }
            stmts.push(self.parse_statement()?);
        }
        if self.check(TokenKind::Dedent) {
            self.advance();
        }
        Ok(stmts)
    }

    // ─── Token helpers ──────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, kind: TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(&kind)
    }

    fn check_newline_or_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Newline | TokenKind::Eof)
    }

    fn expect(&mut self, kind: TokenKind) -> Result<&Token, ParseError> {
        if self.check(kind.clone()) {
            Ok(self.advance())
        } else {
            Err(ParseError::new(
                ParseErrorKind::UnexpectedToken(format!(
                    "expected {:?}, got {:?}",
                    kind,
                    self.peek().kind
                )),
                self.peek().span,
            ))
        }
    }

    fn expect_name(&mut self) -> Result<CompactString, ParseError> {
        if let TokenKind::Name(name) = &self.peek().kind {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            Err(ParseError::new(
                ParseErrorKind::UnexpectedToken(format!(
                    "expected identifier, got {:?}",
                    self.peek().kind
                )),
                self.peek().span,
            ))
        }
    }

    fn expect_newline(&mut self) -> Result<(), ParseError> {
        self.skip_newlines();
        Ok(())
    }

    fn skip_newlines(&mut self) {
        while self.pos < self.tokens.len() && matches!(self.peek().kind, TokenKind::Newline) {
            self.advance();
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len() || matches!(self.peek().kind, TokenKind::Eof)
    }

    fn current_location(&self) -> SourceLocation {
        let span = self.peek().span;
        SourceLocation::new(span.start_line, span.start_col)
    }
}
