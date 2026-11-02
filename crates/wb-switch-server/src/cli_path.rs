//! Windows installer integration for the per-user PATH. The installer invokes
//! this binary from its own directory; it never accepts an arbitrary path.
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
use winreg::enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ};
#[cfg(test)]
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

const OWNER: &str = r"Software\WorkBuddySwitchNext\CliPath";

#[derive(Clone, Copy)]
enum Action {
    Install,
    Uninstall,
}

pub fn installer_command(args: &[String]) -> io::Result<()> {
    let action = match args {
        [_, action] if action == "install" => Action::Install,
        [_, action] if action == "uninstall" => Action::Uninstall,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid installer action",
            ))
        }
    };
    let exe = std::env::current_exe()?;
    if !exe
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("workbuddy-switch-next.exe"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unexpected executable name",
        ));
    }
    let install = exe
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| io::Error::other("Invalid install directory"))?;
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = root.create_subkey("Environment")?;
    let (owner, _) = root.create_subkey(OWNER)?;
    if update(&env, &owner, install, action)? {
        let text: Vec<u16> = "Environment\0".encode_utf16().collect();
        let mut result = 0usize;
        // Notify Explorer and future terminals; existing terminals keep their environment.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                text.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5000,
                &mut result,
            );
        }
    }
    Ok(())
}

fn expanded(value: &str) -> String {
    let input: Vec<u16> = std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let size = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), std::ptr::null_mut(), 0) };
    if size == 0 {
        return value.to_string();
    }
    let mut output = vec![0u16; size as usize];
    let written = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), output.as_mut_ptr(), size) };
    if written == 0 || written > size {
        return value.to_string();
    }
    String::from_utf16_lossy(&output[..written.saturating_sub(1) as usize])
}

fn same_path(a: &str, b: &str) -> bool {
    expanded(a)
        .trim()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim().trim_end_matches(['\\', '/']))
}

fn raw_string(value: &str, kind: winreg::enums::RegType) -> RegValue {
    RegValue {
        vtype: kind,
        bytes: value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}

fn decode_path(value: &RegValue) -> io::Result<String> {
    if !value.bytes.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Malformed user PATH",
        ));
    }
    let mut words: Vec<u16> = value
        .bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    while words.last() == Some(&0) {
        words.pop();
    }
    if words.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Malformed user PATH",
        ));
    }
    String::from_utf16(&words)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Malformed user PATH"))
}

fn update(env: &RegKey, owner: &RegKey, install: &str, action: Action) -> io::Result<bool> {
    if install.is_empty() || install.contains(';') || !Path::new(install).is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid installation directory",
        ));
    }
    let raw = match env.get_raw_value("Path") {
        Ok(value) => Some(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let kind = raw
        .as_ref()
        .map(|v| v.vtype.clone())
        .unwrap_or(REG_EXPAND_SZ);
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Err(io::Error::other("Unsupported user PATH type"));
    }
    let current = raw
        .as_ref()
        .map(decode_path)
        .transpose()?
        .unwrap_or_default();
    let stored: String = owner.get_value("AddedPath").unwrap_or_default();
    match action {
        Action::Install => {
            if current.split(';').any(|part| same_path(part, install)) {
                return Ok(false);
            }
            let next = if current.is_empty() {
                install.to_string()
            } else {
                format!("{current};{install}")
            };
            if next.encode_utf16().count() > 32767 {
                return Err(io::Error::other("User PATH is too long"));
            }
            owner.set_value("AddedPath", &install)?;
            owner.set_value("PathExisted", &(u32::from(raw.is_some())))?;
            env.set_raw_value("Path", &raw_string(&next, kind))?;
            Ok(true)
        }
        Action::Uninstall => {
            if stored.is_empty() || !same_path(&stored, install) {
                return Ok(false);
            }
            let mut parts: Vec<&str> = current.split(';').collect();
            let index = parts.iter().rposition(|part| same_path(part, install));
            if let Some(index) = index {
                parts.remove(index);
                let next = parts.join(";");
                let existed: u32 = owner.get_value("PathExisted").unwrap_or(1);
                if next.is_empty() && existed == 0 {
                    env.delete_value("Path")?;
                } else {
                    env.set_raw_value("Path", &raw_string(&next, kind))?;
                }
            }
            let _ = owner.delete_value("AddedPath");
            let _ = owner.delete_value("PathExisted");
            Ok(index.is_some())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_path_install_and_uninstall_preserve_user_entries() {
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key_name = format!(
            r"Software\WorkBuddySwitchNextTests\{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (parent, _) = root.create_subkey(&key_name).unwrap();
        let (env, _) = parent.create_subkey("Environment").unwrap();
        let (owner, _) = parent.create_subkey("Owner").unwrap();
        let original = r"%LOCALAPPDATA%\Tools;;C:\Existing;";
        let install = r"D:\Program Files\WorkBuddy Switch Next";
        env.set_raw_value("Path", &raw_string(original, REG_EXPAND_SZ))
            .unwrap();
        assert!(update(&env, &owner, install, Action::Install).unwrap());
        assert!(!update(&env, &owner, install, Action::Install).unwrap());
        let raw = env.get_raw_value("Path").unwrap();
        assert_eq!(raw.vtype, REG_EXPAND_SZ);
        assert_eq!(
            String::from_reg_value(&raw).unwrap(),
            format!("{original};{install}")
        );
        assert!(update(&env, &owner, install, Action::Uninstall).unwrap());
        assert_eq!(
            String::from_reg_value(&env.get_raw_value("Path").unwrap()).unwrap(),
            original
        );
        env.set_value("Path", &format!("{install};{original}"))
            .unwrap();
        assert!(!update(&env, &owner, install, Action::Install).unwrap());
        assert!(!update(&env, &owner, install, Action::Uninstall).unwrap());
        assert_eq!(
            env.get_value::<String, _>("Path").unwrap(),
            format!("{install};{original}")
        );
        env.delete_value("Path").unwrap();
        assert!(update(&env, &owner, install, Action::Install).unwrap());
        assert!(update(&env, &owner, install, Action::Uninstall).unwrap());
        assert_eq!(
            env.get_raw_value("Path").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        let invalid = RegValue {
            vtype: REG_SZ,
            bytes: vec![0xff],
        };
        env.set_raw_value("Path", &invalid).unwrap();
        assert_eq!(
            update(&env, &owner, install, Action::Install)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(env.get_raw_value("Path").unwrap().bytes, invalid.bytes);
        drop(owner);
        drop(env);
        drop(parent);
        root.delete_subkey_all(key_name).unwrap();
    }
}
