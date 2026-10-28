use crate::{
    error::{Result, invalid},
    model::{ServiceId, Settings},
    storage::{Storage, atomic_json},
    toolchain,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Registration {
    directory: String,
    owned: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum RegisteredPaths {
    Legacy(Registration),
    Multiple(Vec<Registration>),
}
impl RegisteredPaths {
    fn entries(&self) -> &[Registration] {
        match self {
            Self::Legacy(entry) => std::slice::from_ref(entry),
            Self::Multiple(entries) => entries,
        }
    }
}
type Registrations = BTreeMap<ServiceId, RegisteredPaths>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PathValue {
    text: String,
    expandable: bool,
}

trait Environment {
    fn read(&self) -> Result<Option<PathValue>>;
    fn write(&self, value: Option<&PathValue>) -> Result<()>;
}

#[derive(Serialize, Deserialize)]
struct Journal {
    revision: u64,
    before: Option<PathValue>,
    after: Option<PathValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathStatus {
    pub enabled: bool,
    pub present: bool,
    pub directory: Option<PathBuf>,
    pub version: Option<String>,
    pub command: String,
    pub first_match: Option<PathBuf>,
    pub shadowed: bool,
    pub external: bool,
    pub php_config: Option<PathBuf>,
    pub directories: Vec<PathBuf>,
    pub commands: Vec<CommandStatus>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandStatus {
    pub command: String,
    pub first_match: Option<PathBuf>,
    pub shadowed: bool,
}

fn registrations(storage: &Storage) -> Result<Registrations> {
    let path = storage.root.join("config/path/registrations.json");
    if path.exists() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        Ok(BTreeMap::new())
    }
}

fn key(entry: &str) -> String {
    key_with_expansion(entry, true)
}

fn key_with_expansion(entry: &str, expandable: bool) -> String {
    let entry = entry.trim().trim_matches('"').replace('/', "\\");
    let expanded = if expandable { expand(&entry) } else { entry };
    let expanded = expanded.strip_prefix("\\\\?\\").unwrap_or(&expanded);
    let mut normalized = PathBuf::new();
    for component in Path::new(expanded).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_lowercase()
}

fn contains(value: &str, directory: &str) -> bool {
    contains_typed(value, directory, true)
}

fn contains_typed(value: &str, directory: &str, expandable: bool) -> bool {
    let directory = key(directory);
    value
        .split(';')
        .any(|entry| !entry.trim().is_empty() && key_with_expansion(entry, expandable) == directory)
}

fn remove(value: &str, directory: &str) -> String {
    remove_typed(value, directory, true)
}

fn remove_typed(value: &str, directory: &str, expandable: bool) -> String {
    let directory = key(directory);
    value
        .split(';')
        .filter(|entry| {
            entry.trim().is_empty() || key_with_expansion(entry, expandable) != directory
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn reconcile(
    value: Option<PathValue>,
    records: &mut Registrations,
    storage: &Storage,
    settings: &Settings,
    affected: &[ServiceId],
) -> Result<Option<PathValue>> {
    let expandable = value.as_ref().is_none_or(|value| value.expandable);
    let mut text = value
        .as_ref()
        .map(|value| value.text.clone())
        .unwrap_or_default();
    for id in affected {
        let previous = records.remove(id);
        let desired = if settings.services[id].add_to_path {
            toolchain::layout(storage, settings, *id)?.directories
        } else {
            Vec::new()
        };
        if let Some(previous) = &previous {
            for entry in previous.entries().iter().filter(|entry| entry.owned) {
                text = remove_typed(&text, &entry.directory, expandable);
            }
        }
        let mut entries = Vec::new();
        // Prepending in reverse preserves adapter -> runtime -> global-bin order.
        for directory in desired.into_iter().rev() {
            let directory = directory.to_string_lossy().into_owned();
            let present = contains_typed(&text, &directory, expandable);
            if !present {
                text = if text.is_empty() {
                    directory.clone()
                } else {
                    format!("{directory};{text}")
                };
            }
            entries.push(Registration {
                directory,
                owned: !present,
            });
        }
        if !entries.is_empty() {
            entries.reverse();
            records.insert(*id, RegisteredPaths::Multiple(entries));
        }
    }
    if text.encode_utf16().count() >= 32767 {
        return Err(invalid(
            "The user PATH would exceed the Windows environment limit",
        ));
    }
    if value.is_none() && text.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathValue { text, expandable }))
}

// Restore our delta only if someone else changed PATH after our write. Never
// replace an external edit with a stale snapshot during recovery.
fn rollback(environment: &impl Environment, journal: &Journal) -> Result<()> {
    let current = environment.read()?;
    if current == journal.after {
        return environment.write(journal.before.as_ref());
    }
    let before = journal
        .before
        .as_ref()
        .map(|value| value.text.as_str())
        .unwrap_or_default();
    let after = journal
        .after
        .as_ref()
        .map(|value| value.text.as_str())
        .unwrap_or_default();
    let mut value = current.unwrap_or(PathValue {
        text: String::new(),
        expandable: true,
    });
    for entry in after
        .split(';')
        .filter(|entry| !entry.is_empty() && !contains(before, entry))
    {
        value.text = remove(&value.text, entry);
    }
    for entry in before
        .split(';')
        .filter(|entry| !entry.is_empty() && !contains(after, entry))
    {
        if !contains(&value.text, entry) {
            value.text = if value.text.is_empty() {
                entry.into()
            } else {
                format!("{entry};{}", value.text)
            };
        }
    }
    environment.write(Some(&value))
}

fn transact(
    environment: &impl Environment,
    storage: &Storage,
    previous: &Settings,
    settings: &mut Settings,
    changes: impl Into<crate::storage::ConfigChanges>,
    refresh: Option<ServiceId>,
) -> Result<bool> {
    let mut changes = changes.into();
    let affected: Vec<_> = ServiceId::ALL
        .into_iter()
        .filter(|id| toolchain::changed(previous, settings, *id, refresh))
        .collect();
    if affected.is_empty() {
        storage.apply(settings, &changes)?;
        return Ok(false);
    }
    // Removing the last version also unregisters it; no dangling PATH entry.
    for id in &affected {
        if settings.services[id].active_version.is_none() {
            settings.services.get_mut(id).unwrap().add_to_path = false;
        }
        if settings.services[id].add_to_path {
            changes
                .writes
                .extend(toolchain::layout(storage, settings, *id)?.files);
        }
    }
    let before = environment.read()?;
    let mut records = registrations(storage)?;
    let after = reconcile(before.clone(), &mut records, storage, settings, &affected)?;
    let pending = storage.root.join("config/path/pending.json");
    let journal = Journal {
        revision: settings.revision,
        before,
        after,
    };
    changes.writes.push((
        storage.root.join("config/path/registrations.json"),
        serde_json::to_string_pretty(&records)?,
    ));
    atomic_json(&pending, &journal)?;
    let mut applied = false;
    let result = (|| {
        // Re-read immediately before writing to detect external registry editors.
        if environment.read()? != journal.before {
            return Err(invalid(
                "The user PATH changed during this operation. Retry to preserve the latest entries.",
            ));
        }
        if journal.before != journal.after {
            environment.write(journal.after.as_ref())?;
            applied = true;
        }
        storage.apply(settings, &changes)
    })();
    if let Err(error) = result {
        if applied {
            rollback(environment, &journal).map_err(|rollback| {
                invalid(format!("{error}. PATH recovery failed: {rollback}"))
            })?;
        }
        fs::remove_file(pending)?;
        return Err(error);
    }
    let _ = fs::remove_file(pending);
    Ok(journal.before != journal.after)
}

#[cfg(windows)]
mod windows_environment {
    use super::*;
    use windows::{
        Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*},
        core::w,
    };
    use winreg::{RegKey, RegValue, enums::*};

    pub(super) struct Registry(pub RegKey);
    impl Registry {
        pub fn user() -> Result<Self> {
            Ok(Self(
                RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?,
            ))
        }
        pub fn user_read() -> Result<Self> {
            Ok(Self(
                RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment")?,
            ))
        }
    }
    impl Environment for Registry {
        fn read(&self) -> Result<Option<PathValue>> {
            match self.0.get_raw_value("Path") {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
                Ok(value) => {
                    if !matches!(value.vtype, REG_SZ | REG_EXPAND_SZ) || value.bytes.len() % 2 != 0
                    {
                        return Err(invalid("The user PATH is not a valid registry string"));
                    }
                    let mut wide: Vec<_> = value
                        .bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    if wide.last() == Some(&0) {
                        wide.pop();
                    }
                    if wide.contains(&0) {
                        return Err(invalid("The user PATH contains embedded NUL characters"));
                    }
                    Ok(Some(PathValue {
                        text: String::from_utf16(&wide)
                            .map_err(|_| invalid("The user PATH contains invalid UTF-16"))?,
                        expandable: value.vtype == REG_EXPAND_SZ,
                    }))
                }
            }
        }
        fn write(&self, value: Option<&PathValue>) -> Result<()> {
            match value {
                Some(value) => self.0.set_raw_value(
                    "Path",
                    &RegValue {
                        vtype: if value.expandable {
                            REG_EXPAND_SZ
                        } else {
                            REG_SZ
                        },
                        bytes: value
                            .text
                            .encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect(),
                    },
                )?,
                None => match self.0.delete_value("Path") {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
            Ok(())
        }
    }
    pub(super) struct Lock(HANDLE);
    impl Lock {
        pub fn acquire() -> Result<Self> {
            unsafe {
                let handle = CreateMutexW(None, false, w!("Local\\OpenLarager.UserPath"))
                    .map_err(|error| invalid(error.to_string()))?;
                match WaitForSingleObject(handle, 5000) {
                    WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
                    _ => {
                        let _ = CloseHandle(handle);
                        Err(invalid(
                            "Another Open Larager window is updating PATH. Retry shortly.",
                        ))
                    }
                }
            }
        }
    }
    impl Drop for Lock {
        fn drop(&mut self) {
            unsafe {
                let _ = ReleaseMutex(self.0);
                let _ = CloseHandle(self.0);
            }
        }
    }
    pub(super) fn notify() {
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("Environment").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                100,
                None,
            );
        }
    }

    pub(super) fn machine_path() -> Result<PathValue> {
        let key = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")?;
        Ok(Registry(key).read()?.unwrap_or(PathValue {
            text: String::new(),
            expandable: true,
        }))
    }
}

#[cfg(windows)]
fn expand(entry: &str) -> String {
    use windows::{Win32::System::Environment::ExpandEnvironmentStringsW, core::PCWSTR};
    let source: Vec<_> = entry.encode_utf16().chain(Some(0)).collect();
    let mut result = vec![0; 32768];
    let length =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut result)) } as usize;
    if length == 0 || length > result.len() {
        return entry.into();
    }
    String::from_utf16_lossy(&result[..length - 1])
}
#[cfg(not(windows))]
fn expand(entry: &str) -> String {
    entry.into()
}

