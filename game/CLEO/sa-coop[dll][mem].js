/// <reference path=".config/sa.d.ts" />
// SA Co-op: your friend in your world. Keys:
//   F9  the co-op window (session name + password, through Steam), from the title screen on
//   G   sit next to your friend in their car      E   revive your friend when they are down
// Talks to coopnet64.asi (next to SanAndreas.exe; the ASI loader starts it with the game, we use the same copy).

// ---- the DLL -------------------------------------------------------------------------------------------
let lib;
for (const path of ["coopnet64.asi", "../coopnet64.asi"]) {
  lib = DynamicLibrary.Load(path);
  if (lib) break;
}
if (!lib) { log("[sa-coop] coopnet64.asi not found"); exit("coopnet64.asi not found"); }

const api = {};
try {
  for (const name of ["coop_start", "coop_host", "coop_join_clipboard", "coop_code", "coop_status", "coop_slot",
                      "coop_send", "coop_get", "coop_revive", "coop_take_revived", "coop_name"]) {
    api[name] = Memory.Fn.X64(lib.getProcedure(name));
  }
} catch (e) {
  log(`[sa-coop] setup failed: ${e}`);
  exit("setup failed");
}

// proto::State, 80 bytes
const SIZE = 80;
const F = { x: 0, y: 4, z: 8, heading: 12, vx: 16, vy: 20, vz: 24, qx: 28, qy: 32, qz: 36, qw: 40, carSpeed: 44 };
const I = { inCar: 48, carModel: 52, health: 56, carHealth: 60, interior: 64, clock: 68, weapon: 72, flags: 76 };
const mine = Memory.Allocate(SIZE);
const theirs = Memory.Allocate(SIZE);
const codeBuf = Memory.Allocate(32);
const nameBuf = Memory.Allocate(32);

const KEY = { G: 71, E: 69 };
const SKINS = [105, 106, 107, 105]; // Grove Street looks until we sync clothes
const DOWN_AT = 20;                 // health below this: down instead of dead (while a friend is here)
const BLEED_OUT = 60000;            // ms down before the game takes over
const DOWN_ANIM = ["KO_skid_back", "PED"];

const player = new Player(0);
const remotes = [null, null, null, null];
let downed = false, downedAt = 0;

// ---- small helpers ------------------------------------------------------------------------------------
function say(text) { try { showTextBox(text); } catch (e) {} log(`[sa-coop] ${text}`); }

const keyWas = {};
function pressed(code) { // true once per press
  const down = Pad.IsKeyPressed(code);
  const fired = down && !keyWas[code];
  keyWas[code] = down;
  return fired;
}

function dist2(a, b) { const dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z; return dx * dx + dy * dy + dz * dz; }

function readAscii(buf, n) {
  let s = "";
  for (let i = 0; i < n; i++) s += String.fromCharCode(Memory.ReadU8(buf + i, false, false));
  return s;
}
function readCode() { return readAscii(codeBuf, api.coop_code(codeBuf, 32)); }

// ---- names over heads -----------------------------------------------------------------------------------
// The game draws text by key: each friend's name is stored under its own key, refreshed when it changes.
function nameKey(slot, r) {
  const name = readAscii(nameBuf, api.coop_name(slot, nameBuf, 16)) || `Player${slot + 1}`;
  if (name !== r.name) {
    r.name = name;
    try { FxtStore.insert(`COOPN${slot}`, name); } catch (e) { log(`[sa-coop] name key: ${e}`); }
  }
  return `COOPN${slot}`;
}

// world point -> the 640 x 448 screen the game's text uses; null when behind the camera or far away
function toScreen(p) {
  const cam = Camera.GetActiveCoordinates(), at = Camera.GetActivePointAt();
  let fx = at.x - cam.x, fy = at.y - cam.y, fz = at.z - cam.z;
  const fl = Math.sqrt(fx * fx + fy * fy + fz * fz) || 1; fx /= fl; fy /= fl; fz /= fl;
  let rx = fy, ry = -fx; const rl = Math.sqrt(rx * rx + ry * ry) || 1; rx /= rl; ry /= rl; // right = forward x up
  const ux = ry * fz, uy = -rx * fz, uz = rx * fy - ry * fx;                                 // up = right x forward
  const dx = p.x - cam.x, dy = p.y - cam.y, dz = p.z - cam.z;
  const depth = dx * fx + dy * fy + dz * fz;
  if (depth < 1 || depth > 80) return null;
  const tanX = Math.tan((Camera.GetFov() * Math.PI / 180) / 2), aspect = 16 / 9;
  const sx = 320 + ((dx * rx + dy * ry) / depth / tanX) * 320;
  const sy = 224 - ((dx * ux + dy * uy + dz * uz) / depth / (tanX / aspect)) * 224;
  return sx < 0 || sx > 640 || sy < 0 || sy > 448 ? null : { x: sx, y: sy };
}

