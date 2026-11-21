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

## Install

1. Download `paddle-keys.exe` from the [latest release](../../releases/latest). Compare it with `SHA256SUMS.txt` if you like.
2. Close the 8BitDo Ultimate Software. It talks to the same channel and the two will fight over replies.
3. Run `paddle-keys.exe`. A tray icon appears (it may be under the `^` overflow arrow).

Windows SmartScreen or antivirus may warn about an unsigned program that reads a HID device and sends keystrokes. The source is in this repo and the release is built from it; build it yourself if you prefer.

### Tray menu

- **Paused:** stop pressing keys (held keys are released).
- **Start with Windows:** adds or removes a per-user Run entry.
- **Open config (restart to apply):** opens the config file in Notepad.
- **Quit**

## Configuration

`%APPDATA%\paddle-keys\config.toml` is created on first run:

```toml
[keys]
L4 = "F13"
R4 = "F14"
PL = "F15"
PR = "F16"
```

Values are an optional list of modifiers joined with `+`, then one key: `F1` to `F24`, `A` to `Z`, `0` to `9`, `Space`, `Enter`, `Esc`, `Tab`. Modifiers are `Ctrl`, `Shift`, `Alt`, `Win`. Example: `L4 = "Ctrl+Shift+F13"`. A bad value falls back to that paddle's default. Restart the app after editing.

## Behavior

- A paddle held means its key is held. Releasing it releases the key.
- If the controller turns off or is unplugged while a paddle is held, the key is released and the app reconnects by itself within a few seconds.
- Keys do not reach windows running as administrator unless `paddle-keys` is also run as administrator.
- Only one copy runs at a time.
- Gyro is not exposed. The Motion feature in the 8BitDo software runs inside the controller and turns movement into right-stick input, which XInput already carries.

## Tested on

- 8BitDo Ultimate 2 Wireless Controller for PC, USB `2DC8:310B` (XInput mode), device release `0x0114`, Windows 11.
- This is one unit. The firmware version is unknown. Other firmware may answer differently; please open an issue with the output of `paddle-keys.exe --probe 10` run from a terminal while you press each paddle.

## Troubleshooting

`paddle-keys.exe --probe 10` runs for 10 seconds and prints the button mask whenever it changes, with the paddle names, then exits. If nothing prints, the controller is not found or another program holds the channel.

## How it works

See [docs/PROTOCOL.md](docs/PROTOCOL.md). The app sends one read-only command, nothing else, to the controller.

## Build

```
cargo build --release
```

Windows only. Rust 1.85 or newer (2024 edition).

## License

MIT, see [LICENSE](LICENSE). Not affiliated with 8BitDo.
