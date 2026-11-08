//! sa-coop.exe: install the mod into GTA San Andreas or Vice City The Definitive Edition, host a game (the server runs here)
//! or join one with a code, then start the game through Steam. Like Saints Row 2: everything before the game starts.
#![windows_subsystem = "windows"]

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
    if let Some(i) = args.iter().position(|a| a == "--install") {
        let g = if args.get(i + 1).is_some_and(|a| a.eq_ignore_ascii_case("vc")) { &GAMES[1] } else { &GAMES[0] };
        let lines = match find_game(g) {
            Some(dir) => install(g, &dir).unwrap_or_else(|e| vec![e]),
            None => vec![format!("{} nerastas Steam bibliotekose", g.name)],
        };
        let dir = data_dir(g);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("install.log"), lines.join("
"));
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([460.0, 520.0]).with_resizable(false),
        ..Default::default()
    };
    eframe::run_native("SA / VC Co-op", options, Box::new(|_| Ok(Box::new(App::new()))))
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
                log.push("Ankstesnis version.dll išsaugotas kaip version.dll.sa-coop-backup".into());
            }
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

fn install(g: &GameInfo, game: &Path) -> Result<Vec<String>, String> {
    let src = bundle_dir();
    if !src.join("coopnet64.asi").is_file() {
        return Err(format!("Nerastas modo paketas: {}", src.display()));
    }
    let mut log = vec![];
    copy_tree(g, &src, game, &mut log).map_err(|e| format!("Nepavyko nukopijuoti: {e}"))?;
    // files of older versions: a second copy of the DLL would run its own, separate network
    for old in ["coopnet64.dll", r"CLEO\sa-coop.js", r"CLEO\coop-test.js", r"CLEO\mano-masina.js"] {
        let _ = std::fs::remove_file(game.join(old));
    }
    log.push("Modas įdiegtas.".into());
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
        return Err("trūksta repak.exe arba MenuPatch.exe".into());
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
        Ok(()) => log.push("„co-op“ įrašas meniu įdiegtas.".into()),
        Err(e) => log.push(format!("„co-op“ meniu įrašo nepavyko sukurti: {e}")),
    }
    if let Some(docs) = docs_dir(g).map(|d| d.join(r"Config\WindowsNoEditor")) {
        let ini = docs.join("Engine.ini");
        if !ini.exists() && extra.join("Engine.ini").is_file() && std::fs::create_dir_all(&docs).is_ok() && std::fs::copy(extra.join("Engine.ini"), &ini).is_ok() {
            log.push("Grafikos patobulinimai įjungti (Engine.ini).".into());
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
                    log.push(format!("Šriftas {f} paimtas iš žaidimo."));
                }
            }
        }
    }
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

fn start_game(g: &GameInfo) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", g.steam]).creation_flags(0x0800_0000).spawn();
}

// ---- UI -------------------------------------------------------------------------------------------------

#[derive(PartialEq)]
enum Tab {
    Host,
    Join,
}

struct Hosting {
    internet_code: Option<String>,
    lan_code: Option<String>,
    players: Players,
}

struct App {
    which: usize,
    games: [Option<PathBuf>; 2],
    nick: String,
    tab: Tab,
    code: String,
    hosting: Option<Hosting>,
    msg: Vec<String>,
}

impl App {
    fn new() -> App {
        let games = [find_game(&GAMES[0]), find_game(&GAMES[1])];
        // Vice City first when only Vice City is there
        let which = if games[0].is_none() && games[1].is_some() { 1 } else { 0 };
        App { which, games, nick: "Žaidėjas".into(), tab: Tab::Host, code: String::new(), hosting: None, msg: vec![] }
    }

