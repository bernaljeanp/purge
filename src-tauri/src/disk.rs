//! Disk analyzer: sizes of everything directly inside a folder, measured in parallel.
use crate::leftovers::dir_size;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub is_dir: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub path: String,
    pub total: u64,
    pub free: u64,
}

pub fn drives() -> Vec<Drive> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    ('A'..='Z')
        .map(|c| format!(r"{c}:\"))
        .filter(|d| Path::new(d).exists())
        .map(|path| {
            let (mut free, mut total) = (0u64, 0u64);
            // Unreadable drives (empty card reader...) just show no bar
            let _ = unsafe { GetDiskFreeSpaceExW(&HSTRING::from(path.as_str()), Some(&mut free), Some(&mut total), None) };
            Drive { path, total, free }
        })
        .collect()
}

pub fn analyze(path: &str) -> Result<Vec<DiskEntry>, String> {
    let entries: Vec<(PathBuf, bool, u64)> = fs::read_dir(path)
        .map_err(|e| format!("{path}: {e}"))?
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            // Junctions and symlinks would double count (or loop); skip them
            (!m.file_type().is_symlink()).then(|| (e.path(), m.is_dir(), if m.is_dir() { 0 } else { m.len() }))
        })
        .collect();

    // Measure folders on a few threads; a drive root has ~20, WinSxS has thousands
    let dirs: Vec<&PathBuf> = entries.iter().filter(|(_, d, _)| *d).map(|(p, _, _)| p).collect();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let chunk = dirs.len().div_ceil(workers).max(1);
    let sizes: Vec<(PathBuf, u64)> = std::thread::scope(|s| {
        dirs.chunks(chunk)
            .map(|c| s.spawn(move || c.iter().map(|p| ((*p).clone(), dir_size(p))).collect::<Vec<_>>()))
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });

    let mut out: Vec<DiskEntry> = entries
        .into_iter()
        .map(|(p, is_dir, len)| {
            let size_bytes = if is_dir { sizes.iter().find(|(d, _)| *d == p).map_or(0, |(_, s)| *s) } else { len };
            DiskEntry {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p.to_string_lossy().into_owned(),
                size_bytes,
                is_dir,
            }
        })
        .collect();
    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    Ok(out)
}
