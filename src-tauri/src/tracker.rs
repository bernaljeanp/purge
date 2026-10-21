//! Install tracker: snapshot before an install, diff after, keep the list of what it
//! created ("trace") so it can be removed completely later through the quarantine.
use crate::leftovers::{Kind, Leftover, REG_ROOTS, child_keys, dir_size, file_roots};
use crate::programs::UNINSTALL_ROOTS;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub id: String,
    pub name: String,
    /// Unix milliseconds
    pub created: u64,
    pub items: Vec<Leftover>,
}

fn base() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Purge").join("tracker")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn roots() -> Vec<PathBuf> {
    let mut r = file_roots();
    let var = |v: &str, rest: &str| env::var_os(v).map(|p| PathBuf::from(p).join(rest));
    r.extend(var("APPDATA", r"Microsoft\Windows\Start Menu\Programs"));
    r.extend(var("ProgramData", r"Microsoft\Windows\Start Menu\Programs"));
    r
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

/// Two levels of folders/files under the install roots and two levels of registry keys.
/// Entries are `F|path` or `R|path`.
fn snapshot() -> Vec<String> {
    let mut out = Vec::new();
    for root in roots() {
        for c in children(&root) {
            if c.is_dir() {
                out.extend(children(&c).into_iter().map(|g| format!("F|{}", g.display())));
            }
            out.push(format!("F|{}", c.display()));
        }
    }
    let uninstall: Vec<String> = UNINSTALL_ROOTS.iter().map(|(h, p)| format!(r"{h}\{p}")).collect();
    for root in REG_ROOTS.iter().map(|s| s.to_string()).chain(uninstall) {
        for (k, _) in child_keys(&root) {
            out.extend(child_keys(&k).into_iter().map(|(g, _)| format!("R|{g}")));
            out.push(format!("R|{k}"));
        }
    }
    out
}

pub fn started() -> Option<u64> {
    let meta = fs::metadata(base().join("baseline.json")).ok()?;
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

pub fn start() -> Result<(), String> {
    fs::create_dir_all(base()).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec(&snapshot()).map_err(|e| e.to_string())?;
    fs::write(base().join("baseline.json"), json).map_err(|e| e.to_string())
}

pub fn cancel() {
    let _ = fs::remove_file(base().join("baseline.json"));
}

/// Everything that appeared since `start()`, nested entries collapsed into their parent.
pub fn finish() -> Result<Vec<Leftover>, String> {
    let bytes = fs::read(base().join("baseline.json")).map_err(|_| "tracking was not started".to_string())?;
    let before: HashSet<String> = serde_json::from_slice::<Vec<String>>(&bytes)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();

    let mut added: Vec<String> = snapshot().into_iter().filter(|s| !before.contains(&s.to_lowercase())).collect();
    added.sort_by_key(|s| s.len());
    let mut kept: Vec<String> = Vec::new();
    for a in added {
        let l = a.to_lowercase();
        if !kept.iter().any(|k| l.starts_with(&format!("{}\\", k.to_lowercase()))) {
            kept.push(a);
        }
    }
    cancel();

    Ok(kept
        .into_iter()
        .map(|entry| {
            let (tag, path) = entry.split_at(2);
            let path = path.to_string();
            if tag == "R|" {
                Leftover { kind: Kind::Registry, path, size_bytes: 0 }
            } else {
                let p = Path::new(&path);
                let size_bytes = if p.is_dir() { dir_size(p) } else { fs::metadata(p).map(|m| m.len()).unwrap_or(0) };
                Leftover { kind: Kind::File, path, size_bytes }
            }
        })
        .collect())
}

fn traces_dir() -> PathBuf {
    base().join("traces")
}

pub fn save(name: &str, items: Vec<Leftover>) -> Result<Trace, String> {
    fs::create_dir_all(traces_dir()).map_err(|e| e.to_string())?;
    let created = now_ms();
    let trace = Trace { id: created.to_string(), name: name.trim().to_string(), created, items };
    let json = serde_json::to_vec_pretty(&trace).map_err(|e| e.to_string())?;
    fs::write(traces_dir().join(format!("{}.json", trace.id)), json).map_err(|e| e.to_string())?;
    Ok(trace)
}

pub fn list() -> Vec<Trace> {
    let Ok(rd) = fs::read_dir(traces_dir()) else { return vec![] };
    let mut out: Vec<Trace> = rd
        .flatten()
        .filter_map(|e| fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn delete(id: &str) -> Result<(), String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid trace id: {id}"));
    }
    fs::remove_file(traces_dir().join(format!("{id}.json"))).map_err(|e| e.to_string())
}
