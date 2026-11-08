//! sa-coop.exe: install the mod into GTA San Andreas The Definitive Edition, host a game (the server runs here)
//! or join one with a code, then start the game through Steam. Like Saints Row 2: everything before the game starts.
#![windows_subsystem = "windows"]

use eframe::egui;
use proto::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STEAM_APP: &str = "steam://rungameid/1547000";
const GAME_SUBDIR: &str = r"steamapps\common\GTA San Andreas - The Definitive Edition\Gameface\Binaries\Win64";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([460.0, 520.0]).with_resizable(false),
        ..Default::default()
    };
    eframe::run_native("SA Co-op", options, Box::new(|_| Ok(Box::new(App::new()))))
}

// ---- game folder and install ---------------------------------------------------------------------------

/// Every Steam library from libraryfolders.vdf, checked for the game's exe.
fn find_game() -> Option<PathBuf> {
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
    libs.into_iter().map(|l| l.join(GAME_SUBDIR)).find(|p| p.join("SanAndreas.exe").is_file())
}

/// The files to put next to SanAndreas.exe live in `game\` beside this exe (same layout).
fn bundle_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("game"))).unwrap_or_else(|| PathBuf::from("game"))
}

fn copy_tree(from: &Path, to: &Path, log: &mut Vec<String>) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dst, log)?;
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

fn install(game: &Path) -> Result<Vec<String>, String> {
    let src = bundle_dir();
    if !src.join("coopnet64.dll").is_file() {
        return Err(format!("Nerastas modo paketas: {}", src.display()));
    }
    let mut log = vec![];
    copy_tree(&src, game, &mut log).map_err(|e| format!("Nepavyko nukopijuoti: {e}"))?;
    log.push("Modas įdiegtas.".into());
    Ok(log)
}

fn installed(game: &Path) -> bool {
    ["version.dll", "cleo_redux64.asi", "coopnet64.dll", r"CLEO\sa-coop.js"].iter().all(|f| game.join(f).is_file())
}

// ---- session file the game's DLL reads --------------------------------------------------------------------

fn write_session(server: SocketAddr, key: u32, nick: &str) -> std::io::Result<()> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("sa-de-coop");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("session.txt"), format!("server={server}\nkey={key:08x}\nnick={nick}\n"))
}

fn start_game() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", STEAM_APP]).creation_flags(0x0800_0000).spawn();
}

fn random_key() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos() ^ std::process::id() as u128);
    (h.finish() as u32) | 1
}

fn lan_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:80").ok()?; // no packet is sent: this only picks the outgoing interface
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(v) => Some(v),
        _ => None,
    }
}

// ---- server ---------------------------------------------------------------------------------------------

struct Peer {
    addr: SocketAddr,
    nick: String,
    seen: Instant,
}

type Players = Arc<Mutex<Vec<String>>>;

fn run_server(sock: UdpSocket, key: u32, players: Players) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let mut peers: [Option<Peer>; MAX_PLAYERS] = Default::default();
    let mut buf = [0u8; 512];
    loop {
        // drop the silent: a crashed game must not hold a seat
        for slot in 0..MAX_PLAYERS {
            if peers[slot].as_ref().is_some_and(|p| p.seen.elapsed() > Duration::from_secs(10)) {
                peers[slot] = None;
                broadcast(&sock, &peers, None, &packet(LEAVE, &[slot as u8]));
            }
        }
        *players.lock().unwrap() = peers.iter().flatten().map(|p| p.nick.clone()).collect();

        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let Some((kind, body)) = parse(&buf[..n]) else { continue };
        let known = peers.iter().position(|p| p.as_ref().is_some_and(|p| p.addr == from));
        match (kind, known) {
            (JOIN, _) if body.len() >= 4 + NICK_LEN => {
                if u32::from_le_bytes(body[..4].try_into().unwrap()) != key {
                    let _ = sock.send_to(&packet(DENY, &[1]), from);
                    continue;
                }
                let nick = String::from_utf8_lossy(&body[4..4 + NICK_LEN]).trim_end_matches('\0').to_string();
                let slot = known.or_else(|| peers.iter().position(|p| p.is_none()));
                match slot {
                    Some(s) => {
                        peers[s] = Some(Peer { addr: from, nick, seen: Instant::now() });
                        let _ = sock.send_to(&packet(WELCOME, &[s as u8]), from);
                    }
                    None => {
                        let _ = sock.send_to(&packet(DENY, &[2]), from);
                    }
                }
            }
            (STATE, Some(s)) if body.len() == STATE_SIZE => {
                if let Some(p) = peers[s].as_mut() {
                    p.seen = Instant::now();
                }
                if State::from_bytes(body).is_some() {
                    let mut out = vec![s as u8];
                    out.extend_from_slice(body);
                    broadcast(&sock, &peers, Some(s), &packet(STATE, &out));
                }
            }
            (LEAVE, Some(s)) => {
                peers[s] = None;
                broadcast(&sock, &peers, None, &packet(LEAVE, &[s as u8]));
            }
            _ => {}
        }
    }
}

