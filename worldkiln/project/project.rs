use super::recent::RecentProjects;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn default_user_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("Aeowun")
                .join("UserData");
        }
    }

    // The repository's UserData is release staging, never a working-directory profile.
    #[cfg(target_os = "windows")]
    let home =
        std::env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join("AppData/Local"));
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share"));
    fallback_user_data_dir(home.as_deref(), &std::env::temp_dir())
}

fn fallback_user_data_dir(home: Option<&Path>, temporary: &Path) -> PathBuf {
    home.unwrap_or(temporary).join("Aeowun").join("UserData")
}

// Worldkiln 0.8.7 | WO 0.8.7-29 | 2026-10-06 15:19 -05:00
fn reject_release_staging(path: &Path) -> Result<(), String> {
    let staging = Path::new(env!("CARGO_MANIFEST_DIR")).join("UserData");

    // Protect the repository staging tree even when UserData itself does not
    // exist in a clean checkout. The previous implementation canonicalized the
    // first existing ancestor, which could stop at the repository root and
    // incorrectly accept paths lexically inside <repo>/UserData.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };

    if absolute.starts_with(&staging) {
        return Err(
            "Repository UserData is release staging only, not an active project/profile".into(),
        );
    }

    // Preserve symlink/junction protection for existing paths by comparing
    // canonicalized locations as a second layer.
    let canonical_staging = fs::canonicalize(&staging).unwrap_or_else(|_| staging.clone());
    for ancestor in absolute.ancestors() {
        if let Ok(resolved) = fs::canonicalize(ancestor) {
            if resolved.starts_with(&canonical_staging) {
                return Err(
                    "Repository UserData is release staging only, not an active project/profile"
                        .into(),
                );
            }
        }
    }

    Ok(())
}

fn resolve_recent_project_path(user_data_dir: &Path, stored_path: &str) -> PathBuf {
    let stored_path = PathBuf::from(stored_path);

    if stored_path.is_absolute() {
        return stored_path;
    }

    // Compatibility with existing recent_projects.json entries such as
    // "UserData\\Escape!".
    let relative = stored_path
        .strip_prefix("UserData")
        .unwrap_or(stored_path.as_path());

    user_data_dir.join(relative)
}

pub struct ProjectManager {
    pub user_data_dir: PathBuf,
    pub current_project: Option<PathBuf>,
    pub recent_projects: Vec<PathBuf>,
}

impl ProjectManager {
    pub fn rename_project(&mut self, source: &Path, name: &str) -> Result<PathBuf, String> {
        let destination = source
            .parent()
            .ok_or("Project has no parent")?
            .join(valid_name(name)?);
        rename_directory(source, &destination)?;
        for path in &mut self.recent_projects {
            if same_path(path, source) {
                *path = destination.clone();
            }
        }
        if self
            .current_project
            .as_deref()
            .is_some_and(|path| same_path(path, source))
        {
            self.current_project = Some(destination.clone());
        }
        self.save_recent_checked()?;
        Ok(destination)
    }

    pub fn duplicate_project(&mut self, source: &Path) -> Result<PathBuf, String> {
        let destination = unique_copy_path(source)?;
        copy_package(source, &destination)?;

        // Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:24 -05:00
        // A duplicated authored project is a new game identity. Export/move
        // operations preserve the original ID, but Duplicate must not share
        // installed-game/save state with its source.
        let duplicated_game_id = destination.join(".worldkiln-game-id");
        if duplicated_game_id.exists() {
            fs::remove_file(&duplicated_game_id)
                .map_err(|error| fs_error(&duplicated_game_id, error))?;
        }

        self.recent_projects.insert(0, destination.clone());
        self.recent_projects.truncate(10);
        self.save_recent_checked()?;
        Ok(destination)
    }

    /// MOVE intentionally exports a complete copy; the authored source stays in place.
    pub fn export_project(&self, source: &Path, folder: &Path) -> Result<PathBuf, String> {
        let destination = folder.join(source.file_name().ok_or("Project has no name")?);
        copy_package(source, &destination)?;
        Ok(destination)
    }

    /// The caller must obtain confirmation before invoking this destructive operation.
    pub fn delete_project(&mut self, source: &Path) -> Result<(), String> {
        checked_directory(source)?;
        fs::remove_dir_all(source).map_err(|e| fs_error(source, e))?;
        self.recent_projects.retain(|path| !same_path(path, source));
        if self
            .current_project
            .as_deref()
            .is_some_and(|path| same_path(path, source))
        {
            self.current_project = None;
        }
        self.save_recent_checked()
    }

