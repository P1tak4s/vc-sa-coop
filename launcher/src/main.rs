//! sa-coop.exe: install the mod into GTA San Andreas or Vice City The Definitive Edition, host a game (the server runs here)
//! or join one with a code, then start the game through Steam. Like Saints Row 2: everything before the game starts.
#![windows_subsystem = "windows"]

mod icon;
mod ui;

use eframe::egui;
use proto::*;
use relay::{lan_ip, random_key, run_server, upnp, Players};
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// What differs between the two games for the launcher.
struct GameInfo {
    name: &'static str,
    steam: &'static str,
    subdir: &'static str,
    exe: &'static str,
    /// %LOCALAPPDATA%\<data>: session file, fonts (the DLL reads the same folder, see coopnet/src/game.rs)
    data: &'static str,
    /// Documents\Rockstar Games\<docs>
    docs: &'static str,
    menu_rel: &'static str,
    menus: [&'static str; 2],
    /// the game's own co-op script
    script: &'static str,
}

const GAMES: [GameInfo; 2] = [
    GameInfo {
        name: "GTA San Andreas – The Definitive Edition",
        steam: "steam://rungameid/1547000",
        subdir: r"steamapps\common\GTA San Andreas - The Definitive Edition\Gameface\Binaries\Win64",
        exe: "SanAndreas.exe",
        data: "sa-de-coop",
        docs: "GTA San Andreas Definitive Edition",
        menu_rel: "Gameface/Content/SanAndreas/UI/MenuTemplates",
        menus: ["GTASA_MainMenu", "GTASA_PauseMenu"],
        script: r"CLEO\sa-coop[dll][mem].js",
    },
    GameInfo {
        name: "GTA Vice City – The Definitive Edition",
        steam: "steam://rungameid/1546990",
        subdir: r"steamapps\common\GTA Vice City - The Definitive Edition\Gameface\Binaries\Win64",
        exe: "ViceCity.exe",
        data: "vc-de-coop",
        docs: "GTA Vice City Definitive Edition",
        menu_rel: "Gameface/Content/ViceCity/UI/MenuTemplates",
        menus: ["GTAVC_MainMenu", "GTAVC_PauseMenu"],
        script: r"CLEO\vc-coop[dll][mem].js",
    },
];

/// Graphics levels: Engine.ini files written to Documents\Rockstar Games\<game>\Config\WindowsNoEditor.
/// Only a file of ours (its first line says "Co-op graphics") is ever replaced or removed.
const GRAPHICS: [(&str, &str); 4] = [
    ("Original", ""),
    ("Simple", include_str!("../../game-config/Engine.ini")),
    ("Nice", include_str!("../../game-config/Engine-grazu.ini")),
    ("Maximum", include_str!("../../game-config/Engine-maksimum.ini")),
];

fn engine_ini(g: &GameInfo) -> Option<PathBuf> {
    docs_dir(g).map(|d| d.join(r"Config\WindowsNoEditor\Engine.ini"))
}

/// Which level is in place: Some(index) for ours, None for the player's own file.
fn graphics_level(g: &GameInfo) -> Option<usize> {
    let Some(ini) = engine_ini(g) else { return Some(0) };
    let Ok(text) = std::fs::read_to_string(&ini) else { return Some(0) };
    if !text.lines().next().is_some_and(|l| l.contains("Co-op graphics")) {
        return None;
    }
    GRAPHICS.iter().position(|(_, t)| !t.is_empty() && t.replace("\r\n", "\n").trim() == text.replace("\r\n", "\n").trim()).or(Some(1))
}

/// The DE's own override file (CustomSettings.ini, [CustomSettings]): our lines live between two marker lines, the
/// rest of the file is the player's and stays as it is.
const CUSTOM: [&str; 4] = [
    "",
    "gta.worldcomp.distmult=3.0",
    "gta.worldcomp.distmult=4.0\nr.DefaultFeature.AmbientOcclusion=1\ngta.reflections.size_planar=2048",
    "gta.worldcomp.distmult=8.0\nr.DefaultFeature.AmbientOcclusion=1\ngta.reflections.size_planar=4096",
];
const CUSTOM_BEGIN: &str = "; >>> Co-op graphics (sa-coop.exe changes only these lines)";
const CUSTOM_END: &str = "; <<< Co-op graphics";

fn set_custom_settings(g: &GameInfo, level: usize) -> Result<(), String> {
    let Some(path) = engine_ini(g).map(|p| p.with_file_name("CustomSettings.ini")) else { return Ok(()) };
    let old = std::fs::read_to_string(&path).unwrap_or_else(|_| "[CustomSettings]\n".into()).replace("\r\n", "\n");
    // drop our previous block
    let mut kept = vec![];
    let mut inside = false;
    for line in old.lines() {
        if line == CUSTOM_BEGIN { inside = true; continue; }
        if line == CUSTOM_END { inside = false; continue; }
        if !inside { kept.push(line.to_string()); }
    }
    let lines = CUSTOM[level.min(CUSTOM.len() - 1)];
    if !lines.is_empty() {
        if !kept.iter().any(|l| l.trim() == "[CustomSettings]") {
            kept.push("[CustomSettings]".into());
        }
        let at = kept.iter().position(|l| l.trim() == "[CustomSettings]").unwrap() + 1;
        let block: Vec<String> = std::iter::once(CUSTOM_BEGIN.to_string()).chain(lines.lines().map(String::from)).chain(std::iter::once(CUSTOM_END.to_string())).collect();
        kept.splice(at..at, block);
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, kept.join("\r\n") + "\r\n").map_err(|e| e.to_string())
}

fn set_graphics(g: &GameInfo, level: usize) -> Result<String, String> {
    let ini = engine_ini(g).ok_or("Documents folder not found")?;
    if graphics_level(g).is_none() {
        return Err(format!("{} is your own: left as is (delete it to use ours)", ini.display()));
    }
    let (name, text) = GRAPHICS[level.min(GRAPHICS.len() - 1)];
    if is_vc(g) {
        set_custom_settings(g, level)?;
    }
    if text.is_empty() {
        let _ = std::fs::remove_file(&ini);
    } else {
        std::fs::create_dir_all(ini.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&ini, text).map_err(|e| e.to_string())?;
    }
    Ok(format!("Graphics: {name} (applies next time the game starts)"))
}

fn is_vc(g: &GameInfo) -> bool {
    g.exe == "ViceCity.exe"
}

/// CLEO runs every script in its folder: Vice City gets only the vc-*.js scripts, San Andreas all the others.
fn belongs(g: &GameInfo, file: &str) -> bool {
    !file.ends_with(".js") || file.starts_with("vc-") == is_vc(g)
}

fn data_dir(g: &GameInfo) -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(g.data)
}

