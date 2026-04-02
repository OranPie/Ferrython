//! Parse error types.

use crate::token::Span;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[error("{kind} at {span:?}")]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

impl ParseError {
    pub fn new(kind: ParseErrorKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, Error)]
pub enum ParseErrorKind {
    #[error("unexpected token: {0}")]
    UnexpectedToken(String),
    #[error("unexpected EOF")]
    UnexpectedEof,
    #[error("invalid syntax: {0}")]
    InvalidSyntax(String),
    #[error("invalid indentation")]
    IndentationError,
    #[error("inconsistent use of tabs and spaces")]
    TabError,
    #[error("unterminated string literal")]
    UnterminatedString,
    #[error("invalid escape sequence: \\{0}")]
    InvalidEscape(char),
    #[error("invalid number literal: {0}")]
    InvalidNumber(String),
    #[error("expression expected")]
    ExpressionExpected,
    #[error("assignment to keyword")]
    AssignToKeyword,
    #[error("'break' outside loop")]
    BreakOutsideLoop,
    #[error("'continue' outside loop")]
    ContinueOutsideLoop,
    #[error("'return' outside function")]
    ReturnOutsideFunction,
    #[error("'yield' outside function")]
    YieldOutsideFunction,
    #[error("multiple starred expressions in assignment")]
    MultipleStarredInAssignment,
}
