//! Ferrython parser — Python 3.8 lexer and recursive-descent parser.
//!
//! Converts Python source code into an AST.

mod error;
pub mod lexer;
pub mod parser;
mod string_parser;
pub mod token;

pub use error::{ParseError, ParseErrorKind};
pub use parser::parse;
pub use parser::parse_expression;
