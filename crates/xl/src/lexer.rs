use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendError {
    pub source_name: String,
    pub location: SourceLocation,
    pub message: String,
    pub diagnostic: Option<Box<crate::source::Diagnostic>>,
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
            diagnostic: None,
        }
    }

    pub(crate) fn from_diagnostic(
        sources: &crate::source::SourceDatabase,
        diagnostic: crate::source::Diagnostic,
    ) -> Self {
        let label = diagnostic.labels.first().expect("diagnostic has a label");
        let file = sources.get(label.span.source);
        let position = file.position(label.span.range.start);
        Self {
            source_name: file.name.to_string(),
            location: SourceLocation {
                offset: label.span.range.start,
                line: position.line,
                column: position.column,
            },
            message: diagnostic.message.clone(),
            diagnostic: Some(Box::new(diagnostic)),
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
