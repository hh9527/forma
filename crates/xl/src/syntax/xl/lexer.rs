use super::parser::{Diagnostic, Span};
use codespan_reporting::diagnostic::Label;
use logos::{Lexer, Logos};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum LexerError {
    #[default]
    Invalid,
}

impl LexerError {
    pub fn into_diagnostic(self, span: Span) -> Diagnostic {
        match self {
            Self::Invalid => Diagnostic::error()
                .with_message("invalid token")
                .with_label(Label::primary((), span)),
        }
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Token {
    EOF,
    Let,
    Type,
    Fn,
    If,
    Else,
    Match,
    Import,
    From,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Less,
    EqualEqual,
    Equal,
    FatArrow,
    Pipe,
    Int,
    Float,
    DoubleQuote,
    StringText,
    EscapeSequence,
    InterpolationStart,
    Bytes,
    Atom,
    Identifier,
    Whitespace,
    Comment,
    Error,
}

#[derive(Logos, Debug, PartialEq, Copy, Clone)]
#[logos(error = LexerError, extras = LexerState)]
enum NormalToken {
    #[token("let")]
    Let,
    #[token("type")]
    Type,
    #[token("fn")]
    Fn,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("match")]
    Match,
    #[token("import")]
    Import,
    #[token("from")]
    From,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("<")]
    Less,
    #[token("==")]
    EqualEqual,
    #[token("=")]
    Equal,
    #[token("=>")]
    FatArrow,
    #[token("|>")]
    Pipe,
    #[regex(r"[0-9]+")]
    Int,
    #[regex(r"[0-9]+\.[0-9]+")]
    Float,
    #[token("\"")]
    DoubleQuote,
    #[regex(r#"b\"([^\"\\]|\\.)*\""#)]
    Bytes,
    #[regex(r"'[A-Za-z_][A-Za-z0-9_]*")]
    Atom,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Identifier,
    #[regex(r"[ \t\r\n]+")]
    Whitespace,
    #[regex(r"//[^\r\n]*", allow_greedy = true)]
    Comment,
}

#[derive(Logos, Debug, PartialEq, Copy, Clone)]
#[logos(error = LexerError, extras = LexerState)]
enum StringToken {
    #[token("\"")]
    DoubleQuote,
    #[token("\\{", priority = 5)]
    InterpolationStart,
    #[regex(r#"\\."#)]
    EscapeSequence,
    #[regex(r#"[^\"\\]+"#)]
    StringText,
}

#[derive(Clone, Copy, Debug)]
enum Context {
    Root,
    Interpolation { brace_depth: usize },
    String,
}

#[derive(Debug)]
struct LexerState {
    contexts: Vec<Context>,
}

impl Default for LexerState {
    fn default() -> Self {
        Self {
            contexts: vec![Context::Root],
        }
    }
}

