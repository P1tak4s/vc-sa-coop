"""Test friend without a friend: joins the session the launcher wrote (as host) and walks in a circle around
player 0 (you). Run while the game is running: python -I tools/botas.py"""
import math, os, socket, struct, time

sess = dict(l.split("=", 1) for l in open(os.path.join(os.environ["LOCALAPPDATA"], "sa-de-coop", "session.txt")).read().split())
host, port = sess["server"].rsplit(":", 1)
srv = (host, int(port)); key = int(sess["key"], 16)
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(0.05)
MAGIC = b"SDC1"
s.sendto(MAGIC + bytes([1]) + struct.pack("<I", key) + b"Botas".ljust(16, b"\0"), srv)
center = None; t0 = time.time(); slot = None
while True:
    try:
        d, _ = s.recvfrom(512)
        if d[:4] == MAGIC and d[4] == 2: slot = d[5]; print("prisijungta, vieta", slot)
        if d[:4] == MAGIC and d[4] == 3: print("serveris nepriėmė"); break
        if d[:4] == MAGIC and d[4] == 4 and center is None:
            center = struct.unpack_from("<3f", d, 6); print("tavo vieta", center)
    except socket.timeout:
        pass
    if slot is None:
        s.sendto(MAGIC + bytes([1]) + struct.pack("<I", key) + b"Botas".ljust(16, b"\0"), srv); time.sleep(0.5); continue
    if center is None: continue
    a = (time.time() - t0) * 0.4  # ~1.6 m/s on a 4 m circle: a walk
    x, y = center[0] + 4 * math.cos(a), center[1] + 4 * math.sin(a)
    heading = (math.degrees(a) + 180) % 360
    st = struct.pack("<12f4i", x, y, center[2], heading, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 100, 0)
    s.sendto(MAGIC + bytes([4]) + st, srv)
    time.sleep(0.05)
