use crate::{BuiltinAtom, Value, Vm};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonError {
    pub source_name: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}: {}",
            self.source_name, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for JsonError {}

pub fn parse_json(source_name: &str, source: &str) -> Result<Value, JsonError> {
    let mut parser = JsonParser {
        source_name,
        source,
        offset: 0,
        line: 1,
        column: 1,
        vm: Vm::new(),
    };
    let value = parser.value()?;
    parser.whitespace();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected content after JSON value"));
    }
    Ok(value)
}

struct JsonParser<'a> {
    source_name: &'a str,
    source: &'a str,
    offset: usize,
    line: usize,
    column: usize,
    vm: Vm,
}

impl JsonParser<'_> {
    fn value(&mut self) -> Result<Value, JsonError> {
        self.whitespace();
        match self.peek() {
            Some('n') => {
                self.keyword("null")?;
                Ok(Value::none())
            }
            Some('t') => {
                self.keyword("true")?;
                Ok(Value::Atom(crate::Atom::builtin(BuiltinAtom::True)))
            }
            Some('f') => {
                self.keyword("false")?;
                Ok(Value::Atom(crate::Atom::builtin(BuiltinAtom::False)))
            }
            Some('"') => Ok(Value::string(self.string()?)),
            Some('[') => self.array(),
            Some('{') => self.object(),
            Some('-' | '0'..='9') => self.number(),
            Some(character) => Err(self.error(format!("unexpected character {character:?}"))),
            None => Err(self.error("expected a JSON value")),
        }
    }

    fn array(&mut self) -> Result<Value, JsonError> {
        self.expect('[')?;
        self.whitespace();
        let mut values = Vec::new();
        if self.consume(']') {
            return Ok(Value::Array(values.into()));
        }
        loop {
            values.push(self.value()?);
            self.whitespace();
            if self.consume(']') {
                break;
            }
            self.expect(',')?;
            self.whitespace();
        }
        Ok(Value::Array(values.into()))
    }

    fn object(&mut self) -> Result<Value, JsonError> {
        self.expect('{')?;
        self.whitespace();
        let mut fields = BTreeMap::new();
        if self.consume('}') {
            return self
                .vm
                .make_dict(fields)
                .map_err(|message| self.error(message));
        }
        loop {
            if self.peek() != Some('"') {
                return Err(self.error("JSON object keys must be strings"));
            }
            let field = self.string()?;
            self.whitespace();
            self.expect(':')?;
            let value = self.value()?;
            if fields.insert(field.clone(), value).is_some() {
                return Err(self.error(format!("duplicate JSON object key {field:?}")));
            }
            self.whitespace();
            if self.consume('}') {
                break;
            }
            self.expect(',')?;
            self.whitespace();
        }
        self.vm
            .make_dict(fields)
            .map_err(|message| self.error(message))
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.offset;
        self.consume('-');
        match self.peek() {
            Some('0') => {
                self.advance();
                if self.peek().is_some_and(|value| value.is_ascii_digit()) {
                    return Err(self.error("leading zero is not valid in a JSON number"));
                }
            }
            Some('1'..='9') => {
                while self.peek().is_some_and(|value| value.is_ascii_digit()) {
                    self.advance();
                }
            }
            _ => return Err(self.error("invalid JSON number")),
        }
        let mut float = false;
        if self.consume('.') {
            float = true;
            if !self.peek().is_some_and(|value| value.is_ascii_digit()) {
                return Err(self.error("fraction requires at least one digit"));
            }
            while self.peek().is_some_and(|value| value.is_ascii_digit()) {
                self.advance();
            }
        }
        if self.peek().is_some_and(|value| matches!(value, 'e' | 'E')) {
            float = true;
            self.advance();
            if self.peek().is_some_and(|value| matches!(value, '+' | '-')) {
                self.advance();
            }
            if !self.peek().is_some_and(|value| value.is_ascii_digit()) {
                return Err(self.error("exponent requires at least one digit"));
            }
            while self.peek().is_some_and(|value| value.is_ascii_digit()) {
                self.advance();
            }
        }
        let number = &self.source[start..self.offset];
        if float {
            let value = number
                .parse::<f64>()
                .map_err(|_| self.error("invalid Float value"))?;
            if !value.is_finite() {
                return Err(self.error("JSON Float must be finite"));
            }
            Ok(Value::Float(value))
        } else {
            number
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| self.error("JSON integer is outside the i64 range"))
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect('"')?;
        let mut result = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(result),
                Some('\\') => match self.advance() {
                    Some('"') => result.push('"'),
                    Some('\\') => result.push('\\'),
                    Some('/') => result.push('/'),
                    Some('b') => result.push('\u{0008}'),
                    Some('f') => result.push('\u{000c}'),
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some('u') => result.push(self.unicode_escape()?),
                    Some(other) => {
                        return Err(self.error(format!("invalid JSON escape \\{other}")));
                    }
                    None => return Err(self.error("unterminated JSON string")),
                },
                Some(character) if character <= '\u{001f}' => {
                    return Err(self.error("control character in JSON string"));
                }
                Some(character) => result.push(character),
                None => return Err(self.error("unterminated JSON string")),
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let first = self.hex_quad()?;
        let codepoint = if (0xd800..=0xdbff).contains(&first) {
            if self.advance() != Some('\\') || self.advance() != Some('u') {
                return Err(self.error("high surrogate requires a low surrogate"));
            }
            let second = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(self.error("invalid low surrogate"));
            }
            0x10000 + (((first - 0xd800) as u32) << 10) + (second - 0xdc00) as u32
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err(self.error("unexpected low surrogate"));
        } else {
            first as u32
        };
        char::from_u32(codepoint).ok_or_else(|| self.error("invalid Unicode scalar value"))
    }

    fn hex_quad(&mut self) -> Result<u16, JsonError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .advance()
                .and_then(|character| character.to_digit(16))
                .ok_or_else(|| self.error("Unicode escape requires four hex digits"))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    fn keyword(&mut self, expected: &str) -> Result<(), JsonError> {
        for expected in expected.chars() {
            if self.advance() != Some(expected) {
                return Err(self.error("invalid JSON keyword"));
            }
        }
        Ok(())
    }

    fn whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|character| matches!(character, ' ' | '\n' | '\r' | '\t'))
        {
            self.advance();
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), JsonError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected {expected:?}")))
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError {
            source_name: self.source_name.into(),
            line: self.line,
            column: self.column,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_json_value_categories() {
        let value = parse_json(
            "data.json",
            r#"{"z": null, "a": [true, false, 1, 2.5, "\u4f60\u597d"]}"#,
        )
        .unwrap();
        assert_eq!(
            value.to_string(),
            "{a: ['True, 'False, 1, 2.5, \"你好\"], z: 'None}"
        );
    }

    #[test]
    fn rejects_duplicate_keys_and_large_integers() {
        let duplicate = parse_json("bad.json", r#"{"a": 1, "a": 2}"#).unwrap_err();
        assert!(duplicate.message.contains("duplicate"));

        let large = parse_json("bad.json", "9223372036854775808").unwrap_err();
        assert!(large.message.contains("i64"));
    }

    #[test]
    fn decodes_unicode_surrogate_pairs() {
        let value = parse_json("unicode.json", r#""\ud83d\ude00""#).unwrap();
        assert_eq!(value.to_string(), "\"😀\"");
    }
}
