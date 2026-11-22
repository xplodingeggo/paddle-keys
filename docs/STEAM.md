# Steam Elite mode: how it works, and everything we learned

Notes from reverse engineering, for interoperability. Observed on Windows 11 with the Steam client of November 2026 (Steam's bundled SDL 3.x), Valve's `steamxbox.sys` and one 8BitDo Ultimate 2 Wireless (`2DC8:310B`, release `0x0114`). Nothing here is documented by Valve; any Steam update can change it.

If you want to add another controller, read this, then [ADDING-CONTROLLERS.md](ADDING-CONTROLLERS.md).

## The short version

Steam reads Xbox controllers through Valve's driver and its own copy of SDL. SDL picks a parser from the USB interface class the driver reports. The 8BitDo reports the Xbox 360 class, and SDL's Xbox 360 parser has no paddles. The hook rewrites what the driver tells Steam, so SDL's Xbox One (GIP) parser takes the pad as an Elite Series 2, and translates every packet in both directions. The paddle state comes from `paddle-keys`, which already reads it from the 8BitDo's vendor HID channel ([PROTOCOL.md](PROTOCOL.md)).

## How Steam talks to Xbox pads

- **Driver:** "Xbox Extended Feature Support" in Steam's controller settings installs `steamxbox.sys` (service `steamxbox`, a KMDF driver). It gives Steam the raw USB packets of Xbox 360 (XNA, class `ff/5d/01`) and Xbox One (GIP, class `ff/47/d0`) controllers through device I/O control calls on a sideband device. SDL shows those devices with paths like `nvagipman:c01337769843:0:132:5`.
- **Parser:** `steam.exe` loads `SDL3.dll` from the Steam folder. Its HIDAPI layer treats sideband devices like USB devices and picks a driver from VID/PID and interface class:
  - `ff/5d/01`: `SDL_hidapi_xbox360.c`. No paddles.
  - `ff/47/d0`: `SDL_hidapi_gip.c`. Elite 2 paddles, guide, rumble and impulse triggers. Its `IsSupportedDevice` also needs the VID/PID to map to `SDL_GAMEPAD_TYPE_XBOXONE`, which `045E:0B00` does and `2DC8:310B` does not.
- **Steam's controller type:** comes from a VID/PID table in `steamclient64.dll` (Elite 2 is type `0x2E`), plus a `,paddle` substring check on the SDL mapping string. Both follow from the VID/PID and the GIP driver, so faking `045E:0B00` and feeding the GIP driver is enough.
- **Setting:** "Enable Steam Input for Xbox controllers" is `"SteamController_XBoxSupport" "1"` in `Steam\userdata\<id>\config\localconfig.vdf` (older clients wrote `Steam\config\config.vdf`). The Steam folder is `HKCU\Software\Valve\Steam\SteamPath`.

## The sideband calls

All calls are `DeviceIoControl` on the sideband handle, usually overlapped. Results arrive through `GetOverlappedResult`, so the hook has to watch both.

| Code | Direction | Content |
|------|-----------|---------|
| `0x2ae808` | out | Device info: 8-byte device id, VID `u16`, PID `u16`, release, UTF-16 manufacturer string |
| `0x2ae80c` | in: id + index; out | Interface info: 8-byte id, index, class, subclass, protocol, endpoints |
| `0x2aec04` | out, 269-byte buffer | Read one packet (overlapped, one always pending) |
| `0x2aac08` | in, METHOD_BUFFERED | Write one packet |
| `0x2aac0c`, `0x2aac10` | in, 9 bytes | Device id + endpoint (pipe control; not needed here) |
| `0x2ae804` | out, 12 bytes | Status |

Buffers for reads and writes start with a **13-byte prefix**:

```
23 64 1b 37 13 c0 00 00   84   0d   01 00 00   <packet ...>
|-- 8-byte device id --|  ep   size  3 bytes
```

- The device id changes on every enumeration and matches the id in the `0x2ae808` reply. Everything in the hook is keyed on it, so other pads are never touched.
- Byte 8 is the USB endpoint (`0x84` IN on this pad, `0x05` OUT). **Never change it**: SDL's reads fail and the pad disconnects.
- For writes, byte 9 is the total size. The hook sets it to `13 + 8 = 21` when it swaps in a 360 rumble packet.

## Packet formats

### Xbox 360 (what the 8BitDo sends)

Input report `00 14` + 18 bytes: `b2` = dpad (`0x0f`: up, down, left, right), start `0x10`, back `0x20`, LS `0x40`, RS `0x80`; `b3` = LB `0x01`, RB `0x02`, guide `0x04`, A/B/X/Y `0x10..0x80`; `b4`, `b5` = triggers 0 to 255; `b6..b13` = LX, LY, RX, RY as `i16` little-endian. Rumble out: `00 08 00 L R 00 00 00`. Other inbound messages (`01 03 xx` LED status, `02 03 xx`, `03 03 xx`) are status chatter.

### GIP (what SDL's GIP driver expects)

Header: type, flags, sequence, length (LEB128), then the payload.

| Message | Bytes | Notes |
|---------|-------|-------|
| Hello | `02 20 seq 1c` + 28 bytes | 8-byte device id, VID, PID, firmware major/minor/build/rev (`u16` each), hardware, RF, security, GIP versions |
| Input | `20 00 seq 2f` + 47 bytes | buttons `[0..2]`, triggers 10-bit `[2..6]`, sticks `i16` `[6..14]`, Elite paddles `[14]`, profile `[20]` |
| Guide | `07 20 seq 02 state 5b` | virtual key `0x5b` (`VK_LWIN`); flag `0x20` without ACME, so SDL never acks |
| Rumble (from SDL) | `09 00 seq 09 00 bitmap LT RT L R duration delay repeat` | levels 0 to 100 |

GIP input buttons: byte 0: menu `0x04`, view `0x08`, A `0x10`, B `0x20`, X `0x40`, Y `0x80`; byte 1: dpad up/down/left/right `0x01..0x08`, LB `0x10`, RB `0x20`, LS `0x40`, RS `0x80`.

### The details that cost the most time

- **Firmware version picks the paddle format.** SDL reads major/minor from the hello. 5.17 or later (or minor below 11) selects `XBE2_5`: paddles in payload byte 14, bits P1 `0x01`, P2 `0x02`, P3 `0x04`, P4 `0x08`. Other versions put them elsewhere.
- **The profile byte (payload 20) must be 0.** Elite 2 firmware clears the paddle bits while a non-default profile is active, and SDL copies that rule. A stray value there silently kills the paddles.
- **Metadata.** After the hello, SDL asks for metadata. The fake pad cannot answer. By default SDL then resets the device forever. The hint `SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA=0` makes SDL fall back to default metadata after 3 retries. The hook sets it at OVERRIDE priority, so Steam can't undo it.
- **360 status messages must never reach SDL.** `01 03 xx` parses as a GIP message for an attachment and confuses the driver. Every non-input 360 packet is replaced with a re-stamped copy of the last input.
- **Writes must not reach the pad.** SDL sends GIP commands (metadata request, LED, power state) to the OUT endpoint. A 360 pad ignores some and misbehaves on others. Every write is replaced with a 360 rumble packet carrying the current levels.
- **Triggers** are 8-bit on the 360 side and 10-bit on GIP: `v * 1023 / 255`, rounded, in `u32` (a `u16` product overflows).

## What the hook changes

`paddle_keys_steam.dll` hooks `kernel32!DeviceIoControl` and `kernelbase!GetOverlappedResult` in `steam.exe` with MinHook. It only touches devices whose info reply carried `2DC8:310B` while Elite mode was enabled.

| Call | Original | Changed to |
|------|----------|------------|
| `0x2ae808` device info reply | VID/PID `2DC8:310B` | `045E:0B00` (Xbox Elite Series 2) |
| `0x2ae80c` interface reply | class `ff 5d 01` | `ff 47 d0` (GIP); endpoint bytes untouched |
| `0x2aec04` read, first one | 360 report | GIP hello: `045E:0B00`, firmware 5.17 |
| `0x2aec04` read, 360 input `00 14 ...` | 360 report | GIP input with the paddles from shared memory |
| `0x2aec04` read, guide bit changed | 360 report | GIP guide message (the input follows on the next report) |
| `0x2aec04` read, anything else | status | repeat of the last GIP input |
| `0x2aac08` write, GIP rumble `09 ...` | GIP motors | 360 rumble `00 08 00 L R 00 00 00` |
| `0x2aac08` write, anything else | GIP command | repeat of the current 360 rumble |

It also sets two SDL hints through `SDL_SetHintWithPriority` in `SDL3.dll`:

- `SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA=0`, see above.
- `SDL_GAMECONTROLLER_IGNORE_DEVICES=0x2DC8/0x310B` while Elite mode is enabled, reset with `SDL_ResetHint` when it is turned off. Without it Steam lists the controller **four times**: the Elite, SDL's XInput entry (`XInput#0`) and two DirectInput entries (the vendor HID and consumer-control interfaces). SDL normally hides XInput and DirectInput duplicates when a HIDAPI device has the same VID/PID, but the Elite now reports `045E:0B00`. SDL re-reads this hint live, and Steam sets no ignore list of its own. The real HID interfaces are still opened by `paddle-keys` from its own process; the hint only affects Steam.

Timing: the read for `0x2ae808` can complete synchronously, through `GetOverlappedResult`, or with neither (Steam polls `OVERLAPPED.Internal`). The hook records each pending call and, for info and interface calls, spins briefly on `Internal` until it leaves `STATUS_PENDING`.

## Paddle state and "live"

`paddle-keys` owns the vendor HID poll and shares state through the section `Local\paddle-keys-steam` (layout in `steam-hook/src/shm.rs`): enabled flag, paddle bits, heartbeat, generation, and what the hook reports back (its PID, generation, frame count).

- **Heartbeat:** the hook treats paddles as released when the heartbeat is older than 250 ms, so a closed or crashed `paddle-keys` never leaves a paddle held.
- **Live:** in the default routing mode, paddles go to Steam only while Steam has the pad as an Elite, and to keys otherwise. The hook copies `generation` into `hook_generation` when it sends a hello. `paddle-keys` bumps `generation` once when the controller really disappears: a HID write error, or the device no longer enumerating. Live means `hook_generation == generation` and the hook process is alive.
- Poll **timeouts** must not bump the generation: right after a reconnect the vendor channel stalls for a moment, after Steam's hello. Counting those left the status stuck on "reconnect" with the Elite working.

## Injection

- `paddle-keys` finds `steam.exe` with a toolhelp snapshot, checks its modules for `paddle_keys_steam*`, and loads the DLL with `CreateRemoteThread` + `LoadLibraryW`.
- The DLL is copied to `%LOCALAPPDATA%\paddle-keys\paddle_keys_steam-<version>-<content hash>.dll` first. Steam keeps a loaded DLL locked until it exits, and a name keyed on content makes every rebuild load fresh. A name keyed on file size does not, because PE files are padded to 512 bytes.
- The DLL is never unloaded. A per-PID mutex stops a second copy from hooking.
- **Steam enumerates pads within about 2 seconds of starting**, before the hook is in. A pad connected before injection stays a plain 360 pad (and stays listed four times) until it reconnects. That is why the README says to turn the controller off and on.
- `DllMain` only starts a thread; all hooking happens there, outside the loader lock.

## Dead ends, so nobody repeats them

- **Rewriting the endpoint byte** in the prefix to make reads look like GIP: SDL's reads fail and the pad drops.
- **Feeding 360 reports to the GIP driver** without a hello: SDL never finishes init and the pad is dead.
- **Wrong firmware version in the hello**: paddles land in the wrong byte or are dropped.
- **Frida Stalker** to trace SDL's parser inside Steam: too slow, Steam drops the pad. Read SDL's source (`src/joystick/hidapi/SDL_hidapi_gip.c`) instead; Steam ships a nearly stock SDL 3.
- **Hiding the duplicate entries by failing `CreateFileW`** on the 8BitDo's HID and XUSB paths: the opens fail but XInput and DirectInput list the device anyway. The ignore hint is the fix.
- **Several Frida bridges attached at once** give results that make no sense. Always check for and kill leftover ones.

## Diagnostics

- `Steam\logs\controller.txt` logs every `Local Device Found` with VID/PID, `sdl://` path, product name and the SDL mapping string. The second-to-last byte of the mapping's GUID is the SDL driver: `68` (`h`) HIDAPI, `78` (`x`) XInput, `00` DirectInput.
- `tools/sdl_devices.py <steam pid>`: the joysticks Steam's SDL has right now, with backend, path and the relevant hints.
- `tools/shm_watch.py N`: the shared memory, to see enabled, paddles, heartbeat age, generations and frame count.
- `tools/frida_gip_bridge.py`: the original working prototype, a template for trying a new controller without rebuilding.
