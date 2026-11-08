//! Separate the full CPython distribution from the versioned venv used by pip and third-party packages.
use crate::{
    error::{Result, invalid},
    model::ServiceId,
    platform,
    storage::{Storage, atomic_write},
};
use std::{fs, path::PathBuf, time::Duration};

pub fn environment(storage: &Storage, version: &str) -> PathBuf {
    storage.data(ServiceId::Python, version).join("environment")
}

pub async fn initialize(storage: &Storage, version: &str) -> Result<()> {
    let environment = environment(storage, version);
    let base = storage
        .package(ServiceId::Python, version)
        .join("python.exe");
    let marker = environment.join(".larager-python");
    let configuration = environment.join("pyvenv.cfg");
    if configuration.exists() && fs::read_to_string(&marker).ok().as_deref() != Some(version) {
        return Err(invalid(
            "Python environment is not managed by this app; nothing was replaced",
        ));
    }
    if !configuration.exists() {
        if environment.exists() && !marker.is_file() && fs::read_dir(&environment)?.next().is_some()
        {
            return Err(invalid(
                "Python environment contains unrecognized data; nothing was replaced",
            ));
        }
        fs::create_dir_all(&environment)?;
        atomic_write(&marker, version.as_bytes())?;
        let mut command = platform::command(&base);
        command
            .args(["-I", "-B", "-m", "venv", "--without-pip", "--copies"])
            .arg(&environment);
        platform::checked_output(command, Duration::from_secs(120)).await?;
    }
    // ensurepip uses only the wheel shipped in this pinned runtime, never get-pip or the network.
    let python = environment.join("Scripts/python.exe");
    let mut command = platform::command(&python);
    command.args(["-I", "-B", "-m", "ensurepip", "--upgrade", "--default-pip"]);
    platform::checked_output(command, Duration::from_secs(120)).await?;
    let mut command = platform::command(python);
    command.args([
        "-I", "-B", "-c",
        "import sys, pip, ssl, sqlite3, tkinter, venv; assert sys.prefix != sys.base_prefix; assert '.'.join(map(str, sys.version_info[:3])) == sys.argv[1]",
        version,
    ]);
    platform::checked_output(command, Duration::from_secs(30)).await?;
    Ok(())
}

/// Only the app-owned venv configuration may be rebased; project venvs remain user data.
pub fn is_managed_config(storage: &Storage, relative: &std::path::Path) -> bool {
    let Some(version) = relative
        .strip_prefix("data/python")
        .ok()
        .and_then(|path| path.components().next())
        .and_then(|part| part.as_os_str().to_str())
    else {
        return false;
    };
    crate::model::validate_version(version).is_ok()
        && relative
            == std::path::Path::new("data/python")
                .join(version)
                .join("environment/pyvenv.cfg")
        && fs::read_to_string(environment(storage, version).join(".larager-python"))
            .ok()
            .as_deref()
            == Some(version)
}
