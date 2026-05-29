use super::*;

fn parse_char_ref(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    if bytes[0] == b'#' {
        return parse_numeric_char_ref(input);
    }

    parse_named_char_ref(input)
}

fn parse_numeric_char_ref(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let mut index = 1;
    let is_hex = matches!(bytes.get(index), Some(b'x' | b'X'));
    if is_hex {
        index += 1;
    }

    let digits_start = index;
    while let Some(&byte) = bytes.get(index) {
        let is_digit = if is_hex {
            byte.is_ascii_hexdigit()
        } else {
            byte.is_ascii_digit()
        };
        if !is_digit {
            break;
        }
        index += 1;
    }

    if index == digits_start {
        return None;
    }

    let digit_text = &input[digits_start..index];
    let mut consumed = index;
    if bytes.get(consumed) == Some(&b';') {
        consumed += 1;
    }

    let value = parse_char_ref_number(digit_text, if is_hex { 16 } else { 10 });
    Some((replace_numeric_char_ref(value), consumed))
}

fn parse_char_ref_number(digits: &str, radix: u32) -> u32 {
    let mut value = 0u32;
    for ch in digits.bytes() {
        let digit = match ch {
            b'0'..=b'9' => (ch - b'0') as u32,
            b'a'..=b'f' => (ch - b'a' + 10) as u32,
            b'A'..=b'F' => (ch - b'A' + 10) as u32,
            _ => return 0x110000,
        };
        if value > (0x110000 - digit) / radix {
            return 0x110000;
        }
        value = value * radix + digit;
    }
    value
}

fn replace_numeric_char_ref(value: u32) -> String {
    if let Some(replacement) = invalid_char_ref_replacement(value) {
        return replacement.to_string();
    }
    if (0xD800..=0xDFFF).contains(&value) || value > 0x10FFFF {
        return "\u{FFFD}".to_string();
    }
    if is_invalid_html_codepoint(value) {
        return String::new();
    }
    char::from_u32(value).unwrap_or('\u{FFFD}').to_string()
}

fn invalid_char_ref_replacement(value: u32) -> Option<&'static str> {
    match value {
        0x00 => Some("\u{FFFD}"),
        0x0D => Some("\r"),
        0x80 => Some("\u{20AC}"),
        0x81 => Some("\u{0081}"),
        0x82 => Some("\u{201A}"),
        0x83 => Some("\u{0192}"),
        0x84 => Some("\u{201E}"),
        0x85 => Some("\u{2026}"),
        0x86 => Some("\u{2020}"),
        0x87 => Some("\u{2021}"),
        0x88 => Some("\u{02C6}"),
        0x89 => Some("\u{2030}"),
        0x8A => Some("\u{0160}"),
        0x8B => Some("\u{2039}"),
        0x8C => Some("\u{0152}"),
        0x8D => Some("\u{008D}"),
        0x8E => Some("\u{017D}"),
        0x8F => Some("\u{008F}"),
        0x90 => Some("\u{0090}"),
        0x91 => Some("\u{2018}"),
        0x92 => Some("\u{2019}"),
        0x93 => Some("\u{201C}"),
        0x94 => Some("\u{201D}"),
        0x95 => Some("\u{2022}"),
        0x96 => Some("\u{2013}"),
        0x97 => Some("\u{2014}"),
        0x98 => Some("\u{02DC}"),
        0x99 => Some("\u{2122}"),
        0x9A => Some("\u{0161}"),
        0x9B => Some("\u{203A}"),
        0x9C => Some("\u{0153}"),
        0x9D => Some("\u{009D}"),
        0x9E => Some("\u{017E}"),
        0x9F => Some("\u{0178}"),
        _ => None,
    }
}

fn is_invalid_html_codepoint(value: u32) -> bool {
    matches!(
        value,
        0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F | 0xFDD0..=0xFDEF
    ) || (value & 0xFFFF == 0xFFFE || value & 0xFFFF == 0xFFFF)
}

fn parse_named_char_ref(input: &str) -> Option<(String, usize)> {
    let mut consumed = 0;
    let mut char_count = 0;

    for (index, ch) in input.char_indices() {
        if ch == ';' {
            consumed = index + 1;
            break;
        }
        if is_disallowed_named_char_ref_char(ch) || char_count == 32 {
            break;
        }
        consumed = index + ch.len_utf8();
        char_count += 1;
    }

    if consumed == 0 {
        return None;
    }

    let name = &input[..consumed];
    if let Some(replacement) = html5_entity(name) {
        return Some((replacement.to_string(), consumed));
    }

    let mut prefix_end = name.len();
    while prefix_end > 1 {
        prefix_end -= 1;
        while !name.is_char_boundary(prefix_end) {
            prefix_end -= 1;
        }
        if let Some(replacement) = html5_entity(&name[..prefix_end]) {
            return Some((format!("{}{}", replacement, &name[prefix_end..]), consumed));
        }
    }

    None
}

