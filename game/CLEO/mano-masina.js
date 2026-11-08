/// <reference path=".config/sa.d.ts" />
// The last car you drove stays in the world and gets a map blip while you are out of it.
// Taking another car moves the mark to it; a wrecked car is let go. Not kept across a save/load.

const player = new Player(0);
let mine = null;   // the car we keep
let blip = null;   // its blip while we are on foot

function exists(car) {
  try { return car && Car.DoesExist(car) && car.getHealth() > 0; } catch (e) { return false; }
}

function dropBlip() {
  if (blip) { try { blip.remove(); } catch (e) {} blip = null; }
}

function letGo() {
  dropBlip();
  if (mine) { try { if (Car.DoesExist(mine)) mine.markAsNoLongerNeeded(); } catch (e) {} mine = null; }
}

log("[mano-masina] ready");

while (true) {
  wait(250);
  let me;
  try {
    if (!player.isPlaying()) continue;
    me = player.getChar();
  } catch (e) { continue; }

  if (me.isInAnyCar()) {
    // storeCarIsIn returns -1 on the Definitive Edition; take the car and make it a mission car ourselves
    const car = me.storeCarIsInNoSave();
    // during a mission the car may belong to the mission: releasing it later could delete it and break the mission
    if (+car > 0 && !ONMISSION && (!mine || +car !== +mine)) {
      letGo();
      mine = car;
      try { mine.setAsMissionCar(); } catch (e) { log(`[mano-masina] setAsMissionCar failed: ${e}`); }
      log(`[mano-masina] keeping car ${+car}`);
    }
    dropBlip(); // no blip while driving
    continue;
  }

  if (mine && !exists(mine)) {
    log("[mano-masina] car wrecked or gone, letting go");
    letGo();
    continue;
  }
  if (mine && !blip) {
    try { blip = Blip.AddForCar(mine); } catch (e) { log(`[mano-masina] blip failed: ${e}`); }
  }
}