pub fn transaction(
    storage: &Storage,
    previous: &Settings,
    settings: &mut Settings,
    changes: Vec<(PathBuf, String)>,
    refresh: Option<ServiceId>,
) -> Result<()> {
    transaction_changes(storage, previous, settings, changes.into(), refresh)
}

pub fn transaction_changes(
    storage: &Storage,
    previous: &Settings,
    settings: &mut Settings,
    changes: crate::storage::ConfigChanges,
    refresh: Option<ServiceId>,
) -> Result<()> {
    let relevant = ServiceId::ALL
        .iter()
        .any(|id| toolchain::changed(previous, settings, *id, refresh));
    if !relevant {
        return storage.apply(settings, &changes);
    }
    #[cfg(windows)]
    {
        let lock = windows_environment::Lock::acquire()?;
        let changed = transact(
            &windows_environment::Registry::user()?,
            storage,
            previous,
            settings,
            changes,
            refresh,
        )?;
        drop(lock);
        if changed {
            windows_environment::notify();
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(invalid(
            "User PATH registration is supported on Windows only",
        ))
    }
}

/// Reconcile all opted-in commands after changing the environment root. The
/// copied registrations still identify the old entries owned by this app.
pub fn relocate(storage: &Storage, settings: &mut Settings) -> Result<()> {
    let mut previous = settings.clone();
    for service in previous.services.values_mut() {
        if service.add_to_path {
            service.add_to_path = false;
        }
    }
    let mut next = settings.clone();
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("Settings revision overflow"))?;
    transaction(storage, &previous, &mut next, Vec::new(), None)?;
    *settings = next;
    Ok(())
}

