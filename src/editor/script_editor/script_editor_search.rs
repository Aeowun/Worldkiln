// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{ScriptDocument, ScriptEditor};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchScope {
    #[default]
    Script,
    Project,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub source_hash: u64,
}

fn source_hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub(crate) fn find_matches(
    source: &str,
    query: &str,
    match_case: bool,
    whole_word: bool,
) -> Vec<(usize, usize, usize, usize, String)> {
    if query.is_empty() || query.contains('\n') || query.contains('\r') {
        return Vec::new();
    }
    let query_len = query.chars().count();
    let query_lower = (!match_case).then(|| query.to_lowercase());
    let mut found = Vec::new();
    let mut line = 1;
    let mut line_start = 0;
    let mut next_allowed = 0;
    for (start, character) in source.char_indices() {
        if character == '\n' {
            line += 1;
            line_start = start + 1;
            continue;
        }
        if start < next_allowed {
            continue;
        }
        let end = if match_case {
            source[start..]
                .starts_with(query)
                .then_some(start + query.len())
        } else {
            let candidate: String = source[start..].chars().take(query_len).collect();
            (candidate.chars().count() == query_len
                && candidate.to_lowercase() == *query_lower.as_ref().unwrap())
            .then_some(start + candidate.len())
        };
        let Some(end) = end else { continue };
        if whole_word {
            let previous = source[..start].chars().next_back();
            let following = source[end..].chars().next();
            if previous.is_some_and(word_char) || following.is_some_and(word_char) {
                continue;
            }
        }
        let text_end = source[end..]
            .find('\n')
            .map(|offset| end + offset)
            .unwrap_or(source.len());
        let text = source[line_start..text_end]
            .trim_end_matches('\r')
            .to_string();
        let column = source[line_start..start].chars().count() + 1;
        found.push((start, end, line, column, text));
        next_allowed = end;
    }
    found
}

