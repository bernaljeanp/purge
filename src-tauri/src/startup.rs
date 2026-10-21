//! Startup apps, toggled the same way Task Manager does it: through the
//! `StartupApproved` flags, so nothing is deleted and Windows agrees with us.
use crate::programs::split_reg_path;
use crate::uninstall::split_command;
use serde::Serialize;
use std::path::PathBuf;
use std::{env, fs};
use winreg::RegValue;
use winreg::enums::REG_BINARY;

const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    /// Executable to pull an icon from
    pub exe: String,
    pub location: String,
    pub machine_wide: bool,
    pub enabled: bool,
    /// Where the enabled flag lives, e.g. `HKCU\...\StartupApproved\Run`
    pub approved_key: String,
}

enum Source {
    Reg(String),
    Folder(PathBuf),
}

/// (run key or folder, StartupApproved key, label, machine-wide)
fn sources() -> Vec<(Source, String, &'static str, bool)> {
    let folder = |var: &str, rest: &str| env::var_os(var).map(|v| PathBuf::from(v).join(rest));
    let mut s = vec![
        (Source::Reg(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run".into()), format!(r"HKCU\{APPROVED}\Run"), "Registry · user", false),
        (Source::Reg(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run".into()), format!(r"HKLM\{APPROVED}\Run"), "Registry · all users", true),
        (Source::Reg(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run".into()), format!(r"HKLM\{APPROVED}\Run32"), "Registry · all users (32-bit)", true),
    ];
    if let Some(f) = folder("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup") {
        s.push((Source::Folder(f), format!(r"HKCU\{APPROVED}\StartupFolder"), "Startup folder · user", false));
    }
    if let Some(f) = folder("ProgramData", r"Microsoft\Windows\Start Menu\Programs\StartUp") {
        s.push((Source::Folder(f), format!(r"HKLM\{APPROVED}\StartupFolder"), "Startup folder · all users", true));
    }
    s
}

fn is_enabled(approved_key: &str, name: &str) -> bool {
    let Some((root, sub)) = split_reg_path(approved_key) else { return true };
    match root.open_subkey(sub).and_then(|k| k.get_raw_value(name)) {
        // 0x02/0x06 = enabled, 0x03/0x07 = disabled (low bit set)
        Ok(v) => v.bytes.first().is_none_or(|b| b & 1 == 0),
        Err(_) => true,
    }
}

pub fn list() -> Vec<StartupItem> {
    let mut out = Vec::new();
    for (source, approved_key, location, machine_wide) in sources() {
        let entries: Vec<(String, String)> = match source {
            Source::Reg(path) => {
                let Some((root, sub)) = split_reg_path(&path) else { continue };
                let Ok(k) = root.open_subkey(sub) else { continue };
                k.enum_values()
                    .flatten()
                    .filter(|(name, _)| !name.is_empty())
                    .filter_map(|(name, _)| k.get_value::<String, _>(&name).ok().map(|cmd| (name, cmd)))
                    .collect()
            }
            Source::Folder(dir) => fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| !n.eq_ignore_ascii_case("desktop.ini"))
                        .map(|n| (n.clone(), dir.join(&n).to_string_lossy().into_owned()))
                        .collect()
                })
                .unwrap_or_default(),
        };
        for (name, command) in entries {
            out.push(StartupItem {
                enabled: is_enabled(&approved_key, &name),
                exe: split_command(&command).0,
                command,
                location: location.to_string(),
                machine_wide,
                approved_key: approved_key.clone(),
                name,
            });
        }
    }
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

pub fn set_enabled(approved_key: &str, name: &str, enabled: bool) -> Result<(), String> {
    // Only ever write inside StartupApproved
    let valid = [r"\Run", r"\Run32", r"\StartupFolder"]
        .iter()
        .any(|s| approved_key.ends_with(&format!("{APPROVED}{s}")));
    if !valid {
        return Err("refused: not a StartupApproved key".into());
    }
    let (root, sub) = split_reg_path(approved_key).ok_or("unsupported hive")?;
    let (key, _) = root.create_subkey(sub).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; 12];
    bytes[0] = if enabled { 0x02 } else { 0x03 };
    key.set_raw_value(name, &RegValue { bytes, vtype: REG_BINARY }).map_err(|e| e.to_string())
}
