use super::*;

pub(super) fn normalize_encoding(enc: &str) -> String {
    enc.to_lowercase().replace('-', "_")
}

pub(super) fn rot13(c: char) -> char {
    match c {
        'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
        'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
        _ => c,
    }
}

pub(super) fn punycode_digit(d: u32) -> u8 {
    if d < 26 {
        b'a' + d as u8
    } else {
        b'0' + (d as u8 - 26)
    }
}

pub(super) fn punycode_adapt(delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    let mut d = if firsttime { delta / 700 } else { delta / 2 };
    d += d / numpoints;
    let mut k = 0u32;
    while d > 455 {
        d /= 35;
        k += 36;
    }
    k + (36 * d) / (d + 38)
}

pub(super) fn cp1252_encode(c: char) -> Result<u8, String> {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return Ok(u as u8);
    }
    match u {
        0x20AC => Ok(0x80),
        0x201A => Ok(0x82),
        0x0192 => Ok(0x83),
        0x201E => Ok(0x84),
        0x2026 => Ok(0x85),
        0x2020 => Ok(0x86),
        0x2021 => Ok(0x87),
        0x02C6 => Ok(0x88),
        0x2030 => Ok(0x89),
        0x0160 => Ok(0x8A),
        0x2039 => Ok(0x8B),
        0x0152 => Ok(0x8C),
        0x017D => Ok(0x8E),
        0x2018 => Ok(0x91),
        0x2019 => Ok(0x92),
        0x201C => Ok(0x93),
        0x201D => Ok(0x94),
        0x2022 => Ok(0x95),
        0x2013 => Ok(0x96),
        0x2014 => Ok(0x97),
        0x02DC => Ok(0x98),
        0x2122 => Ok(0x99),
        0x0161 => Ok(0x9A),
        0x203A => Ok(0x9B),
        0x0153 => Ok(0x9C),
        0x017E => Ok(0x9E),
        0x0178 => Ok(0x9F),
        _ => Err(format!(
            "'cp1252' codec can't encode character '\\u{:04x}'",
            u
        )),
    }
}

pub(super) fn cp1252_decode(b: u8) -> char {
    if b < 0x80 || b >= 0xA0 {
        return b as char;
    }
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        _ => '\u{FFFD}',
    }
}

pub(super) fn decode_utf16_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        decode_utf16_le(&bytes[2..])
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        decode_utf16_be(&bytes[2..])
    } else {
        decode_utf16_le(bytes)
    }
}

pub(super) fn decode_utf16_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-le: truncated data"));
    }
    let u16s: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-le"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn decode_utf16_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-be: truncated data"));
    }
    let u16s: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-be"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn decode_utf32_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 4 && bytes[..4] == [0xFF, 0xFE, 0x00, 0x00] {
        decode_utf32_le(&bytes[4..])
    } else if bytes.len() >= 4 && bytes[..4] == [0x00, 0x00, 0xFE, 0xFF] {
        decode_utf32_be(&bytes[4..])
    } else {
        decode_utf32_le(bytes)
    }
}

