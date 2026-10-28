use crate::{
    error::{Result, invalid},
    file_diagnostics::{FileOperation as Op, FileResultExt},
    model::{ServiceId, Settings, validate_version},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Storage {
    pub root: PathBuf,
    _lock: File,
}

#[derive(Default)]
pub struct ConfigChanges {
    pub writes: Vec<(PathBuf, String)>,
    pub removals: Vec<PathBuf>,
}

impl From<Vec<(PathBuf, String)>> for ConfigChanges {
    fn from(writes: Vec<(PathBuf, String)>) -> Self {
        Self {
            writes,
            removals: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ConfigJournal {
    revision: u64,
    settings_before: Vec<u8>,
    files: Vec<JournalFile>,
}
#[derive(Serialize, Deserialize)]
struct JournalFile {
    path: PathBuf,
    before: Option<Vec<u8>>,
}

impl Storage {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).at(Op::Create, &root)?;
        // Most upstream Windows runtimes do not accept Win32 verbatim (\\?\) paths in CLI options.
        let root = PathBuf::from(slash(&root.canonicalize().at(Op::Inspect, &root)?));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("instance.lock"))
            .at(Op::Lock, &root.join("instance.lock"))?;
        lock.try_lock_exclusive()
            .at(Op::Lock, &root.join("instance.lock"))?;
        for directory in [
            "packages", "config", "data", "logs", "projects", ".staging", "secrets", "backups",
        ] {
            fs::create_dir_all(root.join(directory)).at(Op::Create, &root.join(directory))?;
        }
        crate::platform::secure_directory(&root)?;
        let storage = Self { root, _lock: lock };
        storage.recover()?;
        // Interrupted installations are never promoted to live packages.
        for entry in fs::read_dir(storage.root.join(".staging"))? {
            let path = entry?.path();
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
        Ok(storage)
    }
    pub fn load(&self) -> Result<Settings> {
        let path = self.root.join("settings.json");
        if !path.exists() {
            let settings = Settings::default();
            self.save(&settings)?;
            return Ok(settings);
        }
        let mut settings: Settings = serde_json::from_slice(&fs::read(&path).at(Op::Read, &path)?)?;
        if settings.schema_version != 1 {
            return Err(invalid(
                "Unsupported settings schema; restore a compatible backup",
            ));
        }
        for (id, config) in Settings::default().services {
            settings.services.entry(id).or_insert(config);
        }
        for (id, config) in &settings.services {
            crate::model::validate_ports(*id, &config.ports, &settings)?;
            for install in &config.installations {
                validate_version(&install.version)?;
            }
            if let Some(version) = &config.active_version {
                validate_version(version)?;
                if !config.installations.iter().any(|i| &i.version == version) {
                    return Err(invalid("Active version is not installed"));
                }
            }
        }
        for site in &settings.sites {
            crate::model::validate_hostname(&site.hostname)?;
        }
        Ok(settings)
    }
    pub fn save(&self, settings: &Settings) -> Result<()> {
        let path = self.root.join("settings.json");
        if path.exists() {
            atomic_write(
                &self.root.join("backups/settings.previous.json"),
                &fs::read(&path).at(Op::Read, &path)?,
            )?;
        }
        atomic_json(&path, settings)
    }
    pub fn transaction(&self, settings: &Settings, changes: &[(PathBuf, String)]) -> Result<()> {
        self.apply(settings, &ConfigChanges::from(changes.to_vec()))
    }
    pub fn apply(&self, settings: &Settings, changes: &ConfigChanges) -> Result<()> {
        if changes.writes.is_empty() && changes.removals.is_empty() {
            return self.save(settings);
        }
        let mut files = Vec::new();
        for path in &changes.removals {
            if changes.writes.iter().any(|(written, _)| written == path) {
                return Err(invalid(
                    "A configuration cannot be written and removed in one transaction",
                ));
            }
        }
        for path in changes
            .writes
            .iter()
            .map(|(path, _)| path)
            .chain(&changes.removals)
        {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| invalid("Configuration is outside the workspace"))?
                .to_path_buf();
            self.journal_path(&relative)?;
            files.push(JournalFile {
                path: relative,
                before: if path.exists() {
                    Some(fs::read(path)?)
                } else {
                    None
                },
            });
        }
        let journal = ConfigJournal {
            revision: settings.revision,
            settings_before: fs::read(self.root.join("settings.json"))?,
            files,
        };
        let pending = self.root.join("backups/pending-config.json");
        atomic_json(&pending, &journal)?;
        let result = (|| {
            for (path, content) in &changes.writes {
                atomic_write(path, content.as_bytes())?;
            }
            for path in &changes.removals {
                fs::remove_file(path).at(Op::Remove, path)?;
            }
            self.save(settings)
        })();
        if result.is_err() {
            self.rollback(&journal)?;
        }
        let _ = fs::remove_file(pending);
        result
    }
    fn journal_path(&self, relative: &Path) -> Result<PathBuf> {
        if !relative.starts_with("config")
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid("Unsafe configuration recovery path"));
        }
        Ok(self.root.join(relative))
    }
    fn rollback(&self, journal: &ConfigJournal) -> Result<()> {
        for file in &journal.files {
            let path = self.journal_path(&file.path)?;
            match &file.before {
                Some(bytes) => {
                    // A failed deletion can leave a locked file unchanged. Do
                    // not replace it unnecessarily while rolling back others.
                    if fs::read(&path).ok().as_ref() != Some(bytes) {
                        atomic_write(&path, bytes)?;
                    }
                }
                None if path.exists() => fs::remove_file(path)?,
                None => {}
            }
        }
        atomic_write(&self.root.join("settings.json"), &journal.settings_before)
    }
    fn recover(&self) -> Result<()> {
        let pending = self.root.join("backups/pending-config.json");
        if !pending.exists() {
            return Ok(());
        }
        let journal: ConfigJournal = serde_json::from_slice(&fs::read(&pending)?)?;
        let revision = fs::read(self.root.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .map(|s| s.revision);
        if !revision.is_some_and(|revision| revision >= journal.revision) {
            self.rollback(&journal)?;
        }
        fs::remove_file(pending)?;
        Ok(())
    }
    pub fn package(&self, id: ServiceId, version: &str) -> PathBuf {
        self.root.join("packages").join(id.key()).join(version)
    }
    pub fn config_dir(&self, id: ServiceId, version: &str) -> PathBuf {
        self.root.join("config").join(id.key()).join(version)
    }
    pub fn config(&self, id: ServiceId, version: &str) -> PathBuf {
        self.config_dir(id, version).join(id.config_name())
    }
    pub fn data(&self, id: ServiceId, version: &str) -> PathBuf {
        self.root.join("data").join(id.key()).join(version)
    }
    pub fn logs(&self, id: ServiceId, version: &str) -> PathBuf {
        self.root.join("logs").join(id.key()).join(version)
    }
    pub fn prepare(&self, id: ServiceId, version: &str) -> Result<()> {
        for path in [
            self.config_dir(id, version),
            self.data(id, version),
            self.logs(id, version),
        ] {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing parent directory"))?;
    fs::create_dir_all(parent).at(Op::Create, parent)?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .at(Op::Create, &temp)?;
        file.write_all(bytes).at(Op::Write, &temp)?;
        file.sync_all().at(Op::Write, &temp)?;
        drop(file);
        replace_file(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(from_wide.as_ptr()),
            PCWSTR(to_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xffff))
    .between(Op::Write, to, from)
}
#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).between(Op::Write, to, from)?;
    Ok(())
}

pub fn slash(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .replace('\\', "/")
}

pub fn cygwin_path(path: &Path) -> String {
    let path = slash(path);
    if path.as_bytes().get(1) == Some(&b':') {
        format!("/cygdrive/{}{}", path[..1].to_ascii_lowercase(), &path[2..])
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_atomically_and_locks_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(temp.path().to_owned()).unwrap();
        assert!(Storage::open(temp.path().to_owned()).is_err());
        let mut settings = store.load().unwrap();
        settings.language = "pt-BR".into();
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().language, "pt-BR");
        assert!(!store.root.join("packages/php").exists());
    }
    #[test]
    fn corrupt_state_is_never_silently_reset() {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(temp.path().to_owned()).unwrap();
        fs::write(store.root.join("settings.json"), "broken").unwrap();
        assert!(store.load().is_err());
    }
    #[test]
    fn recovers_an_interrupted_configuration_transaction() {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(temp.path().to_owned()).unwrap();
        let settings = store.load().unwrap();
        let path = PathBuf::from("config/php/8.5/php.ini");
        let full = store.root.join(&path);
        atomic_write(&full, b"old").unwrap();
        let journal = ConfigJournal {
            revision: 1,
            settings_before: serde_json::to_vec(&settings).unwrap(),
            files: vec![JournalFile {
                path,
                before: Some(b"old".to_vec()),
            }],
        };
        atomic_json(&store.root.join("backups/pending-config.json"), &journal).unwrap();
        atomic_write(&full, b"interrupted").unwrap();
        drop(store);
        let store = Storage::open(temp.path().to_owned()).unwrap();
        assert_eq!(fs::read(full).unwrap(), b"old");
        assert_eq!(store.load().unwrap().revision, 0);
    }

    #[test]
    fn recovers_deleted_configuration_files_after_an_interrupted_reset() {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(temp.path().to_owned()).unwrap();
        let settings = store.load().unwrap();
        let path = PathBuf::from("config/sites/custom/nginx.conf");
        atomic_write(&store.root.join(&path), b"original rules").unwrap();
        let journal = ConfigJournal {
            revision: settings.revision + 1,
            settings_before: serde_json::to_vec(&settings).unwrap(),
            files: vec![JournalFile {
                path: path.clone(),
                before: Some(b"original rules".to_vec()),
            }],
        };
        atomic_json(&store.root.join("backups/pending-config.json"), &journal).unwrap();
        fs::remove_file(store.root.join(&path)).unwrap();
        drop(store);
        let reopened = Storage::open(temp.path().to_owned()).unwrap();
        assert_eq!(
            fs::read(reopened.root.join(path)).unwrap(),
            b"original rules"
        );
        assert_eq!(reopened.load().unwrap().revision, settings.revision);
    }

    #[test]
    fn rejects_configuration_deletions_outside_config_before_writing_anything() {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(temp.path().to_owned()).unwrap();
        let settings = store.load().unwrap();
        let config = store.root.join("config/custom.ini");
        let database = store.root.join("data/database.bin");
        atomic_write(&config, b"original").unwrap();
        atomic_write(&database, b"database").unwrap();
        let changes = ConfigChanges {
            writes: vec![(config.clone(), "default".into())],
            removals: vec![database.clone()],
        };
        assert!(store.apply(&settings, &changes).is_err());
        assert_eq!(fs::read(config).unwrap(), b"original");
        assert_eq!(fs::read(database).unwrap(), b"database");
    }
}
