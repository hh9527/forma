use super::parser::{Diagnostic, Span};
use codespan_reporting::diagnostic::Label;
use logos::Logos;

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
#[logos(error = LexerError)]
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
#[logos(error = LexerError)]
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

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Interpolation { brace_depth: usize },
    String,
}

pub fn tokenize(source: &str, diags: &mut Vec<Diagnostic>) -> (Vec<Token>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut modes = vec![Mode::Normal];
    let mut offset = 0;

    while offset < source.len() {
        let mode = *modes.last().expect("lexer has a root mode");
        let (token, length, error) = match mode {
            Mode::String => lex_string(&source[offset..]),
            Mode::Normal | Mode::Interpolation { .. } => lex_normal(&source[offset..]),
        };
        let span = offset..offset + length;
        if error {
            diags.push(LexerError::Invalid.into_diagnostic(span.clone()));
        }

        match (mode, token) {
            (Mode::Normal | Mode::Interpolation { .. }, Token::DoubleQuote) => {
                modes.push(Mode::String);
            }
            (Mode::String, Token::DoubleQuote) => {
                modes.pop();
            }
            (Mode::String, Token::InterpolationStart) => {
                modes.push(Mode::Interpolation { brace_depth: 0 });
            }
            (Mode::Interpolation { brace_depth }, Token::LBrace) => {
                *modes.last_mut().expect("interpolation mode") = Mode::Interpolation {
                    brace_depth: brace_depth + 1,
                };
            }
            (Mode::Interpolation { brace_depth: 0 }, Token::RBrace) => {
                modes.pop();
            }
            (Mode::Interpolation { brace_depth }, Token::RBrace) => {
                *modes.last_mut().expect("interpolation mode") = Mode::Interpolation {
                    brace_depth: brace_depth - 1,
                };
            }
            _ => {}
        }

        tokens.push(token);
        spans.push(span);
        offset += length;
    }
    (tokens, spans)
}

fn lex_normal(source: &str) -> (Token, usize, bool) {
    let mut lexer = NormalToken::lexer(source);
    let result = lexer.next().expect("non-empty lexer input");
    let length = lexer.span().end;
    match result {
        Ok(token) => (normal_token(token), length, false),
        Err(_) => (Token::Error, length, true),
    }
}

fn lex_string(source: &str) -> (Token, usize, bool) {
    let mut lexer = StringToken::lexer(source);
    let result = lexer.next().expect("non-empty lexer input");
    let length = lexer.span().end;
    match result {
        Ok(StringToken::DoubleQuote) => (Token::DoubleQuote, length, false),
        Ok(StringToken::InterpolationStart) => (Token::InterpolationStart, length, false),
        Ok(StringToken::EscapeSequence) => (Token::EscapeSequence, length, false),
        Ok(StringToken::StringText) => (Token::StringText, length, false),
        Err(_) => (Token::Error, length, true),
    }
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
    }
}