pub fn recover(storage: &Storage, settings: &Settings) -> Result<()> {
    let pending = storage.root.join("config/path/pending.json");
    if !pending.exists() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let lock = windows_environment::Lock::acquire()?;
        recover_environment(
            &windows_environment::Registry::user()?,
            storage,
            settings.revision,
        )?;
        drop(lock);
        windows_environment::notify();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(invalid("Recover this pending PATH operation on Windows"))
    }
}

fn has_legacy_php_path(storage: &Storage, settings: &Settings) -> Result<bool> {
    let php = &settings.services[&ServiceId::Php];
    if !php.add_to_path {
        return Ok(false);
    }
    Ok(registrations(storage)?
        .get(&ServiceId::Php)
        .is_some_and(|record| {
            record.entries().iter().any(|entry| {
                php.installations.iter().any(|installation| {
                    key(&entry.directory)
                        == key(&storage
                            .package(ServiceId::Php, &installation.version)
                            .to_string_lossy())
                })
            })
        }))
}

/// Upgrade existing opt-in registrations through the same recoverable PATH
/// transaction as a version switch, preserving externally owned entries.
pub fn upgrade_php_cli(storage: &Storage, settings: &mut Settings) -> Result<()> {
    if !has_legacy_php_path(storage, settings)? {
        return Ok(());
    }
    let previous = settings.clone();
    let mut next = previous.clone();
    next.revision = previous
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("Settings revision overflow"))?;
    transaction(
        storage,
        &previous,
        &mut next,
        Vec::new(),
        Some(ServiceId::Php),
    )?;
    *settings = next;
    Ok(())
}

fn recover_environment(
    environment: &impl Environment,
    storage: &Storage,
    revision: u64,
) -> Result<()> {
    let pending = storage.root.join("config/path/pending.json");
    let journal: Journal = serde_json::from_slice(&fs::read(&pending)?)?;
    if revision < journal.revision {
        rollback(environment, &journal)?;
    }
    fs::remove_file(pending)?;
    Ok(())
}

