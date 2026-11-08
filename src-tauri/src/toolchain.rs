//! Fixed CLI adapters. No command, executable or destination comes from IPC.
use crate::{
    error::{Result, invalid},
    model::{ServiceId, Settings, validate_version},
    storage::{Storage, slash},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct CliLayout {
    pub directories: Vec<PathBuf>,
    pub commands: Vec<(String, PathBuf)>,
    pub files: Vec<(PathBuf, String)>,
}

pub fn active(settings: &Settings, id: ServiceId) -> Result<&str> {
    let service = &settings.services[&id];
    let version = service.active_version.as_deref().ok_or_else(|| {
        invalid(format!(
            "Install and select {} before registering PATH",
            id.key()
        ))
    })?;
    validate_version(version)?;
    if !service
        .installations
        .iter()
        .any(|install| install.version == version)
    {
        return Err(invalid("PATH requires an installed version"));
    }
    Ok(version)
}

pub fn package_file(
    storage: &Storage,
    settings: &Settings,
    id: ServiceId,
    file: &str,
) -> Result<PathBuf> {
    let package = storage.package(id, active(settings, id)?).canonicalize()?;
    let path = package.join(file);
    if !path.is_file() {
        return Err(invalid(format!("This package does not contain {file}")));
    }
    let resolved = path.canonicalize()?;
    if !package.starts_with(storage.root.join("packages").canonicalize()?)
        || !resolved.starts_with(&package)
    {
        return Err(invalid("CLI points outside the selected package"));
    }
    Ok(PathBuf::from(slash(&resolved)))
}

fn safe_path(path: &Path) -> Result<String> {
    let text = slash(path).replace('/', "\\");
    if text.contains([';', '"', '%', '\r', '\n', '\0']) {
        return Err(invalid(
            "This workspace path cannot be represented safely in Windows CLI launchers or PATH",
        ));
    }
    Ok(text)
}

// ASCII source with paths expanded by cmd.exe at runtime also works when the
// workspace contains Unicode and the terminal uses a legacy Windows code page.
const HEADER: &str = "@echo off\r\nsetlocal DisableDelayedExpansion\r\nfor %%I in (\"%~dp0..\\..\\..\") do set \"LARAGER_ROOT=%%~fI\"\r\n";
fn launcher_path(storage: &Storage, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(&storage.root)
        .map_err(|_| invalid("CLI path is outside managed storage"))?;
    let suffix = safe_path(relative)?;
    if !suffix.is_ascii() {
        return Err(invalid("CLI package paths must be ASCII"));
    }
    Ok(format!("%LARAGER_ROOT%\\{suffix}"))
}

fn managed_directory(storage: &Storage, path: PathBuf) -> Result<PathBuf> {
    // Check existing ancestors as well as the final directory for junction escapes.
    let mut ancestor = path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("Invalid CLI directory"))?;
    }
    if !ancestor
        .canonicalize()?
        .starts_with(storage.root.canonicalize()?)
    {
        return Err(invalid("CLI directory points outside managed storage"));
    }
    safe_path(&path)?;
    Ok(path)
}

pub fn prepare(storage: &Storage, id: ServiceId, version: &str) -> Result<()> {
    if id == ServiceId::Notepadpp {
        return crate::notepadpp::prepare(storage, version);
    }
    if id.is_daemon() && id != ServiceId::Php {
        return Ok(());
    }
    let launcher = managed_directory(storage, storage.root.join("config/cli").join(id.key()))?;
    fs::create_dir_all(launcher)?;
    for sub in match id {
        ServiceId::Nodejs => &["global", "cache"][..],
        ServiceId::Python => &["environment", "cache/pip", "cache/bytecode"][..],
        ServiceId::Composer => &["home/vendor/bin", "cache"][..],
        _ => &[],
    } {
        fs::create_dir_all(managed_directory(
            storage,
            storage.data(id, version).join(sub),
        )?)?;
    }
    if id == ServiceId::Python {
        let pip = storage.config_dir(id, version).join("pip.ini");
        if !pip.exists() {
            crate::storage::atomic_write(&pip, b"# Open Larager defaults.\n")?;
        }
    }
    Ok(())
}

