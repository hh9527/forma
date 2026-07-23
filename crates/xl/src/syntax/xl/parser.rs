#![allow(unused_variables)]

use super::lexer::{Token, tokenize};

// TODO: change if codespan_reporting is not used
use codespan_reporting::diagnostic::Label;
pub type Diagnostic = codespan_reporting::diagnostic::Diagnostic<()>;

#[derive(Clone, Copy)]
enum StringLookahead {
    String,
    Interpolation { brace_depth: usize },
}

include!(concat!(env!("OUT_DIR"), "/xl/generated.rs"));

impl<'a> ParserCallbacks<'a> for Parser<'a> {
    type Diagnostic = Diagnostic;
    type Context = (); // TODO: add context information to the parser if required

    fn create_tokens(
        _context: &mut Self::Context,
        source: &'a str,
        diags: &mut Vec<Self::Diagnostic>,
    ) -> (Vec<Token>, Vec<Span>) {
        tokenize(source, diags)
    }
    fn create_diagnostic(&self, span: Span, message: String) -> Self::Diagnostic {
        Self::Diagnostic::error()
            .with_message(message)
            .with_label(Label::primary((), span))
    }
    fn predicate_body_1(&self) -> bool {
        matches!(
            self.current,
            Token::Let | Token::Decl | Token::Def | Token::Native | Token::Type | Token::Import
        ) || self.current == Token::Fn && self.peek(1) == Token::Identifier
    }
    fn predicate_primary_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_primary_2(&self) -> bool {
        self.peek(1) != Token::RBracket
    }
    fn predicate_primary_3(&self) -> bool {
        let mut depth = 0usize;
        let mut lookahead = 1usize;
        loop {
            match self.peek(lookahead) {
                Token::EOF => return false,
                Token::LParen => depth += 1,
                Token::RParen if depth == 1 => return self.peek(lookahead + 1) == Token::Arrow,
                Token::RParen => depth = depth.saturating_sub(1),
                _ => {}
            }
            lookahead += 1;
        }
    }
    fn predicate_braced_1(&self) -> bool {
        if self.peek(1) == Token::RBrace
            || self.peek(1) == Token::Identifier && self.peek(2) == Token::Colon
        {
            return true;
        }
        if self.peek(1) != Token::DoubleQuote {
            return false;
        }
        let mut lookahead = 2;
        let mut contexts = vec![StringLookahead::String];
        loop {
            let token = self.peek(lookahead);
            let context = *contexts.last().expect("lookahead has a string context");
            match (context, token) {
                (_, Token::EOF) => return false,
                (StringLookahead::String, Token::DoubleQuote) => {
                    contexts.pop();
                    if contexts.is_empty() {
                        return self.peek(lookahead + 1) == Token::Colon;
                    }
                }
                (StringLookahead::String, Token::InterpolationStart) => {
                    contexts.push(StringLookahead::Interpolation { brace_depth: 0 });
                }
                (StringLookahead::Interpolation { .. }, Token::DoubleQuote) => {
                    contexts.push(StringLookahead::String);
                }
                (StringLookahead::Interpolation { brace_depth }, Token::LBrace) => {
                    *contexts.last_mut().expect("interpolation lookahead") =
                        StringLookahead::Interpolation {
                            brace_depth: brace_depth + 1,
                        };
                }
                (StringLookahead::Interpolation { brace_depth: 0 }, Token::RBrace) => {
                    contexts.pop();
                }
                (StringLookahead::Interpolation { brace_depth }, Token::RBrace) => {
                    *contexts.last_mut().expect("interpolation lookahead") =
                        StringLookahead::Interpolation {
                            brace_depth: brace_depth - 1,
                        };
                }
                _ => {}
            }
            lookahead += 1;
        }
    }
    fn predicate_braced_2(&self) -> bool {
        self.peek(1) != Token::RBrace
    }
    fn predicate_parameters_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_annotated_parameters_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_arguments_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_section_arguments_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_contract_1(&self) -> bool {
        self.current == Token::LParen
    }
    fn predicate_contract_2(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_function_contract_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_match_expr_1(&self) -> bool {
        self.peek(1) != Token::RBrace
    }
    fn predicate_pattern_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
}
