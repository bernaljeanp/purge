//! Microsoft Store (AppX/MSIX) apps, via the built-in Appx PowerShell module.
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreApp {
    pub name: String,
    pub full_name: String,
    pub publisher: String,
    pub version: String,
    /// base64 PNG of the app's logo, may be empty
    pub logo: String,
}

fn powershell(script: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// ponytail: reads every manifest, ~5s on a typical machine; cache in the UI, not here.
pub fn list() -> Result<Vec<StoreApp>, String> {
    const SCRIPT: &str = r#"
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$r = foreach ($p in Get-AppxPackage -PackageTypeFilter Main) {
  if ($p.IsFramework -or $p.NonRemovable -or $p.SignatureKind -eq 'System') { continue }
  $m = $null; try { $m = Get-AppxPackageManifest $p } catch {}
  $dn = $m.Package.Properties.DisplayName
  if (-not $dn -or $dn -like 'ms-resource:*') { $dn = (($p.Name -split '\.')[-1]) -creplace '([a-z])([A-Z])', '$1 $2' }
  $pub = $m.Package.Properties.PublisherDisplayName
  if (-not $pub -or $pub -like 'ms-resource:*') { $pub = '' }
  $logo = ''
  $rel = $m.Package.Properties.Logo
  if ($rel -and $p.InstallLocation) {
    try {
      $full = Join-Path $p.InstallLocation $rel
      $stem = [IO.Path]::GetFileNameWithoutExtension($full)
      $f = Get-ChildItem -LiteralPath (Split-Path $full) -Filter "$stem*.png" -ErrorAction Stop |
        Where-Object { $_.Length -lt 60000 } | Sort-Object Length -Descending | Select-Object -First 1
      if ($f) { $logo = [Convert]::ToBase64String([IO.File]::ReadAllBytes($f.FullName)) }
    } catch {}
  }
  [pscustomobject]@{ name = $dn; fullName = $p.PackageFullName; publisher = $pub; version = $p.Version.ToString(); logo = $logo }
}
ConvertTo-Json -InputObject @($r) -Compress
"#;
    let out = powershell(SCRIPT)?;
    let mut apps: Vec<StoreApp> = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

pub fn remove(full_name: &str) -> Result<(), String> {
    // Package full names are [A-Za-z0-9._~-]; reject anything that could break out of the quotes
    if full_name.is_empty() || !full_name.chars().all(|c| c.is_ascii_alphanumeric() || "._~-".contains(c)) {
        return Err(format!("invalid package name: {full_name}"));
    }
    powershell(&format!("Remove-AppxPackage -Package '{full_name}' -ErrorAction Stop")).map(|_| ())
}
