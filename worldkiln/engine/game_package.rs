use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

const GAME_PACKAGE_MAGIC: &[u8; 16] = b"WKGAMEPACKAGEV1!";
const GAME_PACKAGE_FOOTER_SIZE: u64 = 32;
const GAME_PACKAGE_FORMAT_VERSION: u32 = 1;
const GAME_ID_FILE: &str = ".worldkiln-game-id";
const RUNTIME_RELEASE_ASSET_NAME: &str = "UserData.zip";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddedGameManifest {
    pub format_version: u32,
    pub game_id: String,
    pub game_name: String,
    pub engine_version: String,
    pub payload_sha256: String,
    pub runtime_release_tag: String,
    pub runtime_asset_name: String,
    pub runtime_asset_url: String,
    pub runtime_asset_sha256: String,
    pub runtime_asset_size: u64,
}

#[derive(Clone, Debug)]
pub struct EmbeddedGamePackage {
    pub manifest: EmbeddedGameManifest,
    pub payload_offset: u64,
    pub payload_len: u64,
}

#[derive(Clone, Debug)]
pub struct RuntimeReleaseAsset {
    pub release_tag: String,
    pub asset_name: String,
    pub browser_download_url: String,
    pub sha256: String,
    pub size: u64,
    pub immutable_release: bool,
}

#[derive(Clone, Debug)]
pub struct PreparedGame {
    pub game_data: PathBuf,
    pub runtime_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    immutable: bool,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct InstalledPayloadMarker {
    format_version: u32,
    game_id: String,
    engine_version: String,
    payload_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeCacheMarker {
    format_version: u32,
    engine_version: String,
    runtime_asset_name: String,
    runtime_asset_sha256: String,
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:18 -05:00
// Small streaming SHA-256 implementation keeps packaged-game integrity checks
// independent of external executables and avoids adding release dependency drift.
struct Sha256State {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Sha256State {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self
            .total_len
            .checked_add(data.len() as u64)
            .expect("SHA-256 input length overflow");

        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];

            if self.buffer_len == 64 {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }

        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().expect("64-byte SHA-256 block");
            self.process_block(block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0_u32; 64];

        for (index, chunk) in block.chunks_exact(4).take(16).enumerate() {
            w[index] = u32::from_be_bytes(chunk.try_into().expect("4-byte SHA-256 word"));
        }

        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(Self::K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self
            .total_len
            .checked_mul(8)
            .expect("SHA-256 bit length overflow");

        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.process_block(&block);
            self.buffer = [0; 64];
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.process_block(&block);

        let mut output = [0_u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        output
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

fn sha256_reader<R: Read>(reader: &mut R) -> Result<(String, u64), String> {
    let mut hasher = Sha256State::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read bytes for SHA-256: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "SHA-256 byte count overflowed".to_string())?;
    }

    Ok((bytes_to_hex(&hasher.finalize()), total))
}

fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let mut file =
        File::open(path).map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
    sha256_reader(&mut file)
}

fn sha256_segment(path: &Path, offset: u64, length: u64) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Failed to seek {}: {error}", path.display()))?;

    let mut limited = file.take(length);
    let (digest, read) = sha256_reader(&mut limited)?;
    if read != length {
        return Err(format!(
            "Embedded payload was truncated: expected {length} bytes, read {read}"
        ));
    }
    Ok(digest)
}

fn validate_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_game_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn sanitize_game_name(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars().take(48) {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            result.push(character);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        result.push_str("game");
    }
    result
}

fn new_game_id(project_name: &str) -> String {
    let random = rand::random::<u128>();
    format!("{}-{random:032x}", sanitize_game_name(project_name))
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn ensure_game_id(project_path: &Path) -> Result<String, String> {
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("game");
    let path = project_path.join(GAME_ID_FILE);

    if path.is_file() {
        let existing = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let existing = existing.trim();
        if validate_game_id(existing) {
            return Ok(existing.to_string());
        }
        return Err(format!(
            "{} contains an invalid Worldkiln game ID",
            path.display()
        ));
    }

    let id = new_game_id(project_name);
    let temporary = project_path.join(format!(".{GAME_ID_FILE}.{}.tmp", std::process::id()));
    fs::write(&temporary, format!("{id}\n"))
        .map_err(|error| format!("Failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Failed to publish {}: {error}", path.display()))?;
    Ok(id)
}

fn release_tag(engine_version: &str) -> String {
    format!("win11-v{engine_version}")
}

fn reqwest_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("Worldkiln/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Failed to create HTTPS client: {error}"))
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn resolve_runtime_release_asset(engine_version: &str) -> Result<RuntimeReleaseAsset, String> {
    let tag = release_tag(engine_version);
    let url = format!("https://api.github.com/repos/Aeowun/Worldkiln/releases/tags/{tag}");
    let release = reqwest_client()?
        .get(&url)
        .send()
        .map_err(|error| format!("Failed to query Worldkiln release {tag}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Worldkiln release {tag} is unavailable: {error}"))?
        .bytes()
        .map_err(|error| format!("Failed to read Worldkiln release {tag}: {error}"))
        .and_then(|bytes| {
            serde_json::from_slice::<GithubRelease>(&bytes)
                .map_err(|error| format!("Failed to parse Worldkiln release {tag}: {error}"))
        })?;

    if release.tag_name != tag {
        return Err(format!(
            "GitHub returned release '{}' while '{}' was requested",
            release.tag_name, tag
        ));
    }

    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == RUNTIME_RELEASE_ASSET_NAME)
        .ok_or_else(|| {
            format!(
                "Release {tag} does not contain required runtime asset {RUNTIME_RELEASE_ASSET_NAME}"
            )
        })?;

    let digest = asset
        .digest
        .ok_or_else(|| format!("{RUNTIME_RELEASE_ASSET_NAME} on {tag} has no SHA-256 digest"))?;
    let sha256 = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("Unsupported release digest format '{digest}'"))?
        .to_ascii_lowercase();