function drawNames() {
  for (let slot = 0; slot < 4; slot++) {
    const r = remotes[slot];
    if (!r || !Char.DoesExist(r.ped)) continue;
    const head = r.ped.getCoordinates();
    head.z += 1.1;
    const at = toScreen(head);
    if (!at) continue;
    const key = nameKey(slot, r);
    try {
      Text.UseCommands(true);
      Text.SetFont(1);
      Text.SetScale(0.35, 1.4);
      Text.SetCenter(true);
      Text.SetColor(r.down ? 255 : 120, r.down ? 80 : 200, r.down ? 80 : 255, 255);
      Text.SetDropshadow(1, 0, 0, 0, 255);
      Text.Display(at.x, at.y, key);
    } catch (e) {
      if (!r.drawErr) { r.drawErr = true; log(`[sa-coop] draw name: ${e}`); }
    }
  }
}

function readRemote() {
  const s = {};
  for (const k in F) s[k] = Memory.ReadFloat(theirs + F[k], false, false);
  for (const k in I) s[k] = Memory.ReadI32(theirs + I[k], false, false);
  return s;
}

function loadModel(id) {
  Streaming.RequestModel(id);
  Streaming.LoadAllModelsNow();
  for (let i = 0; i < 40 && !Streaming.HasModelLoaded(id); i++) wait(25);
  return Streaming.HasModelLoaded(id);
}

function remoteWithCar(car) {
  for (let i = 0; i < 4; i++) if (remotes[i] && remotes[i].car && +remotes[i].car === +car) return i;
  return -1;
}

// ---- our state ------------------------------------------------------------------------------------------
function writeMine(me) {
  const p = me.getCoordinates();
  let v = { x: 0, y: 0, z: 0 }, q = { x: 0, y: 0, z: 0, w: 1 }, carSpeed = 0, inCar = 0, carModel = 0, carHealth = 0;
  if (me.isInAnyCar()) {
    const car = me.storeCarIsInNoSave();
    const c = car.getCoordinates();
    p.x = c.x; p.y = c.y; p.z = c.z;
    const ownerSlot = remoteWithCar(car);
    let driver = null;
    try { driver = car.getDriver(); } catch (e) {}
    if (ownerSlot >= 0 && !(driver && +driver === +me)) {
      inCar = 2; carModel = ownerSlot; carHealth = 0; // riding with a friend: they send the car
    } else {
      q = car.getQuaternion();
      carSpeed = car.getSpeed();
      inCar = 1;
      carModel = car.getModel();
      carHealth = car.getHealth();
    }
  } else {
    v = me.getVelocity();
  }
  const f = { x: p.x, y: p.y, z: p.z, heading: me.getHeading(), vx: v.x, vy: v.y, vz: v.z, qx: q.x, qy: q.y, qz: q.z, qw: q.w, carSpeed };
  for (const k in F) Memory.WriteFloat(mine + F[k], f[k], false, false);
  Memory.WriteI32(mine + I.inCar, inCar, false, false);
  Memory.WriteI32(mine + I.carModel, carModel, false, false);
  Memory.WriteI32(mine + I.health, downed ? -1 : me.getHealth(), false, false);
  Memory.WriteI32(mine + I.carHealth, carHealth, false, false);
  let area = 0;
  try { area = me.getAreaVisible(); } catch (e) {}
  Memory.WriteI32(mine + I.interior, area, false, false);
  const t = Clock.GetTimeOfDay();
  Memory.WriteI32(mine + I.clock, t.hours * 60 + t.minutes, false, false);
  let weapon = 0, flags = 0;
  try { weapon = me.getCurrentWeapon(); } catch (e) {}
  try { if (player.isTargetingAnything()) flags |= 1; } catch (e) {}
  try { if (me.isShooting()) flags |= 2; } catch (e) {}
  try { flags |= Math.min(6, player.storeWantedLevel()) << 8; } catch (e) {}
  Memory.WriteI32(mine + I.weapon, weapon, false, false);
  Memory.WriteI32(mine + I.flags, flags, false, false);
  api.coop_send(mine);
}

// ---- down and revive (us) -------------------------------------------------------------------------------
function anyFriend() { return remotes.some((r) => r); }

