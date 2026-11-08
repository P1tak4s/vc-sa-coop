//! coopnet64.dll: loaded by the CLEO Redux script `sa-coop.js` (DynamicLibrary.Load) and called through Fn.X64.
//! Only integers cross the boundary: states go through memory the script allocates (layout = proto::State).
//! The network runs on its own thread, so a slow or lost packet never stalls the game.

mod dialog;
mod steam;

use proto::*;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const STALE: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Shared {
    status: i32, // 0 off, 1 joining, 2 joined, 3 denied, 4 bad session
    slot: i32,
    mine: Option<State>,
    peers: [Option<(State, Instant)>; MAX_PLAYERS],
    stop: bool,
    code: Option<String>,       // host: the code to give friends (internet one once UPnP answers)
    revive_out: Option<u8>,     // a revive to send to that slot
    revived: bool,              // someone revived us; the script takes it once
    names: [String; MAX_PLAYERS],
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Shared { slot: -1, ..Default::default() }))
}

struct Session {
    server: SocketAddr,
    key: u32,
    nick: String,
}

/// `%LOCALAPPDATA%\sa-de-coop\session.txt`, written by the launcher: `server=`, `key=` (hex), `nick=`.
fn read_session() -> Option<Session> {
    let path = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("sa-de-coop").join("session.txt");
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::rename(&path, path.with_extension("used"));
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()));
    Some(Session {
        server: get("server=")?.to_socket_addrs().ok()?.next()?,
        key: u32::from_str_radix(&get("key=")?, 16).ok()?,
        nick: get("nick=").unwrap_or_else(|| "Player".into()),
    })
}

fn net_thread(sess: Session) {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        shared().lock().unwrap().status = 4;
        return;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(20)));
    let mut join = Vec::with_capacity(20);
    join.extend_from_slice(&sess.key.to_le_bytes());
    join.extend_from_slice(&nick_bytes(&sess.nick));
    let join = packet(JOIN, &join);
    let mut last_join = Instant::now() - Duration::from_secs(5);
    let mut last_send = Instant::now();
    let mut buf = [0u8; 512];

    loop {
        let (status, mine, stop, revive) = {
            let mut s = shared().lock().unwrap();
            (s.status, s.mine, s.stop, s.revive_out.take())
        };
        if let (2, Some(target)) = (status, revive) {
            let _ = sock.send_to(&packet(REVIVE, &[target]), sess.server);
        }
        if stop {
            let _ = sock.send_to(&packet(LEAVE, &[]), sess.server);
            return;
        }
        if status == 1 && last_join.elapsed() > Duration::from_secs(1) {
            let _ = sock.send_to(&join, sess.server);
            last_join = Instant::now();
        }
        // 20 Hz is plenty for walking and driving; the script smooths in between
        if status == 2 && last_send.elapsed() >= Duration::from_millis(50) {
            if let Some(m) = mine {
                let _ = sock.send_to(&packet(STATE, &m.to_bytes()), sess.server);
            }
            last_send = Instant::now();
        }

        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        if from != sess.server {
            continue; // only the server may talk to us
        }
        let Some((kind, body)) = parse(&buf[..n]) else { continue };
        let mut s = shared().lock().unwrap();
        match kind {
            WELCOME if !body.is_empty() => {
                s.status = 2;
                s.slot = body[0] as i32;
            }
            DENY => s.status = 3,
            STATE if body.len() > STATE_SIZE => {
                let slot = body[0] as usize;
                if slot < MAX_PLAYERS && slot as i32 != s.slot {
                    if let Some(st) = State::from_bytes(&body[1..]) {
                        s.peers[slot] = Some((st, Instant::now()));
                    }
                }
            }
            NAMES if !body.is_empty() => {
                for e in body[1..].chunks_exact(1 + NICK_LEN).take(body[0] as usize) {
                    if (e[0] as usize) < MAX_PLAYERS {
                        // ASCII only: the game's font and our script both expect it
                        let n: String = e[1..].iter().take_while(|&&b| b != 0).filter(|b| b.is_ascii_graphic()).map(|&b| b as char).collect();
                        s.names[e[0] as usize] = n;
                    }
                }
            }
            REVIVE if body.len() == 2 && body[1] as i32 == s.slot => s.revived = true,
            LEAVE if !body.is_empty() && (body[0] as usize) < MAX_PLAYERS => s.peers[body[0] as usize] = None,
            _ => {}
        }
    }
}

fn start_client(sess: Session) -> i64 {
    let mut s = shared().lock().unwrap();
    if matches!(s.status, 1 | 2) {
        return 1;
    }
    s.status = 1;
    s.stop = false;
    drop(s);
    std::thread::spawn(move || net_thread(sess));
    1
}

