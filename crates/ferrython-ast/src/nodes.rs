//! Python 3.8 AST node definitions.
//!
//! Matches the `ast` module specification from CPython 3.8.

use crate::SourceLocation;
use compact_str::CompactString;

// ─── Top-level Module Types ─────────────────────────────────────────

/// A complete Python module (file, interactive input, or expression).
#[derive(Debug, Clone)]
pub enum Module {
    /// A file or exec() input.
    Module {
        body: Vec<Statement>,
        type_ignores: Vec<TypeIgnore>,
    },
    /// A single interactive statement (REPL).
    Interactive { body: Vec<Statement> },
    /// A single expression (eval()).
    Expression { body: Box<Expression> },
}

#[derive(Debug, Clone)]
pub struct TypeIgnore {
    pub lineno: u32,
    pub tag: CompactString,
}

// ─── Statements ─────────────────────────────────────────────────────

/// A statement with source location.
#[derive(Debug, Clone)]
pub struct Statement {
    pub node: StatementKind,
    pub location: SourceLocation,
}

#[derive(Debug, Clone)]
pub enum StatementKind {
    /// `def name(args): body`
    FunctionDef {
        name: CompactString,
        args: Box<Arguments>,
        body: Vec<Statement>,
        decorator_list: Vec<Expression>,
        returns: Option<Box<Expression>>,
        type_comment: Option<CompactString>,
        is_async: bool,
    },
    /// `class name(bases): body`
    ClassDef {
        name: CompactString,
        bases: Vec<Expression>,
        keywords: Vec<Keyword>,
        body: Vec<Statement>,
        decorator_list: Vec<Expression>,
    },
    /// `return [value]`
    Return {
        value: Option<Box<Expression>>,
    },
    /// `del targets`
    Delete {
        targets: Vec<Expression>,
    },
    /// `targets = value`
    Assign {
        targets: Vec<Expression>,
        value: Box<Expression>,
        type_comment: Option<CompactString>,
    },
    /// `target op= value` (e.g., `x += 1`)
    AugAssign {
        target: Box<Expression>,
        op: Operator,
        value: Box<Expression>,
    },
    /// `target: annotation [= value]`
    AnnAssign {
        target: Box<Expression>,
        annotation: Box<Expression>,
        value: Option<Box<Expression>>,
        simple: bool,
    },
    /// `for target in iter: body [else: orelse]`
    For {
        target: Box<Expression>,
        iter: Box<Expression>,
        body: Vec<Statement>,
        orelse: Vec<Statement>,
        type_comment: Option<CompactString>,
        is_async: bool,
    },
    /// `while test: body [else: orelse]`
    While {
        test: Box<Expression>,
        body: Vec<Statement>,
        orelse: Vec<Statement>,
    },
    /// `if test: body [elif ...: ...] [else: orelse]`
    If {
        test: Box<Expression>,
        body: Vec<Statement>,
        orelse: Vec<Statement>,
    },
    /// `with items: body`
    With {
        items: Vec<WithItem>,
        body: Vec<Statement>,
        type_comment: Option<CompactString>,
        is_async: bool,
    },
    /// `raise [exc [from cause]]`
    Raise {
        exc: Option<Box<Expression>>,
        cause: Option<Box<Expression>>,
    },
    /// `try: body except: handlers else: orelse finally: finalbody`
    Try {
        body: Vec<Statement>,
        handlers: Vec<ExceptHandler>,
        orelse: Vec<Statement>,
        finalbody: Vec<Statement>,
    },
    /// `assert test [, msg]`
    Assert {
        test: Box<Expression>,
        msg: Option<Box<Expression>>,
    },
    /// `import names`
    Import {
        names: Vec<Alias>,
    },
    /// `from module import names`
    ImportFrom {
        module: Option<CompactString>,
        names: Vec<Alias>,
        level: u32,
    },
    /// `global names`
    Global {
        names: Vec<CompactString>,
    },
    /// `nonlocal names`
    Nonlocal {
        names: Vec<CompactString>,
    },
    /// Expression as statement.
    Expr {
        value: Box<Expression>,
    },
    /// `pass`
    Pass,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `match subject: case pattern: body ...` (Python 3.10+)
    Match {
        subject: Box<Expression>,
        cases: Vec<MatchCase>,
    },
}

// ─── Match/Case (Python 3.10+) ─────────────────────────────────────

/// A single `case pattern [if guard]: body` clause.
#[derive(Debug, Clone)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub guard: Option<Expression>,
    pub body: Vec<Statement>,
}

