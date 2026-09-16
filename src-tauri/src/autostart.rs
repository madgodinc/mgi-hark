//! Start with Windows, quietly in the tray.
//!
//! A per-user Run key: no admin rights, visible and switchable in Task
//! Manager's Startup tab, removed with the app's own toggle.

/// Launched by Windows at sign-in rather than by a person: stay in the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

#[cfg(windows)]
const VALUE: &str = "Hark";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn set(on: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe.display().to_string();
    // A build inside target\ disappears on the next compile; never register it.
    if path.to_lowercase().contains(r"\target\") && on {
        return Ok(());
    }
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
        .map_err(|e| e.to_string())?;
    if on {
        key.set_value(VALUE, &format!("\"{path}\" {HIDDEN_FLAG}")).map_err(|e| e.to_string())
    } else {
        match key.delete_value(VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(not(windows))]
pub fn set(_on: bool) -> Result<(), String> {
    Ok(())
}

pub fn started_hidden() -> bool {
    std::env::args().any(|a| a == HIDDEN_FLAG)
}