function goDown(me, now) {
  downed = true; downedAt = now;
  me.setProofs(true, true, true, true, true);
  player.setControl(false);
  try { Task.PlayAnim(me, DOWN_ANIM[0], DOWN_ANIM[1], 4.0, false, false, false, true, -1); } catch (e) {}
  say("SA Co-op: tu suzeistas. Draugas gali tave atgaivinti (60 s)");
}

function getUp(me, health) {
  downed = false;
  try { me.clearTasksImmediately(); } catch (e) {}
  me.setProofs(false, false, false, false, false);
  player.setControl(true);
  me.setHealth(health);
}

function handleOwnHealth(me, now) {
  if (!downed) {
    if (anyFriend() && !me.isInAnyCar() && me.getHealth() < DOWN_AT && me.getHealth() > 0) goDown(me, now);
    return;
  }
  if (api.coop_take_revived()) { getUp(me, 60); say("SA Co-op: draugas tave atgaivino"); return; }
  if (now - downedAt > BLEED_OUT || !anyFriend()) { getUp(me, 0); } // nobody came: the game's own death
}

// ---- friends --------------------------------------------------------------------------------------------
function spawnPed(slot, s) {
  const model = SKINS[slot];
  if (!loadModel(model)) return null;
  const ped = Char.Create(4, model, s.x, s.y, s.z);
  Streaming.MarkModelAsNoLongerNeeded(model);
  ped.setProofs(true, true, true, true, true); // traffic and cops must not kill the copy of your friend
  ped.setCantBeDraggedOut(true);
  ped.setKeepTask(true);
  const blip = Blip.AddForChar(ped);
  try { blip.setAsFriendly(true); blip.changeDisplay(2); } catch (e) {} // map and radar only: the name is over the head
  log(`[sa-coop] friend ${slot} appeared`);
  return { ped, car: null, carModel: 0, blip, lastTask: 0, lostAt: 0, lastPos: null, rx: null, down: false, hinted: false };
}

function leaveCar(r, s) {
  try { if (r.ped.isInAnyCar()) r.ped.warpFromCarToCoord(s.x, s.y, s.z); } catch (e) {}
}

function dropCar(r) {
  if (!r.car) return;
  try {
    if (Car.DoesExist(r.car)) {
      const c = r.car.getCoordinates();
      if (r.ped.isInAnyCar()) r.ped.warpFromCarToCoord(c.x, c.y, c.z);
      r.car.delete(); // our copy of their car: no wreck left behind
    }
  } catch (e) {}
  r.car = null; r.carModel = 0; r.rx = null;
}

function removeRemote(slot) {
  const r = remotes[slot];
  if (!r) return;
  dropCar(r);
  try { r.blip.remove(); } catch (e) {}
  try { r.ped.delete(); } catch (e) {}
  remotes[slot] = null;
  log(`[sa-coop] friend ${slot} left`);
}

function driveRemote(r, s, now) {
  if (!r.car || r.carModel !== s.carModel || !Car.DoesExist(r.car)) {
    dropCar(r);
    if (!loadModel(s.carModel)) return;
    r.car = Car.Create(s.carModel, s.x, s.y, s.z);
    Streaming.MarkModelAsNoLongerNeeded(s.carModel);
    r.car.setProofs(true, true, true, true, true);
    r.carModel = s.carModel;
    leaveCar(r, s);
    r.ped.warpIntoCar(r.car);
    r.rx = null;
  }
  // a new packet: remember where and when, and how fast it moved since the last one
  if (r.rx === null || s.x !== r.rx || s.y !== r.ry || s.z !== r.rz) {
    if (r.rx !== null && now > r.rt) {
      const dt = (now - r.rt) / 1000;
      r.vx = (s.x - r.rx) / dt; r.vy = (s.y - r.ry) / dt; r.vz = (s.z - r.rz) / dt;
    } else { r.vx = r.vy = r.vz = 0; }
    r.rx = s.x; r.ry = s.y; r.rz = s.z; r.rt = now;
  }
  const ahead = Math.min((now - r.rt) / 1000, 0.3);
  const tx = r.rx + r.vx * ahead, ty = r.ry + r.vy * ahead, tz = r.rz + r.vz * ahead;
  const c = r.car.getCoordinates();
  const ex = tx - c.x, ey = ty - c.y, ez = tz - c.z;
  const err = Math.sqrt(ex * ex + ey * ey + ez * ez);
  const speed = Math.sqrt(r.vx * r.vx + r.vy * r.vy);
  r.car.setQuaternion(s.qx, s.qy, s.qz, s.qw);
  if (err > 2) {
    r.car.setCoordinatesNoOffset(tx, ty, tz); // too far off: put it where it should be
    const fx0 = 2 * (s.qx * s.qy - s.qw * s.qz), fy0 = 1 - 2 * (s.qx * s.qx + s.qz * s.qz);
    r.car.setForwardSpeed(r.vx * fx0 + r.vy * fy0 < 0 ? -speed : speed);
    return;
  }
  // close enough: the game's physics drives it (wheels turn); catch up or fall back through the speed
  // which way the car faces (quaternion applied to +y): moving against it is reverse
  const fx = 2 * (s.qx * s.qy - s.qw * s.qz), fy = 1 - 2 * (s.qx * s.qx + s.qz * s.qz);
  const sign = r.vx * fx + r.vy * fy < 0 ? -1 : 1;
  let along = 0;
  if (speed > 0.5) along = (ex * r.vx + ey * r.vy) / speed;
  r.car.setForwardSpeed(sign * Math.max(0, speed + along * 1.5));
}

