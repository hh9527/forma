use crate::ast::{
    BinaryOperator, Binding, BindingData, BindingKind, Block, BlockKind, Decorator, DecoratorKind,
    DictFieldKind, Expr, ExprKind, Identifier, MatchArm, MatchArmKind, Pattern, PatternKind,
    Program, ProgramKind, StringPartKind, UnaryOperator, located,
};
use crate::lexer::{FrontendError, SourceLocation};
use crate::source::{Diagnostic, Location, SourceDatabase, SourceId};
use crate::syntax::xl::lexer::Token;
use crate::syntax::xl::parser::{CstData, Node, NodeRef, Rule};

#[derive(Debug)]
pub struct FrontendParse {
    pub cst: CstData,
    pub program: Option<Program>,
    pub recovered: RecoveredProgram,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub struct RecoveredProgram {
    pub location: Location,
    pub bindings: Vec<Binding>,
    pub result: Option<Expr>,
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
    let parsed = crate::syntax::xl::parse_document(source_id, source.text());
    let mut diagnostics = parsed.diagnostics;
    let lowerer = Lowerer::new(source_id, source.text(), &parsed.syntax);
    let recovered = lowerer.recover_program(&mut diagnostics);
    let program = if diagnostics.is_empty() {
        match lowerer.program() {
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
        recovered,
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
        .map_or(0, |label| label.location.start);
    let position = sources.get(source_id).position(offset);
    FrontendError::new(
        sources.get(source_id).name.as_ref(),
        SourceLocation {
            offset: offset as usize,
            line: position.line,
            column: position.column,
        },
        &diagnostic.message,
    )
}

struct Lowerer<'a> {
    source_id: SourceId,
    source: &'a crate::document::DocumentText,
    cst: &'a CstData,
}

enum CallArgument {
    Expression(Expr),
    Bare {
        node: NodeRef,
        location: Location,
    },
    Indexed {
        node: NodeRef,
        index: usize,
        location: Location,
    },
}

impl<'a> Lowerer<'a> {
    fn new(
        source_id: SourceId,
        source: &'a crate::document::DocumentText,
        cst: &'a CstData,
    ) -> Self {
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
        Ok(located(
            ProgramKind {
                body: self.block_body(body_node)?,
            },
            self.location(root),
        ))
    }

    fn recover_program(&self, diagnostics: &mut Vec<Diagnostic>) -> RecoveredProgram {
        use crate::syntax::xl::ast::{AstNode, Program as SyntaxProgram};

        let root = SyntaxProgram::root(self.cst);
        let mut bindings = Vec::new();
        let mut result = None;
        if let Some(body) = root.body() {
            for binding in body.bindings() {
                match self.binding(binding.syntax().node_ref()) {
                    Ok(binding) => bindings.push(binding),
                    Err(diagnostic) => push_unique_diagnostic(diagnostics, diagnostic),
                }
            }
            if let Some(expression) = body.result() {
                match self.expression(expression.syntax().node_ref()) {
                    Ok(expression) => result = Some(expression),
                    Err(diagnostic) => push_unique_diagnostic(diagnostics, diagnostic),
                }
            }
        }
        diagnostics.sort_by_key(|diagnostic| {
            diagnostic
                .labels
                .first()
                .map_or(0, |label| label.location.start)
        });
        RecoveredProgram {
            location: self.location(NodeRef::ROOT),
            bindings,
            result,
        }
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
                    | Rule::DeclBinding
                    | Rule::DefBinding
                    | Rule::NativeBinding
                    | Rule::TypeBinding
                    | Rule::ImportBinding,
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
        Ok(located(
            BlockKind {
                bindings,
                result: Box::new(result),
            },
            self.location(node),
        ))
    }

    fn binding(&self, node: NodeRef) -> Result<Binding, Diagnostic> {
        let identifiers = self
            .token_children(node, Token::Identifier)
            .collect::<Vec<_>>();
        let name_node = identifiers
            .first()
            .copied()
            .ok_or_else(|| self.error(node, "binding has no name"))?;
        let name = self.identifier(name_node);
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
                Ok(located(
                    BindingData {
                        decorators: Vec::new(),
                        kind: BindingKind::Let,
                        name,
                        type_parameters: Vec::new(),
                        annotation,
                        value,
                    },
                    self.location(node),
                ))
            }
            Rule::DeclBinding => {
                let scheme = self
                    .rule_children(node)
                    .find(|child| self.rule(*child) == Some(Rule::TypeScheme))
                    .ok_or_else(|| self.error(node, "declaration has no type scheme"))?;
                let type_parameters = self
                    .rule_children(scheme)
                    .find(|child| self.rule(*child) == Some(Rule::TypeParameters))
                    .map(|parameters| {
                        self.token_children(parameters, Token::Identifier)
                            .map(|parameter| {
                                located(self.text(parameter).into_owned(), self.location(parameter))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let contract_node = self
                    .rule_children(scheme)
                    .find(|child| {
                        matches!(
                            self.rule(*child),
                            Some(Rule::Contract | Rule::ContractExpr | Rule::FunctionContract)
                        )
                    })
                    .ok_or_else(|| self.error(node, "declaration has no contract"))?;
                let contract = self.contract_expression(contract_node)?;
                Ok(located(
                    BindingData {
                        decorators: Vec::new(),
                        kind: BindingKind::Decl,
                        name,
                        type_parameters,
                        annotation: Some(contract.clone()),
                        value: contract,
                    },
                    self.location(node),
                ))
            }
            Rule::NativeBinding => {
                let scheme = self
                    .rule_children(node)
                    .find(|child| self.rule(*child) == Some(Rule::TypeScheme))
                    .ok_or_else(|| self.error(node, "native declaration has no type scheme"))?;
                let type_parameters = self
                    .rule_children(scheme)
                    .find(|child| self.rule(*child) == Some(Rule::TypeParameters))
                    .map(|parameters| {
                        self.token_children(parameters, Token::Identifier)
                            .map(|parameter| {
                                located(self.text(parameter).into_owned(), self.location(parameter))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let contract_node = self
                    .rule_children(scheme)
                    .find(|child| {
                        matches!(
                            self.rule(*child),
                            Some(Rule::Contract | Rule::ContractExpr | Rule::FunctionContract)
                        )
                    })
                    .ok_or_else(|| self.error(node, "native declaration has no contract"))?;
                let contract = self.contract_expression(contract_node)?;
                Ok(located(
                    BindingData {
                        decorators: Vec::new(),
                        kind: BindingKind::Native,
                        name,
                        type_parameters,
                        annotation: Some(contract.clone()),
                        value: contract,
                    },
                    self.location(node),
                ))
            }
            Rule::DefBinding => {
                let equal = self.first_token(node, Token::Equal)?;
                let scheme = self
                    .rule_children(node)
                    .find(|child| self.rule(*child) == Some(Rule::TypeScheme));
                let type_parameters = scheme
                    .and_then(|scheme| {
                        self.rule_children(scheme)
                            .find(|child| self.rule(*child) == Some(Rule::TypeParameters))
                    })
                    .map(|parameters| {
                        self.token_children(parameters, Token::Identifier)
                            .map(|parameter| {
                                located(self.text(parameter).into_owned(), self.location(parameter))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let annotation = scheme
                    .map(|scheme| {
                        self.rule_children(scheme)
                            .find(|child| {
                                matches!(
                                    self.rule(*child),
                                    Some(
                                        Rule::Contract
                                            | Rule::ContractExpr
                                            | Rule::FunctionContract
                                    )
                                )
                            })
                            .ok_or_else(|| self.error(node, "definition has no contract"))
                            .and_then(|contract| self.contract_expression(contract))
                    })
                    .transpose()?;
                let value_node = self
                    .children(node)
                    .find(|child| {
                        self.is_expression(*child)
                            && self.cst.span(*child).start > self.cst.span(equal).start
                    })
                    .ok_or_else(|| self.error(node, "definition has no value"))?;
                Ok(located(
                    BindingData {
                        decorators: Vec::new(),
                        kind: BindingKind::Def,
                        name,
                        type_parameters,
                        annotation,
                        value: self.expression(value_node)?,
                    },
                    self.location(node),
                ))
            }
            Rule::TypeBinding => {
                let decorators = self.decorators(node)?;
                let equal = self.first_token(node, Token::Equal)?;
                let start = self.cst.span(equal).start;
                let value = self.expression(
                    self.children(node)
                        .find(|child| {
                            self.is_expression(*child) && self.cst.span(*child).start > start
                        })
                        .ok_or_else(|| self.error(node, "type has no value"))?,
                )?;
                let value =
                    self.apply_decorators(&decorators, "Type", &name, value, self.location(node));
                Ok(located(
                    BindingData {
                        decorators,
                        kind: BindingKind::Type,
                        name,
                        type_parameters: Vec::new(),
                        annotation: None,
                        value,
                    },
                    self.location(node),
                ))
            }
            Rule::ImportBinding => {
                let path = self
                    .rule_children(node)
                    .find(|child| self.rule(*child) == Some(Rule::StringLiteral))
                    .ok_or_else(|| self.error(node, "import has no path"))?;
                Ok(located(
                    BindingData {
                        decorators: Vec::new(),
                        kind: BindingKind::Import,
                        name,
                        type_parameters: Vec::new(),
                        annotation: None,
                        value: located(
                            ExprKind::String(self.plain_string(path, "import path")?),
                            self.location(path),
                        ),
                    },
                    self.location(node),
                ))
            }
            _ => Err(self.error(node, "unexpected binding rule")),
        }
    }

    fn expression(&self, node: NodeRef) -> Result<Expr, Diagnostic> {
        if let Node::Token(token, _) = self.cst.get(node) {
            let location = self.location(node);
            let inner = match token {
                Token::Int => ExprKind::Int(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "Int literal is outside the i64 range"))?,
                ),
                Token::Float => ExprKind::Float(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Float literal"))?,
                ),
                Token::Bytes => ExprKind::Bytes(self.decode_xl_string(node)?.into_bytes()),
                Token::Atom => ExprKind::Atom(self.text(node).trim_start_matches('\'').to_owned()),
                Token::Identifier => ExprKind::Variable(self.identifier(node)),
                _ => return Err(self.error(node, "expected expression token")),
            };
            return Ok(located(inner, location));
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
        let location = self.location(node);
        let rules = self.rule_children(node).collect::<Vec<_>>();
        let inner = match rule {
            Rule::IntExpr => ExprKind::Int(
                self.text(self.first_token(node, Token::Int)?)
                    .parse()
                    .map_err(|_| self.error(node, "Int literal is outside the i64 range"))?,
            ),
            Rule::FloatExpr => ExprKind::Float(
                self.text(self.first_token(node, Token::Float)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Float literal"))?,
            ),
            Rule::StringExpr => return self.string_expression(node),
            Rule::BytesExpr => ExprKind::Bytes(
                self.decode_xl_string(self.first_token(node, Token::Bytes)?)?
                    .into_bytes(),
            ),
            Rule::AtomExpr => ExprKind::Atom(
                self.text(self.first_token(node, Token::Atom)?)
                    .trim_start_matches('\'')
                    .to_owned(),
            ),
            Rule::VariableExpr => {
                ExprKind::Variable(self.identifier(self.first_token(node, Token::Identifier)?))
            }
            Rule::ArrayExpr => ExprKind::Array(self.expression_children(node)?),
            Rule::ParenExpr => {
                let items = self.expression_children(node)?;
                if items.len() == 1 && self.token_children(node, Token::Comma).next().is_none() {
                    return Ok(items.into_iter().next().unwrap());
                }
                ExprKind::Tuple(items)
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
                            matches!(self.cst.get(*child), Node::Token(Token::Identifier, _))
                                || self.rule(*child) == Some(Rule::StringLiteral)
                        })
                        .ok_or_else(|| self.error(field, "Dict field has no key"))?;
                    let name = if self.rule(key) == Some(Rule::StringLiteral) {
                        located(
                            self.plain_string(key, "Dict field name")?,
                            self.location(key),
                        )
                    } else {
                        self.identifier(key)
                    };
                    let colon = self.first_token(field, Token::Colon)?;
                    let value = self
                        .children(field)
                        .find(|child| {
                            self.is_expression(*child)
                                && self.cst.span(*child).start > self.cst.span(colon).start
                        })
                        .ok_or_else(|| self.error(field, "Dict field has no value"))?;
                    let decorators = self.decorators(field)?;
                    let value = self.expression(value)?;
                    let value = self.apply_decorators(
                        &decorators,
                        "Field",
                        &name,
                        value,
                        self.location(field),
                    );
                    fields.push(located(
                        DictFieldKind {
                            decorators,
                            name,
                            value,
                        },
                        self.location(field),
                    ));
                }
                ExprKind::Dict(fields)
            }
            Rule::Block => ExprKind::Block(self.block_body(node)?),
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
                ExprKind::Closure {
                    parameters: self.parameters(parameters),
                    body: self.block_body(block)?,
                }
            }
            Rule::FunctionContract => return self.contract_expression(node),
            Rule::UnaryExpr => ExprKind::Unary {
                operator: located(
                    UnaryOperator::Negate,
                    self.location(self.first_token(node, Token::Minus)?),
                ),
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
                let (operator, operator_node) =
                    if let Some(operator) = self.token_children(node, Token::Plus).next() {
                        (BinaryOperator::Add, operator)
                    } else if let Some(operator) = self.token_children(node, Token::Minus).next() {
                        (BinaryOperator::Subtract, operator)
                    } else if let Some(operator) = self.token_children(node, Token::Star).next() {
                        (BinaryOperator::Multiply, operator)
                    } else if let Some(operator) = self.token_children(node, Token::Slash).next() {
                        (BinaryOperator::Divide, operator)
                    } else if let Some(operator) = self.token_children(node, Token::Less).next() {
                        (BinaryOperator::LessThan, operator)
                    } else {
                        (
                            BinaryOperator::Equal,
                            self.first_token(node, Token::EqualEqual)?,
                        )
                    };
                ExprKind::Binary {
                    operator: located(operator, self.location(operator_node)),
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
                ExprKind::Field {
                    receiver: Box::new(self.expression(receiver)?),
                    field: self.identifier(field),
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
                ExprKind::Call {
                    callee: Box::new(callee),
                    arguments,
                }
            }
            Rule::SectionExpr => {
                let callee_node = self
                    .children(node)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(node, "call section has no callee"))?;
                let callee = self.expression(callee_node)?;
                let arguments_node = rules
                    .iter()
                    .find(|child| self.rule(**child) == Some(Rule::SectionArguments))
                    .copied();
                return self.section_expression(callee, arguments_node, node, location);
            }
            Rule::PipelineExpr => {
                let values = self.expression_children(node)?;
                return Ok(elaborate_pipeline(
                    location,
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
                ExprKind::If {
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
                ExprKind::Match {
                    value: Box::new(value),
                    arms,
                }
            }
            _ => return Err(self.error(node, format!("unexpected expression rule {rule:?}"))),
        };
        Ok(located(inner, location))
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
        Ok(located(
            MatchArmKind {
                pattern: self.pattern(pattern)?,
                value: self.expression(value)?,
            },
            self.location(node),
        ))
    }

    fn pattern(&self, node: NodeRef) -> Result<Pattern, Diagnostic> {
        if let Node::Token(token, _) = self.cst.get(node) {
            let inner = match token {
                Token::Identifier | Token::Placeholder => {
                    let name = self.text(node);
                    if name == "_" {
                        PatternKind::Wildcard
                    } else {
                        PatternKind::Binding(self.identifier(node))
                    }
                }
                Token::Int => PatternKind::Int(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Int pattern"))?,
                ),
                Token::Float => PatternKind::Float(
                    self.text(node)
                        .parse()
                        .map_err(|_| self.error(node, "invalid Float pattern"))?,
                ),
                Token::Atom => {
                    PatternKind::Atom(self.text(node).trim_start_matches('\'').to_owned())
                }
                _ => return Err(self.error(node, "expected pattern token")),
            };
            return Ok(located(inner, self.location(node)));
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
                if self
                    .token_children(node, Token::Placeholder)
                    .next()
                    .is_some()
                {
                    PatternKind::Wildcard
                } else {
                    PatternKind::Binding(
                        self.identifier(self.first_token(node, Token::Identifier)?),
                    )
                }
            }
            Rule::IntPattern => PatternKind::Int(
                self.text(self.first_token(node, Token::Int)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Int pattern"))?,
            ),
            Rule::FloatPattern => PatternKind::Float(
                self.text(self.first_token(node, Token::Float)?)
                    .parse()
                    .map_err(|_| self.error(node, "invalid Float pattern"))?,
            ),
            Rule::StringPattern => {
                let string = self
                    .rule_children(node)
                    .find(|child| self.rule(*child) == Some(Rule::StringLiteral))
                    .ok_or_else(|| self.error(node, "string pattern has no literal"))?;
                PatternKind::String(self.plain_string(string, "string pattern")?)
            }
            Rule::AtomPattern => PatternKind::Atom(
                self.text(self.first_token(node, Token::Atom)?)
                    .trim_start_matches('\'')
                    .to_owned(),
            ),
            Rule::TaggedPattern => PatternKind::Tagged {
                tag: self
                    .text(self.first_token(node, Token::Atom)?)
                    .trim_start_matches('\'')
                    .to_owned(),
                payload: Box::new(
                    self.pattern(
                        self.children(node)
                            .filter(|child| {
                                !matches!(self.cst.get(*child), Node::Token(Token::Atom, _))
                            })
                            .find(|child| self.is_pattern(*child))
                            .ok_or_else(|| self.error(node, "tagged pattern has no payload"))?,
                    )?,
                ),
            },
            Rule::TuplePattern => PatternKind::Tuple(
                self.children(node)
                    .filter(|child| self.is_pattern(*child))
                    .map(|child| self.pattern(child))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err(self.error(node, "unexpected pattern rule")),
        };
        Ok(located(inner, self.location(node)))
    }

    fn parameters(&self, node: NodeRef) -> Vec<Identifier> {
        self.token_children(node, Token::Identifier)
            .map(|child| self.identifier(child))
            .collect()
    }

    fn function_contract_expression(
        &self,
        parameters: Vec<Expr>,
        result: Expr,
        location: Location,
    ) -> Expr {
        let parameters = located(ExprKind::Array(parameters), location);
        let callee_name = located("Fn".to_owned(), location);
        located(
            ExprKind::Call {
                callee: Box::new(located(ExprKind::Variable(callee_name), location)),
                arguments: vec![parameters, result],
            },
            location,
        )
    }

    fn contract_expression(&self, node: NodeRef) -> Result<Expr, Diagnostic> {
        let location = self.location(node);
        match self.rule(node) {
            Some(Rule::Contract) => {
                let inner = self
                    .first_rule(node)
                    .ok_or_else(|| self.error(node, "empty contract"))?;
                self.contract_expression(inner)
            }
            Some(Rule::ContractExpr) => {
                let name = self.identifier(self.first_token(node, Token::Identifier)?);
                let arguments = self
                    .rule_children(node)
                    .filter(|child| {
                        matches!(
                            self.rule(*child),
                            Some(Rule::Contract | Rule::ContractExpr | Rule::FunctionContract)
                        )
                    })
                    .map(|child| self.contract_expression(child))
                    .collect::<Result<Vec<_>, _>>()?;
                if arguments.is_empty() {
                    Ok(located(ExprKind::Variable(name), location))
                } else {
                    Ok(located(
                        ExprKind::Call {
                            callee: Box::new(located(ExprKind::Variable(name), location)),
                            arguments,
                        },
                        location,
                    ))
                }
            }
            Some(Rule::FunctionContract) => {
                let mut parts = self
                    .rule_children(node)
                    .filter(|child| {
                        matches!(
                            self.rule(*child),
                            Some(Rule::Contract | Rule::ContractExpr | Rule::FunctionContract)
                        )
                    })
                    .map(|child| self.contract_expression(child))
                    .collect::<Result<Vec<_>, _>>()?;
                let result = parts
                    .pop()
                    .ok_or_else(|| self.error(node, "function contract has no result"))?;
                Ok(self.function_contract_expression(parts, result, location))
            }
            _ => Err(self.error(node, "invalid contract")),
        }
    }

    fn decorators(&self, node: NodeRef) -> Result<Vec<Decorator>, Diagnostic> {
        self.rule_children(node)
            .filter(|child| self.rule(*child) == Some(Rule::Decorator))
            .map(|decorator| {
                let path = self
                    .rule_children(decorator)
                    .find(|child| self.rule(*child) == Some(Rule::DecoratorPath))
                    .ok_or_else(|| self.error(decorator, "decorator has no path"))?;
                let mut identifiers = self
                    .token_children(path, Token::Identifier)
                    .map(|token| self.identifier(token));
                let first = identifiers
                    .next()
                    .ok_or_else(|| self.error(path, "decorator path is empty"))?;
                let mut callee = located(ExprKind::Variable(first.clone()), first.location);
                for field in identifiers {
                    let location = Location::new(
                        callee.location.source,
                        crate::source::TextRange::from_usize(
                            callee.location.start as usize..field.location.end as usize,
                        )
                        .expect("decorator path is within a parsed source"),
                    );
                    callee = located(
                        ExprKind::Field {
                            receiver: Box::new(callee),
                            field,
                        },
                        location,
                    );
                }
                let arguments_node = self
                    .rule_children(decorator)
                    .find(|child| self.rule(*child) == Some(Rule::Arguments));
                let arguments = arguments_node
                    .map(|arguments| self.expression_children(arguments))
                    .transpose()?
                    .unwrap_or_default();
                Ok(located(
                    DecoratorKind {
                        callee,
                        arguments,
                        configured: arguments_node.is_some(),
                    },
                    self.location(decorator),
                ))
            })
            .collect()
    }

    fn apply_decorators(
        &self,
        decorators: &[Decorator],
        kind: &str,
        name: &Identifier,
        mut value: Expr,
        target_location: Location,
    ) -> Expr {
        let context = located(
            ExprKind::Dict(vec![
                located(
                    DictFieldKind {
                        decorators: Vec::new(),
                        name: located("kind".to_owned(), target_location),
                        value: located(ExprKind::Atom(kind.to_owned()), target_location),
                    },
                    target_location,
                ),
                located(
                    DictFieldKind {
                        decorators: Vec::new(),
                        name: located("name".to_owned(), name.location),
                        value: located(ExprKind::String(name.value.clone()), name.location),
                    },
                    name.location,
                ),
            ]),
            target_location,
        );
        for decorator in decorators.iter().rev() {
            let callee = if decorator.value.configured {
                located(
                    ExprKind::Call {
                        callee: Box::new(decorator.value.callee.clone()),
                        arguments: decorator.value.arguments.clone(),
                    },
                    decorator.location,
                )
            } else {
                decorator.value.callee.clone()
            };
            value = located(
                ExprKind::Call {
                    callee: Box::new(callee),
                    arguments: vec![context.clone(), value],
                },
                decorator.location,
            );
        }
        value
    }

    fn expression_children(&self, node: NodeRef) -> Result<Vec<Expr>, Diagnostic> {
        self.children(node)
            .filter(|child| self.is_expression(*child))
            .map(|child| self.expression(child))
            .collect()
    }

    fn section_expression(
        &self,
        callee: Expr,
        arguments_node: Option<NodeRef>,
        section_node: NodeRef,
        location: Location,
    ) -> Result<Expr, Diagnostic> {
        let Some(arguments_node) = arguments_node else {
            return Err(self.error(section_node, "call section has no arguments"));
        };
        let arguments = self
            .rule_children(arguments_node)
            .filter(|child| self.rule(*child) == Some(Rule::Argument))
            .map(|argument| self.call_argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        elaborate_call_section(callee, arguments, section_node, location)
            .map_err(|(node, message)| self.error(node, message))
    }

    fn call_argument(&self, node: NodeRef) -> Result<CallArgument, Diagnostic> {
        if let Some(placeholder) = self.token_children(node, Token::Placeholder).next() {
            return Ok(CallArgument::Bare {
                node: placeholder,
                location: self.location(placeholder),
            });
        }
        if let Some(placeholder) = self.token_children(node, Token::IndexedPlaceholder).next() {
            let text = self.text(placeholder);
            let index = text[1..].parse::<usize>().map_err(|_| {
                self.error(placeholder, "placeholder index exceeds the supported range")
            })?;
            return Ok(CallArgument::Indexed {
                node: placeholder,
                index,
                location: self.location(placeholder),
            });
        }
        let expression = self
            .children(node)
            .find(|child| self.is_expression(*child))
            .ok_or_else(|| self.error(node, "call argument has no expression"))?;
        Ok(CallArgument::Expression(self.expression(expression)?))
    }
    fn is_expression(&self, node: NodeRef) -> bool {
        matches!(
            self.cst.get(node),
            Node::Token(
                Token::Int | Token::Float | Token::Bytes | Token::Atom | Token::Identifier,
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
                    | Rule::FunctionContract
                    | Rule::IfExpr
                    | Rule::IntExpr
                    | Rule::MatchExpr
                    | Rule::ParenExpr
                    | Rule::PipelineExpr
                    | Rule::SectionExpr
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
                Token::Identifier | Token::Placeholder | Token::Int | Token::Float | Token::Atom,
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
                    | Rule::TaggedPattern
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
    fn location(&self, node: NodeRef) -> Location {
        Location::from_usize(self.source_id, self.cst.span(node))
            .expect("CST span fits registered source")
    }
    fn identifier(&self, node: NodeRef) -> Identifier {
        located(self.text(node).into_owned(), self.location(node))
    }
    fn text(&self, node: NodeRef) -> std::borrow::Cow<'_, str> {
        self.source
            .slice(
                crate::source::TextRange::from_usize(self.cst.span(node))
                    .expect("CST span fits registered source"),
            )
            .expect("CST span is a valid source slice")
    }
    fn error(&self, node: NodeRef, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(message, self.location(node))
    }

    fn string_expression(&self, node: NodeRef) -> Result<Expr, Diagnostic> {
        let literal = self
            .rule_children(node)
            .find(|child| self.rule(*child) == Some(Rule::StringLiteral))
            .ok_or_else(|| self.error(node, "string expression has no literal"))?;
        let mut components = Vec::new();
        self.collect_string_components(literal, &mut components);
        let has_interpolation = components
            .iter()
            .any(|component| self.rule(*component) == Some(Rule::Interpolation));
        if !has_interpolation {
            return Ok(located(
                ExprKind::String(self.decode_string_components(&components)?),
                self.location(node),
            ));
        }

        let mut parts = Vec::new();
        for component in components {
            if self.rule(component) == Some(Rule::Interpolation) {
                let expression_node = self
                    .children(component)
                    .find(|child| self.is_expression(*child))
                    .ok_or_else(|| self.error(component, "interpolation has no expression"))?;
                let expression = self.expression(expression_node)?;
                let location = expression.location;
                parts.push(located(StringPartKind::Expression(expression), location));
            } else {
                parts.push(located(
                    StringPartKind::Text(self.decode_string_component(component)?),
                    self.location(component),
                ));
            }
        }
        Ok(located(
            ExprKind::InterpolatedString(parts),
            self.location(node),
        ))
    }

    fn plain_string(&self, node: NodeRef, context: &str) -> Result<String, Diagnostic> {
        let mut components = Vec::new();
        self.collect_string_components(node, &mut components);
        if let Some(interpolation) = components
            .iter()
            .find(|component| self.rule(**component) == Some(Rule::Interpolation))
        {
            return Err(self.error(
                *interpolation,
                format!("interpolation is not allowed in {context}"),
            ));
        }
        self.decode_string_components(&components)
    }

    fn collect_string_components(&self, node: NodeRef, output: &mut Vec<NodeRef>) {
        match self.cst.get(node) {
            Node::Token(Token::StringText | Token::EscapeSequence, _) => output.push(node),
            Node::Rule(Rule::Interpolation, _) => output.push(node),
            Node::Token(..) => {}
            Node::Rule(..) => {
                for child in self.children(node) {
                    self.collect_string_components(child, output);
                }
            }
        }
    }

    fn decode_string_components(&self, components: &[NodeRef]) -> Result<String, Diagnostic> {
        let mut output = String::new();
        for component in components {
            output.push_str(&self.decode_string_component(*component)?);
        }
        Ok(output)
    }

    fn decode_string_component(&self, node: NodeRef) -> Result<String, Diagnostic> {
        match self.cst.get(node) {
            Node::Token(Token::StringText, _) => Ok(self.text(node).into_owned()),
            Node::Token(Token::EscapeSequence, _) => {
                let escaped = self.text(node)[1..]
                    .chars()
                    .next()
                    .expect("escape token includes a character");
                let decoded = match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => {
                        return Err(self.error(node, format!("unsupported escape \\{other}")));
                    }
                };
                Ok(decoded.to_string())
            }
            _ => Err(self.error(node, "expected string text or escape")),
        }
    }

    fn decode_xl_string(&self, node: NodeRef) -> Result<String, Diagnostic> {
        let text = self.text(node);
        let quoted = text.strip_prefix('b').unwrap_or(&text);
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

fn push_unique_diagnostic(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    let location = diagnostic.labels.first().map(|label| label.location);
    if diagnostics.iter().any(|existing| {
        existing.message == diagnostic.message
            && existing.labels.first().map(|label| label.location) == location
    }) {
        return;
    }
    diagnostics.push(diagnostic);
}

fn elaborate_pipeline(location: Location, left: Expr, right: Expr) -> Expr {
    located(
        ExprKind::Call {
            callee: Box::new(right),
            arguments: vec![left],
        },
        location,
    )
}

const MAX_PLACEHOLDER_PARAMETERS: usize = u16::MAX as usize;

fn elaborate_call_section(
    callee: Expr,
    arguments: Vec<CallArgument>,
    section_node: NodeRef,
    location: Location,
) -> Result<Expr, (NodeRef, String)> {
    let first_bare = arguments.iter().find_map(|argument| match argument {
        CallArgument::Bare { node, .. } => Some(*node),
        _ => None,
    });
    let first_indexed = arguments.iter().find_map(|argument| match argument {
        CallArgument::Indexed { node, .. } => Some(*node),
        _ => None,
    });
    if first_bare.is_some()
        && let Some(indexed) = first_indexed
    {
        return Err((
            indexed,
            "cannot mix '_' and indexed placeholders in one call".into(),
        ));
    }

    if first_bare.is_none() && first_indexed.is_none() {
        return Err((
            section_node,
            "call section requires at least one placeholder".into(),
        ));
    }

    let mut parameter_locations = Vec::new();
    if first_bare.is_some() {
        parameter_locations.extend(arguments.iter().filter_map(|argument| match argument {
            CallArgument::Bare { location, .. } => Some(Some(*location)),
            _ => None,
        }));
    } else {
        let max = arguments
            .iter()
            .filter_map(|argument| match argument {
                CallArgument::Indexed { index, .. } => Some(*index),
                _ => None,
            })
            .max()
            .expect("indexed placeholder exists");
        if max >= MAX_PLACEHOLDER_PARAMETERS {
            let node = arguments
                .iter()
                .find_map(|argument| match argument {
                    CallArgument::Indexed { node, index, .. } if *index == max => Some(*node),
                    _ => None,
                })
                .expect("maximum placeholder has a node");
            return Err((
                node,
                format!(
                    "placeholder index exceeds the limit of {} parameters",
                    MAX_PLACEHOLDER_PARAMETERS
                ),
            ));
        }
        parameter_locations.resize(max + 1, None);
        for argument in &arguments {
            if let CallArgument::Indexed {
                index, location, ..
            } = argument
            {
                parameter_locations[*index].get_or_insert(*location);
            }
        }
        if let Some(missing) = parameter_locations.iter().position(Option::is_none) {
            return Err((
                first_indexed.expect("indexed placeholder exists"),
                format!("indexed placeholders are missing _{missing}"),
            ));
        }
    }

    let parameter_locations = parameter_locations
        .into_iter()
        .map(|location| location.expect("placeholder location was assigned"))
        .collect::<Vec<_>>();
    let parameters = parameter_locations
        .iter()
        .enumerate()
        .map(|(index, location)| located(placeholder_parameter(index), *location))
        .collect::<Vec<_>>();
    let mut next_bare = 0usize;
    let arguments = arguments
        .into_iter()
        .map(|argument| match argument {
            CallArgument::Expression(expression) => expression,
            CallArgument::Bare { location, .. } => {
                let index = next_bare;
                next_bare += 1;
                placeholder_variable(index, location)
            }
            CallArgument::Indexed {
                index, location, ..
            } => placeholder_variable(index, location),
        })
        .collect();
    let call = located(
        ExprKind::Call {
            callee: Box::new(callee),
            arguments,
        },
        location,
    );
    Ok(located(
        ExprKind::Closure {
            parameters,
            body: located(
                BlockKind {
                    bindings: Vec::new(),
                    result: Box::new(call),
                },
                location,
            ),
        },
        location,
    ))
}

fn placeholder_parameter(index: usize) -> String {
    format!("\0xl_placeholder_{index}")
}

fn placeholder_variable(index: usize, location: Location) -> Expr {
    located(
        ExprKind::Variable(located(placeholder_parameter(index), location)),
        location,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Located;

    #[test]
    fn lowers_directly_from_cst_with_spans_and_precedence() {
        let mut sources = SourceDatabase::default();
        let id = sources.add("test.xl", "let x = 1; x == 2");
        let parsed = parse_registered(&sources, id);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let program = parsed.program.unwrap();
        assert_eq!(program.location.range(), 0..17);
        assert_eq!(program.value.body.value.bindings[0].location.range(), 0..10);
        assert!(matches!(
            &program.value.body.value.result.value,
            ExprKind::Binary {
                operator: Located {
                    value: BinaryOperator::Equal,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn lowers_tagged_patterns() {
        let program = parse("test.xl", "match 'Some(1) { 'Some(value) => value }").unwrap();
        let ExprKind::Match { arms, .. } = &program.value.body.value.result.value else {
            panic!("expected match");
        };
        assert!(
            matches!(
                &arms[0].value.pattern.value,
                PatternKind::Tagged { payload, .. }
                    if matches!(payload.value, PatternKind::Binding(_))
            ),
            "{:?}",
            arms[0].value.pattern.value
        );
    }

    #[test]
    fn diagnoses_invalid_placeholder_sections_with_source_locations() {
        let cases = [
            (
                "mixed.xl",
                "let f = fn(a, b) { a }; f\\(_0, _)",
                "cannot mix",
            ),
            (
                "gap.xl",
                "let f = fn(a, b) { a }; f\\(_2, _0)",
                "missing _1",
            ),
            (
                "limit.xl",
                "let f = fn(a) { a }; f\\(_65535)",
                "exceeds the limit",
            ),
            (
                "overflow.xl",
                "let f = fn(a) { a }; f\\(_999999999999999999999999999999999)",
                "exceeds the supported range",
            ),
        ];
        for (name, source, expected) in cases {
            let mut sources = SourceDatabase::default();
            let id = sources.add(name, source);
            let parsed = parse_registered(&sources, id);
            assert!(parsed.program.is_none(), "{name} unexpectedly lowered");
            let rendered = parsed
                .diagnostics
                .iter()
                .map(|diagnostic| sources.render(diagnostic))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains(&format!("{name}:1:")), "{rendered}");
        }

        let mut sources = SourceDatabase::default();
        let id = sources.add("outside.xl", "let value = _; value");
        let parsed = parse_registered(&sources, id);
        assert!(parsed.program.is_none());
        assert!(!parsed.diagnostics.is_empty());

        for (name, source) in [
            ("ordinary-call.xl", "let f = fn(a, b) { a }; f(_, 1)"),
            ("reserved-name.xl", "let _0 = 1; _0"),
        ] {
            let mut sources = SourceDatabase::default();
            let id = sources.add(name, source);
            let parsed = parse_registered(&sources, id);
            assert!(parsed.program.is_none(), "{name} unexpectedly lowered");
            assert!(!parsed.diagnostics.is_empty(), "{name} has no diagnostic");
        }

        let mut sources = SourceDatabase::default();
        let id = sources.add("empty-section.xl", "let f = fn(a) { a }; f\\(1)");
        let parsed = parse_registered(&sources, id);
        assert!(parsed.program.is_none());
        assert!(
            parsed.diagnostics[0]
                .message
                .contains("requires at least one placeholder")
        );
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
    fn recovers_complete_bindings_around_a_damaged_sibling() {
        let mut sources = SourceDatabase::default();
        let id = sources.add(
            "recover.xl",
            "let before = 1; let broken = ; let after = 2; after",
        );
        let parsed = parse_registered(&sources, id);
        assert!(parsed.program.is_none());
        assert!(!parsed.diagnostics.is_empty());
        let names = parsed
            .recovered
            .bindings
            .iter()
            .map(|binding| binding.value.name.value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["before", "after"]);
        assert!(parsed.recovered.result.is_some());
    }

    #[test]
    fn comparisons_share_a_non_associative_precedence_level() {
        let chained = parse("test", "1 < 2 == 3").unwrap_err();
        assert!(chained.message.contains("do not associate"));
        assert!(parse("test", "(1 < 2) == 3").is_ok());
        assert!(parse("test", "1 < (2 == 3)").is_ok());
    }

    #[test]
    fn lowers_interpolation_with_located_text_and_expression_parts() {
        let program = parse("test", r#"let name = "Ada"; "hi, \{name}""#).unwrap();
        let ExprKind::InterpolatedString(parts) = &program.value.body.value.result.value else {
            panic!("expected interpolated string");
        };
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[0].value, StringPartKind::Text(text) if text == "hi, "));
        assert!(matches!(
            &parts[1].value,
            StringPartKind::Expression(expression)
                if matches!(&expression.value, ExprKind::Variable(name) if name.value == "name")
        ));
        assert_eq!(parts[1].location.range(), 25..29);
    }

    #[test]
    fn lowers_definition_bindings_and_function_contracts() {
        let program = parse("defs.xl", "decl f: Fn(Int) -> Int; def f = fn(x) { x }; f").unwrap();
        assert_eq!(program.value.body.value.bindings.len(), 2);
        assert_eq!(
            program.value.body.value.bindings[0].value.kind,
            BindingKind::Decl
        );
        assert_eq!(
            program.value.body.value.bindings[1].value.kind,
            BindingKind::Def
        );
        assert!(matches!(
            program.value.body.value.bindings[0]
                .value
                .annotation
                .as_ref()
                .map(|annotation| &annotation.value),
            Some(ExprKind::Call { .. })
        ));
    }

    #[test]
    fn lowers_generic_definition_declarations_with_located_parameters() {
        let program = parse(
            "identity.xl",
            "decl identity: for(A) Fn(A) -> A; def identity = fn(value) { value }; identity",
        )
        .unwrap();
        let declaration = &program.value.body.value.bindings[0];
        assert_eq!(declaration.value.kind, BindingKind::Decl);
        assert_eq!(declaration.value.type_parameters.len(), 1);
        assert_eq!(declaration.value.type_parameters[0].value, "A");
        assert_eq!(
            declaration.value.type_parameters[0].location.range(),
            19..20
        );
        assert!(declaration.value.annotation.is_some());
    }

    #[test]
    fn lowers_located_native_bindings_with_contracts() {
        let program = parse(
            "native.xl",
            "native map: for(A, B) Fn(Array(A), Fn(A) -> B) -> Array(B); map",
        )
        .unwrap();
        let binding = &program.value.body.value.bindings[0];
        assert_eq!(binding.value.kind, BindingKind::Native);
        assert_eq!(binding.value.name.value, "map");
        assert_eq!(
            binding
                .value
                .type_parameters
                .iter()
                .map(|parameter| parameter.value.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert_eq!(binding.value.type_parameters[0].location.range(), 16..17);
        assert!(binding.value.annotation.is_some());
        assert_eq!(binding.location.range(), 0..59);
    }

    #[test]
    fn retains_decorators_and_lowers_their_rhs_calls() {
        let program = parse(
            "decorators.xl",
            "@outer @factory(1) type T = Int; { @field value: 2 }",
        )
        .unwrap();
        let binding = &program.value.body.value.bindings[0];
        assert_eq!(binding.value.decorators.len(), 2);
        assert!(!binding.value.decorators[0].value.configured);
        assert!(binding.value.decorators[1].value.configured);
        assert!(matches!(binding.value.value.value, ExprKind::Call { .. }));
        let ExprKind::Dict(fields) = &program.value.body.value.result.value else {
            panic!("expected Dict")
        };
        assert_eq!(fields[0].value.decorators.len(), 1);
        assert!(matches!(fields[0].value.value.value, ExprKind::Call { .. }));
    }

    #[test]
    fn rejects_interpolation_in_plain_string_contexts() {
        let error = parse("test", r#"match "x" { "\{1}" => 1 }"#).unwrap_err();
        assert!(error.message.contains("not allowed in string pattern"));

        let key_error = parse("test", r#"{"\{"x"}": 1}"#).unwrap_err();
        assert!(key_error.message.contains("not allowed in Dict field name"));
    }

    #[test]
    fn reports_invalid_and_unterminated_string_parts() {
        let invalid = parse("test", r#""bad\q""#).unwrap_err();
        assert!(invalid.message.contains("unsupported string escape"));
        assert_eq!(invalid.location.offset, 4);

        let unterminated = parse("test", r#""unfinished"#).unwrap_err();
        assert!(unterminated.message.contains("expected"));
    }
}
