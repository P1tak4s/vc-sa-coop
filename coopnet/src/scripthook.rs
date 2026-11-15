//! The host's story with a partner (the SA:COOP model, ported to the Definitive Edition, build 1.0.113.49697469):
//! - every script command goes through the opcode handler table (0x4307D00, 27 group handlers); we wrap them so
//!   that, for chosen checks, a false answer is asked again with $PLAYER_ACTOR set to the partner's copy
//!   ("either player": in a car, on a train, shooting, damaged by...);
//! - CPlayerPed::CanPlayerStartMission (0x124E750): the guest never starts a mission of its own, the host only
//!   when the partner stands with it in the marker.
//! The script tells us the partner's ped handle and whether it is near (coop_set_partner).
//! Vice City DE has no handler table: its 15 group handlers are hooked one by one (MinHook) with the same wrapper,
//! and the mission start rule goes through CAN_PLAYER_START_MISSION (03EE), which VC's main.scm asks.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

// Addresses and the CRunningScript (x64) layout per game: crate::game (SA values documented above).
const MAX_GROUPS: usize = 32;

/// Checks a partner can answer for the host ("either player"). From SA:COOP's generic rules plus vehicle checks.
const EITHER: &[u32] = &[
    0x00DB, // IS_CHAR_IN_CAR
    0x00DD, // IS_CHAR_IN_MODEL
    0x00DF, // IS_CHAR_IN_ANY_CAR
    0x0449, // IS_CHAR_IN_ANY_TRAIN? (classic: IS_CHAR_IN_ANY_POLICE_VEHICLE family varies; harmless if false)
    0x02D6, // IS_CHAR_SHOOTING_IN_AREA
    0x02E0, // IS_CHAR_SHOOTING
    0x051A, // HAS_CHAR_BEEN_DAMAGED_BY_CHAR
    0x051C, // HAS_CAR_BEEN_DAMAGED_BY_CHAR
    0x0457, // IS_PLAYER_TARGETTING_CHAR
    0x068C, // IS_PLAYER_TARGETTING_ANYTHING
];

pub static PARTNER_PED: AtomicI32 = AtomicI32::new(-1); // script handle of the partner's copy in our world
pub static PARTNER_NEAR: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

type Handler = unsafe extern "C" fn(*mut u8, u32) -> u8;
type CanStart = unsafe extern "C" fn(*mut u8) -> u8;
static mut ORIG: [usize; MAX_GROUPS] = [0; MAX_GROUPS];
static mut GROUPS: usize = 0;
static mut ORIG_CAN_START: Option<CanStart> = None;
static mut BASE: usize = 0;
static mut UPDATE_COMPARE: usize = 0;

fn connected() -> bool {
    crate::status_code() == 2
}

unsafe fn get_ip(s: *mut u8, at: usize, wide: bool) -> usize {
    if wide { *(s.add(at) as *const usize) } else { *(s.add(at) as *const u32) as usize }
}

unsafe fn set_ip(s: *mut u8, at: usize, wide: bool, v: usize) {
    if wide { *(s.add(at) as *mut usize) = v } else { *(s.add(at) as *mut u32) = v as u32 }
}

/// The game should not start a mission here: the guest plays the host's story, the host waits for its partner.
/// 11-15: the guest may start its own missions again. Refusing them left a friend on a new game stuck: VC has no save
/// house before the first mission, so he could neither save nor play.
fn refuse_mission_start() -> bool {
    connected() && crate::is_host() && PARTNER_PED.load(Ordering::Relaxed) > 0 && !PARTNER_NEAR.load(Ordering::Relaxed)
}

unsafe extern "C" fn wrap(s: *mut u8, op: u32) -> u8 {
    let g = crate::game::current();
    let (s_ip, s_cond, s_logic, s_not, s_mission) = (g.s_ip, g.s_cond, g.s_logic, g.s_not, g.s_mission);
    let wide = g.s_ip_wide;
    let f: Handler = std::mem::transmute(ORIG[(op / 100) as usize % GROUPS]);
    let ip = get_ip(s, s_ip, wide);
    let cond = *s.add(s_cond);
    let logic = *(s.add(s_logic) as *const u16);
    let not = *s.add(s_not);
    let r = f(s, op);
    if op == g.start_mission_op && op != 0 && UPDATE_COMPARE != 0 && refuse_mission_start() {
        // undo the game's answer, then give "no" the way the game would (it applies NOT and AND/OR itself)
        *s.add(s_cond) = cond;
        *(s.add(s_logic) as *mut u16) = logic;
        let update: unsafe extern "C" fn(*mut u8, u8) = std::mem::transmute(UPDATE_COMPARE);
        update(s, 0);
        return r;
    }
    // ask again for the partner: host only, mission scripts only, plain (not NOT-ed) checks that said no
    let partner = PARTNER_PED.load(Ordering::Relaxed);
    if partner > 0
        && not == 0
        && *s.add(s_mission) != 0
        && *s.add(s_cond) == 0
        && EITHER.contains(&op)
        && connected()
        && crate::is_host()
    {
        let actor = (BASE + g.script_space + g.player_actor) as *mut i32;
        let end_ip = get_ip(s, s_ip, wide);
        let saved_actor = *actor;
        set_ip(s, s_ip, wide, ip);
        *s.add(s_cond) = cond;
        *(s.add(s_logic) as *mut u16) = logic;
        *actor = partner;
        let r2 = f(s, op);
        *actor = saved_actor;
        // same command, same parameters: it must end where the first run ended
        if get_ip(s, s_ip, wide) != end_ip {
            set_ip(s, s_ip, wide, end_ip);
        }
        return r2;
    }
    r
}

