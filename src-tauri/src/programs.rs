use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE};
use winreg::{HKEY, RegKey};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    /// Full registry path of the uninstall entry, e.g. `HKLM\SOFTWARE\...\Uninstall\{GUID}`
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// As stored by the installer, usually `YYYYMMDD`
    pub install_date: String,
    pub size_kb: u64,
    pub install_location: String,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
}

const UNINSTALL_ROOTS: [(&str, &str); 3] = [
    ("HKLM", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ("HKLM", r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    ("HKCU", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
];

pub fn hive(name: &str) -> Option<HKEY> {
    match name {
        "HKLM" => Some(HKEY_LOCAL_MACHINE),
        "HKCU" => Some(HKEY_CURRENT_USER),
        _ => None,
    }
}

/// Splits `HKLM\SOFTWARE\Foo` into (HKLM root key, `SOFTWARE\Foo`).
pub fn split_reg_path(path: &str) -> Option<(RegKey, &str)> {
    let (h, sub) = path.split_once('\\')?;
    Some((RegKey::predef(hive(h)?), sub))
}

pub fn reg_key_exists(path: &str) -> bool {
    split_reg_path(path).is_some_and(|(root, sub)| root.open_subkey(sub).is_ok())
}

pub fn list() -> Vec<Program> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for (hive_name, path) in UNINSTALL_ROOTS {
        let Ok(root) = RegKey::predef(hive(hive_name).unwrap()).open_subkey(path) else {
            continue;
        };
        for key_name in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&key_name) else { continue };
            let s = |n: &str| k.get_value::<String, _>(n).unwrap_or_default().trim().to_string();

            let name = s("DisplayName");
            let uninstall_string = s("UninstallString");
            // Same filters Windows' own "Apps" list uses: hidden components, patches, no uninstaller
            if name.is_empty()
                || uninstall_string.is_empty()
                || k.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1
                || !s("ParentKeyName").is_empty()
            {
                continue;
            }
            let version = s("DisplayVersion");
            if !seen.insert((name.to_lowercase(), version.clone())) {
                continue;
            }

            out.push(Program {
                id: format!(r"{hive_name}\{path}\{key_name}"),
                publisher: s("Publisher"),
                install_date: s("InstallDate"),
                size_kb: k.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64,
                install_location: s("InstallLocation").trim_matches('"').to_string(),
                quiet_uninstall_string: s("QuietUninstallString"),
                name,
                version,
                uninstall_string,
            });
        }
    }

    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// ponytail: "can we open HKLM\SOFTWARE for writing" as the admin check; swap for
/// a token-elevation query if it ever misreports.
pub fn is_admin() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags("SOFTWARE", KEY_WRITE)
        .is_ok()
}
