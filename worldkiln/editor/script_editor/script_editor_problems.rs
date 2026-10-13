use super::ScriptEditor;
use crate::scripting::diagnostic::DiagnosticSeverity;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::source_map::SourceMap;
use std::path::Path;

// Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:44 -05:00
// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:25 -05:00
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectProblemKind {
    Lexer,
    Parser,
    Semantic,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectProblem {
    pub kind: ProjectProblemKind,
    pub severity: DiagnosticSeverity,
    pub logical_path: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub byte_offset: usize,
}

fn severity_rank(severity: &DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        _ => 2,
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 03:06 -05:00
// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:25 -05:00
fn parse_problems(
    logical_path: &str,
    source: &str,
    available_modules: &std::collections::BTreeSet<String>,
) -> Vec<ProjectProblem> {
    let source_map = SourceMap::new(source);
    match Lexer::new(source).tokenize() {
        Ok(tokens) => match Parser::new(tokens).parse() {
            Ok(program) => crate::scripting::semantic::analyze_program(&program, available_modules)
                .into_iter()
                .map(|diagnostic| {
                    let (start, _) = source_map.span_positions(diagnostic.span);
                    ProjectProblem {
                        kind: ProjectProblemKind::Semantic,
                        severity: diagnostic.severity,
                        logical_path: logical_path.to_string(),
                        line: start.line,
                        column: start.column,
                        message: diagnostic.message,
                        byte_offset: diagnostic.span.start as usize,
                    }
                })
                .collect(),
            Err(error) => {
                let (start, _) = source_map.span_positions(error.span);
                vec![ProjectProblem {
                    kind: ProjectProblemKind::Parser,
                    severity: DiagnosticSeverity::Error,
                    logical_path: logical_path.to_string(),
                    line: start.line,
                    column: start.column,
                    message: error.message,
                    byte_offset: error.span.start as usize,
                }]
            }
        },
        Err(error) => {
            let offset = source_map
                .line_start(error.line)
                .unwrap_or(0)
                .saturating_add((error.column as u32).saturating_sub(1));
            vec![ProjectProblem {
                kind: ProjectProblemKind::Lexer,
                severity: DiagnosticSeverity::Error,
                logical_path: logical_path.to_string(),
                line: error.line,
                column: error.column,
                message: error.message,
                byte_offset: offset as usize,
            }]
        }
    }
}

impl ScriptEditor {
    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:44 -05:00
    pub fn refresh_project_problems(&mut self, project_root: Option<&Path>) {
        self.project_problems.clear();

        let Some(project_root) = project_root else {
            return;
        };

        // Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:25 -05:00
        let paths = self.indexed_script_paths();
        let available_modules = paths
            .iter()
            .filter_map(|path| Self::logical_script_path(project_root, path))
            .collect::<std::collections::BTreeSet<_>>();

        for path in paths {
            let Some(logical_path) = Self::logical_script_path(project_root, &path) else {
                continue;
            };

            let source = if let Some(document) = self.open_documents.get(&path) {
                Ok(document.source.clone())
            } else {
                std::fs::read_to_string(&path)
            };

            match source {
                Ok(source) => {
                    self.project_problems.extend(parse_problems(
                        &logical_path,
                        &source,
                        &available_modules,
                    ));
                }
                Err(error) => self.project_problems.push(ProjectProblem {
                    kind: ProjectProblemKind::Io,
                    severity: DiagnosticSeverity::Error,
                    logical_path,
                    line: 1,
                    column: 1,
                    message: format!("Failed to read indexed script: {error}"),
                    byte_offset: 0,
                }),
            }
        }

        self.project_problems.sort_by(|left, right| {
            severity_rank(&left.severity)
                .cmp(&severity_rank(&right.severity))
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.logical_path.cmp(&right.logical_path))
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.column.cmp(&right.column))
                .then_with(|| left.message.cmp(&right.message))
        });
        self.project_problems.dedup();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 03:06 -05:00
    pub fn project_problem_count(&self) -> usize {
        self.project_problems.len()
    }

    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 03:06 -05:00
    pub fn open_project_problem(&mut self, index: usize) -> Result<(), String> {
        let problem = self
            .project_problems
            .get(index)
            .cloned()
            .ok_or_else(|| format!("unknown project problem index {index}"))?;
        let indexed = self
            .project_script_index
            .as_ref()
            .and_then(|index| index.find_module(&problem.logical_path))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Problem target '{}' is no longer in the project script index.",
                    problem.logical_path
                )
            })?;
        let path = self
            .project_script_index
            .as_ref()
            .expect("index existence checked above")
            .absolute_path(&indexed);

        self.open_file(path.clone());
        if !self.open_documents.contains_key(&path) {
            return Err(format!(
                "Could not open indexed script '{}'.",
                problem.logical_path
            ));
        }
        self.cursor_request = Some((path, problem.byte_offset));
        Ok(())
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 03:06 -05:00
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "Worldkiln_ProjectProblems_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts/A")).unwrap();
        fs::create_dir_all(root.join("playerscripts/B")).unwrap();
        root
    }

    #[test]
    fn aggregates_both_roots_and_distinct_files() {
        let root = root("aggregate");
        fs::write(root.join("scripts/A/one.aeo"), "fn broken( {\n").unwrap();
        fs::write(root.join("playerscripts/B/two.aeo"), "const = 2\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));

        assert_eq!(editor.project_problem_count(), 2);
        assert!(
            editor
                .project_problems
                .iter()
                .any(|p| p.logical_path == "scripts/A/one.aeo")
        );
        assert!(
            editor
                .project_problems
                .iter()
                .any(|p| p.logical_path == "playerscripts/B/two.aeo")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsaved_open_buffer_overrides_disk_and_is_not_duplicated() {
        let root = root("unsaved");
        let path = root.join("scripts/A/one.aeo");
        fs::write(&path, "fn valid() {}\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        editor.open_file(path.clone());
        {
            let document = editor.open_documents.get_mut(&path).unwrap();
            document.source = "fn broken( {\n".to_string();
            document.dirty = true;
            document.reparse();
        }
        editor.refresh_project_problems(Some(&root));

        assert_eq!(editor.project_problem_count(), 1);
        assert_eq!(editor.project_problems[0].logical_path, "scripts/A/one.aeo");

        {
            let document = editor.open_documents.get_mut(&path).unwrap();
            document.source = "fn fixed() {}\n".to_string();
            document.reparse();
        }
        editor.refresh_project_problems(Some(&root));
        assert_eq!(editor.project_problem_count(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_filenames_remain_path_unambiguous_and_navigation_uses_exact_index() {
        let root = root("duplicate");
        fs::create_dir_all(root.join("scripts/B")).unwrap();
        let first = root.join("scripts/A/same.aeo");
        let second = root.join("scripts/B/same.aeo");
        fs::write(&first, "fn a( {\n").unwrap();
        fs::write(&second, "fn b( {\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));

        assert_eq!(editor.project_problem_count(), 2);
        let second_index = editor
            .project_problems
            .iter()
            .position(|p| p.logical_path == "scripts/B/same.aeo")
            .unwrap();
        editor.open_project_problem(second_index).unwrap();

        assert_eq!(editor.active_document.as_ref(), Some(&second));
        assert_eq!(
            editor.cursor_request.as_ref().map(|(path, _)| path),
            Some(&second)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clean_open_buffer_is_not_duplicated_with_disk_entry() {
        let root = root("clean_open");
        let path = root.join("scripts/A/one.aeo");
        fs::write(&path, "fn broken( {\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        editor.open_file(path);
        editor.refresh_project_problems(Some(&root));

        assert_eq!(editor.project_problem_count(), 1);
        assert_eq!(editor.project_problems[0].logical_path, "scripts/A/one.aeo");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rename_and_delete_refresh_problem_inventory_without_stale_entries() {
        let root = root("rename_delete");
        let old = root.join("scripts/A/old.aeo");
        fs::write(&old, "fn broken( {\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        assert!(
            editor
                .project_problems
                .iter()
                .any(|problem| problem.logical_path == "scripts/A/old.aeo")
        );

        let (_, renamed_logical) = editor
            .rename_script(&root, old.clone(), "renamed")
            .expect("rename should succeed");
        assert_eq!(renamed_logical, "scripts/A/renamed.aeo");
        assert!(
            editor
                .project_problems
                .iter()
                .all(|problem| problem.logical_path != "scripts/A/old.aeo")
        );
        assert!(
            editor
                .project_problems
                .iter()
                .any(|problem| problem.logical_path == "scripts/A/renamed.aeo")
        );

        let renamed = root.join("scripts/A/renamed.aeo");
        assert!(editor.delete_script(&Some(root.clone()), renamed));
        assert!(
            editor
                .project_problems
                .iter()
                .all(|problem| problem.logical_path != "scripts/A/renamed.aeo")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn problem_navigation_requests_exact_parser_byte_offset() {
        let root = root("offset");
        let path = root.join("scripts/A/offset.aeo");
        let source = "fn good() {}\nfn broken( {\n";
        fs::write(&path, source).unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        assert_eq!(editor.project_problem_count(), 1);

        let expected = editor.project_problems[0].byte_offset;
        editor.open_project_problem(0).unwrap();

        assert_eq!(editor.cursor_request, Some((path.clone(), expected)));
        assert!(expected >= source.find("broken").unwrap());

        let _ = fs::remove_dir_all(root);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:25 -05:00
    #[test]
    fn project_problems_distinguish_syntax_and_semantic_stages() {
        let root = root("kinds");
        fs::write(
            root.join("scripts/A/semantic.aeo"),
            "fn test(value, value) {}\n",
        )
        .unwrap();
        fs::write(root.join("playerscripts/B/parser.aeo"), "fn broken( {\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));

        assert!(editor.project_problems.iter().any(|problem| {
            problem.kind == ProjectProblemKind::Semantic
                && problem.logical_path == "scripts/A/semantic.aeo"
        }));
        assert!(editor.project_problems.iter().any(|problem| {
            problem.kind == ProjectProblemKind::Parser
                && problem.logical_path == "playerscripts/B/parser.aeo"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsaved_import_change_uses_project_index_for_semantic_resolution() {
        let root = root("semantic_import");
        let target = root.join("scripts/A/health.aeo");
        let source_path = root.join("scripts/A/main.aeo");
        fs::write(&target, "fn value() { return 1 }\n").unwrap();
        fs::write(&source_path, "import scripts.A.health.aeo\nfn main() {}\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        assert!(!editor.project_problems.iter().any(|problem| {
            problem.kind == ProjectProblemKind::Semantic
                && problem.logical_path == "scripts/A/main.aeo"
        }));

        editor.open_file(source_path.clone());
        {
            let document = editor.open_documents.get_mut(&source_path).unwrap();
            document.source = "import scripts.A.missing.aeo\nfn main() {}\n".to_string();
            document.dirty = true;
            document.reparse();
        }
        editor.refresh_project_problems(Some(&root));

        assert!(editor.project_problems.iter().any(|problem| {
            problem.kind == ProjectProblemKind::Semantic
                && problem.logical_path == "scripts/A/main.aeo"
                && problem.message.contains("does not resolve")
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_problem_inventory_never_includes_outside_files() {
        let root = root("safety");
        fs::write(root.join("outside.aeo"), "fn bad( {\n").unwrap();
        fs::write(root.join("scripts/A/inside.aeo"), "fn good() {}\n").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));

        assert!(editor.project_problems.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
