use super::*;

// ── unicodedata module──

pub fn create_unicodedata_module() -> PyObjectRef {
    let name_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.name", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cp = ch as u32;
        let name = unicode_char_name(ch, cp);
        if name.is_empty() {
            if args.len() > 1 {
                return Ok(args[1].clone());
            }
            return Err(PyException::value_error("no such name"));
        }
        Ok(PyObject::str_val(CompactString::from(name)))
    });

    let lookup_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.lookup", args, 1)?;
        let name = args[0].py_to_string().to_uppercase();
        match unicode_lookup_name(&name) {
            Some(ch) => Ok(PyObject::str_val(CompactString::from(
                ch.to_string().as_str(),
            ))),
            None => Err(PyException::key_error(format!(
                "undefined character name '{}'",
                name
            ))),
        }
    });

    let category_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.category", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cat = unicode_category(ch);
        Ok(PyObject::str_val(CompactString::from(cat)))
    });

    let numeric_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.numeric", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::float(d as f64))
        } else if ch == '\u{00BD}' {
            Ok(PyObject::float(0.5))
        } else if ch == '\u{2153}' {
            Ok(PyObject::float(1.0 / 3.0))
        } else if ch == '\u{00BC}' {
            Ok(PyObject::float(0.25))
        } else if ch == '\u{00BE}' {
            Ok(PyObject::float(0.75))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a numeric character"))
        }
    });

    let decimal_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.decimal", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::int(d as i64))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a decimal character"))
        }
    });

    let digit_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.digit", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::int(d as i64))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a digit character"))
        }
    });

    let bidirectional_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.bidirectional", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let bidi = if ch.is_ascii_alphabetic() {
            "L"
        } else if ch.is_ascii_digit() {
            "EN"
        } else if ch == ' ' || ch == '\t' {
            "WS"
        } else if ch.is_ascii_punctuation() {
            "ON"
        } else if ch.is_ascii_control() {
            "BN"
        } else if ch.is_alphabetic() {
            "L"
        } else {
            "ON"
        };
        Ok(PyObject::str_val(CompactString::from(bidi)))
    });

    let combining_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.combining", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let ccc = if ('\u{0300}'..='\u{036F}').contains(&ch) {
            // Combining Diacritical Marks — approximate canonical combining class
            match ch {
                '\u{0300}' | '\u{0301}' | '\u{0302}' | '\u{0303}' => 230,
                '\u{0327}' | '\u{0328}' => 202,
                '\u{0338}' => 1,
                _ => 230,
            }
        } else {
            0
        };
        Ok(PyObject::int(ccc))
    });

    let east_asian_width_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.east_asian_width", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cp = ch as u32;
        let w = if cp <= 0x007F {
            "Na" // Narrow (ASCII)
        } else if (0x1100..=0x115F).contains(&cp)
            || (0x2E80..=0x303E).contains(&cp)
            || (0x3040..=0x9FFF).contains(&cp)
            || (0xAC00..=0xD7AF).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp)
            || (0xFE10..=0xFE6F).contains(&cp)
            || (0xFF01..=0xFF60).contains(&cp)
            || (0xFFE0..=0xFFE6).contains(&cp)
            || (0x20000..=0x2FFFF).contains(&cp)
            || (0x30000..=0x3FFFF).contains(&cp)
        {
            "W" // Wide
        } else if (0xFF61..=0xFFDC).contains(&cp) || (0xFFE8..=0xFFEE).contains(&cp) {
            "H" // Halfwidth
        } else if (0x0080..=0x00FF).contains(&cp) {
            "N" // Neutral
        } else {
            "N"
        };
        Ok(PyObject::str_val(CompactString::from(w)))
    });

    let mirrored_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.mirrored", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let m = matches!(
            ch,
            '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '\u{00AB}' | '\u{00BB}'
        );
        Ok(PyObject::int(if m { 1 } else { 0 }))
    });

    let normalize_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.normalize", args, 2)?;
        let _form = args[0].py_to_string().to_uppercase();
        let s = args[1].py_to_string();
        // For ASCII-only strings all normalization forms are identity
        // For non-ASCII, apply basic decomposition/composition
        if s.is_ascii() {
            return Ok(args[1].clone());
        }
        // Handle common normalization cases
        match _form.as_str() {
            "NFC" | "NFKC" => {
                // Compose: replace common decomposed sequences
                let result = nfc_compose(&s);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            "NFD" | "NFKD" => {
                // Decompose: expand composed characters
                let result = nfd_decompose(&s);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            _ => Ok(args[1].clone()),
        }
    });

    make_module(
        "unicodedata",
        vec![
            ("name", name_fn),
            ("lookup", lookup_fn),
            ("category", category_fn),
            ("numeric", numeric_fn),
            ("decimal", decimal_fn),
            ("digit", digit_fn),
            ("bidirectional", bidirectional_fn),
            ("combining", combining_fn),
            ("east_asian_width", east_asian_width_fn),
            ("mirrored", mirrored_fn),
            ("normalize", normalize_fn),
            (
                "unidata_version",
                PyObject::str_val(CompactString::from("15.0.0")),
            ),
        ],
    )
}

