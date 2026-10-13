// Worldkiln | WO 0.8.7-65 | 2026-10-09
// Bounded retrieval over a provenance-bearing snapshot of the live AeoScript documentation.
// Generated with tools/audit_aeowun_docs.py and tools/generate_ai_knowledge_index.py.
use serde_json::Value;

const INDEX: &str = include_str!("ai_knowledge_index.json");
const SOURCE_FACTS: &str = include_str!("ai_source_facts.json");
// Worldkiln 0.8.5 | WO 0.8.7-81 | 2026-10-10 01:02 -05:00
const EXAMPLES: &str = include_str!("ai_examples.json");
// Worldkiln 0.8.5 | WO 0.8.7-83 | 2026-10-10 01:08 -05:00
// Build-time source fingerprint gate excludes examples made stale by engine edits.
const ACTIVE_EXAMPLES: &str = include_str!(concat!(env!("OUT_DIR"), "/ai_examples_active.json"));
// Worldkiln 0.8.5 | WO 0.8.7-83 | 2026-10-10 01:08 -05:00
fn passes_current_parser(entry: &Value) -> bool {
    let context = match entry["context"].as_str() {
        Some("global") => super::ai_script::ScriptKind::Global,
        Some("actor_module") => super::ai_script::ScriptKind::Attached,
        Some("cell_legacy") => super::ai_script::ScriptKind::Cell,
        _ => return false,
    };
    entry["code"]
        .as_str()
        .is_some_and(|source| super::ai_script::validate_for_context(source, context).is_ok())
}

