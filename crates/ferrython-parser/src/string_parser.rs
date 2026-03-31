//! String literal parsing — handles escape sequences, raw strings, byte strings.

use compact_str::CompactString;
use crate::error::{ParseError, ParseErrorKind};
use crate::token::Span;

/// Parse a regular string literal (process escape sequences).
pub fn parse_string_literal(s: &str, span: Span) -> Result<CompactString, ParseError> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('f') => result.push('\x0C'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('v') => result.push('\x0B'),
                Some('0') => result.push('\0'),
                Some('x') => {
                    let hex = take_n(&mut chars, 2);
                    match u32::from_str_radix(&hex, 16) {
                        Ok(n) => {
                            if let Some(c) = char::from_u32(n) {
                                result.push(c);
                            } else {
                                return Err(ParseError::new(
                                    ParseErrorKind::InvalidEscape('x'),
                                    span,
                                ));
                            }
                        }
                        Err(_) => {
                            return Err(ParseError::new(
                                ParseErrorKind::InvalidEscape('x'),
                                span,
                            ));
                        }
                    }
                }
                Some('u') => {
                    let hex = take_n(&mut chars, 4);
                    match u32::from_str_radix(&hex, 16) {
                        Ok(n) => {
                            if let Some(c) = char::from_u32(n) {
                                result.push(c);
                            } else {
                                return Err(ParseError::new(
                                    ParseErrorKind::InvalidEscape('u'),
                                    span,
                                ));
                            }
                        }
                        Err(_) => {
                            return Err(ParseError::new(
                                ParseErrorKind::InvalidEscape('u'),
                                span,
                            ));
                        }
                    }
                }
                Some('U') => {
                    let hex = take_n(&mut chars, 8);
                    match u32::from_str_radix(&hex, 16) {
                        Ok(n) => {
                            if let Some(c) = char::from_u32(n) {
                                result.push(c);
                            } else {
                                return Err(ParseError::new(
                                    ParseErrorKind::InvalidEscape('U'),
                                    span,
                                ));
                            }
                        }
                        Err(_) => {
                            return Err(ParseError::new(
                                ParseErrorKind::InvalidEscape('U'),
                                span,
                            ));
                        }
                    }
                }
                Some('N') => {
                    // \N{name} — unicode name lookup (simplified: skip for now)
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        while let Some(c) = chars.next() {
                            if c == '}' {
                                break;
                            }
                        }
                        // TODO: implement unicode name lookup
                        result.push('\u{FFFD}'); // replacement char placeholder
                    } else {
                        result.push('\\');
                        result.push('N');
                    }
                }
                Some(c @ '0'..='7') => {
                    // Octal escape
                    let mut octal = String::from(c);
                    for _ in 0..2 {
                        if let Some(&next) = chars.peek() {
                            if next.is_ascii_digit() && next < '8' {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(n) = u32::from_str_radix(&octal, 8) {
                        if let Some(c) = char::from_u32(n) {
                            result.push(c);
                        }
                    }
                }
                Some('\n') => {
                    // Line continuation — skip the newline
                }
                Some(other) => {
                    // Unknown escape — in Python 3.8 this is a DeprecationWarning
                    result.push('\\');
                    result.push(other);
                }
                None => {
                    result.push('\\');
                }
            }
        } else {
            result.push(c);
        }
    }
    Ok(CompactString::from(result))
}

/// Parse a raw string literal (no escape processing).
pub fn parse_raw_string(s: &str) -> CompactString {
    CompactString::from(s)
}

/// Parse a bytes literal.
pub fn parse_bytes_literal(s: &str, span: Span) -> Result<Vec<u8>, ParseError> {
    let mut result = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push(b'\\'),
                Some('\'') => result.push(b'\''),
                Some('"') => result.push(b'"'),
                Some('a') => result.push(0x07),
                Some('b') => result.push(0x08),
                Some('f') => result.push(0x0C),
                Some('n') => result.push(b'\n'),
                Some('r') => result.push(b'\r'),
                Some('t') => result.push(b'\t'),
                Some('v') => result.push(0x0B),
                Some('0') => result.push(0),
                Some('x') => {
                    let hex = take_n(&mut chars, 2);
                    match u8::from_str_radix(&hex, 16) {
                        Ok(n) => result.push(n),
                        Err(_) => {
                            return Err(ParseError::new(
                                ParseErrorKind::InvalidEscape('x'),
                                span,
                            ));
                        }
                    }
                }
                Some(other) => {
                    result.push(b'\\');
                    if other.is_ascii() {
                        result.push(other as u8);
                    }
                }
                None => result.push(b'\\'),
            }
        } else if c.is_ascii() {
            result.push(c as u8);
        } else {
            return Err(ParseError::new(
                ParseErrorKind::InvalidSyntax(
                    "bytes can only contain ASCII literal characters".into(),
                ),
                span,
            ));
        }
    }
    Ok(result)
}

fn take_n(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, n: usize) -> String {
    let mut s = String::with_capacity(n);
    for _ in 0..n {
        if let Some(c) = chars.next() {
            s.push(c);
        }
    }
    s
}
