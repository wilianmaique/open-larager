use crate::{
    error::{Result, invalid},
    model::validate_hostname,
};
use std::{fs, path::PathBuf};

pub const START: &str = "# BEGIN Open Larager managed hosts";
pub const END: &str = "# END Open Larager managed hosts";

pub fn render(existing: &str, hosts: &[String]) -> Result<String> {
    let starts = existing.matches(START).count();
    let ends = existing.matches(END).count();
    if starts != ends || starts > 1 {
        return Err(invalid(
            "Managed hosts block is malformed; repair it before applying changes",
        ));
    }
    let mut output = existing.to_owned();
    if let Some(start) = output.find(START) {
        let end = output
            .find(END)
            .ok_or_else(|| invalid("Missing hosts block terminator"))?
            + END.len();
        if end < start {
            return Err(invalid("Malformed hosts block"));
        }
        let end = if output[end..].starts_with("\r\n") {
            end + 2
        } else if output[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        output.replace_range(start..end, "");
    }
    let mut unique = std::collections::BTreeSet::new();
    for hostname in hosts {
        unique.insert(validate_hostname(hostname)?);
    }
    // Refuse to shadow entries owned by the user or another tool.
    for line in output.lines() {
        let content = line.split('#').next().unwrap_or_default();
        for name in content.split_whitespace().skip(1) {
            if unique.contains(&name.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "Hostname is already managed outside Open Larager: {name}"
                )));
            }
        }
    }
    if !unique.is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str("\r\n");
        }
        output.push_str(START);
        output.push_str("\r\n");
        for hostname in unique {
            output.push_str(&format!("127.0.0.1\t{hostname}\r\n"));
        }
        output.push_str(END);
        output.push_str("\r\n");
    }
    Ok(output)
}

pub fn hosts_path() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

#[cfg(windows)]
pub fn open_in_editor() -> Result<()> {
    use windows::{
        Win32::{
            System::Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            },
            UI::{
                Shell::{
                    ASSOCF_NOTRUNCATE, ASSOCSTR, ASSOCSTR_COMMAND, ASSOCSTR_EXECUTABLE,
                    ASSOCSTR_PROGID, AssocQueryStringW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
                    SHELLEXECUTEINFOW, ShellExecuteExW,
                },
                WindowsAndMessaging::SW_SHOWNORMAL,
            },
        },
        core::{PCWSTR, PWSTR, w},
    };

    // This runs on a blocking worker, so initialize COM for Shell extensions.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|e| {
            invalid(format!(
                "Unable to initialize the Windows text editor integration: {e}"
            ))
        })?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;

    // Elevation applies to the editor executable, not to the extensionless hosts
    // document. Resolve the user's .txt choice before asking Windows to elevate it.
    fn association_string(association: PCWSTR, kind: ASSOCSTR) -> Result<Vec<u16>> {
        let association_error = |e| invalid(format!("Unable to find the default text editor: {e}"));
        let mut length = 0;
        unsafe {
            AssocQueryStringW(
                ASSOCF_NOTRUNCATE,
                kind,
                association,
                w!("open"),
                None,
                &mut length,
            )
            .ok()
            .map_err(association_error)?;
        }
        if !(2..=32768).contains(&length) {
            return Err(invalid("Windows has no default text editor for .txt files"));
        }
        let mut value = vec![0u16; length as usize];
        unsafe {
            AssocQueryStringW(
                ASSOCF_NOTRUNCATE,
                kind,
                association,
                w!("open"),
                Some(PWSTR(value.as_mut_ptr())),
                &mut length,
            )
            .ok()
            .map_err(association_error)?;
        }
        Ok(value)
    }
    let class = association_string(w!(".txt"), ASSOCSTR_PROGID)?;
    let editor = association_string(PCWSTR(class.as_ptr()), ASSOCSTR_EXECUTABLE)?;
    let command = association_string(PCWSTR(class.as_ptr()), ASSOCSTR_COMMAND)?;
    let command = unsafe { PCWSTR(command.as_ptr()).to_string() }
        .map_err(|e| invalid(format!("Invalid text editor command: {e}")))?;
    let path = hosts_path();
    fs::metadata(&path)?;
    // Keep registered switches and portable editor profiles. No command shell is used.
    let arguments: Vec<u16> = editor_parameters(&command, &path.to_string_lossy())?
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(editor.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| {
        invalid(format!(
            "Windows administrator approval was cancelled or the hosts editor could not be opened: {e}"
        ))
    })
}

