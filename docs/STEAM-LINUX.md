# Steam Elite mode on Linux

Notes from reverse engineering, for interoperability. Observed on Arch Linux (kernel 7.3, glibc 2.44) with the 32-bit Steam client of November 2026 (`ubuntu12_32/steam`, bundled SDL 3.x), headcrab/SLSsteam loaded, and one 8BitDo Ultimate 2 Wireless (`2DC8:310B`, release `0x0114`) in XInput mode. Nothing here is documented by Valve; any Steam update can change it. Read [STEAM.md](STEAM.md) first for the Windows side and the shared memory protocol.

## The short version

On Linux there is no Valve driver and no sideband. The kernel's `xpad` driver binds the pad's Xbox 360 interface and creates an evdev node, and Steam's bundled SDL3 reads that node with its evdev backend. SDL already maps `BTN_GRIPR`, `BTN_GRIPR2`, `BTN_GRIPL` and `BTN_GRIPL2` to paddle1 to paddle4, and `xpad` reports a real Elite 2's paddles exactly that way. So the hook does not translate a protocol. It makes the 8BitDo's evdev node look, to Steam's SDL only, like `xpad`'s node for a wired Elite 2, and feeds the paddle keys from the shared memory that `paddle-keys` writes.

## How Steam sees pads on Linux

- `steam.exe`'s Linux counterpart is `~/.steam/steam/ubuntu12_32/steam`, a **32-bit** process. It loads `ubuntu12_32/libSDL3.so.0` (linked `BIND_NOW`), which imports `open`, `read`, `ioctl`, `close` and `poll` from glibc.
- `Steam/logs/controller.txt` shows the SDL backend in the GUID: `0300f6b6c82d00000b31000014010000` for the plain 8BitDo has driver byte `00`, the evdev backend.
- SDL's evdev enumeration (`src/joystick/linux/SDL_sysjoystick.c`, `IsJoystick`) classifies the node through udev, then uses `EVIOCGID` for VID/PID/version and `EVIOCGNAME` for the name. `ConfigJoystick` assigns button numbers from the `EVIOCGBIT(EV_KEY)` bitmap in code order. `LINUX_JoystickGetGamepadMapping` maps the `BTN_GRIP*` keys to paddles for any vendor (and `BTN_TRIGGER_HAPPY5..8` for Microsoft pads with older drivers).
- Steam's controller type follows the VID/PID: `045E:0B00` makes it an Xbox One Elite 2 with the `controller_xboxone` config set, and the `paddle` entries in the mapping string give P1 to P4.
- The pad's other interfaces (vendor HID `hidraw`, keyboard and mouse evdev nodes) share VID/PID `2DC8:310B` but are not joysticks to SDL. The hook leaves them alone.

## Loading: LD_AUDIT, like SLSsteam

- headcrab replaces `steam.sh`. It writes `INJECT_SLS="LD_AUDIT=…/library-inject.so:…/SLSsteam.so"`, runs `export $INJECT_SLS`, and then sources `client.sh`, the original script. It also makes `steam.sh` read-only, so Steam's updater does not replace it.
- `ptrace_scope` is 1 on most distributions, so injecting into a running Steam is out. Loading at launch through the dynamic loader is the Linux way.
- `LD_AUDIT` modules live in their own link-map namespace with their own libc. Their symbols do **not** interpose on the program. Hooking is done through the audit callbacks:
  - `la_version` accepts the interface.
  - `la_objopen` returns `LA_FLG_BINDFROM` only for `…/ubuntu12_32/libSDL3.so.0`, and only when `/proc/self/exe` is Steam's `ubuntu12_*/steam`. It returns `LA_FLG_BINDTO` for the other objects in the base namespace.
  - `la_symbind32` (`la_symbind64` in the 64-bit build) sees each of SDL's imports. For `open`, `open64`, `openat`, `openat64`, `ioctl`, `read` and `close`, it records the real address and returns a wrapper. glibc 2.35 and later calls `la_symbind` for `BIND_NOW` objects too.
- `LD_AUDIT` is inherited by everything Steam starts: the 64-bit web helper, games, Proton. ld.so skips a module of the wrong ELF class with `wrong ELF class … ignored` (SLSsteam has the same messages), so both a 32- and a 64-bit hook are listed. Elsewhere `la_objopen` binds nothing, so games see no change.
- paddle-keys stages the hooks to fixed paths, `~/.local/share/paddle-keys/libpaddle_keys_steam{32,64}.so`, by writing a temp file and renaming it, so a running Steam keeps the file it mapped. It adds them to the `LD_AUDIT=` assignment in `steam.sh` (or inserts `export LD_AUDIT="${LD_AUDIT:+$LD_AUDIT:}…"` after the shebang) with a `# paddle-keys` marker, and restores a read-only mode after writing.
- `steam -shutdown` stops the client, but the `steam.sh` shell that started it can survive and relaunch the client **with its old environment**. Wait until both are gone before starting Steam again, or the new `LD_AUDIT` is not used.

