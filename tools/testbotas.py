"""Test friend that checks the co-op by itself and writes down what it finds.

Joins the in-game host (co-op window -> "Bandymas su robotu"), then runs test phases as your friend: walking,
running, a jump, driving, a parked car for G, riding in your car, being down, another interior. The game
(vc-coop.js) answers every 2 s with what it sees of the bot (REPORT events); the bot compares that with where it
really is and writes a report. After the phases it stays as a friend: it follows you on foot and rides along
when you drive.

    KODAS=<code from coopnet.log> python -I tools/testbotas.py          # Vice City (default)
    KODAS=... ZAIDIMAS=sa python -I tools/testbotas.py                   # San Andreas (no REPORT events there)
    KODAS=... ETAPAI=eina,masina python -I tools/testbotas.py            # only these phases

Report: %LOCALAPPDATA%\\<vc|sa>-de-coop\\bot-report.md (summary) and bot-log.jsonl (everything it saw).

With an OpenAI key (OPENAI_API_KEY, or ~/.config/openai.env) the friend mode is run by gpt-6-luna: every ~5 s it
gets the situation (you, what the game reports about the bot, new events) and picks what to do, with a one-line
note on how the co-op behaves; it also writes the conclusion of the test phases. DI=0 turns that off.
"""
import json, math, os, socket, struct, sys, threading, time, urllib.request

sys.stdout.reconfigure(encoding="utf-8", errors="replace")  # Lithuanian letters on a cp1252 console
GAME = os.environ.get("ZAIDIMAS", "vc").lower()
DATA = os.path.join(os.environ.get("LOCALAPPDATA", "."), f"{GAME}-de-coop")
CAR = 141 if GAME == "vc" else 411  # Infernus
SKIN = int(os.environ.get("SKIN", "1"))
MAGIC = b"SDC1"
JOIN, WELCOME, DENY, STATE, LEAVE, REVIVE, NAMES, EVENT, ENTS = 1, 2, 3, 4, 5, 6, 7, 8, 9
EV_NAMES = {1: "oras", 2: "pjuvis", 3: "misija", 4: "smugis", 5: "pinigai", 6: "tekstas", 7: "zymes", 9: "raportas"}
PHASES = [  # name, seconds, what to do (shown on screen through the log)
    ("eina", 20, "vaikscioja ratu 4 m aplink tave su pistoletu"),
    ("bega", 15, "bega ratu 10 m aplink tave"),
    ("suolis", 12, "suoliuoja 30 m ir atgal kas 4 s"),
    ("masina", 25, "vairuoja Infernus ratu 15 m"),
    ("stovi", 30, "stovi masinoje 4 m i rytus: prieik ir spausk G"),
    ("keleivis", 30, "sesk i masina ir vaziuok: robotas sedes salia"),
    ("bagazine", 30, "vaziuok masina: robotas vaziuos ant bagazines (papildoma vieta)"),
    ("suzeistas", 35, "guli suzeistas 3 m i rytus: prieik ir laikyk E"),
    ("interjeras", 12, "persikelia i kita interjera (turi dingti)"),
]
SCALE = float(os.environ.get("TRUKME", "1"))  # 0.2 = every phase five times shorter (tests of the bot itself)
PHASES = [(n, max(3, int(s * SCALE)), w) for n, s, w in PHASES]
only = [p for p in os.environ.get("ETAPAI", "").split(",") if p]
if only:
    PHASES = [p for p in PHASES if p[0] in only]

os.makedirs(DATA, exist_ok=True)
log_f = open(os.path.join(DATA, "bot-log.jsonl"), "a", encoding="utf-8")


def note(kind, **kw):
    kw.update(t=round(time.time(), 2), kind=kind)
    log_f.write(json.dumps(kw, ensure_ascii=False) + "\n"); log_f.flush()


def say(text):
    print(time.strftime("%H:%M:%S"), text, flush=True)
    note("say", text=text)


# ---- connect -------------------------------------------------------------------------------------------
code = os.environ.get("KODAS", "")
if not code:
    sys.exit("KODAS=<kodas> reikia: co-op lange spausk 'Bandymas su robotu', kodas yra coopnet.log")
A = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"; v = 0
for c in code.replace("-", "").upper():
    v = v << 5 | A.index({"O": "0", "I": "1", "L": "1"}.get(c, c))
srv = ("127.0.0.1", (v >> 32) & 0xFFFF); key = v & 0xFFFFFFFF
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); sock.settimeout(0.02)
join = MAGIC + bytes([JOIN]) + struct.pack("<I", key) + b"Botas".ljust(16, b"\0")

slot = None
you = {}          # slot -> latest state of a player (you)
host_slot = None
reports = []      # (time, slot of reporter, dict) of F<botslot> lines
world = {}        # latest W line per reporter
heard = []        # events from the game
revived_by_you = False
ents_seen = 0

STATE_FMT = "<12f8i"
FIELDS = "x y z heading vx vy vz qx qy qz qw car_speed in_car car_model health car_health interior clock weapon flags".split()


def parse_report(text):
    out = {}
    for tok in text.split()[1:]:
        if "=" in tok:
            k, v = tok.split("=", 1); out[k] = v
    return out


CLEO_LOG = os.environ.get("CLEO_LOG", r"C:\Program Files (x86)\Steam\steamapps\common\GTA Vice City - The Definitive Edition\Gameface\Binaries\Win64\cleo_redux.log"
                          if GAME == "vc" else r"C:\Program Files (x86)\Steam\steamapps\common\GTA San Andreas - The Definitive Edition\Gameface\Binaries\Win64\cleo_redux.log")
cleo_seen = os.path.getsize(CLEO_LOG) if os.path.exists(CLEO_LOG) else 0
cleo_problems = []


def watch_cleo():
    """New ERROR / timed out / disposed lines of the game's CLEO log: the script died or misbehaves."""
    global cleo_seen
    try:
        size = os.path.getsize(CLEO_LOG)
        if size < cleo_seen:
            cleo_seen = 0
        if size == cleo_seen:
            return
        with open(CLEO_LOG, encoding="utf-8", errors="replace") as f:
            f.seek(cleo_seen); new = f.read(); cleo_seen = size
        for line in new.splitlines():
            if any(w in line for w in ("[ERROR]", "timed out", "disposed", "exception")) and "CLEO_TEXT" not in line:
                cleo_problems.append(line.strip()); say(f"CLEO: {line.strip()}"); note("cleo", text=line.strip())
    except OSError:
        pass


def pump():
    watch_cleo()
    """Read everything waiting; returns nothing, fills the globals."""
    global slot, host_slot, revived_by_you, ents_seen
    while True:
        try:
            d, _ = sock.recvfrom(2048)
        except (socket.timeout, BlockingIOError, ConnectionResetError):
            return
        if d[:4] != MAGIC or len(d) < 5:
            continue
        k = d[4]
        if k == WELCOME:
            slot = d[5]; say(f"prisijungta, mano vieta {slot}")
        elif k == DENY:
            say("serveris nepriėmė (blogas kodas?)"); sys.exit(1)
        elif k == STATE and len(d) >= 6 + 80:
            st = dict(zip(FIELDS, struct.unpack_from(STATE_FMT, d, 6)))
            prev = you.get(d[5])
            you[d[5]] = st
            if host_slot is None:
                host_slot = d[5]
            if prev:
                for f, label in (("in_car", "masinoje"), ("interior", "interjeras"), ("weapon", "ginklas")):
                    if prev[f] != st[f]:
                        note("you", field=f, value=st[f]); say(f"tu: {label} = {st[f]}")
                if (prev["health"] < 0) != (st["health"] < 0):
                    say("tu: " + ("suzeistas" if st["health"] < 0 else "atsikelei"))
                if (prev["flags"] >> 8 & 0xff) != (st["flags"] >> 8 & 0xff):
                    say(f"tu: ieskomumas {st['flags'] >> 8 & 0xff}")
        elif k == EVENT and len(d) > 7:
            frm, kind, text = d[5], d[6], d[7:].split(b"\0")[0].decode(errors="replace")
            if kind == 9:
                if text.startswith(f"F{slot} "):
                    reports.append((time.time(), frm, parse_report(text)))
                elif text.startswith("W "):
                    w = parse_report(text); old = world.get(frm, {})
                    for f in ("cut", "mis", "a", "car", "down", "err"):
                        if old.get(f) != w.get(f):
                            say(f"zaidimas {frm}: {f} = {w.get(f)}")
                    world[frm] = w
                    note("world", frm=frm, **w)
                elif text.startswith("E "):
                    say(f"KLAIDA zaidime {frm}: {text[2:]}"); note("error", frm=frm, text=text[2:])
            else:
                heard.append((time.time(), frm, kind, text))
                say(f"ivykis is {frm}: {EV_NAMES.get(kind, kind)} {text!r}"); note("event", frm=frm, ev=EV_NAMES.get(kind, kind), text=text)
        elif k == ENTS and len(d) > 6:
            if d[6] != ents_seen:
                ents_seen = d[6]; say(f"misijos objektai is {d[5]}: {d[6]}")
        elif k == REVIVE and len(d) >= 7 and d[6] == slot:
            revived_by_you = True; say("tu mane atgaivinai (E)")