/// Documents may be redirected to OneDrive (VC's folder was, 11-08)
fn docs_dir(g: &GameInfo) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE")?);
    let rel = PathBuf::from("Rockstar Games").join(g.docs);
    let found = ["Documents", r"OneDrive\Documents"].iter().map(|d| home.join(d).join(&rel)).find(|p| p.is_dir());
    Some(found.unwrap_or_else(|| home.join("Documents").join(rel)))
}

fn main() -> eframe::Result {
    // sa-coop.exe --install sa|vc: install without the window, the result goes to %LOCALAPPDATA%\<game>-de-coop\install.log
    let args: Vec<String> = std::env::args().collect();
    // VC-SA-Coop.exe --write-icon <file.png>: the app icon as a 256 px PNG (for the README)
    if let Some(i) = args.iter().position(|a| a == "--write-icon") {
        let px = icon::draw(256);
        if let (Some(path), Some(img)) = (args.get(i + 1), image::RgbaImage::from_raw(256, 256, px)) {
            let _ = img.save(path);
        }
        return Ok(());
    }
    // sa-coop.exe --valdymas vc 0|1 (original / better car handling)
    if let Some(i) = args.iter().position(|a| a == "--valdymas") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let better = args.get(i + 2).is_none_or(|v| v != "0");
        let line = match find_game(g) {
            Some(dir) => set_handling(g, &dir, better).unwrap_or_else(|e| e),
            None => format!("{} not found", g.name),
        };
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("valdymas.log"), line);
        return Ok(());
    }
    // sa-coop.exe --uninstall sa|vc
    if let Some(i) = args.iter().position(|a| a == "--uninstall") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let lines = match find_game(g) {
            Some(dir) => uninstall(g, &dir),
            None => vec![format!("{} not found", g.name)],
        };
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("uninstall.log"), lines.join("\n"));
        return Ok(());
    }
    // sa-coop.exe --reshade sa|vc 0|1
    if let Some(i) = args.iter().position(|a| a == "--reshade") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let on = args.get(i + 2).is_none_or(|v| v != "0");
        let line = match find_game(g) {
            Some(dir) => set_reshade(&dir, on).unwrap_or_else(|e| e),
            None => format!("{} not found", g.name),
        };
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("reshade.log"), line);
        return Ok(());
    }
    // sa-coop.exe --grafika sa|vc 0..3 (Original, Simple, Nice, Maximum)
    if let Some(i) = args.iter().position(|a| a == "--grafika") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let level = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(1);
        let line = set_graphics(g, level).unwrap_or_else(|e| e);
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("grafika.log"), line);
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--install") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let lines = match find_game(g) {
            Some(dir) => install(g, &dir).unwrap_or_else(|e| vec![e]),
            None => vec![format!("{} not found in Steam libraries", g.name)],
        };
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("install.log"), lines.join("
"));
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VC / SA Co-op")
            .with_inner_size([600.0, 780.0])
            .with_min_inner_size([520.0, 560.0])
            .with_icon(Arc::new(ui::icon())),
        ..Default::default()
    };
    eframe::run_native("VC / SA Co-op", options, Box::new(|cc| Ok(Box::new(ui::App::new(cc)))))
}

