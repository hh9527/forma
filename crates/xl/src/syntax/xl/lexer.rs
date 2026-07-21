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
    UnknownEscapeSequence,
    UnterminatedEscapeSequence,
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
    #[regex(r#"\\[nrt"\\]"#, priority = 4)]
    EscapeSequence,
    #[regex(r#"\\[^\r\n]"#, priority = 3)]
    UnknownEscapeSequence,
    #[token("\\", priority = 1)]
    UnterminatedEscapeSequence,
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
                    Ok(token) => (token.into(), false),
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
                    Ok(token) => (token.into(), false),
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
        let error = error || token_is_invalid_escape(token);
        if error {
            let message = match token {
                Token::UnknownEscapeSequence => "unsupported string escape",
                Token::UnterminatedEscapeSequence => "unterminated string escape",
                _ => "invalid token",
            };
            diags.push(
                Diagnostic::error()
                    .with_message(message)
                    .with_label(Label::primary((), span.clone())),
            );
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
            NormalToken::Let => Self::Let,
            NormalToken::Type => Self::Type,
            NormalToken::Fn => Self::Fn,
            NormalToken::If => Self::If,
            NormalToken::Else => Self::Else,
            NormalToken::Match => Self::Match,
            NormalToken::Import => Self::Import,
            NormalToken::From => Self::From,
            NormalToken::LParen => Self::LParen,
            NormalToken::RParen => Self::RParen,
            NormalToken::LBrace => Self::LBrace,
            NormalToken::RBrace => Self::RBrace,
            NormalToken::LBracket => Self::LBracket,
            NormalToken::RBracket => Self::RBracket,
            NormalToken::Comma => Self::Comma,
            NormalToken::Colon => Self::Colon,
            NormalToken::Semicolon => Self::Semicolon,
            NormalToken::Dot => Self::Dot,
            NormalToken::Plus => Self::Plus,
            NormalToken::Minus => Self::Minus,
            NormalToken::Star => Self::Star,
            NormalToken::Slash => Self::Slash,
            NormalToken::Less => Self::Less,
            NormalToken::EqualEqual => Self::EqualEqual,
            NormalToken::Equal => Self::Equal,
            NormalToken::FatArrow => Self::FatArrow,
            NormalToken::Pipe => Self::Pipe,
            NormalToken::Int => Self::Int,
            NormalToken::Float => Self::Float,
            NormalToken::DoubleQuote => Self::DoubleQuote,
            NormalToken::Bytes => Self::Bytes,
            NormalToken::Atom => Self::Atom,
            NormalToken::Identifier => Self::Identifier,
            NormalToken::Whitespace => Self::Whitespace,
            NormalToken::Comment => Self::Comment,
        }
    }
}

impl From<StringToken> for Token {
    fn from(token: StringToken) -> Self {
        match token {
            StringToken::DoubleQuote => Self::DoubleQuote,
            StringToken::InterpolationStart => Self::InterpolationStart,
            StringToken::EscapeSequence => Self::EscapeSequence,
            StringToken::UnknownEscapeSequence => Self::UnknownEscapeSequence,
            StringToken::UnterminatedEscapeSequence => Self::UnterminatedEscapeSequence,
            StringToken::StringText => Self::StringText,
        }
    }
}

fn token_is_invalid_escape(token: Token) -> bool {
    matches!(
        token,
        Token::UnknownEscapeSequence | Token::UnterminatedEscapeSequence
    )
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

    #[test]
    fn preserves_unknown_and_unterminated_escapes_as_tokens() {
        let mut diagnostics = Vec::new();
        let (tokens, spans) = tokenize(r#""a\(b""#, &mut diagnostics);
        assert_eq!(tokens[2], Token::UnknownEscapeSequence);
        assert_eq!(spans[2], 2..4);
        assert_eq!(diagnostics.len(), 1);

        diagnostics.clear();
        let (tokens, spans) = tokenize("\"a\\", &mut diagnostics);
        assert_eq!(tokens.last(), Some(&Token::UnterminatedEscapeSequence));
        assert_eq!(spans.last(), Some(&(2..3)));
        assert_eq!(diagnostics.len(), 1);
    }
}
