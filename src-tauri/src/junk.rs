//! Disposable files: temp folders, crash dumps, error reports and GPU shader caches.
//! Only the *contents* of these well-known folders are removed; files in use are skipped.
use crate::leftovers::dir_size;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkGroup {
    /// Stable id the UI translates (`userTemp`, `nvidiaDx`, ...)
    pub id: &'static str,
    pub path: String,
    pub size_bytes: u64,
}

fn targets() -> Vec<(&'static str, PathBuf)> {
    let var = |v: &str, rest: &str| env::var_os(v).map(|p| PathBuf::from(p).join(rest));
    [
        ("userTemp", env::var_os("TEMP").map(PathBuf::from)),
        ("windowsTemp", var("SystemRoot", "Temp")),
        ("crashDumps", var("LOCALAPPDATA", "CrashDumps")),
        ("errorReports", var("ProgramData", r"Microsoft\Windows\WER\ReportArchive")),
        ("errorQueue", var("ProgramData", r"Microsoft\Windows\WER\ReportQueue")),
        ("dxCache", var("LOCALAPPDATA", "D3DSCache")),
        ("nvidiaDx", var("LOCALAPPDATA", r"NVIDIA\DXCache")),
        ("nvidiaGl", var("LOCALAPPDATA", r"NVIDIA\GLCache")),
        ("amdDx", var("LOCALAPPDATA", r"AMD\DxCache")),
    ]
    .into_iter()
    .filter_map(|(id, p)| p.filter(|p| p.is_dir()).map(|p| (id, p)))
    .collect()
}

pub fn scan() -> Vec<JunkGroup> {
    targets()
        .into_iter()
        .map(|(id, p)| JunkGroup { id, size_bytes: dir_size(&p), path: p.to_string_lossy().into_owned() })
        .collect()
}

fn empty_dir(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        // Ignore failures: those are files Windows or a running app still holds open
        let _ = if e.file_type().is_ok_and(|t| t.is_dir()) { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
    }
}

/// Empties the selected groups and returns the bytes actually freed.
pub fn clean(ids: &[String]) -> u64 {
    targets()
        .into_iter()
        .filter(|(id, _)| ids.iter().any(|i| i == id))
        .map(|(_, p)| {
            let before = dir_size(&p);
            empty_dir(&p);
            before.saturating_sub(dir_size(&p))
        })
        .sum()
}