/// Pattern node for structural pattern matching.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Wildcard `_` — matches anything, binds nothing.
    MatchWildcard,
    /// Capture pattern — binds the subject to a name (e.g. `x`).
    MatchCapture { name: CompactString },
    /// Value pattern — dotted name treated as a constant (e.g. `Color.RED`).
    MatchValue { value: Expression },
    /// Literal pattern — a constant value (int, float, str, bool, None).
    MatchLiteral { value: Expression },
    /// Sequence pattern — `[p1, p2, ...]` or `(p1, p2, ...)`.
    MatchSequence { patterns: Vec<Pattern> },
    /// Mapping pattern — `{key1: p1, key2: p2, ...}`.
    MatchMapping {
        keys: Vec<Expression>,
        patterns: Vec<Pattern>,
        rest: Option<CompactString>,
    },
    /// Class pattern — `ClassName(p1, key=p2, ...)`.
    MatchClass {
        cls: Expression,
        patterns: Vec<Pattern>,
        kwd_attrs: Vec<CompactString>,
        kwd_patterns: Vec<Pattern>,
    },
    /// OR pattern — `p1 | p2 | ...`.
    MatchOr { patterns: Vec<Pattern> },
    /// AS pattern — `pattern as name`.
    MatchAs {
        pattern: Option<Box<Pattern>>,
        name: Option<CompactString>,
    },
    /// Star pattern in sequence — `*name` or `*_`.
    MatchStar { name: Option<CompactString> },
}

// ─── Expressions ────────────────────────────────────────────────────

/// An expression with source location.
#[derive(Debug, Clone)]
pub struct Expression {
    pub node: ExpressionKind,
    pub location: SourceLocation,
}

#[derive(Debug, Clone)]
pub enum ExpressionKind {
    /// `left op right op ...` (e.g., `a and b and c`)
    BoolOp {
        op: BoolOperator,
        values: Vec<Expression>,
    },
    /// Walrus operator: `target := value`
    NamedExpr {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    /// `left op right`
    BinOp {
        left: Box<Expression>,
        op: Operator,
        right: Box<Expression>,
    },
    /// `op operand`
    UnaryOp {
        op: UnaryOperator,
        operand: Box<Expression>,
    },
    /// `lambda args: body`
    Lambda {
        args: Box<Arguments>,
        body: Box<Expression>,
    },
    /// `body if test else orelse`
    IfExp {
        test: Box<Expression>,
        body: Box<Expression>,
        orelse: Box<Expression>,
    },
    /// `{key: value, ...}`
    Dict {
        keys: Vec<Option<Expression>>,
        values: Vec<Expression>,
    },
    /// `{elts}`
    Set {
        elts: Vec<Expression>,
    },
    /// `[elt for generators]`
    ListComp {
        elt: Box<Expression>,
        generators: Vec<Comprehension>,
    },
    /// `{elt for generators}`
    SetComp {
        elt: Box<Expression>,
        generators: Vec<Comprehension>,
    },
    /// `{key: value for generators}`
    DictComp {
        key: Box<Expression>,
        value: Box<Expression>,
        generators: Vec<Comprehension>,
    },
    /// `(elt for generators)`
    GeneratorExp {
        elt: Box<Expression>,
        generators: Vec<Comprehension>,
    },
    /// `await expr`
    Await {
        value: Box<Expression>,
    },
    /// `yield [value]`
    Yield {
        value: Option<Box<Expression>>,
    },
    /// `yield from value`
    YieldFrom {
        value: Box<Expression>,
    },
    /// `left comparators` (e.g., `a < b < c`)
    Compare {
        left: Box<Expression>,
        ops: Vec<CompareOperator>,
        comparators: Vec<Expression>,
    },
    /// `func(args)`
    Call {
        func: Box<Expression>,
        args: Vec<Expression>,
        keywords: Vec<Keyword>,
    },
    /// f-string component: `{value!conversion:format_spec}`
    FormattedValue {
        value: Box<Expression>,
        conversion: Option<char>,
        format_spec: Option<Box<Expression>>,
    },
    /// f-string: `f"...{expr}..."`
    JoinedStr {
        values: Vec<Expression>,
    },
    /// A constant value (int, float, str, bytes, bool, None, Ellipsis).
    Constant {
        value: Constant,
    },
    /// `value.attr`
    Attribute {
        value: Box<Expression>,
        attr: CompactString,
        ctx: ExprContext,
    },
    /// `value[slice]`
    Subscript {
        value: Box<Expression>,
        slice: Box<Expression>,
        ctx: ExprContext,
    },
    /// `*value` (starred expression in assignments)
    Starred {
        value: Box<Expression>,
        ctx: ExprContext,
    },
    /// A variable name.
    Name {
        id: CompactString,
        ctx: ExprContext,
    },
    /// `[elts]`
    List {
        elts: Vec<Expression>,
        ctx: ExprContext,
    },
    /// `(elts)` or `elt,`
    Tuple {
        elts: Vec<Expression>,
        ctx: ExprContext,
    },
    /// `lower:upper:step`
    Slice {
        lower: Option<Box<Expression>>,
        upper: Option<Box<Expression>>,
        step: Option<Box<Expression>>,
    },
}

// ─── Constants ──────────────────────────────────────────────────────

/// Python constant values.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    None,
    Bool(bool),
    Int(BigInt),
    Float(f64),
    Complex { real: f64, imag: f64 },
    Str(CompactString),
    Bytes(Vec<u8>),
    Ellipsis,
}

