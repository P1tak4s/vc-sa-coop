/// <reference path=".config/sa.d.ts" />
// SA Co-op (M1): your friend appears in your world, walks, runs and drives. Talks to coopnet64.dll (next to
// SanAndreas.exe), which the launcher configured with a session. Each world stays its own: missions come later.

const lib = DynamicLibrary.Load("coopnet64.dll");
if (!lib) exit("coopnet64.dll not found");
const fn = (name) => Fn.X64(lib.getProcedure(name));
const coopStart = fn("coop_start"), coopStatus = fn("coop_status"), coopSlot = fn("coop_slot");
const coopSend = fn("coop_send"), coopGet = fn("coop_get");

// proto::State, 64 bytes
const SIZE = 64;
const F = { x: 0, y: 4, z: 8, heading: 12, vx: 16, vy: 20, vz: 24, qx: 28, qy: 32, qz: 36, qw: 40, carSpeed: 44 };
const I = { inCar: 48, carModel: 52, health: 56, carHealth: 60 };
const mine = Memory.Allocate(SIZE);
const theirs = Memory.Allocate(SIZE);
const SKINS = [105, 106, 107, 105]; // Grove Street looks until we sync clothes

const player = new Player(0);
const remotes = [null, null, null, null]; // { ped, car, carModel, blip, lastTask, lost }

function readRemote() {
  const s = {};
  for (const k in F) s[k] = Memory.ReadFloat(theirs + F[k], false, false);
  for (const k in I) s[k] = Memory.ReadI32(theirs + I[k], false, false);
  return s;
}

function writeMine(me) {
  const p = me.getCoordinates();
  let v = { x: 0, y: 0, z: 0 }, q = { x: 0, y: 0, z: 0, w: 1 }, carSpeed = 0, inCar = 0, carModel = 0, carHealth = 0;
  if (me.isInAnyCar()) {
    const car = me.storeCarIsInNoSave();
    const c = car.getCoordinates();
    p.x = c.x; p.y = c.y; p.z = c.z;
    q = car.getQuaternion();
    carSpeed = car.getSpeed();
    inCar = 1;
    carModel = car.getModel();
    carHealth = car.getHealth();
  } else {
    v = me.getVelocity();
  }
  const f = { x: p.x, y: p.y, z: p.z, heading: me.getHeading(), vx: v.x, vy: v.y, vz: v.z, qx: q.x, qy: q.y, qz: q.z, qw: q.w, carSpeed };
  for (const k in F) Memory.WriteFloat(mine + F[k], f[k], false, false);
  Memory.WriteI32(mine + I.inCar, inCar, false, false);
  Memory.WriteI32(mine + I.carModel, carModel, false, false);
  Memory.WriteI32(mine + I.health, me.getHealth(), false, false);
  Memory.WriteI32(mine + I.carHealth, carHealth, false, false);
  coopSend(mine);
}

function loadModel(id) {
  Streaming.RequestModel(id);
  Streaming.LoadAllModelsNow();
  for (let i = 0; i < 40 && !Streaming.HasModelLoaded(id); i++) wait(25);
  return Streaming.HasModelLoaded(id);
}

function spawnPed(slot, s) {
  const model = SKINS[slot];
  if (!loadModel(model)) return null;
  const ped = Char.Create(4, model, s.x, s.y, s.z);
  Streaming.MarkModelAsNoLongerNeeded(model);
  ped.setProofs(true, true, true, true, true); // no traffic or cops kill the copy of your friend
  ped.setCantBeDraggedOut(true);
  ped.setKeepTask(true);
  const blip = Blip.AddForChar(ped);
  try { blip.setAsFriendly(true); } catch (e) {}
  log(`[sa-coop] friend ${slot} appeared`);
  return { ped, car: null, carModel: 0, blip, lastTask: 0, lost: 0, lastPos: null, lastT: 0 };
}

