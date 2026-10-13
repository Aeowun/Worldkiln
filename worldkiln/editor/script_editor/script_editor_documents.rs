use super::ScriptEditor;
use crate::scripting::ast::Program;
use crate::scripting::diagnostic::Diagnostics;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::source::SourceLocation;
use crate::scripting::source_map::SourceMap;
use std::path::PathBuf;

pub struct ScriptDocument {
    pub path: PathBuf,
    pub source: String,
    pub original_source: String,
    pub dirty: bool,
    pub diagnostics: Diagnostics,
    pub program: Option<Program>,
    pub source_map: SourceMap,
}

impl ScriptDocument {
    pub fn new(path: PathBuf, source: String) -> Self {
        let source_map = SourceMap::new(&source);

        let mut document = Self {
            path,
            original_source: source.clone(),
            source,
            dirty: false,
            diagnostics: Diagnostics::new(),
            program: None,
            source_map,
        };

        document.reparse();
        document
    }

    pub fn reparse(&mut self) {
        self.diagnostics = Diagnostics::new();
        self.source_map = SourceMap::new(&self.source);

        // Worldkiln 0.8.7 | WO 0.8.7-28 | 2026-10-06 12:40 -05:00
        // Syntax presentation is now owned by the editor widget. Parsing remains the
        // authority for AeoScript diagnostics/program state and no longer produces a
        // parallel render-span cache that can drift from the live text model.

        let file_name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());

        match Lexer::new(&self.source).tokenize() {
            Ok(tokens) => match Parser::new(tokens).parse() {
                Ok(program) => {
                    self.program = Some(program);
                }

                Err(error) => {
                    self.program = None;

                    self.diagnostics
                        .error(error.message, SourceLocation::new(file_name, error.span));
                }
            },

            Err(error) => {
                self.program = None;

                let offset = self
                    .source_map
                    .line_start(error.line)
                    .unwrap_or(0)
                    .saturating_add((error.column as u32).saturating_sub(1));

                let span = crate::scripting::source::SourceSpan::single(offset);

                self.diagnostics
                    .error(error.message, SourceLocation::new(file_name, span));
            }
        }
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        std::fs::write(&self.path, &self.source)?;

        self.original_source = self.source.clone();
        self.dirty = false;

        Ok(())
    }
}

impl ScriptEditor {
    pub(crate) fn next_open_document(&self) -> Option<PathBuf> {
        self.scripts_list
            .iter()
            .chain(self.playerscripts_list.iter())
            .find(|path| self.open_documents.contains_key(*path))
            .cloned()
            .or_else(|| self.open_documents.keys().next().cloned())
    }
}
