# Purge

A clean, modern uninstaller for Windows. Removes programs **and** the leftovers they leave behind — safely.

Built with [Tauri 2](https://tauri.app) (Rust) + React. Tiny, fast, no runtime to install.

## Features

- **Programs** — every installed program (64-bit, 32-bit, per-user) with real icons, size, version and install date. A capacity strip shows what is eating your disk. Search, sort, export to CSV.
- **Batch uninstall + leftover scan** — runs each program's own uninstaller, waits for background uninstallers, then finds remaining folders and registry keys.
- **Quarantine, not delete** — leftovers are moved aside (registry exported to `.reg`) so every removal can be **restored**. Or delete permanently, your call.
- **Force remove** — for programs whose uninstaller is broken or missing; broken entries are flagged.
- **Store apps** — list and remove Microsoft Store apps.
- **Startup** — enable or disable apps that launch at sign-in (same flags Task Manager uses).
- **Cleaner** — temp files, crash dumps, error reports and DirectX / NVIDIA / AMD shader caches.
- **Install tracker** — snapshot before installing something, diff after, and keep a trace to remove *everything* it created later.
- **Speaks your language** — follows the Windows display language (English, Spanish).
- **Safety rails** — never touches Windows, scan roots, your profile or vendor umbrella folders.

## Development

Requires Rust, Node 20+ and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # dev build (runs unelevated)
npm run tauri build    # release installer (requests admin)
cd src-tauri && cargo test
```

## License

Apache-2.0. Leftover-detection ideas inspired by [Bulk Crap Uninstaller](https://github.com/BCUninstaller/Bulk-Crap-Uninstaller).
