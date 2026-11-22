# Notes for coding agents

- Windows only, x64. Build `cargo build --release --workspace`, test `cargo test --workspace`. Two crates: the tray app (root) and the Steam hook DLL (`steam-hook/`).
- Read first: `docs/PROTOCOL.md` (8BitDo vendor HID channel), `docs/STEAM.md` (Steam Elite mode, with dead ends), `docs/ADDING-CONTROLLERS.md` (how to add a device, test checklist).
- Never send a controller anything except the proven read command. Vendor channels also carry firmware update commands.
- The hook runs inside `steam.exe`: no panics, no blocking, keep everything keyed on the sideband device id. A new hook build needs a Steam restart and a controller power-cycle to take effect.
- Diagnostics live in `tools/` (Python, need `frida`, `hidapi`).
