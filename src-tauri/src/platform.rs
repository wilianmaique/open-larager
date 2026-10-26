use crate::error::{Result, invalid};

/// Some Windows distributions require ANSI-compatible paths. Use existing OS aliases;
/// never create junctions, alter system code pages, or rewrite third-party executables.
#[cfg(windows)]
pub fn runtime_path(path: &Path) -> Result<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{Win32::Storage::FileSystem::GetShortPathNameW, core::PCWSTR};
    let normal = crate::storage::slash(path);
    if normal.is_ascii() {
        return Ok(normal);
    }
    let mut existing = path.to_path_buf();
    let mut remaining = Vec::new();
    while !existing.exists() {
        remaining.push(
            existing
                .file_name()
                .ok_or_else(|| invalid("Invalid runtime path"))?
                .to_os_string(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| invalid("Runtime path has no existing parent"))?
            .to_path_buf();
    }
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buffer = vec![0u16; 32768];
    let length = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buffer)) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(invalid(
            "This distribution requires an ASCII-compatible path. Windows short path names are unavailable; use portable mode in an ASCII directory.",
        ));
    }
    let mut short = std::path::PathBuf::from(
        String::from_utf16(&buffer[..length]).map_err(|_| invalid("Invalid short path"))?,
    );
    for part in remaining.into_iter().rev() {
        short.push(part);
    }
    let result = crate::storage::slash(&short);
    if !result.is_ascii() {
        return Err(invalid(
            "This distribution requires an ASCII-compatible path. Use portable mode in an ASCII directory.",
        ));
    }
    Ok(result)
}
#[cfg(not(windows))]
pub fn runtime_path(path: &Path) -> Result<String> {
    Ok(crate::storage::slash(path))
}
use std::{
    fs::OpenOptions,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub fn command(executable: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(executable);
    for variable in [
        "PHPRC",
        "PGDATA",
        "PGHOST",
        "PGPORT",
        "PGSERVICE",
        "MYSQL_HOME",
        "MYSQL_TCP_PORT",
    ] {
        command.env_remove(variable);
    }
    command.env("PHP_INI_SCAN_DIR", "");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

pub struct Process {
    pub child: Child,
    #[cfg(windows)]
    _job: Job,
}
impl Process {
    pub fn spawn(mut command: Command, log: &Path) -> Result<Self> {
        // Assign a suspended process to the job before its first instruction can create descendants.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000004); // CREATE_NO_WINDOW | CREATE_SUSPENDED
        }
        let output = OpenOptions::new().create(true).append(true).open(log)?;
        command
            .stdout(Stdio::from(output.try_clone()?))
            .stderr(Stdio::from(output))
            .stdin(Stdio::null());
        let mut child = command.spawn().map_err(|e| invalid(format!("Unable to launch service: {e}. Check the Microsoft Visual C++ runtime and service log.")))?;
        #[cfg(windows)]
        {
            match Job::attach(&child) {
                Ok(job) => {
                    if let Err(error) = resume_main_thread(child.id()) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                    Ok(Self { child, _job: job })
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(error)
                }
            }
        }
        #[cfg(not(windows))]
        {
            Ok(Self { child })
        }
    }
    pub fn alive(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }
    pub fn terminate(&mut self) -> Result<()> {
        #[cfg(windows)]
        self._job.terminate()?;
        #[cfg(not(windows))]
        self.child.kill()?;
        let _ = self.child.wait();
        Ok(())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.terminate();
        }
    }
}

#[cfg(windows)]
struct Job(isize);
#[cfg(windows)]
impl Job {
    fn attach(child: &Child) -> Result<Self> {
        use std::os::windows::io::AsRawHandle;
        Self::attach_raw(child.as_raw_handle())
    }
    fn attach_raw(process: std::os::windows::io::RawHandle) -> Result<Self> {
        use windows::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::*,
        };
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| invalid(e.to_string()))?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(e) = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .and_then(|_| AssignProcessToJobObject(handle, HANDLE(process)))
            {
                let _ = CloseHandle(handle);
                return Err(invalid(format!("Cannot isolate service process: {e}")));
            }
            Ok(Self(handle.0 as isize))
        }
    }
    fn terminate(&self) -> Result<()> {
        unsafe {
            windows::Win32::System::JobObjects::TerminateJobObject(
                windows::Win32::Foundation::HANDLE(self.0 as *mut _),
                1,
            )
        }
        .map_err(|e| invalid(e.to_string()))
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(
                self.0 as *mut _,
            ));
        }
    }
}

#[cfg(windows)]
fn resume_main_thread(pid: u32) -> Result<()> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::ToolHelp::*,
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };
    unsafe {
        let snapshot =
            CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(|e| invalid(e.to_string()))?;
        let result = (|| {
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            Thread32First(snapshot, &mut entry).map_err(|e| invalid(e.to_string()))?;
            loop {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                        .map_err(|e| invalid(e.to_string()))?;
                    let resumed = ResumeThread(thread);
                    let _ = CloseHandle(thread);
                    if resumed == u32::MAX {
                        return Err(invalid("Unable to resume isolated service process"));
                    }
                    return Ok(());
                }
                if Thread32Next(snapshot, &mut entry).is_err() {
                    return Err(invalid("Suspended process thread was not found"));
                }
            }
        })();
        let _ = CloseHandle(snapshot);
        result
    }
}

