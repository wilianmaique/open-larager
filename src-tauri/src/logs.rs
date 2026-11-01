//! Explicit log clearing, confined to regular .log files under managed logs/.
use crate::{
    error::{Result, invalid},
    file_diagnostics::{FileOperation as Op, FileResultExt},
    model::ServiceId,
    storage::{Storage, slash},
};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

fn regular(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).at(Op::Inspect, path)?;
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let link = metadata.file_type().is_symlink();
    if link || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(invalid(
            "Log clearing refuses links, junctions and special files",
        ));
    }
    Ok(())
}

fn open(path: &Path, directory: bool, root: &Path) -> Result<File> {
    regular(path, directory)?;
    let mut options = OpenOptions::new();
    options.read(directory).write(!directory);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Pin the directory tree while opening all files, without preventing
        // servers from continuing to append. Do not follow a replaced reparse point.
        options
            .share_mode(0x1 | 0x2)
            .custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
    let file = options.open(path).at(Op::Write, path)?;
    validate_handle(&file, path, directory, root)?;
    Ok(file)
}

#[cfg(windows)]
fn validate_handle(file: &File, path: &Path, directory: bool, root: &Path) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_NAME_NORMALIZED, GetFileInformationByHandle, GetFinalPathNameByHandleW,
        },
    };
    let handle = HANDLE(file.as_raw_handle());
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut info) }
        .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
        .at(Op::Inspect, path)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(invalid(
            "Log clearing refuses links, junctions and special files",
        ));
    }
    let mut buffer = vec![0u16; 32768];
    let length =
        unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, FILE_NAME_NORMALIZED) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(invalid("Unable to verify the log file location"));
    }
    let resolved = PathBuf::from(slash(Path::new(&String::from_utf16_lossy(
        &buffer[..length],
    ))));
    if !resolved.starts_with(root) {
        return Err(invalid("Log path points outside managed logs"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_handle(file: &File, path: &Path, directory: bool, root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !directory && file.metadata()?.nlink() != 1 {
            return Err(invalid("Log clearing refuses hard links"));
        }
    }
    let _ = (file, directory);
    if !path.canonicalize()?.starts_with(root) {
        return Err(invalid("Log path points outside managed logs"));
    }
    Ok(())
}

fn scan(
    path: &Path,
    root: &Path,
    directories: &mut Vec<File>,
    files: &mut Vec<(PathBuf, File)>,
    depth: usize,
) -> Result<()> {
    if depth > 64 || files.len() > 100_000 || directories.len() > 10_000 {
        return Err(invalid(
            "Log clearing exceeds the supported file count or nesting limit",
        ));
    }
    directories.push(open(path, true, root)?);
    for entry in fs::read_dir(path).at(Op::Inspect, path)? {
        let path = entry.at(Op::Inspect, path)?.path();
        let metadata = fs::symlink_metadata(&path).at(Op::Inspect, &path)?;
        // A directory junction can be reported as a symlink rather than a
        // directory by symlink_metadata. Reject it before classifying entries.
        regular(&path, metadata.is_dir())?;
        if metadata.is_dir() {
            scan(&path, root, directories, files, depth + 1)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
        {
            files.push((path.clone(), open(&path, false, root)?));
        }
    }
    Ok(())
}

pub fn clear(storage: &Storage, id: Option<ServiceId>) -> Result<usize> {
    if id.is_some_and(|id| !id.is_daemon()) {
        return Err(invalid("This tool has no managed service logs"));
    }
    let root = PathBuf::from(slash(&storage.root.canonicalize()?)).join("logs");
    let mut directories = vec![open(&root, true, &root)?];
    let mut files = Vec::new();
    let targets = match id {
        Some(ServiceId::Nginx) => vec![root.join("nginx"), root.join("sites")],
        Some(id) => vec![root.join(id.key())],
        None => vec![root.clone()],
    };
    // Open and validate every target before truncating any file. A lock or an
    // unsafe path discovered during preparation leaves all logs unchanged.
    for target in targets {
        match fs::symlink_metadata(&target) {
            Ok(_) => scan(&target, &root, &mut directories, &mut files, 0)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).at(Op::Inspect, &target),
        }
    }
    for (path, file) in &files {
        file.set_len(0).at(Op::Write, path)?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(storage: &Storage, path: &str, contents: &str) {
        let path = storage.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    #[test]
    fn scopes_clear_to_service_versions_or_all_logs_and_preserves_appenders() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().to_owned()).unwrap();
        for path in [
            "logs/php/1/process.log",
            "logs/php/2/php-error.LOG",
            "logs/postgresql/1/postgresql.log",
            "logs/sites/removed/access.log",
            "logs/old-service/orphan.log",
        ] {
            write(&storage, path, "old entry\n");
        }
        for path in [
            "data/postgresql/1/wal.log",
            "projects/user.log",
            "config/php/custom.ini",
            "logs/php/1/notes.txt",
        ] {
            write(&storage, path, "keep");
        }
        let process = storage.root.join("logs/php/1/process.log");
        let mut writer = OpenOptions::new().append(true).open(&process).unwrap();
        assert_eq!(clear(&storage, Some(ServiceId::Php)).unwrap(), 2);
        assert!(fs::read(&process).unwrap().is_empty());
        writeln!(writer, "new entry").unwrap();
        assert_eq!(fs::read_to_string(&process).unwrap(), "new entry\n");
        assert!(
            !fs::read(storage.root.join("logs/postgresql/1/postgresql.log"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(clear(&storage, Some(ServiceId::Nginx)).unwrap(), 1);
        assert_eq!(clear(&storage, None).unwrap(), 5);
        for path in [
            "data/postgresql/1/wal.log",
            "projects/user.log",
            "config/php/custom.ini",
            "logs/php/1/notes.txt",
        ] {
            assert_eq!(fs::read_to_string(storage.root.join(path)).unwrap(), "keep");
        }
        assert!(clear(&storage, Some(ServiceId::Notepadpp)).is_err());
    }

    #[test]
    fn refuses_hard_links_before_clearing_any_log() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().to_owned()).unwrap();
        write(&storage, "logs/php/1/process.log", "keep log");
        write(&storage, "projects/important.txt", "keep user data");
        fs::hard_link(
            storage.root.join("projects/important.txt"),
            storage.root.join("logs/php/1/unsafe.log"),
        )
        .unwrap();
        assert!(clear(&storage, Some(ServiceId::Php)).is_err());
        assert_eq!(
            fs::read_to_string(storage.root.join("logs/php/1/process.log")).unwrap(),
            "keep log"
        );
        assert_eq!(
            fs::read_to_string(storage.root.join("projects/important.txt")).unwrap(),
            "keep user data"
        );
    }

    #[cfg(windows)]
    #[test]
    fn refuses_locked_files_and_junctions_before_modifying_logs() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("workspace")).unwrap();
        write(&storage, "logs/php/1/process.log", "keep");
        let path = storage.root.join("logs/php/1/locked.log");
        fs::write(&path, "locked").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(clear(&storage, None).is_err());
        assert_eq!(
            fs::read_to_string(storage.root.join("logs/php/1/process.log")).unwrap(),
            "keep"
        );
        drop(held);
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.log"), "external").unwrap();
        let junction = storage.root.join("logs/escape");
        let status = crate::platform::command("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(junction.to_string_lossy().replace('/', "\\"))
            .arg(&outside)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(clear(&storage, None).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("keep.log")).unwrap(),
            "external"
        );
        fs::remove_dir(junction).unwrap();
    }
}