function rideRemote(r, s, mySlot, me) {
  if (r.car) dropCar(r);
  const driverSlot = s.carModel;
  let car = null;
  if (driverSlot === mySlot) {
    if (me.isInAnyCar()) car = me.storeCarIsInNoSave(); // they sit in our car
  } else if (remotes[driverSlot] && remotes[driverSlot].car) {
    car = remotes[driverSlot].car; // they sit in another friend's car
  }
  if (!car) { r.ped.setCoordinatesNoOffset(s.x, s.y, s.z); return; }
  if (!r.ped.isInAnyCar()) {
    try { r.ped.warpIntoCarAsPassenger(car, s.carHealth); } catch (e) { log(`[sa-coop] passenger: ${e}`); }
  }
}

function walkRemote(r, s, now) {
  if (r.car) dropCar(r);
  leaveCar(r, s);
  if (!r.lastPos) { r.lastPos = { x: s.x, y: s.y }; r.lastT = now; r.speed = 0; r.ux = 0; r.uy = 0; }
  const mx = s.x - r.lastPos.x, my = s.y - r.lastPos.y, m = Math.sqrt(mx * mx + my * my);
  if (m > 0.0001 && now > r.lastT) {
    r.speed = m / ((now - r.lastT) / 1000); r.ux = mx / m; r.uy = my / m;
    r.lastPos = { x: s.x, y: s.y }; r.lastT = now;
  } else if (now - r.lastT > 300) {
    r.speed = 0;
  }
  const p = r.ped.getCoordinates();
  const d = Math.sqrt(dist2(p, s));
  if (d > 5) {
    r.ped.setCoordinatesNoOffset(s.x, s.y, s.z);
    r.ped.setHeading(s.heading);
  } else if (r.speed > 0.3 || d > 1) {
    if (now - r.lastTask > 250) {
      const moveState = r.speed > 6 ? 7 : r.speed > 2.5 ? 6 : 4; // sprint, run, walk
      Task.GoStraightToCoord(r.ped, s.x + r.ux * 1.5, s.y + r.uy * 1.5, s.z, moveState, 1000);
      r.lastTask = now;
    }
  } else {
    r.ped.setHeading(s.heading);
  }
}

function downRemote(r, s, slot, me) {
  leaveCar(r, s);
  if (r.car) dropCar(r);
  if (!r.down) {
    r.down = true; r.hinted = false;
    r.ped.setCoordinatesNoOffset(s.x, s.y, s.z);
    try { Task.PlayAnim(r.ped, DOWN_ANIM[0], DOWN_ANIM[1], 4.0, false, false, false, true, -1); } catch (e) {}
    say("SA Co-op: draugas suzeistas! Prieik ir spausk E");
  }
  if (!downed && !me.isInAnyCar() && dist2(me.getCoordinates(), r.ped.getCoordinates()) < 2.5 * 2.5) {
    if (!r.hinted) { say("Spausk E, kad atgaivintum drauga"); r.hinted = true; }
    if (pressed(KEY.E)) api.coop_revive(slot);
  }
}

