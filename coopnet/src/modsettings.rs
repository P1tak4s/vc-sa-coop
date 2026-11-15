//! The mod's own settings (the launcher's Settings card), changeable from the co-op menu's Settings tab too.
//! Stored in %LOCALAPPDATA%\<game>\settings.txt, the file the launcher writes. They change game files that are locked
//! while the game runs, so a change starts Win64\coop-helper\VC-SA-Coop.exe --apply-when-closed, which applies them
//! as soon as the game has closed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq)]
pub struct Opts {
    pub gfx: u8, // 0 original, 1 simple, 2 nice, 3 maximum
    pub grip: bool,
    pub fast: bool,
    pub reshade: bool,
    pub tough: bool, // armed enemies aim better and take more hits; works at once (the script asks coop_setting(2))
}

pub const GFX: [&str; 4] = ["Original", "Simple", "Nice", "Maximum"];

fn file() -> Option<std::path::PathBuf> {
    crate::game::data_dir().map(|d| d.join("settings.txt"))
}

fn read() -> Opts {
    let t = file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    let flag = |k: &str| t.lines().any(|l| l.trim() == format!("{k}=1"));
    let gfx = t.lines().find_map(|l| l.trim().strip_prefix("gfx=")?.parse::<u8>().ok()).unwrap_or(1).min(3);
    Opts { gfx, grip: flag("grip"), fast: flag("fast"), reshade: flag("reshade"), tough: flag("tough") }
}

fn state() -> &'static Mutex<Option<Opts>> {
    static S: Mutex<Option<Opts>> = Mutex::new(None);
    &S
}

pub fn get() -> Opts {
    *state().lock().unwrap().get_or_insert_with(read)
}

/// True once a setting was changed in this run of the game (the menu then says when it applies).
pub static CHANGED: AtomicBool = AtomicBool::new(false);

/// Save a change and make sure the helper will apply it after the game closes.
pub fn set(o: Opts) {
    let Some(f) = file() else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let before = state().lock().unwrap().replace(o);
    let text = format!("gfx={}\ngrip={}\nfast={}\nreshade={}\ntough={}\n", o.gfx, o.grip as u8, o.fast as u8, o.reshade as u8, o.tough as u8);
    if std::fs::write(&f, text).is_err() {
        return;
    }
    // only tougher enemies changed: nothing for the helper to do
    if before.is_some_and(|b| Opts { tough: o.tough, ..b } == o) {
        return;
    }
    if CHANGED.swap(true, Ordering::SeqCst) {
        return; // the helper is already waiting: it reads the file when the game closes
    }
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(r"coop-helper\VC-SA-Coop.exe")));
    let game = if crate::game::current().kind == crate::game::Kind::Vc { "vc" } else { "sa" };
    match exe.filter(|e| e.is_file()) {
        Some(exe) => match std::process::Command::new(&exe).args(["--apply-when-closed", game]).creation_flags(0x0800_0000).spawn() {
            Ok(_) => crate::log_line("settings: helper started, changes apply when the game closes"),
            Err(e) => crate::log_line(&format!("settings: helper did not start: {e}")),
        },
        None => crate::log_line("settings: no coop-helper in the game folder (run Install in VC-SA-Coop.exe once)"),
    }
}
