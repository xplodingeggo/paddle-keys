# PROTOTYPE, kept as a template for new controllers. Do not use day to day: Frida slows Steam and the
# patching is less careful than steam-hook (it touches any 360-class pad). Stop paddle-keys and run
# only one bridge at a time. Usage: python frida_gip_bridge.py SECONDS LOG.jsonl STEAM_PID
# Needs: pip install frida hidapi
# Makes Steam's SDL GIP driver own the 8BitDo (XInput 2DC8:310B) as an Xbox Elite 2:
#  - 0x2ae808 device info: VID/PID -> 045E:0B00
#  - 0x2ae80c interface info: class ff/5d/01 (XNA) -> ff/47/d0 (GIP), endpoints untouched
#  - first frame after enumeration -> GIP hello (fw 5.17 => SDL XBE2_5 paddle format)
#  - 360 input frames -> GIP input (0x20) with paddles in payload byte 14; other 360 messages -> repeat last input
# shortcut: patches any XNA interface / any endpoint-0x84 frame, fine while the 8BitDo is the only 360 pad
import frida, sys, time, json, hid
secs = float(sys.argv[1]); out = open(sys.argv[2], 'w', buffering=1); pid = int(sys.argv[3])

JS = r"""
let mask = 0, seq = 0, sideOvl = null, sideBuf = null, sideH = null, conv = 0, needHello = false, last = null, other = 0, ioLog = 0;
const pend = new Map();
function hx(p,n){ const a=new Uint8Array(p.readByteArray(n)); return Array.from(a).map(x=>x.toString(16).padStart(2,'0')).join(''); }
let idInfo = null, frameLog = 0;
function onMask() { recv('mask', m => { mask = m.v; onMask(); }); } onMask();

const sdl = Process.getModuleByName('SDL3.dll');
const hintKey = Memory.allocUtf8String('SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA'), hintVal = Memory.allocUtf8String('0');
try {
  const setHint = new NativeFunction(sdl.getExportByName('SDL_SetHint'), 'bool', ['pointer', 'pointer']);
  const getHint = new NativeFunction(sdl.getExportByName('SDL_GetHint'), 'pointer', ['pointer']);
  const ok = setHint(hintKey, hintVal); const g = getHint(hintKey);
  send({t: 'hint', ok: ok, val: g.isNull() ? null : g.readUtf8String()});
  const setPrio = new NativeFunction(sdl.getExportByName('SDL_SetLogPriority'), 'void', ['int', 'int']);
  setPrio(7, 3);  // SDL_LOG_CATEGORY_INPUT -> DEBUG, so GIP log lines reach Steam's log if it routes them
} catch (e) { send({t: 'hinterr', e: e.toString()}); }

function patchInfo(b) {
  if (b.add(8).readU16() === 0x2dc8 && b.add(10).readU16() === 0x310b) {
    idInfo = hx(b, 8); b.add(8).writeU16(0x045e); b.add(10).writeU16(0x0b00); needHello = true; send({t: 'p808', id: idInfo}); return true;
  }
  return false;
}
function patchIface(b) {
  if (b.add(10).readU8() === 0xff && b.add(11).readU8() === 0x5d && b.add(12).readU8() === 0x01) {
    b.add(11).writeU8(0x47); b.add(12).writeU8(0xd0); send({t: 'p80c', id: hx(b, 8), rest: hx(b.add(8), 8)}); return true;
  }
  return false;
}
function tryPatch(kind, b) { try { return kind === 8 ? patchInfo(b) : patchIface(b); } catch (e) { return false; } }

Interceptor.attach(Process.getModuleByName('kernel32.dll').getExportByName('DeviceIoControl'), {
  onEnter(a) {
    const c = a[1].toUInt32(); this.c = c;
    if (c === 0x2ae808 || c === 0x2ae80c) { this.kind = c === 0x2ae808 ? 8 : 12; this.o = a[4]; pend.set(a[7].toString(), {kind: this.kind, buf: a[4]}); }
    else if (c === 0x2aec04) { sideOvl = a[7]; sideBuf = a[4]; sideH = a[0]; }
    else if (sideH !== null && a[0].equals(sideH) && ioLog < 200) { ioLog++; send({t: 'io', code: c.toString(16), insz: a[3].toUInt32(), inp: a[3].toUInt32() ? hx(a[2], Math.min(64, a[3].toUInt32())) : null}); }
  },
  onLeave(r) {
    if (this.c !== 0x2ae808 && this.c !== 0x2ae80c) return;
    const b = this.o, k = this.kind;
    if (tryPatch(k, b)) return;
    let t = 0; const iv = setInterval(() => { if (tryPatch(k, b) || ++t > 300) clearInterval(iv); }, 1);
  }
});

function writeMsg(b, cnt, bytes) { b.add(13).writeByteArray(bytes); cnt.writeU32(13 + bytes.length); }
function hello() {
  return [0x02, 0x20, (seq = (seq + 1) & 0xff), 0x1c,
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x7e,  // device id
    0x5e, 0x04, 0x00, 0x0b,                          // 045E:0B00
    5, 0, 17, 0, 0, 0, 0, 0,                         // firmware 5.17.0 rev 0
    1, 0, 1, 0, 1, 0, 1, 0];                         // hw, rf, security, gip versions
}
function input(b2, b3, lt, rt, sticks) {
  const p = new Array(47).fill(0);
  let p0 = 0, p1 = 0;
  if (b3 & 0x10) p0 |= 0x10; if (b3 & 0x20) p0 |= 0x20; if (b3 & 0x40) p0 |= 0x40; if (b3 & 0x80) p0 |= 0x80;  // A B X Y
  if (b2 & 0x10) p0 |= 0x04; if (b2 & 0x20) p0 |= 0x08;                                                    // start(menu) back(view)
  if (b2 & 0x01) p1 |= 0x01; if (b2 & 0x02) p1 |= 0x02; if (b2 & 0x04) p1 |= 0x04; if (b2 & 0x08) p1 |= 0x08;  // dpad U D L R
  if (b3 & 0x01) p1 |= 0x10; if (b3 & 0x02) p1 |= 0x20; if (b2 & 0x40) p1 |= 0x40; if (b2 & 0x80) p1 |= 0x80;  // LB RB LS RS
  p[0] = p0; p[1] = p1;
  const ltv = Math.round(lt * 1023 / 255), rtv = Math.round(rt * 1023 / 255);
  p[2] = ltv & 0xff; p[3] = ltv >> 8; p[4] = rtv & 0xff; p[5] = rtv >> 8;
  for (let i = 0; i < 8; i++) p[6 + i] = sticks[i];
  p[14] = mask & 0x0f;  // Elite 2 paddles; p[20] (profile) stays 0 or SDL clears them
  return [0x20, 0x00, (seq = (seq + 1) & 0xff), 47].concat(p);
}

Interceptor.attach(Process.getModuleByName('kernelbase.dll').getExportByName('GetOverlappedResult'), {
  onEnter(a) { this.ov = a[1]; this.cnt = a[2]; },
  onLeave(r) {
    if (r.toInt32() === 0) return;
    const k = this.ov.toString();
    if (pend.has(k)) { const p = pend.get(k); tryPatch(p.kind, p.buf); }
    if (sideOvl === null || !this.ov.equals(sideOvl)) return;
    try {
      const b = sideBuf, n = this.cnt.readU32();
      if (n < 15) return; if (frameLog < 8) { frameLog++; send({t: 'frame', id: hx(b, 13), n: n, msg: hx(b.add(13), Math.min(n - 13, 8))}); } if (b.add(8).readU8() !== 0x84) return;
      if (needHello) { needHello = false; writeMsg(b, this.cnt, hello()); send({t: 'hello'}); return; }
      const t = b.add(13).readU8(), l = b.add(14).readU8();
      if (t === 0x00 && l === 0x14 && n >= 33) {
        const sticks = new Uint8Array(b.add(19).readByteArray(8));
        last = input(b.add(15).readU8(), b.add(16).readU8(), b.add(17).readU8(), b.add(18).readU8(), Array.from(sticks));
        writeMsg(b, this.cnt, last); conv++;
        if (conv === 1 || conv % 2000 === 0) send({t: 'conv', n: conv});
      } else {
        if (other < 20) { other++; send({t: 'other', n: n, h: b.add(13).readByteArray(Math.min(n - 13, 16))}); }
        const m = last ? last.slice() : input(0, 0, 0, 0, [0, 0, 0, 0, 0, 0, 0, 0]);
        m[2] = (seq = (seq + 1) & 0xff); m[18] = mask & 0x0f;
        writeMsg(b, this.cnt, m);
      }
    } catch (e) { send({t: 'err', e: e.toString()}); }
  }
});
send({t: 'ready'});
"""