pub fn status(storage: &Storage, settings: &Settings, id: ServiceId) -> Result<PathStatus> {
    let service = &settings.services[&id];
    let layout = if service.active_version.is_some() {
        Some(toolchain::layout(storage, settings, id)?)
    } else {
        None
    };
    #[cfg(windows)]
    let (user, machine) = {
        use windows_environment::*;
        (
            Registry::user_read()?.read()?.unwrap_or(PathValue {
                text: String::new(),
                expandable: true,
            }),
            machine_path()?,
        )
    };
    #[cfg(not(windows))]
    let (user, machine) = (
        PathValue {
            text: String::new(),
            expandable: true,
        },
        PathValue {
            text: String::new(),
            expandable: true,
        },
    );
    let command = layout
        .as_ref()
        .and_then(|layout| layout.commands.first())
        .map(|(command, _)| command.clone())
        .unwrap_or_else(|| {
            Path::new(id.cli_executable())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        });
    let search_directories: Vec<_> = machine
        .text
        .split(';')
        .map(|entry| (entry, machine.expandable))
        .chain(user.text.split(';').map(|entry| (entry, user.expandable)))
        .filter(|(entry, _)| !entry.trim().is_empty())
        .map(|(entry, expandable)| {
            let entry = entry.trim().trim_matches('"');
            PathBuf::from(if expandable {
                expand(entry)
            } else {
                entry.into()
            })
        })
        .filter(|path| path.is_absolute())
        .collect();
    let directories = layout
        .as_ref()
        .map(|layout| layout.directories.clone())
        .unwrap_or_default();
    let commands: Vec<_> = layout
        .as_ref()
        .map(|layout| {
            layout
                .commands
                .iter()
                .map(|(command, expected)| {
                    let stem = Path::new(command).file_stem().unwrap().to_string_lossy();
                    let first_match = search_directories.iter().find_map(|directory| {
                        ["com", "exe", "bat", "cmd", "ps1"]
                            .into_iter()
                            .map(|extension| directory.join(format!("{stem}.{extension}")))
                            .find(|path| path.is_file())
                    });
                    let shadowed = first_match.as_ref().is_some_and(|first| {
                        key(&first.to_string_lossy()) != key(&expected.to_string_lossy())
                    });
                    CommandStatus {
                        command: command.clone(),
                        first_match,
                        shadowed,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let primary = commands.iter().find(|status| status.command == command);
    let first_match = primary.and_then(|status| status.first_match.clone());
    let shadowed = primary.is_some_and(|status| status.shadowed);
    let present = !directories.is_empty()
        && directories
            .iter()
            .all(|path| contains_typed(&user.text, &path.to_string_lossy(), user.expandable))
        && layout.as_ref().is_some_and(|layout| {
            layout.files.iter().all(|(path, expected)| {
                fs::read_to_string(path).is_ok_and(|content| content == *expected)
            })
        });
    let external = registrations(storage)?
        .get(&id)
        .is_some_and(|record| record.entries().iter().any(|entry| !entry.owned));
    Ok(PathStatus {
        enabled: service.add_to_path,
        present,
        directory: directories.first().cloned(),
        directories,
        commands,
        version: service.active_version.clone(),
        command,
        first_match,
        shadowed,
        external,
        php_config: if id == ServiceId::Php {
            service
                .active_version
                .as_ref()
                .map(|version| storage.config(id, version))
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Fake {
        value: RefCell<Option<PathValue>>,
        reads: Cell<usize>,
    }
    impl Fake {
        fn new(text: &str, expandable: bool) -> Self {
            Self {
                value: RefCell::new(Some(PathValue {
                    text: text.into(),
                    expandable,
                })),
                reads: Cell::new(0),
            }
        }
    }
    impl Environment for Fake {
        fn read(&self) -> Result<Option<PathValue>> {
            self.reads.set(self.reads.get() + 1);
            Ok(self.value.borrow().clone())
        }
        fn write(&self, value: Option<&PathValue>) -> Result<()> {
            *self.value.borrow_mut() = value.cloned();
            Ok(())
        }
    }
    fn installed(storage: &Storage) -> Settings {
        storage.load().unwrap();
        let mut settings = Settings::default();
        let service = settings.services.get_mut(&ServiceId::Php).unwrap();
        for version in ["8.3.0", "8.4.0"] {
            let directory = storage.package(ServiceId::Php, version);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("php.exe"), b"test fixture; never executed").unwrap();
            service.installations.push(crate::model::Installation {
                version: version.into(),
                installed_at: 0,
                sha256: "a".repeat(64),
            });
        }
        service.active_version = Some("8.3.0".into());
        settings
    }
    fn update(fake: &Fake, storage: &Storage, previous: &Settings, next: &mut Settings) {
        next.revision = previous.revision + 1;
        transact(
            fake,
            storage,
            previous,
            next,
            Vec::new(),
            Some(ServiceId::Php),
        )
        .unwrap();
    }

    fn install_tool(storage: &Storage, settings: &mut Settings, id: ServiceId, versions: &[&str]) {
        for version in versions {
            let package = storage.package(id, version);
            for file in &id.definition().required_files {
                let file = package.join(file);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, b"fixture; never executed").unwrap();
            }
            storage.prepare(id, version).unwrap();
            toolchain::prepare(storage, id, version).unwrap();
            settings.services.get_mut(&id).unwrap().installations.push(
                crate::model::Installation {
                    version: (*version).into(),
                    installed_at: 0,
                    sha256: "a".repeat(64),
                },
            );
        }
        settings.services.get_mut(&id).unwrap().active_version = Some(versions[0].into());
    }

    #[test]
    fn toolchains_follow_versions_and_dependencies_with_multiple_owned_paths() {
        let workspace = tempfile::Builder::new()
            .prefix("Larager á ! & ")
            .tempdir()
            .unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let mut previous = installed(&storage);
        install_tool(
            &storage,
            &mut previous,
            ServiceId::Nodejs,
            &["24.0.0", "22.0.0"],
        );
        install_tool(
            &storage,
            &mut previous,
            ServiceId::Composer,
            &["2.10.0", "2.9.0"],
        );
        let fake = Fake::new("C:\\External;;%SystemRoot%\\System32", true);
        let original = fake.read().unwrap();
        let mut first = previous.clone();
        for id in [ServiceId::Nodejs, ServiceId::Composer] {
            first.services.get_mut(&id).unwrap().add_to_path = true;
        }
        first.revision += 1;
        transact(&fake, &storage, &previous, &mut first, Vec::new(), None).unwrap();
        let node_layout = toolchain::layout(&storage, &first, ServiceId::Nodejs).unwrap();
        let value = fake.read().unwrap().unwrap().text;
        let positions: Vec<_> = node_layout
            .directories
            .iter()
            .map(|path| value.find(&*path.to_string_lossy()).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let npm = storage.root.join("config/cli/nodejs/npm.cmd");
        let composer = storage.root.join("config/cli/composer/composer.cmd");
        assert!(
            fs::read_to_string(&npm)
                .unwrap()
                .contains("packages\\nodejs\\24.0.0\\node.exe")
        );
        let content = fs::read_to_string(&composer).unwrap();
        assert!(
            content.is_ascii(),
            "Launcher source must not depend on the terminal code page"
        );
        assert!(content.contains("packages\\php\\8.3.0\\php.exe"));
        assert!(content.contains("config\\php\\8.3.0\\php.ini"));
        assert!(!first.services[&ServiceId::Php].add_to_path);
        let saved = storage
            .data(ServiceId::Nodejs, "24.0.0")
            .join("global/keep.txt");
        fs::write(&saved, "user package").unwrap();
        let mut second = first.clone();
        for (id, version) in [
            (ServiceId::Nodejs, "22.0.0"),
            (ServiceId::Php, "8.4.0"),
            (ServiceId::Composer, "2.9.0"),
        ] {
            second.services.get_mut(&id).unwrap().active_version = Some(version.into());
        }
        second.revision += 1;
        transact(&fake, &storage, &first, &mut second, Vec::new(), None).unwrap();
        assert!(
            fs::read_to_string(&npm)
                .unwrap()
                .contains("packages\\nodejs\\22.0.0\\node.exe")
        );
        let content = fs::read_to_string(&composer).unwrap();
        assert!(content.contains("packages\\php\\8.4.0\\php.exe"));
        assert!(content.contains("packages\\composer\\2.9.0\\composer.phar"));
        assert!(!fake.read().unwrap().unwrap().text.contains("24.0.0"));
        let mut removed = second.clone();
        for id in [ServiceId::Nodejs, ServiceId::Composer] {
            removed.services.get_mut(&id).unwrap().active_version = None;
        }
        removed.revision += 1;
        transact(&fake, &storage, &second, &mut removed, Vec::new(), None).unwrap();
        assert_eq!(fake.read().unwrap(), original);
        assert_eq!(fs::read_to_string(saved).unwrap(), "user package");
    }

    #[test]
    fn composer_dependency_changes_are_atomic_and_last_php_cannot_leave_a_broken_launcher() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let mut previous = installed(&storage);
        install_tool(&storage, &mut previous, ServiceId::Composer, &["2.10.0"]);
        let fake = Fake::new("C:\\External", false);
        let mut first = previous.clone();
        first
            .services
            .get_mut(&ServiceId::Composer)
            .unwrap()
            .add_to_path = true;
        first.revision += 1;
        transact(&fake, &storage, &previous, &mut first, Vec::new(), None).unwrap();
        let launcher = storage.root.join("config/cli/composer/composer.cmd");
        let before = fs::read(&launcher).unwrap();
        let path_before = fake.read().unwrap();
        let mut second = first.clone();
        second
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .active_version = Some("8.4.0".into());
        second.revision += 1;
        assert!(
            transact(
                &fake,
                &storage,
                &first,
                &mut second,
                vec![(storage.root.join("not-config"), "bad".into())],
                None
            )
            .is_err()
        );
        assert_eq!(fs::read(&launcher).unwrap(), before);
        assert_eq!(fake.read().unwrap(), path_before);
        second
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .active_version = None;
        assert!(transact(&fake, &storage, &first, &mut second, Vec::new(), None).is_err());
        assert_eq!(fake.read().unwrap(), path_before);
        assert_eq!(fs::read(&launcher).unwrap(), before);
        second
            .services
            .get_mut(&ServiceId::Composer)
            .unwrap()
            .add_to_path = false;
        transact(&fake, &storage, &first, &mut second, Vec::new(), None).unwrap();
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\External");
    }

    #[test]
    fn reads_legacy_single_path_ownership_records() {
        let records: Registrations =
            serde_json::from_str(r#"{"php":{"directory":"C:/PHP","owned":false}}"#).unwrap();
        assert!(!records[&ServiceId::Php].entries()[0].owned);
    }

    #[test]
    fn relocation_replaces_only_owned_old_paths_and_recovers_registry_failures() {
        let temp = tempfile::tempdir().unwrap();
        let source = Storage::open(temp.path().join("old")).unwrap();
        let mut settings = installed(&source);
        install_tool(&source, &mut settings, ServiceId::Nodejs, &["24.0.0"]);
        let external_php = toolchain::layout(&source, &settings, ServiceId::Php)
            .unwrap()
            .directories[0]
            .to_string_lossy()
            .into_owned();
        let original = format!("C:\\Other;;%SystemRoot%\\System32;{external_php}");
        let fake = Fake::new(&original, false);
        let previous = settings.clone();
        settings
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = true;
        settings
            .services
            .get_mut(&ServiceId::Nodejs)
            .unwrap()
            .add_to_path = true;
        settings.revision += 1;
        transact(&fake, &source, &previous, &mut settings, Vec::new(), None).unwrap();
        let old_node = toolchain::layout(&source, &settings, ServiceId::Nodejs)
            .unwrap()
            .directories[0]
            .to_string_lossy()
            .into_owned();

        let target = Storage::open(temp.path().join("new")).unwrap();
        installed(&target);
        let mut target_settings = settings.clone();
        install_tool(
            &target,
            &mut target_settings,
            ServiceId::Nodejs,
            &["24.0.0"],
        );
        target.save(&target_settings).unwrap();
        atomic_json(
            &target.root.join("config/path/registrations.json"),
            &registrations(&source).unwrap(),
        )
        .unwrap();
        let mut before = target_settings.clone();
        for service in before.services.values_mut() {
            service.add_to_path = false;
        }
        target_settings.revision += 1;
        let path_before = fake.read().unwrap();
        assert!(
            transact(
                &fake,
                &target,
                &before,
                &mut target_settings,
                vec![(target.root.join("outside-config"), "invalid".into())],
                None
            )
            .is_err()
        );
        assert_eq!(fake.read().unwrap(), path_before);
        transact(
            &fake,
            &target,
            &before,
            &mut target_settings,
            Vec::new(),
            None,
        )
        .unwrap();
        let value = fake.read().unwrap().unwrap();
        assert!(!value.expandable);
        assert!(
            value.text.ends_with(&original),
            "externally owned entries must survive"
        );
        assert!(!contains(&value.text, &old_node));
        for id in [ServiceId::Php, ServiceId::Nodejs] {
            for directory in toolchain::layout(&target, &target_settings, id)
                .unwrap()
                .directories
            {
                assert!(contains(&value.text, &directory.to_string_lossy()));
            }
        }
    }

    #[test]
    fn follows_active_version_deduplicates_and_preserves_unrelated_entries_and_type() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let original = "C:\\Other;;\"C:\\Case\\\";%SystemRoot%\\System32";
        let fake = Fake::new(original, false);
        let previous = installed(&storage);
        let mut first = previous.clone();
        first.services.get_mut(&ServiceId::Php).unwrap().add_to_path = true;
        update(&fake, &storage, &previous, &mut first);
        let directory = toolchain::layout(&storage, &first, ServiceId::Php)
            .map(|layout| layout.directories[0].clone())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let current = fake.read().unwrap().unwrap();
        assert!(!current.expandable);
        assert!(current.text.ends_with(original));
        // A duplicate with Windows-equivalent spelling is collapsed on repair.
        fake.value
            .borrow_mut()
            .as_mut()
            .unwrap()
            .text
            .push_str(&format!(
                ";\"{}/\"",
                directory.to_uppercase().replace('\\', "/")
            ));
        let mut repaired = first.clone();
        update(&fake, &storage, &first, &mut repaired);
        assert_eq!(
            fake.read()
                .unwrap()
                .unwrap()
                .text
                .split(';')
                .filter(|entry| key(entry) == key(&directory))
                .count(),
            1
        );
        let mut second = repaired.clone();
        second
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .active_version = Some("8.4.0".into());
        update(&fake, &storage, &repaired, &mut second);
        let value = fake.read().unwrap().unwrap();
        assert!(contains(&value.text, &directory));
        let launcher = fs::read_to_string(storage.root.join("config/cli/php/php.cmd")).unwrap();
        assert!(launcher.is_ascii());
        assert!(launcher.contains("packages\\php\\8.4.0\\php.exe"));
        assert!(launcher.contains("config\\php\\8.4.0\\php.ini"));
        assert!(!launcher.contains("8.3.0"));
        let mut disabled = second.clone();
        disabled
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = false;
        update(&fake, &storage, &second, &mut disabled);
        assert_eq!(
            fake.read().unwrap().unwrap(),
            PathValue {
                text: original.into(),
                expandable: false
            }
        );
    }

    #[test]
    fn upgrades_legacy_php_registration_atomically_and_preserves_external_paths_and_config() {
        for owned in [true, false] {
            let workspace = tempfile::tempdir().unwrap();
            let storage = Storage::open(workspace.path().to_owned()).unwrap();
            let mut previous = installed(&storage);
            previous
                .services
                .get_mut(&ServiceId::Php)
                .unwrap()
                .add_to_path = true;
            let native = storage.package(ServiceId::Php, "8.3.0");
            let original = format!("{};C:\\External", native.display());
            let fake = Fake::new(&original, true);
            atomic_json(
                &storage.root.join("config/path/registrations.json"),
                &BTreeMap::from([(
                    ServiceId::Php,
                    Registration {
                        directory: native.to_string_lossy().into_owned(),
                        owned,
                    },
                )]),
            )
            .unwrap();
            storage.prepare(ServiceId::Php, "8.3.0").unwrap();
            let config = storage.config(ServiceId::Php, "8.3.0");
            fs::write(&config, "extension=pdo_pgsql\nmemory_limit=512M\n").unwrap();
            let config_before = fs::read(&config).unwrap();
            assert!(has_legacy_php_path(&storage, &previous).unwrap());
            let mut next = previous.clone();
            next.revision += 1;
            let launcher = storage.root.join("config/cli/php/php.cmd");
            assert!(
                transact(
                    &fake,
                    &storage,
                    &previous,
                    &mut next,
                    vec![(storage.root.join("outside-config"), "bad".into())],
                    Some(ServiceId::Php),
                )
                .is_err()
            );
            assert_eq!(fake.read().unwrap().unwrap().text, original);
            assert!(!launcher.exists());
            assert!(has_legacy_php_path(&storage, &previous).unwrap());
            update(&fake, &storage, &previous, &mut next);
            let value = fake.read().unwrap().unwrap().text;
            assert!(contains(
                &value,
                &launcher.parent().unwrap().to_string_lossy()
            ));
            assert_eq!(contains(&value, &native.to_string_lossy()), !owned);
            assert!(value.ends_with("C:\\External"));
            assert!(!has_legacy_php_path(&storage, &next).unwrap());
            assert_eq!(fs::read(&config).unwrap(), config_before);
            let mut disabled = next.clone();
            disabled
                .services
                .get_mut(&ServiceId::Php)
                .unwrap()
                .add_to_path = false;
            update(&fake, &storage, &next, &mut disabled);
            assert_eq!(
                fake.read().unwrap().unwrap().text,
                if owned { "C:\\External" } else { &original }
            );
        }
    }

    #[test]
    fn never_owns_or_removes_a_preexisting_entry() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let previous = installed(&storage);
        let directory = toolchain::layout(&storage, &previous, ServiceId::Php)
            .map(|layout| layout.directories[0].clone())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let original = format!("C:\\External;\"{}\\\"", directory.to_uppercase());
        let fake = Fake::new(&original, true);
        let mut enabled = previous.clone();
        enabled
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = true;
        update(&fake, &storage, &previous, &mut enabled);
        assert!(!registrations(&storage).unwrap()[&ServiceId::Php].entries()[0].owned);
        let mut disabled = enabled.clone();
        disabled
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = false;
        update(&fake, &storage, &enabled, &mut disabled);
        assert_eq!(fake.read().unwrap().unwrap().text, original);
    }

    #[test]
    fn rolls_back_path_when_settings_cannot_be_committed_and_unregisters_last_version() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let previous = installed(&storage);
        let fake = Fake::new("C:\\External", true);
        let mut enabled = previous.clone();
        enabled.revision += 1;
        enabled
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = true;
        let error = transact(
            &fake,
            &storage,
            &previous,
            &mut enabled,
            vec![(storage.root.join("outside-config"), "bad".into())],
            None,
        );
        assert!(error.is_err());
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\External");
        assert!(!storage.root.join("config/path/pending.json").exists());
        update(&fake, &storage, &previous, &mut enabled);
        let mut removed = enabled.clone();
        let service = removed.services.get_mut(&ServiceId::Php).unwrap();
        service.active_version = None;
        service.installations.clear();
        update(&fake, &storage, &enabled, &mut removed);
        assert!(!removed.services[&ServiceId::Php].add_to_path);
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\External");
    }

    #[test]
    fn configuration_reset_unregisters_only_owned_path_entries_and_removes_custom_files() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let previous = installed(&storage);
        let fake = Fake::new("C:\\External;C:\\Other", true);
        let mut enabled = previous.clone();
        enabled
            .services
            .get_mut(&ServiceId::Php)
            .unwrap()
            .add_to_path = true;
        update(&fake, &storage, &previous, &mut enabled);
        let obsolete = storage.root.join("config/custom/obsolete.ini");
        crate::storage::atomic_write(&obsolete, b"custom setting").unwrap();
        let mut plan = crate::reset::prepare(&storage, &enabled).unwrap();
        let backup = plan.backup(&storage).unwrap();
        transact(
            &fake,
            &storage,
            &enabled,
            &mut plan.settings,
            plan.changes,
            None,
        )
        .unwrap();
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\External;C:\\Other");
        assert!(registrations(&storage).unwrap().is_empty());
        assert!(!obsolete.exists());
        assert_eq!(
            fs::read(backup.join("config/custom/obsolete.ini")).unwrap(),
            b"custom setting"
        );
        assert_eq!(
            plan.settings.services[&ServiceId::Php].active_version,
            enabled.services[&ServiceId::Php].active_version
        );
    }

    #[test]
    fn recovery_preserves_concurrent_external_edits_and_opt_out_does_not_read_registry() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let previous = installed(&storage);
        let fake = Fake::new("C:\\New;C:\\Other;C:\\Concurrent", true);
        let journal = Journal {
            revision: 1,
            before: Some(PathValue {
                text: "C:\\Old;C:\\Other".into(),
                expandable: true,
            }),
            after: Some(PathValue {
                text: "C:\\New;C:\\Other".into(),
                expandable: true,
            }),
        };
        rollback(&fake, &journal).unwrap();
        assert_eq!(
            fake.read().unwrap().unwrap().text,
            "C:\\Old;C:\\Other;C:\\Concurrent"
        );
        fake.reads.set(0);
        let mut next = previous.clone();
        next.revision += 1;
        transact(&fake, &storage, &previous, &mut next, Vec::new(), None).unwrap();
        assert_eq!(fake.reads.get(), 0);
    }

    #[test]
    fn refuses_missing_cli_and_unsafe_path_entries_before_writing() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let previous = installed(&storage);
        fs::remove_file(storage.package(ServiceId::Php, "8.3.0").join("php.exe")).unwrap();
        let mut next = previous.clone();
        next.services.get_mut(&ServiceId::Php).unwrap().add_to_path = true;
        let fake = Fake::new("C:\\Other", true);
        assert!(transact(&fake, &storage, &previous, &mut next, Vec::new(), None).is_err());
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\Other");
    }

    #[test]
    fn recovers_only_uncommitted_registry_changes() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let pending = storage.root.join("config/path/pending.json");
        let fake = Fake::new("C:\\Managed;C:\\External", true);
        let journal = Journal {
            revision: 2,
            before: Some(PathValue {
                text: "C:\\External".into(),
                expandable: true,
            }),
            after: fake.read().unwrap(),
        };
        atomic_json(&pending, &journal).unwrap();
        recover_environment(&fake, &storage, 1).unwrap();
        assert_eq!(fake.read().unwrap().unwrap().text, "C:\\External");
        fake.write(journal.after.as_ref()).unwrap();
        atomic_json(&pending, &journal).unwrap();
        recover_environment(&fake, &storage, 2).unwrap();
        assert_eq!(fake.read().unwrap(), journal.after);
        assert!(!pending.exists());
    }

    #[cfg(windows)]
    #[test]
    fn normalizes_expandable_paths_without_expanding_literal_registry_strings() {
        let system = std::env::var("SystemRoot").unwrap();
        assert_eq!(
            key("\"%SystemRoot%/System32/../System32/\""),
            key(&format!("{system}\\System32"))
        );
        assert!(contains_typed(
            "%SystemRoot%/System32",
            &format!("{system}\\System32"),
            true
        ));
        assert!(!contains_typed(
            "%SystemRoot%/System32",
            &format!("{system}\\System32"),
            false
        ));
    }

    #[cfg(windows)]
    #[test]
    fn round_trips_windows_registry_types_and_long_values_in_an_isolated_key() {
        use windows_environment::Registry;
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!("Software\\OpenLarager\\Tests\\{}", uuid::Uuid::new_v4());
        let (key, _) = root.create_subkey(&path).unwrap();
        let registry = Registry(key);
        for expandable in [true, false] {
            let value = PathValue {
                text: format!("%SystemRoot%;{};C:\\Projetos á", "C:\\Long;".repeat(600)),
                expandable,
            };
            registry.write(Some(&value)).unwrap();
            assert_eq!(registry.read().unwrap(), Some(value));
        }
        registry.write(None).unwrap();
        assert!(registry.read().unwrap().is_none());
        drop(registry);
        root.delete_subkey_all(path).unwrap();
    }
}