fn broadcast(sock: &UdpSocket, peers: &[Option<Peer>; MAX_PLAYERS], except: Option<usize>, p: &[u8]) {
    for (i, peer) in peers.iter().enumerate() {
        if let Some(peer) = peer {
            if Some(i) != except {
                let _ = sock.send_to(p, peer.addr);
            }
        }
    }
}

/// Ask the router (UPnP) to forward the port and tell us the outside address. None: no UPnP, the host forwards
/// the port by hand and the LAN code still works.
fn upnp(port: u16) -> Option<Ipv4Addr> {
    let gw = igd_next::search_gateway(Default::default()).ok()?;
    let local = SocketAddr::new(IpAddr::V4(lan_ip()?), port);
    gw.add_port(igd_next::PortMappingProtocol::UDP, port, local, 6 * 3600, "SA Co-op").ok()?;
    match gw.get_external_ip().ok()? {
        IpAddr::V4(v) => Some(v),
        _ => None,
    }
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
    game: Option<PathBuf>,
    nick: String,
    tab: Tab,
    code: String,
    hosting: Option<Hosting>,
    msg: Vec<String>,
}

impl App {
    fn new() -> App {
        App { game: find_game(), nick: "Žaidėjas".into(), tab: Tab::Host, code: String::new(), hosting: None, msg: vec![] }
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
        if let Err(e) = write_session(SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)), key, &self.nick) {
            self.msg.push(format!("Nepavyko įrašyti sesijos: {e}"));
        }
        self.hosting = Some(Hosting { internet_code, lan_code, players });
    }

    fn join(&mut self) -> bool {
        let Some((ip, port, key)) = parse_code(&self.code) else {
            self.msg.push("Kodas neteisingas: jame turi būti 16 simbolių.".into());
            return false;
        };
        match write_session(SocketAddr::from((ip, port)), key, &self.nick) {
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
            ui.heading("SA Co-op");
            ui.label("GTA San Andreas – The Definitive Edition");
            ui.separator();

            match self.game.clone() {
                None => {
                    ui.colored_label(egui::Color32::RED, "Žaidimas nerastas Steam bibliotekose.");
                    return;
                }
                Some(game) => {
                    if installed(&game) {
                        ui.label("✔ Modas įdiegtas");
                    } else {
                        ui.label("Modas dar neįdiegtas.");
                    }
                    if ui.button("Įdiegti / atnaujinti modą").clicked() {
                        match install(&game) {
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
                            start_game();
                        }
                        ui.small("Nelaikyk šio lango uždaryto, kol žaidžiat: jame veikia serveris.");
                    }
                },
                Tab::Join => {
                    ui.label("Draugo kodas:");
                    ui.text_edit_singleline(&mut self.code);
                    if ui.button("Prisijungti ir paleisti žaidimą").clicked() && self.join() {
                        start_game();
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
