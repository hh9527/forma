#[derive(Clone, Debug)]
pub struct Program {
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub bindings: Vec<Binding>,
    pub result: Box<Expr>,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub kind: BindingKind,
    pub name: String,
    pub annotation: Option<Expr>,
    pub value: Expr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Let,
    Type,
    Import,
}

#[derive(Clone, Debug)]
pub enum Expr {
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
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard,
    Binding(String),
    Int(i64),
    Float(f64),
    String(String),
    Atom(String),
    Tuple(Vec<Pattern>),
}
