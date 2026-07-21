pub mod json;
pub mod xl;

use crate::source::Diagnostic;

#[derive(Debug)]
pub struct Parse<T> {
    pub syntax: T,
    pub diagnostics: Vec<Diagnostic>,
}

impl<T> Parse<T> {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::source::Severity::Error)
    }
}

pub(crate) fn convert_diagnostics(
    source: crate::source::SourceId,
    diagnostics: Vec<codespan_reporting::diagnostic::Diagnostic<()>>,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let severity = match diagnostic.severity {
                codespan_reporting::diagnostic::Severity::Bug
                | codespan_reporting::diagnostic::Severity::Error => crate::source::Severity::Error,
                _ => crate::source::Severity::Warning,
            };
            let labels = diagnostic
                .labels
                .into_iter()
                .enumerate()
                .map(|(index, label)| crate::source::Label {
                    span: crate::source::Span {
                        source,
                        range: label.range,
                    },
                    message: label.message,
                    primary: index == 0,
                })
                .collect();
            Diagnostic {
                severity,
                message: diagnostic.message,
                labels,
                notes: diagnostic.notes,
            }
        })
        .collect()
}
