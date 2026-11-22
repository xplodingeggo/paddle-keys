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

Works on **Windows** and **Linux**: [install on Windows](#install-windows) or [install on Linux](#linux).

## Install (Windows)

1. Download `paddle-keys-<version>-windows-x64.zip` from the [latest release](../../releases/latest) and extract it. Windows zips start at 0.3.2; [v0.2.0](../../releases/tag/v0.2.0) is the older one.
2. Keep `paddle-keys.exe` and `paddle_keys_steam.dll` in the same folder. Compare them with `SHA256SUMS.txt` if you like.
3. Close the 8BitDo Ultimate Software. It talks to the same channel and the two will fight over replies.
4. Run `paddle-keys.exe`. A tray icon appears (it may be under the `^` overflow arrow).

Windows SmartScreen or antivirus may warn about an unsigned program that reads a HID device and sends keystrokes. The source is in this repo and the release is built from it; build it yourself if you prefer.

### Tray menu

- **Paused:** stop pressing keys (held keys are released).
- **Steam Elite mode:** see [Steam Elite mode](#steam-elite-mode).
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

Makes Steam Input see the controller as an **Xbox Elite Series 2**, so the paddles appear in Steam as real paddle buttons (P1 to P4) that you can bind like on a real Elite. You also keep full 16-bit sticks, rumble and the Xbox button. Off by default, on Windows and Linux.

| Paddle | Elite paddle |
|--------|--------------|
| R4 | P1 (upper right) |
| PR | P2 (lower right) |
| L4 | P3 (upper left) |
| PL | P4 (lower left) |

On Linux, set it up as described in [Steam Elite mode on Linux](#steam-elite-mode-on-linux). The rest of this section covers Windows, plus how paddles are routed on both.

### Windows setup

How it works: Steam reads Xbox controllers through Valve's Xbox Extended Feature Support driver and its own copy of SDL. `paddle-keys` loads a small hook (`paddle_keys_steam.dll`) into `steam.exe`. The hook presents the 8BitDo as an Elite 2, translates its reports and feeds Steam the paddle state. Details: [docs/STEAM.md](docs/STEAM.md).

**Requirements**

1. In Steam: **Settings > Controller**, turn on **Enable Steam Input for Xbox controllers**.
2. In the same page, under advanced settings, install **Xbox Extended Feature Support**, then reboot.
3. The controller in XInput mode.

**Turn it on**

1. Tray menu > **Steam Elite mode** > **Enabled**.
2. Turn the controller off and on. Steam picks up controllers within seconds of starting, before the hook is in, so this is also needed after Steam restarts.
3. The status line in the submenu shows what is missing, or that Steam sees an Elite 2. Steam then lists one controller and ignores the 8BitDo's own entries.

### Where the paddles go

In the tray, under Steam Elite mode (Windows and Linux):

| Option | What happens |
|--------|--------------|
| **Steam when active, else keys** (default) | Paddles go to Steam while it has the controller as an Elite 2, and to F13-F16 otherwise (DInput mode, Steam closed, mode just enabled). |
| **Keys only** | Paddles only press keys. |
| **Steam only** | Paddles only go to Steam. |
| **Keys and Steam at once** | Both, which can double up inputs. |

### Good to know

- **Risk (Windows):** the hook injects code into the Steam client, the same way Steam skin and plugin loaders do. It does not touch games or their anti-cheat, and VAC scans games, not the Steam client, but Valve has never said anything about tools like this. Use it at your own risk.
- **Antivirus** may flag loading a DLL into another process. The source is here.
- **Games with Steam Input off** get the plain Xbox controller without paddles. In the default mode the paddles then do nothing; pick **Keys only** for those games.
- **Turning it off:** the hook stays loaded until Steam exits. Turning the mode off, or quitting `paddle-keys`, stops it from taking over newly connected controllers and releases the paddles. A controller that is already taken over stays an Elite 2 until you turn it off and on.
- **Triggers** keep the controller's 8-bit resolution (scaled to the Elite's 10-bit range).
- **Supported:** only the Ultimate 2 Wireless in XInput mode. Want another controller? See [docs/ADDING-CONTROLLERS.md](docs/ADDING-CONTROLLERS.md).

## Linux

The same app and the same Steam Elite mode as on Windows. Nothing here needs root at runtime.

### Install

Pick one.

#### Arch Linux

Packages the prebuilt release into a pacman package. Nothing is compiled, so you don't need Rust.

```
mkdir paddle-keys-pkg && cd paddle-keys-pkg
for f in PKGBUILD paddle-keys-bin.install; do
  curl -fsSLO https://raw.githubusercontent.com/xplodingeggo/paddle-keys/main/packaging/arch/$f
done
makepkg -si
```

- `makepkg` downloads the release from GitHub and checks its checksum. `-si` installs it with pacman and pulls in missing dependencies.
- It installs the app, the Steam hooks, the udev rule and the menu entry.
- Remove it with `sudo pacman -R paddle-keys-bin`.

#### Any distro

1. Download `paddle-keys-<version>-linux-x86_64.tar.gz` from the [latest release](../../releases/latest) and extract it.
2. Run `./install.sh` in the extracted folder.

The script:
- installs to `~/.local/opt/paddle-keys` and the command `~/.local/bin/paddle-keys`;
- adds the app menu entry;
- offers to install the udev rule.

`./install.sh --uninstall` removes everything. Compare the download with `SHA256SUMS.txt` if you like. To build it yourself, see [Build](#build).

**Requirements:** glibc 2.35 or newer, GTK 3 and an AppIndicator library.

| Distro | Packages |
|--------|----------|
| Arch | `gtk3 libayatana-appindicator` |
| Debian / Ubuntu | `libgtk-3-0 libayatana-appindicator3-1` |

### First run

1. **Power-cycle the controller** once after installing, so the udev rule applies. It must be in XInput mode.
2. **Check it is found:** `paddle-keys --probe 5`, then press the paddles. Their names should print.
3. **Start it:** `paddle-keys`. A tray icon appears; it needs a StatusNotifier host (KDE, GNOME with the AppIndicator extension, waybar's `tray` module, ...).
4. **Autostart:** "Start on login" in the tray menu writes an XDG autostart entry. Hyprland doesn't read those, so add `exec-once = paddle-keys` instead.

If the Wine version of the 8BitDo Ultimate Software is running, close it first. It talks to the same channel, as on Windows.

#### Controller access

paddle-keys needs read/write on the pad's vendor `hidraw` node (interface 2) and on `/dev/uinput`. The udev rule `70-paddle-keys.rules` grants both to the logged-in user. `install.sh` and the Arch package install it for you.

#### Keys on Linux

Keys are sent through a virtual keyboard named `paddle-keys` (uinput). F13 to F24 often map to keysyms like `XF86Tools` on X11 and Wayland instead of `F13`. Apps that read raw key codes (games, Steam Input) see the right key; `wev` shows both.

### Steam Elite mode on Linux

Makes Steam see the controller as an Xbox Elite Series 2, with the paddles as P1 to P4. Off by default.

**Set up**

1. In Steam: **Settings > Controller**, turn on **Enable Steam Input for Xbox controllers**. No extra driver is needed on Linux.
2. In the tray: **Steam Elite mode > Enabled**, then **Install Steam hook (edits steam.sh)**. Or run `paddle-keys --install-steam-hook`.
3. Fully quit Steam (`steam -shutdown`) and start it again. No controller power-cycle is needed.

To undo it, use the tray item or run `paddle-keys --remove-steam-hook`.

**Good to know**

- The hook is added to the `LD_AUDIT` line in `~/.steam/steam/steam.sh`, the same way SLSsteam is loaded. If a headcrab `INJECT_SLS` line exists, the hook goes into it; otherwise a new `export` is added after the shebang. A backup is saved as `steam.sh.paddle-keys.bak` and a read-only `steam.sh` stays read-only.
- It only changes what Steam's SDL3 sees of the 8BitDo's gamepad node. Games are not touched.
- A Steam update that replaces `steam.sh` drops the line; the status then tells you to install it again.
- Unlike on Windows, nothing is injected into a running Steam. If Steam was started before the hook was installed, the tray status says to restart Steam.
- Paddle routing works as on Windows, see [Where the paddles go](#where-the-paddles-go).

How it works: [docs/STEAM-LINUX.md](docs/STEAM-LINUX.md).

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
