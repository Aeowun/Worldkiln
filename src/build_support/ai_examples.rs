// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn safe_path(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty()
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == ".."
                || part == "."
                || part.contains(':')
                || part.contains('\\')
        })
    {
        return None;
    }
    let path = root.join(relative);
    path.is_file().then_some(path)
}

fn checked_example(root: &Path, entry: &Value) -> Result<(), String> {
    let id = entry["id"].as_str().ok_or("missing ID")?;
    let source = entry["source_file"].as_str().ok_or("missing source_file")?;
    let test = entry["source_test"].as_str().ok_or("missing source_test")?;
    let dependencies = entry["source_fingerprints"]
        .as_object()
        .ok_or("missing source_fingerprints")?;
    if dependencies.is_empty() || !dependencies.contains_key(source) {
        return Err(format!("{id}: source file is not fingerprinted"));
    }
    for (relative, expected) in dependencies {
        let path = safe_path(root, relative)
            .ok_or_else(|| format!("{id}: source unavailable: {relative}"))?;
        println!("cargo:rerun-if-changed={relative}");
        let bytes = fs::read(&path).map_err(|e| format!("{id}: {relative}: {e}"))?;
        if expected.as_str() != Some(fingerprint(&bytes).as_str()) {
            return Err(format!("{id}: changed source: {relative}"));
        }
    }
    let test_source = fs::read_to_string(
        safe_path(root, source).ok_or_else(|| format!("{id}: missing source test file"))?,
    )
    .map_err(|e| e.to_string())?;
    if !test_source.contains(&format!("fn {test}(")) {
        return Err(format!("{id}: source test no longer exists: {test}"));
    }
    Ok(())
}

pub fn generate() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let manifest = "src/editor/editor/ai_examples.json";
    println!("cargo:rerun-if-changed={manifest}");
    let text = fs::read_to_string(root.join(manifest))
        .expect("AeoScript example manifest must be readable");
    let data: Value =
        serde_json::from_str(&text).expect("AeoScript example manifest must contain valid JSON");
    let entries = data["entries"]
        .as_array()
        .expect("AeoScript example manifest must have an entries array");
    let mut active = Vec::new();
    for entry in entries {
        match checked_example(&root, entry) {
            Ok(()) => active.push(entry.clone()),
            Err(reason) => println!(
                "cargo:warning=AeoScript example disabled: {reason}. Revalidate source fingerprints before enabling this example."
            ),
        }
    }
    let output = json!({"schema_version":1,"authority":"engine source","entries":active});
    let target = PathBuf::from(std::env::var("OUT_DIR").expect("build output"))
        .join("ai_examples_active.json");
    fs::write(
        target,
        serde_json::to_vec(&output).expect("serialize active examples"),
    )
    .expect("write active AeoScript examples");
    println!(
        "cargo:warning=AeoScript examples active: {}/{}",
        active.len(),
        entries.len()
    );
}
