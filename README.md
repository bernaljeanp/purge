# Purge

A clean, modern uninstaller for Windows. Removes programs **and** the leftovers they leave behind — safely.

Built with [Tauri 2](https://tauri.app) (Rust) + React. Tiny, fast, no runtime to install.

## Features

- **All your programs** from the registry (64-bit, 32-bit and per-user), with size, version and install date. Search and sort.
- **Batch uninstall** — select several programs and run their uninstallers one after another.
- **Leftover scan** — after each uninstall, finds remaining folders (Program Files, ProgramData, AppData) and registry keys.
- **Quarantine, not delete** — leftovers are moved to `%LOCALAPPDATA%\Purge\quarantine` with a manifest (registry keys exported to `.reg`), so every removal can be **restored**. Space is only freed when you delete a quarantine entry.
- **Safety rails** — never touches Windows, scan roots, your profile or vendor umbrella folders (`Microsoft`, `Common Files`, …). If a program still looks installed after its uninstaller ran, nothing is pre-selected.

## Roadmap

- [ ] Program icons
- [ ] Force removal (programs with a broken uninstaller)
- [ ] Microsoft Store apps
- [ ] Startup manager
- [ ] Install tracker (snapshot before/after an install for a perfect uninstall later)

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
