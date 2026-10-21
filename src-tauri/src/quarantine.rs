//! Nothing is deleted outright: leftovers are moved here first, with a manifest,
//! so every removal can be restored. Only "Delete" on a session frees the space.
use crate::leftovers::{Kind, Leftover, is_safe_dir};
use crate::programs::split_reg_path;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, io};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ERROR_NOT_SAME_DEVICE: i32 = 17;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub kind: Kind,
    pub original: String,
    pub stored: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub program: String,
    /// Unix milliseconds
    pub created: u64,
    pub items: Vec<Entry>,
}

#[derive(Serialize)]
pub struct Outcome {
    pub done: usize,
    pub errors: Vec<String>,
    /// Quarantine session created, if anything was moved
    #[serde(skip)]
    pub session: Option<String>,
}

fn base() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Purge").join("quarantine")
}

fn session_dir(id: &str) -> Result<PathBuf, String> {
    // ids are our own millisecond timestamps; anything else could escape the quarantine folder
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid session id: {id}"));
    }
    Ok(base().join(id))
}

fn write_manifest(dir: &Path, s: &Session) -> io::Result<()> {
    fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(s)?)
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_file() {
        return fs::copy(src, dst).map(|_| ());
    }
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        copy_dir(&e.path(), &dst.join(e.file_name()))?;
    }
    Ok(())
}

/// Rename; across drives fall back to copy + delete, never leaving a half-moved source.
fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Err(e) if e.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) => {
            copy_dir(src, dst)?;
            let removed = if src.is_dir() { fs::remove_dir_all(src) } else { fs::remove_file(src) };
            if let Err(e) = removed {
                let _ = if dst.is_dir() { fs::remove_dir_all(dst) } else { fs::remove_file(dst) };
                return Err(e);
            }
            Ok(())
        }
        r => r,
    }
}

fn reg(args: &[&str]) -> Result<(), String> {
    let out = Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn backup_and_delete_key(path: &str, file: &Path) -> Result<(), String> {
    reg(&["export", path, &file.to_string_lossy(), "/y"])?;
    let (root, sub) = split_reg_path(path).ok_or("unsupported registry hive")?;
    root.delete_subkey_all(sub).map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn quarantine(program: &str, items: &[Leftover]) -> Result<Outcome, String> {
    let created = now_ms();
    let id = created.to_string();
    let dir = session_dir(&id)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut session = Session { id, program: program.to_string(), created, items: vec![] };
    let mut errors = Vec::new();

    for (i, it) in items.iter().enumerate() {
        let result = match it.kind {
            Kind::File => {
                let src = Path::new(&it.path);
                if !is_safe_dir(src) {
                    Err("refused: protected location".to_string())
                } else {
                    let stored = dir.join(i.to_string());
                    move_path(src, &stored).map(|_| stored).map_err(|e| e.to_string())
                }
            }
            Kind::Registry => {
                let stored = dir.join(format!("{i}.reg"));
                backup_and_delete_key(&it.path, &stored).map(|_| stored)
            }
        };
        match result {
            Ok(stored) => {
                session.items.push(Entry {
                    kind: it.kind,
                    original: it.path.clone(),
                    stored: stored.to_string_lossy().into_owned(),
                    size_bytes: it.size_bytes,
                });
                // Persist after every item so a crash never loses track of moved data
                write_manifest(&dir, &session).map_err(|e| e.to_string())?;
            }
            Err(e) => errors.push(format!("{}: {e}", it.path)),
        }
    }

    if session.items.is_empty() {
        let _ = fs::remove_dir_all(&dir);
    }
    let created = (!session.items.is_empty()).then(|| session.id.clone());
    Ok(Outcome { done: session.items.len(), errors, session: created })
}

pub fn list() -> Vec<Session> {
    let Ok(rd) = fs::read_dir(base()) else { return vec![] };
    let mut out: Vec<Session> = rd
        .flatten()
        .filter_map(|e| fs::read(e.path().join("manifest.json")).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn restore(id: &str) -> Result<Outcome, String> {
    let dir = session_dir(id)?;
    let bytes = fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?;
    let mut session: Session = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;

    let mut errors = Vec::new();
    let mut remaining = Vec::new();
    let mut done = 0;
    for entry in session.items.drain(..) {
        let r = match entry.kind {
            Kind::File => {
                let original = Path::new(&entry.original);
                if original.exists() {
                    Err("something already exists at the original location".to_string())
                } else {
                    original
                        .parent()
                        .map_or(Ok(()), fs::create_dir_all)
                        .and_then(|_| move_path(Path::new(&entry.stored), original))
                        .map_err(|e| e.to_string())
                }
            }
            Kind::Registry => reg(&["import", &entry.stored]),
        };
        match r {
            Ok(()) => done += 1,
            Err(e) => {
                errors.push(format!("{}: {e}", entry.original));
                remaining.push(entry);
            }
        }
    }

    if remaining.is_empty() {
        let _ = fs::remove_dir_all(&dir);
    } else {
        session.items = remaining;
        write_manifest(&dir, &session).map_err(|e| e.to_string())?;
    }
    Ok(Outcome { done, errors, session: None })
}

/// Permanently frees a quarantine session. Only ever triggered by an explicit user click.
pub fn delete(id: &str) -> Result<(), String> {
    fs::remove_dir_all(session_dir(id)?).map_err(|e| e.to_string())
}
