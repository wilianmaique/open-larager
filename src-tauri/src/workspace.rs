use crate::{
    engine::Engine,
    error::{Result, invalid},
    file_diagnostics::{FileFailure, FileOperation as Op, FileResultExt},
    model::{ServiceId, Settings},
    move_progress::{MovePhase, MoveProgress, MoveReporter},
    platform,
    storage::{Storage, atomic_json, atomic_write, cygwin_path, slash},
    user_path,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
};
use tokio::sync::RwLock;

pub struct Workspace {
    pub engine: RwLock<Engine>,
    location: Mutex<Location>,
    selection: Mutex<Option<FolderSelection>>,
    preference: PathBuf,
    last_issue: Mutex<Option<FileFailure>>,
    _lock: File,
}

#[derive(Clone, Serialize, Deserialize)]
struct Location {
    root: PathBuf,
    cleanup: Option<Cleanup>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Cleanup {
    source: PathBuf,
    manifest: Manifest,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Manifest {
    files: Vec<Stamp>,
    directories: Vec<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Stamp {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSelection {
    pub path: PathBuf,
    pub token: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveStatus {
    pub cleanup_source: Option<PathBuf>,
    pub cleanup_error: Option<String>,
    pub cleanup_issue: Option<FileFailure>,
}

impl Workspace {
    pub fn open(default_root: PathBuf, preference: PathBuf) -> Result<Self> {
        fs::create_dir_all(
            preference
                .parent()
                .ok_or_else(|| invalid("Missing settings directory"))?,
        )?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(preference.with_extension("lock"))?;
        lock.try_lock_exclusive()
            .map_err(|_| invalid("This application is already open"))?;
        let location: Location = if preference.exists() {
            let location: Location = serde_json::from_slice(&fs::read(&preference)?)?;
            // A disconnected drive must never silently create a replacement environment.
            if !location.root.is_dir() {
                return Err(invalid(
                    "The selected environment is unavailable. Reconnect its drive before opening the app.",
                ));
            }
            location
        } else {
            Location {
                root: default_root,
                cleanup: None,
            }
        };
        let engine = Engine::open(location.root.clone())?;
        Ok(Self {
            engine: RwLock::new(engine),
            location: Mutex::new(location),
            selection: Mutex::new(None),
            preference,
            last_issue: Mutex::new(None),
            _lock: lock,
        })
    }

    // Only a native folder picker can authorize a destination. IPC accepts the
    // token, never an arbitrary path supplied by web content. A successful move
    // consumes it; failed attempts can retry the same authorized selection.
    pub async fn select(&self, path: Option<PathBuf>) -> Result<Option<FolderSelection>> {
        let mut selection = self
            .selection
            .lock()
            .map_err(|_| invalid("Folder selection unavailable"))?;
        *selection = None;
        let Some(path) = path else { return Ok(None) };
        let selected = FolderSelection {
            path: PathBuf::from(slash(&path.canonicalize()?)),
            token: uuid::Uuid::new_v4().to_string(),
        };
        *selection = Some(selected.clone());
        Ok(Some(selected))
    }

    pub fn status(&self) -> Result<MoveStatus> {
        let location = self
            .location
            .lock()
            .map_err(|_| invalid("Environment settings unavailable"))?;
        Ok(MoveStatus {
            cleanup_source: location.cleanup.as_ref().map(|c| c.source.clone()),
            cleanup_error: None,
            cleanup_issue: self.last_issue.lock().ok().and_then(|issue| issue.clone()),
        })
    }

    pub async fn relocate(&self, token: String) -> Result<MoveStatus> {
        self.relocate_with_progress(token, &mut |_| {}).await
    }

    pub async fn relocate_with_progress(
        &self,
        token: String,
        report: &mut (dyn FnMut(MoveProgress) + Send),
    ) -> Result<MoveStatus> {
        let selected = self
            .selection
            .lock()
            .map_err(|_| invalid("Folder selection unavailable"))?
            .as_ref()
            .filter(|selected| selected.token == token)
            .cloned()
            .ok_or_else(|| invalid("Choose the destination folder again"))?;
        let mut current = self
            .engine
            .try_write()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        current.ensure_stopped().await?;
        if self.status()?.cleanup_source.is_some() {
            return Err(invalid(
                "Finish cleaning the previous environment before moving again",
            ));
        }
        let destination =
            validate_destination(&current.storage.root, &selected.path, &self.preference)?;
        let source = current.storage.root.clone();
        let mut progress = MoveReporter::new(report);
        let mut owned = CopyTarget::new(destination)?;
        let manifest = owned.copy_from(&source, &mut progress)?;
        progress.phase(MovePhase::Switching);
        let storage = owned.storage.as_ref().unwrap();
        let mut settings = current.settings.read().await.clone();
        rebase(storage, &source, &mut settings, &mut owned.files)?;
        let next = Engine::from_storage(owned.storage.take().unwrap())?;
        let location = Location {
            root: next.storage.root.clone(),
            cleanup: Some(Cleanup { source, manifest }),
        };
        // The verified copy becomes authoritative before touching the registry or
        // deleting any source files. Pending PATH/cleanup work resumes on startup.
        if let Err(error) = atomic_json(&self.preference, &location) {
            owned.storage = Some(next.storage);
            return Err(error);
        }
        owned.committed = true;
        if let Ok(mut selection) = self.selection.lock()
            && selection
                .as_ref()
                .is_some_and(|selected| selected.token == token)
        {
            *selection = None;
        }
        *self
            .location
            .lock()
            .map_err(|_| invalid("Environment settings unavailable"))? = location;
        let previous = std::mem::replace(&mut *current, next);
        drop(previous); // release the source instance.lock before cleanup
        progress.phase(MovePhase::Cleaning);
        let status = self.finish(&mut current, &mut |bytes| progress.processed(bytes))?;
        if status.cleanup_source.is_none() {
            progress.phase(MovePhase::Complete);
        }
        Ok(status)
    }

    pub async fn finish_pending(&self) -> Result<MoveStatus> {
        if self.status()?.cleanup_source.is_none() {
            return Ok(MoveStatus::default());
        }
        let mut current = self
            .engine
            .try_write()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        current.ensure_stopped().await?;
        self.finish(&mut current, &mut |_| {})
    }

    fn finish(&self, current: &mut Engine, processed: &mut dyn FnMut(u64)) -> Result<MoveStatus> {
        let mut location = self
            .location
            .lock()
            .map_err(|_| invalid("Environment settings unavailable"))?;
        let Some(cleanup) = location.cleanup.clone() else {
            return Ok(MoveStatus::default());
        };
        let result: Result<()> = (|| {
            user_path::relocate(&current.storage, current.settings.get_mut())?;
            clean_source(&cleanup, &current.storage.root, processed)?;
            let complete = Location {
                root: location.root.clone(),
                cleanup: None,
            };
            atomic_json(&self.preference, &complete)?;
            *location = complete;
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Ok(mut last) = self.last_issue.lock() {
                    *last = None;
                }
                Ok(MoveStatus::default())
            }
            Err(error) => {
                let issue = match &error {
                    crate::error::Error::FileOperation(issue) => Some((**issue).clone()),
                    _ => None,
                };
                if let Ok(mut last) = self.last_issue.lock() {
                    *last = issue.clone();
                }
                Ok(MoveStatus {
                    cleanup_source: location.cleanup.as_ref().map(|c| c.source.clone()),
                    cleanup_error: Some(format!("{error}")),
                    cleanup_issue: issue,
                })
            }
        }
    }
}

fn normalized(path: &Path) -> String {
    let value = slash(path).trim_end_matches('/').to_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn contains(parent: &Path, child: &Path) -> bool {
    let parent = normalized(parent);
    let child = normalized(child);
    child == parent || child.starts_with(&format!("{parent}/"))
}

fn validate_destination(source: &Path, destination: &Path, preference: &Path) -> Result<PathBuf> {
    regular(destination)?;
    let destination = PathBuf::from(slash(
        &destination.canonicalize().at(Op::Inspect, destination)?,
    ));
    #[cfg(windows)]
    if !matches!(destination.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err(invalid("Choose a local drive folder"));
    }
    if destination.parent().is_none()
        || destination.file_name().is_none()
        || contains(source, &destination)
        || contains(&destination, source)
        || contains(&destination, preference)
        || contains(&destination, &std::env::current_exe()?)
        || contains(source, &std::env::current_exe()?)
        || fs::read_dir(&destination)
            .at(Op::Inspect, &destination)?
            .next()
            .is_some()
    {
        return Err(invalid(
            "Choose an empty folder outside the current environment and application directories",
        ));
    }
    Ok(destination)
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).at(Op::Inspect, path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if reparse || metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(invalid(format!(
            "Move blocked by a link or special file: {}",
            path.display()
        )));
    }
    Ok(metadata)
}

fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid("Unsafe environment recovery path"));
    }
    let mut path = root.to_path_buf();
    regular(&path)?;
    for part in relative.components() {
        path.push(part);
        if path.exists() {
            regular(&path)?;
        }
    }
    Ok(path)
}

fn digest(path: &Path) -> Result<String> {
    digest_with_progress(path, &mut |_| {})
}

fn digest_with_progress(path: &Path, processed: &mut dyn FnMut(u64)) -> Result<String> {
    regular(path)?;
    let mut input = File::open(path).at(Op::Read, path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let count = input.read(&mut buffer).at(Op::Verify, path)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        processed(count as u64);
    }
    Ok(hex::encode(hash.finalize()))
}

struct CopyTarget {
    root: PathBuf,
    storage: Option<Storage>,
    files: Vec<Stamp>,
    directories: Vec<PathBuf>,
    committed: bool,
}

#[derive(Default)]
struct CopyPlan {
    files: Vec<(PathBuf, u64)>,
    directories: Vec<PathBuf>,
}

impl CopyPlan {
    fn scan(
        &mut self,
        source: &Path,
        relative: &Path,
        progress: &mut MoveReporter<'_>,
    ) -> Result<()> {
        let directory = safe_join_directory(source, relative)?;
        for entry in fs::read_dir(&directory).at(Op::Inspect, &directory)? {
            let relative = relative.join(entry.at(Op::Inspect, &directory)?.file_name());
            if relative == Path::new("instance.lock") {
                continue;
            }
            let path = safe_join(source, &relative)?;
            let metadata = regular(&path)?;
            if metadata.is_dir() {
                self.directories.push(relative.clone());
                self.scan(source, &relative, progress)?;
            } else {
                self.files.push((relative, metadata.len()));
                progress.found_file(metadata.len());
            }
        }
        Ok(())
    }
}

fn safe_join_directory(source: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty() {
        regular(source)?;
        Ok(source.to_path_buf())
    } else {
        safe_join(source, relative)
    }
}

impl CopyTarget {
    fn new(root: PathBuf) -> Result<Self> {
        let storage = Storage::open(root.clone())?;
        Ok(Self {
            root,
            storage: Some(storage),
            files: Vec::new(),
            directories: Vec::new(),
            committed: false,
        })
    }

    fn copy_from(&mut self, source: &Path, progress: &mut MoveReporter<'_>) -> Result<Manifest> {
        let mut plan = CopyPlan::default();
        plan.scan(source, Path::new(""), progress)?;
        progress.start_copy();
        let mut manifest = Manifest::default();
        for relative in plan.directories {
            let to = safe_join(&self.root, &relative)?;
            fs::create_dir_all(&to).at(Op::Create, &to)?;
            self.directories.push(relative.clone());
            manifest.directories.push(relative);
        }
        for (relative, size) in plan.files {
            let from = safe_join(source, &relative)?;
            let to = safe_join(&self.root, &relative)?;
            if regular(&from)?.len() != size {
                return Err(invalid(
                    "An environment file changed while copying. Stop external tools and retry.",
                ));
            }
            let mut input = File::open(&from).at(Op::Read, &from)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&to)
                .at(Op::Create, &to)?;
            let copied: std::io::Result<()> = (|| {
                let mut buffer = [0; 128 * 1024];
                loop {
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    output.write_all(&buffer[..count])?;
                    progress.copied(count as u64);
                }
                output.sync_all()?;
                Ok(())
            })();
            drop(output);
            drop(input);
            if let Err(error) = copied {
                if let Ok(sha256) = digest(&to) {
                    self.files.push(Stamp {
                        path: relative.clone(),
                        sha256,
                    });
                }
                return Err(crate::error::Error::FileOperation(Box::new(
                    FileFailure::capture(Op::Copy, &from, Some(&to), error),
                )));
            }
            let sha256 = digest_with_progress(&to, &mut |bytes| progress.processed(bytes))?;
            self.files.push(Stamp {
                path: relative.clone(),
                sha256: sha256.clone(),
            });
            if regular(&from)?.len() != size
                || sha256 != digest_with_progress(&from, &mut |bytes| progress.processed(bytes))?
            {
                return Err(invalid(
                    "An environment file changed while copying. Stop external tools and retry.",
                ));
            }
            manifest.files.push(Stamp {
                path: relative,
                sha256,
            });
            progress.copied_file();
        }
        Ok(manifest)
    }
}

impl Drop for CopyTarget {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.storage.take();
        // Roll back only files created by this attempt that still match our
        // bytes. External additions and edits are never removed.
        for file in &self.files {
            if let Ok(path) = safe_join(&self.root, &file.path)
                && digest(&path).is_ok_and(|hash| hash == file.sha256)
            {
                let _ = fs::remove_file(path);
            }
        }
        for directory in self.directories.iter().rev() {
            if let Ok(path) = safe_join(&self.root, directory) {
                let _ = fs::remove_dir(path);
            }
        }
        for directory in [
            "packages", "config", "data", "logs", "projects", ".staging", "secrets", "backups",
        ] {
            if let Ok(path) = safe_join(&self.root, Path::new(directory)) {
                let _ = fs::remove_dir(path);
            }
        }
        let _ = fs::remove_file(self.root.join("instance.lock"));
    }
}

