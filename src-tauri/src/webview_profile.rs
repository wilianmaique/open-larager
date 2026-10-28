use crate::{
    error::{Result, invalid},
    file_diagnostics::{FileOperation as Op, FileResultExt},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Profile {
    pub directory: PathBuf,
    pub legacy_error: Option<crate::error::Error>,
}

pub fn portable(executable_directory: &Path) -> Result<Profile> {
    let directory = executable_directory.join("larager-webview");
    fs::create_dir_all(&directory).at(Op::Create, &directory)?;
    regular_directory(&directory)?;
    Ok(Profile {
        directory,
        legacy_error: None,
    })
}

/// WebView2 keeps its own live databases and byte-range locks. Its profile must
/// never share the environment root that users can relocate while the UI runs.
pub fn prepare(app_data: &Path) -> Result<Profile> {
    let name = app_data
        .file_name()
        .ok_or_else(|| invalid("Missing application data directory"))?;
    let mut profile_name = name.to_os_string();
    profile_name.push(".webview");
    let directory = app_data.with_file_name(profile_name);
    fs::create_dir_all(&directory).at(Op::Create, &directory)?;
    regular_directory(&directory)?;
    let legacy = app_data.join("EBWebView");
    let legacy_error = if legacy.exists() {
        // Preserve the existing profile before starting the browser. If a newer
        // profile already exists, archive the legacy one without replacing it.
        let migrate = (|| {
            regular_directory(&legacy)?;
            let active = directory.join("EBWebView");
            let target = if active.exists() {
                directory.join(format!("legacy-{}", uuid::Uuid::new_v4()))
            } else {
                active
            };
            fs::rename(&legacy, &target).between(Op::Move, &legacy, &target)
        })();
        migrate.err()
    } else {
        None
    };
    // A profile still used by another app is left intact. The new UI can open
    // with its separate profile and file diagnostics can explain any later lock.
    Ok(Profile {
        directory,
        legacy_error,
    })
}

fn regular_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).at(Op::Inspect, path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if !metadata.is_dir() || metadata.is_symlink() || reparse {
        return Err(invalid(format!(
            "Unsafe WebView profile directory: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_legacy_profile_outside_environment_before_browser_startup() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app-data");
        fs::create_dir_all(root.join("EBWebView/Default")).unwrap();
        fs::write(
            root.join("EBWebView/Default/Web Data"),
            b"existing browser profile",
        )
        .unwrap();
        fs::write(root.join("settings.json"), b"environment settings").unwrap();
        let profile = prepare(&root).unwrap();
        assert!(profile.legacy_error.is_none());
        assert!(!profile.directory.starts_with(&root));
        assert!(!root.join("EBWebView").exists());
        assert_eq!(
            fs::read(profile.directory.join("EBWebView/Default/Web Data")).unwrap(),
            b"existing browser profile"
        );
        assert_eq!(
            fs::read(root.join("settings.json")).unwrap(),
            b"environment settings"
        );
    }

    #[test]
    fn preserves_both_profiles_when_the_new_one_already_exists() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app-data");
        let profile = prepare(&root).unwrap();
        fs::create_dir_all(profile.directory.join("EBWebView")).unwrap();
        fs::write(profile.directory.join("EBWebView/keep"), b"active").unwrap();
        fs::create_dir_all(root.join("EBWebView")).unwrap();
        fs::write(root.join("EBWebView/keep"), b"legacy").unwrap();
        let profile = prepare(&root).unwrap();
        assert!(profile.legacy_error.is_none());
        assert_eq!(
            fs::read(profile.directory.join("EBWebView/keep")).unwrap(),
            b"active"
        );
        let archives: Vec<_> = fs::read_dir(&profile.directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("legacy-")
            })
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(archives[0].join("keep")).unwrap(), b"legacy");
    }
}
