use super::source::SourceLocation;

/// Severity used by the AeoScript diagnostic system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A compiler/parser/runtime diagnostic.
///
/// Diagnostics reference a source location rather than storing source text.
/// This keeps diagnostics lightweight and lets the editor resolve the span
/// against the original source when it needs line/column information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub location: SourceLocation,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            location,
        }
    }

    pub fn warning(message: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            location,
        }
    }

    pub fn info(message: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            severity: DiagnosticSeverity::Info,
            message: message.into(),
            location,
        }
    }

    pub fn hint(message: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            severity: DiagnosticSeverity::Hint,
            message: message.into(),
            location,
        }
    }
}

/// Collection of diagnostics produced by one compilation stage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    entries: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.entries.push(diagnostic);
    }

    pub fn error(&mut self, message: impl Into<String>, location: SourceLocation) {
        self.push(Diagnostic::error(message, location));
    }

    pub fn warning(&mut self, message: impl Into<String>, location: SourceLocation) {
        self.push(Diagnostic::warning(message, location));
    }

    pub fn info(&mut self, message: impl Into<String>, location: SourceLocation) {
        self.push(Diagnostic::info(message, location));
    }

    pub fn hint(&mut self, message: impl Into<String>, location: SourceLocation) {
        self.push(Diagnostic::hint(message, location));
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.severity == DiagnosticSeverity::Error)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.entries
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticSeverity, Diagnostics};
    use crate::scripting::source::{SourceLocation, SourceSpan};

    fn test_location() -> SourceLocation {
        SourceLocation::new(Some("test.aeo".to_string()), SourceSpan::new(9, 14))
    }

    #[test]
    fn diagnostic_preserves_message_and_location() {
        let location = test_location();

        let diagnostic = Diagnostic::error("unknown variable", location.clone());

        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.message, "unknown variable");
        assert_eq!(diagnostic.location, location);
    }

    #[test]
    fn diagnostic_collection_tracks_errors() {
        let mut diagnostics = Diagnostics::new();

        assert!(diagnostics.is_empty());
        assert!(!diagnostics.has_errors());

        diagnostics.error("bad expression", test_location());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn warnings_do_not_count_as_errors() {
        let mut diagnostics = Diagnostics::new();

        diagnostics.warning("unused variable", test_location());

        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn diagnostics_can_be_consumed() {
        let mut diagnostics = Diagnostics::new();

        diagnostics.info("script loaded", test_location());

        let entries = diagnostics.into_vec();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn all_severity_levels_are_supported() {
        let location = test_location();

        let error = Diagnostic::error("error", location.clone());

        let warning = Diagnostic::warning("warning", location.clone());

        let info = Diagnostic::info("info", location.clone());

        let hint = Diagnostic::hint("hint", location);

        assert_eq!(error.severity, DiagnosticSeverity::Error);
        assert_eq!(warning.severity, DiagnosticSeverity::Warning);
        assert_eq!(info.severity, DiagnosticSeverity::Info);
        assert_eq!(hint.severity, DiagnosticSeverity::Hint);
    }
}