fn example_matches(query: &str, max: usize) -> String {
    let Ok(index) = serde_json::from_str::<Value>(ACTIVE_EXAMPLES) else {
        return String::new();
    };
    let keys = words(&normalized_query(query));
    let mut ranked = index["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            // Re-run current engine parser and context checks every time an example
            // is retrieved. Syntax failures can never be fed back into the AI.
            if !passes_current_parser(entry) {
                return None;
            }
            let metadata = format!(
                "{} {} {} {}",
                entry["title"].as_str().unwrap_or(""),
                entry["id"].as_str().unwrap_or(""),
                entry["category"].as_str().unwrap_or(""),
                entry["context"].as_str().unwrap_or("")
            )
            .to_ascii_lowercase();
            let score = keys
                .iter()
                .filter(|key| metadata.contains(key.as_str()))
                .count();
            (score > 0).then_some((score, entry))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::new();
    for (_, entry) in ranked.into_iter().take(max) {
        out.push_str(&format!("\nSOURCE-DERIVED EXAMPLE: {} [{}]; context={}\nSOURCE TEST: {} :: {}\nCHECKED: parser/context only, not Play mode\nSOURCE:\n{}\n",
            entry["title"].as_str().unwrap_or(""), entry["id"].as_str().unwrap_or(""),
            entry["context"].as_str().unwrap_or(""), entry["source_file"].as_str().unwrap_or(""),
            entry["source_test"].as_str().unwrap_or(""), entry["code"].as_str().unwrap_or("")));
    }
    out
}
const DEFAULT_LIMIT: usize = 4;
const MAX_QUERY: usize = 300;
const MAX_RESULTS: usize = 8;

fn document_index() -> Result<Value, String> {
    serde_json::from_str(INDEX).map_err(|e| format!("AeoScript knowledge index is invalid: {e}"))
}
fn words(s: &str) -> Vec<String> {
    let mut output = Vec::new();
    for word in s
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if word.len() < 3
            || matches!(
                word,
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "from"
                    | "this"
                    | "that"
                    | "how"
                    | "help"
                    | "make"
                    | "need"
                    | "please"
                    | "want"
                    | "give"
                    | "main"
                    | "game"
            )
        {
            continue;
        }
        if !output.iter().any(|w| w == word) {
            output.push(word.to_owned());
        }
    }
    output
}
fn normalized_query(query: &str) -> String {
    let mut q = query.to_ascii_lowercase();
    // Feature -> API synonyms. This is retrieval vocabulary, not a claim that APIs exist.
    if q.contains("menu") || q.contains("hud") || q.contains("screen") {
        q.push_str(" ui control panel button text on_click anchor position size");
    }
    if q.contains("inventory") || q.contains("quest") {
        q.push_str(" basket maps events save script functions");
    }
    if q.contains("attach") || q.contains("actor") || q.contains("parent") {
        q.push_str(" modules bindings actor self.parent");
    }
    if q.contains("touch") || q.contains("collision") || q.contains("pickup") {
        q.push_str(" cells actor events physics");
    }
    if q.contains("start") || q.contains("startup") || q.contains("run") {
        q.push_str(" getting-started script global functions");
    }
    q
}
fn score(entry: &Value, query: &str, keys: &[String]) -> i64 {
    let title = entry["title"].as_str().unwrap_or("").to_ascii_lowercase();
    let id = entry["id"].as_str().unwrap_or("").to_ascii_lowercase();
    let body = entry["body"].as_str().unwrap_or("").to_ascii_lowercase();
    let examples = entry["examples"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let mut score = 0;
    for key in keys {
        if title.contains(key) {
            score += 25;
        }
        if id.contains(key) {
            score += 18;
        }
        if body.contains(key) {
            score += 4;
        }
        if examples.contains(key) {
            score += 7;
        }
    }
    if query.contains("menu") && id.ends_with("ui") {
        score += 55;
    }
    if query.contains("parent") && (id.contains("module") || id.contains("binding")) {
        score += 40;
    }
    if query.contains("startup") && id.contains("getting-started") {
        score += 55;
    }
    score
}

fn verified_source_facts(query: &str) -> String {
    let Ok(data) = serde_json::from_str::<Value>(SOURCE_FACTS) else {
        return "Source-verified knowledge registry is unavailable.\n".into();
    };
    let Some(facts) = data["facts"].as_array() else {
        return String::new();
    };
    let expanded = normalized_query(query);
    let keys = words(&expanded);
    let mut hits = facts
        .iter()
        .filter_map(|fact| {
            let title = fact["topic"].as_str().unwrap_or("").to_ascii_lowercase();
            let content = fact["fact"].as_str().unwrap_or("").to_ascii_lowercase();
            let mut score = 0;
            for key in &keys {
                if title.contains(key) {
                    score += 10;
                }
                if content.contains(key) {
                    score += 3;
                }
            }
            if query.to_ascii_lowercase().contains("menu")
                && fact["id"].as_str().unwrap_or("").contains("ui")
            {
                score += 22;
            }
            if query.to_ascii_lowercase().contains("parent")
                && fact["id"].as_str().unwrap_or("").contains("module")
            {
                score += 22;
            }
            (score > 0).then_some((score, fact))
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut output = String::from(
        "VERIFIED ENGINE SOURCE FACTS (source code is authoritative where docs differ):\n",
    );
    for (_, item) in hits.into_iter().take(5) {
        output.push_str("\n- ");
        output.push_str(item["fact"].as_str().unwrap_or(""));
        output.push_str("\n  SOURCE: ");
        if let Some(evidence) = item["evidence"].as_array() {
            output.push_str(
                &evidence
                    .iter()
                    .filter_map(Value::as_str)
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        output.push('\n');
    }
    output
}
pub fn search(query: &str, count: usize) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_QUERY {
        return "ERROR: Search query must contain 1-300 characters.".into();
    }
    let Ok(index) = document_index() else {
        return "ERROR: Documentation knowledge index not available.".into();
    };
    let Some(entries) = index["entries"].as_array() else {
        return "ERROR: Documentation index has no entries.".into();
    };
    let expanded = normalized_query(trimmed);
    let keys = words(&expanded);
    let mut ranked = entries
        .iter()
        .filter_map(|entry| {
            let relevance = score(entry, trimmed, &keys);
            (relevance > 0).then_some((relevance, entry))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1["url"].as_str().cmp(&b.1["url"].as_str()))
    });
    // Engine-source-derived examples precede unverified documentation excerpts.
    let mut out = verified_source_facts(trimmed);
    out.push_str(&example_matches(trimmed, 3));
    out.push_str("\nAeoScript documentation matches (live snapshot, not all APIs verified):\n");
    let mut shown = 0;
    for (_, entry) in ranked.into_iter().take(count.clamp(1, MAX_RESULTS)) {
        shown += 1;
        let title = entry["title"].as_str().unwrap_or("Unknown");
        let url = entry["url"].as_str().unwrap_or("");
        let body = entry["body"].as_str().unwrap_or("");
        out.push_str(&format!("\n## {title}\nSOURCE: {url}\n"));
        out.push_str(&body.chars().take(1850).collect::<String>());
        if let Some(examples) = entry["examples"].as_array() {
            for ex in examples.iter().filter_map(Value::as_str).take(2) {
                out.push_str("\nEXAMPLE:\n");
                out.push_str(&ex.chars().take(820).collect::<String>());
            }
        }
        if let Some(refs) = entry["source_candidates"].as_array() {
            out.push_str("\nRELEVANT ENGINE SOURCE TO VERIFY: ");
            out.push_str(
                &refs
                    .iter()
                    .filter_map(Value::as_str)
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        out.push('\n');
    }
    if shown == 0 {
        out.push_str(
            "\nNo matching documents. Don't invent an API; suggest a more specific query.",
        );
    }
    out.chars().take(16_000).collect()
}
pub fn read(id: &str) -> String {
    if id.is_empty() || id.len() > 150 {
        return "ERROR: Invalid documentation ID.".into();
    }
    let Ok(index) = document_index() else {
        return "ERROR: No knowledge index.".into();
    };
    let Some(entries) = index["entries"].as_array() else {
        return "ERROR: Empty knowledge index.".into();
    };
    let Some(item) = entries
        .iter()
        .find(|e| e["id"].as_str() == Some(id) || e["url"].as_str() == Some(id))
    else {
        return "ERROR: Unknown documentation page ID. Search the docs first.".into();
    };
    let title = item["title"].as_str().unwrap_or("");
    let url = item["url"].as_str().unwrap_or("");
    let body = item["body"].as_str().unwrap_or("");
    let mut out = format!("DOCUMENT: {title}\nSOURCE: {url}\n\n{body}\n");
    for ex in item["examples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(12)
    {
        out.push_str("\nEXAMPLE:\n");
        out.push_str(ex);
    }
    out.chars().take(21_000).collect()
}
pub fn prompt_context(query: &str) -> String {
    search(query, DEFAULT_LIMIT)
}
#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-81 | 2026-10-10 01:02 -05:00
    #[test]
    fn source_examples_parse_and_match_declared_context() {
        use super::super::ai_script::{ScriptKind, validate_for_context};
        let index: Value = serde_json::from_str(EXAMPLES).unwrap();
        let entries = index["entries"].as_array().unwrap();
        assert!(entries.len() >= 6);
        for example in entries {
            let code = example["code"].as_str().unwrap();
            let context = match example["context"].as_str().unwrap() {
                "global" => ScriptKind::Global,
                "actor_module" => ScriptKind::Attached,
                "cell_legacy" => ScriptKind::Cell,
                other => panic!("Unknown example context: {other}"),
            };
            validate_for_context(code, context)
                .unwrap_or_else(|error| panic!("Example {} failed: {error}", example["id"]));
            assert!(example["source_file"].as_str().unwrap().starts_with("src/"));
            assert!(!example["source_test"].as_str().unwrap().is_empty());
        }
        // Build-time fingerprint gating can temporarily quarantine all examples
        // while a source change is being revalidated and certificates refreshed.
        let active: Value = serde_json::from_str(ACTIVE_EXAMPLES).unwrap();
        if active["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
        {
            assert!(
                example_matches("actor module", 3).contains("actors.module.touch")
                    || !active["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e["id"] == "actors.module.touch")
            );
        }
    }
    // Worldkiln 0.8.5 | WO 0.8.7-83 | 2026-10-10 01:08 -05:00
    #[test]
    fn source_changes_quarantine_stale_examples_until_revalidated() {
        // Build script produces a filtered, source-fingerprinted index. A changed
        // source dependency must never silently retain a trusted example.
        let raw: Value = serde_json::from_str(EXAMPLES).unwrap();
        let active: Value = serde_json::from_str(ACTIVE_EXAMPLES).unwrap();
        let source_ids = raw["entries"].as_array().unwrap();
        let included = active["entries"].as_array().unwrap();
        assert_eq!(
            source_ids.len(),
            included.len(),
            "Source-dependent AeoScript examples quarantined; run python tools/maintain_aeoscript_examples.py refresh after verification"
        );
        for entry in included {
            assert!(
                passes_current_parser(entry),
                "Active example failed parsing: {}",
                entry["id"]
            );
        }
        let invalid = serde_json::json!({"context":"global","code":"fn broken( {"});
        assert!(!passes_current_parser(&invalid));
        let wrong_context = serde_json::json!({"context":"actor_module",
            "code":"entity Enemy { fn on_ready() { print(1) } }"});
        assert!(!passes_current_parser(&wrong_context));
    }

    #[test]
    fn main_menu_discovers_ui_documentation() {
        let results = search("Help me design a main menu", 4);
        assert!(
            results.contains("aeowun.com/docs/reference/ui/")
                || results.contains("aeowun.com/aeoscript/ui/"),
            "{results}"
        );
        assert!(results.contains("button") || results.contains("Button"));
    }
    #[test]
    fn parent_query_finds_actual_module_guidance() {
        let results = search("self.parent for an attached Actor module", 4);
        assert!(
            results.contains("modules/") || results.contains("reference/project"),
            "{results}"
        );
    }
    #[test]
    fn bounded_read_and_invalid_ids() {
        assert!(read("reference/ui").contains("SOURCE: https://aeowun.com/docs/reference/ui/"));
        assert!(read("../secret").starts_with("ERROR:"));
    }
}

#[cfg(test)]
mod syntax_corpus_audit {
    use super::*;
    use crate::scripting::{lexer::Lexer, parser::Parser};
    #[test]
    fn audit_live_docs_examples_with_real_aeoscript_parser() {
        let index = document_index().expect("knowledge index");
        let entries = index["entries"].as_array().expect("entries");
        let mut count = 0;
        let mut passes = 0;
        let mut failures = Vec::new();
        for entry in entries {
            if let Some(examples) = entry["examples"].as_array() {
                for example in examples.iter().filter_map(Value::as_str) {
                    count += 1;
                    let result = Lexer::new(example)
                        .tokenize()
                        .map_err(|e| format!("Lexer {}:{} {}", e.line, e.column, e.message))
                        .and_then(|tokens| {
                            Parser::new(tokens).parse().map(|_| ()).map_err(|e| {
                                format!("Parser {}:{} {}", e.line, e.column, e.message)
                            })
                        });
                    if result.is_ok() {
                        passes += 1;
                    } else {
                        failures.push(format!(
                            "{} => {} | {}",
                            entry["url"].as_str().unwrap_or(""),
                            example.lines().next().unwrap_or(""),
                            result.unwrap_err()
                        ));
                    }
                }
            }
        }
        println!(
            "AEOSCRIPT_DOCS_AUDIT: {} source pages, {} code examples; {} parse in current engine, {} do not (fragments/invalid syntax may count as failures).",
            entries.len(),
            count,
            passes,
            count - passes
        );
        for fail in failures.iter().take(35) {
            println!("DOC_PARSER_FLAG: {fail}");
        }
        assert!(count >= 260, "Expected all 275 documentation examples");
        assert!(
            passes > 150,
            "Unexpectedly few valid documentation examples"
        );
    }
}
