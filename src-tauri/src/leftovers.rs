use crate::programs::{Program, reg_key_exists, split_reg_path};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Registry,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    pub kind: Kind,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// The uninstall entry is still registered: the uninstaller was cancelled, failed,
    /// or is still running. The UI must not pre-select anything in that case.
    pub still_installed: bool,
    pub items: Vec<Leftover>,
}

/// Names that are never a leftover on their own, whatever a program is called.
pub const NEVER: &[&str] = &[
    "microsoft", "windows", "common", "commonfiles", "system", "system32", "programs", "temp",
    "packages", "windowsapps", "installer", "users", "public", "default", "program",
    "programfiles", "software", "classes", "policies", "wow6432node", "intel", "nvidia", "amd",
    "dell", "alienware", "google", "apple", "adobe", "mozilla",
];

pub const REG_ROOTS: [&str; 3] = [r"HKCU\Software", r"HKLM\SOFTWARE", r"HKLM\SOFTWARE\WOW6432Node"];

pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn is_noise(token: &str) -> bool {
    let t = token.to_ascii_lowercase();
    let v = t.trim_start_matches('v');
    let is_version = !v.is_empty()
        && v.chars().all(|c| c.is_ascii_digit() || c == '.')
        && v.contains(|c: char| c.is_ascii_digit());
    is_version
        || matches!(t.as_str(), "x64" | "x86" | "x86_64" | "amd64" | "arm64" | "64-bit" | "32-bit" | "-" | "–")
}

/// "7-Zip 23.01 (x64)" -> "7-Zip"
pub fn clean_name(name: &str) -> String {
    let mut depth = 0u32;
    let mut s = String::new();
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    s.split_whitespace().filter(|t| !is_noise(t)).collect::<Vec<_>>().join(" ")
}

pub fn publisher_key(p: &Program) -> Option<String> {
    p.publisher.split_whitespace().next().map(norm).filter(|s| s.len() >= 3)
}

/// Normalized folder/key names that identify this program.
pub fn targets(p: &Program) -> Vec<String> {
    let full = norm(&clean_name(&p.name));
    let mut t = vec![full.clone()];
    // "Mozilla Firefox" also lives as "<publisher>\Firefox"
    if let Some(rest) = publisher_key(p).and_then(|pk| full.strip_prefix(&pk).map(str::to_string)) {
        t.push(rest);
    }
    t.retain(|s| s.len() >= 3 && !NEVER.contains(&s.as_str()));
    t.dedup();
    t
}

pub fn file_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData", "APPDATA", "LOCALAPPDATA"]
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(l) = env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(l).join("Programs"));
    }
    let mut seen = Vec::new();
    roots.retain(|r| {
        let k = r.to_string_lossy().to_lowercase();
        !seen.contains(&k) && { seen.push(k); true }
    });
    roots
}

/// A folder we are allowed to propose (and later move): at least two levels below a drive,
/// not a scan root itself, not under Windows, not the user profile.
pub fn is_safe_dir(d: &Path) -> bool {
    let lower = d.to_string_lossy().trim_end_matches('\\').to_lowercase();
    let blocked = |var: &str| env::var_os(var).is_some_and(|v| {
        let v = v.to_string_lossy().trim_end_matches('\\').to_lowercase();
        lower == v || (var == "SystemRoot" && lower.starts_with(&format!("{v}\\")))
    });
    d.components().count() >= 4
        && !file_roots().iter().any(|r| r.to_string_lossy().trim_end_matches('\\').to_lowercase() == lower)
        && !["SystemRoot", "USERPROFILE", "PUBLIC"].iter().any(|v| blocked(v))
        && d.file_name().is_some_and(|n| !NEVER.contains(&norm(&n.to_string_lossy()).as_str()))
}

fn child_dirs(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.path(), norm(&e.file_name().to_string_lossy())))
        .collect()
}

pub fn child_keys(path: &str) -> Vec<(String, String)> {
    let Some((root, sub)) = split_reg_path(path) else { return vec![] };
    let Ok(k) = root.open_subkey(sub) else { return vec![] };
    k.enum_keys().flatten().map(|n| (format!(r"{path}\{n}"), norm(&n))).collect()
}

pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            // DirEntry metadata doesn't follow junctions/symlinks
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Generic "match name, or publisher\name" walk shared by folders and registry keys.
fn matches<T: Clone>(
    children: Vec<(T, String)>,
    targets: &[String],
    publisher: Option<&str>,
    grandchildren: impl Fn(&T) -> Vec<(T, String)>,
) -> Vec<T> {
    let mut out = Vec::new();
    for (child, n) in children {
        if targets.contains(&n) {
            out.push(child);
        } else if publisher == Some(n.as_str()) {
            out.extend(grandchildren(&child).into_iter().filter(|(_, gn)| targets.contains(gn)).map(|(g, _)| g));
        }
    }
    out
}

pub fn scan(p: &Program) -> ScanResult {
    let targets = targets(p);
    let publisher = publisher_key(p);

    let mut dirs: Vec<PathBuf> = Vec::new();
    if !p.install_location.is_empty() {
        dirs.push(PathBuf::from(&p.install_location));
    }
    for root in file_roots() {
        dirs.extend(matches(child_dirs(&root), &targets, publisher.as_deref(), |d| child_dirs(d)));
    }
    dirs.retain(|d| d.is_dir() && is_safe_dir(d));
    dirs.sort_by_key(|d| d.as_os_str().len());
    // Drop duplicates and anything nested inside an already-listed folder
    let mut kept: Vec<String> = Vec::new();
    for d in dirs {
        let s = d.to_string_lossy().trim_end_matches('\\').to_string();
        let l = s.to_lowercase();
        if !kept.iter().any(|k| { let k = k.to_lowercase(); l == k || l.starts_with(&format!("{k}\\")) }) {
            kept.push(s);
        }
    }

    let mut items: Vec<Leftover> = kept
        .into_iter()
        .map(|path| Leftover { kind: Kind::File, size_bytes: dir_size(Path::new(&path)), path })
        .collect();

    let still_installed = reg_key_exists(&p.id);
    if still_installed {
        items.push(Leftover { kind: Kind::Registry, path: p.id.clone(), size_bytes: 0 });
    }
    for root in REG_ROOTS {
        for key in matches(child_keys(root), &targets, publisher.as_deref(), |k| child_keys(k)) {
            if !items.iter().any(|i| i.path.eq_ignore_ascii_case(&key)) {
                items.push(Leftover { kind: Kind::Registry, path: key, size_bytes: 0 });
            }
        }
    }

    ScanResult { still_installed, items }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(name: &str, publisher: &str) -> Program {
        Program { name: name.into(), publisher: publisher.into(), ..Default::default() }
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("7-Zip 23.01 (x64)"), "7-Zip");
        assert_eq!(clean_name("Mozilla Firefox (x64 en-US)"), "Mozilla Firefox");
        assert_eq!(clean_name("Notepad++ v8.6.2 - 64-bit"), "Notepad++");
        assert_eq!(clean_name("Steam"), "Steam");
    }

    #[test]
    fn builds_targets() {
        assert_eq!(targets(&prog("Mozilla Firefox (x64 en-US)", "Mozilla")), vec!["mozillafirefox", "firefox"]);
        assert_eq!(targets(&prog("7-Zip 23.01 (x64)", "Igor Pavlov")), vec!["7zip"]);
        // Never target a vendor umbrella folder
        assert!(targets(&prog("Microsoft", "Microsoft Corporation")).is_empty());
        assert!(targets(&prog("AB", "")).is_empty());
    }

    #[test]
    fn refuses_unsafe_dirs() {
        assert!(!is_safe_dir(Path::new(r"C:\")));
        assert!(!is_safe_dir(Path::new(r"C:\Windows\System32")));
        if let Some(pf) = env::var_os("ProgramFiles") {
            assert!(!is_safe_dir(Path::new(&pf)));
            assert!(!is_safe_dir(&PathBuf::from(&pf).join("Common Files")));
            assert!(is_safe_dir(&PathBuf::from(&pf).join("SomeApp")));
        }
    }
}
