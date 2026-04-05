//! Recursive-descent parser for Python 3.8.
//!
//! Parses a token stream into a Python AST.

mod arguments;
mod expressions;
mod statements;

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
    _filename: CompactString,
}

impl Parser {
    fn new(tokens: Vec<Token>, filename: &str) -> Self {
        Self {
            tokens,
            pos: 0,
            _filename: CompactString::from(filename),
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

    // ─── Block parsing ──────────────────────────────────────────────

    fn parse_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        // Check for inline body (e.g., `def f(): return 1` or `def f(): x = 1; y = 2`)
        // If next token is NOT a newline, parse semicolon-separated simple statements.
        // parse_statement's expect_newline() consumes both semicolons and newlines,
        // so we track whether a newline was hit to know when to stop.
        if !self.check(TokenKind::Newline) && !self.is_at_end() {
            let mut stmts = Vec::new();
            loop {
                let pos_before = self.pos;
                stmts.push(self.parse_statement()?);
                // parse_statement called expect_newline which consumed separators.
                // Check if we hit the end of the inline body:
                // If current pos is at end, or a Newline/Dedent was consumed (meaning
                // we crossed a line boundary), stop.
                if self.is_at_end() || self.check(TokenKind::Dedent) || self.check(TokenKind::Eof) {
                    break;
                }
                // If we're now at a compound statement keyword, we've crossed a line
                if matches!(self.peek().kind,
                    TokenKind::Def | TokenKind::Class | TokenKind::If | TokenKind::While |
                    TokenKind::For | TokenKind::Try | TokenKind::With | TokenKind::Async |
                    TokenKind::At
                ) {
                    break;
                }
                // If the consumed separator was a newline (not semicolon), stop
                // We detect this by checking: did we consume a newline token?
                // The tokens between pos_before and current pos tell us
                let consumed_newline = (pos_before..self.pos).any(|i| {
                    i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline)
                });
                if consumed_newline {
                    break;
                }
            }
            return Ok(stmts);
        }
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
        matches!(self.peek().kind, TokenKind::Newline | TokenKind::Semicolon | TokenKind::Eof)
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
        while self.pos < self.tokens.len() && matches!(self.peek().kind, TokenKind::Newline | TokenKind::Semicolon) {
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

/// Parse a Python expression from a string (used for f-string interpolation).
fn parse_expression_text(text: &str, loc: SourceLocation) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Expression::constant(Constant::Str(CompactString::from("")), loc));
    }
    parse_expression(text, "<fstring>")
}