    if !validate_hex_sha256(&sha256) {
        return Err(format!(
            "{RUNTIME_RELEASE_ASSET_NAME} on {tag} has an invalid SHA-256 digest"
        ));
    }

    Ok(RuntimeReleaseAsset {
        release_tag: tag,
        asset_name: asset.name,
        browser_download_url: asset.browser_download_url,
        sha256,
        size: asset.size,
        immutable_release: release.immutable,
    })
}

fn linked_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn archive_name(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("{} is outside {}", path.display(), root.display()))?;

    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {
                return Err(format!(
                    "Unsupported project path while packaging: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn zip_project_directory(
    writer: &mut zip::ZipWriter<File>,
    root: &Path,
    current: &Path,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|error| format!("Failed to read {}: {error}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Failed to enumerate {}: {error}", current.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if linked_or_reparse(&metadata) {
            return Err(format!(
                "Linked project content is not supported in packaged games: {}",
                path.display()
            ));
        }

        let name = archive_name(root, &path)?;
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        if metadata.is_dir() {
            if !name.is_empty() {
                writer
                    .add_directory(format!("{name}/"), options)
                    .map_err(|error| format!("Failed to add project directory {name}: {error}"))?;
            }
            zip_project_directory(writer, root, &path)?;
        } else if metadata.is_file() {
            writer
                .start_file(&name, options)
                .map_err(|error| format!("Failed to add project file {name}: {error}"))?;
            let mut input = File::open(&path)
                .map_err(|error| format!("Failed to open {}: {error}", path.display()))?;
            std::io::copy(&mut input, writer)
                .map_err(|error| format!("Failed to package {}: {error}", path.display()))?;
        }
    }

    Ok(())
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn create_project_payload(
    project_path: &Path,
    destination: &Path,
) -> Result<(String, u64), String> {
    if !project_path.is_dir() {
        return Err(format!(
            "Project directory does not exist: {}",
            project_path.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }

    let file = File::create(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    let mut writer = zip::ZipWriter::new(file);
    zip_project_directory(&mut writer, project_path, project_path)?;
    writer
        .finish()
        .map_err(|error| format!("Failed to finalize project payload: {error}"))?;

    sha256_file(destination)
}

fn encode_footer(payload_len: u64, manifest_len: u64) -> [u8; GAME_PACKAGE_FOOTER_SIZE as usize] {
    let mut footer = [0_u8; GAME_PACKAGE_FOOTER_SIZE as usize];
    footer[..16].copy_from_slice(GAME_PACKAGE_MAGIC);
    footer[16..24].copy_from_slice(&payload_len.to_le_bytes());
    footer[24..32].copy_from_slice(&manifest_len.to_le_bytes());
    footer
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn append_game_package(
    executable_template: &Path,
    payload_zip: &Path,
    manifest: &EmbeddedGameManifest,
    destination: &Path,
) -> Result<(), String> {
    if probe_embedded_game(executable_template)?.is_some() {
        return Err(
            "Refusing to build a game from an already-packaged game executable".to_string(),
        );
    }

    let manifest_bytes = serde_json::to_vec(manifest)
        .map_err(|error| format!("Failed to serialize embedded game manifest: {error}"))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Embedded game manifest is unexpectedly large".to_string());
    }

    let payload_len = fs::metadata(payload_zip)
        .map_err(|error| format!("Failed to inspect {}: {error}", payload_zip.display()))?
        .len();

    let mut output = File::create(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    let mut template = File::open(executable_template).map_err(|error| {
        format!(
            "Failed to open executable template {}: {error}",
            executable_template.display()
        )
    })?;
    std::io::copy(&mut template, &mut output)
        .map_err(|error| format!("Failed to copy executable template: {error}"))?;

    let mut payload = File::open(payload_zip)
        .map_err(|error| format!("Failed to open {}: {error}", payload_zip.display()))?;
    std::io::copy(&mut payload, &mut output)
        .map_err(|error| format!("Failed to append project payload: {error}"))?;

    output
        .write_all(&manifest_bytes)
        .map_err(|error| format!("Failed to append game manifest: {error}"))?;
    output
        .write_all(&encode_footer(payload_len, manifest_bytes.len() as u64))
        .map_err(|error| format!("Failed to append game package footer: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("Failed to flush {}: {error}", destination.display()))?;

    Ok(())
}

fn decode_u64(bytes: &[u8]) -> Result<u64, String> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| "Invalid embedded game footer integer".to_string())?;
    Ok(u64::from_le_bytes(array))
}

fn validate_manifest(manifest: &EmbeddedGameManifest) -> Result<(), String> {
    if manifest.format_version != GAME_PACKAGE_FORMAT_VERSION {
        return Err(format!(
            "Unsupported embedded game format {}",
            manifest.format_version
        ));
    }
    if !validate_game_id(&manifest.game_id) {
        return Err("Embedded game manifest contains an invalid game ID".to_string());
    }
    if manifest.game_name.trim().is_empty() || manifest.game_name.len() > 256 {
        return Err("Embedded game manifest contains an invalid display name".to_string());
    }
    if manifest.engine_version.trim().is_empty() || manifest.engine_version.len() > 64 {
        return Err("Embedded game manifest contains an invalid engine version".to_string());
    }
    if !validate_hex_sha256(&manifest.payload_sha256)
        || !validate_hex_sha256(&manifest.runtime_asset_sha256)
    {
        return Err("Embedded game manifest contains an invalid SHA-256 digest".to_string());
    }
    if manifest.runtime_release_tag != release_tag(&manifest.engine_version) {
        return Err(
            "Embedded game runtime release tag does not match its engine version".to_string(),
        );
    }
    if manifest.runtime_asset_name != RUNTIME_RELEASE_ASSET_NAME {
        return Err("Embedded game references an unsupported runtime asset".to_string());
    }
    if !manifest
        .runtime_asset_url
        .starts_with("https://github.com/Aeowun/Worldkiln/releases/download/")
    {
        return Err(
            "Embedded game runtime asset URL is not an approved Worldkiln GitHub release URL"
                .to_string(),
        );
    }
    Ok(())
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn probe_embedded_game(executable: &Path) -> Result<Option<EmbeddedGamePackage>, String> {
    let mut file = File::open(executable)
        .map_err(|error| format!("Failed to open {}: {error}", executable.display()))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("Failed to inspect {}: {error}", executable.display()))?
        .len();
    if file_len < GAME_PACKAGE_FOOTER_SIZE {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(GAME_PACKAGE_FOOTER_SIZE as i64)))
        .map_err(|error| format!("Failed to inspect game package footer: {error}"))?;
    let mut footer = [0_u8; GAME_PACKAGE_FOOTER_SIZE as usize];
    file.read_exact(&mut footer)
        .map_err(|error| format!("Failed to read game package footer: {error}"))?;

    if &footer[..16] != GAME_PACKAGE_MAGIC {
        return Ok(None);
    }

    let payload_len = decode_u64(&footer[16..24])?;
    let manifest_len = decode_u64(&footer[24..32])?;
    if payload_len == 0 || manifest_len == 0 || manifest_len > MAX_MANIFEST_BYTES {
        return Err("Embedded game footer contains invalid lengths".to_string());
    }

    let overlay_len = payload_len
        .checked_add(manifest_len)
        .and_then(|value| value.checked_add(GAME_PACKAGE_FOOTER_SIZE))
        .ok_or_else(|| "Embedded game package lengths overflow".to_string())?;
    if overlay_len >= file_len {
        return Err("Embedded game package extends beyond the executable".to_string());
    }

    let payload_offset = file_len - overlay_len;
    let manifest_offset = payload_offset
        .checked_add(payload_len)
        .ok_or_else(|| "Embedded game manifest offset overflow".to_string())?;

    file.seek(SeekFrom::Start(manifest_offset))
        .map_err(|error| format!("Failed to seek embedded game manifest: {error}"))?;
    let mut manifest_bytes = vec![0_u8; manifest_len as usize];
    file.read_exact(&mut manifest_bytes)
        .map_err(|error| format!("Failed to read embedded game manifest: {error}"))?;
    let manifest = serde_json::from_slice::<EmbeddedGameManifest>(&manifest_bytes)
        .map_err(|error| format!("Embedded game manifest is invalid JSON: {error}"))?;
    validate_manifest(&manifest)?;

    Ok(Some(EmbeddedGamePackage {
        manifest,
        payload_offset,
        payload_len,
    }))
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn verify_embedded_payload(
    executable: &Path,
    package: &EmbeddedGamePackage,
) -> Result<(), String> {
    let digest = sha256_segment(executable, package.payload_offset, package.payload_len)?;
    if !digest.eq_ignore_ascii_case(&package.manifest.payload_sha256) {
        return Err(format!(
            "Embedded project payload SHA-256 mismatch: expected {}, got {}",
            package.manifest.payload_sha256, digest
        ));
    }
    Ok(())
}

fn safe_extract_zip(zip_path: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    let file = File::open(zip_path)
        .map_err(|error| format!("Failed to open {}: {error}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Invalid ZIP {}: {error}", zip_path.display()))?;
    let mut total_uncompressed = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Failed to inspect ZIP entry {index}: {error}"))?;

        if entry
            .unix_mode()
            .is_some_and(|mode| (mode & 0o170000) == 0o120000)
        {
            return Err(format!("ZIP contains a symbolic link: {}", entry.name()));
        }

        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP contains an unsafe path: {}", entry.name()))?
            .to_path_buf();
        if enclosed
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("ZIP contains an unsafe path: {}", entry.name()));
        }

        total_uncompressed = total_uncompressed
            .checked_add(entry.size())
            .ok_or_else(|| "ZIP uncompressed size overflowed".to_string())?;
        if total_uncompressed > MAX_EXTRACTED_BYTES {
            return Err("ZIP expands beyond the Worldkiln runtime safety limit".to_string());
        }

        let output_path = destination.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|error| format!("Failed to create {}: {error}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(|error| format!("Failed to create {}: {error}", output_path.display()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| format!("Failed to extract {}: {error}", output_path.display()))?;
    }

    Ok(())
}

fn unique_temp_path(parent: &Path, label: &str) -> PathBuf {
    parent.join(format!(
        ".worldkiln-{label}-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    ))
}

fn transactional_replace_directory(staging: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;

    let backup = unique_temp_path(parent, "backup");
    let had_destination = destination.exists();

    if had_destination {
        fs::rename(destination, &backup).map_err(|error| {
            format!(
                "Failed to stage existing {} for replacement: {error}",
                destination.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(staging, destination) {
        let restore = if had_destination {
            fs::rename(&backup, destination)
        } else {
            Ok(())
        };
        return Err(format!(
            "Failed to publish {}: {error}; rollback: {restore:?}",
            destination.display()
        ));
    }

    if had_destination && backup.exists() {
        fs::remove_dir_all(&backup).map_err(|error| {
            format!(
                "Published {}, but failed to remove backup {}: {error}",
                destination.display(),
                backup.display()
            )
        })?;
    }

    Ok(())
}

fn write_marker<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize {}: {error}", path.display()))?;
    fs::write(path, bytes).map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

fn marker_matches<T>(path: &Path, expected: &T) -> bool
where
    T: for<'de> Deserialize<'de> + PartialEq,
{
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<T>(&bytes).ok())
        .is_some_and(|actual| actual == *expected)
}

fn copy_embedded_payload_to_zip(
    executable: &Path,
    package: &EmbeddedGamePackage,
    destination: &Path,
) -> Result<(), String> {
    let mut input = File::open(executable)
        .map_err(|error| format!("Failed to open {}: {error}", executable.display()))?;
    input
        .seek(SeekFrom::Start(package.payload_offset))
        .map_err(|error| format!("Failed to seek embedded payload: {error}"))?;
    let mut limited = input.take(package.payload_len);

    let mut output = File::create(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    let mut hasher = Sha256State::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = limited
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read embedded payload: {error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Failed to stage embedded payload: {error}"))?;
        hasher.update(&buffer[..read]);
        copied += read as u64;
    }

    if copied != package.payload_len {
        return Err(format!(
            "Embedded payload was truncated: expected {}, copied {}",
            package.payload_len, copied
        ));
    }

    let digest = bytes_to_hex(&hasher.finalize());
    if !digest.eq_ignore_ascii_case(&package.manifest.payload_sha256) {
        return Err(format!(
            "Embedded project payload SHA-256 mismatch: expected {}, got {}",
            package.manifest.payload_sha256, digest
        ));
    }

    output
        .sync_all()
        .map_err(|error| format!("Failed to flush {}: {error}", destination.display()))
}

fn installed_payload_root(base: &Path, game_id: &str) -> PathBuf {
    base.join("Aeowun")
        .join("Games")
        .join("Installed")
        .join(game_id)
}

fn runtime_cache_root(base: &Path, engine_version: &str) -> PathBuf {
    base.join("Aeowun")
        .join("Games")
        .join("Runtime")
        .join(format!("v{engine_version}"))
}

fn ensure_project_payload_installed(
    executable: &Path,
    package: &EmbeddedGamePackage,
    base: &Path,
) -> Result<PathBuf, String> {
    let root = installed_payload_root(base, &package.manifest.game_id);
    let marker = InstalledPayloadMarker {
        format_version: GAME_PACKAGE_FORMAT_VERSION,
        game_id: package.manifest.game_id.clone(),
        engine_version: package.manifest.engine_version.clone(),
        payload_sha256: package.manifest.payload_sha256.clone(),
    };
    let marker_path = root.join("package.json");
    let game_data = root.join("GameData");

    if game_data.is_dir() && marker_matches(&marker_path, &marker) {
        return Ok(game_data);
    }

    let parent = root
        .parent()
        .ok_or_else(|| "Installed game root has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    let staging = unique_temp_path(parent, "game-stage");
    fs::create_dir(&staging)
        .map_err(|error| format!("Failed to create {}: {error}", staging.display()))?;

    let result = (|| {
        let zip_path = staging.join("GameData.zip");
        copy_embedded_payload_to_zip(executable, package, &zip_path)?;
        let extracted = staging.join("GameData");
        safe_extract_zip(&zip_path, &extracted)?;
        fs::remove_file(&zip_path)
            .map_err(|error| format!("Failed to remove staged project ZIP: {error}"))?;
        if !extracted.join("world.dat").is_file() {
            return Err("Embedded project payload is missing world.dat".to_string());
        }
        write_marker(&staging.join("package.json"), &marker)?;
        transactional_replace_directory(&staging, &root)?;
        Ok(root.join("GameData"))
    })();

    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn download_verified_runtime_asset(
    manifest: &EmbeddedGameManifest,
    destination: &Path,
) -> Result<(), String> {
    let mut response = reqwest_client()?
        .get(&manifest.runtime_asset_url)
        .send()
        .map_err(|error| {
            format!(
                "Failed to download {}: {error}",
                manifest.runtime_asset_name
            )
        })?
        .error_for_status()
        .map_err(|error| format!("Runtime package download failed: {error}"))?;

    let mut output = File::create(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    let mut hasher = Sha256State::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("Failed while downloading runtime package: {error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Failed to write runtime package: {error}"))?;
        hasher.update(&buffer[..read]);
        total += read as u64;
    }

    if manifest.runtime_asset_size != 0 && total != manifest.runtime_asset_size {
        return Err(format!(
            "Runtime package size mismatch: expected {}, downloaded {}",
            manifest.runtime_asset_size, total
        ));
    }

    let digest = bytes_to_hex(&hasher.finalize());
    if !digest.eq_ignore_ascii_case(&manifest.runtime_asset_sha256) {
        return Err(format!(
            "Runtime package SHA-256 mismatch: expected {}, got {}",
            manifest.runtime_asset_sha256, digest
        ));
    }

    output
        .sync_all()
        .map_err(|error| format!("Failed to flush runtime package: {error}"))
}

fn resolve_extracted_package_root(extracted: &Path) -> Result<PathBuf, String> {
    if extracted.join(".assets").join("version.txt").is_file() {
        return Ok(extracted.to_path_buf());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(extracted)
        .map_err(|error| format!("Failed to inspect runtime package: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to inspect runtime package: {error}"))?;
        let path = entry.path();
        if path.is_dir() && path.join(".assets").join("version.txt").is_file() {
            candidates.push(path);
        }
    }

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err("Runtime package does not contain .assets/version.txt".to_string()),
        _ => Err("Runtime package contains multiple possible package roots".to_string()),
    }
}

fn runtime_assets_current(root: &Path, manifest: &EmbeddedGameManifest) -> bool {
    let marker = RuntimeCacheMarker {
        format_version: GAME_PACKAGE_FORMAT_VERSION,
        engine_version: manifest.engine_version.clone(),
        runtime_asset_name: manifest.runtime_asset_name.clone(),
        runtime_asset_sha256: manifest.runtime_asset_sha256.clone(),
    };

    marker_matches(&root.join("package.json"), &marker)
        && fs::read_to_string(root.join(".assets").join("version.txt"))
            .ok()
            .is_some_and(|version| version.trim() == manifest.engine_version)
}

fn ensure_runtime_assets(manifest: &EmbeddedGameManifest, base: &Path) -> Result<PathBuf, String> {
    let root = runtime_cache_root(base, &manifest.engine_version);
    if runtime_assets_current(&root, manifest) {
        return Ok(root);
    }

    let parent = root
        .parent()
        .ok_or_else(|| "Runtime cache root has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    let staging = unique_temp_path(parent, "runtime-stage");
    fs::create_dir(&staging)
        .map_err(|error| format!("Failed to create {}: {error}", staging.display()))?;

    let result = (|| {
        let zip_path = staging.join(&manifest.runtime_asset_name);
        download_verified_runtime_asset(manifest, &zip_path)?;

        let extracted = staging.join("extract");
        safe_extract_zip(&zip_path, &extracted)?;
        let package_root = resolve_extracted_package_root(&extracted)?;
        let version = fs::read_to_string(package_root.join(".assets").join("version.txt"))
            .map_err(|error| format!("Runtime package is missing .assets/version.txt: {error}"))?;
        if version.trim() != manifest.engine_version {
            return Err(format!(
                "Runtime package version '{}' does not match game engine version '{}'",
                version.trim(),
                manifest.engine_version
            ));
        }

        let cache = staging.join("cache");
        fs::create_dir(&cache)
            .map_err(|error| format!("Failed to create runtime cache staging: {error}"))?;
        fs::rename(package_root.join(".assets"), cache.join(".assets"))
            .map_err(|error| format!("Failed to stage runtime .assets: {error}"))?;
        let marker = RuntimeCacheMarker {
            format_version: GAME_PACKAGE_FORMAT_VERSION,
            engine_version: manifest.engine_version.clone(),
            runtime_asset_name: manifest.runtime_asset_name.clone(),
            runtime_asset_sha256: manifest.runtime_asset_sha256.clone(),
        };
        write_marker(&cache.join("package.json"), &marker)?;
        transactional_replace_directory(&cache, &root)?;
        Ok(root.clone())
    })();

    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 10:53 -05:00
pub fn prepare_embedded_game(
    executable: &Path,
    package: &EmbeddedGamePackage,
) -> Result<PreparedGame, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA is not available for game bootstrap".to_string())?;
    prepare_embedded_game_from(executable, package, &PathBuf::from(local_app_data))
}

pub fn prepare_embedded_game_from(
    executable: &Path,
    package: &EmbeddedGamePackage,
    local_app_data: &Path,
) -> Result<PreparedGame, String> {
    validate_manifest(&package.manifest)?;
    let game_data = ensure_project_payload_installed(executable, package, local_app_data)?;
    let runtime_root = ensure_runtime_assets(&package.manifest, local_app_data)?;
    Ok(PreparedGame {
        game_data,
        runtime_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "Worldkiln_GamePackage_{label}_{}_{}",
            std::process::id(),
            nonce
        ))
    }

    fn sample_manifest(payload_sha256: String, payload_size: u64) -> EmbeddedGameManifest {
        EmbeddedGameManifest {
            format_version: GAME_PACKAGE_FORMAT_VERSION,
            game_id: "SampleGame-0123456789abcdef".to_string(),
            game_name: "Sample Game".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            payload_sha256,
            runtime_release_tag: release_tag(env!("CARGO_PKG_VERSION")),
            runtime_asset_name: RUNTIME_RELEASE_ASSET_NAME.to_string(),
            runtime_asset_url: format!(
                "https://github.com/Aeowun/Worldkiln/releases/download/{}/{}",
                release_tag(env!("CARGO_PKG_VERSION")),
                RUNTIME_RELEASE_ASSET_NAME
            ),
            runtime_asset_sha256:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            runtime_asset_size: payload_size,
        }
    }

    #[test]
    fn internal_sha256_matches_known_vector() {
        let mut hasher = Sha256State::new();
        hasher.update(b"abc");
        assert_eq!(
            bytes_to_hex(&hasher.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn ordinary_executable_without_footer_is_not_a_game() {
        let root = test_root("ordinary");
        fs::create_dir_all(&root).unwrap();
        let exe = root.join("Worldkiln.exe");
        fs::write(&exe, b"MZ ordinary editor bytes").unwrap();

        assert!(probe_embedded_game(&exe).unwrap().is_none());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn packaged_executable_footer_manifest_and_payload_round_trip() {
        let root = test_root("round_trip");
        let project = root.join("Project");
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();
        fs::write(project.join("scripts/main.aeo"), b"print(\"hi\")").unwrap();
        let payload = root.join("payload.zip");
        let (payload_sha, payload_len) = create_project_payload(&project, &payload).unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ template bytes").unwrap();
        let manifest = sample_manifest(payload_sha, payload_len);
        let output = root.join("SampleGame.exe");

        append_game_package(&template, &payload, &manifest, &output).unwrap();
        let package = probe_embedded_game(&output).unwrap().unwrap();

        assert_eq!(package.manifest, manifest);
        verify_embedded_payload(&output, &package).unwrap();

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tampered_embedded_payload_fails_hash_verification() {
        let root = test_root("tamper");
        let project = root.join("Project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();
        let payload = root.join("payload.zip");
        let (payload_sha, payload_len) = create_project_payload(&project, &payload).unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ template bytes").unwrap();
        let manifest = sample_manifest(payload_sha, payload_len);
        let output = root.join("SampleGame.exe");
        append_game_package(&template, &payload, &manifest, &output).unwrap();

        let package = probe_embedded_game(&output).unwrap().unwrap();
        let mut file = OpenOptions::new().write(true).open(&output).unwrap();
        file.seek(SeekFrom::Start(package.payload_offset)).unwrap();
        file.write_all(b"X").unwrap();
        file.sync_all().unwrap();

        assert!(verify_embedded_payload(&output, &package).is_err());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn embedded_identity_survives_executable_rename() {
        let root = test_root("rename");
        let project = root.join("Project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();
        let payload = root.join("payload.zip");
        let (payload_sha, payload_len) = create_project_payload(&project, &payload).unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ template bytes").unwrap();
        let manifest = sample_manifest(payload_sha, payload_len);
        let first = root.join("First.exe");
        append_game_package(&template, &payload, &manifest, &first).unwrap();
        let second = root.join("Renamed.exe");
        fs::rename(&first, &second).unwrap();

        assert_eq!(
            probe_embedded_game(&second)
                .unwrap()
                .unwrap()
                .manifest
                .game_id,
            manifest.game_id
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unsafe_zip_parent_traversal_is_rejected() {
        use std::io::Write as _;
        use zip::write::SimpleFileOptions;

        let root = test_root("unsafe_zip");
        fs::create_dir_all(&root).unwrap();
        let zip_path = root.join("unsafe.zip");
        let file = File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../escape.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"escape").unwrap();
        writer.finish().unwrap();

        let extract = root.join("extract");
        assert!(safe_extract_zip(&zip_path, &extract).is_err());
        assert!(!root.join("escape.txt").exists());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verified_runtime_cache_allows_offline_game_preparation() {
        let root = test_root("offline_cache");
        let project = root.join("Project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();

        let payload = root.join("payload.zip");
        let (payload_sha, payload_len) = create_project_payload(&project, &payload).unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ template bytes").unwrap();
        let manifest = sample_manifest(payload_sha, payload_len);
        let game = root.join("SampleGame.exe");
        append_game_package(&template, &payload, &manifest, &game).unwrap();
        let package = probe_embedded_game(&game).unwrap().unwrap();

        let local_app_data = root.join("LocalAppData");
        let runtime_root = runtime_cache_root(&local_app_data, &manifest.engine_version);
        fs::create_dir_all(runtime_root.join(".assets")).unwrap();
        fs::write(
            runtime_root.join(".assets").join("version.txt"),
            &manifest.engine_version,
        )
        .unwrap();
        let runtime_marker = RuntimeCacheMarker {
            format_version: GAME_PACKAGE_FORMAT_VERSION,
            engine_version: manifest.engine_version.clone(),
            runtime_asset_name: manifest.runtime_asset_name.clone(),
            runtime_asset_sha256: manifest.runtime_asset_sha256.clone(),
        };
        write_marker(&runtime_root.join("package.json"), &runtime_marker).unwrap();

        let prepared = prepare_embedded_game_from(&game, &package, &local_app_data).unwrap();

        assert_eq!(prepared.runtime_root, runtime_root);
        assert!(prepared.game_data.join("world.dat").is_file());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn wrong_runtime_asset_version_is_not_accepted_as_current() {
        let root = test_root("wrong_runtime_version");
        let manifest = sample_manifest(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            123,
        );
        let runtime_root = runtime_cache_root(&root, &manifest.engine_version);
        fs::create_dir_all(runtime_root.join(".assets")).unwrap();
        fs::write(runtime_root.join(".assets").join("version.txt"), "0.0.0").unwrap();
        let marker = RuntimeCacheMarker {
            format_version: GAME_PACKAGE_FORMAT_VERSION,
            engine_version: manifest.engine_version.clone(),
            runtime_asset_name: manifest.runtime_asset_name.clone(),
            runtime_asset_sha256: manifest.runtime_asset_sha256.clone(),
        };
        write_marker(&runtime_root.join("package.json"), &marker).unwrap();

        assert!(!runtime_assets_current(&runtime_root, &manifest));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn project_game_id_is_persistent() {
        let root = test_root("id");
        fs::create_dir_all(&root).unwrap();

        let first = ensure_game_id(&root).unwrap();
        let second = ensure_game_id(&root).unwrap();

        assert_eq!(first, second);
        assert!(validate_game_id(&first));
        fs::remove_dir_all(root).ok();
    }
}