fn is_disallowed_named_char_ref_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\u{000C}' | ' ' | '<' | '&' | '#' | ';')
}

fn html5_entity(name: &str) -> Option<&'static str> {
    match name {
        "AElig" | "AElig;" => Some("\u{00C6}"),
        "AMP" | "AMP;" | "amp" | "amp;" => Some("&"),
        "Aacute" | "Aacute;" => Some("\u{00C1}"),
        "Acirc" | "Acirc;" => Some("\u{00C2}"),
        "Agrave" | "Agrave;" => Some("\u{00C0}"),
        "Aring" | "Aring;" => Some("\u{00C5}"),
        "Atilde" | "Atilde;" => Some("\u{00C3}"),
        "Auml" | "Auml;" => Some("\u{00C4}"),
        "COPY" | "COPY;" | "copy" | "copy;" => Some("\u{00A9}"),
        "CounterClockwiseContourIntegral;" => Some("\u{2233}"),
        "ETH" | "ETH;" => Some("\u{00D0}"),
        "Eacute" | "Eacute;" => Some("\u{00C9}"),
        "Ecirc" | "Ecirc;" => Some("\u{00CA}"),
        "Egrave" | "Egrave;" => Some("\u{00C8}"),
        "Euml" | "Euml;" => Some("\u{00CB}"),
        "GT" | "GT;" | "gt" | "gt;" => Some(">"),
        "Iacute" | "Iacute;" => Some("\u{00CD}"),
        "Icirc" | "Icirc;" => Some("\u{00CE}"),
        "Igrave" | "Igrave;" => Some("\u{00CC}"),
        "Iuml" | "Iuml;" => Some("\u{00CF}"),
        "LT" | "LT;" | "lt" | "lt;" => Some("<"),
        "Ntilde" | "Ntilde;" => Some("\u{00D1}"),
        "Oacute" | "Oacute;" => Some("\u{00D3}"),
        "Ocirc" | "Ocirc;" => Some("\u{00D4}"),
        "Ograve" | "Ograve;" => Some("\u{00D2}"),
        "Oslash" | "Oslash;" => Some("\u{00D8}"),
        "Otilde" | "Otilde;" => Some("\u{00D5}"),
        "Ouml" | "Ouml;" => Some("\u{00D6}"),
        "QUOT" | "QUOT;" | "quot" | "quot;" => Some("\""),
        "REG" | "REG;" | "reg" | "reg;" => Some("\u{00AE}"),
        "THORN" | "THORN;" => Some("\u{00DE}"),
        "Uacute" | "Uacute;" => Some("\u{00DA}"),
        "Ucirc" | "Ucirc;" => Some("\u{00DB}"),
        "Ugrave" | "Ugrave;" => Some("\u{00D9}"),
        "Uuml" | "Uuml;" => Some("\u{00DC}"),
        "Yacute" | "Yacute;" => Some("\u{00DD}"),
        "aacute" | "aacute;" => Some("\u{00E1}"),
        "acE;" => Some("\u{223E}\u{0333}"),
        "acirc" | "acirc;" => Some("\u{00E2}"),
        "acute" | "acute;" => Some("\u{00B4}"),
        "aelig" | "aelig;" => Some("\u{00E6}"),
        "agrave" | "agrave;" => Some("\u{00E0}"),
        "alpha;" => Some("\u{03B1}"),
        "apos;" => Some("'"),
        "aring" | "aring;" => Some("\u{00E5}"),
        "atilde" | "atilde;" => Some("\u{00E3}"),
        "auml" | "auml;" => Some("\u{00E4}"),
        "brvbar" | "brvbar;" => Some("\u{00A6}"),
        "ccedil" | "ccedil;" => Some("\u{00E7}"),
        "cedil" | "cedil;" => Some("\u{00B8}"),
        "cent" | "cent;" => Some("\u{00A2}"),
        "curren" | "curren;" => Some("\u{00A4}"),
        "deg" | "deg;" => Some("\u{00B0}"),
        "divide" | "divide;" => Some("\u{00F7}"),
        "eacute" | "eacute;" => Some("\u{00E9}"),
        "ecirc" | "ecirc;" => Some("\u{00EA}"),
        "egrave" | "egrave;" => Some("\u{00E8}"),
        "eth" | "eth;" => Some("\u{00F0}"),
        "euml" | "euml;" => Some("\u{00EB}"),
        "frac12" | "frac12;" => Some("\u{00BD}"),
        "frac14" | "frac14;" => Some("\u{00BC}"),
        "frac34" | "frac34;" => Some("\u{00BE}"),
        "iacute" | "iacute;" => Some("\u{00ED}"),
        "icirc" | "icirc;" => Some("\u{00EE}"),
        "iexcl" | "iexcl;" => Some("\u{00A1}"),
        "igrave" | "igrave;" => Some("\u{00EC}"),
        "iquest" | "iquest;" => Some("\u{00BF}"),
        "iuml" | "iuml;" => Some("\u{00EF}"),
        "laquo" | "laquo;" => Some("\u{00AB}"),
        "macr" | "macr;" => Some("\u{00AF}"),
        "micro" | "micro;" => Some("\u{00B5}"),
        "middot" | "middot;" => Some("\u{00B7}"),
        "nbsp" | "nbsp;" => Some("\u{00A0}"),
        "not" | "not;" => Some("\u{00AC}"),
        "notin;" => Some("\u{2209}"),
        "ntilde" | "ntilde;" => Some("\u{00F1}"),
        "oacute" | "oacute;" => Some("\u{00F3}"),
        "ocirc" | "ocirc;" => Some("\u{00F4}"),
        "ograve" | "ograve;" => Some("\u{00F2}"),
        "ordf" | "ordf;" => Some("\u{00AA}"),
        "ordm" | "ordm;" => Some("\u{00BA}"),
        "oslash" | "oslash;" => Some("\u{00F8}"),
        "otilde" | "otilde;" => Some("\u{00F5}"),
        "ouml" | "ouml;" => Some("\u{00F6}"),
        "para" | "para;" => Some("\u{00B6}"),
        "plusmn" | "plusmn;" => Some("\u{00B1}"),
        "pound" | "pound;" => Some("\u{00A3}"),
        "raquo" | "raquo;" => Some("\u{00BB}"),
        "sect" | "sect;" => Some("\u{00A7}"),
        "shy" | "shy;" => Some("\u{00AD}"),
        "sup1" | "sup1;" => Some("\u{00B9}"),
        "sup2" | "sup2;" => Some("\u{00B2}"),
        "sup3" | "sup3;" => Some("\u{00B3}"),
        "szlig" | "szlig;" => Some("\u{00DF}"),
        "thorn" | "thorn;" => Some("\u{00FE}"),
        "times" | "times;" => Some("\u{00D7}"),
        "uacute" | "uacute;" => Some("\u{00FA}"),
        "ucirc" | "ucirc;" => Some("\u{00FB}"),
        "ugrave" | "ugrave;" => Some("\u{00F9}"),
        "uml" | "uml;" => Some("\u{00A8}"),
        "uuml" | "uuml;" => Some("\u{00FC}"),
        "yacute" | "yacute;" => Some("\u{00FD}"),
        "yen" | "yen;" => Some("\u{00A5}"),
        "yuml" | "yuml;" => Some("\u{00FF}"),
        _ => None,
    }
}