## What the hook changes

Only fds that libSDL3 opened on a `/dev/input/event*` node are touched, and only when that node passes all three checks: real `EVIOCGID` is `2DC8:310B`, `EV_KEY` has `BTN_SOUTH` (the gamepad node, not the pad's keyboard or mouse node), and Elite mode was enabled at open time. Up to 16 fds at once, kept in a fixed table with no allocation.

| Call | Original | Changed to |
|------|----------|------------|
| `open*` | | tracks the fd; writes `hook_pid` and `hook_generation = generation` (the Windows "hello") |
| `EVIOCGID` | `2DC8:310B` | `045E:0B00`, bus and version kept |
| `EVIOCGNAME` | `8BitDo Ultimate 2 Wireless Controller` | `Xbox Elite Wireless Controller Series 2` |
| `EVIOCGBIT(EV_KEY)` | xpad's 360 buttons | plus `BTN_GRIPR`, `BTN_GRIPR2`, `BTN_GRIPL`, `BTN_GRIPL2` |
| `EVIOCGKEY` | real key state | plus the held paddles (SDL's resync after `SYN_DROPPED`) |
| `EVIOCSCLOCKID` | | remembered, so synthetic events use the clock SDL asked for |
| `read` | real events | when the paddles changed since the fd was last told: `EV_KEY` events for the changed paddles and `SYN_REPORT`; real events follow on SDL's next read, since it reads until `EAGAIN` |
| `close` | | untracks the fd |

Everything else passes through, including `EVIOCSFF` and the force-feedback writes, so rumble goes to `xpad` unchanged. The guide button is `xpad`'s `BTN_MODE`, already mapped.

Paddle keys follow the Elite layout and `xpad`: P1 (upper right) `BTN_GRIPR`, P2 (lower right) `BTN_GRIPR2`, P3 (upper left) `BTN_GRIPL`, P4 (lower left) `BTN_GRIPL2`. With `elite_bits` that is R4, PR, L4 and PL. In Steam's mapping string they show up as `paddle1:b12,paddle2:b11,paddle3:b14,paddle4:b13`: SDL3 numbers paddles right1, left1, right2, left2.

## Shared memory

The same `Shared` layout as on Windows (`steam-hook/src/shm.rs`, 48 bytes, identical on 32- and 64-bit). It lives in the per-user file `/dev/shm/paddle-keys-steam-<uid>`, which paddle-keys creates and both sides map. `heartbeat` is `CLOCK_MONOTONIC` in milliseconds on both sides. The rules from STEAM.md hold:
- a heartbeat older than 250 ms means released, so a crashed paddle-keys never leaves a paddle held;
- the generation is bumped only when the pad really goes away;
- live means `hook_generation == generation` and the hook's PID is alive (`/proc/<pid>`).

The tray's status checks, in order:
- Steam Elite mode enabled;
- the hook marker in `steam.sh`;
- the Xbox setting in `localconfig.vdf`;
- a running `ubuntu12_*/steam`;
- the hook in that PID's `/proc/<pid>/maps`;
- live.

## Testing without Steam

A process whose path ends in `ubuntu12_32/steam` and that links an `ubuntu12_32/libSDL3.so.0` gets the hook like Steam does. A 30-line C `libSDL3.so.0` that opens the event node, prints `EVIOCGID`, `EVIOCGNAME` and the `EV_KEY` bits, and then reads events, plus a `steam` that calls it (both `-m32 -Wl,-z,now`), shows exactly what Steam's SDL would get. Run it with `LD_AUDIT=…/libpaddle_keys_steam32.so` next to the real Steam: evdev allows several readers. It also lets you test the crash rule. Write `enabled=1`, `paddles=1` and a fresh heartbeat into the shm file for a second, stop, and the stand-in sees the paddle press and then its release.

Note that the stand-in writes its own PID as `hook_pid`, so afterwards paddle-keys reports "not live" until the pad reconnects to Steam.

## Dead ends and pitfalls

- **Only VID/PID matching** also took over the pad's keyboard and mouse evdev nodes (same VID/PID, interface 1). SDL ignored them, but they got Elite IDs. Checking for `BTN_SOUTH` limits the hook to the gamepad node.
- **`steam -shutdown` and relaunch** without waiting for the old `steam.sh` shell: the client came back with the old `LD_AUDIT` and no hook.
- **Writing `steam.sh` directly** fails with `EACCES`, because headcrab leaves it `r-xr-xr-x`.
- **Symbol interposition from the audit module** does nothing. Audit modules are in their own namespace; use `la_symbind`.
- A **uinput virtual Elite** was considered: no code in Steam, but Steam would also list the real pad unless it was started with `SDL_GAMECONTROLLER_IGNORE_DEVICES`, which is static and hides the pad when paddle-keys is not running.
