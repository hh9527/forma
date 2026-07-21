use crate::source::Span;

#[derive(Clone, Debug)]
pub struct Program {
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub bindings: Vec<Binding>,
    pub result: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub kind: BindingKind,
    pub name: String,
    pub annotation: Option<Expr>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Let,
    Type,
    Import,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Spanned {
        span: Span,
        expression: Box<Expr>,
    },
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Atom(String),
    Variable(String),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Dict(Vec<(String, Expr)>),
    Block(Block),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expr>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Field {
        receiver: Box<Expr>,
        field: String,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    Closure {
        parameters: Vec<String>,
        body: Block,
    },
    If {
        condition: Box<Expr>,
        then_branch: Block,
        else_branch: Block,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

impl Expr {
    pub fn spanned(span: Span, expression: Expr) -> Self {
        Self::Spanned {
            span,
            expression: Box::new(expression),
        }
    }

    pub fn span(&self) -> Option<&Span> {
        match self {
            Self::Spanned { span, .. } => Some(span),
            _ => None,
        }
    }

    pub fn unspanned(&self) -> &Self {
        match self {
            Self::Spanned { expression, .. } => expression.unspanned(),
            expression => expression,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOperator {
    Negate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    LessThan,
    Equal,
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Spanned { span: Span, pattern: Box<Pattern> },
    Wildcard,
    Binding(String),
    Int(i64),
    Float(f64),
    String(String),
    Atom(String),
    Tuple(Vec<Pattern>),
}

impl Pattern {
    pub fn spanned(span: Span, pattern: Pattern) -> Self {
        Self::Spanned {
            span,
            pattern: Box::new(pattern),
        }
    }

    pub fn span(&self) -> Option<&Span> {
        match self {
            Self::Spanned { span, .. } => Some(span),
            _ => None,
        }
    }

    pub fn unspanned(&self) -> &Self {
        match self {
            Self::Spanned { pattern, .. } => pattern.unspanned(),
            pattern => pattern,
        }
    }
}
