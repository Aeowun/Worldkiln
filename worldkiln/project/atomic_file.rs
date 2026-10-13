use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::hash::Hasher;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
/// Replace one project-owned file without truncating its current contents
/// before the replacement has been fully written and synced.
pub(crate) fn write_atomically(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Destination has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("project-file");
    let temporary =
        destination.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, destination)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
/// Fingerprint files replaced by authored project saves so an external edit
/// can be detected before the editor overwrites it.
pub(crate) fn snapshot_persisted_files(project_root: &Path) -> io::Result<BTreeMap<PathBuf, u64>> {
    let mut paths = vec![
        project_root.join("world.dat"),
        project_root.join("project.json"),
        project_root.join("editor_state.json"),
        project_root.join("ui.json"),
        project_root.join("camera.dat"),
    ];
    for directory in [project_root.join("chunks"), project_root.join("terrain")] {
        if directory.exists() {
            let mut pending = vec![directory];
            while let Some(current) = pending.pop() {
                for entry in fs::read_dir(current)? {
                    let entry = entry?;
                    let path = entry.path();
                    if entry.file_type()?.is_dir() {
                        pending.push(path);
                    } else if entry.file_type()?.is_file() {
                        paths.push(path);
                    }
                }
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(project_root)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        let mut file = File::open(&path)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.write(&buffer[..bytes_read]);
        }
        snapshot.insert(relative.to_path_buf(), hasher.finish());
    }
    Ok(snapshot)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_atomic_project_file_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
    #[test]
    fn replacement_preserves_old_file_when_destination_cannot_be_replaced() {
        let root = test_root();
        let destination = root.join("destination");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("sentinel"), b"old project data").unwrap();

        assert!(write_atomically(&destination, b"new project data").is_err());
        assert_eq!(
            fs::read(destination.join("sentinel")).unwrap(),
            b"old project data"
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
    #[test]
    fn replacement_updates_file_contents() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("project.json");
        fs::write(&destination, b"old").unwrap();

        write_atomically(&destination, b"new").unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
    #[test]
    fn persistence_snapshot_detects_changed_added_and_removed_files() {
        let root = test_root();
        fs::create_dir_all(root.join("chunks")).unwrap();
        fs::write(root.join("world.dat"), b"world").unwrap();
        fs::write(root.join("chunks/0_0_0.chunk"), b"chunk").unwrap();
        let baseline = snapshot_persisted_files(&root).unwrap();

        fs::write(root.join("world.dat"), b"external world").unwrap();
        fs::remove_file(root.join("chunks/0_0_0.chunk")).unwrap();
        fs::write(root.join("chunks/1_0_0.chunk"), b"new chunk").unwrap();

        let changed = snapshot_persisted_files(&root).unwrap();
        assert_ne!(changed, baseline);
        assert!(changed.contains_key(Path::new("chunks/1_0_0.chunk")));
        assert!(!changed.contains_key(Path::new("chunks/0_0_0.chunk")));
        fs::remove_dir_all(root).unwrap();
    }
}