/// Return the Unicode name for a character, or empty string if unknown.
fn unicode_char_name(ch: char, cp: u32) -> String {
    if let Some(name) = unicode_names2::name(ch) {
        return name.to_string();
    }
    // ASCII letters
    if ch.is_ascii_uppercase() {
        return format!("LATIN CAPITAL LETTER {}", ch);
    }
    if ch.is_ascii_lowercase() {
        return format!(
            "LATIN SMALL LETTER {}",
            ch.to_uppercase().next().unwrap_or(ch)
        );
    }
    if ch.is_ascii_digit() {
        let digit_names = [
            "ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE",
        ];
        return format!("DIGIT {}", digit_names[(ch as u8 - b'0') as usize]);
    }
    // Common ASCII punctuation and symbols
    match ch {
        ' ' => "SPACE".to_string(),
        '!' => "EXCLAMATION MARK".to_string(),
        '"' => "QUOTATION MARK".to_string(),
        '#' => "NUMBER SIGN".to_string(),
        '$' => "DOLLAR SIGN".to_string(),
        '%' => "PERCENT SIGN".to_string(),
        '&' => "AMPERSAND".to_string(),
        '\'' => "APOSTROPHE".to_string(),
        '(' => "LEFT PARENTHESIS".to_string(),
        ')' => "RIGHT PARENTHESIS".to_string(),
        '*' => "ASTERISK".to_string(),
        '+' => "PLUS SIGN".to_string(),
        ',' => "COMMA".to_string(),
        '-' => "HYPHEN-MINUS".to_string(),
        '.' => "FULL STOP".to_string(),
        '/' => "SOLIDUS".to_string(),
        ':' => "COLON".to_string(),
        ';' => "SEMICOLON".to_string(),
        '<' => "LESS-THAN SIGN".to_string(),
        '=' => "EQUALS SIGN".to_string(),
        '>' => "GREATER-THAN SIGN".to_string(),
        '?' => "QUESTION MARK".to_string(),
        '@' => "COMMERCIAL AT".to_string(),
        '[' => "LEFT SQUARE BRACKET".to_string(),
        '\\' => "REVERSE SOLIDUS".to_string(),
        ']' => "RIGHT SQUARE BRACKET".to_string(),
        '^' => "CIRCUMFLEX ACCENT".to_string(),
        '_' => "LOW LINE".to_string(),
        '`' => "GRAVE ACCENT".to_string(),
        '{' => "LEFT CURLY BRACKET".to_string(),
        '|' => "VERTICAL LINE".to_string(),
        '}' => "RIGHT CURLY BRACKET".to_string(),
        '~' => "TILDE".to_string(),
        '\t' => "CHARACTER TABULATION".to_string(),
        '\n' => "LINE FEED".to_string(),
        '\r' => "CARRIAGE RETURN".to_string(),
        // Common non-ASCII characters
        '\u{00A0}' => "NO-BREAK SPACE".to_string(),
        '\u{00A9}' => "COPYRIGHT SIGN".to_string(),
        '\u{00AE}' => "REGISTERED SIGN".to_string(),
        '\u{00B0}' => "DEGREE SIGN".to_string(),
        '\u{00B1}' => "PLUS-MINUS SIGN".to_string(),
        '\u{00B5}' => "MICRO SIGN".to_string(),
        '\u{00B7}' => "MIDDLE DOT".to_string(),
        '\u{00BC}' => "VULGAR FRACTION ONE QUARTER".to_string(),
        '\u{00BD}' => "VULGAR FRACTION ONE HALF".to_string(),
        '\u{00BE}' => "VULGAR FRACTION THREE QUARTERS".to_string(),
        '\u{00BF}' => "INVERTED QUESTION MARK".to_string(),
        '\u{00D7}' => "MULTIPLICATION SIGN".to_string(),
        '\u{00F7}' => "DIVISION SIGN".to_string(),
        // Latin Extended-A common
        '\u{0100}' => "LATIN CAPITAL LETTER A WITH MACRON".to_string(),
        '\u{0101}' => "LATIN SMALL LETTER A WITH MACRON".to_string(),
        // Greek letters
        '\u{0391}' => "GREEK CAPITAL LETTER ALPHA".to_string(),
        '\u{0392}' => "GREEK CAPITAL LETTER BETA".to_string(),
        '\u{0393}' => "GREEK CAPITAL LETTER GAMMA".to_string(),
        '\u{0394}' => "GREEK CAPITAL LETTER DELTA".to_string(),
        '\u{0395}' => "GREEK CAPITAL LETTER EPSILON".to_string(),
        '\u{0396}' => "GREEK CAPITAL LETTER ZETA".to_string(),
        '\u{0397}' => "GREEK CAPITAL LETTER ETA".to_string(),
        '\u{0398}' => "GREEK CAPITAL LETTER THETA".to_string(),
        '\u{0399}' => "GREEK CAPITAL LETTER IOTA".to_string(),
        '\u{039A}' => "GREEK CAPITAL LETTER KAPPA".to_string(),
        '\u{039B}' => "GREEK CAPITAL LETTER LAMDA".to_string(),
        '\u{039C}' => "GREEK CAPITAL LETTER MU".to_string(),
        '\u{039D}' => "GREEK CAPITAL LETTER NU".to_string(),
        '\u{039E}' => "GREEK CAPITAL LETTER XI".to_string(),
        '\u{039F}' => "GREEK CAPITAL LETTER OMICRON".to_string(),
        '\u{03A0}' => "GREEK CAPITAL LETTER PI".to_string(),
        '\u{03A1}' => "GREEK CAPITAL LETTER RHO".to_string(),
        '\u{03A3}' => "GREEK CAPITAL LETTER SIGMA".to_string(),
        '\u{03A4}' => "GREEK CAPITAL LETTER TAU".to_string(),
        '\u{03A5}' => "GREEK CAPITAL LETTER UPSILON".to_string(),
        '\u{03A6}' => "GREEK CAPITAL LETTER PHI".to_string(),
        '\u{03A7}' => "GREEK CAPITAL LETTER CHI".to_string(),
        '\u{03A8}' => "GREEK CAPITAL LETTER PSI".to_string(),
        '\u{03A9}' => "GREEK CAPITAL LETTER OMEGA".to_string(),
        '\u{03B1}' => "GREEK SMALL LETTER ALPHA".to_string(),
        '\u{03B2}' => "GREEK SMALL LETTER BETA".to_string(),
        '\u{03B3}' => "GREEK SMALL LETTER GAMMA".to_string(),
        '\u{03B4}' => "GREEK SMALL LETTER DELTA".to_string(),
        '\u{03B5}' => "GREEK SMALL LETTER EPSILON".to_string(),
        '\u{03B6}' => "GREEK SMALL LETTER ZETA".to_string(),
        '\u{03B7}' => "GREEK SMALL LETTER ETA".to_string(),
        '\u{03B8}' => "GREEK SMALL LETTER THETA".to_string(),
        '\u{03B9}' => "GREEK SMALL LETTER IOTA".to_string(),
        '\u{03BA}' => "GREEK SMALL LETTER KAPPA".to_string(),
        '\u{03BB}' => "GREEK SMALL LETTER LAMDA".to_string(),
        '\u{03BC}' => "GREEK SMALL LETTER MU".to_string(),
        '\u{03BD}' => "GREEK SMALL LETTER NU".to_string(),
        '\u{03BE}' => "GREEK SMALL LETTER XI".to_string(),
        '\u{03BF}' => "GREEK SMALL LETTER OMICRON".to_string(),
        '\u{03C0}' => "GREEK SMALL LETTER PI".to_string(),
        '\u{03C1}' => "GREEK SMALL LETTER RHO".to_string(),
        '\u{03C3}' => "GREEK SMALL LETTER SIGMA".to_string(),
        '\u{03C4}' => "GREEK SMALL LETTER TAU".to_string(),
        '\u{03C5}' => "GREEK SMALL LETTER UPSILON".to_string(),
        '\u{03C6}' => "GREEK SMALL LETTER PHI".to_string(),
        '\u{03C7}' => "GREEK SMALL LETTER CHI".to_string(),
        '\u{03C8}' => "GREEK SMALL LETTER PSI".to_string(),
        '\u{03C9}' => "GREEK SMALL LETTER OMEGA".to_string(),
        // Common symbols
        '\u{2013}' => "EN DASH".to_string(),
        '\u{2014}' => "EM DASH".to_string(),
        '\u{2018}' => "LEFT SINGLE QUOTATION MARK".to_string(),
        '\u{2019}' => "RIGHT SINGLE QUOTATION MARK".to_string(),
        '\u{201C}' => "LEFT DOUBLE QUOTATION MARK".to_string(),
        '\u{201D}' => "RIGHT DOUBLE QUOTATION MARK".to_string(),
        '\u{2022}' => "BULLET".to_string(),
        '\u{2026}' => "HORIZONTAL ELLIPSIS".to_string(),
        '\u{2030}' => "PER MILLE SIGN".to_string(),
        '\u{2032}' => "PRIME".to_string(),
        '\u{2033}' => "DOUBLE PRIME".to_string(),
        '\u{20AC}' => "EURO SIGN".to_string(),
        '\u{2122}' => "TRADE MARK SIGN".to_string(),
        '\u{2190}' => "LEFTWARDS ARROW".to_string(),
        '\u{2191}' => "UPWARDS ARROW".to_string(),
        '\u{2192}' => "RIGHTWARDS ARROW".to_string(),
        '\u{2193}' => "DOWNWARDS ARROW".to_string(),
        '\u{2200}' => "FOR ALL".to_string(),
        '\u{2202}' => "PARTIAL DIFFERENTIAL".to_string(),
        '\u{2203}' => "THERE EXISTS".to_string(),
        '\u{2205}' => "EMPTY SET".to_string(),
        '\u{2207}' => "NABLA".to_string(),
        '\u{2208}' => "ELEMENT OF".to_string(),
        '\u{2211}' => "N-ARY SUMMATION".to_string(),
        '\u{221A}' => "SQUARE ROOT".to_string(),
        '\u{221E}' => "INFINITY".to_string(),
        '\u{2227}' => "LOGICAL AND".to_string(),
        '\u{2228}' => "LOGICAL OR".to_string(),
        '\u{2229}' => "INTERSECTION".to_string(),
        '\u{222A}' => "UNION".to_string(),
        '\u{222B}' => "INTEGRAL".to_string(),
        '\u{2248}' => "ALMOST EQUAL TO".to_string(),
        '\u{2260}' => "NOT EQUAL TO".to_string(),
        '\u{2264}' => "LESS-THAN OR EQUAL TO".to_string(),
        '\u{2265}' => "GREATER-THAN OR EQUAL TO".to_string(),
        // CJK common
        '\u{3000}' => "IDEOGRAPHIC SPACE".to_string(),
        '\u{3001}' => "IDEOGRAPHIC COMMA".to_string(),
        '\u{3002}' => "IDEOGRAPHIC FULL STOP".to_string(),
        // Latin-1 supplement letters
        '\u{00C0}' => "LATIN CAPITAL LETTER A WITH GRAVE".to_string(),
        '\u{00C1}' => "LATIN CAPITAL LETTER A WITH ACUTE".to_string(),
        '\u{00C2}' => "LATIN CAPITAL LETTER A WITH CIRCUMFLEX".to_string(),
        '\u{00C3}' => "LATIN CAPITAL LETTER A WITH TILDE".to_string(),
        '\u{00C4}' => "LATIN CAPITAL LETTER A WITH DIAERESIS".to_string(),
        '\u{00C5}' => "LATIN CAPITAL LETTER A WITH RING ABOVE".to_string(),
        '\u{00C6}' => "LATIN CAPITAL LETTER AE".to_string(),
        '\u{00C7}' => "LATIN CAPITAL LETTER C WITH CEDILLA".to_string(),
        '\u{00C8}' => "LATIN CAPITAL LETTER E WITH GRAVE".to_string(),
        '\u{00C9}' => "LATIN CAPITAL LETTER E WITH ACUTE".to_string(),
        '\u{00CA}' => "LATIN CAPITAL LETTER E WITH CIRCUMFLEX".to_string(),
        '\u{00CB}' => "LATIN CAPITAL LETTER E WITH DIAERESIS".to_string(),
        '\u{00CC}' => "LATIN CAPITAL LETTER I WITH GRAVE".to_string(),
        '\u{00CD}' => "LATIN CAPITAL LETTER I WITH ACUTE".to_string(),
        '\u{00CE}' => "LATIN CAPITAL LETTER I WITH CIRCUMFLEX".to_string(),
        '\u{00CF}' => "LATIN CAPITAL LETTER I WITH DIAERESIS".to_string(),
        '\u{00D0}' => "LATIN CAPITAL LETTER ETH".to_string(),
        '\u{00D1}' => "LATIN CAPITAL LETTER N WITH TILDE".to_string(),
        '\u{00D2}' => "LATIN CAPITAL LETTER O WITH GRAVE".to_string(),
        '\u{00D3}' => "LATIN CAPITAL LETTER O WITH ACUTE".to_string(),
        '\u{00D4}' => "LATIN CAPITAL LETTER O WITH CIRCUMFLEX".to_string(),
        '\u{00D5}' => "LATIN CAPITAL LETTER O WITH TILDE".to_string(),
        '\u{00D6}' => "LATIN CAPITAL LETTER O WITH DIAERESIS".to_string(),
        '\u{00D8}' => "LATIN CAPITAL LETTER O WITH STROKE".to_string(),
        '\u{00D9}' => "LATIN CAPITAL LETTER U WITH GRAVE".to_string(),
        '\u{00DA}' => "LATIN CAPITAL LETTER U WITH ACUTE".to_string(),
        '\u{00DB}' => "LATIN CAPITAL LETTER U WITH CIRCUMFLEX".to_string(),
        '\u{00DC}' => "LATIN CAPITAL LETTER U WITH DIAERESIS".to_string(),
        '\u{00DD}' => "LATIN CAPITAL LETTER Y WITH ACUTE".to_string(),
        '\u{00DE}' => "LATIN CAPITAL LETTER THORN".to_string(),
        '\u{00DF}' => "LATIN SMALL LETTER SHARP S".to_string(),
        '\u{00E0}' => "LATIN SMALL LETTER A WITH GRAVE".to_string(),
        '\u{00E1}' => "LATIN SMALL LETTER A WITH ACUTE".to_string(),
        '\u{00E2}' => "LATIN SMALL LETTER A WITH CIRCUMFLEX".to_string(),
        '\u{00E3}' => "LATIN SMALL LETTER A WITH TILDE".to_string(),
        '\u{00E4}' => "LATIN SMALL LETTER A WITH DIAERESIS".to_string(),
        '\u{00E5}' => "LATIN SMALL LETTER A WITH RING ABOVE".to_string(),
        '\u{00E6}' => "LATIN SMALL LETTER AE".to_string(),
        '\u{00E7}' => "LATIN SMALL LETTER C WITH CEDILLA".to_string(),
        '\u{00E8}' => "LATIN SMALL LETTER E WITH GRAVE".to_string(),
        '\u{00E9}' => "LATIN SMALL LETTER E WITH ACUTE".to_string(),
        '\u{00EA}' => "LATIN SMALL LETTER E WITH CIRCUMFLEX".to_string(),
        '\u{00EB}' => "LATIN SMALL LETTER E WITH DIAERESIS".to_string(),
        '\u{00EC}' => "LATIN SMALL LETTER I WITH GRAVE".to_string(),
        '\u{00ED}' => "LATIN SMALL LETTER I WITH ACUTE".to_string(),
        '\u{00EE}' => "LATIN SMALL LETTER I WITH CIRCUMFLEX".to_string(),
        '\u{00EF}' => "LATIN SMALL LETTER I WITH DIAERESIS".to_string(),
        '\u{00F0}' => "LATIN SMALL LETTER ETH".to_string(),
        '\u{00F1}' => "LATIN SMALL LETTER N WITH TILDE".to_string(),
        '\u{00F2}' => "LATIN SMALL LETTER O WITH GRAVE".to_string(),
        '\u{00F3}' => "LATIN SMALL LETTER O WITH ACUTE".to_string(),
        '\u{00F4}' => "LATIN SMALL LETTER O WITH CIRCUMFLEX".to_string(),
        '\u{00F5}' => "LATIN SMALL LETTER O WITH TILDE".to_string(),
        '\u{00F6}' => "LATIN SMALL LETTER O WITH DIAERESIS".to_string(),
        '\u{00F8}' => "LATIN SMALL LETTER O WITH STROKE".to_string(),
        '\u{00F9}' => "LATIN SMALL LETTER U WITH GRAVE".to_string(),
        '\u{00FA}' => "LATIN SMALL LETTER U WITH ACUTE".to_string(),
        '\u{00FB}' => "LATIN SMALL LETTER U WITH CIRCUMFLEX".to_string(),
        '\u{00FC}' => "LATIN SMALL LETTER U WITH DIAERESIS".to_string(),
        '\u{00FD}' => "LATIN SMALL LETTER Y WITH ACUTE".to_string(),
        '\u{00FE}' => "LATIN SMALL LETTER THORN".to_string(),
        '\u{00FF}' => "LATIN SMALL LETTER Y WITH DIAERESIS".to_string(),
        _ => {
            // Control characters have no name in the database
            if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
                return String::new();
            }
            // For unrecognized characters, return empty to trigger default/error
            String::new()
        }
    }
}

