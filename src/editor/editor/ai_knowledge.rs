// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde_json::Value;

const INDEX: &str = include_str!("ai_knowledge_index.json");
const SOURCE_FACTS: &str = include_str!("ai_source_facts.json");
const EXAMPLES: &str = include_str!("../../../.assets/assistant/ai_examples.json");
const BROWSE_CATALOG: &str = include_str!("ai_browse_catalog.json");
fn passes_current_parser(entry: &Value) -> bool {
    let context = match entry["context"].as_str() {
        Some("global") => super::ai_script::ScriptKind::Global,
        Some("actor_module") => super::ai_script::ScriptKind::Attached,
        Some("cell_legacy") => super::ai_script::ScriptKind::Cell,
        _ => return false,
    };
    entry["code"]
        .as_str()
        .is_some_and(|source| super::ai_agent::strict_check_for_context(source, context).is_ok())
}

fn example_matches(query: &str, max: usize) -> String {
    let Ok(index) = serde_json::from_str::<Value>(EXAMPLES) else {
        return String::new();
    };
    let keys = words(&normalized_query(query));
    let mut ranked = index["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
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

pub(super) fn current_api(query: &str) -> String {
    let query = query.trim();
    if query.len() < 2 || query.len() > MAX_QUERY {
        return "ERROR: Use a literal API name or topic of 2–300 bytes.".into();
    }
    let sources = [
        (
            "src/scripting/host/mod.rs",
            include_str!("../../scripting/host/mod.rs"),
        ),
        (
            "src/scripting/stdlib.rs",
            include_str!("../../scripting/stdlib.rs"),
        ),
        (
            "src/scripting/semantic.rs",
            include_str!("../../scripting/semantic.rs"),
        ),
        (
            "src/scripting/host/host_character.rs",
            include_str!("../../scripting/host/host_character.rs"),
        ),
        (
            "src/scripting/host/host_cells.rs",
            include_str!("../../scripting/host/host_cells.rs"),
        ),
        (
            "src/scripting/scene.rs",
            include_str!("../../scripting/scene.rs"),
        ),
    ];
    let mut out = format!(
        "CURRENT BUILD SOURCE — Worldkiln {}; commit {}; fingerprint {}. Rust implementation excerpts, not AeoScript syntax; never paste Rust into .aeo. Not runtime proof.\n",
        env!("CARGO_PKG_VERSION"),
        option_env!("WORLDKILN_SOURCE_COMMIT").unwrap_or("unavailable"),
        option_env!("WORLDKILN_SOURCE_FINGERPRINT").unwrap_or("unavailable")
    );
    let q = query.to_ascii_lowercase();
    let mut hits = 0;
    for (path, source) in sources {
        let lines: Vec<_> = source.lines().collect();
        let mut next = 0;
        for (i, line) in lines.iter().enumerate() {
            if i < next || !line.to_ascii_lowercase().contains(&q) {
                continue;
            }
            out.push_str(&format!("\n{path}:{}\n", i + 1));
            for line in &lines[i.saturating_sub(2)..(i + 9).min(lines.len())] {
                out.extend(line.chars().take(220));
                out.push('\n');
            }
            next = i + 9;
            hits += 1;
            if hits == 6 {
                return out;
            }
        }
    }
    if hits == 0 {
        out.push_str("No literal match in these sources. This is not proof an API is absent; use targeted reference tools and validate proposed source.\n");
    }
    out
}
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
    if q.contains("patrol") || q.contains("waypoint") || q.contains("reach") || q.contains("walk") {
        q.push_str(" navigation actor movement pathfinding");
    }
    if q.contains("roam") || q.contains("wander") || q.contains("villager") {
        q.push_str(" navigation character actor entity find_path navigate_to waypoints");
    }
    if q.contains("door") || q.contains("gate") || q.contains("switch") {
        q.push_str(" actor cell solid anchored collision_enabled events input");
    }
    if q.contains("turn") || q.contains("face") {
        q.push_str(" facing rotation actor player movement direction");
    }
    if q.contains("damage") || q.contains("heal") || q.contains("health") {
        q.push_str(" on.damage save game owned health state events");
    }
    if q.contains("semicolon") || q.contains("syntax") || q.contains("lexer") {
        q.push_str(" syntax statements grammar newline");
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
        "HISTORICAL SOURCE FACTS (manually checked 2026-10-09; may be stale). Use read_current_aeoscript_api and validate_aeoscript for this build; current source wins:\n",
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
    let mut out = verified_source_facts(trimmed);
    out.push_str(&example_matches(trimmed, 3));
    out.push_str("\nAeoScript documentation matches (historical snapshot; not all APIs verified for this build). Use current source and validation when docs disagree:\n");
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
    let mut out = format!(
        "HISTORICAL INDEXED DOCUMENT — advisory; not verified against this build.\nDOCUMENT: {title}\nSOURCE: {url}\n\n{body}\n"
    );
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
fn live_docs_get(client: &reqwest::blocking::Client, path: &str) -> Result<String, String> {
    if !(path.starts_with("/docs/")
        || path.starts_with("/aeoscript/")
        || path.starts_with("/worldkiln/"))
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
        || path.len() > 160
    {
        return Err("The documentation path is not permitted.".into());
    }
    let url = format!("https://aeowun.com{path}");
    let response = client
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .map_err(|_| "Aeowun documentation is unreachable.".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "Aeowun documentation returned HTTP {}.",
            response.status()
        ));
    }
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .is_some_and(|kind| kind.starts_with("text/html"))
    {
        return Err("Aeowun documentation returned a non-HTML response.".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(80_001)
        .read_to_end(&mut bytes)
        .map_err(|_| "Unable to read documentation response.".to_owned())?;
    if bytes.len() > 80_000 {
        return Err("Documentation response exceeded the size limit.".into());
    }
    String::from_utf8(bytes).map_err(|_| "Documentation response is not UTF-8.".into())
}

fn live_docs_visible(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut inside_tag = false;
    let mut suppressed = false;
    for ch in html.chars() {
        if ch == '<' {
            inside_tag = true;
            tag.clear();
        } else if inside_tag {
            if ch == '>' {
                let name = tag.trim().to_ascii_lowercase();
                if name.starts_with("script") || name.starts_with("style") {
                    suppressed = true;
                } else if name.starts_with("/script") || name.starts_with("/style") {
                    suppressed = false;
                } else if !suppressed
                    && (name.starts_with("/p")
                        || name.starts_with("/li")
                        || name.starts_with("/div")
                        || name.starts_with("/h")
                        || name.starts_with("/pre")
                        || name.starts_with("br")
                        || name.starts_with("/section"))
                {
                    out.push('\n');
                }
                inside_tag = false;
            } else if tag.len() < 200 {
                tag.push(ch);
            }
        } else if !suppressed {
            out.push(ch);
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn search_live_docs(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() || query.len() > 200 {
        return Err("Enter a documentation topic (1-200 characters).".into());
    }
    let keys = words(&normalized_query(query));
    if keys.is_empty() {
        return Err("Enter a more specific documentation topic.".into());
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize documentation lookup.".to_owned())?;
    let index = live_docs_get(&client, "/docs/")?;
    let mut remaining = index.as_str();
    let mut candidates = Vec::<(usize, String, String)>::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(start) = remaining.find("<a ") {
        remaining = &remaining[start + 3..];
        let Some(end_tag) = remaining.find('>') else {
            break;
        };
        let attrs = &remaining[..end_tag];
        let after_tag = &remaining[end_tag + 1..];
        let Some(end_anchor) = after_tag.find("</a>") else {
            break;
        };
        let label = live_docs_visible(&after_tag[..end_anchor]);
        remaining = &after_tag[end_anchor + 4..];
        let Some(href) = attrs
            .split("href=\"")
            .nth(1)
            .and_then(|a| a.split('"').next())
        else {
            continue;
        };
        if !(href.starts_with("/docs/")
            || href.starts_with("/aeoscript/")
            || href.starts_with("/worldkiln/"))
            || !href
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
            || !seen.insert(href.to_string())
        {
            continue;
        }
        let label_lower = label.to_ascii_lowercase();
        let path_lower = href.to_ascii_lowercase();
        let score = keys
            .iter()
            .map(|k| {
                usize::from(label_lower.contains(k)) * 6 + usize::from(path_lower.contains(k)) * 3
            })
            .sum::<usize>();
        if score > 0 {
            candidates.push((score, href.to_owned(), label));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if candidates.is_empty() {
        return Ok("No live Aeowun documentation pages matched the topic. Use search_aeoscript_docs or a narrower query; do not invent APIs.".into());
    }
    let mut out = String::from(
        "Live documentation matches (advisory; current engine APIs take precedence). Use read_live_aeowun_doc with PATH to open one useful page if needed.\n",
    );
    for (_, path, title) in candidates.into_iter().take(5) {
        out.push_str(&format!(
            "\nTITLE: {title}\nPATH: {path}\nSOURCE: https://aeowun.com{path}\n"
        ));
    }
    Ok(out)
}

pub fn read_live_doc(path: &str) -> Result<String, String> {
    if !(path.starts_with("/docs/")
        || path.starts_with("/aeoscript/")
        || path.starts_with("/worldkiln/"))
        || path.len() > 160
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
    {
        return Err("Select a valid PATH returned by search_live_aeowun_docs.".into());
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize documentation lookup.".to_owned())?;
    let index = live_docs_get(&client, "/docs/")?;
    if !index.contains(&format!("href=\"{path}\"")) {
        return Err("Page is not listed in the current Aeowun documentation index.".into());
    }
    let page = live_docs_get(&client, path)?;
    let main = page
        .find("<main")
        .and_then(|i| page[i..].find('>').map(|end| i + end + 1))
        .and_then(|start| {
            page[start..]
                .find("</main>")
                .map(|end| &page[start..start + end])
        })
        .ok_or("Documentation page has no readable content.")?;
    Ok(format!(
        "LIVE DOCUMENTATION (not verified engine APIs). SOURCE: https://aeowun.com{path}\n{}",
        live_docs_visible(main)
            .chars()
            .take(3_200)
            .collect::<String>()
    ))
}

pub fn browse_catalog(path: &str, query: &str) -> String {
    use crate::scripting::{
        diagnostic::DiagnosticSeverity, lexer::Lexer, parser::Parser,
        semantic::strict_builtin_api_diagnostics,
    };
    let index: Value = match serde_json::from_str(BROWSE_CATALOG) {
        Ok(index) => index,
        Err(_) => return "ERROR: AeoScript catalogue is unavailable.".into(),
    };
    let Some(items) = index["items"].as_array() else {
        return "ERROR: AeoScript catalogue has no entries.".into();
    };
    let path = if path.trim().is_empty() {
        "root"
    } else {
        path.trim()
    };
    if path.len() > 100
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
    {
        return "ERROR: Invalid catalogue path. Browse root for options.".into();
    }
    if path == "catalog/search" {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() || query.len() > 120 {
            return "Enter a brief query (1–120 characters), or browse catalog for categories."
                .into();
        }
        let terms: Vec<_> = query
            .split_whitespace()
            .filter(|part| part.len() >= 2)
            .collect();
        let mut ranked = items
            .iter()
            .filter_map(|item| {
                let route = item["path"].as_str()?;
                if !route.starts_with("catalog/") {
                    return None;
                }
                let hay = format!(
                    "{} {} {}",
                    item["title"].as_str().unwrap_or(""),
                    item["summary"].as_str().unwrap_or(""),
                    route
                )
                .to_ascii_lowercase();
                let score = terms.iter().filter(|term| hay.contains(**term)).count();
                (score > 0).then_some((score, route, item["title"].as_str().unwrap_or("")))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let mut result =
            String::from("CATALOGUE MATCHES — choose one PATH to open; no article text loaded:\n");
        for (_, route, title) in ranked.iter().take(8) {
            result.push_str(&format!("PATH: {route} — {title}\n"));
        }
        if ranked.is_empty() {
            result.push_str("No match. Browse catalog for categories.\n");
        }
        return result;
    }
    let Some(entry) = items.iter().find(|item| item["path"] == path) else {
        return "ERROR: Unknown path. Call browse_aeoscript_knowledge with path=root for options."
            .into();
    };
    let title = entry["title"].as_str().unwrap_or("Untitled");
    let overview = entry["summary"].as_str().unwrap_or("");
    let mut result = format!("KNOWLEDGE: {title}\nPATH: {path}\nOVERVIEW: {overview}\n");
    if let Some(notes) = entry["notes"].as_str() {
        result.push_str(&format!("NOTES: {notes}\n"));
    }
    if let Some(source) = entry["source"].as_str() {
        result.push_str(&format!("REFERENCE: {source}\n"));
    }
    let prefix = if path == "root" {
        String::new()
    } else {
        format!("{path}/")
    };
    let children = items
        .iter()
        .filter(|item| {
            let Some(other) = item["path"].as_str() else {
                return false;
            };
            if path == "root" {
                return other == "web" || other == "catalog";
            }
            other
                .strip_prefix(&prefix)
                .is_some_and(|tail| !tail.is_empty() && !tail.contains('/'))
        })
        .collect::<Vec<_>>();
    if !children.is_empty() {
        result.push_str("OPTIONS — choose a single PATH to go deeper:\n");
        for child in &children {
            result.push_str(&format!(
                "PATH: {} — {}\n",
                child["path"].as_str().unwrap_or(""),
                child["title"].as_str().unwrap_or("")
            ));
        }
        if path == "catalog" {
            result.push_str("PATH: catalog/search — Search catalogue titles (pass a query)\n");
        }
    } else if let Some(code) = entry["code"].as_str() {
        let attached = entry["context"] == "actor_module";
        let checked = Lexer::new(code)
            .tokenize()
            .map_err(|err| err.message)
            .and_then(|tokens| Parser::new(tokens).parse().map_err(|err| err.message))
            .and_then(|program| {
                let errors = strict_builtin_api_diagnostics(&program, attached)
                    .into_iter()
                    .filter(|diag| diag.severity == DiagnosticSeverity::Error)
                    .map(|diag| diag.message)
                    .collect::<Vec<_>>();
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                }
            });
        if let Err(err) = checked {
            result.push_str(&format!(
                "EXAMPLE UNAVAILABLE: Does not pass the current AeoScript validator ({err}).\n"
            ));
        } else {
            result.push_str(&format!("CONTEXT: {}\nCHECKED: lexer/parser/static API validation; not Play-mode verification.\nAEOSCRIPT:\n```aeoscript\n{code}\n```\n",
                entry["context"].as_str().unwrap_or("global")));
        }
    }
    result
}

pub(super) fn automatic_context(turns: &[(String, String)]) -> String {
    let recent = turns
        .iter()
        .rev()
        .filter(|(role, _)| role == "user")
        .take(2)
        .map(|(_, text)| {
            text.rsplit("USER REQUEST:\n")
                .next()
                .unwrap_or(text)
                .chars()
                .take(1500)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let lower = recent.to_ascii_lowercase();
    let terms = words(&lower);
    let explicitly_scripting = [
        "script",
        "aeo",
        "code",
        "build",
        "create",
        "make",
        "fix",
        "implement",
        "error",
        "player",
        "dungeon",
        "gate",
        "interact",
        "syntax",
        "validation",
        "function",
        "module",
        "import",
        "api",
        "event",
        "controller",
    ]
    .iter()
    .any(|term| lower.contains(term));
    let action = [
        "roam", "wander", "walk", "move", "patrol", "follow", "spawn", "turn", "face", "open",
        "close", "switch", "press", "heal", "damage", "save", "load", "animate", "teleport",
        "handle",
    ];
    let subject = [
        "villager",
        "npc",
        "enemy",
        "actor",
        "character",
        "door",
        "switch",
        "camera",
        "input",
        "button",
        "health",
        "inventory",
        "physics",
        "object",
        "scene",
        "world",
        "waypoint",
    ];
    if !explicitly_scripting
        && !(action
            .iter()
            .any(|term| terms.iter().any(|word| word.contains(term)))
            && subject
                .iter()
                .any(|term| terms.iter().any(|word| word.contains(term))))
    {
        return String::new();
    }
    let mut query = lower;
    if ["switch", "gate", "door", "dungeon"]
        .iter()
        .any(|term| query.contains(term))
    {
        query.push_str(" actor cell event touch interaction");
    }
    if ["press", "key", "switch", "interact"]
        .iter()
        .any(|term| query.contains(term))
    {
        query.push_str(" input keypress");
    }
    let terms = words(&query)
        .into_iter()
        .filter(|word| {
            word.len() >= 3
                && ![
                    "the",
                    "and",
                    "for",
                    "with",
                    "that",
                    "this",
                    "from",
                    "have",
                    "please",
                    "script",
                    "aeoscript",
                ]
                .contains(&word.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let guide = super::ai_script::CANONICAL_LANGUAGE_DOC
        .split("<!-- PRIMARY-REFERENCE-APPENDICES:BEGIN -->")
        .next()
        .unwrap_or("");
    let mut blocks = Vec::new();
    for (index, body) in guide.split("\n## ").skip(1).enumerate() {
        if index == 0 {
            continue;
        }
        let parent = body.lines().next().unwrap_or("");
        let mut parts = body.split("\n### ");
        blocks.push(parts.next().unwrap_or("").to_owned());
        for part in parts {
            blocks.push(format!("{parent}\n### {part}"));
        }
    }
    let mut sections = blocks
        .into_iter()
        .enumerate()
        .filter_map(|(index, body)| {
            let title = body
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            let text = body.to_ascii_lowercase();
            let score = terms
                .iter()
                .map(|term| {
                    usize::from(text.contains(term.as_str()))
                        + 3 * usize::from(title.contains(term.as_str()))
                })
                .sum::<usize>();
            (score > 0).then_some((score, index, body))
        })
        .collect::<Vec<_>>();
    sections.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out = format!(
        "AUTOMATIC TASK REFERENCES — Worldkiln {}; source: {}.\nCanonical authoring references, not runtime proof. Validate proposed APIs against this build. Missing matches do not prove an API is absent.\n",
        env!("CARGO_PKG_VERSION"),
        super::ai_script::LANGUAGE_CONTRACT_PATH
    );
    const LIMIT: usize = 16_000;
    let mut append = |block: &str| {
        if out.len() + block.len() <= LIMIT {
            out.push_str(block);
        }
    };
    let examples = example_matches(&query.chars().take(300).collect::<String>(), 2);
    if examples.is_empty() {
        append("No matching source-certified example is currently available; do not invent one.\n");
    } else {
        append(&examples);
    }
    if let Some(symbol) = recent
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .find(|word| {
            word.len() >= 3 && word.len() <= 100 && (word.contains('_') || word.contains('.'))
        })
    {
        append(&current_api(symbol));
    }
    for (_, _, body) in sections.into_iter().take(6) {
        append(&format!("\n## {body}"));
    }
    out
}

pub fn prompt_context(query: &str) -> String {
    search(query, DEFAULT_LIMIT)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_browse_reveals_only_one_level_at_a_time() {
        let root = browse_catalog("root", "");
        assert!(root.contains("PATH: web") && root.contains("PATH: catalog"));
        assert!(!root.contains("PATH: catalog/ui/runtime"), "{root}");
        assert!(!root.contains("AEOSCRIPT:"), "{root}");
        let catalog = browse_catalog("catalog", "");
        assert!(catalog.contains("PATH: catalog/ui"));
        assert!(catalog.contains("PATH: catalog/nav"));
        assert!(catalog.contains("PATH: catalog/player"));
        assert!(!catalog.contains("PATH: catalog/ui/runtime"));
        assert!(!catalog.contains("AEOSCRIPT:"));
        let ui = browse_catalog("catalog/ui", "");
        assert!(ui.contains("Worldkiln offers editor-authored UI"));
        assert!(ui.contains("PATH: catalog/ui/runtime"));
        assert!(ui.contains("PATH: catalog/ui/authored"));
        assert!(!ui.contains("AEOSCRIPT:"));
        let sample = browse_catalog("catalog/ui/runtime", "");
        assert!(sample.contains("AEOSCRIPT:"));
        assert!(sample.contains("const panel = ui.new"));
        assert!(!sample.contains("PATH: catalog/nav"));
        let web = browse_catalog("web", "");
        assert!(web.contains("PATH: web/search"));
        assert!(!web.contains("AEOSCRIPT:"));
        let search = browse_catalog("catalog/search", "navigation");
        assert!(search.contains("PATH: catalog/nav"));
        assert!(!search.contains("AEOSCRIPT:"));
        assert!(browse_catalog("../private", "").starts_with("ERROR:"));
    }


    #[test]
    fn live_docs_content_is_plain_text_and_source_restricted() {
        let html = "<main><h1>Syntax</h1><p>Newlines &amp; blocks</p><script>LEAK_PRIVATE_DATA()</script><pre>const x = 1</pre></main>";
        let shown = live_docs_visible(html);
        assert!(shown.contains("Syntax"));
        assert!(shown.contains("Newlines & blocks"));
        assert!(shown.contains("const x = 1"));
        assert!(!shown.contains("LEAK_PRIVATE_DATA"));
        let client = reqwest::blocking::Client::new();
        for invalid in [
            "/api/secret",
            "/docs/../secret",
            "/docs/%2e%2e/private",
            "http://example.com",
            "//aeowun.com/docs/",
        ] {
            assert!(live_docs_get(&client, invalid).is_err());
        }
    }
    #[test]
    #[ignore = "Requires live public aeowun.com docs connectivity"]
    fn live_aeowun_docs_search_smoke() {
        let result = search_live_docs("navigation editor actor").expect("Live docs request");
        assert!(result.contains("SOURCE: https://aeowun.com/"), "{result}");
        assert!(result.contains("PATH: /worldkiln/navigation/"), "{result}");
        assert!(
            !result.contains("PAGE:"),
            "Search should not dump full pages"
        );
        let page = read_live_doc("/worldkiln/navigation/").expect("Open one selected page");
        assert!(page.contains("SOURCE: https://aeowun.com/worldkiln/navigation/"));
    }
    #[test]
    fn source_examples_parse_and_match_declared_context() {
        use super::super::ai_script::ScriptKind;
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
            super::super::ai_agent::strict_check_for_context(code, context)
                .unwrap_or_else(|error| panic!("Example {} failed: {error}", example["id"]));
            assert!(example["source_file"].as_str().unwrap().starts_with("src/"));
            assert!(!example["source_test"].as_str().unwrap().is_empty());
        }
        let active: Value = serde_json::from_str(EXAMPLES).unwrap();
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
    fn natural_language_navigation_intent_retrieves_context_without_code_keywords() {
        let turns = vec![(
            "user".into(),
            "Have a villager roam between waypoints".into(),
        )];
        let context = automatic_context(&turns);
        assert!(
            context.starts_with("AUTOMATIC TASK REFERENCES"),
            "{context}"
        );
        assert!(context.len() <= 16_000);
    }

    #[test]
    fn current_build_host_dispatch_is_available_to_api_lookup() {
        let evidence = current_api("damage");
        assert!(evidence.contains("src/scripting/host/mod.rs"), "{evidence}");
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
