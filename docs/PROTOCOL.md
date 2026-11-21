# 8BitDo Ultimate 2 Wireless: reading the paddles in XInput mode

Notes from reverse engineering the controller's vendor HID channel, for interoperability. Everything here was observed on one unit, USB `2DC8:310B`, device release `0x0114` (firmware version unknown). Facts are marked **observed** (seen on hardware) or **read** (taken from 8BitDo's own software; not independently tested unless said).

## Why the paddles are missing in XInput mode

XInput has no fields for extra buttons or gyro. In XInput mode the controller enumerates as a composite device:

| Interface | Function |
|-----------|----------|
| 0 | XInput (Xbox 360 driver) |
| 1 | Keyboard / consumer / mouse HID collections |
| 2 | **Vendor HID**, usage page `0xFF7A`, usage `0x01` |

In DInput mode the paddles are in the normal input report, but the sticks are 8-bit there (255 steps, observed; SDL's 8BitDo driver also reads them as 8-bit). XInput mode keeps 16-bit sticks (`-32768..32767`, 495 distinct values seen on one axis in a short test).

## The vendor channel (interface 2)

Report descriptor (**observed**): input report ID `0x02`, 63 bytes; output report ID `0x81`, 63 bytes. The channel is request/response. Nothing arrives unless you ask: opening it and listening produces zero reports.

8BitDo's configuration software uses the same channel for everything it does, including settings, calibration, and firmware update commands. Because of that, **send nothing except the read command below**. This project never writes anything else, and nothing here should be taken as a description of other commands.

### The poll

Write one 64-byte buffer with hidapi: report ID `0x81`, then `04 03 00 23 01`, zero-padded to 64 bytes total.

```
81 04 03 00 23 01 00 00 ... (64 bytes)
```

The bytes after the report ID are a command header (`04`) plus command `0x0003` with argument `0x0123` (little-endian `03 00`, `23 01`). This is the "read keys" request in 8BitDo's software (**read**), and it works without that software running (**observed**).

### The reply

Read 64 bytes. With hidapi the first byte is the report ID. A valid reply starts with these 7 bytes (**observed**):

```
02 04 04 00 03 00 0c
```

Ignore anything that does not. In the 64-byte buffer:

| Bytes | Meaning |
|-------|---------|
| `[0..7]` | header above |
| `[18..22]` | button mask, `u32` little-endian |
| `[22..26]` | look like four 8-bit stick values (not decoded or verified) |

A reply came back for every poll, at about 330 polls per second on the test unit when a 4 ms pause was used between them.

### Button mask

| Bit | Button | Status |
|-----|--------|--------|
| `0x00200000` | **L4** | observed |
| `0x40000000` | **R4** | observed |
| `0x04000000` | **PL** | observed |
| `0x02000000` | **PR** | observed |
| `0x00002000` | A | observed |
| `0x00001000` | B | observed |
| `0x00000400` | L1 | observed |
| `0x00000800` | R1 | observed |
| `0x00000040`, `0x00000080`, `0x00000100` | Right, Left, Down | observed, matches 8BitDo's key map |
| `0x00000001`, `0x00000002`, `0x00000004`, `0x00000008`, `0x00000010`, `0x00000020`, `0x00000200`, `0x00004000`, `0x00008000` | Start, L3, R3, Select, X, Y, Up, L2, R2 | read from 8BitDo's key map, not tested |

The paddle names above are the labels on the controller. In 8BitDo's software L4 is internally called P3, R4 is P4, PR is P1 and PL is P2.

Paddle presses were verified with a counted test: PL three times, PR five, L4 three, R4 five. Each press showed as one clean on and off with no other bits set.

### Notes

- XInput kept working normally while polling: the XInput button and stick values were unchanged and uninterrupted in a 15 second test.
- If a second program polls at the same time (for example the 8BitDo software on its Stick page), replies can be consumed by the wrong reader, so presses may be missed.
- The poll interval, the three-timeouts rule and the reconnect delay (1 s) in this app are my choices, not protocol requirements.

## Gyro

Not found on this channel. With the Motion feature set up as a profile in 8BitDo's software and loaded on the controller, it works in games, but the software's Motion tab produces no traffic on the vendor channel. The software writes motion settings (sensitivity, dead zone) as configuration, and the controller turns movement into stick output itself (users report it only emulates the stick). Over Bluetooth in DInput mode, SDL reads sensor data from reports of at least 34 bytes on newer firmware; that was not tested here.

## Related

- SDL's 8BitDo HID driver, `src/joystick/hidapi/SDL_hidapi_8bitdo.c`, handles the paddle bits (`L4`/`R4` at report offset 10) for Bluetooth and DInput mode. It does not cover this XInput-mode vendor interface.
- Steam Input supports the Ultimate 2 paddles in DInput and over Bluetooth, which is where the 8-bit sticks come from.
