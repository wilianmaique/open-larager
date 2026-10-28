use crate::error::{Error, Result};
use serde::Serialize;
use std::{
    fmt, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOperation {
    Inspect,
    Read,
    Create,
    Write,
    Copy,
    Verify,
    Remove,
    Lock,
    Move,
    Permissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LockCheck {
    NotRequested,
    Identified,
    NotIdentified,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub current_process: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFailure {
    kind: &'static str,
    pub operation: FileOperation,
    pub path: PathBuf,
    pub related_path: Option<PathBuf>,
    pub os_code: Option<i32>,
    pub message: String,
    pub lock_check: LockCheck,
    pub processes: Vec<LockingProcess>,
    pub diagnostic_code: Option<u32>,
}

impl fmt::Display for FileFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {}: {}",
            self.operation,
            self.path.display(),
            self.message
        )?;
        if let Some(related) = &self.related_path {
            write!(f, " ({})", related.display())?;
        }
        for process in &self.processes {
            write!(f, "; {} (PID {})", process.name, process.pid)?;
        }
        Ok(())
    }
}

impl FileFailure {
    pub fn capture(
        operation: FileOperation,
        path: &Path,
        related_path: Option<&Path>,
        error: io::Error,
    ) -> Self {
        let os_code = error.raw_os_error();
        let mut issue = Self {
            kind: "fileOperation",
            operation,
            path: path.to_owned(),
            related_path: related_path.map(Path::to_owned),
            os_code,
            message: error.to_string(),
            lock_check: LockCheck::NotRequested,
            processes: Vec::new(),
            diagnostic_code: None,
        };
        // Sharing/byte-range locks and access denied may be caused by open files.
        // Other errors (e.g. disk full) must not be labelled as process locks.
        if cfg!(windows) && matches!(os_code, Some(5 | 32 | 33)) {
            let paths: Vec<_> = [Some(path), related_path]
                .into_iter()
                .flatten()
                .filter(|path| path.is_file())
                .collect();
            if paths.is_empty() {
                issue.lock_check = LockCheck::Unavailable;
            } else {
                match locking_processes(&paths) {
                    Ok(processes) => {
                        issue.lock_check = if processes.is_empty() {
                            LockCheck::NotIdentified
                        } else {
                            LockCheck::Identified
                        };
                        issue.processes = processes;
                    }
                    Err(code) => {
                        issue.lock_check = LockCheck::Unavailable;
                        issue.diagnostic_code = Some(code);
                    }
                }
            }
        }
        issue
    }
}

pub trait FileResultExt<T> {
    fn at(self, operation: FileOperation, path: &Path) -> Result<T>;
    fn between(self, operation: FileOperation, path: &Path, related: &Path) -> Result<T>;
}

impl<T> FileResultExt<T> for io::Result<T> {
    fn at(self, operation: FileOperation, path: &Path) -> Result<T> {
        self.map_err(|error| {
            Error::FileOperation(Box::new(FileFailure::capture(operation, path, None, error)))
        })
    }
    fn between(self, operation: FileOperation, path: &Path, related: &Path) -> Result<T> {
        self.map_err(|error| {
            Error::FileOperation(Box::new(FileFailure::capture(
                operation,
                path,
                Some(related),
                error,
            )))
        })
    }
}