/// Reverse-lookup: given a Unicode name, return the character.
pub(super) fn unicode_lookup_name(name: &str) -> Option<char> {
    if let Some(ch) = unicode_names2::character(name) {
        return Some(ch);
    }
    let upper = name.to_ascii_uppercase();
    if upper != name {
        if let Some(ch) = unicode_names2::character(&upper) {
            return Some(ch);
        }
    }
    // Build reverse map from the name function for common chars
    // ASCII letters
    if name.starts_with("LATIN CAPITAL LETTER ") {
        let rest = name.strip_prefix("LATIN CAPITAL LETTER ")?;
        // Handle "X WITH Y" patterns (accented letters)
        return match rest {
            "A" => Some('A'),
            "B" => Some('B'),
            "C" => Some('C'),
            "D" => Some('D'),
            "E" => Some('E'),
            "F" => Some('F'),
            "G" => Some('G'),
            "H" => Some('H'),
            "I" => Some('I'),
            "J" => Some('J'),
            "K" => Some('K'),
            "L" => Some('L'),
            "M" => Some('M'),
            "N" => Some('N'),
            "O" => Some('O'),
            "P" => Some('P'),
            "Q" => Some('Q'),
            "R" => Some('R'),
            "S" => Some('S'),
            "T" => Some('T'),
            "U" => Some('U'),
            "V" => Some('V'),
            "W" => Some('W'),
            "X" => Some('X'),
            "Y" => Some('Y'),
            "Z" => Some('Z'),
            "A WITH GRAVE" => Some('\u{00C0}'),
            "A WITH ACUTE" => Some('\u{00C1}'),
            "A WITH CIRCUMFLEX" => Some('\u{00C2}'),
            "A WITH TILDE" => Some('\u{00C3}'),
            "A WITH DIAERESIS" => Some('\u{00C4}'),
            "A WITH RING ABOVE" => Some('\u{00C5}'),
            "AE" => Some('\u{00C6}'),
            "C WITH CEDILLA" => Some('\u{00C7}'),
            "E WITH GRAVE" => Some('\u{00C8}'),
            "E WITH ACUTE" => Some('\u{00C9}'),
            "E WITH CIRCUMFLEX" => Some('\u{00CA}'),
            "E WITH DIAERESIS" => Some('\u{00CB}'),
            "I WITH GRAVE" => Some('\u{00CC}'),
            "I WITH ACUTE" => Some('\u{00CD}'),
            "I WITH CIRCUMFLEX" => Some('\u{00CE}'),
            "I WITH DIAERESIS" => Some('\u{00CF}'),
            "ETH" => Some('\u{00D0}'),
            "N WITH TILDE" => Some('\u{00D1}'),
            "O WITH GRAVE" => Some('\u{00D2}'),
            "O WITH ACUTE" => Some('\u{00D3}'),
            "O WITH CIRCUMFLEX" => Some('\u{00D4}'),
            "O WITH TILDE" => Some('\u{00D5}'),
            "O WITH DIAERESIS" => Some('\u{00D6}'),
            "O WITH STROKE" => Some('\u{00D8}'),
            "U WITH GRAVE" => Some('\u{00D9}'),
            "U WITH ACUTE" => Some('\u{00DA}'),
            "U WITH CIRCUMFLEX" => Some('\u{00DB}'),
            "U WITH DIAERESIS" => Some('\u{00DC}'),
            "Y WITH ACUTE" => Some('\u{00DD}'),
            "THORN" => Some('\u{00DE}'),
            "A WITH MACRON" => Some('\u{0100}'),
            _ => None,
        };
    }
    if name.starts_with("LATIN SMALL LETTER ") {
        let rest = name.strip_prefix("LATIN SMALL LETTER ")?;
        return match rest {
            "A" => Some('a'),
            "B" => Some('b'),
            "C" => Some('c'),
            "D" => Some('d'),
            "E" => Some('e'),
            "F" => Some('f'),
            "G" => Some('g'),
            "H" => Some('h'),
            "I" => Some('i'),
            "J" => Some('j'),
            "K" => Some('k'),
            "L" => Some('l'),
            "M" => Some('m'),
            "N" => Some('n'),
            "O" => Some('o'),
            "P" => Some('p'),
            "Q" => Some('q'),
            "R" => Some('r'),
            "S" => Some('s'),
            "T" => Some('t'),
            "U" => Some('u'),
            "V" => Some('v'),
            "W" => Some('w'),
            "X" => Some('x'),
            "Y" => Some('y'),
            "Z" => Some('z'),
            "SHARP S" => Some('\u{00DF}'),
            "A WITH GRAVE" => Some('\u{00E0}'),
            "A WITH ACUTE" => Some('\u{00E1}'),
            "A WITH CIRCUMFLEX" => Some('\u{00E2}'),
            "A WITH TILDE" => Some('\u{00E3}'),
            "A WITH DIAERESIS" => Some('\u{00E4}'),
            "A WITH RING ABOVE" => Some('\u{00E5}'),
            "AE" => Some('\u{00E6}'),
            "C WITH CEDILLA" => Some('\u{00E7}'),
            "E WITH GRAVE" => Some('\u{00E8}'),
            "E WITH ACUTE" => Some('\u{00E9}'),
            "E WITH CIRCUMFLEX" => Some('\u{00EA}'),
            "E WITH DIAERESIS" => Some('\u{00EB}'),
            "I WITH GRAVE" => Some('\u{00EC}'),
            "I WITH ACUTE" => Some('\u{00ED}'),
            "I WITH CIRCUMFLEX" => Some('\u{00EE}'),
            "I WITH DIAERESIS" => Some('\u{00EF}'),
            "ETH" => Some('\u{00F0}'),
            "N WITH TILDE" => Some('\u{00F1}'),
            "O WITH GRAVE" => Some('\u{00F2}'),
            "O WITH ACUTE" => Some('\u{00F3}'),
            "O WITH CIRCUMFLEX" => Some('\u{00F4}'),
            "O WITH TILDE" => Some('\u{00F5}'),
            "O WITH DIAERESIS" => Some('\u{00F6}'),
            "O WITH STROKE" => Some('\u{00F8}'),
            "U WITH GRAVE" => Some('\u{00F9}'),
            "U WITH ACUTE" => Some('\u{00FA}'),
            "U WITH CIRCUMFLEX" => Some('\u{00FB}'),
            "U WITH DIAERESIS" => Some('\u{00FC}'),
            "Y WITH ACUTE" => Some('\u{00FD}'),
            "THORN" => Some('\u{00FE}'),
            "Y WITH DIAERESIS" => Some('\u{00FF}'),
            "A WITH MACRON" => Some('\u{0101}'),
            _ => None,
        };
    }
    // Digit names
    if name.starts_with("DIGIT ") {
        let rest = name.strip_prefix("DIGIT ")?;
        return match rest {
            "ZERO" => Some('0'),
            "ONE" => Some('1'),
            "TWO" => Some('2'),
            "THREE" => Some('3'),
            "FOUR" => Some('4'),
            "FIVE" => Some('5'),
            "SIX" => Some('6'),
            "SEVEN" => Some('7'),
            "EIGHT" => Some('8'),
            "NINE" => Some('9'),
            _ => rest.chars().next().filter(|c| c.is_ascii_digit()),
        };
    }
    // Greek letters
    if name.starts_with("GREEK CAPITAL LETTER ") {
        let rest = name.strip_prefix("GREEK CAPITAL LETTER ")?;
        return match rest {
            "ALPHA" => Some('\u{0391}'),
            "BETA" => Some('\u{0392}'),
            "GAMMA" => Some('\u{0393}'),
            "DELTA" => Some('\u{0394}'),
            "EPSILON" => Some('\u{0395}'),
            "ZETA" => Some('\u{0396}'),
            "ETA" => Some('\u{0397}'),
            "THETA" => Some('\u{0398}'),
            "IOTA" => Some('\u{0399}'),
            "KAPPA" => Some('\u{039A}'),
            "LAMDA" => Some('\u{039B}'),
            "MU" => Some('\u{039C}'),
            "NU" => Some('\u{039D}'),
            "XI" => Some('\u{039E}'),
            "OMICRON" => Some('\u{039F}'),
            "PI" => Some('\u{03A0}'),
            "RHO" => Some('\u{03A1}'),
            "SIGMA" => Some('\u{03A3}'),
            "TAU" => Some('\u{03A4}'),
            "UPSILON" => Some('\u{03A5}'),
            "PHI" => Some('\u{03A6}'),
            "CHI" => Some('\u{03A7}'),
            "PSI" => Some('\u{03A8}'),
            "OMEGA" => Some('\u{03A9}'),
            _ => None,
        };
    }
    if name.starts_with("GREEK SMALL LETTER ") {
        let rest = name.strip_prefix("GREEK SMALL LETTER ")?;
        return match rest {
            "ALPHA" => Some('\u{03B1}'),
            "BETA" => Some('\u{03B2}'),
            "GAMMA" => Some('\u{03B3}'),
            "DELTA" => Some('\u{03B4}'),
            "EPSILON" => Some('\u{03B5}'),
            "ZETA" => Some('\u{03B6}'),
            "ETA" => Some('\u{03B7}'),
            "THETA" => Some('\u{03B8}'),
            "IOTA" => Some('\u{03B9}'),
            "KAPPA" => Some('\u{03BA}'),
            "LAMDA" => Some('\u{03BB}'),
            "MU" => Some('\u{03BC}'),
            "NU" => Some('\u{03BD}'),
            "XI" => Some('\u{03BE}'),
            "OMICRON" => Some('\u{03BF}'),
            "PI" => Some('\u{03C0}'),
            "RHO" => Some('\u{03C1}'),
            "SIGMA" => Some('\u{03C3}'),
            "TAU" => Some('\u{03C4}'),
            "UPSILON" => Some('\u{03C5}'),
            "PHI" => Some('\u{03C6}'),
            "CHI" => Some('\u{03C7}'),
            "PSI" => Some('\u{03C8}'),
            "OMEGA" => Some('\u{03C9}'),
            _ => None,
        };
    }
    // Direct matches for symbols and punctuation
    match name {
        "SPACE" => Some(' '),
        "EXCLAMATION MARK" => Some('!'),
        "QUOTATION MARK" => Some('"'),
        "NUMBER SIGN" => Some('#'),
        "DOLLAR SIGN" => Some('$'),
        "PERCENT SIGN" => Some('%'),
        "AMPERSAND" => Some('&'),
        "APOSTROPHE" => Some('\''),
        "LEFT PARENTHESIS" => Some('('),
        "RIGHT PARENTHESIS" => Some(')'),
        "ASTERISK" => Some('*'),
        "PLUS SIGN" => Some('+'),
        "COMMA" => Some(','),
        "HYPHEN-MINUS" => Some('-'),
        "FULL STOP" => Some('.'),
        "SOLIDUS" => Some('/'),
        "COLON" => Some(':'),
        "SEMICOLON" => Some(';'),
        "LESS-THAN SIGN" => Some('<'),
        "EQUALS SIGN" => Some('='),
        "GREATER-THAN SIGN" => Some('>'),
        "QUESTION MARK" => Some('?'),
        "COMMERCIAL AT" => Some('@'),
        "LEFT SQUARE BRACKET" => Some('['),
        "REVERSE SOLIDUS" => Some('\\'),
        "RIGHT SQUARE BRACKET" => Some(']'),
        "CIRCUMFLEX ACCENT" => Some('^'),
        "LOW LINE" => Some('_'),
        "GRAVE ACCENT" => Some('`'),
        "LEFT CURLY BRACKET" => Some('{'),
        "VERTICAL LINE" => Some('|'),
        "RIGHT CURLY BRACKET" => Some('}'),
        "TILDE" => Some('~'),
        "NO-BREAK SPACE" => Some('\u{00A0}'),
        "COPYRIGHT SIGN" => Some('\u{00A9}'),
        "REGISTERED SIGN" => Some('\u{00AE}'),
        "DEGREE SIGN" => Some('\u{00B0}'),
        "PLUS-MINUS SIGN" => Some('\u{00B1}'),
        "MICRO SIGN" => Some('\u{00B5}'),
        "MIDDLE DOT" => Some('\u{00B7}'),
        "VULGAR FRACTION ONE QUARTER" => Some('\u{00BC}'),
        "VULGAR FRACTION ONE HALF" => Some('\u{00BD}'),
        "VULGAR FRACTION THREE QUARTERS" => Some('\u{00BE}'),
        "MULTIPLICATION SIGN" => Some('\u{00D7}'),
        "DIVISION SIGN" => Some('\u{00F7}'),
        "EN DASH" => Some('\u{2013}'),
        "EM DASH" => Some('\u{2014}'),
        "LEFT SINGLE QUOTATION MARK" => Some('\u{2018}'),
        "RIGHT SINGLE QUOTATION MARK" => Some('\u{2019}'),
        "LEFT DOUBLE QUOTATION MARK" => Some('\u{201C}'),
        "RIGHT DOUBLE QUOTATION MARK" => Some('\u{201D}'),
        "BULLET" => Some('\u{2022}'),
        "HORIZONTAL ELLIPSIS" => Some('\u{2026}'),
        "EURO SIGN" => Some('\u{20AC}'),
        "TRADE MARK SIGN" => Some('\u{2122}'),
        "INFINITY" => Some('\u{221E}'),
        "SQUARE ROOT" => Some('\u{221A}'),
        "NOT EQUAL TO" => Some('\u{2260}'),
        "LESS-THAN OR EQUAL TO" => Some('\u{2264}'),
        "GREATER-THAN OR EQUAL TO" => Some('\u{2265}'),
        "SNOWMAN" => Some('\u{2603}'),
        "LATIN CAPITAL LETTER AE" => Some('\u{00C6}'),
        "LATIN SMALL LETTER AE" => Some('\u{00E6}'),
        _ => None,
    }
}

