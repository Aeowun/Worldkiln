// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct Transaction {
    root: PathBuf,
    finished: bool,
}
impl Transaction {
    pub fn begin(project: &Path) -> Result<Self, String> {
        let assets = project.join(".assets");
        fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
        let root = assets.join(".material-transaction");
        fs::create_dir(&root).map_err(|e| {
            format!(
                "Material writer busy or interrupted transaction at {}: {e}",
                root.display()
            )
        })?;
        Ok(Self {
            root,
            finished: false,
        })
    }
    pub fn publish(
        mut self,
        destination: &Path,
        bytes: &[u8],
        expected: Option<&[u8]>,
    ) -> Result<(), String> {
        self.publish_with_hook(destination, bytes, expected, |_| {})
    }
    fn publish_with_hook(
        &mut self,
        destination: &Path,
        bytes: &[u8],
        expected: Option<&[u8]>,
        hook: impl FnOnce(&Path),
    ) -> Result<(), String> {
        let staged = self.root.join("candidate");
        let original = self.root.join("original");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        crate::project::atomic_file::write_atomically(
            &self.root.join("destination.json"),
            &serde_json::to_vec(destination).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(destination.parent().ok_or("Missing Material directory")?)
            .map_err(|e| e.to_string())?;
        if expected.is_some() {
            fs::rename(destination, &original).map_err(|e| e.to_string())?;
            let current = fs::read(&original).map_err(|e| e.to_string())?;
            if Some(current.as_slice()) != expected {
                self.restore(destination)?;
                return Err("Material changed on disk; original preserved".into());
            }
        }
        hook(destination);
        if let Err(error) = fs::hard_link(&staged, destination) {
            self.restore(destination)?;
            return Err(format!("Material publication conflict: {error}"));
        }
        fs::remove_dir_all(&self.root).map_err(|e| {
            format!(
                "Material published; journal cleanup failed at {}: {e}",
                self.root.display()
            )
        })?;
        self.finished = true;
        Ok(())
    }
    fn restore(&mut self, destination: &Path) -> Result<(), String> {
        let original = self.root.join("original");
        if original.exists() {
            if let Err(error) = fs::hard_link(&original, destination) {
                let retained = self
                    .root
                    .with_file_name(format!("material-conflict-{:032x}", rand::random::<u128>()));
                fs::rename(&self.root, &retained).map_err(|e| e.to_string())?;
                self.finished = true;
                return Err(format!(
                    "Material conflict; external file preserved and prior version retained at {}: {error}",
                    retained.display()
                ));
            }
        }
        fs::remove_dir_all(&self.root).map_err(|e| e.to_string())?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished && !self.root.join("original").exists() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

pub(super) fn recover(project: &Path) -> Result<(), String> {
    let root = project.join(".assets/.material-transaction");
    if !root.exists() {
        return Ok(());
    }
    let destination: PathBuf = serde_json::from_slice(
        &fs::read(root.join("destination.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let relative = destination
        .strip_prefix(project.join(".assets/materials"))
        .map_err(|_| "Unsafe Material recovery destination")?;
    if !relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Unsafe Material recovery destination".into());
    }
    let checked = super::file_path(project, &relative.to_string_lossy())?;
    if checked != destination {
        return Err("Unsafe Material recovery destination".into());
    }
    let mut transaction = Transaction {
        root,
        finished: false,
    };
    transaction.restore(&destination)
}

#[cfg(test)]
mod water_material_storage_tests {
    use super::*;
    #[test]
    fn water_material_publication_preserves_writer_between_claim_and_commit() {
        let project = std::env::temp_dir().join(format!(
            "water-material-race-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(project.join(".assets/materials")).unwrap();
        let path = project.join(".assets/materials/a.json");
        fs::write(&path, b"original").unwrap();
        let mut tx = Transaction::begin(&project).unwrap();
        assert!(
            tx.publish_with_hook(&path, b"candidate", Some(b"original"), |p| fs::write(
                p,
                b"external"
            )
            .unwrap())
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"external");
        let retained = fs::read_dir(project.join(".assets"))
            .unwrap()
            .flatten()
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("material-conflict-")
            })
            .unwrap();
        assert_eq!(
            fs::read(retained.path().join("original")).unwrap(),
            b"original"
        );
        fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn water_material_claim_checks_latest_bytes_and_recovers_interruption() {
        let project = std::env::temp_dir().join(format!(
            "water-material-recover-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(project.join(".assets/materials")).unwrap();
        let path = project.join(".assets/materials/a.json");
        fs::write(&path, b"external").unwrap();
        assert!(
            Transaction::begin(&project)
                .unwrap()
                .publish(&path, b"candidate", Some(b"old"))
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"external");
        let tx = Transaction::begin(&project).unwrap();
        fs::rename(&path, tx.root.join("original")).unwrap();
        fs::write(
            tx.root.join("destination.json"),
            serde_json::to_vec(&path).unwrap(),
        )
        .unwrap();
        drop(tx);
        recover(&project).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"external");
        fs::remove_dir_all(project).unwrap();
    }
}
