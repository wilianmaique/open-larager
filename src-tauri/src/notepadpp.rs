//! Optional per-user desktop integration. Windows retains control of UserChoice.
use crate::{
    error::{Result, invalid},
    model::{ServiceId, Settings, validate_version},
    storage::{Storage, atomic_write, slash},
};
use std::{fs, path::PathBuf};

pub const EXTENSIONS: [&str; 5] = [".log", ".txt", ".cfg", ".conf", ".ini"];

fn profile(storage: &Storage) -> Result<PathBuf> {
    let path = storage.root.join("config/notepadpp/profile");
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("Invalid editor profile"))?;
    }
    if !ancestor
        .canonicalize()?
        .starts_with(storage.root.canonicalize()?)
    {
        return Err(invalid("Editor profile points outside managed storage"));
    }
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn prepare(storage: &Storage, version: &str) -> Result<()> {
    validate_version(version)?;
    let package = storage.package(ServiceId::Notepadpp, version);
    let directory = profile(storage)?;
    let config = directory.join("config.xml");
    if !config.exists() {
        atomic_write(&config, b"<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<NotepadPlus><GUIConfigs><GUIConfig name=\"noUpdate\">yes</GUIConfig></GUIConfigs></NotepadPlus>\n")?;
    }
    // The upstream portable ZIP includes an updater. Managed binaries must only
    // change via the pinned catalog, so do not leave that executable available.
    let updater = package.join("updater/GUP.exe");
    if updater.exists() {
        fs::remove_file(updater)?;
    }
    Ok(())
}

fn quoted_path(path: &std::path::Path) -> Result<String> {
    let value = slash(path).replace('/', "\\");
    if value.contains(['"', '%', '\r', '\n', '\0']) {
        return Err(invalid(
            "This path cannot be represented safely in an editor registration",
        ));
    }
    Ok(format!("\"{value}\""))
}

#[cfg(windows)]
fn command(storage: &Storage, settings: &Settings) -> Result<String> {
    let version = crate::toolchain::active(settings, ServiceId::Notepadpp)?;
    Ok(format!(
        "{} -multiInst -settingsDir={} \"%1\"",
        quoted_path(
            &storage
                .package(ServiceId::Notepadpp, version)
                .join("notepad++.exe")
        )?,
        quoted_path(&profile(storage)?)?,
    ))
}

pub fn open(storage: &Storage, settings: &Settings) -> Result<()> {
    let executable =
        crate::toolchain::package_file(storage, settings, ServiceId::Notepadpp, "notepad++.exe")?;
    let directory = profile(storage)?;
    crate::platform::command(executable)
        .arg("-multiInst")
        .arg(format!("-settingsDir={}", slash(&directory)))
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

fn registration_name(settings: &Settings) -> Result<Option<String>> {
    settings
        .notepadpp
        .registration_id
        .as_ref()
        .map(|id| {
            let id =
                uuid::Uuid::parse_str(id).map_err(|_| invalid("Invalid editor registration ID"))?;
            // ProgIDs are limited to 39 characters; retain the full UUID entropy.
            Ok(format!("OL.Npp.{}", id.simple()))
        })
        .transpose()
}

pub fn open_defaults(settings: &Settings) -> Result<()> {
    if !settings.notepadpp.file_associations {
        return Err(invalid(
            "Register Notepad++ before opening its default-app settings",
        ));
    }
    let name =
        registration_name(settings)?.ok_or_else(|| invalid("Missing editor registration"))?;
    crate::platform::open_path(std::path::Path::new(&format!(
        "ms-settings:defaultapps?registeredAppUser={name}"
    )))
}

/// Reconcile after startup recovery or an authoritative environment move.
pub fn sync(storage: &Storage, settings: &Settings) -> Result<()> {
    if registration_name(settings)?.is_none() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        registry::sync(
            &winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER),
            storage,
            settings,
        )?;
        registry::notify();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = storage;
        Err(invalid("Editor associations are supported on Windows only"))
    }
}

/// Registry changes precede the settings commit; failures restore the previous
/// registration. A crash is reconciled from durable settings at the next start.
pub fn transaction<T>(
    storage: &Storage,
    previous: &Settings,
    next: &Settings,
    commit: impl FnOnce() -> Result<T>,
) -> Result<T> {
    transact(previous, next, |settings| sync(storage, settings), commit)
}