/// Return the Unicode General Category for a character.
fn unicode_category(ch: char) -> &'static str {
    let cp = ch as u32;
    // Control characters
    if cp <= 0x1F || (0x7F..=0x9F).contains(&cp) {
        return "Cc";
    }
    // ASCII and Latin-1 fast paths
    if ch.is_ascii_uppercase() {
        return "Lu";
    }
    if ch.is_ascii_lowercase() {
        return "Ll";
    }
    if ch.is_ascii_digit() {
        return "Nd";
    }
    // Specific ASCII punctuation subcategories
    match ch {
        ' ' => return "Zs",
        '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => return "Zs",
        '\u{2028}' => return "Zl",
        '\u{2029}' => return "Zp",
        '(' | '[' | '{' => return "Ps",
        ')' | ']' | '}' => return "Pe",
        '_' => return "Pc",
        '-' | '\u{2010}'..='\u{2015}' => return "Pd",
        '$' | '\u{00A2}'..='\u{00A5}' | '\u{20AC}' => return "Sc",
        '+' | '<' | '=' | '>' | '|' | '~' | '^' | '\u{00AC}' | '\u{00B1}' => return "Sm",
        '#' | '%' | '&' | '*' | '\\' | '@' | '\u{00A7}' | '\u{00B0}' | '\u{00B6}' | '\u{00A9}'
        | '\u{00AE}' => return "So",
        '!' | '"' | '\'' | ',' | '.' | '/' | ':' | ';' | '?' | '\u{00A1}' | '\u{00BF}'
        | '\u{00B7}' => return "Po",
        '`' => return "Sk",
        _ => {}
    }
    // Combining marks
    if ('\u{0300}'..='\u{036F}').contains(&ch)
        || ('\u{0483}'..='\u{0489}').contains(&ch)
        || ('\u{0591}'..='\u{05BD}').contains(&ch)
        || ('\u{0610}'..='\u{061A}').contains(&ch)
        || ('\u{064B}'..='\u{065F}').contains(&ch)
        || ('\u{0900}'..='\u{0903}').contains(&ch)
        || ('\u{093A}'..='\u{094F}').contains(&ch)
        || ('\u{20D0}'..='\u{20FF}').contains(&ch)
    {
        return "Mn";
    }
    // Format characters
    if ch == '\u{00AD}'
        || ('\u{200B}'..='\u{200F}').contains(&ch)
        || ('\u{202A}'..='\u{202E}').contains(&ch)
        || ('\u{2060}'..='\u{2064}').contains(&ch)
        || ch == '\u{FEFF}'
    {
        return "Cf";
    }
    // Surrogates
    if (0xD800..=0xDFFF).contains(&cp) {
        return "Cs";
    }
    // Private use
    if (0xE000..=0xF8FF).contains(&cp)
        || (0xF0000..=0xFFFFF).contains(&cp)
        || (0x100000..=0x10FFFF).contains(&cp)
    {
        return "Co";
    }
    // Numbers (beyond ASCII digits)
    if ch.is_numeric() {
        return "Nd";
    }
    // Letters
    if ch.is_uppercase() {
        return "Lu";
    }
    if ch.is_lowercase() {
        return "Ll";
    }
    // Titlecase letters
    if ('\u{01C5}'..='\u{01C5}').contains(&ch)
        || ('\u{01C8}'..='\u{01C8}').contains(&ch)
        || ('\u{01CB}'..='\u{01CB}').contains(&ch)
        || ch == '\u{01F2}'
    {
        return "Lt";
    }
    // Modifier letters
    if ('\u{02B0}'..='\u{02FF}').contains(&ch) {
        return "Lm";
    }
    if ch.is_alphabetic() {
        return "Lo";
    }
    // Default
    "Cn"
}

