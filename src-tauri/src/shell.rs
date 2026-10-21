//! "Uninstall with Purge" on the right-click menu of shortcuts (.lnk), and the
//! launch path that resolves the clicked shortcut back to an installed program.
use crate::programs::Program;
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const KEY: &str = r"Software\Classes\lnkfile\shell\PurgeUninstall";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn is_registered() -> bool {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(KEY).is_ok()
}

pub fn set(enabled: bool, label: &str) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if !enabled {
        return match hkcu.delete_subkey_all(KEY) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy();
    let (verb, _) = hkcu.create_subkey(KEY).map_err(|e| e.to_string())?;
    verb.set_value("", &label).map_err(|e| e.to_string())?;
    verb.set_value("Icon", &exe.as_ref()).map_err(|e| e.to_string())?;
    let (cmd, _) = verb.create_subkey("command").map_err(|e| e.to_string())?;
    cmd.set_value("", &format!("\"{exe}\" --uninstall \"%1\"")).map_err(|e| e.to_string())
}

/// Target of a .lnk via the Shell's own COM object.
fn shortcut_target(lnk: &str) -> Option<String> {
    let script = format!(
        "(New-Object -ComObject WScript.Shell).CreateShortcut('{}').TargetPath",
        lnk.replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let target = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!target.is_empty()).then_some(target)
}

/// The installed program a file belongs to: the deepest install folder containing it.
pub fn program_for(target: &str, programs: Vec<Program>) -> Option<Program> {
    let t = target.to_lowercase();
    programs
        .into_iter()
        .filter(|p| {
            let loc = p.install_location.trim_end_matches('\\').to_lowercase();
            (!loc.is_empty() && t.starts_with(&format!("{loc}\\")))
                || crate::icons::clean_icon_path(&p.icon).eq_ignore_ascii_case(target)
        })
        .max_by_key(|p| p.install_location.len())
}

/// `purge.exe --uninstall "C:\...\App.lnk"` -> the program to uninstall.
pub fn launch_request() -> Option<Program> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--uninstall")?;
    let path = args.get(i + 1)?;
    let target = if path.to_lowercase().ends_with(".lnk") { shortcut_target(path)? } else { path.clone() };
    program_for(&target, crate::programs::list())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_owning_program() {
        let p = |name: &str, loc: &str| Program { name: name.into(), install_location: loc.into(), ..Default::default() };
        let list = vec![p("Steam", r"C:\Program Files (x86)\Steam"), p("CS2", r"C:\Program Files (x86)\Steam\steamapps\common\cs2")];
        let found = program_for(r"C:\Program Files (x86)\Steam\steamapps\common\cs2\game\bin\win64\cs2.exe", list);
        assert_eq!(found.map(|p| p.name).as_deref(), Some("CS2"));
    }
}
