//! Python 3.8 lexer (tokenizer).
//!
//! Converts source text into a stream of tokens, handling indentation,
//! implicit line joining, and all Python literal formats.

use crate::error::{ParseError, ParseErrorKind};
use crate::string_parser;
use crate::token::{Span, Token, TokenKind};
use ferrython_ast::BigInt;
use compact_str::CompactString;

pub struct Lexer<'src> {
    #[allow(dead_code)]
    source: &'src str,
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    /// Indentation stack (number of spaces per level).
    indent_stack: Vec<u32>,
    /// Pending tokens (for INDENT/DEDENT generation).
    pending: Vec<Token>,
    /// Nesting level of parentheses, brackets, braces (for implicit line joining).
    nesting: u32,
    /// Whether we're at the start of a logical line.
    at_line_start: bool,
    /// Whether we've emitted EOF.
    done: bool,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 0,
            indent_stack: vec![0],
            pending: Vec::new(),
            nesting: 0,
            at_line_start: true,
            done: false,
        }
    }

    /// Tokenize the entire source into a Vec of tokens.
    pub fn tokenize(&mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Get the next token.
    pub fn next_token(&mut self) -> Result<Token, ParseError> {
        // Return any pending tokens first (INDENT/DEDENT).
        if let Some(tok) = self.pending.pop() {
            return Ok(tok);
        }

        if self.done {
            return Ok(Token::new(TokenKind::Eof, self.span_here()));
        }

        // Handle indentation at the start of a line.
        if self.at_line_start {
            self.at_line_start = false;
            return self.handle_indentation();
        }

        self.skip_spaces();

        if self.is_at_end() {
            return self.emit_eof();
        }

        let c = self.peek_char();

        // Skip comments
        if c == '#' {
            self.skip_comment();
            // After a comment, treat the newline
            if !self.is_at_end() && self.peek_char() == '\n' {
                return self.handle_newline();
            }
            return self.emit_eof();
        }

        // Newline
        if c == '\n' || c == '\r' {
            return self.handle_newline();
        }

        // Line continuation
        if c == '\\' && self.peek_char_at(1) == Some('\n') {
            self.advance(); // skip backslash
            self.advance(); // skip newline
            self.line += 1;
            self.col = 0;
            return self.next_token();
        }

        // Numbers
        if c.is_ascii_digit() || (c == '.' && self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit())) {
            return self.lex_number();
        }

        // Identifiers and keywords
        if c == '_' || c.is_alphabetic() || is_id_start(c) {
            return self.lex_identifier();
        }

        // String literals
        if c == '\'' || c == '"' {
            return self.lex_string(false, false);
        }

        // String prefixes
        if matches!(c, 'r' | 'R' | 'b' | 'B' | 'u' | 'U' | 'f' | 'F') {
            if let Some(tok) = self.try_lex_string_prefix()? {
                return Ok(tok);
            }
        }

        // Ellipsis
        if c == '.' && self.peek_char_at(1) == Some('.') && self.peek_char_at(2) == Some('.') {
            let start = self.span_start();
            self.advance();
            self.advance();
            self.advance();
            return Ok(Token::new(TokenKind::Ellipsis, self.span_from(start)));
        }

        // Operators and delimiters
        self.lex_operator()
    }

    // ─── Indentation ────────────────────────────────────────────────

    fn handle_indentation(&mut self) -> Result<Token, ParseError> {
        // Measure indentation at start of line
        let mut indent: u32 = 0;
        while !self.is_at_end() {
            match self.peek_char() {
                ' ' => {
                    indent += 1;
                    self.advance();
                }
                '\t' => {
                    // Tab stops at every 8 spaces (like CPython)
                    indent = (indent / 8 + 1) * 8;
                    self.advance();
                }
                '\n' | '\r' => {
                    // Blank line — skip it entirely
                    self.handle_newline_raw();
                    indent = 0;
                    continue;
                }
                '#' => {
                    // Comment-only line — skip
                    self.skip_comment();
                    if !self.is_at_end() {
                        self.handle_newline_raw();
                    }
                    indent = 0;
                    continue;
                }
                _ => break,
            }
        }

        if self.is_at_end() {
            return self.emit_eof();
        }

        let current_indent = *self.indent_stack.last().unwrap();
        let span = self.span_here();

        if indent > current_indent {
            self.indent_stack.push(indent);
            Ok(Token::new(TokenKind::Indent, span))
        } else if indent < current_indent {
            // Generate DEDENT tokens
            while let Some(&top) = self.indent_stack.last() {
                if top <= indent {
                    break;
                }
                self.indent_stack.pop();
                self.pending.push(Token::new(TokenKind::Dedent, span));
            }
            if *self.indent_stack.last().unwrap() != indent {
                return Err(ParseError::new(ParseErrorKind::IndentationError, span));
            }
            // Return the first DEDENT, rest are pending
            if let Some(tok) = self.pending.pop() {
                Ok(tok)
            } else {
                self.next_token()
            }
        } else {
            // Same indentation level — continue to next token
            self.next_token()
        }
    }

    fn handle_newline(&mut self) -> Result<Token, ParseError> {
        let span = self.span_here();
        self.handle_newline_raw();

        if self.nesting > 0 {
            // Inside brackets — implicit line joining, ignore newline
            return self.next_token();
        }

        self.at_line_start = true;
        Ok(Token::new(TokenKind::Newline, span))
    }

    fn handle_newline_raw(&mut self) {
        if self.peek_char() == '\r' {
            self.advance();
        }
        if !self.is_at_end() && self.peek_char() == '\n' {
            self.advance();
        }
        self.line += 1;
        self.col = 0;
    }

    // ─── Numbers ────────────────────────────────────────────────────

    fn lex_number(&mut self) -> Result<Token, ParseError> {
        let start = self.span_start();
        let c = self.peek_char();

        // Hex, octal, binary
        if c == '0' {
            if let Some(next) = self.peek_char_at(1) {
                match next {
                    'x' | 'X' => return self.lex_hex_int(start),
                    'o' | 'O' => return self.lex_oct_int(start),
                    'b' | 'B' => return self.lex_bin_int(start),
                    _ => {}
                }
            }
        }

        // Decimal int or float
        let mut num_str = String::new();
        self.collect_digits(&mut num_str);

        let is_float = !self.is_at_end()
            && (self.peek_char() == '.'
                || self.peek_char() == 'e'
                || self.peek_char() == 'E');

        if !self.is_at_end() && self.peek_char() == '.' {
            // Check it's not ellipsis
            if self.peek_char_at(1) != Some('.') {
                num_str.push('.');
                self.advance();
                self.collect_digits(&mut num_str);
            } else if !is_float {
                // It's an integer followed by ellipsis or attribute access
                return self.make_int_token(num_str, 10, start);
            }
        }

        if !self.is_at_end() && matches!(self.peek_char(), 'e' | 'E') {
            num_str.push('e');
            self.advance();
            if !self.is_at_end() && matches!(self.peek_char(), '+' | '-') {
                num_str.push(self.peek_char());
                self.advance();
            }
            self.collect_digits(&mut num_str);
            return self.make_float_or_complex(num_str, start);
        }

        if is_float || num_str.contains('.') {
            return self.make_float_or_complex(num_str, start);
        }

        // Check for complex suffix
        if !self.is_at_end() && matches!(self.peek_char(), 'j' | 'J') {
            self.advance();
            let val: f64 = num_str.replace('_', "").parse().unwrap_or(0.0);
            return Ok(Token::new(
                TokenKind::Complex(val),
                self.span_from(start),
            ));
        }

        self.make_int_token(num_str, 10, start)
    }

    fn lex_hex_int(&mut self, start: (u32, u32)) -> Result<Token, ParseError> {
        self.advance(); // 0
        self.advance(); // x/X
        let mut s = String::new();
        self.collect_hex_digits(&mut s);
        if s.is_empty() {
            return Err(ParseError::new(
                ParseErrorKind::InvalidNumber("empty hex literal".into()),
                self.span_from(start),
            ));
        }
        self.make_int_token(s, 16, start)
    }

    fn lex_oct_int(&mut self, start: (u32, u32)) -> Result<Token, ParseError> {
        self.advance(); // 0
        self.advance(); // o/O
        let mut s = String::new();
        while !self.is_at_end() && (self.peek_char().is_ascii_digit() || self.peek_char() == '_') {
            let c = self.peek_char();
            if c == '_' {
                self.advance();
                continue;
            }
            if c >= '8' {
                return Err(ParseError::new(
                    ParseErrorKind::InvalidNumber("invalid digit in octal literal".into()),
                    self.span_from(start),
                ));
            }
            s.push(c);
            self.advance();
        }
        self.make_int_token(s, 8, start)
    }

    fn lex_bin_int(&mut self, start: (u32, u32)) -> Result<Token, ParseError> {
        self.advance(); // 0
        self.advance(); // b/B
        let mut s = String::new();
        while !self.is_at_end() && (self.peek_char() == '0' || self.peek_char() == '1' || self.peek_char() == '_') {
            let c = self.peek_char();
            if c != '_' {
                s.push(c);
            }
            self.advance();
        }
        self.make_int_token(s, 2, start)
    }

    fn make_int_token(&self, s: String, radix: u32, start: (u32, u32)) -> Result<Token, ParseError> {
        let clean = s.replace('_', "");
        let span = self.span_from(start);
        if clean.is_empty() || clean == "0" {
            return Ok(Token::new(TokenKind::Int(BigInt::Small(0)), span));
        }

        // Try i64 first
        match i64::from_str_radix(&clean, radix) {
            Ok(n) => Ok(Token::new(TokenKind::Int(BigInt::Small(n)), span)),
            Err(_) => {
                // Fall back to big int
                match num_bigint::BigInt::parse_bytes(clean.as_bytes(), radix) {
                    Some(n) => Ok(Token::new(TokenKind::Int(BigInt::Big(Box::new(n))), span)),
                    None => Err(ParseError::new(
                        ParseErrorKind::InvalidNumber(s),
                        span,
                    )),
                }
            }
        }
    }

    fn make_float_or_complex(&mut self, s: String, start: (u32, u32)) -> Result<Token, ParseError> {
        let clean = s.replace('_', "");
        let span = self.span_from(start);

        // Check for complex suffix
        if !self.is_at_end() && matches!(self.peek_char(), 'j' | 'J') {
            self.advance();
            let val: f64 = clean.parse().unwrap_or(0.0);
            return Ok(Token::new(TokenKind::Complex(val), span));
        }

        match clean.parse::<f64>() {
            Ok(f) => Ok(Token::new(TokenKind::Float(f), span)),
            Err(_) => Err(ParseError::new(
                ParseErrorKind::InvalidNumber(s),
                span,
            )),
        }
    }

    fn collect_digits(&mut self, s: &mut String) {
        while !self.is_at_end() && (self.peek_char().is_ascii_digit() || self.peek_char() == '_') {
            let c = self.peek_char();
            if c != '_' {
                s.push(c);
            }
            self.advance();
        }
    }

    fn collect_hex_digits(&mut self, s: &mut String) {
        while !self.is_at_end()
            && (self.peek_char().is_ascii_hexdigit() || self.peek_char() == '_')
        {
            let c = self.peek_char();
            if c != '_' {
                s.push(c);
            }
            self.advance();
        }
    }

    // ─── Identifiers ────────────────────────────────────────────────

    fn lex_identifier(&mut self) -> Result<Token, ParseError> {
        let start = self.span_start();
        let mut name = String::new();
        while !self.is_at_end() && is_id_continue(self.peek_char()) {
            name.push(self.peek_char());
            self.advance();
        }

        let span = self.span_from(start);
        let kind = TokenKind::from_keyword(&name)
            .unwrap_or(TokenKind::Name(CompactString::from(name)));
        Ok(Token::new(kind, span))
    }

    // ─── Strings ────────────────────────────────────────────────────

    fn try_lex_string_prefix(&mut self) -> Result<Option<Token>, ParseError> {
        let _start_pos = self.pos;
        let _start_line = self.line;
        let _start_col = self.col;

        let mut is_raw = false;
        let mut is_bytes = false;
        let mut is_fstring = false;

        // Consume prefix characters
        let mut prefix_len = 0;
        loop {
            if self.pos + prefix_len >= self.chars.len() {
                break;
            }
            let c = self.chars[self.pos + prefix_len];
            match c {
                'r' | 'R' if !is_raw => { is_raw = true; prefix_len += 1; }
                'b' | 'B' if !is_bytes && !is_fstring => { is_bytes = true; prefix_len += 1; }
                'f' | 'F' if !is_fstring && !is_bytes => { is_fstring = true; prefix_len += 1; }
                'u' | 'U' if prefix_len == 0 => { prefix_len += 1; }
                '\'' | '"' => break,
                _ => {
                    // Not a string prefix — treat as identifier
                    return Ok(None);
                }
            }
            if prefix_len > 2 {
                return Ok(None);
            }
        }

        if self.pos + prefix_len >= self.chars.len() {
            return Ok(None);
        }
        let next = self.chars[self.pos + prefix_len];
        if next != '\'' && next != '"' {
            return Ok(None);
        }

        // Skip prefix characters
        for _ in 0..prefix_len {
            self.advance();
        }

        // Now lex the string
        self.lex_string(is_raw, is_bytes).map(Some)
    }

    fn lex_string(&mut self, is_raw: bool, is_bytes: bool) -> Result<Token, ParseError> {
        let start = (self.line, self.col);
        let quote = self.peek_char();
        self.advance();

        // Triple-quoted string?
        let triple = if !self.is_at_end()
            && self.peek_char() == quote
            && self.peek_char_at(1) == Some(quote)
        {
            self.advance();
            self.advance();
            true
        } else {
            false
        };

        let mut content = String::new();
        loop {
            if self.is_at_end() {
                return Err(ParseError::new(
                    ParseErrorKind::UnterminatedString,
                    self.span_from(start),
                ));
            }

            let c = self.peek_char();

            if c == quote {
                if triple {
                    if self.peek_char_at(1) == Some(quote)
                        && self.peek_char_at(2) == Some(quote)
                    {
                        self.advance();
                        self.advance();
                        self.advance();
                        break;
                    }
                    content.push(c);
                    self.advance();
                } else {
                    self.advance();
                    break;
                }
            } else if c == '\\' && !is_raw {
                // Collect raw escape sequence for later processing
                content.push(c);
                self.advance();
                if !self.is_at_end() {
                    let esc = self.peek_char();
                    content.push(esc);
                    self.advance();
                    if esc == '\n' {
                        self.line += 1;
                        self.col = 0;
                    }
                }
            } else if c == '\\' && is_raw {
                content.push(c);
                self.advance();
                if !self.is_at_end() {
                    content.push(self.peek_char());
                    self.advance();
                }
            } else {
                if c == '\n' {
                    if !triple {
                        return Err(ParseError::new(
                            ParseErrorKind::UnterminatedString,
                            self.span_from(start),
                        ));
                    }
                    self.line += 1;
                    self.col = 0;
                }
                content.push(c);
                self.advance();
            }
        }

        let span = self.span_from(start);

        if is_bytes {
            if is_raw {
                let bytes = content.into_bytes();
                Ok(Token::new(TokenKind::Bytes(bytes), span))
            } else {
                let bytes = string_parser::parse_bytes_literal(&content, span)?;
                Ok(Token::new(TokenKind::Bytes(bytes), span))
            }
        } else if is_raw {
            Ok(Token::new(
                TokenKind::String(string_parser::parse_raw_string(&content)),
                span,
            ))
        } else {
            let processed = string_parser::parse_string_literal(&content, span)?;
            Ok(Token::new(TokenKind::String(processed), span))
        }
    }

    // ─── Operators ──────────────────────────────────────────────────

    fn lex_operator(&mut self) -> Result<Token, ParseError> {
        let start = self.span_start();
        let c = self.peek_char();
        self.advance();

        let kind = match c {
            '(' => { self.nesting += 1; TokenKind::LeftParen }
            ')' => { self.nesting = self.nesting.saturating_sub(1); TokenKind::RightParen }
            '[' => { self.nesting += 1; TokenKind::LeftBracket }
            ']' => { self.nesting = self.nesting.saturating_sub(1); TokenKind::RightBracket }
            '{' => { self.nesting += 1; TokenKind::LeftBrace }
            '}' => { self.nesting = self.nesting.saturating_sub(1); TokenKind::RightBrace }
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            '~' => TokenKind::Tilde,
            '+' => self.match_char('=', TokenKind::PlusEqual, TokenKind::Plus),
            '-' => {
                if !self.is_at_end() && self.peek_char() == '>' {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    self.match_char('=', TokenKind::MinusEqual, TokenKind::Minus)
                }
            }
            '*' => {
                if !self.is_at_end() && self.peek_char() == '*' {
                    self.advance();
                    self.match_char('=', TokenKind::DoubleStarEqual, TokenKind::DoubleStar)
                } else {
                    self.match_char('=', TokenKind::StarEqual, TokenKind::Star)
                }
            }
            '/' => {
                if !self.is_at_end() && self.peek_char() == '/' {
                    self.advance();
                    self.match_char('=', TokenKind::DoubleSlashEqual, TokenKind::DoubleSlash)
                } else {
                    self.match_char('=', TokenKind::SlashEqual, TokenKind::Slash)
                }
            }
            '%' => self.match_char('=', TokenKind::PercentEqual, TokenKind::Percent),
            '@' => self.match_char('=', TokenKind::AtEqual, TokenKind::At),
            '&' => self.match_char('=', TokenKind::AmpersandEqual, TokenKind::Ampersand),
            '|' => self.match_char('=', TokenKind::PipeEqual, TokenKind::Pipe),
            '^' => self.match_char('=', TokenKind::CaretEqual, TokenKind::Caret),
            '<' => {
                if !self.is_at_end() && self.peek_char() == '<' {
                    self.advance();
                    self.match_char('=', TokenKind::LeftShiftEqual, TokenKind::LeftShift)
                } else {
                    self.match_char('=', TokenKind::LessEqual, TokenKind::Less)
                }
            }
            '>' => {
                if !self.is_at_end() && self.peek_char() == '>' {
                    self.advance();
                    self.match_char('=', TokenKind::RightShiftEqual, TokenKind::RightShift)
                } else {
                    self.match_char('=', TokenKind::GreaterEqual, TokenKind::Greater)
                }
            }
            '=' => self.match_char('=', TokenKind::EqualEqual, TokenKind::Equal),
            '!' => {
                if !self.is_at_end() && self.peek_char() == '=' {
                    self.advance();
                    TokenKind::NotEqual
                } else {
                    return Err(ParseError::new(
                        ParseErrorKind::UnexpectedToken("!".into()),
                        self.span_from(start),
                    ));
                }
            }
            ':' => self.match_char('=', TokenKind::ColonEqual, TokenKind::Colon),
            '.' => TokenKind::Dot,
            _ => {
                return Err(ParseError::new(
                    ParseErrorKind::UnexpectedToken(c.to_string()),
                    self.span_from(start),
                ));
            }
        };

        Ok(Token::new(kind, self.span_from(start)))
    }

    // ─── Helpers ────────────────────────────────────────────────────

    fn match_char(&mut self, expected: char, if_match: TokenKind, if_not: TokenKind) -> TokenKind {
        if !self.is_at_end() && self.peek_char() == expected {
            self.advance();
            if_match
        } else {
            if_not
        }
    }

    fn emit_eof(&mut self) -> Result<Token, ParseError> {
        let span = self.span_here();
        // Emit remaining DEDENT tokens
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.pending.push(Token::new(TokenKind::Dedent, span));
        }
        self.pending.push(Token::new(TokenKind::Eof, span));
        self.done = true;
        // Add a final NEWLINE if the file doesn't end with one
        if !self.at_line_start {
            self.pending.push(Token::new(TokenKind::Newline, span));
        }
        // Return first pending token
        Ok(self.pending.pop().unwrap())
    }

    fn skip_spaces(&mut self) {
        while !self.is_at_end() && (self.peek_char() == ' ' || self.peek_char() == '\t') {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        while !self.is_at_end() && self.peek_char() != '\n' {
            self.advance();
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek_char(&self) -> char {
        self.chars[self.pos]
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        if self.pos < self.chars.len() {
            self.pos += 1;
            self.col += 1;
        }
    }

    fn span_start(&self) -> (u32, u32) {
        (self.line, self.col)
    }

    fn span_here(&self) -> Span {
        Span::point(self.line, self.col)
    }

    fn span_from(&self, start: (u32, u32)) -> Span {
        Span::new(start.0, start.1, self.line, self.col)
    }
}

fn is_id_start(c: char) -> bool {
    c == '_' || unicode_xid::UnicodeXID::is_xid_start(c)
}

fn is_id_continue(c: char) -> bool {
    c == '_' || unicode_xid::UnicodeXID::is_xid_continue(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_tokens() {
        let mut lexer = Lexer::new("x = 42\n");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Name(_)));
        assert!(matches!(tokens[1].kind, TokenKind::Equal));
        assert!(matches!(tokens[2].kind, TokenKind::Int(_)));
        assert!(matches!(tokens[3].kind, TokenKind::Newline));
    }

    #[test]
    fn test_string_literal() {
        let mut lexer = Lexer::new("'hello'\n");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::String(s) if s.as_str() == "hello"));
    }

    #[test]
    fn test_indentation() {
        let mut lexer = Lexer::new("if True:\n    x = 1\n");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| std::mem::discriminant(&t.kind)).collect();
        // Should contain: If, True, Colon, Newline, Indent, Name, Equal, Int, Newline, Dedent, ...
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Indent)));
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Dedent)));
    }

    #[test]
    fn test_hex_literal() {
        let mut lexer = Lexer::new("0xFF\n");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Int(BigInt::Small(255))));
    }

    #[test]
    fn test_float_literal() {
        let mut lexer = Lexer::new("3.14\n");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Float(f) if (*f - 3.14).abs() < 1e-10));
    }
}
