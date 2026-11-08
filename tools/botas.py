"""Test friend without a friend: joins the session the launcher wrote (as host) and walks in a circle around
player 0 (you). Run while the game is running: python -I tools/botas.py [masina]
With "masina" it drives an Infernus on a 15 m circle instead of walking."""
import math, os, socket, struct, sys, time
DRIVE = len(sys.argv) > 1 and sys.argv[1] == "masina"

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
    if DRIVE:
        r, w = 15.0, 0.5  # 7.5 m/s around a 15 m circle
        a = (time.time() - t0) * w
        x, y = center[0] + r * math.cos(a), center[1] + r * math.sin(a)
        heading = math.degrees(a) % 360  # SA: 0 = north (+y), counter-clockwise; at angle a the circle runs (-sin a, cos a)
        h = math.radians(heading)
        st = struct.pack("<12f4i", x, y, center[2] + 0.5, heading, 0, 0, 0, 0, 0, math.sin(h / 2), math.cos(h / 2), r * w,
                         1, 411, 100, 1000)
    else:
        a = (time.time() - t0) * 0.4  # ~1.6 m/s on a 4 m circle: a walk
        x, y = center[0] + 4 * math.cos(a), center[1] + 4 * math.sin(a)
        heading = (math.degrees(a) + 180) % 360
        st = struct.pack("<12f4i", x, y, center[2], heading, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 100, 0)
    s.sendto(MAGIC + bytes([4]) + st, srv)
    time.sleep(0.05)