/// Basic NFC composition for common precomposed characters.
fn nfc_compose(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() {
            if let Some(composed) = compose_pair(chars[i], chars[i + 1]) {
                result.push(composed);
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Basic NFD decomposition for common precomposed characters.
fn nfd_decompose(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        if let Some((base, combining)) = decompose_char(ch) {
            result.push(base);
            result.push(combining);
        } else {
            result.push(ch);
        }
    }
    result
}

fn compose_pair(base: char, combining: char) -> Option<char> {
    match (base, combining) {
        ('A', '\u{0300}') => Some('\u{00C0}'),
        ('A', '\u{0301}') => Some('\u{00C1}'),
        ('A', '\u{0302}') => Some('\u{00C2}'),
        ('A', '\u{0303}') => Some('\u{00C3}'),
        ('A', '\u{0308}') => Some('\u{00C4}'),
        ('A', '\u{030A}') => Some('\u{00C5}'),
        ('C', '\u{0327}') => Some('\u{00C7}'),
        ('E', '\u{0300}') => Some('\u{00C8}'),
        ('E', '\u{0301}') => Some('\u{00C9}'),
        ('E', '\u{0302}') => Some('\u{00CA}'),
        ('E', '\u{0308}') => Some('\u{00CB}'),
        ('I', '\u{0300}') => Some('\u{00CC}'),
        ('I', '\u{0301}') => Some('\u{00CD}'),
        ('I', '\u{0302}') => Some('\u{00CE}'),
        ('I', '\u{0308}') => Some('\u{00CF}'),
        ('N', '\u{0303}') => Some('\u{00D1}'),
        ('O', '\u{0300}') => Some('\u{00D2}'),
        ('O', '\u{0301}') => Some('\u{00D3}'),
        ('O', '\u{0302}') => Some('\u{00D4}'),
        ('O', '\u{0303}') => Some('\u{00D5}'),
        ('O', '\u{0308}') => Some('\u{00D6}'),
        ('U', '\u{0300}') => Some('\u{00D9}'),
        ('U', '\u{0301}') => Some('\u{00DA}'),
        ('U', '\u{0302}') => Some('\u{00DB}'),
        ('U', '\u{0308}') => Some('\u{00DC}'),
        ('Y', '\u{0301}') => Some('\u{00DD}'),
        ('a', '\u{0300}') => Some('\u{00E0}'),
        ('a', '\u{0301}') => Some('\u{00E1}'),
        ('a', '\u{0302}') => Some('\u{00E2}'),
        ('a', '\u{0303}') => Some('\u{00E3}'),
        ('a', '\u{0308}') => Some('\u{00E4}'),
        ('a', '\u{030A}') => Some('\u{00E5}'),
        ('c', '\u{0327}') => Some('\u{00E7}'),
        ('e', '\u{0300}') => Some('\u{00E8}'),
        ('e', '\u{0301}') => Some('\u{00E9}'),
        ('e', '\u{0302}') => Some('\u{00EA}'),
        ('e', '\u{0308}') => Some('\u{00EB}'),
        ('i', '\u{0300}') => Some('\u{00EC}'),
        ('i', '\u{0301}') => Some('\u{00ED}'),
        ('i', '\u{0302}') => Some('\u{00EE}'),
        ('i', '\u{0308}') => Some('\u{00EF}'),
        ('n', '\u{0303}') => Some('\u{00F1}'),
        ('o', '\u{0300}') => Some('\u{00F2}'),
        ('o', '\u{0301}') => Some('\u{00F3}'),
        ('o', '\u{0302}') => Some('\u{00F4}'),
        ('o', '\u{0303}') => Some('\u{00F5}'),
        ('o', '\u{0308}') => Some('\u{00F6}'),
        ('u', '\u{0300}') => Some('\u{00F9}'),
        ('u', '\u{0301}') => Some('\u{00FA}'),
        ('u', '\u{0302}') => Some('\u{00FB}'),
        ('u', '\u{0308}') => Some('\u{00FC}'),
        ('y', '\u{0301}') => Some('\u{00FD}'),
        ('y', '\u{0308}') => Some('\u{00FF}'),
        _ => None,
    }
}

fn decompose_char(ch: char) -> Option<(char, char)> {
    match ch {
        '\u{00C0}' => Some(('A', '\u{0300}')),
        '\u{00C1}' => Some(('A', '\u{0301}')),
        '\u{00C2}' => Some(('A', '\u{0302}')),
        '\u{00C3}' => Some(('A', '\u{0303}')),
        '\u{00C4}' => Some(('A', '\u{0308}')),
        '\u{00C5}' => Some(('A', '\u{030A}')),
        '\u{00C7}' => Some(('C', '\u{0327}')),
        '\u{00C8}' => Some(('E', '\u{0300}')),
        '\u{00C9}' => Some(('E', '\u{0301}')),
        '\u{00CA}' => Some(('E', '\u{0302}')),
        '\u{00CB}' => Some(('E', '\u{0308}')),
        '\u{00CC}' => Some(('I', '\u{0300}')),
        '\u{00CD}' => Some(('I', '\u{0301}')),
        '\u{00CE}' => Some(('I', '\u{0302}')),
        '\u{00CF}' => Some(('I', '\u{0308}')),
        '\u{00D1}' => Some(('N', '\u{0303}')),
        '\u{00D2}' => Some(('O', '\u{0300}')),
        '\u{00D3}' => Some(('O', '\u{0301}')),
        '\u{00D4}' => Some(('O', '\u{0302}')),
        '\u{00D5}' => Some(('O', '\u{0303}')),
        '\u{00D6}' => Some(('O', '\u{0308}')),
        '\u{00D9}' => Some(('U', '\u{0300}')),
        '\u{00DA}' => Some(('U', '\u{0301}')),
        '\u{00DB}' => Some(('U', '\u{0302}')),
        '\u{00DC}' => Some(('U', '\u{0308}')),
        '\u{00DD}' => Some(('Y', '\u{0301}')),
        '\u{00E0}' => Some(('a', '\u{0300}')),
        '\u{00E1}' => Some(('a', '\u{0301}')),
        '\u{00E2}' => Some(('a', '\u{0302}')),
        '\u{00E3}' => Some(('a', '\u{0303}')),
        '\u{00E4}' => Some(('a', '\u{0308}')),
        '\u{00E5}' => Some(('a', '\u{030A}')),
        '\u{00E7}' => Some(('c', '\u{0327}')),
        '\u{00E8}' => Some(('e', '\u{0300}')),
        '\u{00E9}' => Some(('e', '\u{0301}')),
        '\u{00EA}' => Some(('e', '\u{0302}')),
        '\u{00EB}' => Some(('e', '\u{0308}')),
        '\u{00EC}' => Some(('i', '\u{0300}')),
        '\u{00ED}' => Some(('i', '\u{0301}')),
        '\u{00EE}' => Some(('i', '\u{0302}')),
        '\u{00EF}' => Some(('i', '\u{0308}')),
        '\u{00F1}' => Some(('n', '\u{0303}')),
        '\u{00F2}' => Some(('o', '\u{0300}')),
        '\u{00F3}' => Some(('o', '\u{0301}')),
        '\u{00F4}' => Some(('o', '\u{0302}')),
        '\u{00F5}' => Some(('o', '\u{0303}')),
        '\u{00F6}' => Some(('o', '\u{0308}')),
        '\u{00F9}' => Some(('u', '\u{0300}')),
        '\u{00FA}' => Some(('u', '\u{0301}')),
        '\u{00FB}' => Some(('u', '\u{0302}')),
        '\u{00FC}' => Some(('u', '\u{0308}')),
        '\u{00FD}' => Some(('y', '\u{0301}')),
        '\u{00FF}' => Some(('y', '\u{0308}')),
        _ => None,
    }
}
