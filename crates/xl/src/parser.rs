use crate::ast::{
    BinaryOperator, Binding, BindingKind, Block, Expr, MatchArm, Pattern, Program, UnaryOperator,
};
use crate::lexer::{FrontendError, Token, TokenKind, lex};

pub fn parse(source_name: &str, source: &str) -> Result<Program, FrontendError> {
    Parser::new(source_name, lex(source_name, source)?).parse_program()
}

struct Parser<'a> {
    source_name: &'a str,
    tokens: Vec<Token>,
    current: usize,
}

impl<'a> Parser<'a> {
    fn new(source_name: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source_name,
            tokens,
            current: 0,
        }
    }

    fn parse_program(mut self) -> Result<Program, FrontendError> {
        let body = self.parse_body(TokenKind::Eof)?;
        self.expect(TokenKind::Eof, "expected end of source")?;
        Ok(Program { body })
    }

    fn parse_body(&mut self, end: TokenKind) -> Result<Block, FrontendError> {
        let mut bindings = Vec::new();
        loop {
            if self.at(&TokenKind::Let) {
                bindings.push(self.parse_let()?);
            } else if self.at(&TokenKind::Type) {
                bindings.push(self.parse_type()?);
            } else if self.at(&TokenKind::Import) {
                bindings.push(self.parse_import()?);
            } else if self.at(&TokenKind::Fn)
                && matches!(self.peek_kind(1), Some(TokenKind::Identifier(_)))
            {
                bindings.push(self.parse_named_function()?);
            } else {
                break;
            }
        }
        if self.at(&end) {
            return Err(self.error("a block requires a result expression"));
        }
        let result = self.parse_expression(0)?;
        if self.at(&TokenKind::Semicolon) {
            return Err(self.error("the result expression must not end with ';'"));
        }
        Ok(Block {
            bindings,
            result: Box::new(result),
        })
    }

    fn parse_let(&mut self) -> Result<Binding, FrontendError> {
        self.expect(TokenKind::Let, "expected 'let'")?;
        let name = self.identifier("expected a binding name")?;
        let annotation = if self.consume(&TokenKind::Colon) {
            Some(self.parse_expression(0)?)
        } else {
            None
        };
        self.expect(TokenKind::Equal, "expected '=' after binding name")?;
        let value = self.parse_expression(0)?;
        self.expect(TokenKind::Semicolon, "expected ';' after binding")?;
        Ok(Binding {
            kind: BindingKind::Let,
            name,
            annotation,
            value,
        })
    }

    fn parse_type(&mut self) -> Result<Binding, FrontendError> {
        self.expect(TokenKind::Type, "expected 'type'")?;
        let name = self.identifier("expected a type name")?;
        self.expect(TokenKind::Equal, "expected '=' after type name")?;
        let value = self.parse_expression(0)?;
        self.expect(TokenKind::Semicolon, "expected ';' after type declaration")?;
        Ok(Binding {
            kind: BindingKind::Type,
            name,
            annotation: None,
            value,
        })
    }

    fn parse_import(&mut self) -> Result<Binding, FrontendError> {
        self.expect(TokenKind::Import, "expected 'import'")?;
        let name = self.identifier("expected an import binding name")?;
        self.expect(TokenKind::From, "expected 'from' after import name")?;
        let token = self.advance().clone();
        let TokenKind::String(path) = token.kind else {
            return Err(FrontendError::new(
                self.source_name,
                token.location,
                "import path must be a string literal",
            ));
        };
        self.expect(TokenKind::Semicolon, "expected ';' after import")?;
        Ok(Binding {
            kind: BindingKind::Import,
            name,
            annotation: None,
            value: Expr::String(path),
        })
    }

    fn parse_named_function(&mut self) -> Result<Binding, FrontendError> {
        self.expect(TokenKind::Fn, "expected 'fn'")?;
        let name = self.identifier("expected a function name")?;
        let parameters = self.parse_parameters()?;
        let body = self.parse_required_block()?;
        Ok(Binding {
            kind: BindingKind::Let,
            name,
            annotation: None,
            value: Expr::Closure { parameters, body },
        })
    }

    fn parse_expression(&mut self, minimum_precedence: u8) -> Result<Expr, FrontendError> {
        let mut left = self.parse_prefix()?;
        loop {
            left = if self.at(&TokenKind::LeftParen) {
                self.parse_call(left)?
            } else if self.at(&TokenKind::Dot) {
                self.advance();
                let field = self.identifier("expected field name after '.'")?;
                Expr::Field {
                    receiver: Box::new(left),
                    field,
                }
            } else {
                let Some((precedence, operator)) = self.binary_operator() else {
                    break;
                };
                if precedence < minimum_precedence {
                    break;
                }
                let is_pipeline = self.at(&TokenKind::Pipe);
                self.advance();
                let right = self.parse_expression(precedence + 1)?;
                if is_pipeline {
                    elaborate_pipeline(left, right)
                } else {
                    Expr::Binary {
                        operator: operator.expect("non-pipeline operator"),
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }
            };
        }
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, FrontendError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Int(value) => Ok(Expr::Int(value)),
            TokenKind::Float(value) => Ok(Expr::Float(value)),
            TokenKind::String(value) => Ok(Expr::String(value)),
            TokenKind::Bytes(value) => Ok(Expr::Bytes(value)),
            TokenKind::Atom(value) => Ok(Expr::Atom(value)),
            TokenKind::Identifier(value) => Ok(Expr::Variable(value)),
            TokenKind::Minus => Ok(Expr::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(self.parse_expression(6)?),
            }),
            TokenKind::LeftParen => self.parse_tuple_or_group(),
            TokenKind::LeftBracket => self.parse_array(),
            TokenKind::LeftBrace => self.parse_braced(),
            TokenKind::Fn => self.parse_closure(),
            TokenKind::If => self.parse_if(),
            TokenKind::Match => self.parse_match(),
            _ => Err(FrontendError::new(
                self.source_name,
                token.location,
                "expected an expression",
            )),
        }
    }

    fn parse_tuple_or_group(&mut self) -> Result<Expr, FrontendError> {
        if self.consume(&TokenKind::RightParen) {
            return Ok(Expr::Tuple(Vec::new()));
        }
        let first = self.parse_expression(0)?;
        if !self.consume(&TokenKind::Comma) {
            self.expect(TokenKind::RightParen, "expected ')'")?;
            return Ok(first);
        }
        let mut items = vec![first];
        while !self.consume(&TokenKind::RightParen) {
            items.push(self.parse_expression(0)?);
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightParen, "expected ')' after tuple")?;
                break;
            }
        }
        Ok(Expr::Tuple(items))
    }

    fn parse_array(&mut self) -> Result<Expr, FrontendError> {
        let mut items = Vec::new();
        while !self.consume(&TokenKind::RightBracket) {
            items.push(self.parse_expression(0)?);
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightBracket, "expected ']' after Array")?;
                break;
            }
        }
        Ok(Expr::Array(items))
    }

    fn parse_braced(&mut self) -> Result<Expr, FrontendError> {
        if self.consume(&TokenKind::RightBrace) {
            return Ok(Expr::Dict(Vec::new()));
        }
        let is_dict = matches!(
            self.current_kind(),
            TokenKind::Identifier(_) | TokenKind::String(_)
        ) && matches!(self.peek_kind(1), Some(TokenKind::Colon));
        if !is_dict {
            let block = self.parse_body(TokenKind::RightBrace)?;
            self.expect(TokenKind::RightBrace, "expected '}' after block")?;
            return Ok(Expr::Block(block));
        }

        let mut fields = Vec::new();
        loop {
            let token = self.advance().clone();
            let field = match token.kind {
                TokenKind::Identifier(field) | TokenKind::String(field) => field,
                _ => {
                    return Err(FrontendError::new(
                        self.source_name,
                        token.location,
                        "expected a Dict field",
                    ));
                }
            };
            self.expect(TokenKind::Colon, "expected ':' after Dict field")?;
            fields.push((field, self.parse_expression(0)?));
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightBrace, "expected '}' after Dict")?;
                break;
            }
            if self.consume(&TokenKind::RightBrace) {
                break;
            }
        }
        Ok(Expr::Dict(fields))
    }

    fn parse_closure(&mut self) -> Result<Expr, FrontendError> {
        let parameters = self.parse_parameters()?;
        let body = self.parse_required_block()?;
        Ok(Expr::Closure { parameters, body })
    }

    fn parse_parameters(&mut self) -> Result<Vec<String>, FrontendError> {
        self.expect(TokenKind::LeftParen, "expected '(' after 'fn'")?;
        let mut parameters = Vec::new();
        while !self.consume(&TokenKind::RightParen) {
            parameters.push(self.identifier("expected parameter name")?);
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightParen, "expected ')' after parameters")?;
                break;
            }
        }
        Ok(parameters)
    }

    fn parse_required_block(&mut self) -> Result<Block, FrontendError> {
        self.expect(TokenKind::LeftBrace, "expected '{'")?;
        let block = self.parse_body(TokenKind::RightBrace)?;
        self.expect(TokenKind::RightBrace, "expected '}' after block")?;
        Ok(block)
    }

    fn parse_if(&mut self) -> Result<Expr, FrontendError> {
        let condition = self.parse_expression(0)?;
        let then_branch = self.parse_required_block()?;
        self.expect(TokenKind::Else, "expected 'else' after if branch")?;
        let else_branch = self.parse_required_block()?;
        Ok(Expr::If {
            condition: Box::new(condition),
            then_branch,
            else_branch,
        })
    }

    fn parse_match(&mut self) -> Result<Expr, FrontendError> {
        let value = self.parse_expression(0)?;
        self.expect(TokenKind::LeftBrace, "expected '{' after match value")?;
        let mut arms = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            let pattern = self.parse_pattern()?;
            self.expect(TokenKind::FatArrow, "expected '=>' after pattern")?;
            let arm_value = self.parse_expression(0)?;
            arms.push(MatchArm {
                pattern,
                value: arm_value,
            });
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightBrace, "expected '}' after match arms")?;
                break;
            }
        }
        if arms.is_empty() {
            return Err(self.error("match requires at least one arm"));
        }
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, FrontendError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) if name == "_" => Ok(Pattern::Wildcard),
            TokenKind::Identifier(name) => Ok(Pattern::Binding(name)),
            TokenKind::Int(value) => Ok(Pattern::Int(value)),
            TokenKind::Float(value) => Ok(Pattern::Float(value)),
            TokenKind::String(value) => Ok(Pattern::String(value)),
            TokenKind::Atom(value) => Ok(Pattern::Atom(value)),
            TokenKind::LeftParen => {
                let mut items = Vec::new();
                if self.consume(&TokenKind::RightParen) {
                    return Ok(Pattern::Tuple(items));
                }
                loop {
                    items.push(self.parse_pattern()?);
                    if !self.consume(&TokenKind::Comma) {
                        self.expect(TokenKind::RightParen, "expected ')' after tuple pattern")?;
                        break;
                    }
                    if self.consume(&TokenKind::RightParen) {
                        break;
                    }
                }
                Ok(Pattern::Tuple(items))
            }
            _ => Err(FrontendError::new(
                self.source_name,
                token.location,
                "unsupported pattern",
            )),
        }
    }

    fn parse_call(&mut self, callee: Expr) -> Result<Expr, FrontendError> {
        self.expect(TokenKind::LeftParen, "expected '('")?;
        let mut arguments = Vec::new();
        while !self.consume(&TokenKind::RightParen) {
            arguments.push(self.parse_expression(0)?);
            if !self.consume(&TokenKind::Comma) {
                self.expect(TokenKind::RightParen, "expected ')' after arguments")?;
                break;
            }
        }
        Ok(Expr::Call {
            callee: Box::new(callee),
            arguments,
        })
    }

    fn binary_operator(&self) -> Option<(u8, Option<BinaryOperator>)> {
        match self.current_kind() {
            TokenKind::Pipe => Some((1, None)),
            TokenKind::EqualEqual => Some((2, Some(BinaryOperator::Equal))),
            TokenKind::Less => Some((3, Some(BinaryOperator::LessThan))),
            TokenKind::Plus => Some((4, Some(BinaryOperator::Add))),
            TokenKind::Minus => Some((4, Some(BinaryOperator::Subtract))),
            TokenKind::Star => Some((5, Some(BinaryOperator::Multiply))),
            TokenKind::Slash => Some((5, Some(BinaryOperator::Divide))),
            _ => None,
        }
    }

    fn identifier(&mut self, message: &str) -> Result<String, FrontendError> {
        let token = self.advance().clone();
        if let TokenKind::Identifier(name) = token.kind {
            Ok(name)
        } else {
            Err(FrontendError::new(
                self.source_name,
                token.location,
                message,
            ))
        }
    }

    fn expect(&mut self, expected: TokenKind, message: &str) -> Result<(), FrontendError> {
        if self.consume(&expected) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn consume(&mut self, expected: &TokenKind) -> bool {
        if self.at(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at(&self, expected: &TokenKind) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(expected)
    }

    fn current_kind(&self) -> &TokenKind {
        &self.tokens[self.current].kind
    }

    fn peek_kind(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens
            .get(self.current + offset)
            .map(|token| &token.kind)
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.current];
        if !matches!(token.kind, TokenKind::Eof) {
            self.current += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> FrontendError {
        FrontendError::new(
            self.source_name,
            self.tokens[self.current].location,
            message,
        )
    }
}

fn elaborate_pipeline(left: Expr, right: Expr) -> Expr {
    match right {
        Expr::Call {
            callee,
            mut arguments,
        } => {
            arguments.insert(0, left);
            Expr::Call { callee, arguments }
        }
        callee => Expr::Call {
            callee: Box::new(callee),
            arguments: vec![left],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bindings_closures_and_pipeline() {
        let program = parse("test", "let add = fn(a, b) { a + b }; 40 |> add(2)").unwrap();
        assert_eq!(program.body.bindings.len(), 1);
        let Expr::Call { arguments, .. } = *program.body.result else {
            panic!("expected pipeline to elaborate to a call");
        };
        assert_eq!(arguments.len(), 2);
        assert!(matches!(arguments[0], Expr::Int(40)));
    }

    #[test]
    fn parses_if_match_and_dict() {
        let program = parse(
            "test",
            "match ('Ok, {b: 2, a: 1}) { ('Ok, value) => value.a, _ => 0 }",
        )
        .unwrap();
        let Expr::Match { arms, .. } = *program.body.result else {
            panic!("expected match");
        };
        assert_eq!(arms.len(), 2);
    }

    #[test]
    fn rejects_a_result_semicolon() {
        let error = parse("test", "let x = 1; x;").unwrap_err();
        assert!(error.message.contains("must not end"));
    }
}