function dropCar(r) {
  if (!r.car) return;
  try {
    if (r.ped.isInAnyCar()) r.ped.warpFromCarToCoord(r.ped.getCoordinates().x, r.ped.getCoordinates().y, r.ped.getCoordinates().z);
    r.car.markAsNoLongerNeeded();
  } catch (e) {}
  r.car = null;
  r.carModel = 0;
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

function driveRemote(r, s) {
  if (!r.car || r.carModel !== s.carModel) {
    dropCar(r);
    if (!loadModel(s.carModel)) return;
    r.car = Car.Create(s.carModel, s.x, s.y, s.z);
    Streaming.MarkModelAsNoLongerNeeded(s.carModel);
    r.car.setProofs(true, true, true, true, true);
    r.carModel = s.carModel;
    r.ped.warpIntoCar(r.car);
  }
  r.car.setCoordinatesNoOffset(s.x, s.y, s.z);
  r.car.setQuaternion(s.qx, s.qy, s.qz, s.qw);
  r.car.setForwardSpeed(s.carSpeed);
}

function walkRemote(r, s, now) {
  if (r.car) dropCar(r);
  // speed from the last two positions we got (m/s): no guessing about the game's velocity units
  // the same packet can be read twice (both run at 20 Hz): keep the last speed until the position moves
  if (!r.lastPos) { r.lastPos = { x: s.x, y: s.y }; r.lastT = now; r.speed = 0; r.ux = 0; r.uy = 0; }
  const mx = s.x - r.lastPos.x, my = s.y - r.lastPos.y, m = Math.sqrt(mx * mx + my * my);
  if (m > 0.0001 && now > r.lastT) {
    r.speed = m / ((now - r.lastT) / 1000); r.ux = mx / m; r.uy = my / m;
    r.lastPos = { x: s.x, y: s.y }; r.lastT = now;
  } else if (now - r.lastT > 300) {
    r.speed = 0; // no movement for a while: standing
  }
  const speed = r.speed, ux = r.ux, uy = r.uy;

  const p = r.ped.getCoordinates();
  const dx = s.x - p.x, dy = s.y - p.y, dz = s.z - p.z;
  const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
  if (dist > 5) {
    r.ped.setCoordinatesNoOffset(s.x, s.y, s.z); // fell behind: jump to where they are
    r.ped.setHeading(s.heading);
  } else if (speed > 0.3 || dist > 1) {
    if (now - r.lastTask > 250) {
      const moveState = speed > 6 ? 7 : speed > 2.5 ? 6 : 4; // sprint, run, walk
      // aim a little ahead so the walk does not stop between packets
      Task.GoStraightToCoord(r.ped, s.x + ux * 1.5, s.y + uy * 1.5, s.z, moveState, 1000);
      r.lastTask = now;
    }
  } else {
    r.ped.setHeading(s.heading);
  }
}

log("[sa-coop] loaded");
let announced = 0;
let started = false;

while (true) {
  wait(50);
  if (!player.isPlaying()) continue;
  if (!started) {
    started = true;
    if (!coopStart()) {
      showTextBox("SA Co-op: paleisk zaidima per SA Co-op langa");
      exit("no session");
    }
  }

  const status = coopStatus();
  if (status !== announced) {
    announced = status;
    if (status === 2) showTextBox("SA Co-op: prisijungta");
    if (status === 3) showTextBox("SA Co-op: serveris nepriemė (kodas arba vietų nėra)");
  }
  if (status !== 2) continue;

  const me = player.getChar();
  writeMine(me);
  const mySlot = coopSlot();
  const now = Date.now();

  for (let slot = 0; slot < 4; slot++) {
    if (slot === mySlot) continue;
    if (!coopGet(slot, theirs)) {
      const r = remotes[slot];
      if (r && ++r.lost > 100) removeRemote(slot); // 5 s without them
      continue;
    }
    const s = readRemote();
    let r = remotes[slot];
    if (r && !Char.DoesExist(r.ped)) { remotes[slot] = null; r = null; }
    if (!r) r = remotes[slot] = spawnPed(slot, s);
    if (!r) continue;
    r.lost = 0;
    try {
      if (s.inCar) driveRemote(r, s);
      else walkRemote(r, s, now);
    } catch (e) {
      log(`[sa-coop] sync ${slot}: ${e}`);
    }
  }
}
