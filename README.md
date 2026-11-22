# paddle-keys

Use the back paddles (L4, R4, PL, PR) of the **8BitDo Ultimate 2 Wireless** while the controller stays in **XInput mode**, so you keep the full 16-bit stick resolution.

In XInput mode the paddles are invisible to games: XInput has no fields for them. In DInput mode they work (Steam Input sees them), but the sticks drop to 8 bits, only 255 steps. `paddle-keys` is a small tray app that reads the paddles over the controller's vendor HID channel and presses keyboard keys while they are held, by default F13 to F16. Bind those keys in Steam Input, your game, or anything else.

| Paddle | Default key |
|--------|-------------|
| L4     | F13 |
| R4     | F14 |
| PL     | F15 |
| PR     | F16 |

XInput is never opened or modified. Your sticks, triggers and buttons behave exactly as before.

Optional: **Steam Elite mode** makes Steam Input see the controller as an Xbox Elite Series 2, so the paddles show up as real paddle buttons (P1 to P4) in Steam, next to full 16-bit sticks, rumble and the Xbox button. It is off by default; see [Steam Elite mode](#steam-elite-mode).

Works on **Windows** and **Linux**. Linux specifics are in [Linux](#linux).

## Install (Windows)

1. Download `paddle-keys-<version>-windows-x64.zip` from [Releases](../../releases) and extract it. The newest Windows build is [v0.2.0](../../releases/tag/v0.2.0); 0.3.0 only added Linux. Keep `paddle-keys.exe` and `paddle_keys_steam.dll` in the same folder. Compare them with `SHA256SUMS.txt` if you like.
2. Close the 8BitDo Ultimate Software. It talks to the same channel and the two will fight over replies.
3. Run `paddle-keys.exe`. A tray icon appears (it may be under the `^` overflow arrow).

Windows SmartScreen or antivirus may warn about an unsigned program that reads a HID device and sends keystrokes. The source is in this repo and the release is built from it; build it yourself if you prefer.

### Tray menu

- **Paused:** stop pressing keys (held keys are released).
- **Steam Elite mode:** see below.
- **Start with Windows:** adds or removes a per-user Run entry.
- **Open config (restart to apply):** opens the config file in Notepad.
- **Quit**

## Configuration

`%APPDATA%\paddle-keys\config.toml` (Linux: `~/.config/paddle-keys/config.toml`) is created on first run:

```toml
[keys]
L4 = "F13"
R4 = "F14"
PL = "F15"
PR = "F16"
```

Values are an optional list of modifiers joined with `+`, then one key: `F1` to `F24`, `A` to `Z`, `0` to `9`, `Space`, `Enter`, `Esc`, `Tab`. Modifiers are `Ctrl`, `Shift`, `Alt`, `Win` (Super on Linux). Example: `L4 = "Ctrl+Shift+F13"`. A bad value falls back to that paddle's default. Restart the app after editing.

## Behavior

- A paddle held means its key is held. Releasing it releases the key.
- If the controller turns off or is unplugged while a paddle is held, the key is released and the app reconnects by itself within a few seconds.
- Keys do not reach windows running as administrator unless `paddle-keys` is also run as administrator.
- Only one copy runs at a time.
- Gyro is not exposed. The Motion feature in the 8BitDo software runs inside the controller and turns movement into right-stick input, which XInput already carries.

## Steam Elite mode

Steam reads Xbox controllers through Valve's Xbox Extended Feature Support driver and its own copy of SDL. With this mode on, `paddle-keys` loads a small hook (`paddle_keys_steam.dll`) into `steam.exe` that presents the 8BitDo as an Xbox Elite Series 2 controller, translates its reports, and feeds Steam the paddle state that `paddle-keys` reads. In Steam's controller settings the paddles then appear as P1 to P4 and can be bound like on a real Elite. Details: [docs/STEAM.md](docs/STEAM.md).

**Requirements**

1. In Steam: **Settings > Controller**, turn on **Enable Steam Input for Xbox controllers**.
2. In the same page, under advanced settings, install **Xbox Extended Feature Support**, then reboot.
3. The controller in XInput mode.

**Use it**

1. Tray menu > **Steam Elite mode** > **Enabled**.
2. Turn the controller off and on. Steam picks up controllers within seconds of starting, before the hook is in, so this is also needed after Steam restarts. The status line in the submenu shows what is missing, or that Steam sees an Elite 2.
3. Steam lists one controller, the Xbox Elite 2. While the mode is on, Steam ignores the 8BitDo's own entries.

| Paddle | Elite paddle |
|--------|--------------|
| R4 | P1 (upper right) |
| PR | P2 (lower right) |
| L4 | P3 (upper left) |
| PL | P4 (lower left) |

**Where the paddles go** (same submenu):

- **Steam when active, else keys** (default): paddles go to Steam while it has the controller as an Elite 2, and to F13-F16 otherwise (DInput mode, Steam closed, mode just enabled).
- **Keys only**, **Steam only**.
- **Keys and Steam at once**: both, which can double up inputs.

**Good to know**

- This injects code into the Steam client, the same way Steam skin and plugin loaders do. It does not touch games or their anti-cheat, and VAC scans games, not the Steam client, but Valve has never said anything about tools like this. Use it at your own risk.
- Antivirus may flag loading a DLL into another process. The source is here.
- Games that turn Steam Input off get the plain Xbox controller without paddles. In the default mode the paddles then do nothing; pick **Keys only** for those games.
- The hook stays loaded until Steam exits. Turning the mode off, or quitting `paddle-keys`, stops it from taking over newly connected controllers and releases the paddles; a controller that is already taken over stays an Elite 2 until you turn it off and on.
- Triggers keep the controller's 8-bit resolution (scaled to the Elite's 10-bit range).
- Only the Ultimate 2 Wireless in XInput mode is supported. Want another controller? See [docs/ADDING-CONTROLLERS.md](docs/ADDING-CONTROLLERS.md).

## Linux

The same app and the same Steam Elite mode, built for Linux. Nothing here needs root at runtime.

### Install

1. Download `paddle-keys-<version>-linux-x86_64.tar.gz` from the [latest release](../../releases/latest) and extract it anywhere, for example `~/.local/opt/paddle-keys`. Keep `paddle-keys`, `libpaddle_keys_steam32.so` and `libpaddle_keys_steam64.so` in one folder. Compare with `SHA256SUMS.txt` if you like. It needs glibc 2.35 or newer and the GTK 3, libxdo and AppIndicator libraries (Arch: `gtk3 xdotool libayatana-appindicator`; Debian/Ubuntu: `libgtk-3-0 libxdo3 libayatana-appindicator3-1`). Or build it yourself (see [Build](#build)).
2. Access: paddle-keys needs read/write on the pad's vendor `hidraw` node (interface 2) and on `/dev/uinput`. Steam's udev rules (`60-steam-input.rules`) give the logged-in user `/dev/uinput`. Check the pad with `paddle-keys --probe 5`; if it says not found, add a rule such as `/etc/udev/rules.d/70-paddle-keys.rules`:

   ```
   KERNEL=="hidraw*", ATTRS{idVendor}=="2dc8", ATTRS{idProduct}=="310b", TAG+="uaccess"
   ```

   then `sudo udevadm control --reload && sudo udevadm trigger`.
3. Close the 8BitDo Ultimate Software if you run it under Wine, for the same reason as on Windows.
4. Run `paddle-keys`. The tray icon needs a StatusNotifier host (KDE, GNOME with the AppIndicator extension, waybar's `tray` module, ...). On Hyprland, start it with `exec-once = paddle-keys`; "Start on login" writes an XDG autostart entry, which Hyprland does not read.

Keys are sent through a virtual keyboard named `paddle-keys` (uinput). Under X11/Wayland, F13 to F24 often map to keysyms like `XF86Tools` rather than `F13`; apps that read raw key codes (games, Steam Input, `wev` shows both) see the right key.

### Steam Elite mode on Linux

Steam on Linux reads the pad through its bundled SDL3 and the kernel's `xpad` driver, not through a Valve driver. The hook is an `LD_AUDIT` module that Steam loads at start, the same way SLSsteam (via headcrab) is loaded. It only touches the calls Steam's `libSDL3` makes on the 8BitDo's gamepad node, and presents it as a wired Xbox Elite Series 2 the way `xpad` presents a real one, with the paddles as P1 to P4. Details: [docs/STEAM-LINUX.md](docs/STEAM-LINUX.md).

1. In Steam: **Settings > Controller**, turn on **Enable Steam Input for Xbox controllers**. No extra driver is needed on Linux.
2. Tray menu > **Steam Elite mode** > **Enabled**, then **Install Steam hook (edits steam.sh)**. Or run `paddle-keys --install-steam-hook`. This adds the hook to the `LD_AUDIT` line in `~/.steam/steam/steam.sh` (headcrab's `INJECT_SLS` line if present, otherwise a new `export` after the shebang), marked `# paddle-keys`, after saving `steam.sh.paddle-keys.bak`. A read-only `steam.sh` stays read-only.
3. Fully quit Steam (`steam -shutdown`) and start it again. Because the hook is in from the start, no controller power-cycle is needed.

`paddle-keys --remove-steam-hook` (or the tray item) takes it out again. Steam updates that replace a stock `steam.sh` drop the line; the status then says to install it again. Routing, status and the rules about safety are the same as on Windows, except that there is no injection into a running Steam: if Steam was started before the hook was installed, the status says to restart Steam.

## Tested on

- 8BitDo Ultimate 2 Wireless Controller for PC, USB `2DC8:310B` (XInput mode), device release `0x0114`, Windows 11.
- Arch Linux, kernel 7.3, Hyprland, 32-bit Steam client of November 2026 with headcrab/SLSsteam loaded: paddles as keys and Steam Elite mode (paddles, buttons, sticks, triggers, guide and rumble in Steam's test page, power-cycle, default routing; release on a crashed paddle-keys checked with a stand-in for Steam's SDL).
- This is one unit. The firmware version is unknown. Other firmware may answer differently; please open an issue with the output of `paddle-keys.exe --probe 10` run from a terminal while you press each paddle.

## Troubleshooting

`paddle-keys.exe --probe 10` (Linux: `paddle-keys --probe 10`) runs for 10 seconds and prints the button mask whenever it changes, with the paddle names, then exits. If nothing prints, the controller is not found or another program holds the channel.

## How it works

See [docs/PROTOCOL.md](docs/PROTOCOL.md) for the controller and [docs/STEAM.md](docs/STEAM.md) for Steam Elite mode. The app sends one read-only command, nothing else, to the controller. Diagnostic scripts used during the research are in [tools/](tools/).

## Build

```
cargo build --release --workspace
```

This builds `paddle-keys.exe` and the Steam hook `paddle_keys_steam.dll`; keep them in one folder. Windows x64. Rust 1.85 or newer (2024 edition).

On Linux the same command builds `paddle-keys` and a 64-bit hook. Steam's client is 32-bit, so also build the 32-bit hook (Arch: `pacman -S lib32-rust-libs`; Debian/Ubuntu: `rustup target add i686-unknown-linux-gnu` plus `gcc-multilib`):

```
cargo build --release --workspace
cargo build --release -p paddle-keys-steam --target i686-unknown-linux-gnu
```

Run `target/release/paddle-keys` from there, or copy the hooks next to the binary as `libpaddle_keys_steam32.so` (from `target/i686-unknown-linux-gnu/release/`) and `libpaddle_keys_steam64.so` (from `target/release/`). paddle-keys stages them in `~/.local/share/paddle-keys/`.

## License

MIT, see [LICENSE](LICENSE). Not affiliated with 8BitDo.