unsafe extern "C" fn can_start(ped: *mut u8) -> u8 {
    if connected() {
        if crate::is_host() && PARTNER_PED.load(Ordering::Relaxed) > 0 && !PARTNER_NEAR.load(Ordering::Relaxed) {
            return 0; // the host waits in the marker until the partner is there too
        }
    }
    match ORIG_CAN_START {
        Some(f) => f(ped),
        None => 1,
    }
}

/// Install both hooks once (game process only; checks the bytes of the build we know).
pub unsafe fn install() {
    use hudhook::mh::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_STATUS};
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    BASE = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as usize;
    let g = crate::game::current();
    if !g.opcode_handlers.is_empty() {
        install_handlers(g);
        return;
    }
    if g.opcode_table == 0 || g.script_space == 0 || g.opcode_groups == 0 || g.opcode_groups > MAX_GROUPS {
        crate::log_line("story hooks: addresses unknown for this game, off");
        return;
    }
    let groups = g.opcode_groups;
    let cs = (BASE + g.can_start_mission) as *const u8;
    if g.can_start_mission != 0 && std::slice::from_raw_parts(cs, 6) != [0x40, 0x53, 0x48, 0x83, 0xEC, 0x20] {
        crate::log_line("story hooks: unknown game build, off");
        return;
    }
    let table = (BASE + g.opcode_table) as *mut usize;
    // every entry must point into the exe, and the first handler must start with the bytes we know
    let image = BASE..BASE + 0x1000_0000;
    if (0..groups).any(|i| !image.contains(&*table.add(i)))
        || (!g.opcode_check.is_empty() && std::slice::from_raw_parts(*table as *const u8, g.opcode_check.len()) != g.opcode_check)
    {
        crate::log_line("story hooks: opcode table does not match this build, off");
        return;
    }
    let mut old = 0u32;
    if VirtualProtect(table as *const _, groups * 8, PAGE_READWRITE, &mut old) == 0 {
        crate::log_line("story hooks: cannot unprotect the opcode table");
        return;
    }
    for i in 0..groups {
        ORIG[i] = *table.add(i);
        *table.add(i) = wrap as *const () as usize;
    }
    GROUPS = groups;
    VirtualProtect(table as *const _, groups * 8, old, &mut old);
    if g.can_start_mission == 0 {
        crate::log_line("story hooks: opcode table wrapped (no mission start hook in this game)");
        return;
    }
    let _ = MH_Initialize();
    let mut orig: *mut std::ffi::c_void = std::ptr::null_mut();
    let st = MH_CreateHook(cs as *mut _, can_start as *mut _, &mut orig);
    if matches!(st, MH_STATUS::MH_OK) {
        ORIG_CAN_START = Some(std::mem::transmute::<*mut std::ffi::c_void, CanStart>(orig));
        let e = MH_EnableHook(cs as *mut _);
        crate::log_line(&format!("story hooks: opcode table wrapped, mission start {e:?}"));
    } else {
        crate::log_line(&format!("story hooks: mission start hook failed {st:?}"));
    }
}

/// Vice City: hook each group handler (checked by its first bytes) with `wrap`; all or nothing.
unsafe fn install_handlers(g: &crate::game::Game) {
    use hudhook::mh::{MH_ApplyQueued, MH_CreateHook, MH_Initialize, MH_QueueEnableHook, MH_STATUS};
    let handlers = g.opcode_handlers;
    if handlers.len() > MAX_GROUPS || g.script_space == 0 {
        crate::log_line("story hooks: bad handler list, off");
        return;
    }
    for (rva, bytes) in handlers {
        if std::slice::from_raw_parts((BASE + rva) as *const u8, bytes.len()) != bytes {
            crate::log_line(&format!("story hooks: handler {rva:#x} does not match this build, off"));
            return;
        }
    }
    let _ = MH_Initialize();
    let mut made = vec![];
    for (i, (rva, _)) in handlers.iter().enumerate() {
        let target = (BASE + rva) as *mut std::ffi::c_void;
        let mut orig: *mut std::ffi::c_void = std::ptr::null_mut();
        let st = MH_CreateHook(target, wrap as *mut _, &mut orig);
        if !matches!(st, MH_STATUS::MH_OK) {
            // the ones made so far are never enabled: the game runs unhooked
            crate::log_line(&format!("story hooks: hook {rva:#x} failed {st:?}, off"));
            return;
        }
        ORIG[i] = orig as usize;
        made.push(target);
    }
    GROUPS = handlers.len();
    let uc = BASE + g.update_compare_flag;
    if g.update_compare_flag != 0 && std::slice::from_raw_parts(uc as *const u8, g.update_compare_check.len()) == g.update_compare_check {
        UPDATE_COMPARE = uc;
    } else {
        crate::log_line("story hooks: UpdateCompareFlag unknown, mission start rule off");
    }
    // all at once: the game's threads are held while the jumps are written
    for t in &made {
        let _ = MH_QueueEnableHook(*t);
    }
    let st = MH_ApplyQueued();
    crate::log_line(&format!("story hooks: {} group handlers {st:?}, mission start rule {}", made.len(), UPDATE_COMPARE != 0));
}
