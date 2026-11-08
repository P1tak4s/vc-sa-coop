//! Which game loaded us: San Andreas or Vice City Definitive Edition (Steam build 1.0.113.49697469 of each).
//! Everything game-specific lives here: data folder, save folder, and the addresses the hooks use.
//! An address of 0 means "not known for this game": that hook stays off, the rest of the mod still runs.

use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Sa,
    Vc,
}

pub struct Game {
    pub kind: Kind,
    pub exe: &'static str,
    /// %LOCALAPPDATA%\<data> holds logs, the session file, save sets, fonts
    pub data: &'static str,
    /// Documents\Rockstar Games\<docs>\Profiles holds the game's saves
    pub docs: &'static str,
    pub title: &'static str,
    // ---- UE: menu hook (UObject::ProcessEvent, FNamePool) ----
    pub process_event: usize,
    pub process_event_prologue: &'static [u8],
    pub name_pool: usize,
    /// menu button classes and the ValueToBind of our co-op entry in the main and pause menus
    pub main_button: &'static str,
    pub main_coop: i32,
    pub pause_button: &'static str,
    pub pause_coop: i32,
    // ---- script engine: story hooks ----
    pub opcode_table: usize,
    pub opcode_groups: usize,
    /// first bytes of the first group handler, checked before the table is wrapped
    pub opcode_check: &'static [u8],
    pub can_start_mission: usize,
    pub script_space: usize,
    /// $PLAYER_ACTOR offset inside ScriptSpace
    pub player_actor: usize,
    pub s_ip: usize,
    pub s_cond: usize,
    pub s_logic: usize,
    pub s_not: usize,
    pub s_mission: usize,
}

pub const SA: Game = Game {
    kind: Kind::Sa,
    exe: "SanAndreas.exe",
    data: "sa-de-coop",
    docs: "GTA San Andreas Definitive Edition",
    title: "SA Co-op",
    process_event: 0x1C7C9C0,
    process_event_prologue: &[0x40, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC],
    name_pool: 0x570DDC0,
    main_button: "UI_Button_MainMenu_SA_C",
    main_coop: 4,
    pause_button: "UI_Button_Menu_SA_C",
    pause_coop: 7,
    opcode_table: 0x4307D00,
    opcode_groups: 27,
    opcode_check: &[],
    can_start_mission: 0x124E750,
    script_space: 0x51BEAE0,
    player_actor: 12,
    s_ip: 0x20,
    s_cond: 0x115,
    s_logic: 0x120,
    s_not: 0x122,
    s_mission: 0x12C,
};

/// Vice City DE, Steam build 1.0.113.49697469 (static RE, see re/ and docs/VICE-CITY-PLANAS.md).
pub const VC: Game = Game {
    kind: Kind::Vc,
    exe: "ViceCity.exe",
    data: "vc-de-coop",
    docs: "GTA Vice City Definitive Edition",
    title: "VC Co-op",
    // same UE 4.26 code as SA: unique 63-byte match of SA's ProcessEvent; the FNamePool passed to its constructor
    process_event: 0x1A87D40,
    process_event_prologue: &[0x40, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC],
    name_pool: 0x5441000,
    // GTAVC_MainMenu: start, game, options, social club, [co-op], exit; GTAVC_PauseMenu: ... social club, [co-op], exit
    main_button: "UI_Button_MainMenu_VC_C",
    main_coop: 4,
    pause_button: "UI_Button_Menu_VC_C",
    pause_coop: 7,
    opcode_table: 0,
    opcode_groups: 0,
    opcode_check: &[],
    can_start_mission: 0,
    script_space: 0,
    player_actor: 0,
    s_ip: 0,
    s_cond: 0,
    s_logic: 0,
    s_not: 0,
    s_mission: 0,
};

/// The game this process is, from the exe name; None in tools (tools/dll_test.py loads the DLL too).
pub fn detect() -> Option<&'static Game> {
    let exe = std::env::current_exe().ok()?;
    let name = exe.file_name()?.to_str()?;
    [&SA, &VC].into_iter().find(|g| name.eq_ignore_ascii_case(g.exe))
}

/// The game we run in, SA outside a game (tools).
pub fn current() -> &'static Game {
    static G: OnceLock<&'static Game> = OnceLock::new();
    G.get_or_init(|| detect().unwrap_or(&SA))
}

/// %LOCALAPPDATA%\sa-de-coop or \vc-de-coop
pub fn data_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(current().data))
}

/// Documents\Rockstar Games\<game>\Profiles; Documents may be redirected to OneDrive (VC's was, 11-08).
pub fn profiles_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE")?);
    let rel = PathBuf::from("Rockstar Games").join(current().docs).join("Profiles");
    let found = ["Documents", r"OneDrive\Documents"].iter().map(|d| home.join(d).join(&rel)).find(|p| p.is_dir());
    Some(found.unwrap_or_else(|| home.join("Documents").join(rel)))
}
