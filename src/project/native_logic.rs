// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

pub fn valid_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let upper = name.to_ascii_uppercase();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

pub fn script_path(project: &Path, category: &str, name: &str) -> PathBuf {
    project
        .join(".assets")
        .join(category)
        .join(name)
        .join(format!("{name}.aeo"))
}

pub fn editable_script(project: &Path, category: &str, name: &str) -> Result<PathBuf> {
    let local = project.join(".assets").join(category).join(name);
    let dir = if local.is_dir() {
        local
    } else {
        crate::project::engine_assets_dir()
            .join(category)
            .join(name)
    };
    let mut files = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "aeo"))
        .collect::<Vec<_>>();
    files.sort();
    files.into_iter().next().ok_or_else(|| {
        Error::new(
            ErrorKind::NotFound,
            format!("No AeoScript found in {}", dir.display()),
        )
    })
}

pub fn starter_script(category: &str, name: &str) -> Result<String> {
    let (builtin, old) = match category {
        "cameras" => (
            "cameras/thirdPerson/thirdPersonCamera.aeo",
            "ThirdPersonCamera",
        ),
        "controllers" => (
            "controllers/thirdPerson_Controller/thirdPersonController.aeo",
            "ThirdPersonController",
        ),
        _ => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Unknown native asset category",
            ));
        }
    };
    let source = std::fs::read_to_string(crate::project::engine_assets_dir().join(builtin))
        .unwrap_or_else(|_| match category {
            "cameras" => {
                include_str!("../../.assets/cameras/thirdPerson/thirdPersonCamera.aeo").to_owned()
            }
            "controllers" => include_str!(
                "../../.assets/controllers/thirdPerson_Controller/thirdPersonController.aeo"
            )
            .to_owned(),
            _ => unreachable!(),
        });
    let source = source.replacen(&format!("entity {old}"), &format!("entity {name}"), 1);
    validate_source(category, name, &source)?;
    Ok(source)
}

fn validate_source(category: &str, name: &str, source: &str) -> Result<()> {
    use crate::scripting::ast::{Declaration, EntityMember};
    let invalid = |message: String| Error::new(ErrorKind::InvalidInput, message);
    let tokens = crate::scripting::lexer::Lexer::new(source)
        .tokenize()
        .map_err(|e| invalid(format!("Native asset syntax: {}", e.message)))?;
    let program = crate::scripting::parser::Parser::new(tokens)
        .parse()
        .map_err(|e| invalid(format!("Native asset syntax: {}", e.message)))?;
    let entities = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Entity(e) => Some(e),
            _ => None,
        })
        .collect::<Vec<_>>();
    if entities.len() != 1 || entities[0].name != name {
        return Err(invalid(format!(
            "Native asset must declare exactly one entity named '{name}'"
        )));
    }
    let expected = if category == "cameras" { 1 } else { 3 };
    let constructor = entities[0].members.iter().find_map(|m| match m {
        EntityMember::Function(f) if f.name == "constructor" => Some(f),
        _ => None,
    });
    if !constructor.is_some_and(|f| f.parameters.len() == expected) {
        return Err(invalid(format!(
            "{category} constructor requires {expected} runtime arguments"
        )));
    }
    Ok(())
}

pub fn name_in_use(project: &Path, name: &str) -> bool {
    use crate::scripting::ast::Declaration;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::SystemTime;
    type Cached = (u64, Option<SystemTime>, Vec<String>);
    static DECLARATIONS: OnceLock<Mutex<HashMap<PathBuf, Cached>>> = OnceLock::new();
    let cache = DECLARATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    for assets in [project.join(".assets"), crate::project::engine_assets_dir()] {
        for kind in ["cameras", "controllers"] {
            let root = assets.join(kind);
            if root.join(name).exists() {
                return true;
            }
            let Ok(packages) = std::fs::read_dir(root) else {
                continue;
            };
            for package in packages.flatten() {
                if !super::fs_walk::safe_kind(&package.path()).is_ok_and(|k| k == Some(true)) {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(package.path()) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().is_none_or(|e| e != "aeo")
                        || !super::fs_walk::safe_kind(&path).is_ok_and(|k| k == Some(false))
                    {
                        continue;
                    }
                    let Ok(meta) = std::fs::metadata(&path) else {
                        continue;
                    };
                    let signature = (meta.len(), meta.modified().ok());
                    let mut guard = cache
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let fresh = guard
                        .get(&path)
                        .filter(|old| old.0 == signature.0 && old.1 == signature.1)
                        .map(|old| old.2.clone());
                    let declarations = match fresh {
                        Some(names) => names,
                        None => {
                            let names = std::fs::read_to_string(&path)
                                .ok()
                                .and_then(|source| {
                                    crate::scripting::lexer::Lexer::new(&source).tokenize().ok()
                                })
                                .and_then(|tokens| {
                                    crate::scripting::parser::Parser::new(tokens).parse().ok()
                                })
                                .map(|program| {
                                    program
                                        .declarations
                                        .into_iter()
                                        .filter_map(|declaration| {
                                            if let Declaration::Entity(entity) = declaration {
                                                Some(entity.name)
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            if guard.len() > 8192 {
                                guard.clear();
                            }
                            guard.insert(path.clone(), (signature.0, signature.1, names.clone()));
                            names
                        }
                    };
                    drop(guard);
                    if declarations.iter().any(|found| found == name) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn save_native_script(path: &Path, source: &str, new_draft: bool) -> Result<bool> {
    let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(false);
    };
    let Some(package) = path.parent() else {
        return Ok(false);
    };
    let Some(category) = package.parent() else {
        return Ok(false);
    };
    let kind = category.file_name().and_then(|s| s.to_str());
    if path.extension().is_none_or(|e| e != "aeo")
        || package.file_name().and_then(|s| s.to_str()) != Some(name)
        || !matches!(kind, Some("cameras" | "controllers"))
        || category
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            != Some(".assets")
    {
        return Ok(false);
    }
    if !valid_package_name(name) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "Invalid native package name",
        ));
    }
    validate_source(kind.unwrap(), name, source)?;
    if new_draft || !package.exists() {
        if package.exists() || name_in_use(category.parent().and_then(Path::parent).unwrap(), name)
        {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!("Package '{name}' already exists"),
            ));
        }
        std::fs::create_dir_all(category)?;
        std::fs::create_dir(package)?;
        let metadata = serde_json::json!({"name": name, "type": "custom"});
        let result = (|| {
            std::fs::write(path, source)?;
            std::fs::write(
                package.join("package.json"),
                serde_json::to_vec_pretty(&metadata)?,
            )
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(package.join("package.json"));
            let _ = std::fs::remove_dir(package);
        }
        result?;
    } else {
        std::fs::write(path, source)?;
    }
    let project = category
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::other("Missing native asset project root"))?;
    super::asset_registry::AssetRegistry::reconcile(project)
        .map_err(Error::other)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_cannot_escape_package_root() {
        for name in ["../bad", "", "CON", "com1", "LPT9"] {
            assert!(!valid_package_name(name));
        }
        assert!(valid_package_name("MyCamera_2"));
    }
}

#[cfg(all(test, feature = "editor"))]
#[path = "native_logic_tests.rs"]
mod verification;