fn is_project_script(root: &Path, path: &Path) -> bool {
    if !path.extension().is_some_and(|extension| extension == "aeo") {
        return false;
    }
    if !path.starts_with(root) {
        return false;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if !matches!(relative.components().next(), Some(std::path::Component::Normal(name)) if name == "scripts" || name == "playerscripts")
    {
        return false;
    }
    match (root.canonicalize(), path.canonicalize()) {
        (Ok(project), Ok(actual)) => actual.starts_with(project),
        _ => false,
    }
}

impl ScriptEditor {
    pub fn open_find(&mut self, replace: bool) {
        self.show_search = true;
        self.search_replace = replace;
        self.search_scope = SearchScope::Script;
        self.search_focus_requested = true;
        self.search_active_index = None;
        self.perform_search();
    }

    pub fn perform_search(&mut self) {
        let previous = self
            .search_active_index
            .and_then(|index| self.search_results.get(index))
            .map(|result| (result.path.clone(), result.start));
        self.search_results.clear();
        self.search_active_index = None;

        if self.search_query.is_empty() {
            return;
        }
        let Some(index) = self.project_script_index.as_ref() else {
            return;
        };
        let root = &index.project_root;
        let paths = match self.search_scope {
            SearchScope::Script => self.active_document.clone().into_iter().collect::<Vec<_>>(),
            SearchScope::Project => self.indexed_script_paths(),
        };
        for path in paths {
            if !is_project_script(root, &path) {
                continue;
            }
            let source = if let Some(document) = self.open_documents.get(&path) {
                document.source.clone()
            } else if let Ok(source) = std::fs::read_to_string(&path) {
                source
            } else {
                continue;
            };
            let source_hash = source_hash(&source);
            for (start, end, line, column, text) in find_matches(
                &source,
                &self.search_query,
                self.search_match_case,
                self.search_whole_word,
            ) {
                self.search_results.push(SearchResult {
                    path: path.clone(),
                    line,
                    column,
                    start,
                    end,
                    text,
                    source_hash,
                });
            }
        }
        self.search_active_index = previous.and_then(|(path, start)| {
            self.search_results
                .iter()
                .position(|result| result.path == path && result.start == start)
        });
    }

    pub fn navigate_search(&mut self, next: bool) {
        if self.search_results.is_empty() {
            self.search_active_index = None;
            return;
        }
        let last = self.search_results.len() - 1;
        let index = match self.search_active_index {
            Some(index) if next => (index + 1) % self.search_results.len(),
            Some(0) if !next => last,
            Some(index) => index - 1,
            None if next => 0,
            None => last,
        };
        self.select_search_result(index);
    }

    pub fn select_search_result(&mut self, index: usize) {
        let Some(result) = self.search_results.get(index).cloned() else {
            return;
        };
        self.search_active_index = Some(index);
        self.open_file(result.path.clone());
        self.search_selection_request = Some((result.path, result.start, result.end));
    }

    pub fn replace_selected(&mut self) -> usize {
        if self.search_active_index.is_none() && !self.search_results.is_empty() {
            self.select_search_result(0);
        }
        let Some(result) = self
            .search_active_index
            .and_then(|index| self.search_results.get(index))
            .cloned()
        else {
            return 0;
        };
        let Some(document) = self.open_documents.get_mut(&result.path) else {
            return 0;
        };
        if document
            .source
            .get(result.start..result.end)
            .is_none_or(|matched| {
                find_matches(matched, &self.search_query, self.search_match_case, false)
                    .first()
                    .is_none_or(|match_| match_.0 != 0 || match_.1 != matched.len())
            })
        {
            self.perform_search();
            return 0;
        }
        if source_hash(&document.source) != result.source_hash {
            self.perform_search();
            return 0;
        }
        let before = document.source.clone();
        document
            .source
            .replace_range(result.start..result.end, &self.search_replacement);
        document.dirty = document.source != document.original_source;
        document.reparse();
        self.search_undo_pending
            .push((result.path.clone(), before, document.source.clone()));
        self.perform_search();
        if !self.search_results.is_empty() {
            let next = self
                .search_results
                .iter()
                .position(|match_| {
                    match_.path > result.path
                        || (match_.path == result.path && match_.start >= result.start)
                })
                .unwrap_or(0);
            self.select_search_result(next);
        }
        1
    }

    pub fn replace_all_matches(&mut self) -> usize {
        if self.search_query.is_empty() || self.search_results.is_empty() {
            return 0;
        }
        let Some(index) = self.project_script_index.as_ref() else {
            return 0;
        };
        let root = index.project_root.clone();
        let mut grouped: std::collections::BTreeMap<PathBuf, Vec<SearchResult>> =
            std::collections::BTreeMap::new();
        for result in &self.search_results {
            if is_project_script(&root, &result.path) {
                grouped
                    .entry(result.path.clone())
                    .or_default()
                    .push(result.clone());
            }
        }
        let mut changed = 0;
        for (path, matches) in grouped {
            if !self.open_documents.contains_key(&path) {
                if let Ok(source) = std::fs::read_to_string(&path) {
                    self.open_documents
                        .insert(path.clone(), ScriptDocument::new(path.clone(), source));
                }
            }
            let Some(document) = self.open_documents.get_mut(&path) else {
                continue;
            };
            let before = document.source.clone();
            let mut valid = matches
                .iter()
                .all(|result| result.source_hash == source_hash(&before));
            for result in &matches {
                let Some(matched) = before.get(result.start..result.end) else {
                    valid = false;
                    break;
                };
                if find_matches(matched, &self.search_query, self.search_match_case, false).len()
                    != 1
                {
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }
            for result in matches.iter().rev() {
                document
                    .source
                    .replace_range(result.start..result.end, &self.search_replacement);
            }
            document.dirty = document.source != document.original_source;
            document.reparse();
            self.search_undo_pending
                .push((path, before, document.source.clone()));
            changed += matches.len();
        }
        self.perform_search();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn unicode_and_whole_word_offsets() {
        let matches = find_matches("é network networked\nnetwork", "network", true, true);
        assert_eq!(
            matches.iter().map(|m| (m.2, m.3)).collect::<Vec<_>>(),
            vec![(1, 3), (2, 1)]
        );
        assert_eq!(
            &"é network networked\nnetwork"[matches[0].0..matches[0].1],
            "network"
        );
        assert_eq!(
            find_matches("NETwork network", "network", false, false).len(),
            2
        );
        assert_eq!(
            find_matches("NETwork network", "network", true, false).len(),
            1
        );
    }

    #[test]
    fn project_scope_ignores_non_scripts_and_uses_unsaved_buffers() {
        let root =
            std::env::temp_dir().join(format!("worldkiln_search_scope_{}", std::process::id()));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("playerscripts")).unwrap();
        fs::write(root.join("scripts/a.aeo"), "network").unwrap();
        fs::write(root.join("scripts/ignore.txt"), "network").unwrap();
        fs::write(root.join("playerscripts/b.aeo"), "network").unwrap();
        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        let first = root.join("scripts/a.aeo");
        editor.open_file(first.clone());
        editor.open_documents.get_mut(&first).unwrap().source =
            "updated network network".to_string();
        editor.search_scope = SearchScope::Project;
        editor.search_query = "network".into();
        editor.perform_search();
        assert_eq!(editor.search_results.len(), 3);
        assert!(editor
            .search_results
            .iter()
            .all(|result| result.path.extension().is_some_and(|e| e == "aeo")));
        editor.search_scope = SearchScope::Script;
        editor.perform_search();
        assert_eq!(editor.search_results.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_all_stays_unsaved_and_respects_scope() {
        let root =
            std::env::temp_dir().join(format!("worldkiln_search_replace_{}", std::process::id()));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("playerscripts")).unwrap();
        let path = root.join("scripts/a.aeo");
        let other = root.join("playerscripts/b.aeo");
        fs::write(&path, "let network = network").unwrap();
        fs::write(&other, "network").unwrap();
        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(root.clone()));
        editor.open_file(path.clone());
        editor.search_scope = SearchScope::Script;
        editor.search_query = "network".into();
        editor.search_replacement = "offline".into();
        editor.perform_search();
        assert_eq!(editor.replace_all_matches(), 2);
        assert_eq!(editor.open_documents[&path].source, "let offline = offline");
        assert!(editor.open_documents[&path].dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "let network = network");
        assert_eq!(fs::read_to_string(&other).unwrap(), "network");
        fs::remove_dir_all(root).unwrap();
    }
}