// ---- game folder and install ---------------------------------------------------------------------------

/// Every Steam library from libraryfolders.vdf, checked for the game's exe.
fn find_game(g: &GameInfo) -> Option<PathBuf> {
    let steam = PathBuf::from(r"C:\Program Files (x86)\Steam");
    let mut libs = vec![steam.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")) {
        for line in vdf.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("\"path\"") {
                libs.push(PathBuf::from(rest.trim().trim_matches('"').replace("\\\\", "\\")));
            }
        }
    }
    libs.into_iter().map(|l| l.join(g.subdir)).find(|p| p.join(g.exe).is_file())
}

/// The files to put next to the game's exe live in `game\` beside this exe (same layout).
fn bundle_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("game"))).unwrap_or_else(|| PathBuf::from("game"))
}

fn copy_tree(g: &GameInfo, from: &Path, to: &Path, log: &mut Vec<String>) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(g, &entry.path(), &dst, log)?;
        } else if !belongs(g, &entry.file_name().to_string_lossy()) {
            continue;
        } else {
            // another mod's ASI loader under the same name: keep a copy instead of losing it
            if entry.file_name() == "version.dll" && dst.is_file() && std::fs::read(&dst)? != std::fs::read(entry.path())? {
                std::fs::copy(&dst, dst.with_extension("dll.sa-coop-backup"))?;
                log.push("Previous version.dll saved as version.dll.sa-coop-backup".into());
            }
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

fn install(g: &GameInfo, game: &Path) -> Result<Vec<String>, String> {
    let src = bundle_dir();
    if !src.join("coopnet64.asi").is_file() {
        return Err(format!("Mod package not found: {}", src.display()));
    }
    let mut log = vec![];
    copy_tree(g, &src, game, &mut log).map_err(|e| format!("Copy failed: {e}"))?;
    // files of older versions: a second copy of the DLL would run its own, separate network
    for old in ["coopnet64.dll", r"CLEO\sa-coop.js", r"CLEO\coop-test.js", r"CLEO\mano-masina.js"] {
        let _ = std::fs::remove_file(game.join(old));
    }
    log.push("Mod installed.".into());
    extras(g, game, &mut log);
    Ok(log)
}

/// The co-op entry for the game's menus, made on this PC from this PC's own game files: the two menu templates come
/// out of pakchunk0 (repak), MenuPatch adds "co-op" (FunctionToCallOnPress = COOP), repak packs them as a mod pak.
fn build_menu_pak(g: &GameInfo, extra: &Path, gameface: &Path, mods: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let rel = g.menu_rel;
    let (repak, patch) = (extra.join("repak.exe"), extra.join("MenuPatch.exe"));
    if !repak.is_file() || !patch.is_file() {
        return Err("repak.exe or MenuPatch.exe is missing".into());
    }
    let pak = gameface.join(r"Content\Paks\pakchunk0-WindowsNoEditor.pak");
    let tmp = std::env::temp_dir().join(format!("{}-menu", g.data));
    let _ = std::fs::remove_dir_all(&tmp);
    let (src, dst) = (tmp.join("src"), tmp.join("dst"));
    std::fs::create_dir_all(src.join(rel)).map_err(|e| e.to_string())?;
    let run = |exe: &Path, args: &[&std::ffi::OsStr]| -> Result<Vec<u8>, String> {
        let o = std::process::Command::new(exe).args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
        if o.status.success() { Ok(o.stdout) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
    };
    for m in g.menus {
        for ext in ["uasset", "uexp"] {
            let inner = format!("{rel}/{m}.{ext}");
            let bytes = run(&repak, &["get".as_ref(), pak.as_os_str(), inner.as_ref()])?;
            std::fs::write(src.join(rel).join(format!("{m}.{ext}")), bytes).map_err(|e| e.to_string())?;
        }
    }
    let (rel_arg, m0, m1) = (std::ffi::OsString::from(rel), std::ffi::OsString::from(g.menus[0]), std::ffi::OsString::from(g.menus[1]));
    run(&patch, &["patch".as_ref(), src.as_os_str(), dst.as_os_str(), &rel_arg, &m0, &m1])?;
    let out = tmp.join("coop_P.pak");
    run(&repak, &["pack".as_ref(), "--version".as_ref(), "V11".as_ref(), "--mount-point".as_ref(), "../../../".as_ref(), dst.as_os_str(), out.as_os_str()])?;
    std::fs::create_dir_all(mods).map_err(|e| e.to_string())?;
    std::fs::copy(&out, mods.join("coop_P.pak")).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Beside `game\`: `extra\coop_P.pak` (the co-op entry in the game's menus), `extra\Engine.ini` (sharper, further
/// graphics; only if the player has none) and `extra\repak.exe`, which takes the menu fonts out of this PC's own
/// copy of the game (they are never shipped with the mod).
fn extras(g: &GameInfo, win64: &Path, log: &mut Vec<String>) {
    let extra = bundle_dir().with_file_name("extra");
    let Some(gameface) = win64.parent().and_then(|p| p.parent()) else { return };
    let mods = gameface.join(r"Content\Paks\~mods");
    match build_menu_pak(g, &extra, gameface, &mods) {
        Ok(()) => log.push("\"co-op\" menu entry installed.".into()),
        Err(e) => log.push(format!("\"co-op\" menu entry failed: {e}")),
    }
    if let Some(docs) = docs_dir(g).map(|d| d.join(r"Config\WindowsNoEditor")) {
        if !docs.join("Engine.ini").exists() {
            if let Ok(l) = set_graphics(g, 1) {
                log.push(l);
            }
        }
    }
    let fonts = data_dir(g).join("fonts");
    let pak = gameface.join(r"Content\Paks\pakchunk0-WindowsNoEditor.pak");
    if extra.join("repak.exe").is_file() && std::fs::create_dir_all(&fonts).is_ok() {
        use std::os::windows::process::CommandExt;
        for f in ["Beckett", "BankGothicMdBT"] {
            let out = std::process::Command::new(extra.join("repak.exe"))
                .arg("get")
                .arg(&pak)
                .arg(format!("Gameface/Content/Common/Fonts/{f}.ufont"))
                .creation_flags(0x0800_0000)
                .output();
            if let Ok(o) = out {
                if o.status.success() && o.stdout.len() > 1000 && std::fs::write(fonts.join(format!("{f}.ttf")), &o.stdout).is_ok() {
                    log.push(format!("Font {f} taken from the game."));
                }
            }
        }
    }
}

// ---- better car handling (Vice City): the game's own handling.cfg, retuned, packed as ~mods\coop_handling_P.pak ----
const HANDLING_REL: &str = "Gameface/Content/OriginalData/ViceCity/data/handling.cfg";

fn handling_pak(win64: &Path) -> Option<PathBuf> {
    Some(win64.parent()?.parent()?.join(r"Content\Paks\~mods\coop_handling_P.pak"))
}

/// One car line of VC's handling.cfg (33 fields, see the file's header): more grip, stronger brakes with ABS, firmer
/// suspension, a lower centre of mass, a little more pull. Bikes, boats, helicopters and planes are left alone.
fn retune_line(line: &str) -> Option<String> {
    let mut f: Vec<String> = line.split_whitespace().map(String::from).collect();
    if f.len() != 33 || !f[0].chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    let flags = u32::from_str_radix(&f[30], 16).ok()?;
    if flags & 0xF0000 != 0 {
        return None; // IS_BIKE / IS_HELI / IS_PLANE / IS_BOAT
    }
    let num = |s: &str| s.parse::<f32>().ok();
    let (com_z, traction, loss, accel, brake, damping) = (num(&f[7])?, num(&f[9])?, num(&f[10])?, num(&f[14])?, num(&f[17])?, num(&f[22])?);
    let mut set = |i: usize, v: f32| f[i] = format!("{v:.3}");
    set(7, com_z - 0.10);
    set(9, (traction * 1.2).min(2.0));
    set(10, (loss * 1.1).min(1.0));
    set(14, (accel * 1.1).min(10.0));
    set(17, (brake * 1.25).min(10.0));
    set(22, (damping * 1.3).min(1.0));
    f[19] = "1".into(); // ABS
    Some(f.join(" "))
}

fn retune_handling(text: &str) -> (String, usize) {
    let mut n = 0;
    let out: Vec<String> = text
        .lines()
        .map(|l| match retune_line(l) {
            Some(t) => {
                n += 1;
                t
            }
            None => l.to_string(),
        })
        .collect();
    (out.join("\r\n") + "\r\n", n)
}

/// better = true: build the pak from this PC's own handling.cfg; false: remove it.
fn set_handling(g: &GameInfo, win64: &Path, better: bool) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    if !is_vc(g) {
        return Err("the handling fix is Vice City only for now".into());
    }
    let pak_out = handling_pak(win64).ok_or("game folder not found")?;
    if !better {
        let _ = std::fs::remove_file(&pak_out);
        return Ok("Handling: original (applies next time the game starts)".into());
    }
    let repak = bundle_dir().with_file_name("extra").join("repak.exe");
    let gameface = win64.parent().and_then(|p| p.parent()).ok_or("Gameface not found")?;
    let pak = gameface.join(r"Content\Paks\pakchunk0-WindowsNoEditor.pak");
    let o = std::process::Command::new(&repak).arg("get").arg(&pak).arg(HANDLING_REL).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    if !o.status.success() || o.stdout.len() < 1000 {
        return Err(format!("could not read handling.cfg: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    let (text, n) = retune_handling(&String::from_utf8_lossy(&o.stdout));
    let tmp = std::env::temp_dir().join(format!("{}-handling", g.data));
    let _ = std::fs::remove_dir_all(&tmp);
    let file = tmp.join("src").join(HANDLING_REL);
    std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&file, text).map_err(|e| e.to_string())?;
    let out = tmp.join("coop_handling_P.pak");
    let st = std::process::Command::new(&repak)
        .args(["pack", "--version", "V11", "--mount-point", "../../../"])
        .arg(tmp.join("src"))
        .arg(&out)
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("repak pack failed".into());
    }
    std::fs::create_dir_all(pak_out.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(&out, &pak_out).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(format!("Handling: better, {n} cars (applies next time the game starts)"))
}

// ---- ReShade (extra\reshade: the official ReShade64.dll, our VCCoop look) as dxgi.dll next to the game ----------
fn reshade_src() -> PathBuf {
    bundle_dir().with_file_name("extra").join("reshade")
}

/// On when the game's dxgi.dll is our ReShade copy (a dxgi.dll of some other mod is never touched).
fn reshade_on(win64: &Path) -> Option<bool> {
    let ours = std::fs::read(reshade_src().join("ReShade64.dll")).ok();
    match std::fs::read(win64.join("dxgi.dll")) {
        Err(_) => Some(false),
        Ok(have) if ours.as_ref() == Some(&have) => Some(true),
        Ok(_) => None,
    }
}

fn set_reshade(win64: &Path, on: bool) -> Result<String, String> {
    let src = reshade_src();
    match reshade_on(win64) {
        None => return Err("the game folder already has another mod's dxgi.dll: left as is".into()),
        Some(now) if now == on => return Ok(format!("ReShade already {}", if on { "on" } else { "off" })),
        _ => {}
    }
    if !on {
        std::fs::remove_file(win64.join("dxgi.dll")).map_err(|e| e.to_string())?;
        return Ok("ReShade off".into());
    }
    if !src.join("ReShade64.dll").is_file() {
        return Err("the package has no extra\\reshade".into());
    }
    let mut log = vec![];
    if !win64.join("ReShade.ini").exists() {
        std::fs::copy(src.join("ReShade.ini"), win64.join("ReShade.ini")).map_err(|e| e.to_string())?;
    }
    copy_tree(&GAMES[0], &src.join("reshade-shaders"), &win64.join("reshade-shaders"), &mut log).map_err(|e| e.to_string())?;
    std::fs::copy(src.join("ReShade64.dll"), win64.join("dxgi.dll")).map_err(|e| e.to_string())?;
    Ok("ReShade on: Home opens its menu, Scroll Lock turns effects off".into())
}

/// Remove what install() put in: every file of the bundle (same relative path), our paks, our ReShade and our
/// graphics settings. A file the player had before is left alone where we can tell (version.dll, dxgi.dll).
fn uninstall(g: &GameInfo, win64: &Path) -> Vec<String> {
    let mut log = vec![];
    fn walk(g: &GameInfo, from: &Path, to: &Path, n: &mut usize) {
        let Ok(rd) = std::fs::read_dir(from) else { return };
        for e in rd.flatten() {
            let (src, dst) = (e.path(), to.join(e.file_name()));
            if src.is_dir() {
                walk(g, &src, &dst, n);
                let _ = std::fs::remove_dir(&dst); // only when empty
            } else if belongs(g, &e.file_name().to_string_lossy()) && dst.is_file() {
                if e.file_name() == "version.dll" && std::fs::read(&dst).ok() != std::fs::read(&src).ok() {
                    continue; // another mod's loader now: not ours to delete
                }
                if std::fs::remove_file(&dst).is_ok() {
                    *n += 1;
                }
            }
        }
    }
    let mut n = 0;
    walk(g, &bundle_dir(), win64, &mut n);
    let backup = win64.join("version.dll.sa-coop-backup");
    if backup.is_file() && !win64.join("version.dll").exists() {
        let _ = std::fs::rename(&backup, win64.join("version.dll"));
        log.push("Previous version.dll restored".into());
    }
    log.push(format!("Files removed: {n}"));
    if reshade_on(win64) == Some(true) {
        let _ = set_reshade(win64, false);
        let _ = std::fs::remove_dir_all(win64.join("reshade-shaders"));
        let _ = std::fs::remove_file(win64.join("ReShade.ini"));
        log.push("ReShade removed".into());
    }
    if let Some(mods) = win64.parent().and_then(|p| p.parent()).map(|p| p.join(r"Content\Paks\~mods")) {
        for pak in ["coop_P.pak", "coop_handling_P.pak"] {
            if std::fs::remove_file(mods.join(pak)).is_ok() {
                log.push(format!("Removed {pak}"));
            }
        }
        let _ = std::fs::remove_dir(&mods);
    }
    if graphics_level(g).is_some() {
        let _ = set_graphics(g, 0);
        log.push("Graphics settings restored".into());
    }
    log
}

fn installed(g: &GameInfo, game: &Path) -> bool {
    ["version.dll", "cleo_redux64.asi", "coopnet64.asi", g.script].iter().all(|f| game.join(f).is_file())
}

// ---- session file the game's DLL reads --------------------------------------------------------------------

fn write_session(g: &GameInfo, server: SocketAddr, key: u32, nick: &str) -> std::io::Result<()> {
    let dir = data_dir(g);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("session.txt"), format!("server={server}\nkey={key:08x}\nnick={nick}\n"))
}

/// Start the game through Steam (the steam:// link, opened by Windows like any link: no command prompt).
fn start_game(g: &GameInfo) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (op, url) = (wide("open"), wide(g.steam));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(), 1);
    }
}

/// Is the game's exe running now (its files are locked while it runs)?
fn game_running(g: &GameInfo) -> bool {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return false;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok && !found {
            let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            found = String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(g.exe);
            ok = Process32NextW(snap, &mut e) != 0;
        }
        windows_sys::Win32::Foundation::CloseHandle(snap);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retunes_cars_only() {
        let car = "INFERNUS        1500.0 1.9 4.6 1.3 0.0 0.0 -0.25 70  0.80 0.85 0.50 5 240.0 30.0 R P 11.0  0.51 0 30.0  1.5  0.10 0.0  0.27 35000 0.25 -0.10 0.5  0.4		40002004	1  1";
        let t = retune_line(car).unwrap();
        let f: Vec<&str> = t.split_whitespace().collect();
        assert_eq!(f[9], "0.960");
        assert_eq!(f[19], "1");
        assert_eq!(f[7], "-0.350");
        let bike = "PCJ600          200.0  0.8 2.1 1.1 0.0 0.05 -0.1 103 1.4  0.85 0.48 5 190.0 35.0 R P 15.0  0.50 0 30.0  1.2  0.15 0.0  0.10 10000 0.20 -0.10 0.55 0.0		10000		1  1";
        assert!(retune_line(bike).is_none());
        assert!(retune_line("; comment").is_none());
    }
}