fn replace_root(content: &str, from: &str, to: &str) -> String {
    let mut result = String::new();
    let mut rest = content;
    while let Some(index) = rest.find(from) {
        result.push_str(&rest[..index]);
        rest = &rest[index + from.len()..];
        if rest
            .chars()
            .next()
            .is_none_or(|c| c == '/' || c == '\\' || c == '"' || c == '\'' || c.is_whitespace())
        {
            result.push_str(to);
        } else {
            result.push_str(from);
        }
    }
    result.push_str(rest);
    result
}

fn rebase(
    storage: &Storage,
    source: &Path,
    settings: &mut Settings,
    copied: &mut [Stamp],
) -> Result<()> {
    let mut mappings = vec![
        (slash(source), slash(&storage.root)),
        (cygwin_path(source), cygwin_path(&storage.root)),
        (
            source.to_string_lossy().into_owned(),
            storage.root.to_string_lossy().into_owned(),
        ),
        (
            slash(source).replace('/', "\\"),
            slash(&storage.root).replace('/', "\\"),
        ),
    ];
    for id in [ServiceId::Php, ServiceId::Mysql, ServiceId::Postgresql] {
        let mut versions: BTreeSet<_> = settings.services[&id]
            .installations
            .iter()
            .map(|i| i.version.clone())
            .collect();
        // Removed packages retain their configuration and databases. Their
        // Windows short-path references must also survive a later reinstall.
        let configs = storage.root.join("config").join(id.key());
        if configs.is_dir() {
            for entry in fs::read_dir(&configs).at(Op::Inspect, &configs)? {
                let entry = entry.at(Op::Inspect, &configs)?;
                let version = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().at(Op::Inspect, &entry.path())?.is_dir()
                    && crate::model::validate_version(&version).is_ok()
                {
                    versions.insert(version);
                }
            }
        }
        for version in versions {
            let paths = match id {
                ServiceId::Php => vec![storage.package(id, &version).join("ext")],
                ServiceId::Mysql => vec![
                    storage.package(id, &version),
                    storage.data(id, &version),
                    storage.logs(id, &version),
                ],
                ServiceId::Postgresql => vec![storage.logs(id, &version)],
                _ => Vec::new(),
            };
            for to in paths {
                let from = source.join(
                    to.strip_prefix(&storage.root)
                        .map_err(|_| invalid("Invalid managed path"))?,
                );
                mappings.push((platform::runtime_path(&from)?, platform::runtime_path(&to)?));
            }
        }
    }
    mappings.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    for site in &mut settings.sites {
        if contains(source, &site.root) {
            // Match canonical spelling without changing external project paths.
            let suffix = slash(&site.root)[slash(source).len()..]
                .trim_start_matches('/')
                .to_owned();
            site.root = storage.root.join(suffix);
        }
        if let Some(directives) = &mut site.nginx_directives {
            for (from, to) in &mappings {
                *directives = replace_root(directives, from, to);
            }
        }
    }
    for stamp in copied {
        let path = safe_join(&storage.root, &stamp.path)?;
        if stamp.path == Path::new("settings.json") {
            atomic_json(&path, settings)?;
        } else if stamp.path.starts_with("config") && !stamp.path.starts_with("config/path") {
            let bytes = fs::read(&path).at(Op::Read, &path)?;
            // Config files are text, but preserve unknown binary assets verbatim.
            if let Ok(before) = String::from_utf8(bytes) {
                let mut content = before.clone();
                for (from, to) in &mappings {
                    content = replace_root(&content, from, to);
                    content = replace_root(
                        &content,
                        &from.replace('\\', "\\\\"),
                        &to.replace('\\', "\\\\"),
                    );
                }
                if content == before {
                    continue;
                }
                atomic_write(&path, content.as_bytes())?;
            } else {
                continue;
            }
        } else {
            continue;
        }
        stamp.sha256 = digest(&path)?;
    }
    Ok(())
}