    pub fn rename_built_game(&self, game: &BuiltGame, name: &str) -> Result<PathBuf, String> {
        let name = valid_name(name)?;
        let source = game.package_dir()?;
        let executable_name = match game.executable.extension() {
            Some(extension) => format!("{}.{}", name, extension.to_string_lossy()),
            None => name.to_string(),
        };
        let original_name = game
            .executable
            .file_name()
            .ok_or("Build has no executable")?;
        if original_name != std::ffi::OsStr::new(&executable_name)
            && source.join(&executable_name).exists()
        {
            return Err("The renamed executable would overwrite existing package content".into());
        }
        let parent = source
            .parent()
            .and_then(Path::parent)
            .ok_or("Build has no root")?
            .join(name);
        let destination = parent.join(source.file_name().ok_or("Build has no version")?);
        if destination.exists() {
            return Err(format!(
                "Destination already exists: {}",
                destination.display()
            ));
        }
        let readme_path = source.join("README.txt");
        let readme = if readme_path
            .try_exists()
            .map_err(|e| fs_error(&readme_path, e))?
        {
            Some(fs::read_to_string(&readme_path).map_err(|e| fs_error(&readme_path, e))?)
        } else {
            None
        };
        fs::create_dir_all(&parent).map_err(|e| fs_error(&parent, e))?;
        rename_directory(source, &destination)?;
        let old_exe = destination.join(
            game.executable
                .file_name()
                .ok_or("Build has no executable")?,
        );
        let new_exe = destination.join(executable_name);
        if old_exe != new_exe {
            if let Err(error) = fs::rename(&old_exe, &new_exe) {
                // Restore the original package location if the executable rename fails.
                let rollback = fs::rename(&destination, source);
                return Err(format!(
                    "{}; package rollback: {:?}",
                    fs_error(&old_exe, error),
                    rollback
                ));
            }
        }
        if let Some(readme) = readme {
            let updated = readme
                .replace(
                    &format!("Game: {}", game.project_name),
                    &format!("Game: {name}"),
                )
                .replace(
                    &format!("Run {}", original_name.to_string_lossy()),
                    &format!("Run {}", new_exe.file_name().unwrap().to_string_lossy()),
                );
            if let Err(error) = fs::write(destination.join("README.txt"), updated) {
                // Preserve the launch instructions and package identity together on failure.
                let restore_readme = fs::write(destination.join("README.txt"), readme);
                let restore_exe = if old_exe != new_exe {
                    fs::rename(&new_exe, &old_exe)
                } else {
                    Ok(())
                };
                let restore_package = fs::rename(&destination, source);
                return Err(format!(
                    "README update failed: {error}; rollback: {restore_readme:?}, {restore_exe:?}, {restore_package:?}"
                ));
            }
        }
        Ok(new_exe)
    }

    pub fn duplicate_built_game(&self, game: &BuiltGame) -> Result<PathBuf, String> {
        let source = game.package_dir()?;
        let parent = unique_copy_path(source.parent().ok_or("Build has no parent")?)?;
        fs::create_dir(&parent).map_err(|e| fs_error(&parent, e))?;
        let destination = parent.join(source.file_name().ok_or("Build has no version")?);
        copy_package(source, &destination)?;
        Ok(destination.join(
            game.executable
                .file_name()
                .ok_or("Build has no executable")?,
        ))
    }

    /// Export only this version's package, directly beneath a user-selected folder.
    pub fn export_built_game(&self, game: &BuiltGame, folder: &Path) -> Result<PathBuf, String> {
        let destination = folder.join(&game.project_name);
        copy_package(game.package_dir()?, &destination)?;
        Ok(destination)
    }

    /// Delete the selected version's package, never its sibling versions.
    pub fn delete_built_game(&self, game: &BuiltGame) -> Result<(), String> {
        let source = game.package_dir()?;
        checked_directory(source)?;
        fs::remove_dir_all(source).map_err(|e| fs_error(source, e))
    }

    pub fn new() -> Self {
        let user_data_dir = default_user_data_dir();

        if !user_data_dir.exists() {
            fs::create_dir_all(&user_data_dir).ok();
        }

        let mut manager = Self {
            user_data_dir,
            current_project: None,
            recent_projects: Vec::new(),
        };

        manager.load_recent();

        manager
    }

    pub fn list_built_games(&self) -> Vec<BuiltGame> {
        if reject_release_staging(&self.user_data_dir).is_err() {
            return Vec::new();
        }
        let builds_root = self.user_data_dir.join("Builds");
        let mut games = Vec::new();

        let Ok(project_entries) = fs::read_dir(&builds_root) else {
            return games;
        };

        for project_entry in project_entries.flatten() {
            let project_path = project_entry.path();

            if !project_path.is_dir() {
                continue;
            }

            let project_name = project_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Unknown Project")
                .to_string();

            let Ok(version_entries) = fs::read_dir(&project_path) else {
                continue;
            };

            for version_entry in version_entries.flatten() {
                let version_path = version_entry.path();

                if !version_path.is_dir() {
                    continue;
                }

                let version = version_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Unknown Version")
                    .to_string();

                let Ok(files) = fs::read_dir(&version_path) else {
                    continue;
                };

                for file_entry in files.flatten() {
                    let executable = file_entry.path();

                    if !executable.is_file() {
                        continue;
                    }

                    let is_executable = if cfg!(windows) {
                        executable
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("exe"))
                            .unwrap_or(false)
                    } else {
                        executable.extension().is_none()
                    };

                    if !is_executable {
                        continue;
                    }

                    games.push(BuiltGame {
                        project_name: project_name.clone(),
                        version: version.clone(),
                        executable,
                    });
                }
            }
        }

        games.sort_by(|a, b| {
            a.project_name
                .cmp(&b.project_name)
                .then_with(|| b.version.cmp(&a.version))
        });

