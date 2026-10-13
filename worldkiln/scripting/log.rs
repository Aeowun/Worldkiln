#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogSeverity {
    Script,
    Error,
    Warning,
    System,
}

impl LogSeverity {
    pub fn name(self) -> &'static str {
        match self {
            Self::Script => "SCRIPT",
            Self::Error => "ERROR",
            Self::Warning => "WARNING",
            Self::System => "SYSTEM",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogRecord {
    pub severity: LogSeverity,
    pub script_path: Option<String>,
    pub entity_name: Option<String>,
    pub entity_id: Option<u64>,
    pub context_name: Option<String>, // function or event name
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
