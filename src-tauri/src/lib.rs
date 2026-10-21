mod leftovers;
mod programs;
mod quarantine;
mod uninstall;

use leftovers::{Leftover, ScanResult};
use programs::Program;
use quarantine::{Outcome, Session};

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
fn uninstall(program: Program) -> Result<i32, String> {
    let code = uninstall::run(&program.uninstall_string)?;
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
        .invoke_handler(tauri::generate_handler![
            list_programs,
            is_admin,
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
