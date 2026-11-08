// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) fn safe_kind(path: &Path) -> io::Result<Option<bool>> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(None);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Ok(None);
        }
    }
    if meta.is_dir() {
        Ok(Some(true))
    } else if meta.is_file() {
        Ok(Some(false))
    } else {
        Ok(None)
    }
}
pub(crate) fn scan(
    root: &Path,
    max_depth: usize,
    max_entries: usize,
) -> io::Result<Vec<(PathBuf, bool)>> {
    let kind = match safe_kind(root) {
        Ok(kind) => kind,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if kind != Some(true) {
        return Ok(Vec::new());
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut result = Vec::new();
    while let Some((folder, depth)) = pending.pop() {
        let mut entries = fs::read_dir(&folder)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let Some(is_dir) = safe_kind(&path)? else {
                continue;
            };
            if result.len() >= max_entries {
                return Err(io::Error::other("Asset directory entry limit exceeded"));
            }
            result.push((path.clone(), is_dir));
            if is_dir {
                if depth >= max_depth {
                    return Err(io::Error::other("Asset directory nesting limit exceeded"));
                }
                pending.push((path, depth + 1));
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorted_tree_and_count_limit() {
        let root = std::env::temp_dir().join(format!("worldkiln-safe-walk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("nested/z.png"), b"x").unwrap();
        fs::write(root.join("a.png"), b"y").unwrap();
        let found = scan(&root, 8, 100).unwrap();
        assert_eq!(found.iter().filter(|e| !e.1).count(), 2);
        assert!(found.iter().all(|e| e.0.starts_with(&root)));
        assert!(scan(&root, 8, 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn junction_not_followed() {
        use std::process::Command;
        let base = std::env::temp_dir().join(format!("worldkiln-junction-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("in")).unwrap();
        fs::create_dir_all(base.join("out")).unwrap();
        fs::write(base.join("out/outside.png"), b"external").unwrap();
        let junction = base.join("in/link");
        let target = base.join("out");
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
                junction.display(),
                target.display()
            ))
            .status()
            .unwrap();
        assert!(status.success(), "Windows junction fixture creation failed");
        assert!(scan(&base.join("in"), 8, 100).unwrap().is_empty());
        fs::remove_dir(&junction).unwrap();
        fs::remove_dir_all(base).unwrap();
    }
}