fn clean_source(
    cleanup: &Cleanup,
    destination: &Path,
    processed: &mut dyn FnMut(u64),
) -> Result<()> {
    let source = &cleanup.source;
    if !source.exists() {
        return Ok(());
    }
    regular(source)?;
    if contains(source, destination) || contains(destination, source) {
        return Err(invalid("Unsafe environment cleanup location"));
    }
    let lock_path = safe_join(source, Path::new("instance.lock"))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .at(Op::Lock, &lock_path)?;
    lock.try_lock_exclusive().at(Op::Lock, &lock_path)?;
    // Check every file before deleting any: edited originals remain available.
    for file in &cleanup.manifest.files {
        let path = safe_join(source, &file.path)?;
        if !safe_join(destination, &file.path)?.is_file() {
            return Err(invalid(
                "A destination file is missing; the original environment was preserved",
            ));
        }
        if path.exists() && digest_with_progress(&path, processed)? != file.sha256 {
            return Err(invalid(format!(
                "The original file changed and was preserved: {}",
                path.display()
            )));
        }
    }
    for file in &cleanup.manifest.files {
        let path = safe_join(source, &file.path)?;
        if path.exists() {
            if digest_with_progress(&path, processed)? != file.sha256 {
                return Err(invalid(format!(
                    "The original file changed and was preserved: {}",
                    path.display()
                )));
            }
            // Git objects and some distributions are read-only on Windows.
            // Clear that attribute only after verification of the retained copy.
            #[cfg(windows)]
            {
                let mut permissions = regular(&path)?.permissions();
                if permissions.readonly() {
                    #[expect(
                        clippy::permissions_set_readonly_false,
                        reason = "This Windows-only block clears a file attribute; Unix permission bits are never changed"
                    )]
                    permissions.set_readonly(false);
                    fs::set_permissions(&path, permissions).at(Op::Permissions, &path)?;
                }
            }
            fs::remove_file(&path).at(Op::Remove, &path)?;
        }
    }
    for relative in cleanup.manifest.directories.iter().rev() {
        let path = safe_join(source, relative)?;
        if path.exists() {
            fs::remove_dir(&path).at(Op::Remove, &path)?;
        }
    }
    drop(lock);
    fs::remove_file(&lock_path).at(Op::Remove, &lock_path)?;
    fs::remove_dir(source).at(Op::Remove, source)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Site, SiteMode, SiteServer};

    fn fixture() -> (tempfile::TempDir, Workspace, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("original");
        let target = temp.path().join("new environment");
        let preference = temp.path().join("app-config/location.json");
        fs::create_dir(&target).unwrap();
        let workspace = Workspace::open(source.clone(), preference.clone()).unwrap();
        (temp, workspace, source, target, preference)
    }

    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[tokio::test]
    async fn reports_measured_bytes_through_verification_and_source_cleanup() {
        let (_temp, workspace, source, target, _preference) = fixture();
        let settings_bytes = fs::metadata(source.join("settings.json")).unwrap().len();
        let data = vec![73; 1024 * 1024 + 17];
        write(&source, "data/database.bin", &data);
        write(&source, "projects/empty.txt", b"");
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        let mut updates = Vec::new();
        let status = workspace
            .relocate_with_progress(selected.token, &mut |progress| updates.push(progress))
            .await
            .unwrap();
        assert!(status.cleanup_source.is_none());
        assert_eq!(updates.first().unwrap().phase, MovePhase::Scanning);
        let copying = updates
            .iter()
            .find(|p| p.phase == MovePhase::Copying)
            .unwrap();
        let total = settings_bytes + data.len() as u64;
        assert_eq!(copying.total_bytes, total);
        assert_eq!(copying.total_files, 3);
        assert_eq!(copying.copied_bytes, 0);
        let switching = updates
            .iter()
            .find(|p| p.phase == MovePhase::Switching)
            .unwrap();
        assert_eq!(switching.copied_bytes, total);
        assert_eq!(switching.copied_files, 3);
        assert_eq!(switching.processed_bytes, total * 3);
        assert!(updates.iter().any(|p| p.phase == MovePhase::Cleaning));
        let complete = updates.last().unwrap();
        assert_eq!(complete.phase, MovePhase::Complete);
        assert_eq!(complete.processed_bytes, total * 5);
        assert_eq!(complete.total_processing_bytes, total * 5);
        assert!(
            updates
                .windows(2)
                .all(|p| p[0].processed_bytes <= p[1].processed_bytes)
        );
        assert!(!source.exists(), "completion follows source cleanup");
        assert_eq!(fs::read(target.join("data/database.bin")).unwrap(), data);
        assert_eq!(
            fs::metadata(target.join("projects/empty.txt"))
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn rejects_files_that_grow_after_the_size_scan_without_reporting_completion() {
        let (_temp, workspace, source, target, _preference) = fixture();
        write(&source, "projects/growing.bin", b"before");
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        let mut completed = false;
        let result = workspace
            .relocate_with_progress(selected.token, &mut |progress| {
                if progress.phase == MovePhase::Copying {
                    write(&source, "projects/growing.bin", b"external addition");
                }
                completed |= progress.phase == MovePhase::Complete;
            })
            .await;
        assert!(result.is_err());
        assert!(!completed);
        assert_eq!(
            fs::read(source.join("projects/growing.bin")).unwrap(),
            b"external addition"
        );
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        // Windows CI may supply an 8.3 TEMP alias; Storage resolves its full path.
        assert_eq!(
            normalized(&workspace.engine.read().await.storage.root),
            normalized(&source.canonicalize().unwrap())
        );
    }

    #[tokio::test]
    async fn moves_data_rebases_configuration_cleans_source_and_remembers_location() {
        let (_temp, workspace, source, target, preference) = fixture();
        for path in [
            "packages/mysql/9.0.0/bin/mysqld.exe",
            "data/mysql/9.0.0/database.ibd",
            "projects/site/index.php",
            "logs/mysql/9.0.0/mysql.log",
            "secrets/mysql-9.0.0.bin",
        ] {
            write(&source, path, &[0, 255, 12, 128, 31]);
        }
        let engine = workspace.engine.read().await;
        let old_root = slash(&engine.storage.root);
        let config = format!(
            "; custom setting\nmemory_limit=384M\nextension_dir=\"{old_root}/packages/php/8.4.0/ext\"\nexternal=\"{old_root}-external/project\"\n"
        );
        write(&source, "config/php/8.4.0/php.ini", config.as_bytes());
        write(
            &source,
            "config/redis/7.0.0/redis.conf",
            format!(
                "dir \"{}/data/redis/7.0.0\"",
                cygwin_path(&engine.storage.root)
            )
            .as_bytes(),
        );
        let mut settings = engine.settings.write().await;
        settings.language = "pt-BR".into();
        settings.sites = vec![
            Site {
                id: uuid::Uuid::new_v4().to_string(),
                hostname: "example.test".into(),
                root: engine.storage.root.join("projects/site"),
                mode: SiteMode::Php,
                upstream_port: None,
                enabled: false,
                server: SiteServer::Nginx,
                nginx_directives: Some(format!(
                    "error_page 404 {old_root}/projects/site/404.html;"
                )),
            },
            Site {
                id: uuid::Uuid::new_v4().to_string(),
                hostname: "outside.test".into(),
                root: PathBuf::from(format!("{old_root}-external/project")),
                mode: SiteMode::Static,
                upstream_port: None,
                enabled: false,
                server: SiteServer::Caddy,
                nginx_directives: None,
            },
        ];
        let external = settings.sites[1].root.clone();
        engine.storage.save(&settings).unwrap();
        drop(settings);
        drop(engine);

        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        assert!(source.exists());
        assert!(!preference.exists(), "picking a folder must not apply it");
        let status = workspace.relocate(selected.token.clone()).await.unwrap();
        assert!(
            status.cleanup_source.is_none(),
            "{:?}",
            status.cleanup_error
        );
        assert!(!source.exists());
        let engine = workspace.engine.read().await;
        let new_root = slash(&engine.storage.root);
        assert_eq!(
            normalized(&engine.storage.root),
            normalized(&target.canonicalize().unwrap())
        );
        for path in [
            "packages/mysql/9.0.0/bin/mysqld.exe",
            "data/mysql/9.0.0/database.ibd",
            "projects/site/index.php",
            "logs/mysql/9.0.0/mysql.log",
            "secrets/mysql-9.0.0.bin",
        ] {
            assert_eq!(fs::read(target.join(path)).unwrap(), [0, 255, 12, 128, 31]);
        }
        let config = fs::read_to_string(target.join("config/php/8.4.0/php.ini")).unwrap();
        assert!(config.contains("memory_limit=384M"));
        assert!(config.contains(&format!("{new_root}/packages/php")));
        assert!(config.contains(&format!("{old_root}-external/project")));
        let settings = engine.settings.read().await;
        assert_eq!(settings.language, "pt-BR");
        assert_eq!(
            settings.sites[0].root,
            engine.storage.root.join("projects/site")
        );
        assert_eq!(settings.sites[1].root, external);
        assert!(
            settings.sites[0]
                .nginx_directives
                .as_ref()
                .unwrap()
                .contains(&new_root)
        );
        drop(settings);
        drop(engine);
        assert!(
            workspace.relocate(selected.token).await.is_err(),
            "tokens are single use"
        );
        drop(workspace);
        let reopened = Workspace::open(source.clone(), preference).unwrap();
        assert_eq!(
            normalized(&reopened.engine.read().await.storage.root),
            normalized(&target.canonicalize().unwrap())
        );
        assert!(
            !source.exists(),
            "reopening must not recreate the default environment"
        );
    }

    #[tokio::test]
    async fn rejects_nonempty_nested_same_and_unauthorized_destinations_without_changes() {
        let (_temp, workspace, source, target, preference) = fixture();
        write(&source, "projects/keep.txt", b"keep");
        write(&target, "other.txt", b"unrelated");
        assert!(workspace.relocate("invented token".into()).await.is_err());
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        assert!(workspace.relocate(selected.token).await.is_err());
        for path in [source.clone(), source.join("projects")] {
            let selected = workspace.select(Some(path)).await.unwrap().unwrap();
            assert!(workspace.relocate(selected.token).await.is_err());
        }
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        workspace.select(None).await.unwrap();
        assert!(workspace.relocate(selected.token).await.is_err());
        assert_eq!(fs::read(source.join("projects/keep.txt")).unwrap(), b"keep");
        assert_eq!(fs::read(target.join("other.txt")).unwrap(), b"unrelated");
        assert!(!preference.exists());
    }

    #[tokio::test]
    async fn rejects_a_concurrent_workspace_operation() {
        let (_temp, workspace, _source, target, _preference) = fixture();
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        let reading = workspace.engine.read().await;
        assert!(workspace.relocate(selected.token).await.is_err());
        assert!(fs::read_dir(target).unwrap().next().is_none());
        drop(reading);
    }

    #[tokio::test]
    async fn unavailable_selected_drive_does_not_create_a_new_environment() {
        let (_temp, workspace, source, target, preference) = fixture();
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        workspace.relocate(selected.token).await.unwrap();
        drop(workspace);
        fs::rename(&target, target.with_extension("disconnected")).unwrap();
        assert!(Workspace::open(source.clone(), preference).is_err());
        assert!(!source.exists());
        assert!(!target.exists());
    }

    #[test]
    fn cleanup_preserves_changed_originals_and_missing_destination_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let storage = Storage::open(source.clone()).unwrap();
        storage.load().unwrap();
        write(&source, "projects/keep.txt", b"original");
        fs::create_dir(&target).unwrap();
        let mut owned = CopyTarget::new(target.clone()).unwrap();
        let manifest = owned
            .copy_from(&storage.root, &mut MoveReporter::new(&mut |_| {}))
            .unwrap();
        let cleanup = Cleanup {
            source: storage.root.clone(),
            manifest,
        };
        drop(storage);
        write(&source, "projects/keep.txt", b"external edit");
        assert!(clean_source(&cleanup, &target, &mut |_| {}).is_err());
        assert!(
            source.join("settings.json").exists(),
            "preflight must precede deletion"
        );
        assert_eq!(
            fs::read(source.join("projects/keep.txt")).unwrap(),
            b"external edit"
        );
        write(&source, "projects/keep.txt", b"original");
        fs::remove_file(target.join("projects/keep.txt")).unwrap();
        assert!(clean_source(&cleanup, &target, &mut |_| {}).is_err());
        assert!(source.join("projects/keep.txt").exists());
        write(&target, "projects/keep.txt", b"original");
        clean_source(&cleanup, &target, &mut |_| {}).unwrap();
        assert!(!source.exists());
        owned.committed = true;
    }

    #[tokio::test]
    async fn resumes_cleanup_after_reopening_without_recreating_old_folders() {
        let (_temp, workspace, source, target, preference) = fixture();
        write(&source, "projects/project.txt", b"project");
        let current = workspace.engine.read().await;
        let mut owned = CopyTarget::new(target.clone()).unwrap();
        let manifest = owned
            .copy_from(&current.storage.root, &mut MoveReporter::new(&mut |_| {}))
            .unwrap();
        let location = Location {
            root: owned.storage.as_ref().unwrap().root.clone(),
            cleanup: Some(Cleanup {
                source: current.storage.root.clone(),
                manifest,
            }),
        };
        atomic_json(&preference, &location).unwrap();
        owned.committed = true;
        drop(owned);
        drop(current);
        drop(workspace);
        let reopened = Workspace::open(source.clone(), preference).unwrap();
        assert!(reopened.status().unwrap().cleanup_source.is_some());
        assert!(
            reopened
                .finish_pending()
                .await
                .unwrap()
                .cleanup_source
                .is_none()
        );
        assert!(!source.exists());
        assert_eq!(
            fs::read(target.join("projects/project.txt")).unwrap(),
            b"project"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn locked_originals_remain_pending_then_finish_after_unlocking() {
        use std::os::windows::fs::OpenOptionsExt;
        let (_temp, workspace, source, target, _preference) = fixture();
        write(&source, "projects/.git/objects/readonly", b"git object");
        let path = source.join("projects/.git/objects/readonly");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        write(&source, "data/locked.db", b"database");
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(source.join("data/locked.db"))
            .unwrap();
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        let mut phases = Vec::new();
        let status = workspace
            .relocate_with_progress(selected.token, &mut |progress| phases.push(progress.phase))
            .await
            .unwrap();
        assert!(status.cleanup_source.is_some());
        assert!(status.cleanup_error.is_some());
        assert_eq!(phases.last(), Some(&MovePhase::Cleaning));
        assert!(!phases.contains(&MovePhase::Complete));
        let issue = status
            .cleanup_issue
            .as_ref()
            .expect("cleanup must include the blocked path");
        assert!(issue.path.ends_with("data/locked.db"));
        assert_eq!(issue.os_code, Some(32));
        assert_eq!(
            workspace
                .status()
                .unwrap()
                .cleanup_issue
                .as_ref()
                .unwrap()
                .path,
            issue.path
        );
        assert_eq!(
            fs::read(source.join("data/locked.db")).unwrap(),
            b"database"
        );
        assert_eq!(
            fs::read(target.join("data/locked.db")).unwrap(),
            b"database"
        );
        assert_eq!(
            fs::read(target.join("projects/.git/objects/readonly")).unwrap(),
            b"git object"
        );
        drop(locked);
        assert!(
            workspace
                .finish_pending()
                .await
                .unwrap()
                .cleanup_source
                .is_none()
        );
        assert!(!source.exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn byte_range_locks_report_the_exact_file_instead_of_a_bare_os_error_33() {
        let (_temp, workspace, source, target, _preference) = fixture();
        let relative = "EBWebView/Default/Web Data";
        write(&source, relative, b"browser database");
        let locked = OpenOptions::new()
            .read(true)
            .write(true)
            .open(source.join(relative))
            .unwrap();
        locked.try_lock_exclusive().unwrap();
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        let error = workspace.relocate(selected.token).await.unwrap_err();
        let crate::error::Error::FileOperation(issue) = error else {
            panic!("Expected a structured file error")
        };
        assert!(issue.path.ends_with(relative), "{issue:?}");
        assert_eq!(issue.os_code, Some(33));
        assert!(source.join(relative).exists());
        assert!(
            fs::read_dir(target).unwrap().next().is_none(),
            "The failed copy must roll back"
        );
        drop(locked);
        assert_eq!(
            fs::read(source.join(relative)).unwrap(),
            b"browser database"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn refuses_junctions_and_rolls_back_only_its_own_copy() {
        let (_temp, workspace, source, target, preference) = fixture();
        let outside = source.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside, "keep.txt", b"external");
        // Junctions do not require Developer Mode or symlink privilege.
        let link = source.join("projects").join("link");
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{} {}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(regular(&link).is_err());
        let selected = workspace
            .select(Some(target.clone()))
            .await
            .unwrap()
            .unwrap();
        assert!(workspace.relocate(selected.token).await.is_err());
        assert!(!preference.exists());
        assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"external");
        assert!(fs::read_dir(target).unwrap().next().is_none());
        fs::remove_dir(link).unwrap();
    }
}
