# Notes for coding agents

- Windows x64 and Linux. Build `cargo build --release --workspace`, test `cargo test --workspace`. Two crates: the tray app (root) and the Steam hook (`steam-hook/`: a DLL on Windows, an LD_AUDIT module on Linux). Platform code sits behind `cfg(windows)` / `cfg(target_os = "linux")`: `src/steam/{windows,linux}.rs`, `steam-hook/src/{windows,linux}.rs`.
- Linux: Steam is a 32-bit client, so also `cargo build --release -p paddle-keys-steam --target i686-unknown-linux-gnu` and `cargo test -p paddle-keys-steam --target i686-unknown-linux-gnu`. A new hook build needs a full Steam restart (`steam -shutdown`, wait for the `steam.sh` shell to exit too), not a power-cycle. Hook log: `$XDG_RUNTIME_DIR/paddle-keys-hook.log`.
- Read first: `docs/PROTOCOL.md` (8BitDo vendor HID channel), `docs/STEAM.md` (Steam Elite mode, with dead ends), `docs/STEAM-LINUX.md` (the Linux hook), `docs/ADDING-CONTROLLERS.md` (how to add a device, test checklist).
- Never send a controller anything except the proven read command. Vendor channels also carry firmware update commands.
- The hook runs inside `steam.exe`: no panics, no blocking, keep everything keyed on the sideband device id. A new hook build needs a Steam restart and a controller power-cycle to take effect.
- Diagnostics live in `tools/` (Python, need `frida`, `hidapi`).