pub fn layout(storage: &Storage, settings: &Settings, id: ServiceId) -> Result<CliLayout> {
    if id == ServiceId::Notepadpp {
        return Err(invalid(
            "Notepad++ is a desktop tool and does not support PATH registration",
        ));
    }
    let version = active(settings, id)?;
    let mut layout = CliLayout {
        directories: Vec::new(),
        commands: Vec::new(),
        files: Vec::new(),
    };
    if id.is_daemon() && id != ServiceId::Php {
        let executable = package_file(storage, settings, id, id.cli_executable())?;
        layout
            .directories
            .push(executable.parent().unwrap().to_owned());
        layout.commands.push((
            Path::new(id.cli_executable())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            executable,
        ));
    } else {
        let launcher = managed_directory(storage, storage.root.join("config/cli").join(id.key()))?;
        layout.directories.push(launcher.clone());
        match id {
            ServiceId::Php => {
                let php = package_file(storage, settings, id, "php.exe")?;
                let php_config = storage.config(id, version);
                let content = format!(
                    "{HEADER}set \"PHPRC={}\"\r\nset \"PHP_INI_SCAN_DIR=\"\r\nset \"PATH={};%PATH%\"\r\n\"{}\" -c \"{}\" %*\r\nexit /b %errorlevel%\r\n",
                    launcher_path(storage, &php_config)?,
                    launcher_path(storage, php.parent().unwrap())?,
                    launcher_path(storage, &php)?,
                    launcher_path(storage, &php_config)?
                );
                let file = launcher.join("php.cmd");
                layout.commands.push(("php.cmd".into(), file.clone()));
                layout.files.push((file, content));
            }
            ServiceId::Nodejs => {
                let node = package_file(storage, settings, id, "node.exe")?;
                let native = node.parent().unwrap();
                let prefix = managed_directory(storage, storage.data(id, version).join("global"))?;
                layout
                    .directories
                    .extend([native.to_owned(), prefix.clone()]);
                layout.commands.push(("node.exe".into(), node.clone()));
                for (name, script) in [("npm", "npm-cli.js"), ("npx", "npx-cli.js")] {
                    let script = package_file(
                        storage,
                        settings,
                        id,
                        &format!("node_modules/npm/bin/{script}"),
                    )?;
                    let content = format!(
                        "{HEADER}set \"NPM_CONFIG_PREFIX={}\"\r\nset \"NPM_CONFIG_CACHE={}\"\r\nset \"NPM_CONFIG_USERCONFIG={}\"\r\nset \"PATH={};%PATH%\"\r\n\"{}\" \"{}\" %*\r\nexit /b %errorlevel%\r\n",
                        launcher_path(storage, &prefix)?,
                        launcher_path(storage, &storage.data(id, version).join("cache"))?,
                        launcher_path(storage, &storage.config_dir(id, version).join("npmrc"))?,
                        launcher_path(storage, native)?,
                        launcher_path(storage, &node)?,
                        launcher_path(storage, &script)?
                    );
                    let file = launcher.join(format!("{name}.cmd"));
                    layout.commands.push((format!("{name}.cmd"), file.clone()));
                    layout.files.push((file, content));
                }
            }
            ServiceId::Python => {
                // Check the selected pinned base as well as the separate mutable environment.
                package_file(storage, settings, id, "python.exe")?;
                let environment =
                    managed_directory(storage, crate::python::environment(storage, version))?;
                let scripts = managed_directory(storage, environment.join("Scripts"))?;
                let python = scripts.join("python.exe");
                if !python.is_file() {
                    return Err(invalid(
                        "Python environment is incomplete; reinstall this version",
                    ));
                }
                layout.directories.push(scripts.clone());
                let setup = format!(
                    "{HEADER}set \"PYTHONHOME=\"\r\nset \"PYTHONPATH=\"\r\nset \"PYTHONNOUSERSITE=1\"\r\nset \"VIRTUAL_ENV={}\"\r\nset \"PIP_CONFIG_FILE={}\"\r\nset \"PIP_CACHE_DIR={}\"\r\nset \"PYTHONPYCACHEPREFIX={}\"\r\nset \"PATH={};%PATH%\"\r\n",
                    launcher_path(storage, &environment)?,
                    launcher_path(storage, &storage.config_dir(id, version).join("pip.ini"))?,
                    launcher_path(storage, &storage.data(id, version).join("cache/pip"))?,
                    launcher_path(storage, &storage.data(id, version).join("cache/bytecode"))?,
                    launcher_path(storage, &scripts)?
                );
                for (name, args) in [
                    ("python", ""),
                    ("python3", ""),
                    ("pip", "-m pip "),
                    ("pip3", "-m pip "),
                ] {
                    let file = launcher.join(format!("{name}.cmd"));
                    let content = format!(
                        "{setup}\"{}\" {args}%*\r\nexit /b %errorlevel%\r\n",
                        launcher_path(storage, &python)?
                    );
                    layout.commands.push((format!("{name}.cmd"), file.clone()));
                    layout.files.push((file, content));
                }
            }
            ServiceId::Composer => {
                let php = package_file(storage, settings, ServiceId::Php, "php.exe")?;
                let phar = package_file(storage, settings, id, "composer.phar")?;
                let php_config = storage.config(ServiceId::Php, active(settings, ServiceId::Php)?);
                let home = managed_directory(storage, storage.data(id, version).join("home"))?;
                layout.directories.push(home.join("vendor/bin"));
                let content = format!(
                    "{HEADER}set \"COMPOSER_HOME={}\"\r\nset \"COMPOSER_CACHE_DIR={}\"\r\nset \"PHPRC={}\"\r\nset \"PHP_INI_SCAN_DIR=\"\r\nset \"PATH={};%PATH%\"\r\n\"{}\" -c \"{}\" \"{}\" %*\r\nexit /b %errorlevel%\r\n",
                    launcher_path(storage, &home)?,
                    launcher_path(storage, &storage.data(id, version).join("cache"))?,
                    launcher_path(storage, &php_config)?,
                    launcher_path(storage, php.parent().unwrap())?,
                    launcher_path(storage, &php)?,
                    launcher_path(storage, &php_config)?,
                    launcher_path(storage, &phar)?
                );
                let file = launcher.join("composer.cmd");
                layout.commands.push(("composer.cmd".into(), file.clone()));
                layout.files.push((file, content));
            }
            _ => return Err(invalid("Missing CLI adapter")),
        }
    }
    for directory in &layout.directories {
        safe_path(directory)?;
    }
    Ok(layout)
}

/// A dependent launcher must be regenerated even if its own version is unchanged.
pub fn changed(
    previous: &Settings,
    settings: &Settings,
    id: ServiceId,
    refresh: Option<ServiceId>,
) -> bool {
    let before = &previous.services[&id];
    let after = &settings.services[&id];
    refresh == Some(id)
        || before.add_to_path != after.add_to_path
        || ((before.add_to_path || after.add_to_path)
            && (before.active_version != after.active_version
                || id.definition().dependencies.iter().any(|dependency| {
                    previous.services[dependency].active_version
                        != settings.services[dependency].active_version
                })))
}
