//! Configuration-only reset. No package, database, project, log or credential is removed.
use crate::{
    config,
    error::{Result, invalid},
    file_diagnostics::{FileOperation as Op, FileResultExt},
    model::{ServiceId, Settings},
    php_tls, platform, sites,
    storage::{ConfigChanges, Storage, atomic_write},
    toolchain,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MAX_CONFIG_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {
    pub backup_path: PathBuf,
    pub removed_files: usize,
}

pub(crate) struct ResetPlan {
    pub settings: Settings,
    pub changes: ConfigChanges,
    files: BTreeMap<PathBuf, Vec<u8>>,
    settings_before: Vec<u8>,
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).at(Op::Inspect, path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(invalid(
            "Configuration reset refuses symbolic links, junctions and special files",
        ));
    }
    Ok(metadata)
}

fn scan(
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    bytes: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 64 || files.len() > 10000 {
        return Err(invalid(
            "Configuration reset exceeds the supported file count or nesting limit",
        ));
    }
    regular(directory)?;
    for entry in fs::read_dir(directory).at(Op::Inspect, directory)? {
        let path = entry.at(Op::Inspect, directory)?.path();
        let metadata = regular(&path)?;
        if metadata.is_dir() {
            scan(&path, files, bytes, depth + 1)?;
        } else {
            if files.len() >= 10000 {
                return Err(invalid(
                    "Configuration reset exceeds the supported file count",
                ));
            }
            if metadata.len() > (MAX_CONFIG_BYTES.saturating_sub(*bytes)) as u64 {
                return Err(invalid(
                    "Configuration reset exceeds the 64 MiB backup limit",
                ));
            }
            let content = fs::read(&path).at(Op::Read, &path)?;
            *bytes = bytes
                .checked_add(content.len())
                .filter(|size| *size <= MAX_CONFIG_BYTES)
                .ok_or_else(|| invalid("Configuration changed while preparing the reset"))?;
            files.insert(path, content);
        }
    }
    Ok(())
}

fn snapshot(storage: &Storage) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    scan(&storage.root.join("config"), &mut files, &mut 0, 0)?;
    Ok(files)
}

pub(crate) fn prepare(storage: &Storage, current: &Settings) -> Result<ResetPlan> {
    if storage.root.join("config/path/pending.json").exists() {
        return Err(invalid(
            "Finish pending PATH recovery before resetting configuration",
        ));
    }
    let files = snapshot(storage)?;
    let configuration = storage.root.join("config").canonicalize()?;
    for site in &current.sites {
        if let Ok(project) = site.root.canonicalize()
            && (project.starts_with(&configuration) || configuration.starts_with(&project))
        {
            return Err(invalid(
                "A project overlaps the configuration directory. Move the project before resetting configuration.",
            ));
        }
    }
    let mut settings = Settings {
        revision: current
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Settings revision overflow"))?,
        ..Settings::default()
    };
    let mut writes = Vec::new();
    for id in ServiceId::ALL {
        let service = settings.services.get_mut(&id).unwrap();
        service.installations = current.services[&id].installations.clone();
        service.active_version = current.services[&id].active_version.clone();
    }
    for id in ServiceId::ALL {
        for install in &settings.services[&id].installations {
            if id.definition().config_name.is_some() {
                writes.push((
                    storage.config(id, &install.version),
                    config::reset_defaults(storage, id, &install.version, &settings)?,
                ));
            } else if id == ServiceId::Nodejs {
                writes.push((
                    storage.config_dir(id, &install.version).join("npmrc"),
                    "# Open Larager defaults.\n".into(),
                ));
            }
        }
    }
    if !settings.services[&ServiceId::Php].installations.is_empty() {
        writes.extend(php_tls::default_files(storage)?);
    }
    writes.extend(sites::files(storage, &settings)?);
    // Preserve working adapters for pre-existing PATH entries we do not own.
    for id in [ServiceId::Php, ServiceId::Nodejs, ServiceId::Composer] {
        if settings.services[&id].active_version.is_some()
            && id
                .definition()
                .dependencies
                .iter()
                .all(|dependency| settings.services[dependency].active_version.is_some())
        {
            writes.extend(toolchain::layout(storage, &settings, id)?.files);
        }
    }
    let registrations = storage.root.join("config/path/registrations.json");
    let removals = files
        .keys()
        .filter(|path| {
            **path != registrations && !writes.iter().any(|(written, _)| written == *path)
        })
        .cloned()
        .collect();
    Ok(ResetPlan {
        settings,
        changes: ConfigChanges { writes, removals },
        files,
        settings_before: fs::read(storage.root.join("settings.json"))
            .at(Op::Read, &storage.root.join("settings.json"))?,
    })
}

impl ResetPlan {
    pub fn backup(&self, storage: &Storage) -> Result<PathBuf> {
        let backups = storage.root.join("backups");
        regular(&backups)?;
        let path = backups.join(format!("configuration-reset-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).at(Op::Create, &path)?;
        platform::secure_directory(&path)?;
        let settings_backup = path.join("settings.json");
        atomic_write(&settings_backup, &self.settings_before)?;
        if fs::read(&settings_backup).at(Op::Verify, &settings_backup)? != self.settings_before {
            return Err(invalid("Settings backup verification failed"));
        }
        for (source, bytes) in &self.files {
            let relative = source
                .strip_prefix(&storage.root)
                .map_err(|_| invalid("Configuration backup is outside managed storage"))?;
            let target = path.join(relative);
            atomic_write(&target, bytes)?;
            if fs::read(&target).at(Op::Verify, &target)? != *bytes {
                return Err(invalid("Configuration backup verification failed"));
            }
        }
        self.verify(storage)?;
        Ok(path)
    }

    pub fn verify(&self, storage: &Storage) -> Result<()> {
        if snapshot(storage)? != self.files
            || fs::read(storage.root.join("settings.json"))? != self.settings_before
        {
            return Err(invalid(
                "Configuration changed during reset preparation. Close external editors and try again.",
            ));
        }
        Ok(())
    }
}
