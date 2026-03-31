/// Source location information attached to every AST node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// 1-based line number.
    pub line: u32,
    /// 0-based column offset (in UTF-8 bytes).
    pub column: u32,
    /// Optional end line (for multi-line nodes).
    pub end_line: Option<u32>,
    /// Optional end column.
    pub end_column: Option<u32>,
}

impl SourceLocation {
    pub fn new(line: u32, column: u32) -> Self {
        Self {
            line,
            column,
            end_line: None,
            end_column: None,
        }
    }

    pub fn with_end(mut self, end_line: u32, end_column: u32) -> Self {
        self.end_line = Some(end_line);
        self.end_column = Some(end_column);
        self
    }
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self::new(1, 0)
    }
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, col {}", self.line, self.column)
    }
}
