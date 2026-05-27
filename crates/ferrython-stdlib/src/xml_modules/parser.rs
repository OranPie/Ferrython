#[derive(Debug, Clone)]
pub(super) struct XmlElement {
    pub(super) tag: String,
    pub(super) text: String,
    pub(super) tail: String,
    pub(super) attrib: Vec<(String, String)>,
    pub(super) children: Vec<XmlElement>,
}

impl XmlElement {
    pub(super) fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            text: String::new(),
            tail: String::new(),
            attrib: Vec::new(),
            children: Vec::new(),
        }
    }
}

pub(super) struct XmlParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> XmlParser<'a> {
    pub(super) fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn remaining(&self) -> &str {
        std::str::from_utf8(&self.input[self.pos..]).unwrap_or("")
    }

    fn starts_with(&self, s: &str) -> bool {
        self.remaining().starts_with(s)
    }

    fn skip_str(&mut self, s: &str) {
        self.pos += s.len();
    }

    fn read_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b':' {
                self.advance();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    fn read_attr_value(&mut self) -> Result<String, String> {
        let quote = match self.peek() {
            Some(b'"') => b'"',
            Some(b'\'') => b'\'',
            _ => return Err("expected quote for attribute value".to_string()),
        };
        self.advance();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == quote {
                break;
            }
            self.advance();
        }
        let val = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
        self.advance();
        Ok(unescape_xml(&val))
    }

    fn read_text_until_lt(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'<' {
                break;
            }
            self.advance();
        }
        let raw = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
        unescape_xml(&raw)
    }

    fn skip_xml_declaration(&mut self) {
        self.skip_ws();
        if self.starts_with("<?xml") {
            while let Some(c) = self.peek() {
                if c == b'>' {
                    self.advance();
                    break;
                }
                self.advance();
            }
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.starts_with("<!--") {
            while !self.starts_with("-->") && self.pos < self.input.len() {
                self.advance();
            }
            if self.starts_with("-->") {
                self.skip_str("-->");
            }
            true
        } else {
            false
        }
    }

    fn parse_element(&mut self) -> Result<XmlElement, String> {
        self.skip_ws();
        while self.skip_comment() {
            self.skip_ws();
        }

        if self.peek() != Some(b'<') {
            return Err(format!(
                "expected '<', found {:?} at pos {}",
                self.peek().map(|c| c as char),
                self.pos
            ));
        }
        self.advance();

        let tag = self.read_name();
        if tag.is_empty() {
            return Err("empty tag name".to_string());
        }

        let mut elem = XmlElement::new(&tag);

        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') => {
                    self.advance();
                    break;
                }
                Some(b'/') => {
                    self.advance();
                    if self.peek() == Some(b'>') {
                        self.advance();
                        return Ok(elem);
                    }
                    return Err("expected '>' after '/'".to_string());
                }
                Some(_) => {
                    let attr_name = self.read_name();
                    if attr_name.is_empty() {
                        return Err("empty attribute name".to_string());
                    }
                    self.skip_ws();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        self.skip_ws();
                        let attr_val = self.read_attr_value()?;
                        elem.attrib.push((attr_name, attr_val));
                    } else {
                        elem.attrib.push((attr_name, String::new()));
                    }
                }
                None => return Err("unexpected end of input in tag".to_string()),
            }
        }

        let text = self.read_text_until_lt();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            elem.text = trimmed.to_string();
        }

        loop {
            self.skip_ws();
            while self.skip_comment() {
                self.skip_ws();
            }

            if self.pos >= self.input.len() {
                break;
            }

            let closing = format!("</{}", tag);
            if self.starts_with(&closing) {
                self.skip_str(&closing);
                self.skip_ws();
                if self.peek() == Some(b'>') {
                    self.advance();
                }
                break;
            }

            if self.peek() == Some(b'<') && self.input.get(self.pos + 1) == Some(&b'/') {
                while let Some(c) = self.peek() {
                    self.advance();
                    if c == b'>' {
                        break;
                    }
                }
                break;
            }

            if self.peek() == Some(b'<') {
                let child = self.parse_element()?;
                let tail_text = self.read_text_until_lt();
                let tail_trimmed = tail_text.trim();
                let mut child = child;
                if !tail_trimmed.is_empty() {
                    child.tail = tail_trimmed.to_string();
                }
                elem.children.push(child);
            } else {
                break;
            }
        }

        Ok(elem)
    }

    pub(super) fn parse_document(&mut self) -> Result<XmlElement, String> {
        self.skip_xml_declaration();
        self.skip_ws();
        while self.skip_comment() {
            self.skip_ws();
        }
        self.parse_element()
    }
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub(super) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
