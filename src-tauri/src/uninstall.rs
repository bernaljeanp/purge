use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ERROR_ELEVATION_REQUIRED: i32 = 740;

/// Splits a registry UninstallString into (executable, raw arguments).
pub fn split_command(cmd: &str) -> (String, String) {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    // Unquoted paths with spaces: cut right after ".exe"
    if let Some(i) = cmd.to_ascii_lowercase().find(".exe") {
        let end = i + 4;
        return (cmd[..end].to_string(), cmd[end..].trim().to_string());
    }
    match cmd.split_once(' ') {
        Some((exe, args)) => (exe.to_string(), args.trim().to_string()),
        None => (cmd.to_string(), String::new()),
    }
}

/// Many MSI entries register `MsiExec.exe /I{GUID}` (repair/modify); `/X` is the uninstall.
fn fix_msiexec(exe: &str, args: &str) -> String {
    let is_msi = exe.to_ascii_lowercase().ends_with("msiexec.exe")
        || exe.eq_ignore_ascii_case("msiexec");
    if is_msi && (args.starts_with("/I") || args.starts_with("/i")) {
        format!("/X{}", &args[2..])
    } else {
        args.to_string()
    }
}

/// An unattended variant of the uninstall command, when we know one is safe:
/// the program's own QuietUninstallString, MSI (`/qn`) or Inno Setup (`/VERYSILENT`).
/// None means "no known silent mode" and the caller falls back to the normal wizard.
pub fn quiet_command(uninstall_string: &str, quiet_uninstall_string: &str) -> Option<String> {
    if !quiet_uninstall_string.trim().is_empty() {
        return Some(quiet_uninstall_string.trim().to_string());
    }
    let (exe, args) = split_command(uninstall_string);
    let file = exe.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
    if file == "msiexec.exe" || file == "msiexec" {
        return Some(format!("{exe} {} /qn /norestart", fix_msiexec(&exe, &args)));
    }
    // Inno Setup always names it unins000.exe, unins001.exe, ...
    let inno = file.len() == 12 && file.starts_with("unins") && file.ends_with(".exe") && file[5..8].chars().all(|c| c.is_ascii_digit());
    if inno {
        return Some(format!("\"{exe}\" {args} /VERYSILENT /SUPPRESSMSGBOXES /NORESTART").replace("  ", " "));
    }
    None
}

/// Runs the program's own uninstaller and waits for it. Returns its exit code.
/// ponytail: waits on the launched process only; NSIS-style uninstallers that respawn
/// from %TEMP% return early, so the UI offers a manual rescan. Track child processes if that bites.
pub fn run(uninstall_string: &str) -> Result<i32, String> {
    let (exe, args) = split_command(uninstall_string);
    let args = fix_msiexec(&exe, &args);

    match Command::new(&exe).raw_arg(&args).status() {
        Ok(s) => Ok(s.code().unwrap_or(-1)),
        Err(e) if e.raw_os_error() == Some(ERROR_ELEVATION_REQUIRED) => run_elevated(&exe, &args),
        Err(e) => Err(format!("{exe}: {e}")),
    }
}

/// Re-launches through UAC (needed in unelevated dev builds).
fn run_elevated(exe: &str, args: &str) -> Result<i32, String> {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let arg_list = if args.is_empty() { String::new() } else { format!(" -ArgumentList {}", q(args)) };
    let script = format!(
        "$p = Start-Process -FilePath {}{} -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        q(exe),
        arg_list
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .map_err(|e| format!("{exe}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_uninstall_strings() {
        let s = |c: &str| split_command(c);
        assert_eq!(s(r#""C:\Program Files\7-Zip\Uninstall.exe""#), (r"C:\Program Files\7-Zip\Uninstall.exe".into(), "".into()));
        assert_eq!(s(r#""C:\a b\unins000.exe" /SILENT"#), (r"C:\a b\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(s(r"C:\Program Files\Foo\uninst.exe /S"), (r"C:\Program Files\Foo\uninst.exe".into(), "/S".into()));
        assert_eq!(s("MsiExec.exe /I{ABC}"), ("MsiExec.exe".into(), "/I{ABC}".into()));
        assert_eq!(fix_msiexec("MsiExec.exe", "/I{ABC}"), "/X{ABC}");
        assert_eq!(fix_msiexec(r"C:\x\setup.exe", "/I{ABC}"), "/I{ABC}");
    }

    #[test]
    fn picks_silent_modes() {
        assert_eq!(quiet_command("MsiExec.exe /I{ABC}", "").as_deref(), Some("MsiExec.exe /X{ABC} /qn /norestart"));
        assert_eq!(
            quiet_command(r#""C:\Program Files\App\unins000.exe""#, "").as_deref(),
            Some(r#""C:\Program Files\App\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART"#)
        );
        assert_eq!(quiet_command(r"C:\App\uninstall.exe", r"C:\App\uninstall.exe /S").as_deref(), Some(r"C:\App\uninstall.exe /S"));
        assert_eq!(quiet_command(r"C:\App\uninstall.exe", ""), None);
    }
}
