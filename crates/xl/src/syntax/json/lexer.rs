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
#[logos(error = LexerError, extras = LexerState)]
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
#[logos(error = LexerError, extras = LexerState)]
enum StringToken {
    #[token("\"")]
    DoubleQuote,
    #[regex(r#"\\(u[0-9A-Fa-f]{4}|.)"#)]
    EscapeSequence,
    #[regex(r#"[^\"\\\x00-\x1f]+"#)]
    StringText,
}

#[derive(Clone, Copy, Debug, Default)]
enum Mode {
    #[default]
    Normal,
    String,
}

#[derive(Debug, Default)]
struct LexerState {
    mode: Mode,
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
                    Ok(token) => (token.into(), false),
                    Err(_) => (Token::Error, true),
                };
                let next = if token == Token::DoubleQuote {
                    lexer.extras.mode = Mode::String;
                    ActiveLexer::String(lexer.morph())
                } else {
                    ActiveLexer::Normal(lexer)
                };
                (token, span, error, next)
            }
            ActiveLexer::String(mut lexer) => {
                let Some(result) = lexer.next() else {
                    break;
                };
                let span = lexer.span();
                let (token, error) = match result {
                    Ok(token) => (token.into(), false),
                    Err(_) => (Token::Error, true),
                };
                let next = if token == Token::DoubleQuote {
                    lexer.extras.mode = Mode::Normal;
                    ActiveLexer::Normal(lexer.morph())
                } else {
                    ActiveLexer::String(lexer)
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

impl From<NormalToken> for Token {
    fn from(token: NormalToken) -> Self {
        match token {
            NormalToken::True => Self::True,
            NormalToken::False => Self::False,
            NormalToken::Null => Self::Null,
            NormalToken::LBrace => Self::LBrace,
            NormalToken::RBrace => Self::RBrace,
            NormalToken::LBracket => Self::LBracket,
            NormalToken::RBracket => Self::RBracket,
            NormalToken::Comma => Self::Comma,
            NormalToken::Colon => Self::Colon,
            NormalToken::DoubleQuote => Self::DoubleQuote,
            NormalToken::Number => Self::Number,
            NormalToken::Whitespace => Self::Whitespace,
        }
    }
}

impl From<StringToken> for Token {
    fn from(token: StringToken) -> Self {
        match token {
            StringToken::DoubleQuote => Self::DoubleQuote,
            StringToken::EscapeSequence => Self::EscapeSequence,
            StringToken::StringText => Self::StringText,
        }
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

        let (tokens, spans) = tokenize(r#"["text"]"#, &mut diagnostics);
        assert_eq!(tokens[1], Token::DoubleQuote);
        assert_eq!(spans[1], 1..2);
        assert_eq!(spans[2], 2..6);
        assert_eq!(spans[3], 6..7);
    }
}
