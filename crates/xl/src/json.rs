use crate::source::{Diagnostic, SourceDatabase, SourceId, Span};
use crate::syntax::json::lexer::Token;
use crate::syntax::json::parser::{CstData, Node, NodeRef, Rule};
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ValuePathSegment {
    Index(usize),
    Key(String),
}

pub type ValuePath = Vec<ValuePathSegment>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Provenance {
    pub values: BTreeMap<ValuePath, Span>,
    pub keys: BTreeMap<ValuePath, Span>,
}

#[derive(Clone, Debug)]
pub struct SourcedValue {
    pub value: Value,
    pub provenance: Provenance,
}

#[derive(Debug)]
pub struct JsonParse {
    pub cst: CstData,
    pub value: Option<SourcedValue>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_json(source_name: &str, source: &str) -> Result<Value, JsonError> {
    parse_json_with_provenance(source_name, source).map(|parsed| parsed.value)
}

pub fn parse_json_with_provenance(
    source_name: &str,
    source: &str,
) -> Result<SourcedValue, JsonError> {
    let mut sources = SourceDatabase::default();
    let source_id = sources.add(source_name, source);
    let parsed = parse_json_registered(&sources, source_id);
    parsed
        .value
        .ok_or_else(|| compatibility_error(&sources, source_id, &parsed.diagnostics))
}

pub fn parse_json_registered(sources: &SourceDatabase, source_id: SourceId) -> JsonParse {
    let source = sources.get(source_id);
    let parsed = crate::syntax::json::parse(source_id, &source.text);
    let mut diagnostics = parsed.diagnostics;
    let value = if diagnostics.is_empty() {
        match JsonLowerer::new(source_id, &source.text, &parsed.syntax).lower() {
            Ok(value) => Some(value),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        None
    };
    JsonParse {
        cst: parsed.syntax,
        value,
        diagnostics,
    }
}

fn compatibility_error(
    sources: &SourceDatabase,
    source_id: SourceId,
    diagnostics: &[Diagnostic],
) -> JsonError {
    let diagnostic = diagnostics
        .first()
        .expect("failed JSON parse has a diagnostic");
    let offset = diagnostic
        .labels
        .first()
        .map_or(0, |label| label.span.range.start);
    let position = sources.get(source_id).position(offset);
    JsonError {
        source_name: sources.get(source_id).name.to_string(),
        line: position.line,
        column: position.column,
        message: diagnostic.message.clone(),
    }
}

struct JsonLowerer<'a> {
    source_id: SourceId,
    source: &'a str,
    cst: &'a CstData,
    vm: Vm,
    path: ValuePath,
    provenance: Provenance,
}

impl<'a> JsonLowerer<'a> {
    fn new(source_id: SourceId, source: &'a str, cst: &'a CstData) -> Self {
        Self {
            source_id,
            source,
            cst,
            vm: Vm::new(),
            path: Vec::new(),
            provenance: Provenance::default(),
        }
    }

    fn lower(mut self) -> Result<SourcedValue, Diagnostic> {
        let value_node = self
            .children(NodeRef::ROOT)
            .find(|node| self.is_value(*node))
            .ok_or_else(|| self.error(NodeRef::ROOT, "expected a JSON value"))?;
        let value = self.value(value_node)?;
        Ok(SourcedValue {
            value,
            provenance: self.provenance,
        })
    }

    fn value(&mut self, node: NodeRef) -> Result<Value, Diagnostic> {
        let value = match self.cst.get(node) {
            Node::Token(Token::Null, _) => Value::none(),
            Node::Token(Token::True, _) => Value::Atom(crate::Atom::builtin(BuiltinAtom::True)),
            Node::Token(Token::False, _) => Value::Atom(crate::Atom::builtin(BuiltinAtom::False)),
            Node::Token(Token::String, _) => Value::string(self.decode_string(node)?),
            Node::Token(Token::Number, _) => self.number(node)?,
            Node::Rule(Rule::Literal | Rule::Value, _) => {
                let child = self
                    .children(node)
                    .find(|child| self.is_value(*child))
                    .ok_or_else(|| self.error(node, "empty JSON value"))?;
                return self.value(child);
            }
            Node::Rule(Rule::Array, _) => self.array(node)?,
            Node::Rule(Rule::Object, _) => self.object(node)?,
            _ => return Err(self.error(node, "expected a JSON value")),
        };
        self.provenance
            .values
            .insert(self.path.clone(), self.span(node));
        Ok(value)
    }

