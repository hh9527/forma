use crate::ast::{
    BinaryOperator, Binding, BindingKind, Block, Expr, MatchArm, Pattern, Program, UnaryOperator,
};
use crate::lexer::{FrontendError, SourceLocation};
use crate::source::{Diagnostic, SourceDatabase, SourceId, Span};
use crate::syntax::xl::lexer::Token;
use crate::syntax::xl::parser::{CstData, Node, NodeRef, Rule};

#[derive(Debug)]
pub struct FrontendParse {
    pub cst: CstData,
    pub program: Option<Program>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(source_name: &str, source: &str) -> Result<Program, FrontendError> {
    let mut sources = SourceDatabase::default();
    let source_id = sources.add(source_name, source);
    let parsed = parse_registered(&sources, source_id);
    if let Some(program) = parsed.program {
        return Ok(program);
    }
    Err(compatibility_error(
        &sources,
        source_id,
        &parsed.diagnostics,
    ))
}

pub fn parse_registered(sources: &SourceDatabase, source_id: SourceId) -> FrontendParse {
    let source = sources.get(source_id);
    let parsed = crate::syntax::xl::parse(source_id, &source.text);
    let mut diagnostics = parsed.diagnostics;
    let program = if diagnostics.is_empty() {
        match Lowerer::new(source_id, &source.text, &parsed.syntax).program() {
            Ok(program) => Some(program),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        None
    };
    FrontendParse {
        cst: parsed.syntax,
        program,
        diagnostics,
    }
}

fn compatibility_error(
    sources: &SourceDatabase,
    source_id: SourceId,
    diagnostics: &[Diagnostic],
) -> FrontendError {
    let diagnostic = diagnostics.first().expect("failed parse has a diagnostic");
    let offset = diagnostic
        .labels
        .first()
        .map_or(0, |label| label.span.range.start);
    let position = sources.get(source_id).position(offset);
    FrontendError::new(
        sources.get(source_id).name.as_ref(),
        SourceLocation {
            offset,
            line: position.line,
            column: position.column,
        },
        &diagnostic.message,
    )
}

struct Lowerer<'a> {
    source_id: SourceId,
    source: &'a str,
    cst: &'a CstData,
}

impl<'a> Lowerer<'a> {
    fn new(source_id: SourceId, source: &'a str, cst: &'a CstData) -> Self {
        Self {
            source_id,
            source,
            cst,
        }
    }

    fn program(&self) -> Result<Program, Diagnostic> {
        let root = NodeRef::ROOT;
        let body_node = self
            .rule_children(root)
            .find(|node| self.rule(*node) == Some(Rule::Body))
            .or_else(|| self.first_rule(root))
            .ok_or_else(|| self.error(root, "program has no body"))?;
        Ok(Program {
            body: self.block_body(body_node)?,
            span: self.span(root),
        })
    }

    fn block_body(&self, node: NodeRef) -> Result<Block, Diagnostic> {
        let body = if self.rule(node) == Some(Rule::Block) {
            self.rule_children(node)
                .find(|child| self.rule(*child) == Some(Rule::Body))
                .ok_or_else(|| self.error(node, "block has no body"))?
        } else {
            node
        };
        let children = self.children(body).collect::<Vec<_>>();
        let mut bindings = Vec::new();
        let mut result = None;
        for child in children {
            match self.rule(child) {
                Some(
                    Rule::LetBinding
                    | Rule::TypeBinding
                    | Rule::ImportBinding
                    | Rule::NamedFunction,
                ) => bindings.push(self.binding(child)?),
                Some(Rule::Binding) => {
                    let inner = self
                        .first_rule(child)
                        .ok_or_else(|| self.error(child, "empty binding"))?;
                    bindings.push(self.binding(inner)?);
                }
                Some(_) => result = Some(self.expression(child)?),
                None if self.is_expression(child) => result = Some(self.expression(child)?),
                None => {}
            }
        }
        let result =
            result.ok_or_else(|| self.error(body, "a block requires a result expression"))?;
        Ok(Block {
            bindings,
            result: Box::new(result),
            span: self.span(node),
        })
    }