#[cfg(windows)]
fn locking_processes(paths: &[&Path]) -> std::result::Result<Vec<LockingProcess>, u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_MORE_DATA, ERROR_SUCCESS},
            System::{
                RestartManager::*,
                Threading::{
                    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
                    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
                },
            },
        },
        core::{PCWSTR, PWSTR},
    };
    struct Session(u32);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                let _ = RmEndSession(self.0);
            }
        }
    }
    let mut handle = 0;
    let mut key = [0; CCH_RM_SESSION_KEY as usize + 1];
    let status = unsafe { RmStartSession(&mut handle, None, PWSTR(key.as_mut_ptr())) };
    if status != ERROR_SUCCESS {
        return Err(status.0);
    }
    let session = Session(handle);
    let names: Vec<Vec<u16>> = paths
        .iter()
        .map(|path| path.as_os_str().encode_wide().chain(Some(0)).collect())
        .collect();
    let pointers: Vec<_> = names.iter().map(|name| PCWSTR(name.as_ptr())).collect();
    let status = unsafe { RmRegisterResources(session.0, Some(&pointers), None, None) };
    if status != ERROR_SUCCESS {
        return Err(status.0);
    }
    let mut needed = 0;
    let mut count = 0;
    let mut reasons = 0;
    let mut entries = Vec::<RM_PROCESS_INFO>::new();
    // Processes can open or close files between the sizing and listing calls.
    for _ in 0..4 {
        let status = unsafe {
            RmGetList(
                session.0,
                &mut needed,
                &mut count,
                if entries.is_empty() {
                    None
                } else {
                    Some(entries.as_mut_ptr())
                },
                &mut reasons,
            )
        };
        if status == ERROR_SUCCESS {
            entries.truncate(count as usize);
            let mut processes: Vec<_> = entries
                .into_iter()
                .map(|entry| {
                    let name_end = entry
                        .strAppName
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(entry.strAppName.len());
                    let name = String::from_utf16_lossy(&entry.strAppName[..name_end]);
                    let pid = entry.Process.dwProcessId;
                    let executable = unsafe {
                        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                            .ok()
                            .and_then(|process| {
                                let mut created = Default::default();
                                let mut exited = Default::default();
                                let mut kernel = Default::default();
                                let mut user = Default::default();
                                let same_process = GetProcessTimes(
                                    process,
                                    &mut created,
                                    &mut exited,
                                    &mut kernel,
                                    &mut user,
                                )
                                .is_ok()
                                    && created == entry.Process.ProcessStartTime;
                                let mut buffer = vec![0u16; 32768];
                                let mut length = buffer.len() as u32;
                                let found = same_process
                                    && QueryFullProcessImageNameW(
                                        process,
                                        PROCESS_NAME_WIN32,
                                        PWSTR(buffer.as_mut_ptr()),
                                        &mut length,
                                    )
                                    .is_ok();
                                let _ = CloseHandle(process);
                                found.then(|| {
                                    PathBuf::from(String::from_utf16_lossy(
                                        &buffer[..length as usize],
                                    ))
                                })
                            })
                    };
                    LockingProcess {
                        pid,
                        name,
                        executable,
                        current_process: pid == std::process::id(),
                    }
                })
                .collect();
            processes.sort_by_key(|process| process.pid);
            processes.dedup_by_key(|process| process.pid);
            return Ok(processes);
        }
        if status != ERROR_MORE_DATA || needed > 4096 {
            return Err(status.0);
        }
        entries.resize(needed as usize, RM_PROCESS_INFO::default());
        count = needed;
    }
    Err(ERROR_MORE_DATA.0)
}

#[cfg(not(windows))]
fn locking_processes(_: &[&Path]) -> std::result::Result<Vec<LockingProcess>, u32> {
    Err(50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_io_context_and_serializes_structured_errors() {
        let path = Path::new("C:/environment/data/missing.db");
        let result: io::Result<()> = Err(io::Error::from_raw_os_error(2));
        let error = result.at(FileOperation::Read, path).unwrap_err();
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(value["kind"], "fileOperation");
        assert_eq!(value["path"], "C:/environment/data/missing.db");
        assert_eq!(value["operation"], "read");
        assert_eq!(value["osCode"], 2);
        assert_eq!(value["lockCheck"], "notRequested");
        assert_eq!(
            serde_json::to_value(crate::error::invalid("existing message")).unwrap(),
            "existing message"
        );
    }

    #[cfg(windows)]
    #[test]
    fn identifies_a_real_external_process_holding_a_file() {
        use std::{
            fs,
            io::{BufRead, BufReader},
            process::{Child, Stdio},
        };
        struct Locker(Child);
        impl Drop for Locker {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("locked database.db");
        fs::write(&path, b"preserved data").unwrap();
        // Fixed helper script; paths are environment values, never shell code.
        let mut command = crate::platform::command("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$f = [IO.File]::Open($env:LARAGER_TEST_LOCK_FILE, 'Open', 'Read', 'None'); [Console]::WriteLine('locked'); [Console]::Out.Flush(); [Console]::ReadLine() | Out-Null; $f.Dispose()"])
            .env("LARAGER_TEST_LOCK_FILE", &path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = Locker(command.spawn().unwrap());
        let mut ready = String::new();
        BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "locked");
        let error = fs::read(&path).at(FileOperation::Read, &path).unwrap_err();
        let Error::FileOperation(issue) = error else {
            panic!("Missing structured diagnostic")
        };
        assert_eq!(issue.path, path);
        assert_eq!(issue.os_code, Some(32));
        assert_eq!(issue.lock_check, LockCheck::Identified, "{issue:?}");
        let process = issue
            .processes
            .iter()
            .find(|process| process.pid == child.0.id())
            .expect("The locker PID must be identified");
        assert!(
            process
                .executable
                .as_ref()
                .unwrap()
                .ends_with("powershell.exe")
        );
        assert!(!process.current_process);
        drop(child);
        assert_eq!(fs::read(path).unwrap(), b"preserved data");
    }

    #[cfg(windows)]
    #[test]
    fn reports_limits_of_directory_queries_without_guessing_a_process() {
        let temp = tempfile::tempdir().unwrap();
        let issue = FileFailure::capture(
            FileOperation::Remove,
            temp.path(),
            None,
            io::Error::from_raw_os_error(32),
        );
        assert_eq!(issue.lock_check, LockCheck::Unavailable);
        assert!(issue.processes.is_empty());
        assert_eq!(issue.path, temp.path());
    }
}