#[cfg(windows)]
fn parse_editor_command(command: &str) -> Result<Vec<String>> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            UI::Shell::CommandLineToArgvW,
        },
        core::PCWSTR,
    };
    if command.is_empty() || command.contains('\0') {
        return Err(invalid("Invalid text editor command"));
    }
    let command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(PCWSTR(command.as_ptr()), &mut count) };
    if argv.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let arguments: std::result::Result<Vec<_>, _> = unsafe {
        std::slice::from_raw_parts(argv, count as usize)
            .iter()
            .map(|argument| argument.to_string())
            .collect()
    };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(argv.cast())));
    }
    arguments.map_err(|e| invalid(format!("Invalid text editor arguments: {e}")))
}

#[cfg(windows)]
fn editor_parameters(command: &str, path: &str) -> Result<String> {
    let mut has_file = false;
    let mut arguments = Vec::new();
    for mut argument in parse_editor_command(command)?.into_iter().skip(1) {
        if argument == "%*" {
            continue;
        }
        for placeholder in ["%1", "%L", "%l"] {
            if argument.contains(placeholder) {
                has_file = true;
                argument = argument.replace(placeholder, path);
                break;
            }
        }
        arguments.push(argument);
    }
    if !has_file {
        arguments.push(path.to_owned());
    }
    Ok(arguments
        .iter()
        .map(|argument| quote_editor_argument(argument))
        .collect::<Vec<_>>()
        .join(" "))
}

#[cfg(windows)]
fn quote_editor_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        quoted.extend(std::iter::repeat_n(
            '\\',
            if character == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            },
        ));
        quoted.push(character);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(not(windows))]
pub fn open_in_editor() -> Result<()> {
    Err(invalid("Hosts integration requires Windows"))
}

pub fn read() -> Result<String> {
    // Preserve non-UTF-8 comments byte-for-byte by using a reversible byte -> code point mapping.
    Ok(fs::read(hosts_path())?
        .into_iter()
        .map(char::from)
        .collect())
}
pub fn encode(content: &str) -> Result<Vec<u8>> {
    content
        .chars()
        .map(|c| u8::try_from(c as u32).map_err(|_| invalid("Unexpected hosts file encoding")))
        .collect()
}

#[cfg(windows)]
pub async fn apply(hostnames: &[String]) -> Result<()> {
    use std::time::Duration;
    let hostnames = hostnames
        .iter()
        .map(|name| validate_hostname(name))
        .collect::<Result<Vec<_>>>()?;
    let _ = render(&read()?, &hostnames)?;
    let executable = std::env::current_exe()?;
    // Arguments contain only strictly validated ASCII hostnames. The elevated helper has one operation.
    let arguments = if hostnames.is_empty() {
        "--apply-hosts".to_owned()
    } else {
        format!("--apply-hosts {}", hostnames.join(" "))
    };
    tokio::task::spawn_blocking(move || elevate(&executable, &arguments))
        .await
        .map_err(|e| invalid(e.to_string()))??;
    let mut flush = crate::platform::command("ipconfig.exe");
    flush.arg("/flushdns");
    let _ = crate::platform::checked_output(flush, Duration::from_secs(10)).await;
    Ok(())
}
#[cfg(not(windows))]
pub async fn apply(_: &[String]) -> Result<()> {
    Err(invalid("Hosts integration requires Windows"))
}

