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
    Decl,
    Def,
    Native,
    Type,
    Fn,
    If,
    Else,
    Match,
    Import,
    From,
    SectionLParen,
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
    At,
    Plus,
    Minus,
    Star,
    Slash,
    Less,
    EqualEqual,
    Equal,
    Arrow,
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
    Placeholder,
    IndexedPlaceholder,
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
    #[token("decl")]
    Decl,
    #[token("def")]
    Def,
    #[token("native")]
    Native,
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
    #[token("\\(")]
    SectionLParen,
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
    #[token("@")]
    At,
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
    #[token("->")]
    Arrow,
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
    #[token("_", priority = 4)]
    Placeholder,
    #[regex(r"_[0-9]+", priority = 4)]
    IndexedPlaceholder,
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

pub fn tokenize_document(
    source: &crate::document::DocumentText,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<Token>, Vec<Span>) {
    tokenize_fragments(source.chunks(), diags)
}

fn tokenize_fragments<'a>(
    fragments: impl IntoIterator<Item = &'a str>,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<Token>, Vec<Span>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut pending = String::new();
    let mut pending_start = 0;

    for fragment in fragments {
        pending.push_str(fragment);
        let mut local_diags = Vec::new();
        let (local_tokens, local_spans) = tokenize(&pending, &mut local_diags);
        let commit = stable_root_prefix(&local_tokens);
        if commit == 0 {
            continue;
        }
        let committed_end = local_spans[commit - 1].end;
        tokens.extend_from_slice(&local_tokens[..commit]);
        spans.extend(
            local_spans[..commit]
                .iter()
                .map(|span| pending_start + span.start..pending_start + span.end),
        );
        for mut diagnostic in local_diags {
            if diagnostic
                .labels
                .iter()
                .all(|label| label.range.end <= committed_end)
            {
                for label in &mut diagnostic.labels {
                    label.range =
                        pending_start + label.range.start..pending_start + label.range.end;
                }
                diags.push(diagnostic);
            }
        }
        pending.drain(..committed_end);
        pending_start += committed_end;
    }

    if !pending.is_empty() {
        let mut local_diags = Vec::new();
        let (local_tokens, local_spans) = tokenize(&pending, &mut local_diags);
        tokens.extend(local_tokens);
        spans.extend(
            local_spans
                .into_iter()
                .map(|span| pending_start + span.start..pending_start + span.end),
        );
        for mut diagnostic in local_diags {
            for label in &mut diagnostic.labels {
                label.range = pending_start + label.range.start..pending_start + label.range.end;
            }
            diags.push(diagnostic);
        }
    }

    (tokens, spans)
}

fn stable_root_prefix(tokens: &[Token]) -> usize {
    let mut contexts = vec![Context::Root];
    let mut root_boundaries = Vec::new();
    for (index, token) in tokens.iter().copied().enumerate() {
        let context = *contexts.last().expect("lexer has a root context");
        match (context, token) {
            (Context::Root | Context::Interpolation { .. }, Token::DoubleQuote) => {
                contexts.push(Context::String);
            }
            (Context::String, Token::DoubleQuote) => {
                contexts.pop();
            }
            (Context::String, Token::InterpolationStart) => {
                contexts.push(Context::Interpolation { brace_depth: 0 });
            }
            (Context::Interpolation { brace_depth }, Token::LBrace) => {
                *contexts.last_mut().expect("interpolation context") = Context::Interpolation {
                    brace_depth: brace_depth + 1,
                };
            }
            (Context::Interpolation { brace_depth: 0 }, Token::RBrace) => {
                contexts.pop();
            }
            (Context::Interpolation { brace_depth }, Token::RBrace) => {
                *contexts.last_mut().expect("interpolation context") = Context::Interpolation {
                    brace_depth: brace_depth - 1,
                };
            }
            _ => {}
        }
        if contexts.len() == 1 && index + 9 <= tokens.len() {
            root_boundaries.push(index + 1);
        }
    }
    root_boundaries.last().copied().unwrap_or(0)
}

