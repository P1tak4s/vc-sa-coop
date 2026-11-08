/// <reference path=".config/sa.d.ts" />
// AI partner: a Grove Street gang member in your group (the game makes it follow you, get in your car and fight),
// with TypeSafe Jev choosing what it does every few seconds from a short description of the situation.
// Switched on in the co-op menu ("AI partneris"). Talks to coopnet64.asi (Jev runs there, off the game thread).

let lib;
for (const path of ["coopnet64.asi", "../coopnet64.asi"]) { lib = DynamicLibrary.Load(path); if (lib) break; }
if (!lib) exit("coopnet64.asi not found");
const fn = (n) => Memory.Fn.X64(lib.getProcedure(n));
const aiEnabled = fn("coop_ai_enabled"), aiAsk = fn("coop_ai_ask"), aiResult = fn("coop_ai_result");
const buf = Memory.Allocate(512);

const MODEL = 106;          // a Grove Street member
const WEAPON = 30, AMMO = 600; // AK-47
const ACTIONS = ["follow", "enter_car", "attack", "take_cover", "wait"];
const SAY = { follow: "AI: seku", enter_car: "AI: sedu i masina", attack: "AI: puolu", take_cover: "AI: slepiuosi", wait: "AI: laukiu" };
const CLEANUP = 0x53DB410; // mission cleanup list (Steam build 1.0.113.49697469): peds of the current mission

const player = new Player(0);
let ped = null, blip = null, action = "follow", lastAsk = 0, lastSit = "", deadAt = 0, target = null;

function writeAscii(text) {
  const n = Math.min(text.length, 500);
  for (let i = 0; i < n; i++) Memory.WriteU8(buf + i, text.charCodeAt(i) & 0x7f, false, false);
  Memory.WriteU8(buf + n, 0, false, false);
}

function d2(a, b) { return (a.x - b.x) ** 2 + (a.y - b.y) ** 2 + (a.z - b.z) ** 2; }

function spawn(me) {
  Streaming.RequestModel(MODEL);
  Streaming.LoadAllModelsNow();
  for (let i = 0; i < 40 && !Streaming.HasModelLoaded(MODEL); i++) wait(25);
  const p = me.getCoordinates();
  ped = Char.Create(8, MODEL, p.x + 1.5, p.y + 1.5, p.z);
  Streaming.MarkModelAsNoLongerNeeded(MODEL);
  try { ped.setHealth(200); } catch (e) {}
  try { ped.giveWeapon(WEAPON, AMMO); ped.setCurrentWeapon(WEAPON); } catch (e) {}
  try { player.getGroup().setMember(ped); } catch (e) { log(`[ai] group: ${e}`); }
  try { blip = Blip.AddForChar(ped); blip.setAsFriendly(true); } catch (e) {}
  action = "follow";
  log("[ai] partner spawned");
}

function despawn() {
  try { if (blip) blip.remove(); } catch (e) {}
  try { if (ped && Char.DoesExist(ped)) ped.delete(); } catch (e) {}
  ped = null; blip = null;
}

// mission peds that are alive and within 60 m: the enemies the partner should care about
function enemies(me) {
  const p = me.getCoordinates(), out = [];
  for (let i = 0; i < 75; i++) {
    const at = CLEANUP + i * 8;
    if (Memory.ReadU8(at, false, true) !== 2) continue;
    const h = Memory.ReadI32(at + 4, false, true);
    try {
      if (!Char.DoesExist(h)) continue;
      const c = new Char(h);
      if (ped && +c === +ped) continue;
      if (c.getHealth() <= 0) continue;
      const dd = d2(p, c.getCoordinates());
      if (dd < 3600) out.push({ c, dd });
    } catch (e) {}
  }
  return out.sort((a, b) => a.dd - b.dd);
}

function situation(me, foes) {
  const inCar = me.isInAnyCar();
  const dist = Math.sqrt(d2(me.getCoordinates(), ped.getCoordinates())).toFixed(0);
  return `CJ is ${inCar ? "in a car" : "on foot"}, CJ health ${me.getHealth()}; partner on foot ${dist} m from CJ, ` +
    `partner health ${ped.getHealth()}; ${ONMISSION ? "a story mission is running" : "free roam"}; ` +
    `${foes.length ? `${foes.length} hostile mission enemies within 60 m, nearest ${Math.sqrt(foes[0].dd).toFixed(0)} m` : "no enemies around"}.`;
}

function apply(next, me, foes) {
  if (next !== action) { action = next; try { showTextBox(SAY[next]); } catch (e) {} }
  try {
    if (next === "attack" && foes.length) {
      target = foes[0].c;
      Task.KillCharOnFoot(ped, target);
    } else if (next === "take_cover" && foes.length) {
      Task.SmartFleeChar(ped, foes[0].c, 25, 6000);
    } else if (next === "wait") {
      Task.StandStill(ped, 6000);
    } else {
      ped.clearTasks(); // follow / enter_car: the group does it
    }
  } catch (e) { log(`[ai] ${next}: ${e}`); }
}

log("[ai] ready");
while (true) {
  wait(100);
  if (!player.isPlaying()) continue;
  const me = player.getChar();
  if (!aiEnabled()) { if (ped) { despawn(); try { showTextBox("AI partneris isjungtas"); } catch (e) {} } continue; }

  if (ped && (!Char.DoesExist(ped) || ped.getHealth() <= 0)) {
    if (!deadAt) { deadAt = Date.now(); try { showTextBox("AI partneris zuvo. Grizta po 10 s"); } catch (e) {} }
    if (Date.now() - deadAt < 10000) continue;
    despawn(); deadAt = 0;
  }
  if (!ped) { spawn(me); continue; }
  // too far behind (e.g. CJ drove off alone): catch up next to CJ
  if (!me.isInAnyCar() && d2(me.getCoordinates(), ped.getCoordinates()) > 150 * 150) {
    const p = me.getCoordinates(); ped.setCoordinates(p.x + 2, p.y + 2, p.z);
  }

  const now = Date.now();
  const foes = enemies(me);
  const sit = situation(me, foes);
  // ask when something changed, at most every 2.5 s, and at least every 8 s
  if ((sit !== lastSit && now - lastAsk > 2500) || now - lastAsk > 8000) {
    writeAscii(sit);
    if (aiAsk(buf)) { lastAsk = now; lastSit = sit; }
  }
  const r = aiResult();
  if (r >= 0 && r < ACTIONS.length) apply(ACTIONS[r], me, foes);
}