pub fn create_html_module() -> PyObjectRef {
    fn html_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("html.escape requires 1 argument"));
        }
        let s = args[0].py_to_string();
        let quote = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Bool(b) => *b,
                _ => true,
            }
        } else {
            true
        };
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' if quote => out.push_str("&quot;"),
                '\'' if quote => out.push_str("&#x27;"),
                _ => out.push(c),
            }
        }
        Ok(PyObject::str_val(CompactString::from(out)))
    }

    fn html_unescape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("html.unescape requires 1 argument"));
        }
        let s = args[0].py_to_string();
        if !s.contains('&') {
            return Ok(PyObject::str_val(CompactString::from(s)));
        }

        let mut result = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] != b'&' {
                let ch = s[i..].chars().next().unwrap();
                result.push(ch);
                i += ch.len_utf8();
                continue;
            }

            if let Some((replacement, consumed)) = parse_char_ref(&s[i + 1..]) {
                result.push_str(&replacement);
                i += 1 + consumed;
            } else {
                result.push('&');
                i += 1;
            }
        }

        Ok(PyObject::str_val(CompactString::from(result)))
    }

    // _replace_charref is internal CPython — used by html.parser and some libs
    let replace_charref = make_builtin(|args: &[PyObjectRef]| {
        // _replace_charref(s) — replace HTML character references in string
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        let s = args[0].py_to_string();
        // Simple passthrough — mistune uses re.sub with this
        Ok(PyObject::str_val(CompactString::from(s)))
    });

    make_module(
        "html",
        vec![
            ("escape", make_builtin(html_escape)),
            ("unescape", make_builtin(html_unescape)),
            ("_replace_charref", replace_charref),
        ],
    )
}
