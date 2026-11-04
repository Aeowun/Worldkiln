// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const BASE: &str = "https://aeowun.com/api/editor/submissions";
const MAX_ZIP_BYTES: u64 = 500 * 1024 * 1024;
const CHUNK_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct PublishRequest {
    pub archive: PathBuf,
    pub built_folder: Option<PathBuf>,
    pub title: String,
    pub version: String,
    pub description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSession {
    id: String,
    upload_id: String,
    part_size: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedPart {
    part_number: u32,
    etag: String,
}

#[derive(Deserialize)]
struct SubmissionComplete {
    id: String,
    status: String,
}

struct TemporaryArchive {
    path: PathBuf,
}

impl Drop for TemporaryArchive {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn package_built_game(folder: &Path, cancel: &AtomicBool) -> Result<TemporaryArchive, String> {
    let mut entries = Vec::new();
    let mut has_executable = false;
    let mut total = 0u64;
    for entry in std::fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            return Err("Built game contains a directory or link; cannot publish it safely.".into());
        }
        let name = entry.file_name().into_string().map_err(|_| "Invalid game file name.")?;
        has_executable |= name.to_ascii_lowercase().ends_with(".exe");
        total = total.saturating_add(entry.metadata().map_err(|e| e.to_string())?.len());
        if total > MAX_ZIP_BYTES.saturating_mul(3) {
            return Err("Built game is too large to prepare for publishing.".into());
        }
        entries.push((entry.path(), name));
    }
    if !has_executable {
        return Err("Built game has no Windows executable.".into());
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    let path = std::env::temp_dir().join(format!(
        "worldkiln-member-publish-{:032x}.zip",
        rand::random::<u128>()
    ));
    let file = OpenOptions::new().write(true).create_new(true).open(&path)
        .map_err(|e| e.to_string())?;
    let temporary = TemporaryArchive { path };
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut copied_bytes = 0u64;
    for (file_path, name) in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err("Upload cancelled.".into());
        }
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        let mut file = File::open(file_path).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 1024 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Upload cancelled.".into());
            }
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            copied_bytes = copied_bytes.saturating_add(n as u64);
            if copied_bytes > MAX_ZIP_BYTES.saturating_mul(3) {
                return Err("Built game is too large to prepare for publishing.".into());
            }
            zip.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    if std::fs::metadata(&temporary.path).map_err(|e| e.to_string())?.len() > MAX_ZIP_BYTES {
        return Err("The built game ZIP exceeds the 500 MiB submission limit.".into());
    }
    Ok(temporary)
}

fn checked_response(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response, String> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    Err(format!("AEOWUN rejected the upload ({status})."))
}

fn response_json<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<T, String> {
    let response = checked_response(response)?;
    let mut body = Vec::new();
    response.take(64 * 1024 + 1).read_to_end(&mut body)
        .map_err(|_| "Could not read upload response.")?;
    if body.len() > 64 * 1024 {
        return Err("Upload response is too large.".into());
    }
    serde_json::from_slice(&body).map_err(|_| "Invalid upload response.".into())
}