        games
    }

    pub fn load_recent(&mut self) {
        if reject_release_staging(&self.user_data_dir).is_err() {
            return;
        }
        let path = self.user_data_dir.join("recent_projects.json");

        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(recent) = serde_json::from_str::<RecentProjects>(&content) {
                self.recent_projects = recent
                    .paths
                    .iter()
                    .map(|path| resolve_recent_project_path(&self.user_data_dir, path))
                    .filter_map(|path| {
                        if reject_release_staging(&path).is_err() || !path.exists() {
                            return None;
                        }
                        fs::canonicalize(path).ok()
                    })
                    .collect();
            }
        }
    }

    pub fn save_recent(&self) {
        if let Err(error) = self.save_recent_checked() {
            eprintln!("{error}");
        }
    }

    fn save_recent_checked(&self) -> Result<(), String> {
        reject_release_staging(&self.user_data_dir)?;
        let path = self.user_data_dir.join("recent_projects.json");

        let recent = RecentProjects {
            paths: self
                .recent_projects
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.user_data_dir)
                        .unwrap_or(path.as_path())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
        };

        let content = serde_json::to_string_pretty(&recent).map_err(|e| e.to_string())?;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:56 -05:00
        crate::project::atomic_file::write_atomically(&path, content.as_bytes())
            .map_err(|error| fs_error(&path, error))
    }

    pub fn add_recent(&mut self, path: PathBuf) {
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:56 -05:00
        let path = fs::canonicalize(&path).unwrap_or(path);

        self.recent_projects
            .retain(|existing| !same_path(existing, &path));

        self.recent_projects.insert(0, path);

        if self.recent_projects.len() > 10 {
            self.recent_projects.truncate(10);
        }

        self.save_recent();
    }

    pub fn create_project(&mut self, name: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }

        let project_path = self.user_data_dir.join(name);
        reject_release_staging(&project_path).ok()?;

        if !project_path.exists() {
            fs::create_dir_all(&project_path).ok()?;
        }

        fs::create_dir_all(project_path.join("scripts")).ok()?;
        fs::create_dir_all(project_path.join("playerscripts")).ok()?;

        ensure_project_characters(&project_path);
        ensure_project_controllers(&project_path);
        ensure_project_cameras(&project_path);
        ensure_project_audio(&project_path);
        ensure_project_textures(&project_path);

        if let Err(error) = crate::project::ensure_project_metadata(&project_path) {
            eprintln!("Failed to initialize project metadata: {}", error);
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:56 -05:00
        let project_path = fs::canonicalize(project_path).ok()?;
        self.add_recent(project_path.clone());
        self.current_project = Some(project_path.clone());

        Some(project_path)
    }

    pub fn open_project(&mut self, path: PathBuf) -> bool {
        if reject_release_staging(&path).is_err() {
            return false;
        }
        if !path.exists() || !path.is_dir() {
            return false;
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:56 -05:00
        let Ok(path) = fs::canonicalize(path) else {
            return false;
        };
        if reject_release_staging(&path).is_err() {
            return false;
        }

        if let Err(error) = fs::create_dir_all(path.join("scripts")) {
            eprintln!("Failed to create scripts directory: {}", error);
            return false;
        }

        if let Err(error) = fs::create_dir_all(path.join("playerscripts")) {
            eprintln!("Failed to create playerscripts directory: {}", error);
            return false;
        }

        ensure_project_characters(&path);
        ensure_project_controllers(&path);
        ensure_project_cameras(&path);
        ensure_project_audio(&path);
        ensure_project_textures(&path);

        if let Err(error) = crate::project::ensure_project_metadata(&path) {
            eprintln!("Failed to initialize project metadata: {}", error);
        }

        self.add_recent(path.clone());
        self.current_project = Some(path);

        true
    }

    pub fn list_projects(&self) -> Vec<PathBuf> {
        if reject_release_staging(&self.user_data_dir).is_err() {
            return Vec::new();
        }
        let mut projects = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.user_data_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                if !path.is_dir() {
                    continue;
                }

                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("Builds"))
                {
                    continue;
                }

                projects.push(path);
            }
        }

        projects
    }
}

#[derive(Clone, Debug)]
pub struct BuiltGame {
    pub project_name: String,
    pub version: String,
    pub executable: PathBuf,
}

impl BuiltGame {
    pub fn package_dir(&self) -> Result<&Path, String> {
        self.executable
            .parent()
            .ok_or_else(|| "Build has no package directory".to_string())
    }
}

fn fs_error(path: &Path, error: std::io::Error) -> String {
    format!("{}: {}", path.display(), error)
}

// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 17:27 -05:00
// Compare recently loaded canonical Windows paths against ordinary paths
// from read_dir without exposing path-prefix representation to UI callers.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:57 -05:00
    // Windows canonicalize() adds an extended-length prefix; compare it in
    // the same form as older recent entries and paths held before a rename.
    if cfg!(windows) {
        fn normalized(path: &Path) -> String {
            let path = path.to_string_lossy().replace('/', "\\");
            path.strip_prefix(r"\\?\UNC\")
                .map(|rest| format!(r"\\{rest}"))
                .or_else(|| path.strip_prefix(r"\\?\").map(str::to_string))
                .unwrap_or(path)
        }
        normalized(a).eq_ignore_ascii_case(&normalized(b))
    } else {
        a == b
    }
}

fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(
            "Enter a valid folder name without path separators or reserved characters".into(),
        );
    }
    Ok(name)
}

fn checked_directory(path: &Path) -> Result<(), String> {
    reject_release_staging(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| fs_error(path, e))?;
    // Do not traverse links/junctions into another project's content.
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(format!(
                "Linked directory is unsupported: {}",
                path.display()
            ));
        }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "Expected a project/package directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn unique_copy_path(source: &Path) -> Result<PathBuf, String> {
    let parent = source.parent().ok_or("Source has no parent")?;
    let name = source
        .file_name()
        .ok_or("Source has no name")?
        .to_string_lossy();
    for index in 1.. {
        let suffix = if index == 1 {
            " Copy".to_string()
        } else {
            format!(" Copy {index}")
        };
        let path = parent.join(format!("{name}{suffix}"));
        if !path.try_exists().map_err(|e| fs_error(&path, e))? {
            return Ok(path);
        }
    }
    unreachable!()
}