enum ActiveLexer<'source> {
    Normal(Lexer<'source, NormalToken>),
    String(Lexer<'source, StringToken>),
}

pub fn tokenize(source: &str, diags: &mut Vec<Diagnostic>) -> (Vec<Token>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut active = ActiveLexer::Normal(NormalToken::lexer(source));

    loop {
        let (token, span, error, next) = match active {
            ActiveLexer::Normal(mut lexer) => {
                let Some(result) = lexer.next() else {
                    break;
                };
                let span = lexer.span();
                let (token, error) = match result {
                    Ok(token) => (normal_token(token), false),
                    Err(_) => (Token::Error, true),
                };
                let context = *lexer
                    .extras
                    .contexts
                    .last()
                    .expect("lexer has a root context");
                let next = match (context, token) {
                    (Context::Root | Context::Interpolation { .. }, Token::DoubleQuote) => {
                        lexer.extras.contexts.push(Context::String);
                        ActiveLexer::String(lexer.morph())
                    }
                    (Context::Interpolation { brace_depth }, Token::LBrace) => {
                        *lexer
                            .extras
                            .contexts
                            .last_mut()
                            .expect("interpolation context") = Context::Interpolation {
                            brace_depth: brace_depth + 1,
                        };
                        ActiveLexer::Normal(lexer)
                    }
                    (Context::Interpolation { brace_depth: 0 }, Token::RBrace) => {
                        lexer.extras.contexts.pop();
                        ActiveLexer::String(lexer.morph())
                    }
                    (Context::Interpolation { brace_depth }, Token::RBrace) => {
                        *lexer
                            .extras
                            .contexts
                            .last_mut()
                            .expect("interpolation context") = Context::Interpolation {
                            brace_depth: brace_depth - 1,
                        };
                        ActiveLexer::Normal(lexer)
                    }
                    _ => ActiveLexer::Normal(lexer),
                };
                (token, span, error, next)
            }
            ActiveLexer::String(mut lexer) => {
                let Some(result) = lexer.next() else {
                    break;
                };
                let span = lexer.span();
                let (token, error) = match result {
                    Ok(StringToken::DoubleQuote) => (Token::DoubleQuote, false),
                    Ok(StringToken::InterpolationStart) => (Token::InterpolationStart, false),
                    Ok(StringToken::EscapeSequence) => (Token::EscapeSequence, false),
                    Ok(StringToken::StringText) => (Token::StringText, false),
                    Err(_) => (Token::Error, true),
                };
                let next = match token {
                    Token::DoubleQuote => {
                        lexer.extras.contexts.pop();
                        ActiveLexer::Normal(lexer.morph())
                    }
                    Token::InterpolationStart => {
                        lexer
                            .extras
                            .contexts
                            .push(Context::Interpolation { brace_depth: 0 });
                        ActiveLexer::Normal(lexer.morph())
                    }
                    _ => ActiveLexer::String(lexer),
                };
                (token, span, error, next)
            }
        };
        if error {
            diags.push(LexerError::Invalid.into_diagnostic(span.clone()));
        }
        tokens.push(token);
        spans.push(span);
        active = next;
    }
    (tokens, spans)
}

fn normal_token(token: NormalToken) -> Token {
    match token {
        NormalToken::Let => Token::Let,
        NormalToken::Type => Token::Type,
        NormalToken::Fn => Token::Fn,
        NormalToken::If => Token::If,
        NormalToken::Else => Token::Else,
        NormalToken::Match => Token::Match,
        NormalToken::Import => Token::Import,
        NormalToken::From => Token::From,
        NormalToken::LParen => Token::LParen,
        NormalToken::RParen => Token::RParen,
        NormalToken::LBrace => Token::LBrace,
        NormalToken::RBrace => Token::RBrace,
        NormalToken::LBracket => Token::LBracket,
        NormalToken::RBracket => Token::RBracket,
        NormalToken::Comma => Token::Comma,
        NormalToken::Colon => Token::Colon,
        NormalToken::Semicolon => Token::Semicolon,
        NormalToken::Dot => Token::Dot,
        NormalToken::Plus => Token::Plus,
        NormalToken::Minus => Token::Minus,
        NormalToken::Star => Token::Star,
        NormalToken::Slash => Token::Slash,
        NormalToken::Less => Token::Less,
        NormalToken::EqualEqual => Token::EqualEqual,
        NormalToken::Equal => Token::Equal,
        NormalToken::FatArrow => Token::FatArrow,
        NormalToken::Pipe => Token::Pipe,
        NormalToken::Int => Token::Int,
        NormalToken::Float => Token::Float,
        NormalToken::DoubleQuote => Token::DoubleQuote,
        NormalToken::Bytes => Token::Bytes,
        NormalToken::Atom => Token::Atom,
        NormalToken::Identifier => Token::Identifier,
        NormalToken::Whitespace => Token::Whitespace,
        NormalToken::Comment => Token::Comment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_structured_string_slices_without_payloads() {
        let mut diagnostics = Vec::new();
        let (tokens, spans) = tokenize(r#""hi, \{name}\n""#, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens,
            vec![
                Token::DoubleQuote,
                Token::StringText,
                Token::InterpolationStart,
                Token::Identifier,
                Token::RBrace,
                Token::EscapeSequence,
                Token::DoubleQuote,
            ]
        );
        assert_eq!(spans[1], 1..5);
        assert_eq!(spans[2], 5..7);
        assert_eq!(spans[5], 12..14);

        let (tokens, spans) = tokenize(r#"let x = "text""#, &mut diagnostics);
        let quote = tokens
            .iter()
            .position(|token| *token == Token::DoubleQuote)
            .unwrap();
        assert_eq!(spans[quote], 8..9);
        assert_eq!(spans[quote + 1], 9..13);
        assert_eq!(spans[quote + 2], 13..14);
    }
}