POLL = bytes([0x81, 0x04, 0x03, 0x00, 0x23, 0x01]) + bytes(58)
BITS = [(0x40000000, 0x01), (0x02000000, 0x02), (0x00200000, 0x04), (0x04000000, 0x08)]  # R4, PR, L4, PL -> P1..P4


def opendev():
    for d in hid.enumerate(0x2dc8, 0x310b):
        if d['interface_number'] == 2:
            h = hid.device(); h.open_path(d['path']); h.set_nonblocking(False); return h
    return None


def log(o):
    out.write(json.dumps(o) + '\n')


s = frida.get_local_device().attach(pid); sc = s.create_script(JS)
sc.on('message', lambda m, d: log(m['payload'] if m['type'] == 'send' else m))
sc.load(); t0 = time.time(); last = -1; misses = 0; h = None
try:
    while time.time() - t0 < secs:
        if h is None:
            try: h = opendev()
            except Exception: h = None
            if h is None:
                if last != 0: last = 0; sc.post({'type': 'mask', 'v': 0})
                time.sleep(1); continue
            log({'t': 'opened', 'at': round(time.time() - t0, 2)})
        try:
            h.write(POLL); r = h.read(64, 20)
        except Exception as e:
            log({'t': 'lost', 'e': str(e), 'at': round(time.time() - t0, 2)})
            try: h.close()
            except Exception: pass
            h = None; last = 0; sc.post({'type': 'mask', 'v': 0}); time.sleep(1); continue
        if r and bytes(r[:7]) == bytes([2, 4, 4, 0, 3, 0, 12]):
            m = int.from_bytes(bytes(r[18:22]), 'little'); v = 0
            for bit, pb in BITS:
                if m & bit: v |= pb
            if v != last: last = v; sc.post({'type': 'mask', 'v': v}); log({'t': 'paddle', 'v': v, 'at': round(time.time() - t0, 2)})
            misses = 0
        else:
            misses += 1
            if misses >= 3 and last != 0: last = 0; sc.post({'type': 'mask', 'v': 0})
        time.sleep(0.004)
finally:
    try: sc.post({'type': 'mask', 'v': 0}); time.sleep(0.2)
    except Exception: pass
    try: sc.unload(); s.detach()
    except Exception: pass
    log({'t': 'done'})
