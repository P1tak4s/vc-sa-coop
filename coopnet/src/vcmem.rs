//! Vice City DE: what the script cannot do with VC's commands (Steam build 1.0.113.49697469, static RE in re/).
//! WARP_CHAR_INTO_CAR_AS_PASSENGER does not exist in VC, so the game's own WARP_CHAR_INTO_CAR path is used with the
//! "enter as passenger" objective: CPed::SetObjective(ped, 0x11, veh) then CPed::WarpPedIntoCar(ped, veh), which
//! takes the first free passenger seat, removes and re-adds the ped in the world and plays the in-car anims.

const PED_POOL: usize = 0x545DE38; // CPool<CPed>*
const VEH_POOL: usize = 0x545DE30; // CPool<CVehicle>*
const PED_SIZE: usize = 0xB78;
const VEH_SIZE: usize = 0x7B8;
const SET_OBJECTIVE: usize = 0x111BBD0; // void CPed::SetObjective(CPed*, int, void*)
const WARP_INTO_CAR: usize = 0x1119070; // void CPed::WarpPedIntoCar(CPed*, CVehicle*)
const SET_OBJECTIVE_BYTES: [u8; 10] = [0x48, 0x89, 0x5C, 0x24, 0x18, 0x48, 0x89, 0x6C, 0x24, 0x20];
const WARP_BYTES: [u8; 10] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x6C, 0x24, 0x10];
const OBJ_ENTER_CAR_AS_PASSENGER: i32 = 0x11;
// CPed / CVehicle fields
const PED_IN_VEHICLE: usize = 0x558; // bool
const VEH_PASSENGERS: usize = 0x2A0; // CPed*[4]
const VEH_NUM_PASSENGERS: usize = 0x2E0; // u8
const VEH_MAX_PASSENGERS: usize = 0x2E4; // u8

fn base() -> usize {
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as usize }
}

/// Script handle -> object: index = handle >> 8, the slot's flag byte must equal the handle's low byte.
unsafe fn from_pool(pool_at: usize, size: usize, handle: i64) -> Option<*mut u8> {
    let pool = *((base() + pool_at) as *const *const u8);
    if pool.is_null() || handle <= 0 {
        return None;
    }
    let objects = *(pool as *const usize);
    let flags = *(pool.add(8) as *const *const u8);
    let count = *(pool.add(0x10) as *const i32);
    let idx = (handle >> 8) as usize;
    if objects == 0 || flags.is_null() || idx >= count.max(0) as usize || *flags.add(idx) != (handle & 0xFF) as u8 {
        return None;
    }
    Some((objects + idx * size) as *mut u8)
}

/// Put a ped (script handle) straight into a passenger seat of a car (script handle). 1 = done, 0 = not possible
/// (not Vice City, unknown build, ped already in a car, no free seat).
#[no_mangle]
pub extern "C" fn coop_warp_passenger(ped: i64, car: i64) -> i64 {
    if crate::game::current().kind != crate::game::Kind::Vc {
        return 0;
    }
    unsafe {
        let b = base();
        if std::slice::from_raw_parts((b + SET_OBJECTIVE) as *const u8, 10) != SET_OBJECTIVE_BYTES
            || std::slice::from_raw_parts((b + WARP_INTO_CAR) as *const u8, 10) != WARP_BYTES
        {
            crate::log_line("warp passenger: unknown game build");
            return 0;
        }
        let (Some(p), Some(v)) = (from_pool(PED_POOL, PED_SIZE, ped), from_pool(VEH_POOL, VEH_SIZE, car)) else {
            return 0;
        };
        if *p.add(PED_IN_VEHICLE) != 0 {
            return 0;
        }
        // the game takes the first empty slot of four and ignores the car's maximum: that slot must be a real seat
        let max = (*v.add(VEH_MAX_PASSENGERS)).min(4) as usize;
        let first_free = (0..4).find(|i| (*(v.add(VEH_PASSENGERS + i * 8) as *const usize)) == 0);
        match first_free {
            Some(i) if i < max => {}
            _ => return 0,
        }
        let set_objective: unsafe extern "C" fn(*mut u8, i32, *mut u8) = std::mem::transmute(b + SET_OBJECTIVE);
        let warp: unsafe extern "C" fn(*mut u8, *mut u8) = std::mem::transmute(b + WARP_INTO_CAR);
        set_objective(p, OBJ_ENTER_CAR_AS_PASSENGER, v);
        warp(p, v);
        if *p.add(PED_IN_VEHICLE) == 0 {
            crate::log_line("warp passenger: the game did not seat the ped");
            return 0;
        }
        // WarpPedIntoCar does not count the passenger: leaving would take the count below zero
        *v.add(VEH_NUM_PASSENGERS) = (*v.add(VEH_NUM_PASSENGERS)).saturating_add(1);
        1
    }
}

// ---- what the script created (peds, cars, blips), kept in the DLL: a reloaded script deletes the previous copies ----
// Entries are kind << 32 | handle (kind 1 ped, 2 car, 3 blip). Deleted ones are forgotten at once, so a handle the
// game hands out again later is never touched.
static TRACKED: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());

#[no_mangle]
pub extern "C" fn coop_track(kind: i64, handle: i64) -> i64 {
    let code = kind << 32 | (handle & 0xFFFF_FFFF);
    let mut t = TRACKED.lock().unwrap();
    if t.len() < 512 && !t.contains(&code) {
        t.push(code);
    }
    1
}

#[no_mangle]
pub extern "C" fn coop_forget(kind: i64, handle: i64) -> i64 {
    let code = kind << 32 | (handle & 0xFFFF_FFFF);
    TRACKED.lock().unwrap().retain(|c| *c != code);
    1
}

/// Hand over everything tracked (at most `max` into `out`) and clear the list. Returns how many.
#[no_mangle]
pub unsafe extern "C" fn coop_take_tracked(out: *mut i64, max: i64) -> i64 {
    if out.is_null() || max <= 0 {
        return 0;
    }
    let mut t = TRACKED.lock().unwrap();
    let n = t.len().min(max as usize);
    for (i, c) in t.drain(..n).enumerate() {
        *out.add(i) = c;
    }
    t.clear();
    n as i64
}
