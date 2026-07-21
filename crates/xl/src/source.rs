use std::fmt;
use std::ops::Range;
use std::sync::Arc;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    pub fn new(start: u32, end: u32) -> Result<Self, LocationError> {
        if start > end {
            return Err(LocationError::ReversedRange { start, end });
        }
        Ok(Self { start, end })
    }

    pub fn at(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn from_usize(range: Range<usize>) -> Result<Self, LocationError> {
        let start = u32::try_from(range.start).map_err(|_| LocationError::OffsetTooLarge)?;
        let end = u32::try_from(range.end).map_err(|_| LocationError::OffsetTooLarge)?;
        Self::new(start, end)
    }

    pub fn to_usize(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Location {
    pub source: SourceId,
    pub range: TextRange,
}

impl Location {
    pub fn new(source: SourceId, range: TextRange) -> Self {
        Self { source, range }
    }

    pub fn from_usize(source: SourceId, range: Range<usize>) -> Result<Self, LocationError> {
        Ok(Self::new(source, TextRange::from_usize(range)?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocationError {
    OffsetTooLarge,
    SourceTooLarge,
    ReversedRange { start: u32, end: u32 },
}

impl fmt::Display for LocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffsetTooLarge => formatter.write_str("source offset exceeds u32::MAX"),
            Self::SourceTooLarge => formatter.write_str("source text exceeds u32::MAX bytes"),
            Self::ReversedRange { start, end } => {
                write!(
                    formatter,
                    "source range starts at {start} after ending at {end}"
                )
            }
        }
    }
}

impl std::error::Error for LocationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Located<T> {
    pub value: T,
    pub location: Location,
}

impl<T> Located<T> {
    pub fn new(value: T, location: Location) -> Self {
        Self { value, location }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Located<U> {
        Located::new(map(self.value), self.location)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    Source(Location),
    Synthetic { derived_from: Option<Location> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithOrigin<T> {
    pub value: T,
    pub origin: Origin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Label {
    pub location: Location,
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
    pub fn error(message: impl Into<String>, location: Location) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            labels: vec![Label {
                location,
                message: String::new(),
                primary: true,
            }],
            notes: Vec::new(),
        }
    }

    pub fn with_secondary(mut self, message: impl Into<String>, location: Location) -> Self {
        self.labels.push(Label {
            location,
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
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(name: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> Result<Self, LocationError> {
        let text = text.into();
        if text.len() > u32::MAX as usize {
            return Err(LocationError::SourceTooLarge);
        }
        let mut line_starts = vec![0];
        for (offset, _) in text.match_indices('\n') {
            line_starts.push(u32::try_from(offset + 1).map_err(|_| LocationError::SourceTooLarge)?);
        }
        Ok(Self {
            name: name.into(),
            text,
            line_starts,
        })
    }

    pub fn position(&self, offset: u32) -> Position {
        let offset = offset.min(self.text.len() as u32);
        let line = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let start = self.line_starts[line] as usize;
        Position {
            line: line + 1,
            column: self.text[start..offset as usize].chars().count() + 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceDatabase {
    files: Vec<SourceFile>,
}

impl SourceDatabase {
    pub fn try_add(
        &mut self,
        name: impl Into<Arc<str>>,
        text: impl Into<Arc<str>>,
    ) -> Result<SourceId, LocationError> {
        let id =
            SourceId(u32::try_from(self.files.len()).map_err(|_| LocationError::SourceTooLarge)?);
        self.files.push(SourceFile::new(name, text)?);
        Ok(id)
    }

    pub fn add(&mut self, name: impl Into<Arc<str>>, text: impl Into<Arc<str>>) -> SourceId {
        self.try_add(name, text)
            .expect("source fits compact location model")
    }

    pub fn get(&self, id: SourceId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn render(&self, diagnostic: &Diagnostic) -> String {
        let Some(label) = diagnostic.labels.iter().find(|label| label.primary) else {
            return diagnostic.message.clone();
        };
        let file = self.get(label.location.source);
        let position = file.position(label.location.range.start);
        let mut rendered = format!(
            "{}:{}:{}: {}",
            file.name, position.line, position.column, diagnostic.message
        );
        for secondary in diagnostic.labels.iter().filter(|label| !label.primary) {
            let file = self.get(secondary.location.source);
            let position = file.position(secondary.location.range.start);
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
    fn compact_layout_and_checked_ranges() {
        assert_eq!(std::mem::size_of::<SourceId>(), 4);
        assert_eq!(std::mem::size_of::<TextRange>(), 8);
        assert_eq!(std::mem::size_of::<Location>(), 12);
        assert!(TextRange::new(2, 1).is_err());
        assert!(TextRange::from_usize(0..usize::MAX).is_err());
    }

    #[test]
    fn resolves_utf8_byte_offsets_to_character_columns() {
        let mut sources = SourceDatabase::default();
        let source = sources.add("utf8.xl", "一二\n  x");
        let location = Location::new(source, TextRange::new(9, 10).unwrap());
        let diagnostic = Diagnostic::error("bad value", location);
        assert_eq!(sources.render(&diagnostic), "utf8.xl:2:3: bad value");
    }

    #[test]
    fn validation_diagnostic_can_label_data_and_rule_sources() {
        let mut sources = SourceDatabase::default();
        let data = sources.add("user.json", "{\"age\":\"old\"}");
        let rule = sources.add("schema.xl", "type User = Int;");
        let diagnostic = Diagnostic::error(
            "expected Int",
            Location::new(data, TextRange::new(7, 12).unwrap()),
        )
        .with_secondary(
            "required by User",
            Location::new(rule, TextRange::new(12, 15).unwrap()),
        );
        assert_eq!(
            sources.render(&diagnostic),
            "user.json:1:8: expected Int\n  schema.xl:1:13: required by User"
        );
    }
}