pub(super) fn decode_utf32_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-le: truncated data"));
    }
    let s: Result<String, _> = bytes
        .chunks(4)
        .map(|c| {
            let cp = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-le codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

pub(super) fn decode_utf32_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-be: truncated data"));
    }
    let s: Result<String, _> = bytes
        .chunks(4)
        .map(|c| {
            let cp = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-be codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

pub(super) fn backslashreplace_char(c: char) -> String {
    let cp = c as u32;
    if cp <= 0xFF {
        format!("\\x{:02x}", cp)
    } else if cp <= 0xFFFF {
        format!("\\u{:04x}", cp)
    } else {
        format!("\\U{:08x}", cp)
    }
}

pub(super) fn xmlcharrefreplace_char(c: char) -> String {
    format!("&#{};", c as u32)
}

pub(super) fn resolve_encoding(norm: &str) -> &str {
    match norm {
        "utf_8" | "utf8" => "utf_8",
        "ascii" | "us_ascii" => "ascii",
        "latin_1" | "latin1" | "iso_8859_1" | "iso8859_1" | "8859" | "cp819" | "l1" => "latin_1",
        "utf_16" | "utf16" => "utf_16",
        "utf_16_le" | "utf16_le" | "utf_16le" => "utf_16_le",
        "utf_16_be" | "utf16_be" | "utf_16be" => "utf_16_be",
        "utf_32" | "utf32" => "utf_32",
        "utf_32_le" | "utf32_le" => "utf_32_le",
        "utf_32_be" | "utf32_be" => "utf_32_be",
        "cp1252" | "windows_1252" => "cp1252",
        "rot_13" | "rot13" => "rot_13",
        "punycode" => "punycode",
        "idna" => "idna",
        "iso8859_2" | "iso_8859_2" | "latin2" | "l2" => "iso8859_2",
        "iso8859_3" | "iso_8859_3" | "latin3" | "l3" => "iso8859_3",
        "iso8859_4" | "iso_8859_4" | "latin4" | "l4" => "iso8859_4",
        "iso8859_5" | "iso_8859_5" | "cyrillic" => "iso8859_5",
        "iso8859_6" | "iso_8859_6" | "arabic" => "iso8859_6",
        "iso8859_7" | "iso_8859_7" | "greek" => "iso8859_7",
        "iso8859_8" | "iso_8859_8" | "hebrew" => "iso8859_8",
        "iso8859_9" | "iso_8859_9" | "latin5" | "l5" => "iso8859_9",
        "iso8859_10" | "iso_8859_10" | "latin6" | "l6" => "iso8859_10",
        "iso8859_11" | "iso_8859_11" | "thai" => "iso8859_11",
        "iso8859_13" | "iso_8859_13" | "latin7" | "l7" => "iso8859_13",
        "iso8859_14" | "iso_8859_14" | "latin8" | "l8" => "iso8859_14",
        "iso8859_15" | "iso_8859_15" | "latin9" | "l9" => "iso8859_15",
        "iso8859_16" | "iso_8859_16" | "latin10" | "l10" => "iso8859_16",
        "cp437" => "cp437",
        "cp850" => "cp850",
        "cp866" => "cp866",
        "cp874" | "windows_874" => "cp874",
        "cp932" | "ms932" | "mskanji" | "ms_kanji" => "cp932",
        "cp949" | "ms949" | "uhc" => "cp949",
        "cp950" | "ms950" => "cp950",
        "cp1250" | "windows_1250" => "cp1250",
        "cp1251" | "windows_1251" => "cp1251",
        "cp1253" | "windows_1253" => "cp1253",
        "cp1254" | "windows_1254" => "cp1254",
        "cp1255" | "windows_1255" => "cp1255",
        "cp1256" | "windows_1256" => "cp1256",
        "cp1257" | "windows_1257" => "cp1257",
        "cp1258" | "windows_1258" => "cp1258",
        "big5" | "big5_tw" | "csbig5" => "big5",
        "big5hkscs" | "big5_hkscs" => "big5hkscs",
        "euc_jp" | "eucjp" | "ujis" | "u_jis" => "euc_jp",
        "euc_kr" | "euckr" | "korean" => "euc_kr",
        "euc_cn" | "gb2312" | "chinese" | "csiso58gb231280" => "gb2312",
        "gbk" | "cp936" | "ms936" => "gbk",
        "gb18030" => "gb18030",
        "hz" | "hzgb" | "hz_gb" | "hz_gb_2312" => "hz",
        "shift_jis" | "shiftjis" | "sjis" | "s_jis" | "csshiftjis" => "shift_jis",
        "shift_jis_2004" | "shiftjis2004" | "sjis_2004" => "shift_jis_2004",
        "shift_jisx0213" | "shiftjisx0213" | "sjisx0213" => "shift_jisx0213",
        "iso2022_jp" | "iso_2022_jp" | "csiso2022jp" => "iso2022_jp",
        "iso2022_jp_2" | "iso_2022_jp_2" => "iso2022_jp_2",
        "iso2022_kr" | "iso_2022_kr" | "csiso2022kr" => "iso2022_kr",
        "iso2022_cn" | "iso_2022_cn" => "iso2022_cn",
        "koi8_r" | "koi8r" => "koi8_r",
        "koi8_u" | "koi8u" => "koi8_u",
        "koi8_t" => "koi8_t",
        "mac_roman" | "macroman" | "macintosh" => "mac_roman",
        "mac_cyrillic" | "maccyrillic" => "mac_cyrillic",
        "mac_greek" | "macgreek" => "mac_greek",
        "mac_latin2" | "maclatin2" | "maccentraleurope" => "mac_latin2",
        "johab" => "johab",
        "tis_620" | "tis620" => "tis_620",
        "viscii" => "viscii",
        other => other,
    }
}
