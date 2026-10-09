//! Terminates the Cursor desktop process before an explicit takeover.

use crate::Result;

#[cfg(windows)]
pub async fn terminate_cursor() -> Result<bool> {
    tokio::task::spawn_blocking(terminate_cursor_windows)
        .await
        .map_err(|error| crate::Error::Config(format!("failed to run terminate_cursor: {error}")))?
}

#[cfg(windows)]
fn terminate_cursor_windows() -> Result<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_TERMINATE,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut found = false;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name = String::from_utf16_lossy(&entry.szExeFile);
                let trimmed = name.trim_matches('\0');
                if trimmed.eq_ignore_ascii_case("Cursor.exe") {
                    let handle =
                        OpenProcess(PROCESS_TERMINATE | 0x0010_0000, 0, entry.th32ProcessID);
                    if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                        // Parent termination can already have stopped a child
                        // listed in the same snapshot. Its signalled handle is success.
                        let _ = TerminateProcess(handle, 1);
                        if WaitForSingleObject(handle, 5000) != 0 {
                            CloseHandle(handle);
                            CloseHandle(snapshot);
                            return Err(crate::Error::Config(
                                "Cursor could not be stopped before updating its preferences"
                                    .into(),
                            ));
                        }
                        found = true;
                        CloseHandle(handle);
                    } else if std::io::Error::last_os_error().raw_os_error() != Some(87) {
                        CloseHandle(snapshot);
                        return Err(crate::Error::Config(
                            "Cannot access Cursor process to reload BYOK preferences".into(),
                        ));
                    }
                }

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
        Ok(found)
    }
}

#[cfg(not(windows))]
pub async fn terminate_cursor() -> Result<bool> {
    Ok(false)
}

pub fn reopen_cursor(was_running: bool) -> Result<()> {
    if !was_running {
        return Ok(());
    }
    let root = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| crate::Error::Config("Cannot locate Cursor executable".into()))?;
    let executable = std::path::PathBuf::from(root).join("Programs/cursor/Cursor.exe");
    std::process::Command::new(executable).spawn()?;
    Ok(())
}
