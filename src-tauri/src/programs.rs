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
    /// Raw `DisplayIcon` value, e.g. `"C:\App\app.exe",0`
    pub icon: String,
    /// The uninstaller it points to no longer exists (orphaned entry)
    #[serde(default)]
    pub broken: bool,
}

/// MSI stores products under a "packed" GUID: each group reversed, the last 8 bytes nibble-swapped.
fn packed_guid(key_name: &str) -> Option<String> {
    let g: String = key_name.trim_matches(|c| c == '{' || c == '}').chars().filter(|c| *c != '-').collect();
    if g.len() != 32 || !g.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let rev = |s: &str| s.chars().rev().collect::<String>();
    let mut out = format!("{}{}{}", rev(&g[0..8]), rev(&g[8..12]), rev(&g[12..16]));
    for i in (16..32).step_by(2) {
        out.push_str(&rev(&g[i..i + 2]));
    }
    Some(out.to_uppercase())
}

fn msi_product_icon(key_name: &str) -> Option<String> {
    let packed = packed_guid(key_name)?;
    let k = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"SOFTWARE\Classes\Installer\Products\{packed}"))
        .ok()?;
    let icon: String = k.get_value("ProductIcon").ok()?;
    std::path::Path::new(&crate::icons::clean_icon_path(&icon)).is_file().then_some(icon)
}

/// "Counter-Strike 2" -> "cs2": first letter of each word, numbers kept whole.
fn acronym(name: &str) -> String {
    // Not clean_name(): it drops bare numbers as versions, but "2" matters in "cs2"
    name.split('(')
        .next()
        .unwrap_or(name)
        .split(|c: char| c.is_whitespace() || c == '-' || c == ':')
        .filter(|w| !w.is_empty())
        .map(|w| if w.chars().all(|c| c.is_ascii_digit()) { w.to_string() } else { w.chars().take(1).collect() })
        .collect::<String>()
        .to_lowercase()
}

/// `dir` plus its subfolders down to `depth`, skipping redistributable/support trees.
fn dirs_within(dir: &std::path::Path, depth: u32) -> Vec<std::path::PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    let mut frontier = vec![dir.to_path_buf()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for d in &frontier {
            let Ok(rd) = std::fs::read_dir(d) else { continue };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if e.file_type().is_ok_and(|t| t.is_dir()) && !["redist", "_commonredist", "directx", "support", "locales", "resources"].iter().any(|s| n.contains(s)) {
                    next.push(e.path());
                }
            }
        }
        // ponytail: hard cap keeps huge game folders cheap; raise if icons go missing
        next.truncate(200);
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

/// Launchers register their own exe as every game's icon; the game's exe is the better pick.
fn is_launcher_icon(path: &str) -> bool {
    let file = path.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
    ["steam.exe", "epicgameslauncher.exe", "upc.exe", "galaxyclient.exe", "eadesktop.exe", "battle.net.exe"].contains(&file.as_str())
}

/// The app's own executable inside its install folder, preferring a name or acronym match.
fn main_exe(install_location: &str, name: &str) -> Option<String> {
    let dir = std::path::Path::new(install_location);
    if install_location.is_empty() || !dir.is_dir() {
        return None;
    }
    let target = crate::leftovers::norm(name);
    let acro = acronym(name);
    let skip = ["unins", "uninst", "setup", "update", "crash", "helper", "install", "elevat", "report", "redist", "vc_", "dxsetup"];
    let mut exes: Vec<(std::path::PathBuf, u64)> = dirs_within(dir, 3)
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .filter(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            !skip.iter().any(|s| stem.contains(s))
        })
        .map(|p| {
            let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (p, len)
        })
        .collect();
    let named = |p: &std::path::PathBuf| {
        let stem = crate::leftovers::norm(&p.file_stem().unwrap_or_default().to_string_lossy());
        (stem.len() >= 3 && (target.contains(&stem) || stem.contains(&target))) || (acro.len() >= 2 && stem == acro)
    };
    // Name match first, then the biggest executable
    exes.sort_by_key(|(p, len)| (!named(p), std::cmp::Reverse(*len)));
    exes.first().map(|(p, _)| p.to_string_lossy().into_owned())
}

/// Best icon source: DisplayIcon, then the MSI product icon, the app's exe, the uninstaller.
fn resolve_icon(display_icon: &str, key_name: &str, install_location: &str, uninstall: &str, name: &str) -> String {
    let clean = crate::icons::clean_icon_path(display_icon);
    let usable = !clean.is_empty() && (clean.contains('%') || std::path::Path::new(&clean).is_file());
    if usable && !is_launcher_icon(&clean) {
        return display_icon.to_string();
    }
    if usable {
        // Launcher icon: try the game's own exe first, keep the launcher as the fallback
        return main_exe(install_location, name).unwrap_or_else(|| display_icon.to_string());
    }
    if let Some(i) = msi_product_icon(key_name) {
        return i;
    }
    if let Some(exe) = main_exe(install_location, name) {
        return exe;
    }
    let (exe, _) = crate::uninstall::split_command(uninstall);
    if exe.to_ascii_lowercase().ends_with(".exe") && !exe.to_ascii_lowercase().ends_with("msiexec.exe") && std::path::Path::new(&exe).is_file() {
        return exe;
    }
    String::new()
}

/// True when the uninstall command points at an executable that is gone.
fn is_broken(uninstall_string: &str) -> bool {
    let (exe, _) = crate::uninstall::split_command(uninstall_string);
    let lower = exe.to_ascii_lowercase();
    // Bare commands (msiexec, rundll32...) resolve via PATH; env vars we don't expand
    if exe.contains('%') || !exe.contains('\\') || lower.ends_with("msiexec.exe") {
        return false;
    }
    !std::path::Path::new(&exe).exists()
}

pub const UNINSTALL_ROOTS: [(&str, &str); 3] = [
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
            // ponytail: icon resolution touches disk per program (~70 small dir reads); fine at this scale


            out.push(Program {
                id: format!(r"{hive_name}\{path}\{key_name}"),
                publisher: s("Publisher"),
                install_date: s("InstallDate"),
                size_kb: k.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64,
                install_location: s("InstallLocation").trim_matches('"').to_string(),
                quiet_uninstall_string: s("QuietUninstallString"),
                icon: resolve_icon(&s("DisplayIcon"), &key_name, &s("InstallLocation").trim_matches('"'), &uninstall_string, &name),
                broken: is_broken(&uninstall_string),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_msi_guids() {
        // Adobe Reader's well-known pair
        assert_eq!(packed_guid("{AC76BA86-7AD7-1033-7B44-AB0000000001}").as_deref(), Some("68AB67CA7DA73301B744BA0000000010"));
        assert_eq!(packed_guid("Steam App 730"), None);
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn builds_acronyms() {
        assert_eq!(acronym("Counter-Strike 2"), "cs2");
        assert_eq!(acronym("Grand Theft Auto V"), "gtav");
        assert!(is_launcher_icon(r"C:\Program Files (x86)\Steam\steam.exe"));
        assert!(!is_launcher_icon(r"C:\Games\cs2.exe"));
    }
}
