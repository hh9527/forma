use crate::source::{Located, Location};

pub type Identifier = Located<String>;
pub type Program = Located<ProgramKind>;
pub type Block = Located<BlockKind>;
pub type Binding = Located<BindingData>;
pub type Expr = Located<ExprKind>;
pub type Pattern = Located<PatternKind>;
pub type MatchArm = Located<MatchArmKind>;
pub type DictField = Located<DictFieldKind>;
pub type Decorator = Located<DecoratorKind>;
pub type StringPart = Located<StringPartKind>;

#[derive(Clone, Debug)]
pub struct ClosureParameter {
    pub name: Identifier,
    pub annotation: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct ProgramKind {
    pub manifest: Option<Expr>,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct BlockKind {
    pub bindings: Vec<Binding>,
    pub result: Box<Expr>,
}

#[derive(Clone, Debug)]
pub struct BindingData {
    pub decorators: Vec<Decorator>,
    pub kind: BindingKind,
    pub name: Identifier,
    pub type_parameters: Vec<Identifier>,
    pub annotation: Option<Expr>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct DecoratorKind {
    pub callee: Expr,
    pub arguments: Vec<Expr>,
    pub configured: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Let,
    Decl,
    Def,
    Native,
    Type,
    Import,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    String(String),
    InterpolatedString(Vec<StringPart>),
    Bytes(Vec<u8>),
    Atom(String),
    Variable(Identifier),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Dict(Vec<DictField>),
    Block(Block),
    Unary {
        operator: Located<UnaryOperator>,
        operand: Box<Expr>,
    },
    Binary {
        operator: Located<BinaryOperator>,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Field {
        receiver: Box<Expr>,
        field: Identifier,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
    },
    TypeApply {
        callee: Box<Expr>,
        arguments: Vec<TypeArgument>,
    },
    Interpreter {
        operand: Box<Expr>,
        elaboration: Box<Expr>,
    },
    Closure {
        parameters: Vec<ClosureParameter>,
        result_annotation: Option<Box<Expr>>,
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

pub type TypeArgument = Located<TypeArgumentKind>;

#[derive(Clone, Debug)]
pub enum TypeArgumentKind {
    Explicit(Expr),
    Infer,
}

#[derive(Clone, Debug)]
pub enum StringPartKind {
    Text(String),
    Expression(Expr),
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
pub struct DictFieldKind {
    pub decorators: Vec<Decorator>,
    pub name: Identifier,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct MatchArmKind {
    pub pattern: Pattern,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Wildcard,
    Binding(Identifier),
    Int(i64),
    Float(f64),
    String(String),
    Atom(String),
    Tagged { tag: String, payload: Box<Pattern> },
    Tuple(Vec<Pattern>),
}

pub fn located<T>(value: T, location: Location) -> Located<T> {
    Located::new(value, location)
}
