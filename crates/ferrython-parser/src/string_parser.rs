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
                    // \N{name} — unicode name lookup
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        let mut name = String::new();
                        while let Some(c) = chars.next() {
                            if c == '}' {
                                break;
                            }
                            name.push(c);
                        }
                        match unicode_name_to_char(&name) {
                            Some(c) => result.push(c),
                            None => {
                                return Err(ParseError::new(
                                    ParseErrorKind::InvalidSyntax(
                                        format!("unknown Unicode character name: {}", name),
                                    ),
                                    span,
                                ));
                            }
                        }
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
                Some(c @ '0'..='7') => {
                    // Octal escape in bytes literal
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
                    if let Ok(n) = u8::from_str_radix(&octal, 8) {
                        result.push(n);
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

/// Look up a Unicode character by its standard name (e.g., "SNOWMAN" → '☃').
///
/// Covers the most commonly used names. For full coverage, a crate like
/// `unicode_names2` could be used, but this table handles the cases that
/// typically appear in real Python code.
fn unicode_name_to_char(name: &str) -> Option<char> {
    let name_upper = name.to_ascii_uppercase();
    // Fast path: common names used in Python code and test suites
    match name_upper.as_str() {
        // Latin supplement & extensions
        "LATIN SMALL LETTER SHARP S" => Some('\u{00DF}'),
        "LATIN SMALL LETTER A WITH GRAVE" => Some('\u{00E0}'),
        "LATIN SMALL LETTER E WITH ACUTE" => Some('\u{00E9}'),
        "LATIN SMALL LETTER N WITH TILDE" => Some('\u{00F1}'),
        "LATIN SMALL LETTER U WITH DIAERESIS" => Some('\u{00FC}'),
        "LATIN CAPITAL LETTER A WITH RING ABOVE" => Some('\u{00C5}'),
        "LATIN CAPITAL LETTER E WITH ACUTE" => Some('\u{00C9}'),
        // Punctuation & symbols
        "NO-BREAK SPACE" => Some('\u{00A0}'),
        "INVERTED EXCLAMATION MARK" => Some('\u{00A1}'),
        "CENT SIGN" => Some('\u{00A2}'),
        "POUND SIGN" => Some('\u{00A3}'),
        "CURRENCY SIGN" => Some('\u{00A4}'),
        "YEN SIGN" => Some('\u{00A5}'),
        "SECTION SIGN" => Some('\u{00A7}'),
        "COPYRIGHT SIGN" => Some('\u{00A9}'),
        "REGISTERED SIGN" => Some('\u{00AE}'),
        "DEGREE SIGN" => Some('\u{00B0}'),
        "PLUS-MINUS SIGN" => Some('\u{00B1}'),
        "MICRO SIGN" => Some('\u{00B5}'),
        "PILCROW SIGN" => Some('\u{00B6}'),
        "MIDDLE DOT" => Some('\u{00B7}'),
        "MULTIPLICATION SIGN" => Some('\u{00D7}'),
        "DIVISION SIGN" => Some('\u{00F7}'),
        // Greek letters (commonly used in math/science)
        "GREEK CAPITAL LETTER DELTA" => Some('\u{0394}'),
        "GREEK CAPITAL LETTER SIGMA" => Some('\u{03A3}'),
        "GREEK CAPITAL LETTER OMEGA" => Some('\u{03A9}'),
        "GREEK SMALL LETTER ALPHA" => Some('\u{03B1}'),
        "GREEK SMALL LETTER BETA" => Some('\u{03B2}'),
        "GREEK SMALL LETTER GAMMA" => Some('\u{03B3}'),
        "GREEK SMALL LETTER DELTA" => Some('\u{03B4}'),
        "GREEK SMALL LETTER EPSILON" => Some('\u{03B5}'),
        "GREEK SMALL LETTER PI" => Some('\u{03C0}'),
        "GREEK SMALL LETTER SIGMA" => Some('\u{03C3}'),
        "GREEK SMALL LETTER OMEGA" => Some('\u{03C9}'),
        // Arrows & math
        "LEFTWARDS ARROW" => Some('\u{2190}'),
        "UPWARDS ARROW" => Some('\u{2191}'),
        "RIGHTWARDS ARROW" => Some('\u{2192}'),
        "DOWNWARDS ARROW" => Some('\u{2193}'),
        "LEFT RIGHT ARROW" => Some('\u{2194}'),
        "INFINITY" => Some('\u{221E}'),
        "ALMOST EQUAL TO" => Some('\u{2248}'),
        "NOT EQUAL TO" => Some('\u{2260}'),
        "LESS-THAN OR EQUAL TO" => Some('\u{2264}'),
        "GREATER-THAN OR EQUAL TO" => Some('\u{2265}'),
        // Dingbats & emoji building blocks
        "SNOWMAN" => Some('\u{2603}'),
        "SNOWFLAKE" => Some('\u{2744}'),
        "CHECK MARK" => Some('\u{2713}'),
        "HEAVY CHECK MARK" => Some('\u{2714}'),
        "BALLOT X" => Some('\u{2717}'),
        "HEAVY BALLOT X" => Some('\u{2718}'),
        "BLACK STAR" => Some('\u{2605}'),
        "WHITE STAR" => Some('\u{2606}'),
        "BLACK HEART SUIT" => Some('\u{2665}'),
        "BLACK DIAMOND SUIT" => Some('\u{2666}'),
        // Common CJK & misc
        "EM DASH" => Some('\u{2014}'),
        "EN DASH" => Some('\u{2013}'),
        "BULLET" => Some('\u{2022}'),
        "HORIZONTAL ELLIPSIS" => Some('\u{2026}'),
        "TRADE MARK SIGN" => Some('\u{2122}'),
        "LEFT SINGLE QUOTATION MARK" => Some('\u{2018}'),
        "RIGHT SINGLE QUOTATION MARK" => Some('\u{2019}'),
        "LEFT DOUBLE QUOTATION MARK" => Some('\u{201C}'),
        "RIGHT DOUBLE QUOTATION MARK" => Some('\u{201D}'),
        "EURO SIGN" => Some('\u{20AC}'),
        // Box drawing (used in TUI)
        "BOX DRAWINGS LIGHT HORIZONTAL" => Some('\u{2500}'),
        "BOX DRAWINGS LIGHT VERTICAL" => Some('\u{2502}'),
        "BOX DRAWINGS LIGHT DOWN AND RIGHT" => Some('\u{250C}'),
        "BOX DRAWINGS LIGHT DOWN AND LEFT" => Some('\u{2510}'),
        "BOX DRAWINGS LIGHT UP AND RIGHT" => Some('\u{2514}'),
        "BOX DRAWINGS LIGHT UP AND LEFT" => Some('\u{2518}'),
        // Whitespace & control
        "SPACE" => Some(' '),
        "LINE FEED" | "LINE FEED (LF)" => Some('\n'),
        "CARRIAGE RETURN" | "CARRIAGE RETURN (CR)" => Some('\r'),
        "CHARACTER TABULATION" | "HORIZONTAL TABULATION" => Some('\t'),
        "NULL" => Some('\0'),
        "REPLACEMENT CHARACTER" => Some('\u{FFFD}'),
        "BYTE ORDER MARK" | "ZERO WIDTH NO-BREAK SPACE" => Some('\u{FEFF}'),
        "FORM FEED" | "FORM FEED (FF)" => Some('\u{000C}'),
        "NEXT LINE" | "NEXT LINE (NEL)" => Some('\u{0085}'),
        "SOFT HYPHEN" => Some('\u{00AD}'),
        "ZERO WIDTH SPACE" => Some('\u{200B}'),
        "ZERO WIDTH JOINER" => Some('\u{200D}'),
        "ZERO WIDTH NON-JOINER" => Some('\u{200C}'),
        "LEFT-TO-RIGHT MARK" => Some('\u{200E}'),
        "RIGHT-TO-LEFT MARK" => Some('\u{200F}'),
        "WORD JOINER" => Some('\u{2060}'),
        "OBJECT REPLACEMENT CHARACTER" => Some('\u{FFFC}'),
        _ => None,
    }
}
