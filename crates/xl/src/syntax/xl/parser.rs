#![allow(unused_variables)]

use super::lexer::{Token, tokenize};

// TODO: change if codespan_reporting is not used
use codespan_reporting::diagnostic::Label;
pub type Diagnostic = codespan_reporting::diagnostic::Diagnostic<()>;

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
        matches!(self.current, Token::Let | Token::Type | Token::Import)
            || self.current == Token::Fn && self.peek(1) == Token::Identifier
    }
    fn predicate_primary_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_primary_2(&self) -> bool {
        self.peek(1) != Token::RBracket
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
        while !matches!(self.peek(lookahead), Token::DoubleQuote | Token::EOF) {
            lookahead += 1;
        }
        self.peek(lookahead) == Token::DoubleQuote && self.peek(lookahead + 1) == Token::Colon
    }
    fn predicate_braced_2(&self) -> bool {
        self.peek(1) != Token::RBrace
    }
    fn predicate_parameters_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_arguments_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
    fn predicate_match_expr_1(&self) -> bool {
        self.peek(1) != Token::RBrace
    }
    fn predicate_pattern_1(&self) -> bool {
        self.peek(1) != Token::RParen
    }
}