fn my_nick() -> String {
    let n: String = std::env::var("USERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).take(NICK_LEN).collect();
    if n.is_empty() { "Player".into() } else { n }
}

/// Joins the session the launcher wrote, if there is one. 1 = joining (or already in), 0 = no session.
#[no_mangle]
pub extern "C" fn coop_start() -> i64 {
    if matches!(shared().lock().unwrap().status, 1 | 2) {
        return 1;
    }
    match read_session() {
        Some(sess) => start_client(sess),
        None => 0,
    }
}

/// F9: run the server inside the game, join it, and make a code (copied to the clipboard). 1 = hosting.
#[no_mangle]
pub extern "C" fn coop_host() -> i64 {
    if matches!(shared().lock().unwrap().status, 1 | 2) {
        return 1;
    }
    let Ok(sock) = UdpSocket::bind(("0.0.0.0", DEFAULT_PORT)) else { return 0 };
    let key = relay::random_key();
    std::thread::spawn(move || relay::run_server(sock, key, Default::default()));
    if let Some(ip) = relay::lan_ip() {
        set_code(make_code(ip.octets(), DEFAULT_PORT, key));
    }
    // UPnP can take seconds: the LAN code is there at once, the internet one replaces it when the router answers
    std::thread::spawn(move || {
        if let Some(ip) = relay::upnp(DEFAULT_PORT) {
            set_code(make_code(ip.octets(), DEFAULT_PORT, key));
        }
    });
    start_client(Session { server: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)), key, nick: my_nick() })
}

fn set_code(code: String) {
    let _ = clipboard_win::set_clipboard_string(&code);
    shared().lock().unwrap().code = Some(code);
}

/// The host's current code as ASCII into `out` (up to `max` bytes). Returns its length, 0 = none yet.
#[no_mangle]
pub unsafe extern "C" fn coop_code(out: *mut u8, max: i64) -> i64 {
    let s = shared().lock().unwrap();
    let Some(code) = &s.code else { return 0 };
    if out.is_null() || (max as usize) < code.len() {
        return 0;
    }
    std::ptr::copy_nonoverlapping(code.as_ptr(), out, code.len());
    code.len() as i64
}

/// F10: join with the code on the clipboard. 1 = joining, 0 = no valid code there.
#[no_mangle]
pub extern "C" fn coop_join_clipboard() -> i64 {
    let Ok(text) = clipboard_win::get_clipboard_string() else { return 0 };
    let Some((ip, port, key)) = parse_code(text.trim()) else { return 0 };
    start_client(Session { server: SocketAddr::from((ip, port)), key, nick: my_nick() })
}

/// Player `slot`'s name (ASCII) into `out`. Returns its length, 0 = unknown.
#[no_mangle]
pub unsafe extern "C" fn coop_name(slot: i64, out: *mut u8, max: i64) -> i64 {
    if out.is_null() || !(0..MAX_PLAYERS as i64).contains(&slot) {
        return 0;
    }
    let s = shared().lock().unwrap();
    let n = &s.names[slot as usize];
    let len = n.len().min(max.max(0) as usize);
    std::ptr::copy_nonoverlapping(n.as_ptr(), out, len);
    len as i64
}

/// Revive the player in `slot` (the script checked they are down and we stand next to them).
#[no_mangle]
pub extern "C" fn coop_revive(slot: i64) -> i64 {
    if !(0..MAX_PLAYERS as i64).contains(&slot) {
        return 0;
    }
    shared().lock().unwrap().revive_out = Some(slot as u8);
    1
}

/// 1 once after a friend revived us.
#[no_mangle]
pub extern "C" fn coop_take_revived() -> i64 {
    std::mem::take(&mut shared().lock().unwrap().revived) as i64
}

// ---- Steam mode: a named lobby with a password, P2P through Steam, no server and no ports --------------------

unsafe fn c_str(p: *const u8) -> String {
    if p.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(p as *const std::ffi::c_char).to_string_lossy().trim().to_string()
}