    fn binding(&self, node: NodeRef) -> Result<Binding, Diagnostic> {
        let rules = self.rule_children(node).collect::<Vec<_>>();
        let identifiers = self
            .token_children(node, Token::Identifier)
            .collect::<Vec<_>>();
        let name_node = identifiers
            .first()
            .copied()
            .ok_or_else(|| self.error(node, "binding has no name"))?;
        let name = self.text(name_node).to_owned();
        match self
            .rule(node)
            .ok_or_else(|| self.error(node, "invalid binding"))?
        {
            Rule::LetBinding => {
                let equal = self.first_token(node, Token::Equal)?;
                let equal_start = self.cst.span(equal).start;
                let value_node = self
                    .children(node)
                    .find(|child| {
                        self.is_expression(*child) && self.cst.span(*child).start > equal_start
                    })
                    .ok_or_else(|| self.error(node, "binding has no value"))?;
                let annotation = if let Some(colon) = self.token_children(node, Token::Colon).next()
                {
                    let colon_start = self.cst.span(colon).start;
                    self.children(node)
                        .find(|child| {
                            self.is_expression(*child)
                                && self.cst.span(*child).start > colon_start
                                && self.cst.span(*child).end <= equal_start
                        })
                        .map(|child| self.expression(child))
                        .transpose()?
                } else {
                    None
                };
                let value = self.expression(value_node)?;
                Ok(Binding {
                    kind: BindingKind::Let,
                    name,
                    annotation,
                    value,
                    span: self.span(node),
                })
            }
            Rule::TypeBinding => Ok(Binding {
                kind: BindingKind::Type,
                name,
                annotation: None,
                value: {
                    let equal = self.first_token(node, Token::Equal)?;
                    let start = self.cst.span(equal).start;
                    self.expression(
                        self.children(node)
                            .find(|child| {
                                self.is_expression(*child) && self.cst.span(*child).start > start
                            })
                            .ok_or_else(|| self.error(node, "type has no value"))?,
                    )?
                },
                span: self.span(node),
            }),
            Rule::ImportBinding => {
                let path = self
                    .token_children(node, Token::String)
                    .next()
                    .ok_or_else(|| self.error(node, "import has no path"))?;
                Ok(Binding {
                    kind: BindingKind::Import,
                    name,
                    annotation: None,
                    value: Expr::spanned(
                        self.span(path),
                        Expr::String(self.decode_xl_string(path)?),
                    ),
                    span: self.span(node),
                })
            }
            Rule::NamedFunction => {
                let parameters = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::Parameters))
                    .copied()
                    .ok_or_else(|| self.error(node, "function has no parameters"))?;
                let block = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::Block))
                    .copied()
                    .ok_or_else(|| self.error(node, "function has no body"))?;
                Ok(Binding {
                    kind: BindingKind::Let,
                    name,
                    annotation: None,
                    value: Expr::spanned(
                        self.span(node),
                        Expr::Closure {
                            parameters: self.parameters(parameters),
                            body: self.block_body(block)?,
                        },
                    ),
                    span: self.span(node),
                })
            }
            _ => Err(self.error(node, "unexpected binding rule")),
        }
    }

    fn expression(&self, node: NodeRef) -> Result<Expr, Diagnostic> {
        if let Node::Token(token, _) = self.cst.get(node) {
            let span = self.span(node);
            let inner = match token {
                Token::Int => Expr::Int(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "Int literal is outside the i64 range"))?,
                ),
                Token::Float => Expr::Float(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Float literal"))?,
                ),
                Token::String => Expr::String(self.decode_xl_string(node)?),
                Token::Bytes => Expr::Bytes(self.decode_xl_string(node)?.into_bytes()),
                Token::Atom => Expr::Atom(self.text(node).trim_start_matches('\'').to_owned()),
                Token::Identifier => Expr::Variable(self.text(node).to_owned()),
                _ => return Err(self.error(node, "expected expression token")),
            };
            return Ok(Expr::spanned(span, inner));
        }
        let Some(rule) = self.rule(node) else {
            return Err(self.error(node, "expected expression"));
        };
        if matches!(rule, Rule::Expression | Rule::Primary | Rule::Braced) {
            return self.expression(
                self.first_rule(node)
                    .ok_or_else(|| self.error(node, "empty expression"))?,
            );
        }
        let span = self.span(node);
        let rules = self.rule_children(node).collect::<Vec<_>>();
        let inner = match rule {
            Rule::IntExpr => Expr::Int(
                self.text(self.first_token(node, Token::Int)?)
                    .parse()
                    .map_err(|_| self.error(node, "Int literal is outside the i64 range"))?,
            ),
            Rule::FloatExpr => Expr::Float(
                self.text(self.first_token(node, Token::Float)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Float literal"))?,
            ),
            Rule::StringExpr => {
                Expr::String(self.decode_xl_string(self.first_token(node, Token::String)?)?)
            }
            Rule::BytesExpr => Expr::Bytes(
                self.decode_xl_string(self.first_token(node, Token::Bytes)?)?
                    .into_bytes(),
            ),
            Rule::AtomExpr => Expr::Atom(
                self.text(self.first_token(node, Token::Atom)?)
                    .trim_start_matches('\'')
                    .to_owned(),
            ),
            Rule::VariableExpr => Expr::Variable(
                self.text(self.first_token(node, Token::Identifier)?)
                    .to_owned(),
            ),
            Rule::ArrayExpr => Expr::Array(self.expression_children(node)?),
            Rule::ParenExpr => {
                let items = self.expression_children(node)?;
                if items.len() == 1 && self.token_children(node, Token::Comma).next().is_none() {
                    return Ok(items.into_iter().next().unwrap());
                }
                Expr::Tuple(items)
            }
            Rule::DictExpr => {
                let mut fields = Vec::new();
                for field in rules
                    .iter()
                    .copied()
                    .filter(|child| self.rule(*child) == Some(Rule::DictField))
                {
                    let key = self
                        .children(field)
                        .find(|child| {
                            matches!(
                                self.cst.get(*child),
                                Node::Token(Token::Identifier | Token::String, _)
                            )
                        })
                        .ok_or_else(|| self.error(field, "Dict field has no key"))?;
                    let name = if matches!(self.cst.get(key), Node::Token(Token::String, _)) {
                        self.decode_xl_string(key)?
                    } else {
                        self.text(key).to_owned()
                    };
                    let colon = self.first_token(field, Token::Colon)?;
                    let value = self
                        .children(field)
                        .find(|child| {
                            self.is_expression(*child)
                                && self.cst.span(*child).start > self.cst.span(colon).start
                        })
                        .ok_or_else(|| self.error(field, "Dict field has no value"))?;
                    fields.push((name, self.expression(value)?));
                }
                Expr::Dict(fields)
            }
            Rule::Block => Expr::Block(self.block_body(node)?),
            Rule::Closure => {
                let parameters = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::Parameters))
                    .copied()
                    .ok_or_else(|| self.error(node, "closure has no parameters"))?;
                let block = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::Block))
                    .copied()
                    .ok_or_else(|| self.error(node, "closure has no body"))?;
                Expr::Closure {
                    parameters: self.parameters(parameters),
                    body: self.block_body(block)?,
                }
            }
            Rule::UnaryExpr => Expr::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(
                    self.expression(
                        self.children(node)
                            .find(|child| self.is_expression(*child))
                            .ok_or_else(|| self.error(node, "unary expression has no operand"))?,
                    )?,
                ),
            },
            Rule::BinaryExpr => {
                let comparison = self.token_children(node, Token::Less).next().is_some()
                    || self
                        .token_children(node, Token::EqualEqual)
                        .next()
                        .is_some();
                if comparison
                    && self.children(node).any(|child| {
                        self.rule(child) == Some(Rule::BinaryExpr)
                            && (self.token_children(child, Token::Less).next().is_some()
                                || self
                                    .token_children(child, Token::EqualEqual)
                                    .next()
                                    .is_some())
                    })
                {
                    return Err(self.error(
                        node,
                        "comparison operators do not associate; add parentheses",
                    ));
                }
                let values = self.expression_children(node)?;
                let operator = if self.token_children(node, Token::Plus).next().is_some() {
                    BinaryOperator::Add
                } else if self.token_children(node, Token::Minus).next().is_some() {
                    BinaryOperator::Subtract
                } else if self.token_children(node, Token::Star).next().is_some() {
                    BinaryOperator::Multiply
                } else if self.token_children(node, Token::Slash).next().is_some() {
                    BinaryOperator::Divide
                } else if self.token_children(node, Token::Less).next().is_some() {
                    BinaryOperator::LessThan
                } else {
                    BinaryOperator::Equal
                };
                Expr::Binary {
                    operator,
                    left: Box::new(values[0].clone()),
                    right: Box::new(values[1].clone()),
                }
            }
            Rule::FieldExpr => {
                let receiver = self
                    .children(node)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(node, "field access has no receiver"))?;
                let field = self
                    .token_children(node, Token::Identifier)
                    .last()
                    .ok_or_else(|| self.error(node, "field access has no field"))?;
                Expr::Field {
                    receiver: Box::new(self.expression(receiver)?),
                    field: self.text(field).to_owned(),
                }
            }
            Rule::CallExpr => {
                let callee_node = self
                    .children(node)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(node, "call has no callee"))?;
                let callee = self.expression(callee_node)?;
                let arguments = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::Arguments))
                    .map_or(Ok(Vec::new()), |args| self.expression_children(*args))?;
                Expr::Call {
                    callee: Box::new(callee),
                    arguments,
                }
            }
            Rule::PipelineExpr => {
                let values = self.expression_children(node)?;
                return Ok(elaborate_pipeline(
                    span,
                    values[0].clone(),
                    values[1].clone(),
                ));
            }
            Rule::IfExpr => {
                let condition_node = self
                    .children(node)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(node, "if has no condition"))?;
                let condition = self.expression(condition_node)?;
                let blocks = rules
                    .iter()
                    .filter(|child| self.rule(**child) == Some(Rule::Block))
                    .copied()
                    .collect::<Vec<_>>();
                Expr::If {
                    condition: Box::new(condition),
                    then_branch: self.block_body(blocks[0])?,
                    else_branch: self.block_body(blocks[1])?,
                }
            }
            Rule::MatchExpr => {
                let value_node = self
                    .children(node)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(node, "match has no value"))?;
                let value = self.expression(value_node)?;
                let arms = rules
                    .iter()
                    .copied()
                    .filter(|child| self.rule(*child) == Some(Rule::MatchArm))
                    .map(|arm| self.match_arm(arm))
                    .collect::<Result<Vec<_>, _>>()?;
                Expr::Match {
                    value: Box::new(value),
                    arms,
                }
            }
            _ => return Err(self.error(node, format!("unexpected expression rule {rule:?}"))),
        };
        Ok(Expr::spanned(span, inner))
    }

    fn match_arm(&self, node: NodeRef) -> Result<MatchArm, Diagnostic> {
        let arrow = self.first_token(node, Token::FatArrow)?;
        let arrow_start = self.cst.span(arrow).start;
        let pattern = self
            .children(node)
            .find(|child| self.is_pattern(*child) && self.cst.span(*child).end <= arrow_start)
            .ok_or_else(|| self.error(node, "match arm has no pattern"))?;
        let value = self
            .children(node)
            .find(|child| self.is_expression(*child) && self.cst.span(*child).start > arrow_start)
            .ok_or_else(|| self.error(node, "match arm has no value"))?;
        Ok(MatchArm {
            pattern: self.pattern(pattern)?,
            value: self.expression(value)?,
            span: self.span(node),
        })
    }

    fn pattern(&self, node: NodeRef) -> Result<Pattern, Diagnostic> {
        if let Node::Token(token, _) = self.cst.get(node) {
            let inner = match token {
                Token::Identifier => {
                    let name = self.text(node);
                    if name == "_" {
                        Pattern::Wildcard
                    } else {
                        Pattern::Binding(name.to_owned())
                    }
                }
                Token::Int => Pattern::Int(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Int pattern"))?,
                ),
                Token::Float => Pattern::Float(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Float pattern"))?,
                ),
                Token::String => Pattern::String(self.decode_xl_string(node)?),
                Token::Atom => Pattern::Atom(self.text(node).trim_start_matches('\'').to_owned()),
                _ => return Err(self.error(node, "expected pattern token")),
            };
            return Ok(Pattern::spanned(self.span(node), inner));
        }
        let rule = self
            .rule(node)
            .ok_or_else(|| self.error(node, "expected pattern"))?;
        if rule == Rule::Pattern {
            return self.pattern(
                self.first_rule(node)
                    .ok_or_else(|| self.error(node, "empty pattern"))?,
            );
        }
        let inner = match rule {
            Rule::IdentifierPattern => {
                let name = self.text(self.first_token(node, Token::Identifier)?);
                if name == "_" {
                    Pattern::Wildcard
                } else {
                    Pattern::Binding(name.to_owned())
                }
            }
            Rule::IntPattern => Pattern::Int(
                self.text(self.first_token(node, Token::Int)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Int pattern"))?,
            ),
            Rule::FloatPattern => Pattern::Float(
                self.text(self.first_token(node, Token::Float)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Float pattern"))?,
            ),
            Rule::StringPattern => {
                Pattern::String(self.decode_xl_string(self.first_token(node, Token::String)?)?)
            }
            Rule::AtomPattern => Pattern::Atom(
                self.text(self.first_token(node, Token::Atom)?)
                    .trim_start_matches('\'')
                    .to_owned(),
            ),
            Rule::TuplePattern => Pattern::Tuple(
                self.children(node)
                    .filter(|child| self.is_pattern(*child))
                    .map(|child| self.pattern(child))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err(self.error(node, "unexpected pattern rule")),
        };
        Ok(Pattern::spanned(self.span(node), inner))
    }

    fn parameters(&self, node: NodeRef) -> Vec<String> {
        self.token_children(node, Token::Identifier)
            .map(|child| self.text(child).to_owned())
            .collect()
    }
    fn expression_children(&self, node: NodeRef) -> Result<Vec<Expr>, Diagnostic> {
        self.children(node)
            .filter(|child| self.is_expression(*child))
            .map(|child| self.expression(child))
            .collect()
    }
    fn is_expression(&self, node: NodeRef) -> bool {
        matches!(
            self.cst.get(node),
            Node::Token(
                Token::Int
                    | Token::Float
                    | Token::String
                    | Token::Bytes
                    | Token::Atom
                    | Token::Identifier,
                _
            )
        ) || matches!(
            self.rule(node),
            Some(
                Rule::Expression
                    | Rule::Primary
                    | Rule::Braced
                    | Rule::ArrayExpr
                    | Rule::AtomExpr
                    | Rule::BinaryExpr
                    | Rule::Block
                    | Rule::BytesExpr
                    | Rule::CallExpr
                    | Rule::Closure
                    | Rule::DictExpr
                    | Rule::FieldExpr
                    | Rule::FloatExpr
                    | Rule::IfExpr
                    | Rule::IntExpr
                    | Rule::MatchExpr
                    | Rule::ParenExpr
                    | Rule::PipelineExpr
                    | Rule::StringExpr
                    | Rule::UnaryExpr
                    | Rule::VariableExpr
            )
        )
    }
    fn is_pattern(&self, node: NodeRef) -> bool {
        matches!(
            self.cst.get(node),
            Node::Token(
                Token::Identifier | Token::Int | Token::Float | Token::String | Token::Atom,
                _
            )
        ) || matches!(
            self.rule(node),
            Some(
                Rule::Pattern
                    | Rule::AtomPattern
                    | Rule::FloatPattern
                    | Rule::IdentifierPattern
                    | Rule::IntPattern
                    | Rule::StringPattern
                    | Rule::TuplePattern
            )
        )
    }
    fn children(&self, node: NodeRef) -> impl Iterator<Item = NodeRef> + '_ {
        self.cst.children(node)
    }
    fn rule_children(&self, node: NodeRef) -> impl Iterator<Item = NodeRef> + '_ {
        self.children(node)
            .filter(|child| matches!(self.cst.get(*child), Node::Rule(..)))
    }
    fn token_children(&self, node: NodeRef, token: Token) -> impl Iterator<Item = NodeRef> + '_ {
        self.children(node).filter(
            move |child| matches!(self.cst.get(*child), Node::Token(found, _) if found == token),
        )
    }
    fn first_rule(&self, node: NodeRef) -> Option<NodeRef> {
        self.rule_children(node).next()
    }
    fn first_token(&self, node: NodeRef, token: Token) -> Result<NodeRef, Diagnostic> {
        self.token_children(node, token)
            .next()
            .ok_or_else(|| self.error(node, format!("missing {token:?}")))
    }
    fn rule(&self, node: NodeRef) -> Option<Rule> {
        match self.cst.get(node) {
            Node::Rule(rule, _) => Some(rule),
            Node::Token(..) => None,
        }
    }
    fn span(&self, node: NodeRef) -> Span {
        Span {
            source: self.source_id,
            range: self.cst.span(node),
        }
    }
    fn text(&self, node: NodeRef) -> &str {
        &self.source[self.cst.span(node)]
    }
    fn error(&self, node: NodeRef, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(message, self.span(node))
    }

    fn decode_xl_string(&self, node: NodeRef) -> Result<String, Diagnostic> {
        let text = self.text(node);
        let quoted = text.strip_prefix('b').unwrap_or(text);
        let mut chars = quoted[1..quoted.len() - 1].chars();
        let mut output = String::new();
        while let Some(character) = chars.next() {
            if character != '\\' {
                output.push(character);
                continue;
            }
            output.push(match chars.next() {
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('"') => '"',
                Some('\\') => '\\',
                Some(other) => {
                    return Err(self.error(node, format!("unsupported escape \\{other}")));
                }
                None => return Err(self.error(node, "unterminated string escape")),
            });
        }
        Ok(output)
    }
}