// their gun in their copy's hands; shooting is shown as shots straight ahead (no hits sync yet)
function armRemote(r, s, now) {
  if (s.weapon !== r.weapon) {
    r.weapon = s.weapon;
    try {
      if (s.weapon > 0) {
        const model = Weapon.GetModel(s.weapon);
        if (model > 0) loadModel(model);
        r.ped.giveWeapon(s.weapon, 9999);
        if (model > 0) Streaming.MarkModelAsNoLongerNeeded(model);
      }
      r.ped.setCurrentWeapon(s.weapon);
    } catch (e) { log(`[sa-coop] weapon ${s.weapon}: ${e}`); }
  }
  if ((s.flags & 2) && s.inCar === 0 && s.weapon > 15 && now - (r.lastShot || 0) > 300) {
    const h = s.heading * Math.PI / 180;
    try { Task.ShootAtCoord(r.ped, s.x - Math.sin(h) * 25, s.y + Math.cos(h) * 25, s.z + 0.5, 350); } catch (e) {}
    r.lastShot = now;
  }
}

// notoriety is shared: if a friend has more stars, we get them too (never lowered here)
function shareWanted(s) {
  const theirs = (s.flags >> 8) & 0xff;
  try { if (theirs > player.storeWantedLevel()) player.alterWantedLevel(theirs); } catch (e) {}
}

// the lowest slot keeps time for everyone; a jump of more than 2 minutes is corrected
function followClock(minutes) {
  if (minutes < 0 || minutes >= 1440) return;
  const t = Clock.GetTimeOfDay(), ours = t.hours * 60 + t.minutes;
  let diff = Math.abs(ours - minutes); diff = Math.min(diff, 1440 - diff);
  if (diff > 2) Clock.SetTimeOfDay(Math.floor(minutes / 60), minutes % 60);
}

function syncRemotes(me, mySlot, now) {
  for (let slot = 0; slot < 4; slot++) {
    if (slot === mySlot) continue;
    if (!api.coop_get(slot, theirs)) {
      const r = remotes[slot];
      if (r && !r.lostAt) r.lostAt = now;
      if (r && now - r.lostAt > 5000) removeRemote(slot); // 5 s without them
      continue;
    }
    const s = readRemote();
    let r = remotes[slot];
    if (r && !Char.DoesExist(r.ped)) { remotes[slot] = null; r = null; }
    if (!r) r = remotes[slot] = spawnPed(slot, s);
    if (!r) continue;
    r.lostAt = 0;
    try {
      if (r.area !== s.interior) { r.area = s.interior; r.ped.setAreaVisible(s.interior); if (r.car) r.car.setAreaVisible(s.interior); }
      if (slot < mySlot || mySlot < 0) followClock(s.clock);
      shareWanted(s);
      if (s.health >= 0) armRemote(r, s, now);
      if (s.health < 0) { downRemote(r, s, slot, me); continue; }
      if (r.down) { r.down = false; try { r.ped.clearTasksImmediately(); } catch (e) {} }
      if (s.inCar === 1) driveRemote(r, s, now);
      else if (s.inCar === 2) rideRemote(r, s, mySlot, me);
      else walkRemote(r, s, now);
    } catch (e) {
      log(`[sa-coop] sync ${slot}: ${e}`);
    }
  }
}

// G: sit next to the nearest friend who drives
function tryRideAlong(me) {
  if (me.isInAnyCar() || downed) return;
  const p = me.getCoordinates();
  let best = null, bestD = 7 * 7;
  for (const r of remotes) {
    if (!r || !r.car || !Car.DoesExist(r.car)) continue;
    const d = dist2(p, r.car.getCoordinates());
    if (d < bestD) { best = r; bestD = d; }
  }
  if (best) Task.EnterCarAsPassenger(me, best.car, 10000, 0);
}

// ---- main loop ------------------------------------------------------------------------------------------
log("[sa-coop] loaded");
let started = false, announced = -1, lastCode = "", lastCodeCheck = 0;

while (true) {
  wait(0);
  if (!player.isPlaying()) continue;
  const now = Date.now();

  if (!started) {
    started = true;
    // a launcher session (IP mode) if there is one; otherwise the F9 window decides
    if (!api.coop_start() && api.coop_status() !== 2) say("SA Co-op: F9 - co-op langas (pavadinimas ir slaptazodis)");
  }
  if (now - lastCodeCheck > 1000) {
    lastCodeCheck = now;
    const code = readCode();
    if (code && code !== lastCode) { lastCode = code; say(`SA Co-op kodas (nukopijuotas): ${code}`); }
  }

  const status = api.coop_status();
  if (status !== announced) {
    announced = status;
    if (status === 2) say("SA Co-op: prisijungta");
    if (status === 3) say("SA Co-op: serveris neprieme (blogas kodas arba nera vietu)");
  }
  if (status !== 2) continue;

  const me = player.getChar();
  handleOwnHealth(me, now);
  writeMine(me);
  syncRemotes(me, api.coop_slot(), now);
  drawNames();
  if (pressed(KEY.G)) tryRideAlong(me);
}
