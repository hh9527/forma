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
        Diagnostic::error()
            .with_message("invalid token")
            .with_label(Label::primary((), span))
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum Token {
    EOF,
    True,
    False,
    Null,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    DoubleQuote,
    StringText,
    EscapeSequence,
    Number,
    Whitespace,
    Error,
}

#[derive(Logos, Debug, PartialEq, Copy, Clone)]
#[logos(error = LexerError)]
enum NormalToken {
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,
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
    #[token("\"")]
    DoubleQuote,
    #[regex(r"-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")]
    Number,
    #[regex(r"[ \t\r\n]+")]
    Whitespace,
}

#[derive(Logos, Debug, PartialEq, Copy, Clone)]
#[logos(error = LexerError)]
enum StringToken {
    #[token("\"")]
    DoubleQuote,
    #[regex(r#"\\(u[0-9A-Fa-f]{4}|.)"#)]
    EscapeSequence,
    #[regex(r#"[^\"\\\x00-\x1f]+"#)]
    StringText,
}

pub fn tokenize(source: &str, diags: &mut Vec<Diagnostic>) -> (Vec<Token>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut in_string = false;
    let mut offset = 0;
    while offset < source.len() {
        let (token, length, error) = if in_string {
            lex_string(&source[offset..])
        } else {
            lex_normal(&source[offset..])
        };
        let span = offset..offset + length;
        if error {
            diags.push(LexerError::Invalid.into_diagnostic(span.clone()));
        }
        if token == Token::DoubleQuote {
            in_string = !in_string;
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
        Ok(StringToken::EscapeSequence) => (Token::EscapeSequence, length, false),
        Ok(StringToken::StringText) => (Token::StringText, length, false),
        Err(_) => (Token::Error, length, true),
    }
}

fn normal_token(token: NormalToken) -> Token {
    match token {
        NormalToken::True => Token::True,
        NormalToken::False => Token::False,
        NormalToken::Null => Token::Null,
        NormalToken::LBrace => Token::LBrace,
        NormalToken::RBrace => Token::RBrace,
        NormalToken::LBracket => Token::LBracket,
        NormalToken::RBracket => Token::RBracket,
        NormalToken::Comma => Token::Comma,
        NormalToken::Colon => Token::Colon,
        NormalToken::DoubleQuote => Token::DoubleQuote,
        NormalToken::Number => Token::Number,
        NormalToken::Whitespace => Token::Whitespace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_text_and_each_escape_as_source_ranges() {
        let mut diagnostics = Vec::new();
        let (tokens, spans) = tokenize(r#""a\n\u0041b""#, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens,
            vec![
                Token::DoubleQuote,
                Token::StringText,
                Token::EscapeSequence,
                Token::EscapeSequence,
                Token::StringText,
                Token::DoubleQuote,
            ]
        );
        assert_eq!(spans[2], 2..4);
        assert_eq!(spans[3], 4..10);
    }
}