fn elaborate_pipeline(span: Span, left: Expr, right: Expr) -> Expr {
    let expression = match right.unspanned() {
        Expr::Call { callee, arguments } => {
            let mut arguments = arguments.clone();
            arguments.insert(0, left);
            Expr::Call {
                callee: callee.clone(),
                arguments,
            }
        }
        _ => Expr::Call {
            callee: Box::new(right),
            arguments: vec![left],
        },
    };
    Expr::spanned(span, expression)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_directly_from_cst_with_spans_and_precedence() {
        let mut sources = SourceDatabase::default();
        let id = sources.add("test.xl", "let x = 1; x == 2");
        let parsed = parse_registered(&sources, id);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let program = parsed.program.unwrap();
        assert_eq!(program.span.range, 0..17);
        assert_eq!(program.body.bindings[0].span.range, 0..10);
        assert!(matches!(
            program.body.result.unspanned(),
            Expr::Binary {
                operator: BinaryOperator::Equal,
                ..
            }
        ));
    }

    #[test]
    fn exposes_all_recovery_diagnostics() {
        let mut sources = SourceDatabase::default();
        let id = sources.add("broken.xl", "let x = ; let y = ; y");
        let parsed = parse_registered(&sources, id);
        assert!(parsed.program.is_none());
        assert!(parsed.diagnostics.len() >= 2);
    }

    #[test]
    fn comparisons_share_a_non_associative_precedence_level() {
        let chained = parse("test", "1 < 2 == 3").unwrap_err();
        assert!(chained.message.contains("do not associate"));
        assert!(parse("test", "(1 < 2) == 3").is_ok());
        assert!(parse("test", "1 < (2 == 3)").is_ok());
    }
}
