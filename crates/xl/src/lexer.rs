use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl SourceLocation {
    fn start() -> Self {
        Self {
            offset: 0,
            line: 1,
            column: 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendError {
    pub source_name: String,
    pub location: SourceLocation,
    pub message: String,
}

impl FrontendError {
    pub(crate) fn new(
        source_name: impl Into<String>,
        location: SourceLocation,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source_name: source_name.into(),
            location,
            message: message.into(),
        }
    }
}

impl fmt::Display for FrontendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}: {}",
            self.source_name, self.location.line, self.location.column, self.message
        )
    }
}

impl std::error::Error for FrontendError {}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Atom(String),
    Identifier(String),
    Let,
    Fn,
    If,
    Else,
    Match,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
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
    Eof,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub location: SourceLocation,
}

pub(crate) fn lex(source_name: &str, source: &str) -> Result<Vec<Token>, FrontendError> {
    Lexer::new(source_name, source).lex_all()
}

struct Lexer<'a> {
    source_name: &'a str,
    source: &'a str,
    location: SourceLocation,
}

impl<'a> Lexer<'a> {
    fn new(source_name: &'a str, source: &'a str) -> Self {
        Self {
            source_name,
            source,
            location: SourceLocation::start(),
        }
    }

    fn lex_all(mut self) -> Result<Vec<Token>, FrontendError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let location = self.location;
            let Some(character) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    location,
                });
                return Ok(tokens);
            };

            let kind = match character {
                '0'..='9' => self.number()?,
                '"' => TokenKind::String(self.quoted_string()?),
                '\'' => self.atom()?,
                'b' if self.peek_second() == Some('"') => {
                    self.advance();
                    let text = self.quoted_string()?;
                    TokenKind::Bytes(text.into_bytes())
                }
                'a'..='z' | 'A'..='Z' | '_' => self.identifier(),
                '(' => self.single(TokenKind::LeftParen),
                ')' => self.single(TokenKind::RightParen),
                '{' => self.single(TokenKind::LeftBrace),
                '}' => self.single(TokenKind::RightBrace),
                '[' => self.single(TokenKind::LeftBracket),
                ']' => self.single(TokenKind::RightBracket),
                ',' => self.single(TokenKind::Comma),
                ':' => self.single(TokenKind::Colon),
                ';' => self.single(TokenKind::Semicolon),
                '.' => self.single(TokenKind::Dot),
                '+' => self.single(TokenKind::Plus),
                '-' => self.single(TokenKind::Minus),
                '*' => self.single(TokenKind::Star),
                '/' => self.single(TokenKind::Slash),
                '<' => self.single(TokenKind::Less),
                '=' => {
                    self.advance();
                    match self.peek() {
                        Some('=') => {
                            self.advance();
                            TokenKind::EqualEqual
                        }
                        Some('>') => {
                            self.advance();
                            TokenKind::FatArrow
                        }
                        _ => TokenKind::Equal,
                    }
                }
                '|' => {
                    self.advance();
                    if self.peek() != Some('>') {
                        return Err(self.error(location, "expected '>' after '|'"));
                    }
                    self.advance();
                    TokenKind::Pipe
                }
                _ => {
                    return Err(self.error(location, format!("unexpected character {character:?}")));
                }
            };
            tokens.push(Token { kind, location });
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.location.offset..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.source[self.location.offset..].chars().nth(1)
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.location.offset += character.len_utf8();
        if character == '\n' {
            self.location.line += 1;
            self.location.column = 1;
        } else {
            self.location.column += 1;
        }
        Some(character)
    }

    fn single(&mut self, kind: TokenKind) -> TokenKind {
        self.advance();
        kind
    }

    fn skip_trivia(&mut self) {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.advance();
            }
            if self.peek() == Some('/') && self.peek_second() == Some('/') {
                while self.peek().is_some_and(|character| character != '\n') {
                    self.advance();
                }
                continue;
            }
            break;
        }
    }

    fn number(&mut self) -> Result<TokenKind, FrontendError> {
        let location = self.location;
        let start = location.offset;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.advance();
        }
        let is_float = self.peek() == Some('.')
            && self
                .peek_second()
                .is_some_and(|character| character.is_ascii_digit());
        if is_float {
            self.advance();
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.advance();
            }
        }
        let text = &self.source[start..self.location.offset];
        if is_float {
            text.parse::<f64>()
                .map(TokenKind::Float)
                .map_err(|_| self.error(location, "invalid Float literal"))
        } else {
            text.parse::<i64>()
                .map(TokenKind::Int)
                .map_err(|_| self.error(location, "Int literal is outside the i64 range"))
        }
    }

    fn quoted_string(&mut self) -> Result<String, FrontendError> {
        let location = self.location;
        debug_assert_eq!(self.peek(), Some('"'));
        self.advance();
        let mut value = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(value),
                Some('\\') => {
                    let escaped = match self.advance() {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some(other) => {
                            return Err(
                                self.error(self.location, format!("unsupported escape \\{other}"))
                            );
                        }
                        None => return Err(self.error(location, "unterminated string literal")),
                    };
                    value.push(escaped);
                }
                Some(character) => value.push(character),
                None => return Err(self.error(location, "unterminated string literal")),
            }
        }
    }

    fn atom(&mut self) -> Result<TokenKind, FrontendError> {
        let location = self.location;
        self.advance();
        let start = self.location.offset;
        if !self
            .peek()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        {
            return Err(self.error(location, "an atom requires a symbolic name"));
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.advance();
        }
        Ok(TokenKind::Atom(
            self.source[start..self.location.offset].to_owned(),
        ))
    }

    fn identifier(&mut self) -> TokenKind {
        let start = self.location.offset;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.advance();
        }
        match &self.source[start..self.location.offset] {
            "let" => TokenKind::Let,
            "fn" => TokenKind::Fn,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            identifier => TokenKind::Identifier(identifier.to_owned()),
        }
    }

    fn error(&self, location: SourceLocation, message: impl Into<String>) -> FrontendError {
        FrontendError::new(self.source_name, location, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_literals_comments_and_operators() {
        let tokens = lex("test", "let x = b\"hi\"; // comment\nx |> f('Ok, 1.5)").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Let));
        assert!(matches!(tokens[3].kind, TokenKind::Bytes(ref value) if value == b"hi"));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Pipe));
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == TokenKind::Atom("Ok".into()))
        );
    }

    #[test]
    fn reports_source_location() {
        let error = lex("bad.xl", "\n  @").unwrap_err();
        assert_eq!(error.location.line, 2);
        assert_eq!(error.location.column, 3);
        assert!(error.to_string().starts_with("bad.xl:2:3:"));
    }
}
