//! Token definitions for Python 3.8.

use compact_str::CompactString;
use ferrython_ast::BigInt;

/// Source span for a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Span {
    pub fn new(start_line: u32, start_col: u32, end_line: u32, end_col: u32) -> Self {
        Self { start_line, start_col, end_line, end_col }
    }

    pub fn point(line: u32, col: u32) -> Self {
        Self::new(line, col, line, col)
    }
}

/// A token with its kind and location.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// All Python 3.8 token types.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Literals ──
    /// Integer literal.
    Int(BigInt),
    /// Floating point literal.
    Float(f64),
    /// Complex literal (imaginary part only).
    Complex(f64),
    /// String literal (already processed escape sequences).
    String(CompactString),
    /// Bytes literal.
    Bytes(Vec<u8>),
    /// F-string start token.
    FStringStart,
    /// F-string middle (literal text between expressions).
    FStringMiddle(CompactString),
    /// F-string end token.
    FStringEnd,
    /// F-string raw content (to be parsed into JoinedStr AST).
    FString(CompactString),

    // ── Identifiers & Keywords ──
    Name(CompactString),

    // Keywords
    False,
    True,
    None,
    And,
    As,
    Assert,
    Async,
    Await,
    Break,
    Class,
    Continue,
    Def,
    Del,
    Elif,
    Else,
    Except,
    Finally,
    For,
    From,
    Global,
    If,
    Import,
    In,
    Is,
    Lambda,
    Nonlocal,
    Not,
    Or,
    Pass,
    Raise,
    Return,
    Try,
    While,
    With,
    Yield,

    // ── Operators ──
    Plus,           // +
    Minus,          // -
    Star,           // *
    DoubleStar,     // **
    Slash,          // /
    DoubleSlash,    // //
    Percent,        // %
    At,             // @
    LeftShift,      // <<
    RightShift,     // >>
    Ampersand,      // &
    Pipe,           // |
    Caret,          // ^
    Tilde,          // ~
    ColonEqual,     // :=
    Less,           // <
    Greater,        // >
    LessEqual,      // <=
    GreaterEqual,   // >=
    EqualEqual,     // ==
    NotEqual,       // !=
    Arrow,          // ->

    // ── Augmented assignment ──
    PlusEqual,      // +=
    MinusEqual,     // -=
    StarEqual,      // *=
    SlashEqual,     // /=
    DoubleSlashEqual, // //=
    PercentEqual,   // %=
    AtEqual,        // @=
    AmpersandEqual, // &=
    PipeEqual,      // |=
    CaretEqual,     // ^=
    RightShiftEqual, // >>=
    LeftShiftEqual, // <<=
    DoubleStarEqual, // **=

    // ── Delimiters ──
    LeftParen,      // (
    RightParen,     // )
    LeftBracket,    // [
    RightBracket,   // ]
    LeftBrace,      // {
    RightBrace,     // }
    Comma,          // ,
    Colon,          // :
    Dot,            // .
    Semicolon,      // ;
    Equal,          // =
    Ellipsis,       // ...

    // ── Structural ──
    Newline,
    Indent,
    Dedent,

    // ── Special ──
    Comment(CompactString),
    Eof,
}

impl TokenKind {
    /// Returns the keyword token for a name, or None if it's not a keyword.
    pub fn from_keyword(name: &str) -> Option<TokenKind> {
        match name {
            "False" => Some(TokenKind::False),
            "True" => Some(TokenKind::True),
            "None" => Some(TokenKind::None),
            "and" => Some(TokenKind::And),
            "as" => Some(TokenKind::As),
            "assert" => Some(TokenKind::Assert),
            "async" => Some(TokenKind::Async),
            "await" => Some(TokenKind::Await),
            "break" => Some(TokenKind::Break),
            "class" => Some(TokenKind::Class),
            "continue" => Some(TokenKind::Continue),
            "def" => Some(TokenKind::Def),
            "del" => Some(TokenKind::Del),
            "elif" => Some(TokenKind::Elif),
            "else" => Some(TokenKind::Else),
            "except" => Some(TokenKind::Except),
            "finally" => Some(TokenKind::Finally),
            "for" => Some(TokenKind::For),
            "from" => Some(TokenKind::From),
            "global" => Some(TokenKind::Global),
            "if" => Some(TokenKind::If),
            "import" => Some(TokenKind::Import),
            "in" => Some(TokenKind::In),
            "is" => Some(TokenKind::Is),
            "lambda" => Some(TokenKind::Lambda),
            "nonlocal" => Some(TokenKind::Nonlocal),
            "not" => Some(TokenKind::Not),
            "or" => Some(TokenKind::Or),
            "pass" => Some(TokenKind::Pass),
            "raise" => Some(TokenKind::Raise),
            "return" => Some(TokenKind::Return),
            "try" => Some(TokenKind::Try),
            "while" => Some(TokenKind::While),
            "with" => Some(TokenKind::With),
            "yield" => Some(TokenKind::Yield),
            _ => None,
        }
    }
}
