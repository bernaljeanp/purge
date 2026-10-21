mod disk;
mod icons;
mod junk;
mod leftovers;
mod orphans;
mod programs;
mod quarantine;
mod shell;
mod startup;
mod store;
mod tracker;
mod uninstall;

#[tauri::command(async)]
fn scan_orphans() -> Vec<orphans::Orphan> {
    orphans::scan(&programs::list())
}

#[tauri::command]
fn disk_drives() -> Vec<disk::Drive> {
    disk::drives()
}

#[tauri::command(async)]
fn disk_analyze(path: String) -> Result<Vec<disk::DiskEntry>, String> {
    disk::analyze(&path)
}

#[tauri::command]
fn context_menu_enabled() -> bool {
    shell::is_registered()
}

#[tauri::command]
fn set_context_menu(enabled: bool, label: String) -> Result<(), String> {
    shell::set(enabled, &label)
}

/// Program passed on the command line by the "Uninstall with Purge" menu, if any.
#[tauri::command(async)]
fn launch_request() -> Option<Program> {
    shell::launch_request()
}

#[tauri::command(async)]
fn list_store_apps() -> Result<Vec<store::StoreApp>, String> {
    store::list()
}

#[tauri::command(async)]
fn remove_store_app(full_name: String) -> Result<(), String> {
    store::remove(&full_name)
}

#[tauri::command(async)]
fn scan_junk() -> Vec<junk::JunkGroup> {
    junk::scan()
}

#[tauri::command(async)]
fn clean_junk(ids: Vec<String>) -> u64 {
    junk::clean(&ids)
}

#[tauri::command]
fn tracker_started() -> Option<u64> {
    tracker::started()
}

#[tauri::command(async)]
fn tracker_start() -> Result<(), String> {
    tracker::start()
}

#[tauri::command]
fn tracker_cancel() {
    tracker::cancel()
}

#[tauri::command(async)]
fn tracker_finish() -> Result<Vec<Leftover>, String> {
    tracker::finish()
}

#[tauri::command(async)]
fn save_trace(name: String, items: Vec<Leftover>) -> Result<tracker::Trace, String> {
    tracker::save(&name, items)
}

#[tauri::command(async)]
fn list_traces() -> Vec<tracker::Trace> {
    tracker::list()
}

#[tauri::command]
fn delete_trace(id: String) -> Result<(), String> {
    tracker::delete(&id)
}

use leftovers::{Leftover, ScanResult};
use programs::Program;
use quarantine::{Outcome, Session};
use startup::StartupItem;
use std::collections::HashMap;

/// Raw `DisplayIcon`/exe strings in, base64 PNGs out (keyed by the raw string).
#[tauri::command(async)]
fn program_icons(sources: Vec<String>) -> HashMap<String, String> {
    let cleaned: Vec<(String, String)> = sources
        .into_iter()
        .map(|raw| (icons::clean_icon_path(&raw), raw))
        .filter(|(clean, _)| !clean.is_empty())
        .collect();
    let paths: Vec<String> = cleaned.iter().map(|(c, _)| c.clone()).collect();
    let found = icons::extract(&paths);
    cleaned
        .into_iter()
        .filter_map(|(clean, raw)| found.get(&clean).map(|png| (raw, png.clone())))
        .collect()
}

#[tauri::command(async)]
fn list_startup() -> Vec<StartupItem> {
    startup::list()
}

#[tauri::command(async)]
fn set_startup(approved_key: String, name: String, enabled: bool) -> Result<(), String> {
    startup::set_enabled(&approved_key, &name, enabled)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("not a folder: {path}"));
    }
    std::process::Command::new("explorer").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// `async` so registry/disk work runs off the UI thread.
#[tauri::command(async)]
fn list_programs() -> Vec<Program> {
    programs::list()
}

#[tauri::command]
fn is_admin() -> bool {
    programs::is_admin()
}

#[tauri::command(async)]
fn uninstall(program: Program, quiet: bool) -> Result<i32, String> {
    let command = if quiet {
        uninstall::quiet_command(&program.uninstall_string, &program.quiet_uninstall_string)
            .unwrap_or_else(|| program.uninstall_string.clone())
    } else {
        program.uninstall_string.clone()
    };
    let code = uninstall::run(&command)?;
    // Squirrel/Electron and some NSIS uninstallers return immediately and keep working
    // in the background; give them time to drop their uninstall entry before we scan.
    // ponytail: fixed 30s cap; track the spawned process tree if this proves too short.
    for _ in 0..30 {
        if !programs::reg_key_exists(&program.id) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Ok(code)
}

#[tauri::command(async)]
fn scan_leftovers(program: Program) -> ScanResult {
    leftovers::scan(&program)
}

#[tauri::command(async)]
fn quarantine_items(program: String, items: Vec<Leftover>) -> Result<Outcome, String> {
    quarantine::quarantine(&program, &items)
}

/// Permanent removal = quarantine (with all its safety checks) + drop that session.
#[tauri::command(async)]
fn delete_items(program: String, items: Vec<Leftover>) -> Result<Outcome, String> {
    let outcome = quarantine::quarantine(&program, &items)?;
    if let Some(id) = &outcome.session {
        quarantine::delete(id)?;
    }
    Ok(outcome)
}

#[tauri::command(async)]
fn list_quarantine() -> Vec<Session> {
    quarantine::list()
}

#[tauri::command(async)]
fn restore_session(id: String) -> Result<Outcome, String> {
    quarantine::restore(&id)
}

#[tauri::command(async)]
fn delete_session(id: String) -> Result<(), String> {
    quarantine::delete(&id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            list_programs,
            is_admin,
            program_icons,
            scan_orphans,
            disk_drives,
            disk_analyze,
            context_menu_enabled,
            set_context_menu,
            launch_request,
            list_store_apps,
            remove_store_app,
            scan_junk,
            clean_junk,
            tracker_started,
            tracker_start,
            tracker_cancel,
            tracker_finish,
            save_trace,
            list_traces,
            delete_trace,
            list_startup,
            set_startup,
            open_folder,
            uninstall,
            scan_leftovers,
            quarantine_items,
            delete_items,
            list_quarantine,
            restore_session,
            delete_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