/// Status codes the script shows: 5 = no lobby by that name, 6 = wrong password, 7 = Steam failed, 8 = lobby full.
fn steam_thread(host: bool, name: String, pw: String) {
    let fail = |code: i32| shared().lock().unwrap().status = code;
    let st = match steam::Steam::load() {
        Ok(st) => st,
        Err(e) => {
            log_line(&format!("steam: {e}"));
            return fail(7);
        }
    };
    let hash = steam::pw_hash(&pw);
    let lobby = if host {
        match st.host(&name, &hash) {
            Some(l) => l,
            None => return fail(7),
        }
    } else {
        match st.join(&name, &hash) {
            Ok(l) => l,
            Err("not found") => return fail(5),
            Err("password") => return fail(6),
            Err("full") => return fail(8),
            Err(_) => return fail(7),
        }
    };
    log_line(&format!("steam: in lobby {lobby} as {}", st.me()));

    let mut buf = [0u8; 512];
    let mut members: Vec<u64> = vec![];
    let mut last_members = Instant::now() - Duration::from_secs(5);
    let mut last_send = Instant::now();
    loop {
        if last_members.elapsed() > Duration::from_secs(1) {
            members = st.members(lobby);
            let mut s = shared().lock().unwrap();
            s.slot = members.iter().position(|&m| m == st.me()).map_or(-1, |i| i as i32);
            s.status = 2;
            for (i, &m) in members.iter().enumerate() {
                st.accept(m);
                s.names[i] = st.name_of(m).chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(NICK_LEN).collect();
            }
            for i in members.len()..MAX_PLAYERS {
                s.peers[i] = None;
            }
            last_members = Instant::now();
        }
        let (mine, stop, revive, my_slot) = {
            let mut s = shared().lock().unwrap();
            (s.mine, s.stop, s.revive_out.take(), s.slot)
        };
        if stop {
            st.leave(lobby);
            return;
        }
        let others = members.iter().copied().filter(|&m| m != st.me());
        if last_send.elapsed() >= Duration::from_millis(50) {
            if let Some(m) = mine {
                let p = packet(STATE, &m.to_bytes());
                for to in others.clone() {
                    st.send(to, &p, steam::SEND_UNRELIABLE);
                }
            }
            last_send = Instant::now();
        }
        if let Some(t) = revive {
            if let Some(&to) = members.get(t as usize) {
                st.send(to, &packet(REVIVE, &[my_slot.max(0) as u8, t]), steam::SEND_RELIABLE);
            }
        }
        let mut got = false;
        while let Some((from, n)) = st.recv(&mut buf) {
            got = true;
            let Some(slot) = members.iter().position(|&m| m == from) else { continue }; // not in our lobby
            let Some((kind, body)) = parse(&buf[..n]) else { continue };
            let mut s = shared().lock().unwrap();
            match kind {
                STATE if body.len() == STATE_SIZE => {
                    if let Some(stt) = State::from_bytes(body) {
                        s.peers[slot] = Some((stt, Instant::now()));
                    }
                }
                REVIVE if body.len() == 2 && body[1] as i32 == s.slot => s.revived = true,
                _ => {}
            }
        }
        if !got {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn log_line(text: &str) {
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        let p = std::path::PathBuf::from(dir).join("sa-de-coop");
        let _ = std::fs::create_dir_all(&p);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p.join("coopnet.log")) {
            let _ = writeln!(f, "{text}");
        }
    }
}

fn start_steam(host: bool, name: String, pw: String) -> i64 {
    if name.is_empty() {
        return 0;
    }
    let mut s = shared().lock().unwrap();
    if matches!(s.status, 1 | 2) {
        return 1;
    }
    s.status = 1;
    s.stop = false;
    drop(s);
    std::thread::spawn(move || steam_thread(host, name, pw));
    1
}

pub(crate) fn status_code() -> i32 {
    shared().lock().unwrap().status
}

pub(crate) fn start_steam_session(host: bool, name: String, pw: String) -> i64 {
    start_steam(host, name, pw)
}

/// Loaded by the ASI loader at game start (as coopnet64.asi): watch F9 for the co-op window from the title screen on.
#[no_mangle]
pub extern "system" fn DllMain(_module: *mut std::ffi::c_void, reason: u32, _reserved: *mut std::ffi::c_void) -> i32 {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if reason == 1 && !STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        std::thread::spawn(dialog::hotkey_thread);
    }
    1
}

/// Create a Steam lobby with this name and password (null-terminated ASCII). 1 = started.
#[no_mangle]
pub unsafe extern "C" fn coop_steam_host(name: *const u8, pw: *const u8) -> i64 {
    start_steam(true, c_str(name), c_str(pw))
}

/// Find the Steam lobby by name and join it with the password. 1 = started; watch coop_status.
#[no_mangle]
pub unsafe extern "C" fn coop_steam_join(name: *const u8, pw: *const u8) -> i64 {
    start_steam(false, c_str(name), c_str(pw))
}

#[no_mangle]
pub extern "C" fn coop_status() -> i64 {
    shared().lock().unwrap().status as i64
}

#[no_mangle]
pub extern "C" fn coop_slot() -> i64 {
    shared().lock().unwrap().slot as i64
}

/// Our own state, copied from script memory (64 bytes, proto::State layout).
#[no_mangle]
pub unsafe extern "C" fn coop_send(state: *const u8) -> i64 {
    if state.is_null() {
        return 0;
    }
    let bytes = std::slice::from_raw_parts(state, STATE_SIZE);
    let Some(st) = State::from_bytes(bytes) else { return 0 };
    shared().lock().unwrap().mine = Some(st);
    1
}

/// Fills `out` with peer `slot`'s latest state. 1 = fresh, 0 = nobody there (or silent for 3 s).
#[no_mangle]
pub unsafe extern "C" fn coop_get(slot: i64, out: *mut u8) -> i64 {
    if out.is_null() || !(0..MAX_PLAYERS as i64).contains(&slot) {
        return 0;
    }
    let s = shared().lock().unwrap();
    match s.peers[slot as usize] {
        Some((st, at)) if at.elapsed() < STALE => {
            std::ptr::copy_nonoverlapping(st.to_bytes().as_ptr(), out, STATE_SIZE);
            1
        }
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn coop_stop() -> i64 {
    shared().lock().unwrap().stop = true;
    1
}
