// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
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
    pub context_name: Option<String>,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