/// Big integer representation (wraps num-bigint for arbitrary precision).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BigInt {
    /// Small integer that fits in i64.
    Small(i64),
    /// Arbitrary precision integer.
    Big(Box<num_bigint::BigInt>),
}

impl From<i64> for BigInt {
    fn from(v: i64) -> Self {
        BigInt::Small(v)
    }
}

impl From<num_bigint::BigInt> for BigInt {
    fn from(v: num_bigint::BigInt) -> Self {
        use num_traits::ToPrimitive;
        match v.to_i64() {
            Some(small) => BigInt::Small(small),
            None => BigInt::Big(Box::new(v)),
        }
    }
}

// ─── Operators & Enums ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operator {
    Add,
    Sub,
    Mult,
    MatMult,
    Div,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
    FloorDiv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoolOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOperator {
    Invert,
    Not,
    UAdd,
    USub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareOperator {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprContext {
    Load,
    Store,
    Del,
}

// ─── Auxiliary Types ────────────────────────────────────────────────

/// Function / lambda arguments.
#[derive(Debug, Clone)]
pub struct Arguments {
    /// Positional-only args (before `/` in signature).
    pub posonlyargs: Vec<Arg>,
    /// Regular positional args.
    pub args: Vec<Arg>,
    /// `*args` (variadic positional).
    pub vararg: Option<Arg>,
    /// Keyword-only args (after `*` or `*args`).
    pub kwonlyargs: Vec<Arg>,
    /// Default values for keyword-only args.
    pub kw_defaults: Vec<Option<Expression>>,
    /// `**kwargs` (variadic keyword).
    pub kwarg: Option<Arg>,
    /// Default values for positional args.
    pub defaults: Vec<Expression>,
}

impl Arguments {
    pub fn empty() -> Self {
        Self {
            posonlyargs: Vec::new(),
            args: Vec::new(),
            vararg: None,
            kwonlyargs: Vec::new(),
            kw_defaults: Vec::new(),
            kwarg: None,
            defaults: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub arg: CompactString,
    pub annotation: Option<Box<Expression>>,
    pub type_comment: Option<CompactString>,
    pub location: SourceLocation,
}

/// `keyword=value` in function calls or class bases.
#[derive(Debug, Clone)]
pub struct Keyword {
    pub arg: Option<CompactString>,
    pub value: Expression,
    pub location: SourceLocation,
}

/// `import name as asname`
#[derive(Debug, Clone)]
pub struct Alias {
    pub name: CompactString,
    pub asname: Option<CompactString>,
    pub location: SourceLocation,
}

/// `expr as target` in `with` statements.
#[derive(Debug, Clone)]
pub struct WithItem {
    pub context_expr: Expression,
    pub optional_vars: Option<Box<Expression>>,
}

/// `except [type [as name]]: body`
#[derive(Debug, Clone)]
pub struct ExceptHandler {
    pub typ: Option<Box<Expression>>,
    pub name: Option<CompactString>,
    pub body: Vec<Statement>,
    pub location: SourceLocation,
}

/// Comprehension: `for target in iter [if cond] ...`
#[derive(Debug, Clone)]
pub struct Comprehension {
    pub target: Expression,
    pub iter: Expression,
    pub ifs: Vec<Expression>,
    pub is_async: bool,
}

// ─── Convenience Constructors ───────────────────────────────────────

impl Expression {
    pub fn new(node: ExpressionKind, location: SourceLocation) -> Self {
        Self { node, location }
    }

    /// Create a Name expression.
    pub fn name(id: impl Into<CompactString>, ctx: ExprContext, location: SourceLocation) -> Self {
        Self::new(ExpressionKind::Name { id: id.into(), ctx }, location)
    }

    /// Create a Constant expression.
    pub fn constant(value: Constant, location: SourceLocation) -> Self {
        Self::new(ExpressionKind::Constant { value }, location)
    }
}

impl Statement {
    pub fn new(node: StatementKind, location: SourceLocation) -> Self {
        Self { node, location }
    }
}