fn rename_directory(source: &Path, destination: &Path) -> Result<(), String> {
    checked_directory(source)?;
    reject_release_staging(destination)?;
    if destination
        .try_exists()
        .map_err(|e| fs_error(destination, e))?
    {
        return Err(format!(
            "Destination already exists: {}",
            destination.display()
        ));
    }
    fs::rename(source, destination).map_err(|e| fs_error(source, e))
}

fn copy_package(source: &Path, destination: &Path) -> Result<(), String> {
    checked_directory(source)?;
    reject_release_staging(destination)?;
    let canonical_source = fs::canonicalize(source).map_err(|e| fs_error(source, e))?;
    let parent = destination.parent().ok_or("Destination has no parent")?;
    let canonical_parent = fs::canonicalize(parent).map_err(|e| fs_error(parent, e))?;
    if canonical_parent.starts_with(&canonical_source) {
        return Err("Destination cannot be inside the source package".into());
    }
    // Exclusive creation prevents collisions from merging or overwriting packages.
    fs::create_dir(destination).map_err(|e| fs_error(destination, e))?;
    let result = copy_contents(source, destination);
    if let Err(error) = result {
        let cleanup = fs::remove_dir_all(destination);
        return Err(format!("{error}; incomplete export cleanup: {cleanup:?}"));
    }
    Ok(())
}

fn copy_contents(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|e| fs_error(source, e))? {
        let entry = entry.map_err(|e| fs_error(source, e))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| fs_error(&path, e))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(format!("Linked content is unsupported: {}", path.display()));
            }
        }
        if metadata.file_type().is_symlink() {
            return Err(format!("Linked content is unsupported: {}", path.display()));
        }
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|e| fs_error(&target, e))?;
            copy_contents(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(|e| fs_error(&path, e))?;
        }
    }
    Ok(())
}

pub fn engine_assets_dir() -> PathBuf {
    if let Ok(current_dir) = std::env::current_dir() {
        let assets = current_dir.join(".assets");

        if reject_release_staging(&assets).is_ok() && assets.is_dir() {
            return assets;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let assets = parent.join(".assets");

            if reject_release_staging(&assets).is_ok() && assets.is_dir() {
                return assets;
            }
        }
    }

    // A missing asset directory must not fall back to a cwd inside release staging.
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".assets")
}

fn discover_asset_packages(project_path: &Path, category: &str) -> Vec<String> {
    let mut names = HashSet::new();

    let roots = [
        engine_assets_dir().join(category),
        project_path.join(".assets").join(category),
    ];

    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                names.insert(name.to_string());
            }
        }
    }

    let mut names: Vec<String> = names.into_iter().collect();

    names.sort();

    names
}

pub fn ensure_project_characters(project_path: &std::path::Path) {
    let chars_dir = project_path.join(".assets").join("characters");

    let _ = fs::create_dir_all(chars_dir);
}

// Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
fn unique_character_import_temp(parent: &Path, folder_name: &str) -> Result<PathBuf, String> {
    for index in 0..10_000u32 {
        let suffix = if index == 0 {
            format!(".{folder_name}.worldkiln-import-{}", std::process::id())
        } else {
            format!(
                ".{folder_name}.worldkiln-import-{}-{index}",
                std::process::id()
            )
        };
        let candidate = parent.join(suffix);
        if !candidate
            .try_exists()
            .map_err(|error| fs_error(&candidate, error))?
        {
            return Ok(candidate);
        }
    }

    Err("Could not allocate a temporary Character import directory".to_string())
}

// Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
fn transactional_character_copy_with<F>(
    source: &Path,
    destination: &Path,
    copy: F,
) -> Result<(), String>
where
    F: FnOnce(&Path, &Path) -> Result<(), String>,
{
    if destination
        .try_exists()
        .map_err(|error| fs_error(destination, error))?
    {
        return Err(format!(
            "A Character package named '{}' already exists in this project",
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("selected package")
        ));
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "Character destination has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| fs_error(parent, error))?;

    let temp = unique_character_import_temp(
        parent,
        destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("character"),
    )?;
    fs::create_dir(&temp).map_err(|error| fs_error(&temp, error))?;

    if let Err(error) = copy(source, &temp) {
        let cleanup = fs::remove_dir_all(&temp);
        return Err(match cleanup {
            Ok(()) => error,
            Err(cleanup_error) => format!(
                "{error}; failed to clean incomplete Character import '{}': {cleanup_error}",
                temp.display()
            ),
        });
    }

    if let Err(error) = fs::rename(&temp, destination) {
        let cleanup = fs::remove_dir_all(&temp);
        return Err(format!(
            "{}; temporary Character import cleanup: {:?}",
            fs_error(&temp, error),
            cleanup
        ));
    }

    Ok(())
}

// Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
pub fn import_character_package(project_path: &Path, source: &Path) -> Result<String, String> {
    let source_metadata = fs::symlink_metadata(source).map_err(|error| fs_error(source, error))?;
    if !source_metadata.is_dir() || source_metadata.file_type().is_symlink() {
        return Err(format!(
            "Selected Character source is not a normal directory: {}",
            source.display()
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if source_metadata.file_attributes() & 0x400 != 0 {
            return Err(format!(
                "Linked Character directories are unsupported: {}",
                source.display()
            ));
        }
    }

    let validated = crate::character::package::load_package(source)
        .map_err(|error| format!("Invalid Character package: {error}"))?;

    let folder_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "Selected Character folder has no valid folder name".to_string())?;
    let characters_root = project_path.join(".assets").join("characters");
    let destination = characters_root.join(folder_name);

    if destination
        .try_exists()
        .map_err(|error| fs_error(&destination, error))?
    {
        return Err(format!(
            "A Character package named '{folder_name}' already exists in this project"
        ));
    }

    if characters_root.exists() {
        let canonical_source = fs::canonicalize(source).map_err(|error| fs_error(source, error))?;
        let canonical_root = fs::canonicalize(&characters_root)
            .map_err(|error| fs_error(&characters_root, error))?;
        if canonical_root.starts_with(&canonical_source) {
            return Err(
                "The project Character destination cannot be inside the selected source folder"
                    .to_string(),
            );
        }
    }

    transactional_character_copy_with(source, &destination, |source, temp| {
        copy_contents(source, temp)
    })?;

    crate::character::package::reload_packages(Some(project_path));
    let selection = validated.metadata.name.clone();
    if crate::character::package::resolve_package(&selection).is_none() {
        let cleanup = fs::remove_dir_all(&destination);
        crate::character::package::reload_packages(Some(project_path));
        return Err(format!(
            "Imported Character package '{}' could not be resolved after registry refresh; copied package rollback: {:?}",
            selection, cleanup
        ));
    }

    Ok(selection)
}

// Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
pub fn discover_characters(project_path: &std::path::Path) -> Vec<String> {
    ensure_project_characters(project_path);
    crate::character::package::CharacterPackageRegistry::load(Some(project_path)).names()
}

/// Returns every package that can back an authored Actor. Character packages
/// and generic EntityPackages share the Actor.package identity contract.
// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 21:32 -05:00
pub fn discover_actor_packages(_project_path: &std::path::Path) -> Vec<String> {
    // Project load and successful import already refresh both package registries.
    // Reading directories here used to deserialize every GLB for every Inspector
    // frame, even with its package dropdown closed.
    available_actor_package_names()
}

pub fn available_actor_package_names() -> Vec<String> {
    let mut names = crate::character::package::available_package_names();
    names.extend(crate::engine::entity_package::available_package_names());
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

pub fn is_character_package(project_path: Option<&std::path::Path>, package_name: &str) -> bool {
    let pkg = package_name.trim();
    if pkg.is_empty() {
        return false;
    }

    if crate::character::package::resolve_package(pkg).is_some() {
        return true;
    }

    if let Some(path) = project_path {
        let available = discover_characters(path);
        if available.iter().any(|name| name == pkg) {
            return true;
        }
    }

    false
}

pub fn ensure_project_controllers(project_path: &std::path::Path) {
    let ctrl_dir = project_path.join(".assets").join("controllers");

    let _ = fs::create_dir_all(ctrl_dir);
}

pub fn discover_controllers(project_path: &std::path::Path) -> Vec<String> {
    ensure_project_controllers(project_path);
    discover_asset_packages(project_path, "controllers")
}

pub fn ensure_project_cameras(project_path: &std::path::Path) {
    let cam_dir = project_path.join(".assets").join("cameras");

    let _ = fs::create_dir_all(cam_dir);
}

pub fn discover_cameras(project_path: &std::path::Path) -> Vec<String> {
    ensure_project_cameras(project_path);
    discover_asset_packages(project_path, "cameras")
}

pub fn ensure_project_audio(project_path: &std::path::Path) {
    let audio_dir = project_path.join(".assets").join("audio");

    if !audio_dir.exists() {
        let _ = fs::create_dir_all(&audio_dir);
    }
}

pub fn ensure_project_textures(project_path: &std::path::Path) {
    let textures_dir = project_path.join(".assets").join("textures");

    if !textures_dir.exists() {
        let _ = fs::create_dir_all(&textures_dir);
    }
}
#[test]
fn test_create_project_creates_both_script_roots() {
    let test_dir = std::env::temp_dir().join(format!("TestUserData_Roots_{}", std::process::id()));

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).ok();
    }

    fs::create_dir_all(&test_dir).unwrap();

    let mut manager = ProjectManager {
        user_data_dir: test_dir.clone(),
        current_project: None,
        recent_projects: Vec::new(),
    };

    let project_path = manager
        .create_project("RootTest")
        .expect("Project should be created");

    assert!(project_path.join("scripts").is_dir());
    assert!(project_path.join("playerscripts").is_dir());
    assert!(project_path.join(".assets").join("textures").is_dir());

    fs::remove_dir_all(&test_dir).ok();
}

#[test]
fn test_open_project_creates_missing_player_scripts_root() {
    let test_dir =
        std::env::temp_dir().join(format!("TestUserData_PlayerRoot_{}", std::process::id()));

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).ok();
    }

    let project_path = test_dir.join("ExistingProject");

    fs::create_dir_all(project_path.join("scripts")).unwrap();

    let mut manager = ProjectManager {
        user_data_dir: test_dir.clone(),
        current_project: None,
        recent_projects: Vec::new(),
    };

    assert!(manager.open_project(project_path.clone()));

    assert!(project_path.join("scripts").is_dir());
    assert!(project_path.join("playerscripts").is_dir());

    fs::remove_dir_all(&test_dir).ok();
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn profile_fallbacks_never_use_working_directory_userdata() {
        let home = std::env::temp_dir().join("ProfileHome");
        let temporary = std::env::temp_dir().join("ProfileFallback");
        assert_eq!(
            fallback_user_data_dir(Some(&home), &temporary),
            home.join("Aeowun/UserData")
        );
        assert_eq!(
            fallback_user_data_dir(None, &temporary),
            temporary.join("Aeowun/UserData")
        );
        assert_ne!(
            default_user_data_dir(),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("UserData")
        );
    }

    #[test]
    fn repository_release_staging_cannot_be_opened_or_modified_as_a_project() {
        let staging = Path::new(env!("CARGO_MANIFEST_DIR")).join("UserData");
        assert!(reject_release_staging(&staging).is_err());
        assert!(reject_release_staging(&staging.join("UserData/Hello World!")).is_err());
        assert!(reject_release_staging(&staging.join("NewProject/scripts/new.aeo")).is_err());
        let mut manager = ProjectManager {
            user_data_dir: staging.clone(),
            current_project: None,
            recent_projects: vec![],
        };
        assert!(!manager.open_project(staging.clone()));
        assert!(manager.create_project("Forbidden").is_none());
        assert!(manager.list_projects().is_empty());
        assert!(manager.list_built_games().is_empty());
        assert!(manager.save_recent_checked().is_err());
    }

    fn management_fixture(label: &str) -> (PathBuf, ProjectManager) {
        let root = std::env::temp_dir().join(format!(
            "Worldkiln_Management_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let manager = ProjectManager {
            user_data_dir: root.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };
        (root, manager)
    }

    fn authored_content(path: &Path) {
        for file in [
            "scripts/quest.aeo",
            "playerscripts/control.aeo",
            ".assets/textures/nested/a.png",
            "chunks/0/world.dat",
            "UI/hud.json",
            "project.json",
        ] {
            let target = path.join(file);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, file.as_bytes()).unwrap();
        }
    }

    fn assert_authored_content(path: &Path) {
        for file in [
            "scripts/quest.aeo",
            "playerscripts/control.aeo",
            ".assets/textures/nested/a.png",
            "chunks/0/world.dat",
            "UI/hud.json",
            "project.json",
        ] {
            assert_eq!(fs::read(path.join(file)).unwrap(), file.as_bytes());
        }
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:57 -05:00
    #[test]
    fn management_projects_preserve_content_and_update_recent_paths() {
        let (root, mut manager) = management_fixture("projects");
        let source = root.join("Hearthvale");
        authored_content(&source);
        manager.add_recent(source.clone());
        manager.current_project = Some(source.clone());
        let collision = root.join("Other");
        authored_content(&collision);
        assert!(manager.rename_project(&source, "Other").is_err());
        assert_authored_content(&source);
        assert_authored_content(&collision);
        let renamed = manager.rename_project(&source, "Renamed").unwrap();
        assert!(!source.exists());
        assert_authored_content(&renamed);
        assert_eq!(manager.current_project, Some(renamed.clone()));
        manager.load_recent();
        assert_eq!(
            manager.recent_projects,
            vec![fs::canonicalize(&renamed).unwrap()]
        );
        let copy = manager.duplicate_project(&renamed).unwrap();
        assert_eq!(copy.file_name().unwrap(), "Renamed Copy");
        let second = manager.duplicate_project(&renamed).unwrap();
        assert_eq!(second.file_name().unwrap(), "Renamed Copy 2");
        let exports = root.join("Exports");
        fs::create_dir(&exports).unwrap();
        let exported = manager.export_project(&renamed, &exports).unwrap();
        for path in [&renamed, &copy, &second, &exported] {
            assert_authored_content(path);
        }
        assert!(manager.export_project(&renamed, &exports).is_err());
        assert!(
            manager
                .export_project(&renamed, &renamed.join("scripts"))
                .is_err()
        );
        manager.delete_project(&renamed).unwrap();
        assert_eq!(manager.current_project, None);
        manager.load_recent();
        assert!(!manager.recent_projects.contains(&renamed));
        for path in [&copy, &second, &exported, &collision] {
            assert_authored_content(path);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn management_built_games_copy_whole_selected_package() {
        let (root, manager) = management_fixture("builds");
        let package = root.join("Builds/Hearthvale/v1");
        authored_content(&package.join("GameData"));
        fs::create_dir_all(package.join(".assets/audio/nested")).unwrap();
        fs::write(package.join(".assets/audio/nested/sound.bin"), b"sound").unwrap();
        fs::write(package.join("README.txt"), b"readme").unwrap();
        let exe_name = if cfg!(windows) {
            "Hearthvale.exe"
        } else {
            "Hearthvale"
        };
        fs::write(package.join(exe_name), b"executable").unwrap();
        let game = manager.list_built_games().remove(0);
        let duplicate_exe = manager.duplicate_built_game(&game).unwrap();
        let exports = root.join("Exports");
        fs::create_dir(&exports).unwrap();
        let exported = manager.export_built_game(&game, &exports).unwrap();
        assert!(manager.export_built_game(&game, &exports).is_err());
        for path in [&package, duplicate_exe.parent().unwrap(), &exported] {
            assert_authored_content(&path.join("GameData"));
            assert_eq!(
                fs::read(path.join(".assets/audio/nested/sound.bin")).unwrap(),
                b"sound"
            );
            assert_eq!(fs::read(path.join(exe_name)).unwrap(), b"executable");
            assert_eq!(fs::read(path.join("README.txt")).unwrap(), b"readme");
        }
        let renamed_exe = manager.rename_built_game(&game, "Renamed.v2").unwrap();
        assert_eq!(renamed_exe.file_stem().unwrap(), "Renamed.v2");
        assert!(!package.exists());
        assert_authored_content(&renamed_exe.parent().unwrap().join("GameData"));
        let games = manager.list_built_games();
        let renamed_game = games
            .iter()
            .find(|g| g.project_name == "Renamed.v2")
            .unwrap();
        assert_eq!(renamed_game.executable, renamed_exe);
        assert!(
            manager
                .rename_built_game(renamed_game, "Hearthvale Copy")
                .is_err()
        );
        manager.delete_built_game(renamed_game).unwrap();
        assert!(!renamed_exe.exists());
        assert!(duplicate_exe.exists());
        assert!(exported.join(exe_name).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn management_rejects_invalid_names_and_reports_io_failures() {
        let (root, mut manager) = management_fixture("errors");
        let source = root.join("Game");
        authored_content(&source);
        for name in [
            "",
            "..",
            "../escape",
            "C:\\escape",
            "CON",
            "bad?",
            "LPT1",
            "trailing.",
        ] {
            assert!(manager.rename_project(&source, name).is_err(), "{name}");
        }
        assert!(
            manager
                .export_project(&source, &root.join("Missing"))
                .is_err()
        );
        assert!(manager.delete_project(&root.join("Missing")).is_err());
        fs::create_dir(root.join("recent_projects.json")).unwrap();
        let error = manager.rename_project(&source, "Changed").unwrap_err();
        assert!(error.contains("recent_projects.json"));
        assert_authored_content(&root.join("Changed"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_create_project_creates_scripts_dir() {
        let test_dir =
            std::env::temp_dir().join(format!("TestUserData_Create_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&test_dir).unwrap();

        let mut manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        let project_name = "NewProject";

        let project_path = manager
            .create_project(project_name)
            .expect("Should create project");

        let scripts_dir = project_path.join("scripts");

        assert!(scripts_dir.exists(), "Scripts directory should be created");

        assert!(scripts_dir.is_dir(), "Scripts path should be a directory");

        let entries = fs::read_dir(&scripts_dir).unwrap();

        assert_eq!(entries.count(), 0, "Scripts directory should be empty");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_list_projects_excludes_builds_dir() {
        let test_dir =
            std::env::temp_dir().join(format!("TestUserData_List_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("MyProject")).unwrap();
        fs::create_dir_all(test_dir.join("Builds")).unwrap();

        let manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        let projects = manager.list_projects();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0], test_dir.join("MyProject"));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_list_built_games_discovers_executables() {
        let test_dir =
            std::env::temp_dir().join(format!("TestUserData_Builds_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let build_dir = test_dir.join("Builds").join("TestGame").join("v0.7.4");

        fs::create_dir_all(&build_dir).unwrap();

        let executable = if cfg!(windows) {
            build_dir.join("TestGame.exe")
        } else {
            build_dir.join("TestGame")
        };

        fs::write(&executable, b"test").unwrap();

        let manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        let games = manager.list_built_games();

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].project_name, "TestGame");
        assert_eq!(games[0].version, "v0.7.4");
        assert_eq!(games[0].executable, executable);

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_open_project_ensures_scripts_dir() {
        let test_dir =
            std::env::temp_dir().join(format!("TestUserData_Open_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&test_dir).unwrap();

        let project_path = test_dir.join("ExistingProject");

        fs::create_dir_all(&project_path).unwrap();

        let mut manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        let success = manager.open_project(project_path.clone());

        assert!(success, "Should open existing project");

        let scripts_dir = project_path.join("scripts");

        assert!(
            scripts_dir.exists(),
            "Scripts directory should be created on open"
        );

        assert!(scripts_dir.is_dir(), "Scripts path should be a directory");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_metadata_persistence_intact() {
        let test_dir =
            std::env::temp_dir().join(format!("TestUserData_Metadata_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&test_dir).unwrap();

        let mut manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        manager.create_project("Project1");
        manager.create_project("Project2");

        assert_eq!(manager.recent_projects.len(), 2);

        let recent_path = test_dir.join("recent_projects.json");

        assert!(recent_path.exists(), "recent_projects.json should be saved");

        let mut new_manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        new_manager.load_recent();

        assert_eq!(new_manager.recent_projects.len(), 2);

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn recent_project_paths_accept_legacy_and_new_relative_formats() {
        let root = PathBuf::from(r"C:\Users\tester\AppData\Local\Aeowun\UserData");

        assert_eq!(
            resolve_recent_project_path(&root, r"UserData\Escape!"),
            root.join("Escape!")
        );
        assert_eq!(
            resolve_recent_project_path(&root, "Hello World!"),
            root.join("Hello World!")
        );
    }

    #[test]
    fn recent_project_paths_preserve_absolute_locations() {
        let root = PathBuf::from(r"C:\Users\tester\AppData\Local\Aeowun\UserData");
        let external = PathBuf::from(r"D:\Projects\External World");

        assert_eq!(
            resolve_recent_project_path(&root, &external.to_string_lossy()),
            external
        );
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:56 -05:00
    #[test]
    fn load_recent_canonicalizes_paths_and_keeps_other_valid_entries() {
        let root = std::env::temp_dir().join(format!(
            "Worldkiln_RecentCanonical_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = root.join("First");
        let second = root.join("Second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let recent = RecentProjects {
            paths: vec!["First/../First".into(), "Second".into()],
        };
        fs::write(
            root.join("recent_projects.json"),
            serde_json::to_vec(&recent).unwrap(),
        )
        .unwrap();
        let mut manager = ProjectManager {
            user_data_dir: root.clone(),
            current_project: None,
            recent_projects: Vec::new(),
        };

        manager.load_recent();

        assert_eq!(
            manager.recent_projects,
            vec![
                fs::canonicalize(first).unwrap(),
                fs::canonicalize(second).unwrap()
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_recent_stores_projects_under_user_data_as_relative_paths() {
        let test_dir = std::env::temp_dir().join(format!(
            "TestUserData_RelativeRecent_{}",
            std::process::id()
        ));
        fs::create_dir_all(test_dir.join("Escape!")).unwrap();

        let manager = ProjectManager {
            user_data_dir: test_dir.clone(),
            current_project: None,
            recent_projects: vec![test_dir.join("Escape!")],
        };
        manager.save_recent();

        let saved: RecentProjects = serde_json::from_str(
            &fs::read_to_string(test_dir.join("recent_projects.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved.paths, vec!["Escape!".to_string()]);

        fs::remove_dir_all(&test_dir).ok();
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
#[cfg(test)]
mod character_import_hardening_tests {
    use super::*;

    fn fixture_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "Worldkiln_CharacterImport_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_valid_character(dir: &Path, package_name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("package.json"),
            format!(
                "{{\"name\":\"{package_name}\",\"aliases\":[],\"mesh_type\":\"test\",\"has_gun\":false,\"default_animation\":null}}"
            ),
        )
        .unwrap();
        fs::write(dir.join("appearance.json"), "{\"materials\":{}}").unwrap();
        fs::write(
            dir.join("collision.json"),
            "{\"radius\":0.25,\"height\":1.0}",
        )
        .unwrap();
        fs::write(
            dir.join("rig.json"),
            "{\"joints\":[{\"name\":\"Root\",\"parent\":null,\"translation\":[0.0,0.0,0.0],\"rotation\":[0.0,0.0,0.0,1.0],\"scale\":[1.0,1.0,1.0]}]}",
        )
        .unwrap();
        fs::write(dir.join("animation.json"), "{\"animations\":[]}").unwrap();
        fs::write(
            dir.join("blend.json"),
            "{\"default_state\":\"idle\",\"blend_speed\":4.0}",
        )
        .unwrap();
        fs::write(dir.join("geometry.json"), "{\"parts\":[]}").unwrap();
    }

    #[test]
    fn valid_character_import_refreshes_registry_and_returns_selector_identity() {
        let root = fixture_root("valid");
        let project = root.join("Project");
        let source = root.join("SourceCharacter");
        fs::create_dir_all(&project).unwrap();
        write_valid_character(&source, "ImportedCharacter");

        let selected = import_character_package(&project, &source).unwrap();

        assert_eq!(selected, "ImportedCharacter");
        assert!(
            project
                .join(".assets/characters/SourceCharacter/package.json")
                .is_file()
        );
        assert!(crate::character::package::resolve_package("ImportedCharacter").is_some());
        assert!(
            discover_characters(&project)
                .iter()
                .any(|name| name == "ImportedCharacter")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_required_file_is_rejected_before_destination_creation() {
        let root = fixture_root("missing");
        let project = root.join("Project");
        let source = root.join("BrokenCharacter");
        fs::create_dir_all(&project).unwrap();
        write_valid_character(&source, "BrokenCharacter");
        fs::remove_file(source.join("geometry.json")).unwrap();

        let error = import_character_package(&project, &source).unwrap_err();

        assert!(error.contains("Invalid Character package"));
        assert!(!project.join(".assets/characters/BrokenCharacter").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_character_json_is_rejected_before_destination_creation() {
        let root = fixture_root("malformed");
        let project = root.join("Project");
        let source = root.join("BrokenJson");
        fs::create_dir_all(&project).unwrap();
        write_valid_character(&source, "BrokenJson");
        fs::write(source.join("package.json"), "{ definitely not json").unwrap();

        let error = import_character_package(&project, &source).unwrap_err();

        assert!(error.contains("Invalid Character package"));
        assert!(!project.join(".assets/characters/BrokenJson").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_destination_is_never_merged_or_overwritten() {
        let root = fixture_root("collision");
        let project = root.join("Project");
        let source = root.join("SameName");
        let destination = project.join(".assets/characters/SameName");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("sentinel.txt"), "keep").unwrap();
        write_valid_character(&source, "ImportedCharacter");

        let error = import_character_package(&project, &source).unwrap_err();

        assert!(error.contains("already exists"));
        assert_eq!(
            fs::read_to_string(destination.join("sentinel.txt")).unwrap(),
            "keep"
        );
        assert!(!destination.join("package.json").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_transactional_copy_removes_temporary_and_final_package() {
        let root = fixture_root("transaction");
        let source = root.join("Source");
        let destination = root.join("characters/Imported");
        fs::create_dir_all(&source).unwrap();

        let result = transactional_character_copy_with(&source, &destination, |_source, temp| {
            fs::write(temp.join("partial.txt"), "partial").map_err(|error| error.to_string())?;
            Err("injected copy failure".to_string())
        });

        assert!(result.unwrap_err().contains("injected copy failure"));
        assert!(!destination.exists());
        let parent = destination.parent().unwrap();
        let leftovers: Vec<_> = fs::read_dir(parent)
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".worldkiln-import-")
            })
            .collect();
        assert!(leftovers.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn character_discovery_omits_invalid_directories() {
        let root = fixture_root("discovery");
        let project = root.join("Project");
        let valid = project.join(".assets/characters/valid_folder");
        let invalid = project.join(".assets/characters/not_a_package");
        write_valid_character(&valid, "ValidPackage");
        fs::create_dir_all(&invalid).unwrap();

        let names = discover_characters(&project);

        assert!(names.iter().any(|name| name == "ValidPackage"));
        assert!(!names.iter().any(|name| name == "not_a_package"));

        let _ = fs::remove_dir_all(root);
    }
}
