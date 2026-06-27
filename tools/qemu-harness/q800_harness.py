#!/usr/bin/env python3
"""q800_harness.py — boot a MacAtrium disk on QEMU's Quadra 800 (68040), headless,
and capture framebuffer screenshots over QMP. The q800/68040 is what Snow can't
emulate, so this is how the 7.5.5 + 24-bit ("Millions") variants get verified.

  q800_harness.py <rom> <disk> <out_dir> <seconds>
      [--snap-every S] [--keys "T:key;T:key;..."] [--ram MB] [--qemu BIN]

- <rom>  Quadra 800 ROM (1 MiB, crc32 4e70e3c0 — f1acad13.rom from mame macqd800).
- Screenshots -> <out_dir>/snap_NNN_<sec>s.png and final.png (640x480-ish macfb).
- --keys "20:ret;25:down" sends QMP key `ret` at 20 s, `down` at 25 s. Key names
  are QMP QKeyCodes: ret esc up down left right spc a-z 0-9 etc.
- The disk is opened with QEMU -snapshot, so the original image is never mutated.
"""
import sys, os, socket, json, time, subprocess, argparse, struct, zlib


def qmp_cmd(f, execute, **args):
    msg = {"execute": execute}
    if args:
        msg["arguments"] = args
    f.write(json.dumps(msg) + "\n")
    f.flush()
    while True:                       # skip async events, return the reply
        line = f.readline()
        if not line:
            raise RuntimeError("QMP connection closed")
        o = json.loads(line)
        if "return" in o or "error" in o:
            return o


def qmp_connect(path, timeout=40):
    deadline = time.time() + timeout
    s = None
    while time.time() < deadline:
        try:
            s = socket.socket(socket.AF_UNIX)
            s.connect(path)
            break
        except (FileNotFoundError, ConnectionRefusedError):
            s.close()
            s = None
            time.sleep(0.2)
    if s is None:
        raise RuntimeError("QMP socket never came up: " + path)
    f = s.makefile("rw")
    f.readline()                      # greeting
    qmp_cmd(f, "qmp_capabilities")
    return s, f


def ppm_to_png(ppm_path, png_path):
    """Minimal P6 PPM -> PNG (no external deps), so screendump works even where
    QEMU lacks the png format arg."""
    with open(ppm_path, "rb") as fh:
        data = fh.read()
    if not data.startswith(b"P6"):
        raise RuntimeError("not a P6 PPM")
    # parse header: P6 <w> <h> <maxval> then binary
    idx = 2
    fields = []
    while len(fields) < 3:
        while idx < len(data) and data[idx:idx + 1].isspace():
            idx += 1
        if data[idx:idx + 1] == b"#":
            while idx < len(data) and data[idx:idx + 1] != b"\n":
                idx += 1
            continue
        start = idx
        while idx < len(data) and not data[idx:idx + 1].isspace():
            idx += 1
        fields.append(int(data[start:idx]))
    w, h, _maxval = fields
    idx += 1                          # single whitespace after maxval
    rgb = data[idx:idx + w * h * 3]

    def chunk(typ, payload):
        c = typ + payload
        return struct.pack(">I", len(payload)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)

    raw = bytearray()
    for y in range(h):
        raw.append(0)                 # filter type 0
        raw += rgb[y * w * 3:(y + 1) * w * 3]
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 6))
    png += chunk(b"IEND", b"")
    with open(png_path, "wb") as fh:
        fh.write(png)


def screendump(f, png_path):
    r = qmp_cmd(f, "screendump", filename=png_path, format="png")
    if "error" in r:                  # older QEMU: PPM then convert ourselves
        ppm = png_path[:-4] + ".ppm"
        qmp_cmd(f, "screendump", filename=ppm)
        ppm_to_png(ppm, png_path)
        os.remove(ppm)


def send_key(f, name):
    qmp_cmd(f, "send-key", keys=[{"type": "qcode", "data": name}])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("rom")
    ap.add_argument("disk")
    ap.add_argument("out_dir")
    ap.add_argument("seconds", type=float)
    ap.add_argument("--snap-every", type=float, default=10.0)
    ap.add_argument("--keys", default="")
    ap.add_argument("--ram", type=int, default=128)
    ap.add_argument("--qemu", default="qemu-system-m68k")
    a = ap.parse_args()

    os.makedirs(a.out_dir, exist_ok=True)
    sock = os.path.join(a.out_dir, "qmp.sock")
    if os.path.exists(sock):
        os.remove(sock)

    # T(seconds) -> [keys]
    sched = {}
    for part in filter(None, (p.strip() for p in a.keys.split(";"))):
        t, k = part.split(":")
        sched.setdefault(float(t), []).append(k.strip())

    cmd = [
        a.qemu, "-M", "q800", "-bios", a.rom, "-m", str(a.ram),
        "-drive", f"file={a.disk},format=raw,if=none,id=hd0,snapshot=on",
        "-device", "scsi-hd,drive=hd0,scsi-id=0",
        "-display", "none",
        "-serial", "file:" + os.path.join(a.out_dir, "serial.log"),
        "-qmp", f"unix:{sock},server,nowait",
    ]
    print("launch:", " ".join(cmd), flush=True)
    qlog = open(os.path.join(a.out_dir, "qemu.log"), "wb")
    proc = subprocess.Popen(cmd, stdout=qlog, stderr=subprocess.STDOUT)
    try:
        _s, f = qmp_connect(sock)
        start = time.time()
        n = 0
        fired = set()
        next_snap = 0.0
        while True:
            now = time.time() - start
            if now >= a.seconds:
                break
            for t in sorted(sched):
                if t <= now and t not in fired:
                    for k in sched[t]:
                        send_key(f, k)
                        print(f"[{now:5.1f}s] key {k}", flush=True)
                    fired.add(t)
            if now >= next_snap:
                p = os.path.join(a.out_dir, f"snap_{n:03d}_{int(now)}s.png")
                try:
                    screendump(f, p)
                    print(f"[{now:5.1f}s] snapshot {p}", flush=True)
                except Exception as e:
                    print(f"[{now:5.1f}s] screendump failed: {e}", flush=True)
                n += 1
                next_snap += a.snap_every
            time.sleep(0.25)
        screendump(f, os.path.join(a.out_dir, "final.png"))
        print(f"final snapshot after {a.seconds}s", flush=True)
        qmp_cmd(f, "quit")
    finally:
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        qlog.close()


if __name__ == "__main__":
    main()
