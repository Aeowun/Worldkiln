//! Worldkiln 0.8.5 | WO 0.8.7-59 | 2026-10-09
//! Read-only discovery of literal Save State keys in AeoScript.
//! This is not a script evaluator. Dynamic keys remain runtime-only.

use crate::engine::save_state::SaveValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(super) struct ScriptSaveField {
    pub key: String,
    pub references: usize,
    pub writes: usize,
    pub first_source: PathBuf,
    pub suggested: Option<SaveValue>,
}

pub(super) fn discover_script_save_fields(project: &Path) -> Vec<ScriptSaveField> {
    let mut found: BTreeMap<String, ScriptSaveField> = BTreeMap::new();
    let mut pending = vec![project.join("scripts"), project.join("playerscripts")];
    let mut examined = 0;
    // Bounded to avoid traversing imported media, engine asset packs, or
    // creating editor-frame stalls on giant projects. Only called on load.
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if path
                    .strip_prefix(project)
                    .map(|p| p.components().count() <= 7)
                    .unwrap_or(false)
                {
                    pending.push(path);
                }
                continue;
            }
            if !kind.is_file()
                || !path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("aeo"))
            {
                continue;
            }
            if examined >= 512 {
                return found.into_values().collect();
            }
            examined += 1;
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.len() > 512 * 1024 {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            for line in source.lines() {
                let uncommented = line.split("//").next().unwrap_or("");
                for (operation, key, rest) in parse_save_calls(uncommented) {
                    let field = found.entry(key.clone()).or_insert_with(|| ScriptSaveField {
                        key,
                        references: 0,
                        writes: 0,
                        first_source: path.strip_prefix(project).unwrap_or(&path).to_owned(),
                        suggested: None,
                    });
                    field.references += 1;
                    if operation == "set" {
                        field.writes += 1;
                        if field.suggested.is_none() {
                            field.suggested = infer_constant_value(rest);
                        }
                    }
                }
            }
        }
    }
    found.into_values().collect()
}

/// Recognize only literal quoted keys in calls like save.set("key", ...).
/// This intentionally does not guess values for computed key expressions.
fn parse_save_calls(line: &str) -> Vec<(&str, String, &str)> {
    let mut result = Vec::new();
    for (offset, _) in line.match_indices("save.") {
        let tail = &line[offset + 5..];
        let Some((name, rest)) = tail.split_once('(') else {
            continue;
        };
        let name = name.trim();
        if !["set", "get", "has", "remove"].contains(&name) {
            continue;
        }
        let rest = rest.trim_start();
        if !rest.starts_with('"') {
            continue;
        }
        let mut end = None;
        let mut escaped = false;
        for (i, ch) in rest[1..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                end = Some(i + 2);
                break;
            }
        }
        let Some(end) = end else {
            continue;
        };
        let Ok(key) = serde_json::from_str::<String>(&rest[..end]) else {
            continue;
        };
        if key.is_empty() || key.len() > 256 {
            continue;
        }
        let rest = &rest[end..];
        if !(rest.trim_start().starts_with(',') || rest.trim_start().starts_with(')')) {
            continue;
        }
        result.push((name, key, rest));
    }
    result
}

fn infer_constant_value(after_key: &str) -> Option<SaveValue> {
    let arg = after_key.trim_start().strip_prefix(',')?.trim_start();
    if arg.starts_with("true") && arg[4..].starts_with(|c: char| !c.is_alphanumeric() && c != '_') {
        return Some(SaveValue::Bool(true));
    }
    if arg.starts_with("false") && arg[5..].starts_with(|c: char| !c.is_alphanumeric() && c != '_')
    {
        return Some(SaveValue::Bool(false));
    }
    if arg.starts_with('"') {
        if let Some(end) = arg[1..].find('"') {
            return serde_json::from_str::<String>(&arg[..end + 2])
                .ok()
                .map(SaveValue::String);
        }
    }
    let end = arg
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(arg.len());
    if end > 0 {
        let text = &arg[..end];
        if let Ok(value) = text.parse::<f64>() {
            if value.is_finite() {
                return Some(SaveValue::Number(value));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_save_keys_are_discovered_not_dynamic_expressions() {
        let source = r#"
            // save.set("commented", true)
            save.set("marooned_campfire_built", true)
            save.set("marooned_stage", stage)
            save.has("marooned_stage")
            save.get(variable_key)
            save.set("player.name", "Ada")
            save.set("score", 2.5)
        "#;
        let mut found = BTreeMap::new();
        for line in source.lines() {
            for (op, key, rest) in parse_save_calls(line.split("//").next().unwrap_or("")) {
                let value = if op == "set" {
                    infer_constant_value(rest)
                } else {
                    None
                };
                found.insert(key, value);
            }
        }
        assert_eq!(found.len(), 4);
        assert_eq!(
            found["marooned_campfire_built"],
            Some(SaveValue::Bool(true))
        );
        assert_eq!(found["marooned_stage"], None);
        assert_eq!(found["player.name"], Some(SaveValue::String("Ada".into())));
        assert_eq!(found["score"], Some(SaveValue::Number(2.5)));
    }
    #[test]
    fn scanner_reads_scripts_and_nested_player_scripts_not_assets() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_script_keys_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("playerscripts/nested")).unwrap();
        fs::create_dir_all(root.join(".assets")).unwrap();
        fs::write(
            root.join("scripts/story.aeo"),
            r#"save.set("quest.stage", 2)"#,
        )
        .unwrap();
        fs::write(
            root.join("playerscripts/nested/actor.aeo"),
            r#"if save.has("quest.stage") {save.get("quest.stage")}"#,
        )
        .unwrap();
        fs::write(root.join(".assets/ignore.aeo"), r#"save.set("unused", 1)"#).unwrap();
        let found = discover_script_save_fields(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "quest.stage");
        assert_eq!(found[0].references, 3);
        assert_eq!(found[0].suggested, Some(SaveValue::Number(2.0)));
        fs::remove_dir_all(root).ok();
    }
}
