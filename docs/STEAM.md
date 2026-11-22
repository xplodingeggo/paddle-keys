# Steam Elite mode: how it works

Notes from reverse engineering, for interoperability. Observed on Windows 11 with the Steam client of November 2026, Valve's `steamxbox.sys` 1.0.0.17 and one 8BitDo Ultimate 2 Wireless (`2DC8:310B`, release `0x0114`).

## Why Steam never sees the paddles

- **Driver:** with Xbox Extended Feature Support installed, Steam reads Xbox controllers through `steamxbox.sys`. The driver hands Steam the raw USB packets of Xbox 360 (XNA) and Xbox One (GIP) controllers over sideband device I/O control calls.
- **Parser:** Steam parses those packets with its bundled `SDL3.dll`, which picks a driver from the USB interface class. Class `ff/5d/01` (Xbox 360) gets SDL's Xbox 360 driver, which has no paddles. Class `ff/47/d0` gets SDL's GIP driver (`SDL_hidapi_gip.c`), which reads Elite 2 paddles.
- **The 8BitDo:** in XInput mode it reports `ff/5d/01`, and its paddles exist only on the vendor HID channel that `paddle-keys` reads (see [PROTOCOL.md](PROTOCOL.md)).

## What the hook changes

`paddle_keys_steam.dll` hooks `kernel32!DeviceIoControl` and `kernelbase!GetOverlappedResult` in `steam.exe`. Each sideband buffer starts with a 13-byte prefix whose first 8 bytes identify the device. The hook only touches devices whose info reply carried `2DC8:310B`.

| Call | Original | Changed to |
|------|----------|------------|
| `0x2ae808` device info reply | VID/PID `2DC8:310B` | `045E:0B00` (Xbox Elite Series 2) |
| `0x2ae80c` interface reply | class `ff 5d 01` | `ff 47 d0` (GIP); endpoint bytes untouched |
| `0x2aec04` read, first one | 360 report | GIP hello, 28 bytes: `045E:0B00`, firmware 5.17 |
| `0x2aec04` read, 360 input `00 14 ...` | 360 report | GIP input `20 00 seq 2f` + 47 bytes, paddles in byte 14 |
| `0x2aec04` read, guide bit changed | 360 report | GIP guide `07 20 seq 02 state 5b` |
| `0x2aec04` read, any other 360 message | LED or status | repeat of the last GIP input |
| `0x2aac08` write, GIP rumble `09 ...` | GIP direct motor | 360 rumble `00 08 00 L R 00 00 00` |
| `0x2aac08` write, any other GIP command | metadata, LED, state | repeat of the current 360 rumble |

It also sets the SDL hint `SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA=0`. The fake controller cannot answer SDL's metadata request; with the hint SDL falls back to default metadata instead of resetting the controller forever.

## SDL GIP details that matter

- **Firmware version:** the hello's firmware major and minor version select the Elite button format. Version 5.17 or later gives `XBE2_5`: paddles at input payload byte 14 (bits 0 to 3 are P1 to P4), and the profile byte at 20, which must be 0 or SDL clears the paddles.
- **Prefix:** byte 8 of the sideband prefix is the USB endpoint. Changing it makes SDL's reads fail and the controller disconnects.
- **Unknown messages:** 360 status messages such as `01 03 xx` would be misread as GIP messages for other attachments, so they are never passed through.

## Paddle state

`paddle-keys` owns the vendor HID poll and publishes the paddle bits through the shared memory section `Local\paddle-keys-steam` (layout in `steam-hook/src/shm.rs`). The hook treats the state as released when it is older than 250 ms, so a closed or crashed `paddle-keys` never leaves a paddle held. A generation counter, bumped whenever `paddle-keys` loses the controller and echoed by the hook when it takes a connection over, tells `paddle-keys` whether Steam currently has the controller as an Elite.