impl From<NormalToken> for Token {
    fn from(token: NormalToken) -> Self {
        match token {
            NormalToken::Let => Self::Let,
            NormalToken::Decl => Self::Decl,
            NormalToken::Def => Self::Def,
            NormalToken::Native => Self::Native,
            NormalToken::Type => Self::Type,
            NormalToken::Fn => Self::Fn,
            NormalToken::If => Self::If,
            NormalToken::Else => Self::Else,
            NormalToken::Match => Self::Match,
            NormalToken::Import => Self::Import,
            NormalToken::From => Self::From,
            NormalToken::SectionLParen => Self::SectionLParen,
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
            NormalToken::At => Self::At,
            NormalToken::Plus => Self::Plus,
            NormalToken::Minus => Self::Minus,
            NormalToken::Star => Self::Star,
            NormalToken::Slash => Self::Slash,
            NormalToken::Less => Self::Less,
            NormalToken::EqualEqual => Self::EqualEqual,
            NormalToken::Arrow => Self::Arrow,
            NormalToken::Equal => Self::Equal,
            NormalToken::FatArrow => Self::FatArrow,
            NormalToken::Pipe => Self::Pipe,
            NormalToken::Int => Self::Int,
            NormalToken::Float => Self::Float,
            NormalToken::DoubleQuote => Self::DoubleQuote,
            NormalToken::Bytes => Self::Bytes,
            NormalToken::Atom => Self::Atom,
            NormalToken::Placeholder => Self::Placeholder,
            NormalToken::IndexedPlaceholder => Self::IndexedPlaceholder,
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
    fn chunk_bridge_matches_contiguous_lexing() {
        let samples = [
            "let identifier = 123.456 // comment\nidentifier",
            r#"b\"bytes\" \"text \{name} tail\""#,
            "_12 |> transform\\(_1, 2)",
            "let 中 = \"emoji 😀 and escape \\n\"; 中",
        ];
        for sample in samples {
            let mut expected_diags = Vec::new();
            let expected = tokenize(sample, &mut expected_diags);
            for split in sample
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(sample.len()))
            {
                let mut actual_diags = Vec::new();
                let actual = tokenize_fragments(
                    [sample.get(..split).unwrap(), sample.get(split..).unwrap()],
                    &mut actual_diags,
                );
                assert_eq!(actual, expected, "split at {split} in {sample:?}");
                assert_eq!(
                    actual_diags, expected_diags,
                    "split at {split} in {sample:?}"
                );
            }
        }

        let source = format!(
            "let value = \"{}\"; value",
            "long text with an escape \\n and interpolation-like text ".repeat(100)
        );
        let document = crate::document::DocumentText::new(&source);
        assert!(document.chunks().count() > 1);
        let mut expected_diags = Vec::new();
        let expected = tokenize(&source, &mut expected_diags);
        let mut actual_diags = Vec::new();
        let actual = tokenize_document(&document, &mut actual_diags);
        assert_eq!(actual, expected);
        assert_eq!(actual_diags, expected_diags);
    }

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
    fn recognizes_bare_and_indexed_placeholders_as_dedicated_tokens() {
        let mut diagnostics = Vec::new();
        let (tokens, spans) = tokenize(r"f\(_, _1, _0, _name)", &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(
            tokens,
            vec![
                Token::Identifier,
                Token::SectionLParen,
                Token::Placeholder,
                Token::Comma,
                Token::Whitespace,
                Token::IndexedPlaceholder,
                Token::Comma,
                Token::Whitespace,
                Token::IndexedPlaceholder,
                Token::Comma,
                Token::Whitespace,
                Token::Identifier,
                Token::RParen,
            ]
        );
        assert_eq!(spans[2], 3..4);
        assert_eq!(spans[5], 6..8);
        assert_eq!(spans[11], 14..19);
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
