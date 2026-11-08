// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::ScriptEditor;
use crate::scripting::ast::Program;
use crate::scripting::diagnostic::Diagnostics;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::source::SourceLocation;
use crate::scripting::source_map::SourceMap;
use std::path::PathBuf;

pub struct ScriptDocument {
    pub native_draft: bool,
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
            native_draft: false,
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

    pub(crate) fn accept_external_source(&mut self, disk: &str) -> Result<bool, String> {
        if disk == self.original_source {
            return Ok(false);
        }
        let checked = ScriptDocument::new(self.path.clone(), disk.to_owned());
        if checked.program.is_none() {
            return Err("outside script has syntax errors; current buffer preserved".into());
        }
        let merged = merge_external_text(&self.original_source, &self.source, disk)?;
        let checked = ScriptDocument::new(self.path.clone(), merged.clone());
        if checked.program.is_none() {
            return Err("merged script has syntax errors; both versions preserved".into());
        }
        self.source = merged;
        self.original_source = disk.to_owned();
        self.dirty = self.source != self.original_source;
        self.reparse();
        Ok(true)
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let merged = self.source.clone();
        if !crate::project::native_logic::save_native_script(
            &self.path,
            &merged,
            self.native_draft,
        )? {
            crate::project::atomic_file::write_atomically(&self.path, merged.as_bytes())?;
        }
        self.native_draft = false;
        self.source = merged;
        self.original_source = self.source.clone();
        self.dirty = false;
        self.reparse();
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

pub(crate) fn merge_external_text(base: &str, local: &str, disk: &str) -> Result<String, String> {
    if local == disk || disk == base {
        return Ok(local.to_owned());
    }
    if local == base {
        return Ok(disk.to_owned());
    }
    let base: Vec<_> = base.split_inclusive('\n').collect();
    let local: Vec<_> = local.split_inclusive('\n').collect();
    let disk: Vec<_> = disk.split_inclusive('\n').collect();
    fn span<'a>(base: &[&str], next: &[&'a str]) -> (usize, usize, Vec<&'a str>) {
        let prefix = base.iter().zip(next).take_while(|(a, b)| a == b).count();
        let suffix = base[prefix..]
            .iter()
            .rev()
            .zip(next[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        (
            prefix,
            base.len() - suffix,
            next[prefix..next.len() - suffix].to_vec(),
        )
    }
    let a = span(&base, &local);
    let b = span(&base, &disk);
    if (a.0 == a.1 && b.0 == b.1 && a.0 == b.0) || !(a.1 <= b.0 || b.1 <= a.0) {
        return Err(
            "same script region changed inside and outside Worldkiln; both versions preserved"
                .into(),
        );
    }
    let mut merged = base;
    let (first, second) = if a.0 > b.0 { (a, b) } else { (b, a) };
    merged.splice(first.0..first.1, first.2);
    merged.splice(second.0..second.1, second.2);
    Ok(merged.concat())
}
