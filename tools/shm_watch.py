# Prints the paddle-keys <-> hook shared memory N times, every 0.5 s. Usage: python shm_watch.py N
import ctypes, struct, time
k=ctypes.windll.kernel32
k.OpenFileMappingW.restype=ctypes.c_void_p; k.MapViewOfFile.restype=ctypes.c_void_p
k.MapViewOfFile.argtypes=[ctypes.c_void_p,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_size_t]
h=k.OpenFileMappingW(0x0004,0,r"Local\paddle-keys-steam")
v=k.MapViewOfFile(h,0x0004,0,0,48)
k.GetTickCount64.restype=ctypes.c_uint64
for i in range(int(__import__('sys').argv[1])):
    b=ctypes.string_at(v,48)
    magic,ver,en,pad,hb,gen,hpid,hgen,frames=struct.unpack('<IIIIQIIIxxxxQ',b)
    print(f"en={en} paddles={pad:#x} hb_age={k.GetTickCount64()-hb}ms gen={gen} hook_pid={hpid} hook_gen={hgen} frames={frames}",flush=True)
    time.sleep(0.5)