pub(super) fn publish_zip(
    token: &str,
    request: PublishRequest,
    cancel: &AtomicBool,
    mut update: impl FnMut(u8, &str),
) -> Result<String, String> {
    let temporary = if let Some(folder) = request.built_folder.as_deref() {
        update(0, "Packaging built game into ZIP");
        Some(package_built_game(folder, cancel)?)
    } else {
        None
    };
    let archive = temporary.as_ref().map(|temp| temp.path.as_path())
        .unwrap_or(request.archive.as_path());
    let metadata = std::fs::metadata(archive).map_err(|e| e.to_string())?;
    let size = metadata.len();
    if size == 0 || size > MAX_ZIP_BYTES {
        return Err("A Shipping ZIP of at most 500 MiB is required.".into());
    }
    if archive.extension().and_then(|s| s.to_str()).is_none_or(|v| !v.eq_ignore_ascii_case("zip")) {
        return Err("Only a built ZIP archive can be submitted.".into());
    }
    if request.title.trim().is_empty() || request.description.trim().is_empty() || request.version.trim().is_empty() {
        return Err("Game title, version and description are required.".into());
    }
    let filename = if request.built_folder.is_some() {
        let safe_title: String = request
            .title
            .chars()
            .take(100)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{safe_title}-Windows.zip")
    } else {
        archive.file_name()
            .and_then(|s| s.to_str())
            .ok_or("ZIP filename is invalid.")?
            .to_string()
    };
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(150))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Worldkiln-GamePublisher")
        .build()
        .map_err(|e| e.to_string())?;

    if cancel.load(Ordering::Relaxed) {
        return Err("Upload cancelled.".into());
    }
    update(1, "Creating private game submission");
    let response = client.post(BASE)
        .bearer_auth(token)
        .json(&serde_json::json!({
            "title": request.title,
            "version": request.version,
            "description": request.description,
            "filename": filename,
            "sizeBytes": size,
            "engineVersion": env!("CARGO_PKG_VERSION"),
            "rightsAccepted": true,
            "safetyAccepted": true,
            "publicationAccepted": true
        }))
        .send().map_err(|e| format!("Submission request failed: {e}"))?;
    let session: UploadSession = response_json(response)?;
    if session.id.is_empty() || session.id.len() > 128
        || !session.id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        || session.upload_id.is_empty() || session.upload_id.len() > 512
        || session.part_size < CHUNK_BYTES {
        return Err("Invalid upload session.".into());
    }

    let base = format!("{BASE}/{}", session.id);
    let upload = (|| -> Result<String, String> {
        let mut file = File::open(archive).map_err(|e| e.to_string())?;
        let mut uploaded: u64 = 0;
        let mut parts: Vec<UploadedPart> = Vec::new();
        let mut part_number: u32 = 0;
        while uploaded < size {
            if cancel.load(Ordering::Relaxed) {
                return Err("Upload cancelled.".into());
            }
            part_number += 1;
            let part_size = (size - uploaded).min(CHUNK_BYTES) as usize;
            let mut chunk = vec![0u8; part_size];
            file.read_exact(&mut chunk).map_err(|e| e.to_string())?;
            let path = format!("{base}/parts/{part_number}");
            let response = client.put(&path)
                .query(&[("uploadId", session.upload_id.as_str())])
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(chunk)
                .send().map_err(|e| format!("Upload part {part_number} failed: {e}"))?;
            let part: UploadedPart = response_json(response)?;
            if part.part_number != part_number || part.etag.is_empty() {
                return Err("Invalid multipart confirmation.".into());
            }
            parts.push(part);
            uploaded += part_size as u64;
            update((2 + uploaded.saturating_mul(92) / size).min(94) as u8, "Uploading Shipping ZIP");
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Upload cancelled.".into());
        }
        update(96, "Finalizing private submission");
        let response = client.post(format!("{base}/complete"))
            .bearer_auth(token)
            .json(&serde_json::json!({"uploadId":session.upload_id,"parts":parts}))
            .send().map_err(|e| format!("Could not finish upload: {e}"))?;
        let finished: SubmissionComplete = response_json(response)?;
        if finished.id != session.id || finished.status != "pending_scan" {
            return Err("Server did not confirm a private review submission.".into());
        }
        update(100, "Submitted for manual review");
        Ok(finished.id)
    })();

    if upload.is_err() {
        let _ = client.delete(&base)
            .query(&[("uploadId", session.upload_id.as_str())])
            .bearer_auth(token)
            .timeout(Duration::from_secs(6))
            .send();
    }
    upload
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packages_existing_game_without_changing_the_build() {
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-publish-check-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&folder).unwrap();
        let exe = folder.join("Example.exe");
        let readme = folder.join("README.txt");
        std::fs::write(&exe, b"example executable").unwrap();
        std::fs::write(&readme, b"build data").unwrap();

        let archive = package_built_game(&folder, &AtomicBool::new(false)).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&archive.path).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        let mut content = String::new();
        zip.by_name("README.txt").unwrap().read_to_string(&mut content).unwrap();
        assert_eq!(content, "build data");
        drop(zip);

        let archive_path = archive.path.clone();
        drop(archive);
        assert!(!archive_path.exists());
        assert_eq!(std::fs::read(&exe).unwrap(), b"example executable");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn rejects_missing_archive_without_network() {
        let req = PublishRequest {
            archive: PathBuf::from("does-not-exist.zip"),
            built_folder: None,
            title: "Example".into(),
            version: "1.0".into(),
            description: "Test".into(),
        };
        assert!(publish_zip("unused", req, &AtomicBool::new(false), |_, _| {}).is_err());
    }
}