    fn host(&mut self) {
        let sock = match UdpSocket::bind(("0.0.0.0", DEFAULT_PORT)) {
            Ok(s) => s,
            Err(e) => return self.msg.push(format!("Nepavyko atidaryti {DEFAULT_PORT} prievado: {e}")),
        };
        let key = random_key();
        let players: Players = Arc::default();
        let p2 = players.clone();
        std::thread::spawn(move || run_server(sock, key, p2));
        let internet_code = upnp(DEFAULT_PORT).map(|ip| make_code(ip.octets(), DEFAULT_PORT, key));
        let lan_code = lan_ip().map(|ip| make_code(ip.octets(), DEFAULT_PORT, key));
        if internet_code.is_none() {
            self.msg.push(format!("Maršrutizatorius neatidarė prievado (UPnP). Draugui per internetą atidaryk UDP {DEFAULT_PORT} ranka."));
        }
        if let Err(e) = write_session(&GAMES[self.which], SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)), key, &self.nick) {
            self.msg.push(format!("Nepavyko įrašyti sesijos: {e}"));
        }
        self.hosting = Some(Hosting { internet_code, lan_code, players });
    }

    fn join(&mut self) -> bool {
        let Some((ip, port, key)) = parse_code(&self.code) else {
            self.msg.push("Kodas neteisingas: jame turi būti 16 simbolių.".into());
            return false;
        };
        match write_session(&GAMES[self.which], SocketAddr::from((ip, port)), key, &self.nick) {
            Ok(()) => true,
            Err(e) => {
                self.msg.push(format!("Nepavyko įrašyti sesijos: {e}"));
                false
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SA / VC Co-op");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.which, 0, "San Andreas");
                ui.selectable_value(&mut self.which, 1, "Vice City");
            });
            let g = &GAMES[self.which];
            ui.label(g.name);
            ui.separator();

            match self.games[self.which].clone() {
                None => {
                    ui.colored_label(egui::Color32::RED, "Žaidimas nerastas Steam bibliotekose.");
                    return;
                }
                Some(game) => {
                    if installed(g, &game) {
                        ui.label("✔ Modas įdiegtas");
                    } else {
                        ui.label("Modas dar neįdiegtas.");
                    }
                    if ui.button("Įdiegti / atnaujinti modą").clicked() {
                        match install(g, &game) {
                            Ok(l) => self.msg.extend(l),
                            Err(e) => self.msg.push(e),
                        }
                    }
                }
            }
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Slapyvardis:");
                ui.add(egui::TextEdit::singleline(&mut self.nick).char_limit(NICK_LEN));
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Host, "Kurti žaidimą");
                ui.selectable_value(&mut self.tab, Tab::Join, "Prisijungti");
            });
            ui.add_space(6.0);

            match self.tab {
                Tab::Host => match &self.hosting {
                    None => {
                        ui.label("Serveris veiks šiame kompiuteryje. Draugui duosi kodą.");
                        if ui.button("Kurti žaidimą").clicked() {
                            self.host();
                        }
                    }
                    Some(h) => {
                        for (label, code) in [("Kodas draugui per internetą:", &h.internet_code), ("Kodas tame pačiame tinkle:", &h.lan_code)] {
                            if let Some(code) = code {
                                ui.label(label);
                                ui.horizontal(|ui| {
                                    ui.monospace(code);
                                    if ui.button("Kopijuoti").clicked() {
                                        ctx.copy_text(code.clone());
                                    }
                                });
                            }
                        }
                        let players = h.players.lock().unwrap().clone();
                        ui.label(format!("Prisijungę: {}", if players.is_empty() { "dar niekas".into() } else { players.join(", ") }));
                        if ui.button("Paleisti žaidimą").clicked() {
                            start_game(g);
                        }
                        ui.small("Nelaikyk šio lango uždaryto, kol žaidžiat: jame veikia serveris.");
                    }
                },
                Tab::Join => {
                    ui.label("Draugo kodas:");
                    ui.text_edit_singleline(&mut self.code);
                    if ui.button("Prisijungti ir paleisti žaidimą").clicked() && self.join() {
                        start_game(g);
                        self.msg.push("Žaidimas paleidžiamas. Įkėlus išsaugojimą prisijungsi automatiškai.".into());
                    }
                }
            }

            ui.separator();
            for m in self.msg.iter().rev().take(6) {
                ui.small(m);
            }
        });
    }
}