fn transact<T>(
    previous: &Settings,
    next: &Settings,
    mut reconcile: impl FnMut(&Settings) -> Result<()>,
    commit: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let changed = previous.notepadpp.file_associations != next.notepadpp.file_associations
        || (next.notepadpp.file_associations
            && previous.services[&ServiceId::Notepadpp].active_version
                != next.services[&ServiceId::Notepadpp].active_version);
    if !changed {
        return commit();
    }
    // Keep the newly allocated identity even when the first registration fails,
    // so that rollback can remove every partially written key.
    let mut rollback = previous.clone();
    rollback
        .notepadpp
        .registration_id
        .clone_from(&next.notepadpp.registration_id);
    let apply = reconcile(next);
    match apply.and_then(|()| commit()) {
        Ok(value) => Ok(value),
        Err(error) => {
            reconcile(&rollback).map_err(|rollback| {
                invalid(format!(
                    "{error}; editor registration rollback failed: {rollback}"
                ))
            })?;
            Err(error)
        }
    }
}

#[cfg(windows)]
mod registry {
    use super::*;
    use winreg::{RegKey, RegValue, enums::*};

    fn remove_key(root: &RegKey, path: &str) -> Result<()> {
        match root.delete_subkey_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    fn remove_value(key: &RegKey, name: &str) -> Result<()> {
        match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) fn sync(root: &RegKey, storage: &Storage, settings: &Settings) -> Result<()> {
        let Some(name) = registration_name(settings)? else {
            return Ok(());
        };
        let application = format!("Software\\OpenLarager\\Editors\\{name}");
        let progid = format!("Software\\Classes\\{name}");
        let registered = root.create_subkey("Software\\RegisteredApplications")?.0;
        let enabled = settings.notepadpp.file_associations
            && settings.services[&ServiceId::Notepadpp]
                .active_version
                .is_some();
        if enabled {
            let command = command(storage, settings)?;
            let capabilities = root
                .create_subkey(format!("{application}\\Capabilities"))?
                .0;
            capabilities.set_value("ApplicationName", &"Notepad++ (Open Larager)")?;
            capabilities.set_value("ApplicationDescription", &"Notepad++")?;
            let associations = capabilities.create_subkey("FileAssociations")?.0;
            let class = root.create_subkey(&progid)?.0;
            class.set_value("", &"Notepad++")?;
            let version = crate::toolchain::active(settings, ServiceId::Notepadpp)?;
            class.create_subkey("DefaultIcon")?.0.set_value(
                "",
                &format!(
                    "{},0",
                    quoted_path(
                        &storage
                            .package(ServiceId::Notepadpp, version)
                            .join("notepad++.exe")
                    )?
                ),
            )?;
            class
                .create_subkey("shell\\open\\command")?
                .0
                .set_value("", &command)?;
            for extension in EXTENSIONS {
                associations.set_value(extension, &name)?;
                root.create_subkey(format!("Software\\Classes\\{extension}\\OpenWithProgids"))?
                    .0
                    .set_raw_value(
                        &name,
                        &RegValue {
                            bytes: Vec::new().into(),
                            vtype: REG_NONE,
                        },
                    )?;
            }
            registered.set_value(&name, &format!("{application}\\Capabilities"))?;
        } else {
            remove_value(&registered, &name)?;
            for extension in EXTENSIONS {
                match root.open_subkey_with_flags(
                    format!("Software\\Classes\\{extension}\\OpenWithProgids"),
                    KEY_SET_VALUE,
                ) {
                    Ok(key) => remove_value(&key, &name)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            remove_key(root, &application)?;
            remove_key(root, &progid)?;
        }
        // Never write extension defaults, Explorer FileExts or UserChoice hashes.
        Ok(())
    }

    pub(super) fn notify() {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
    }

    #[test]
    fn registration_follows_version_and_root_and_preserves_other_apps() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("ambiente com espaços")).unwrap();
        let mut settings = Settings::default();
        let id = uuid::Uuid::new_v4().to_string();
        settings.notepadpp.registration_id = Some(id.clone());
        settings.notepadpp.file_associations = true;
        let service = settings.services.get_mut(&ServiceId::Notepadpp).unwrap();
        for version in ["1.0", "2.0"] {
            service.installations.push(crate::model::Installation {
                version: version.into(),
                installed_at: 0,
                sha256: "a".repeat(64),
            });
        }
        service.active_version = Some("1.0".into());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let sandbox = format!("Software\\OpenLaragerTests\\{id}");
        let root = hkcu.create_subkey(&sandbox).unwrap().0;
        let ext = root.create_subkey("Software\\Classes\\.txt").unwrap().0;
        let user_choice = root.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.txt\\UserChoice").unwrap().0;
        user_choice.set_value("ProgId", &"Other.Editor").unwrap();
        user_choice
            .set_value("Hash", &"Windows-owned hash")
            .unwrap();
        ext.set_value("", &"Other.Editor").unwrap();
        ext.create_subkey("OpenWithProgids")
            .unwrap()
            .0
            .set_value("Other.Editor", &"")
            .unwrap();
        sync(&root, &storage, &settings).unwrap();
        let name = registration_name(&settings).unwrap().unwrap();
        let key = root
            .open_subkey(format!("Software\\Classes\\{name}\\shell\\open\\command"))
            .unwrap();
        let before: String = key.get_value("").unwrap();
        assert!(before.contains("1.0\\notepad++.exe\" -multiInst -settingsDir=\""));
        assert!(before.ends_with("\"%1\""));
        settings
            .services
            .get_mut(&ServiceId::Notepadpp)
            .unwrap()
            .active_version = Some("2.0".into());
        let moved = Storage::open(temp.path().join("moved")).unwrap();
        sync(&root, &moved, &settings).unwrap();
        let after: String = key.get_value("").unwrap();
        assert!(after.contains("moved\\packages\\notepadpp\\2.0"));
        assert_eq!(name.len(), 39);
        // A failed durable commit restores the old command rather than leaving
        // the new registration pointing to an uncommitted selection.
        let mut failed = settings.clone();
        failed
            .services
            .get_mut(&ServiceId::Notepadpp)
            .unwrap()
            .active_version = Some("1.0".into());
        let result: Result<()> = transact(
            &settings,
            &failed,
            |settings| sync(&root, &moved, settings),
            || Err(invalid("simulated settings failure")),
        );
        assert!(result.is_err());
        assert_eq!(key.get_value::<String, _>("").unwrap(), after);
        // Reconcile the durable state after an interrupted unregister.
        failed.notepadpp.file_associations = false;
        sync(&root, &moved, &failed).unwrap();
        sync(&root, &moved, &settings).unwrap();
        let key = root
            .open_subkey(format!("Software\\Classes\\{name}\\shell\\open\\command"))
            .unwrap();
        assert_eq!(key.get_value::<String, _>("").unwrap(), after);
        settings
            .services
            .get_mut(&ServiceId::Notepadpp)
            .unwrap()
            .active_version = None;
        sync(&root, &moved, &settings).unwrap();
        assert!(
            root.open_subkey(format!("Software\\Classes\\{name}"))
                .is_err()
        );
        assert_eq!(ext.get_value::<String, _>("").unwrap(), "Other.Editor");
        assert_eq!(
            user_choice.get_value::<String, _>("ProgId").unwrap(),
            "Other.Editor"
        );
        assert_eq!(
            user_choice.get_value::<String, _>("Hash").unwrap(),
            "Windows-owned hash"
        );
        assert!(
            ext.open_subkey("OpenWithProgids")
                .unwrap()
                .get_raw_value("Other.Editor")
                .is_ok()
        );
        drop(key);
        drop(ext);
        drop(user_choice);
        drop(root);
        hkcu.delete_subkey_all(sandbox).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_survives_reinstall_and_updater_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().to_owned()).unwrap();
        let package = storage.package(ServiceId::Notepadpp, "1.0");
        fs::create_dir_all(package.join("updater")).unwrap();
        fs::write(package.join("updater/GUP.exe"), "updater").unwrap();
        prepare(&storage, "1.0").unwrap();
        assert!(!package.join("updater/GUP.exe").exists());
        let config = profile(&storage).unwrap().join("config.xml");
        fs::write(&config, "user preferences and session").unwrap();
        prepare(&storage, "1.0").unwrap();
        assert_eq!(
            fs::read_to_string(config).unwrap(),
            "user preferences and session"
        );
        assert!(quoted_path(std::path::Path::new("C:/%1/bad")).is_err());
        assert!(registration_name(&Settings::default()).unwrap().is_none());
    }
}