#[derive(Clone, Copy)]
enum TextEncoding {
    Utf8,
    Utf16Le,
}

fn decode_output(bytes: &[u8], encoding: TextEncoding) -> Result<String> {
    match encoding {
        TextEncoding::Utf8 => Ok(String::from_utf8_lossy(bytes).into_owned()),
        TextEncoding::Utf16Le => {
            let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
            if !bytes.len().is_multiple_of(2) {
                return Err(invalid("Command returned incomplete UTF-16LE output"));
            }
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units)
                .map_err(|_| invalid("Command returned invalid UTF-16LE output"))
        }
    }
}

pub async fn checked_output(cmd: Command, timeout: Duration) -> Result<String> {
    checked_text_output(cmd, timeout, TextEncoding::Utf8).await
}

pub async fn checked_output_utf16le(cmd: Command, timeout: Duration) -> Result<String> {
    checked_text_output(cmd, timeout, TextEncoding::Utf16Le).await
}

async fn checked_text_output(
    mut cmd: Command,
    timeout: Duration,
    encoding: TextEncoding,
) -> Result<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000004);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.kill_on_drop(true);
    let child = cmd.spawn()?;
    #[cfg(windows)]
    let _job = {
        let raw = child
            .raw_handle()
            .ok_or_else(|| invalid("Command process handle is unavailable"))?;
        let job = Job::attach_raw(raw)?;
        resume_main_thread(
            child
                .id()
                .ok_or_else(|| invalid("Command exited before initialization"))?,
        )?;
        job
    };
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| invalid("Command timed out; inspect the service log"))??;
    let combined = format!(
        "{}{}",
        decode_output(&output.stdout, encoding)?,
        decode_output(&output.stderr, encoding)?
    );
    if !output.status.success() {
        return Err(invalid(format!(
            "Command failed ({}): {}",
            output.status,
            combined.chars().take(6000).collect::<String>()
        )));
    }
    Ok(combined)
}

pub async fn wait_exit(process: &mut Process, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process.alive()? {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(false)
}

#[cfg(windows)]
pub fn protect(bytes: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::*,
    };
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut _,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| invalid(e.to_string()))?;
        let data = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Ok(data)
    }
}
#[cfg(windows)]
pub fn unprotect(bytes: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::*,
    };
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut _,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| invalid(e.to_string()))?;
        let data = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Ok(data)
    }
}
#[cfg(not(windows))]
pub fn protect(_: &[u8]) -> Result<Vec<u8>> {
    Err(invalid("Credential storage requires Windows DPAPI"))
}
#[cfg(not(windows))]
pub fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    Err(invalid("Credential storage requires Windows DPAPI"))
}

#[cfg(windows)]
pub fn secure_directory(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                },
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR, SetFileSecurityW,
            },
        },
        core::{PCWSTR, w},
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;OW)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|e| invalid(e.to_string()))?;
        let result = SetFileSecurityW(
            PCWSTR(name.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
        .ok()
        .map_err(|e| invalid(e.to_string()));
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
}
#[cfg(not(windows))]
pub fn secure_directory(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::{PCWSTR, w},
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(invalid("Unable to open target"));
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn open_path(_: &Path) -> Result<()> {
    Err(invalid("Only Windows is supported"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_certificate_details_without_losing_unicode_or_exposing_the_bom() {
        let text =
            "Versão: 3\r\nNúmero de Série\r\nParâmetros\r\nChave pública\r\nEspañol 中文 हिन्दी 🔒";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_output(&bytes, TextEncoding::Utf16Le).unwrap(), text);
        let with_bom = [vec![0xff, 0xfe], bytes].concat();
        assert_eq!(
            decode_output(&with_bom, TextEncoding::Utf16Le).unwrap(),
            text
        );
        assert_eq!(
            decode_output(text.as_bytes(), TextEncoding::Utf8).unwrap(),
            text
        );
        assert!(
            decode_output(&[], TextEncoding::Utf16Le)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_incomplete_or_invalid_utf16_instead_of_replacing_characters() {
        assert!(decode_output(&[0x56], TextEncoding::Utf16Le).is_err());
        assert!(decode_output(&[0x00, 0xd8], TextEncoding::Utf16Le).is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn decodes_real_certutil_error_output_in_unicode() {
        let temporary = tempfile::tempdir().unwrap();
        let mut cmd = command("certutil.exe");
        cmd.args(["-Unicode", "-dump"])
            .arg(temporary.path().join("missing.cer"));
        let error = checked_output_utf16le(cmd, Duration::from_secs(10))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Command failed"));
        assert!(!error.contains(['\u{fffd}', '\u{feff}', '\0']));
    }
}