    fn array(&mut self, node: NodeRef) -> Result<Value, Diagnostic> {
        let children = self
            .children(node)
            .filter(|child| self.is_value(*child))
            .collect::<Vec<_>>();
        let mut values = Vec::with_capacity(children.len());
        for (index, child) in children.into_iter().enumerate() {
            self.path.push(ValuePathSegment::Index(index));
            values.push(self.value(child)?);
            self.path.pop();
        }
        Ok(Value::Array(values.into()))
    }

    fn object(&mut self, node: NodeRef) -> Result<Value, Diagnostic> {
        let members = self
            .rule_children(node)
            .filter(|child| self.rule(*child) == Some(Rule::Member))
            .collect::<Vec<_>>();
        let mut fields = BTreeMap::new();
        let mut key_spans: BTreeMap<String, Span> = BTreeMap::new();
        for member in members {
            let key_node = self
                .token_children(member, Token::String)
                .next()
                .ok_or_else(|| self.error(member, "JSON object key must be a string"))?;
            let key = self.decode_string(key_node)?;
            let key_span = self.span(key_node);
            if let Some(previous) = key_spans.get(&key) {
                return Err(Diagnostic::error(
                    format!("duplicate JSON object key {key:?}"),
                    key_span,
                )
                .with_secondary("first defined here", previous.clone()));
            }
            let value_node = self
                .children(member)
                .find(|child| *child != key_node && self.is_value(*child))
                .ok_or_else(|| self.error(member, "JSON member has no value"))?;
            self.path.push(ValuePathSegment::Key(key.clone()));
            self.provenance
                .keys
                .insert(self.path.clone(), key_span.clone());
            let value = self.value(value_node)?;
            self.path.pop();
            key_spans.insert(key.clone(), key_span);
            fields.insert(key, value);
        }
        self.vm
            .make_dict(fields)
            .map_err(|message| self.error(node, message))
    }

    fn number(&self, node: NodeRef) -> Result<Value, Diagnostic> {
        let text = self.text(node);
        if text.contains(['.', 'e', 'E']) {
            let value = text
                .parse::<f64>()
                .map_err(|_| self.error(node, "invalid Float value"))?;
            if !value.is_finite() {
                return Err(self.error(node, "JSON Float must be finite"));
            }
            Ok(Value::Float(value))
        } else {
            text.parse::<i64>()
                .map(Value::Int)
                .map_err(|_| self.error(node, "JSON integer is outside the i64 range"))
        }
    }

    fn decode_string(&self, node: NodeRef) -> Result<String, Diagnostic> {
        let text = self.text(node);
        let mut decoder = StringDecoder {
            bytes: text.as_bytes(),
            offset: 1,
        };
        let mut output = String::new();
        while decoder.offset < decoder.bytes.len() - 1 {
            let byte = decoder.bytes[decoder.offset];
            if byte != b'\\' {
                let character = text[decoder.offset..].chars().next().expect("valid UTF-8");
                output.push(character);
                decoder.offset += character.len_utf8();
                continue;
            }
            decoder.offset += 1;
            let escaped = *decoder
                .bytes
                .get(decoder.offset)
                .ok_or_else(|| self.error(node, "unterminated JSON escape"))?;
            decoder.offset += 1;
            match escaped {
                b'"' => output.push('"'),
                b'\\' => output.push('\\'),
                b'/' => output.push('/'),
                b'b' => output.push('\u{0008}'),
                b'f' => output.push('\u{000c}'),
                b'n' => output.push('\n'),
                b'r' => output.push('\r'),
                b't' => output.push('\t'),
                b'u' => output.push(
                    decoder
                        .unicode_escape()
                        .map_err(|message| self.error(node, message))?,
                ),
                other => {
                    return Err(
                        self.error(node, format!("invalid JSON escape \\{}", char::from(other)))
                    );
                }
            }
        }
        Ok(output)
    }

