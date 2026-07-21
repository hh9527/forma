use std::fmt;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub source: SourceId,
    pub range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            labels: vec![Label {
                span,
                message: String::new(),
                primary: true,
            }],
            notes: Vec::new(),
        }
    }

    pub fn with_secondary(mut self, message: impl Into<String>, span: Span) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub name: Arc<str>,
    pub text: Arc<str>,
    line_starts: Vec<usize>,
}

impl SourceFile {
    fn new(name: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(offset, _)| offset + 1));
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let start = self.line_starts[line];
        Position {
            line: line + 1,
            column: self.text[start..offset].chars().count() + 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceDatabase {
    files: Vec<SourceFile>,
}

impl SourceDatabase {
    pub fn add(&mut self, name: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> SourceId {
        let id = SourceId(u32::try_from(self.files.len()).expect("too many source files"));
        self.files.push(SourceFile::new(name, text));
        id
    }

    pub fn get(&self, id: SourceId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let Some(label) = diagnostic.labels.iter().find(|label| label.primary) else {
            return diagnostic.message.clone();
        };
        let file = self.get(label.span.source);
        let position = file.position(label.span.range.start);
        let mut rendered = format!(
            "{}:{}:{}: {}",
            file.name, position.line, position.column, diagnostic.message
        );
        for secondary in diagnostic.labels.iter().filter(|label| !label.primary) {
            let file = self.get(secondary.span.source);
            let position = file.position(secondary.span.range.start);
            rendered.push_str(&format!(
                "\n  {}:{}:{}: {}",
                file.name, position.line, position.column, secondary.message
            ));
        }
        rendered
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_utf8_byte_offsets_to_character_columns() {
        let mut sources = SourceDatabase::default();
        let source = sources.add("utf8.xl", "一二\n  x");
        let span = Span {
            source,
            range: 9..10,
        };
        let diagnostic = Diagnostic::error("bad value", span);
        assert_eq!(sources.render(&diagnostic), "utf8.xl:2:3: bad value");
    }

    #[test]
    fn validation_diagnostic_can_label_data_and_rule_sources() {
        let mut sources = SourceDatabase::default();
        let data = sources.add("user.json", "{\"age\":\"old\"}");
        let rule = sources.add("schema.xl", "type User = Int;");
        let diagnostic = Diagnostic::error(
            "expected Int",
            Span {
                source: data,
                range: 7..12,
            },
        )
        .with_secondary(
            "required by User",
            Span {
                source: rule,
                range: 12..15,
            },
        );
        assert_eq!(diagnostic.labels.len(), 2);
        assert!(diagnostic.labels[0].primary);
        assert!(!diagnostic.labels[1].primary);
        assert_eq!(
            sources.render(&diagnostic),
            "user.json:1:8: expected Int\n  schema.xl:1:13: required by User"
        );
    }
}
