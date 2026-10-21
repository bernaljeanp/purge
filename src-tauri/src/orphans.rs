//! Orphaned folders: left behind by programs uninstalled before Purge was around.
//! Heuristic by nature, so the UI never pre-selects these and they go to quarantine.
use crate::leftovers::{NEVER, clean_name, dir_size, file_roots, norm, publisher_key};
use crate::programs::Program;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Orphan {
    pub path: String,
    pub size_bytes: u64,
    /// Unix milliseconds of the last write
    pub modified: u64,
}

/// Folders Windows, drivers and toolchains keep that no "program" owns.
const SYSTEM: &[&str] = &[
    "packagecache", "windowsdefender", "windowsnt", "windowsmail", "windowsmediaplayer", "windowsphotoviewer",
    "windowsportabledevices", "windowssidebar", "windowspowershell", "internetexplorer", "modifiablewindowsapps",
    "referenceassemblies", "msbuild", "dotnet", "crashdumps", "d3dscache", "connecteddevicesplatform", "comms",
    "history", "publishers", "usoshared", "usoprivate", "oem", "ssh", "realtek", "nvidiacorporation", "intelcorporation",
    "amd", "dell", "alienware", "purge", "softwaredistribution", "regid", "desktop", "documents", "fonts", "virtualstore",
    "identitycrl", "peernetworking", "deliveryoptimization", "ssl", "certificates", "uninstallinformation", "sxs", "gamedvr",
];

const MIN_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
const MIN_SIZE: u64 = 100 * 1024;

fn owned(dir: &Path, f: &str, programs: &[Program]) -> bool {
    let lower = dir.to_string_lossy().to_lowercase();
    programs.iter().any(|p| {
        let loc = p.install_location.trim_end_matches('\\').to_lowercase();
        let name = norm(&clean_name(&p.name));
        (!loc.is_empty() && (loc == lower || loc.starts_with(&format!("{lower}\\"))))
            || publisher_key(p).as_deref() == Some(f)
            || (f.len() >= 4 && (name.contains(f) || (name.len() >= 4 && f.contains(&name))))
    })
}

pub fn scan(programs: &[Program]) -> Vec<Orphan> {
    let cutoff = SystemTime::now() - MIN_AGE;
    let mut out = Vec::new();
    for root in file_roots() {
        let Ok(rd) = fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let raw = e.file_name().to_string_lossy().into_owned();
            let f = norm(&raw);
            if raw.starts_with('.') || f.len() < 3 || NEVER.contains(&f.as_str()) || SYSTEM.iter().any(|s| f.starts_with(s)) {
                continue;
            }
            let dir = e.path();
            let Some(modified) = e.metadata().ok().and_then(|m| m.modified().ok()) else { continue };
            if modified > cutoff || owned(&dir, &f, programs) {
                continue;
            }
            let size_bytes = dir_size(&dir);
            if size_bytes < MIN_SIZE {
                continue;
            }
            out.push(Orphan {
                path: dir.to_string_lossy().into_owned(),
                size_bytes,
                modified: modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(name: &str, publisher: &str, loc: &str) -> Program {
        Program { name: name.into(), publisher: publisher.into(), install_location: loc.into(), ..Default::default() }
    }

    #[test]
    fn recognises_owned_folders() {
        let programs = vec![
            prog("Mozilla Firefox (x64 en-US)", "Mozilla", r"C:\Program Files\Mozilla Firefox"),
            prog("Microsoft Visual Studio Code", "Microsoft Corporation", ""),
        ];
        assert!(owned(Path::new(r"C:\Program Files\Mozilla Firefox"), "mozillafirefox", &programs));
        assert!(owned(Path::new(r"C:\Users\x\AppData\Roaming\Mozilla"), "mozilla", &programs));
        assert!(owned(Path::new(r"C:\Users\x\AppData\Roaming\Code"), "code", &programs));
        assert!(!owned(Path::new(r"C:\Users\x\AppData\Roaming\OldGame"), "oldgame", &programs));
    }
}