    fn is_value(&self, node: NodeRef) -> bool {
        matches!(
            self.cst.get(node),
            Node::Token(
                Token::String | Token::Number | Token::True | Token::False | Token::Null,
                _
            )
        ) || matches!(
            self.rule(node),
            Some(Rule::Value | Rule::Literal | Rule::Array | Rule::Object)
        )
    }
    fn children(&self, node: NodeRef) -> impl Iterator<Item = NodeRef> + '_ {
        self.cst.children(node)
    }
    fn rule_children(&self, node: NodeRef) -> impl Iterator<Item = NodeRef> + '_ {
        self.children(node)
            .filter(|child| matches!(self.cst.get(*child), Node::Rule(..)))
    }
    fn token_children(&self, node: NodeRef, token: Token) -> impl Iterator<Item = NodeRef> + '_ {
        self.children(node).filter(
            move |child| matches!(self.cst.get(*child), Node::Token(found, _) if found == token),
        )
    }
    fn rule(&self, node: NodeRef) -> Option<Rule> {
        match self.cst.get(node) {
            Node::Rule(rule, _) => Some(rule),
            Node::Token(..) => None,
        }
    }
    fn text(&self, node: NodeRef) -> &str {
        &self.source[self.cst.span(node)]
    }
    fn span(&self, node: NodeRef) -> Span {
        Span {
            source: self.source_id,
            range: self.cst.span(node),
        }
    }
    fn error(&self, node: NodeRef, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(message, self.span(node))
    }
}

struct StringDecoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl StringDecoder<'_> {
    fn unicode_escape(&mut self) -> Result<char, &'static str> {
        let first = self.hex_quad()?;
        let codepoint = if (0xd800..=0xdbff).contains(&first) {
            if self.bytes.get(self.offset..self.offset + 2) != Some(b"\\u") {
                return Err("high surrogate requires a low surrogate");
            }
            self.offset += 2;
            let second = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err("invalid low surrogate");
            }
            0x10000 + (((first - 0xd800) as u32) << 10) + (second - 0xdc00) as u32
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err("unexpected low surrogate");
        } else {
            first as u32
        };
        char::from_u32(codepoint).ok_or("invalid Unicode scalar value")
    }

    fn hex_quad(&mut self) -> Result<u16, &'static str> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or("Unicode escape requires four hex digits")?;
            self.offset += 1;
            let digit = char::from(byte)
                .to_digit(16)
                .ok_or("Unicode escape requires four hex digits")?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_all_json_categories_directly_from_cst() {
        let value = parse_json(
            "test",
            r#"{"a":null,"b":true,"c":false,"d":-2,"e":1.5,"f":["x"]}"#,
        )
        .unwrap();
        assert_eq!(
            value.to_string(),
            "{a: 'None, b: 'True, c: 'False, d: -2, e: 1.5, f: [\"x\"]}"
        );
    }

    #[test]
    fn decodes_unicode_surrogate_pairs() {
        assert_eq!(
            parse_json("test", r#""\uD83D\uDE00""#).unwrap().to_string(),
            "\"😀\""
        );
    }

    #[test]
    fn reports_precise_duplicate_and_number_ranges() {
        let mut sources = SourceDatabase::default();
        let duplicate = sources.add("duplicate.json", r#"{"a":1,"a":2}"#);
        let parsed = parse_json_registered(&sources, duplicate);
        assert_eq!(parsed.diagnostics[0].labels[0].span.range, 7..10);
        assert_eq!(parsed.diagnostics[0].labels[1].span.range, 1..4);

        let large = sources.add("large.json", "9223372036854775808");
        let parsed = parse_json_registered(&sources, large);
        assert_eq!(parsed.diagnostics[0].labels[0].span.range, 0..19);
    }

    #[test]
    fn records_shared_database_provenance() {
        let mut sources = SourceDatabase::default();
        let first = sources.add("first.json", r#"{"name":"Ada"}"#);
        let second = sources.add("second.json", r#"{"name":"Lin"}"#);
        let first = parse_json_registered(&sources, first).value.unwrap();
        let second = parse_json_registered(&sources, second).value.unwrap();
        let path = vec![ValuePathSegment::Key("name".into())];
        assert_ne!(
            first.provenance.values[&path].source,
            second.provenance.values[&path].source
        );
        assert_eq!(first.provenance.values[&path].range, 8..13);
    }
}