#[cfg(windows)]
fn elevate(executable: &std::path::Path, arguments: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{GetExitCodeProcess, WaitForSingleObject},
            UI::{
                Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
                WindowsAndMessaging::SW_HIDE,
            },
        },
        core::{PCWSTR, w},
    };
    let exe: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let arguments: Vec<u16> = arguments.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe.as_ptr()),
            lpParameters: PCWSTR(arguments.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).map_err(|e| {
            invalid(format!(
                "Windows administrator approval was cancelled or failed: {e}"
            ))
        })?;
        let waited = WaitForSingleObject(info.hProcess, 120_000);
        let mut code = 1;
        let exit = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if waited != WAIT_OBJECT_0 || exit.is_err() || code != 0 {
            return Err(invalid(
                "Hosts update failed. Check administrator access and existing entries.",
            ));
        }
    }
    Ok(())
}

pub fn run_helper(hostnames: &[String]) -> Result<()> {
    if hostnames.len() > 1000 {
        return Err(invalid("Too many hostnames"));
    }
    for name in hostnames {
        validate_hostname(name)?;
    }
    let path = hosts_path();
    let old = fs::read(&path)?;
    let text: String = old.iter().copied().map(char::from).collect();
    let output = encode(&render(&text, hostnames)?)?;
    // Snapshot the original before replacing only our own marked block.
    fs::write(path.with_extension("open-larager.bak"), &old)?;
    crate::storage::atomic_write(&path, &output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn elevated_editor_preserves_profile_and_quotes_hosts_using_windows_argument_rules() {
        let path = r"C:\Windows 11\System32\drivers\etc\hosts";
        let command = r#""C:\Editor\notepad++.exe" -multiInst -settingsDir="C:\Dados do usuário\perfil" "%L" %*"#;
        let parameters = editor_parameters(command, path).unwrap();
        assert_eq!(
            parse_editor_command(&format!("editor.exe {parameters}")).unwrap(),
            vec![
                "editor.exe",
                "-multiInst",
                r"-settingsDir=C:\Dados do usuário\perfil",
                path
            ]
        );
        let parameters = editor_parameters(r#""C:\WindowsApps\Notepad.exe""#, path).unwrap();
        assert_eq!(
            parse_editor_command(&format!("editor.exe {parameters}")).unwrap(),
            vec!["editor.exe", path]
        );
        for argument in [
            "",
            "two words",
            "C:\\profile\\",
            "quote\"inside",
            "slash\\\"quote",
            "ação",
        ] {
            let quoted = quote_editor_argument(argument);
            assert_eq!(
                parse_editor_command(&format!("editor.exe {quoted}")).unwrap(),
                vec!["editor.exe", argument]
            );
        }
        assert!(parse_editor_command("editor.exe\0other.exe").is_err());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Requests administrator approval and opens the real Windows hosts file in the default editor; does not write it."]
    fn opens_windows_hosts_in_default_editor_as_administrator() {
        assert!(hosts_path().is_file());
        open_in_editor().unwrap();
    }

    #[test]
    fn hosts_is_idempotent_and_preserves_other_entries() {
        let old = "# custom\r\n127.0.0.1 other.test\r\n";
        let hosts = vec!["my-site.test".into()];
        let new = render(old, &hosts).unwrap();
        assert!(new.starts_with(old));
        assert_eq!(render(&new, &hosts).unwrap(), new);
        assert_eq!(render(&new, &[]).unwrap(), old);
    }
    #[test]
    fn refuses_conflicts_and_broken_blocks() {
        assert!(render("127.0.0.1 app.test", &["app.test".into()]).is_err());
        assert!(render(START, &[]).is_err());
        let bytes = vec![0x23, 0xe3, 0xf5];
        let text: String = bytes.iter().copied().map(char::from).collect();
        assert_eq!(encode(&text).unwrap(), bytes);
    }
}