def send_state(x, y, z, heading=0.0, in_car=0, car_model=0, health=100, car_health=0, interior=0, weapon=0,
               speed=0.0, q=(0.0, 0.0, 0.0, 1.0), vel=(0.0, 0.0, 0.0), fight=False):
    # flag 4: "fight for me" (the game's copy of the bot attacks mission enemies)
    st = struct.pack(STATE_FMT, x, y, z, heading, *vel, *q, speed, in_car, car_model, health, car_health, interior,
                     720, weapon, SKIN << 16 | (4 if fight else 0))
    sock.sendto(MAGIC + bytes([STATE]) + st, srv)


def heading_q(h_deg):
    h = math.radians(h_deg)
    return (0.0, 0.0, math.sin(h / 2), math.cos(h / 2))


# ---- wait for the game -----------------------------------------------------------------------------------
say(f"jungiuosi prie {srv[0]}:{srv[1]} ({GAME})")
t_join = 0
while slot is None or host_slot is None:
    if time.time() - t_join > 1:
        sock.sendto(join, srv); t_join = time.time()
    pump(); time.sleep(0.02)
me = you[host_slot]
center = (me["x"], me["y"], me["z"])
say(f"tu esi {center[0]:.1f} {center[1]:.1f} {center[2]:.1f}; pradedu testus")

