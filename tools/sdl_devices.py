# Lists the joysticks Steam's SDL3 currently has, with the backend that owns each, plus the SDL hints that
# matter here. Driver letter (GUID byte 14): h = HIDAPI, x = XInput, r = RawInput, w = WGI,
# 0 = DirectInput. Path "nvagipman:..." is the steamxbox sideband device.
# Usage: python sdl_devices.py STEAM_PID      Needs: pip install frida
import frida, sys

JS = r"""
const m = Process.getModuleByName('SDL3.dll');
const f = (n, r, a) => new NativeFunction(m.getExportByName(n), r, a);
const get = f('SDL_GetJoysticks', 'pointer', ['pointer']);
const name = f('SDL_GetJoystickNameForID', 'pointer', ['uint32']);
const path = f('SDL_GetJoystickPathForID', 'pointer', ['uint32']);
const vid = f('SDL_GetJoystickVendorForID', 'uint16', ['uint32']);
const pid = f('SDL_GetJoystickProductForID', 'uint16', ['uint32']);
const guid = f('SDL_GetJoystickGUIDForID', Array(16).fill('uint8'), ['uint32']);
const hint = f('SDL_GetHint', 'pointer', ['pointer']);
const s = p => p.isNull() ? null : p.readUtf8String();
const n = Memory.alloc(4), arr = get(n), out = [];
for (let i = 0; i < n.readS32(); i++) {
  const id = arr.add(4 * i).readU32();
  const g = guid(id).map(b => ('0' + (b & 255).toString(16)).slice(-2)).join('');
  out.push({id, name: s(name(id)), path: s(path(id)), vidpid: vid(id).toString(16) + ':' + pid(id).toString(16),
            guid: g, driver: String.fromCharCode(parseInt(g.substr(28, 2), 16)) || '0'});
}
const hints = {};
for (const h of ['SDL_GAMECONTROLLER_IGNORE_DEVICES', 'SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA',
                 'SDL_JOYSTICK_RAWINPUT', 'SDL_JOYSTICK_WGI', 'SDL_JOYSTICK_DIRECTINPUT'])
  hints[h] = s(hint(Memory.allocUtf8String(h)));
send({joysticks: out, hints});
"""

s = frida.get_local_device().attach(int(sys.argv[1]))
sc = s.create_script(JS)
sc.on('message', lambda m, d: (
    [print(j) for j in m['payload']['joysticks']], print(m['payload']['hints'])) if m['type'] == 'send' else print(m))
sc.load()
s.detach()
