# Adding another controller

For contributors, human or AI. Read [PROTOCOL.md](PROTOCOL.md) and [STEAM.md](STEAM.md) first; this page assumes them.

Support for a controller has two independent halves. Do them in this order, and ship the first half alone if the second doesn't work out.

1. **Paddle source:** read the extra buttons that XInput can't carry, then press keys. Code: `src/device.rs`, `src/decode.rs`.
2. **Steam presentation (optional):** make Steam Input see the pad as an Elite 2 so the extra buttons become real paddles. Code: `steam-hook/`.

Right now both halves are written for exactly one device, `2DC8:310B`. Adding a second one means turning these constants into a small per-device table; keep it a table, not a trait hierarchy.

| Place | Hardcoded today |
|-------|-----------------|
| `src/device.rs` | `VID`, `PID`, `IFACE` (vendor HID interface 2) |
| `src/decode.rs` | `POLL` command, `REPLY_HEADER`, mask offset `18..22`, paddle bits |
| `src/steam.rs` `elite_bits` | which paddle becomes P1 to P4 |
| `steam-hook/src/lib.rs` `patch_info` | VID/PID bytes `c8 2d 0b 31` |
| `steam-hook/src/lib.rs` `patch_iface` | source class `ff 5d 01` |
| `steam-hook/src/lib.rs` `sync_ignore` | ignore list `0x2DC8/0x310B` |
| `steam-hook/src/gip.rs` `Pad360` | the Xbox 360 report layout |
| `steam-hook/src/linux.rs` `VID`/`PID`, `PADDLE_KEYS` | Linux: which evdev node is taken over, and the paddle key codes |

## Part 1: find the extra buttons

1. **Enumerate the interfaces.** In Device Manager or with `Get-PnpDevice -PresentOnly | ? InstanceId -match 'VID_xxxx'`. Note every `MI_xx` interface and HID collection. Vendor-defined usage pages (`0xFF00` and up) are the candidates.
2. **Read the report descriptor** of the candidate (hidapi `get_report_descriptor`, or USBPcap/Wireshark). Note input and output report IDs and sizes.
3. **Listen first.** Open it and read for a while, pressing the buttons. Some channels stream (easy). The 8BitDo's is request/response and sends nothing until asked.
4. **If it's request/response, learn the request from the vendor's own software**, never by guessing. Capture its traffic with USBPcap while it shows a "button test" page, or read its code. Vendor channels also carry configuration, calibration and **firmware update** commands; a guessed command can brick the controller.
5. **Send only the one read command you found.** Never write anything else. Say so in the docs, as PROTOCOL.md does.
6. **Prove each bit with a counted test**: press each button a known, different number of times and check you saw exactly that many clean on/off edges and no other bits. Document bits as *observed* (seen on hardware) or *read* (taken from vendor software), the way PROTOCOL.md does.
7. **Check you broke nothing:** XInput values must stay unchanged and uninterrupted while you poll.

Then add the device to `device.rs`/`decode.rs` with a unit test per bit, like `decode.rs` has.

## Part 2: Steam presentation

### Is it possible for this pad?

On Linux see [STEAM-LINUX.md](STEAM-LINUX.md): any pad that Steam reads through SDL's evdev backend (GUID driver byte `00` in `controller.txt`) can be presented the same way, with no protocol translation. The rest of this section is about Windows.

- The pad must reach Steam through `steamxbox.sys`, which means USB with an Xbox 360 or Xbox One interface. Check `Steam\logs\controller.txt` and `tools/sdl_devices.py`: you want an SDL entry whose path starts with `nvagipman:`. Bluetooth pads, DInput-mode pads and pads Steam reads through plain HID do **not** go through this path; for those, the hook has nothing to rewrite.
- If the pad already reports GIP (`ff/47/d0`), you may only need the VID/PID swap and paddle bits. If it reports 360 (`ff/5d/01`), you need the full translation, exactly like the 8BitDo.

### Fastest way to try it: the Frida prototype

`tools/frida_gip_bridge.py` does everything the hook does, in Python and JavaScript, attached to a running Steam. Changing it takes seconds; rebuilding the hook needs a Steam restart each time.

1. Turn Elite mode off in `paddle-keys`, or quit it. Two bridges fight.
2. Change the VID/PID check in `patchInfo`, the mask bits in `BITS` and the poll in `POLL`/`opendev`.
3. Start Steam, run the bridge with Steam's PID, then **power-cycle the pad** (Steam enumerates before you attach).
4. Watch the log for `p808`, `p80c`, `hello`, `conv` events, and open Steam's controller settings to test buttons.
5. Port what worked into `steam-hook`.

### Things to check per pad

- **The 360 report.** Not every "360" pad fills every field. Check the guide bit (`b3 & 0x04`) and the trigger range on your pad.
- **Endpoints:** log the prefix of a few reads and writes (`frame` events in the prototype). Don't assume `0x84`/`0x05`; the hook never rewrites them anyway.
- **Rumble:** the 360 rumble packet works on any XInput-class pad, but check the motor order (left = low frequency, heavy).
- **Duplicates:** after takeover, run `tools/sdl_devices.py`. Every entry with the pad's original VID/PID besides the Elite has to be covered by the ignore hint.
- **Mapping:** the Elite's P1 to P4 are upper-right, lower-right, upper-left, lower-left. Map the pad's physical layout onto that in `elite_bits`.

### Testing checklist (on hardware)

- Paddles, every face button, dpad, sticks (full range, both axes), triggers (0 to full), guide, rumble, in Steam's controller test page.
- Power-cycle the pad: it comes back as an Elite, still only one entry.
- Restart Steam with `paddle-keys` running: the hook is re-injected, the pad works after one power-cycle.
- Quit `paddle-keys` while holding a paddle: nothing stays pressed in Steam or as a key.
- Elite mode off: after a power-cycle the pad shows as itself again.
- Default routing: F-keys don't fire while Steam has the pad; they do in DInput mode or with Steam closed.
- `cargo test --workspace` passes, and new translation code has unit tests in `gip.rs` or `lib.rs`.

## Rules for the hook

- **Never block or panic in a hooked call.** Everything runs inside `guard` (`catch_unwind`), locks recover from poisoning, and the original function's last error is restored. A panic inside Steam's I/O thread takes Steam down.
- **Key everything on the 8-byte sideband id**, so other controllers pass through untouched.
- **Never touch the endpoint byte** of the prefix.
- **Nothing the pad doesn't understand goes out**, and nothing SDL doesn't expect comes in.
- **Do work in the DLL's own thread**, not in `DllMain`.

## Notes for AI agents

- Build: `cargo build --release --workspace`; test: `cargo test --workspace`. The exe and `paddle_keys_steam.dll` must sit in the same folder.
- A new hook build only takes effect after Steam restarts (`steam.exe -shutdown`, then start `paddle-keys` before Steam), followed by a pad power-cycle. If results look unchanged, check `%LOCALAPPDATA%\paddle-keys` for the staged copy and `tools/shm_watch.py` for `hook_pid` and `frames`.
- Steam's real parser is SDL's source code. Fetch the matching file from `libsdl-org/SDL` (`src/joystick/hidapi/`, `src/joystick/windows/`) instead of disassembling `SDL3.dll`.
- Before attaching Frida, make sure no earlier bridge or session is still attached. Stacked bridges produce nonsense.
- Ask the user to press things and report what they see; you can't see their Steam window. Watch `tools/shm_watch.py` or the bridge log at the same time, so you know whether the press arrived.
- Don't send anything to a controller's vendor channel except a read command you have proven from the vendor's software.