# ---- the AI (gpt-6-luna) ------------------------------------------------------------------------------------
def load_key():
    if os.environ.get("OPENAI_API_KEY"):
        return os.environ["OPENAI_API_KEY"]
    try:
        for line in open(os.path.join(os.path.expanduser("~"), ".config", "openai.env"), encoding="utf-8"):
            if line.startswith("OPENAI_API_KEY="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return ""


AI_KEY = load_key() if os.environ.get("DI", "1") != "0" else ""
ai = {"veiksmas": "sekti", "x": None, "y": None, "busy": False, "notes": [], "asked": 0.0, "events_from": 0, "last_sit": ""}
AI_RULES = (
    "Tu esi testų robotas, žaidžiantis kaip žaidėjo draugas GTA Vice City Definitive Edition co-op modo bandyme. "
    "Gauni situaciją JSON. Grąžink TIK JSON: {\"veiksmas\": \"sekti\"|\"salia\"|\"keleivis\"|\"eiti\"|\"laukti\"|\"kovoti\", "
    "\"x\": skaičius arba null, \"y\": skaičius arba null, \"pastaba\": \"\"}. "
    "sekti = eiti 2 m už žaidėjo; salia = stovėti šalia; keleivis = sėdėti žaidėjo mašinoje (tik kai jis vairuoja); "
    "eiti = nueiti į x,y (pvz. misijos tikslą); laukti = stovėti vietoje; "
    "kovoti = būti šalia žaidėjo su M4 ir pulti misijos priešus (rinkis, kai žaidėjas misijoje (mis ne '-') ir pėsčias, "
    "ypač kai misijos objektų > 0 arba žaidėjas sužeistas/ieškomas). "
    "pastaba: vienas trumpas sakinys lietuviškai apie tai, ką pastebėjai apie co-op veikimą: ar tavo kopija žaidėjo "
    "žaidime yra ten, kur tu (raporto e metrais), ar matomas vardas (nm), pjūviai (cut, hc), misijos (mis, hm), klaidos "
    "(err, E), interjerai (a, ar). Nekartok to, kas jau yra tavo ankstesnėse pastabose ar testų rezultatuose: "
    "jei nieko naujo, pastaba turi būti tuščia. Nieko neišgalvok: rašyk tik tai, kas yra duomenyse. Nenaudok brūkšnių."
)


def ai_call(messages, max_tokens=4096):
    body = json.dumps({"model": "gpt-6-luna", "reasoning_effort": "low", "max_completion_tokens": max_tokens,
                       "response_format": {"type": "json_object"}, "messages": messages}).encode()
    req = urllib.request.Request("https://api.openai.com/v1/chat/completions", data=body,
                                 headers={"Authorization": f"Bearer {AI_KEY}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(json.loads(resp.read())["choices"][0]["message"]["content"] or "{}")


def situation():
    me = you.get(host_slot, {})
    last = reports[-1][2] if reports else {}
    new_events = [{"is": f, "ivykis": EV_NAMES.get(k, k), "tekstas": t} for _, f, k, t in heard[ai["events_from"]:]][-8:]
    player = {k: (round(me[k], 1) if isinstance(me.get(k), float) else me.get(k))
              for k in ("x", "y", "z", "heading", "in_car", "car_model", "health", "interior")}
    player["ieskomumas"] = me.get("flags", 0) >> 8 & 0xff
    return {
        "zaidejas": player,
        "zaidimo_raportas_apie_mane": last,
        "zaidimo_busena": world.get(host_slot, {}),
        "misijos_objektu": ents_seen,
        "nauji_ivykiai": new_events,
        "mano_veiksmas": ai["veiksmas"],
        "testu_rezultatai": [{"etapas": x["etapas"], "ok": x["ok"]} for x in results],
        "tavo_ankstesnes_pastabos": [n for _, n in ai["notes"][-5:]],
    }


def ai_tick():
    if not AI_KEY or ai["busy"] or time.time() - ai["asked"] < 5 or host_slot not in you:
        return
    sit = situation()
    # ask only when something changed (positions rounded to 5 m), or every 30 s
    coarse = json.dumps([sit["zaidejas"].get("in_car"), sit["zaidejas"].get("interior"), sit["zaidejas"].get("health", 0) < 0,
                         round((sit["zaidejas"].get("x") or 0) / 5), round((sit["zaidejas"].get("y") or 0) / 5),
                         sit["zaidimo_raportas_apie_mane"].get("p"), sit["zaidimo_raportas_apie_mane"].get("c"),
                         sit["zaidimo_busena"], sit["nauji_ivykiai"]], sort_keys=True, default=str)
    if coarse == ai["last_sit"] and time.time() - ai["asked"] < 30:
        return
    ai["last_sit"] = coarse
    ai["events_from"] = len(heard)
    ai["busy"] = True
    ai["asked"] = time.time()

    def run():
        try:
            out = ai_call([{"role": "system", "content": AI_RULES},
                           {"role": "user", "content": json.dumps(sit, ensure_ascii=False)}])
            if out.get("veiksmas") in ("sekti", "salia", "keleivis", "eiti", "laukti", "kovoti"):
                if out["veiksmas"] != ai["veiksmas"]:
                    say(f"DI: dabar {out['veiksmas']}")
                ai.update(veiksmas=out["veiksmas"], x=out.get("x"), y=out.get("y"))
            if out.get("pastaba"):
                ai["notes"].append((time.time(), out["pastaba"]))
                say(f"DI: {out['pastaba']}")
                note("ai", text=out["pastaba"])
        except Exception as e:  # network or a bad answer: keep the last action
            note("ai_error", text=str(e)[:200])
        finally:
            ai["busy"] = False
    threading.Thread(target=run, daemon=True).start()


def ai_conclusion():
    if not AI_KEY or not results:
        return
    try:
        out = ai_call([{"role": "system", "content": "Tu vertini GTA Vice City co-op modo testus. Grąžink JSON "
                        "{\"isvada\": \"...\"}: 3-6 sakiniai lietuviškai: kas veikia, kas ne, ką patikrinti toliau. "
                        "Tik pagal duomenis, nieko neišgalvok. Nenaudok brūkšnių."},
                       {"role": "user", "content": json.dumps({"rezultatai": results, "zaidimo_busena": world, "ivykiai": [
                           {"ivykis": EV_NAMES.get(k, k), "tekstas": t} for _, _, k, t in heard[-30:]]}, ensure_ascii=False)}])
        ai["conclusion"] = out.get("isvada", "")
        say(f"DI išvada: {ai['conclusion']}")
    except Exception as e:
        note("ai_error", text=str(e)[:200])


# ---- phases ----------------------------------------------------------------------------------------------
results = []


def run_phase(name, seconds, what):
    global center, revived_by_you
    me = you[host_slot]; center = (me["x"], me["y"], me["z"])
    say(f"=== {name}: {what} ({seconds} s)")
    start = time.time(); r_from = len(reports); you_rode = False; i_rode = False
    revived_by_you = False; truth = []
    while time.time() - start < seconds:
        pump()
        t = time.time() - start
        me = you.get(host_slot, me)
        cx, cy, cz = center
        if name == "eina":
            a = t * 0.4; x, y = cx + 4 * math.cos(a), cy + 4 * math.sin(a)
            send_state(x, y, cz, (math.degrees(a) + 180) % 360, weapon=17, vel=(-1.6 * math.sin(a), 1.6 * math.cos(a), 0))
        elif name == "bega":
            a = t * 0.5; x, y = cx + 10 * math.cos(a), cy + 10 * math.sin(a)
            send_state(x, y, cz, (math.degrees(a) + 180) % 360, vel=(-5 * math.sin(a), 5 * math.cos(a), 0))
        elif name == "suolis":
            far = int(t / 4) % 2 == 1; x, y = cx + (30 if far else 3), cy
            send_state(x, y, cz, 90)
        elif name == "masina":
            r, w = 15.0, 0.5; a = t * w; x, y = cx + r * math.cos(a), cy + r * math.sin(a)
            hd = math.degrees(a) % 360
            send_state(x, y, cz + 0.5, hd, in_car=1, car_model=CAR, car_health=1000, speed=r * w, q=heading_q(hd),
                       vel=(-r * w * math.sin(a), r * w * math.cos(a), 0))
        elif name == "stovi":
            x, y = cx + 4, cy
            send_state(x, y, cz + 0.5, 0, in_car=1, car_model=CAR, car_health=1000, q=heading_q(0))
            if me["in_car"] == 2 and not you_rode:
                you_rode = True; say("tu sedi mano masinoje (G veikia)")
        elif name == "keleivis":
            if me["in_car"] == 1:
                x, y = me["x"], me["y"]
                send_state(x, y, me["z"], me["heading"], in_car=2, car_model=host_slot, car_health=0)
                i_rode = True
            else:
                x, y = cx + 2, cy
                send_state(x, y, cz, 270)
        elif name == "bagazine":
            if me["in_car"] == 1:
                x, y = me["x"], me["y"]
                send_state(x, y, me["z"], me["heading"], in_car=2, car_model=host_slot, car_health=2)
                i_rode = True
            else:
                x, y = cx + 2, cy
                send_state(x, y, cz, 270)
        elif name == "suzeistas":
            x, y = cx + 3, cy
            if revived_by_you:
                send_state(x, y, cz, 270, health=60)
            else:
                send_state(x, y, cz, 270, health=-1)
        elif name == "interjeras":
            x, y = cx + 3, cy
            send_state(x, y, cz, 270, interior=5 if t < seconds * 0.7 else 0)
        truth.append((time.time(), x, y))
        # revive you if you are down for 3 s
        if me["health"] < 0:
            if not hasattr(run_phase, "down_since") or run_phase.down_since is None:
                run_phase.down_since = time.time()
            elif time.time() - run_phase.down_since > 3:
                sock.sendto(MAGIC + bytes([REVIVE, host_slot]), srv); say("atgaivinu tave"); run_phase.down_since = None
        else:
            run_phase.down_since = None
        time.sleep(0.05)
    seen = [r for r in reports[r_from:] if r[1] == host_slot]
    errs = [float(r[2].get("e", -1)) for r in seen if float(r[2].get("e", -1)) >= 0]
    ok, notes = None, []
    if not seen:
        ok = False; notes.append("zaidimas nieko nepranese apie robota" + (" (CLEO: " + cleo_problems[-1][-80:] + ")" if cleo_problems else ""))
    else:
        present = sum(r[2].get("p") == "1" for r in seen) / len(seen)
        avg = sum(errs) / len(errs) if errs else -1
        worst = max(errs) if errs else -1
        names = sum(r[2].get("nm") == "1" for r in seen) / len(seen)
        notes.append(f"kopija yra {present:.0%} laiko, paklaida vid. {avg:.1f} m, blogiausia {worst:.1f} m, vardas matomas {names:.0%}")
        limit = {"bagazine": 5, "eina": 1.5, "bega": 3, "suolis": 31, "masina": 4, "stovi": 2, "keleivis": 5, "suzeistas": 2, "interjeras": 2}[name]
        ok = present > 0.8 and avg >= 0 and avg <= limit
        if name == "eina":
            w = [r[2].get("w") for r in seen]; notes.append(f"ginklas kopijoje: {w[-1] if w else '?'} (turi buti 17)")
            ok = ok and w and w[-1] == "17"
        if name == "suolis":
            late = [float(r[2]["e"]) for r in seen if float(r[2].get("e", -1)) >= 0]
            ok = present > 0.8 and late and min(late) < 2
        if name == "masina":
            cars = [r[2].get("c") for r in seen]; notes.append(f"kopijos masina: {cars[-1] if cars else '?'} (turi buti {CAR})")
            ok = ok and str(CAR) in cars
        if name == "stovi":
            notes.append("tu sedejai mano masinoje" if you_rode else "tu nesesai (G nepaspaustas arba neveikia)")
            if ok and not you_rode:
                ok = None  # the copy was fine, G was not tried
        if name == "keleivis":
            pas = [r[2].get("c") for r in seen if r[2].get("c") == "pas"]
            if not i_rode:
                ok = None; notes.append("tu nevaziavai: nepatikrinta")
            else:
                notes.append(f"kopija sedi tavo masinoje {len(pas)} raportuose"); ok = len(pas) > 0
        if name == "bagazine":
            ext = [r[2].get("c") for r in seen if r[2].get("c") == "ext"]
            if not i_rode:
                ok = None; notes.append("tu nevaziavai: nepatikrinta")
            else:
                notes.append(f"kopija ant bagazines {len(ext)} raportuose"); ok = len(ext) > 0
        if name == "suzeistas":
            dn = [r[2].get("dn") for r in seen]; notes.append(f"kopija guli: {'taip' if '1' in dn else 'ne'}; atgaivinai: {'taip' if revived_by_you else 'ne'}")
            ok = "1" in dn
        if name == "interjeras":
            ar = [r[2].get("ar") for r in seen]; notes.append(f"kopijos interjeras: {', '.join(sorted(set(ar)))} (turi buti 5)")
            ok = "5" in ar
    res = {"etapas": name, "ok": ok, "pastabos": notes, "raportu": len(seen)}
    results.append(res); note("phase", **res)
    say(f"--- {name}: {'GERAI' if ok else 'NEPATIKRINTA' if ok is None else 'BLOGAI'}; " + "; ".join(notes))
    write_report()


def write_report():
    lines = [f"# Testų roboto ataskaita ({GAME.upper()}, {time.strftime('%Y-%m-%d %H:%M')})", "",
             "| Etapas | Rezultatas | Ką matė |", "| --- | --- | --- |"]
    for r in results:
        verdict = "GERAI" if r["ok"] else "nepatikrinta" if r["ok"] is None else "BLOGAI"
        lines.append(f"| {r['etapas']} | {verdict} | {'; '.join(r['pastabos'])} |")
    lines += ["", "## Žaidimo būsena (paskutinis W raportas)", ""]
    for frm, w in world.items():
        lines.append(f"- žaidimas {frm}: " + ", ".join(f"{k}={v}" for k, v in w.items()))
    lines += ["", "## CLEO klaidos (skripto lūžiai)", ""]
    lines += [f"- {x}" for x in cleo_problems[-20:]] or ["- nėra"]
    lines += ["", "## Įvykiai (pjūviai, misijos, tekstai, oras)", ""]
    for t, frm, kind, text in heard[-40:]:
        lines.append(f"- {time.strftime('%H:%M:%S', time.localtime(t))} iš {frm}: {EV_NAMES.get(kind, kind)} `{text}`")
    if not heard:
        lines.append("- nieko")
    if ai.get("conclusion"):
        lines += ["", "## DI išvada", "", ai["conclusion"]]
    if ai["notes"]:
        lines += ["", "## DI pastabos (draugo režimas)", ""]
        lines += [f"- {time.strftime('%H:%M:%S', time.localtime(t))} {n}" for t, n in ai["notes"][-30:]]
    with open(os.path.join(DATA, "bot-report.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


for name, secs, what in PHASES:
    run_phase(name, secs, what)
ai_conclusion()
write_report()
say(f"testai baigti, ataskaita {os.path.join(DATA, 'bot-report.md')}; dabar esu tavo draugas (Ctrl+C baigti)")

# ---- friend mode: follow you, ride along when you drive --------------------------------------------------
last_write = time.time()
pos = None  # where the bot stands now (friend mode)
while True:
    pump()
    me = you.get(host_slot)
    ai_tick()
    if me:
        act = ai["veiksmas"] if AI_KEY else "sekti"
        h = math.radians(me["heading"])
        if me["in_car"] == 1 and act in ("sekti", "keleivis", "salia"):
            send_state(me["x"], me["y"], me["z"], me["heading"], in_car=2, car_model=host_slot, interior=me["interior"])
            pos = (me["x"], me["y"])
        elif act == "eiti" and isinstance(ai["x"], (int, float)) and isinstance(ai["y"], (int, float)):
            px, py = pos or (me["x"], me["y"])
            dx, dy = ai["x"] - px, ai["y"] - py
            d = math.hypot(dx, dy)
            if d > 0.5:
                step = min(d, 5 * 0.05)  # run at 5 m/s
                px, py = px + dx / d * step, py + dy / d * step
            pos = (px, py)
            send_state(px, py, me["z"], math.degrees(math.atan2(-dx, dy)) % 360, interior=me["interior"],
                       vel=(dx / d * 5, dy / d * 5, 0) if d > 0.5 else (0, 0, 0))
        elif act == "laukti" and pos:
            send_state(pos[0], pos[1], me["z"], 0, interior=me["interior"])
        elif act == "kovoti":  # beside you with an M4; the game's copy attacks mission enemies
            pos = (me["x"] + 1.5 * math.cos(h), me["y"] + 1.5 * math.sin(h))
            send_state(pos[0], pos[1], me["z"], me["heading"], interior=me["interior"], weapon=26, fight=True)
        else:
            if act == "salia":  # a step to your right
                pos = (me["x"] + 1.5 * math.cos(h), me["y"] + 1.5 * math.sin(h))
            else:  # sekti: 2 m behind you
                pos = (me["x"] + 2 * math.sin(h), me["y"] - 2 * math.cos(h))
            send_state(pos[0], pos[1], me["z"], me["heading"], interior=me["interior"])
        if me["health"] < 0:
            if not getattr(run_phase, "down_since", None):
                run_phase.down_since = time.time()
            elif time.time() - run_phase.down_since > 3:
                sock.sendto(MAGIC + bytes([REVIVE, host_slot]), srv); say("atgaivinu tave"); run_phase.down_since = None
    if time.time() - last_write > 10:
        write_report(); last_write = time.time()
    time.sleep(0.05)
