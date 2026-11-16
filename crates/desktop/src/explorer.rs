//! "Open in Diskmap" entry of the Explorer context menu, kept in HKCU so no administrator rights are needed.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

const LABEL: &str = "Open in Diskmap";
/// Parents of the `diskmap` key: folders, folder backgrounds and drives.
const PARENTS: [&str; 3] = [
    r"Software\Classes\Directory\shell",
    r"Software\Classes\Directory\Background\shell",
    r"Software\Classes\Drive\shell",
];

fn key_of(parent: &str) -> String {
    format!(r"{parent}\diskmap")
}

/// True when the menu entry for folders is registered.
pub fn installed() -> bool {
    let key = HSTRING::from(format!(r"{}\command", key_of(PARENTS[0])));
    // SAFETY: `key` is a NUL-terminated wide string that outlives the call; no data is requested.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    err == ERROR_SUCCESS
}

/// Adds or removes the menu entry for this executable.
pub fn set(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        PARENTS
            .iter()
            .try_for_each(|p| write_entry(p, &exe.to_string_lossy()))
    } else {
        PARENTS.iter().try_for_each(|p| delete_entry(p))
    }
}

fn write_entry(parent: &str, exe: &str) -> Result<(), String> {
    let key = key_of(parent);
    write_value(&key, "MUIVerb", LABEL)?;
    write_value(&key, "Icon", &format!("\"{exe}\",0"))?;
    write_value(&format!(r"{key}\command"), "", &format!("\"{exe}\" \"%V\""))
}

/// Creates `subkey` under HKCU if needed and sets its value `name` (the default value when empty).
fn write_value(subkey: &str, name: &str, value: &str) -> Result<(), String> {
    let subkey = HSTRING::from(subkey);
    let name = HSTRING::from(name);
    let data: Vec<u8> = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut key = HKEY::default();
    // SAFETY: `subkey` is a NUL-terminated wide string and `key` is writable for the call.
    let err = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(format!("Registry: {}", err.to_hresult().message()));
    }
    // SAFETY: `key` was opened above; `name` is NUL-terminated and `data` holds a NUL-terminated REG_SZ.
    let err = unsafe { RegSetValueExW(key, &name, None, REG_SZ, Some(&data)) };
    // SAFETY: `key` is open and closed once.
    let _ = unsafe { RegCloseKey(key) };
    if err != ERROR_SUCCESS {
        return Err(format!("Registry: {}", err.to_hresult().message()));
    }
    Ok(())
}

fn delete_entry(parent: &str) -> Result<(), String> {
    let key = HSTRING::from(key_of(parent));
    // SAFETY: `key` is a NUL-terminated wide string that outlives the call.
    let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &key) };
    if err == ERROR_SUCCESS || err == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("Registry: {}", err.to_hresult().message()))
    }
}
