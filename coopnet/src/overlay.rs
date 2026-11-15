//! The co-op menu drawn inside the game (Dear ImGui through hudhook, DirectX 11 or 12), from the title screen on.
//! Opened by the "co-op" entry of the game's own menus (see menu_hook) or by F9. While it is open the game gets
//! no mouse or keyboard input.

use hudhook::imgui::{self, Condition, FontConfig, FontGlyphRanges, FontSource, StyleColor, Ui};
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};
use std::sync::atomic::{AtomicBool, Ordering};

pub static OPEN: AtomicBool = AtomicBool::new(false);
/// How the others see us: 0 = a Grove Street member, 1.. = a story character (SKINS in the script).
pub static SKIN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// The last message about save sets, shown in the status row.
pub static SAVE_NOTE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
pub const SKIN_NAMES: [&str; 8] = ["Grove Street member", "Sweet", "Ryder", "Big Smoke", "Cesar", "Kendl", "Woozie", "The Truth"];
/// Vice City: same order as LOOKS in vc-coop.js; ordinary peds only (loading a story character crashed VC DE)
pub const SKIN_NAMES_VC: [&str; 8] = ["Street guy", "Cuban", "Haitian", "Gang member", "Biker", "Beach guy", "Businessman", "Girl"];

fn skin_names() -> &'static [&'static str; 8] {
    match crate::game::current().kind {
        crate::game::Kind::Sa => &SKIN_NAMES,
        crate::game::Kind::Vc => &SKIN_NAMES_VC,
    }
}

/// Name tags over friends' heads: the script projects the head to the 640 x 448 text screen each frame and hands the
/// point over (the game's own text commands draw nothing from a CLEO script in the Definitive Edition).
pub static TAGS: std::sync::Mutex<[Option<(f32, f32, bool, std::time::Instant)>; 4]> = std::sync::Mutex::new([None; 4]);

fn draw_tags(ui: &Ui, font: Option<imgui::FontId>) {
    let [dw, dh] = ui.io().display_size;
    let tags = *TAGS.lock().unwrap();
    let dl = ui.get_background_draw_list();
    let _f = font.map(|f| ui.push_font(f));
    for (slot, t) in tags.iter().enumerate() {
        let Some((x, y, down, at)) = *t else { continue };
        if at.elapsed() > std::time::Duration::from_millis(250) {
            continue; // the script stopped sending it: out of view, gone or in another interior
        }
        let mut name = crate::peer_name(slot);
        if name.is_empty() {
            name = format!("Player{}", slot + 1);
        }
        let size = ui.calc_text_size(&name);
        let p = [x / 640.0 * dw - size[0] / 2.0, y / 448.0 * dh - size[1]];
        let col = if down { [1.0, 0.33, 0.33, 1.0] } else { [0.55, 0.85, 1.0, 1.0] };
        dl.add_text([p[0] + 2.0, p[1] + 2.0], [0.0, 0.0, 0.0, 0.85], &name);
        dl.add_text(p, col, &name);
    }
}

/// Big lines in the middle of the screen (down / revive countdowns, key hints): line, text, colour, until.
/// The script refreshes a line every frame it wants it; a line not refreshed for 250 ms disappears.
pub static HUD: std::sync::Mutex<[Option<(String, [f32; 4], std::time::Instant)>; 3]> = std::sync::Mutex::new([None, None, None]);
/// A short message (4 s) under the HUD lines: what showTextBox would say.
pub static NOTE: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);

fn draw_hud(ui: &Ui, font: Option<imgui::FontId>) {
    let [dw, dh] = ui.io().display_size;
    let dl = ui.get_foreground_draw_list();
    let _f = font.map(|f| ui.push_font(f));
    let line_h = ui.text_line_height() * 1.3;
    let draw = |i: usize, text: &str, col: [f32; 4]| {
        let size = ui.calc_text_size(text);
        let p = [(dw - size[0]) / 2.0, dh * 0.18 + line_h * i as f32];
        dl.add_rect([p[0] - 14.0, p[1] - 6.0], [p[0] + size[0] + 14.0, p[1] + size[1] + 6.0], [0.0, 0.0, 0.0, 0.55]).filled(true).rounding(6.0).build();
        dl.add_text(p, col, text);
    };
    let hud = HUD.lock().unwrap().clone();
    for (i, l) in hud.iter().enumerate() {
        if let Some((text, col, at)) = l {
            if at.elapsed() < std::time::Duration::from_millis(250) {
                draw(i, text, *col);
            }
        }
    }
    if let Some((text, at)) = NOTE.lock().unwrap().clone() {
        if at.elapsed() < std::time::Duration::from_secs(4) {
            draw(3, &text, [1.0, 1.0, 1.0, 1.0]);
        }
    }
}

pub fn toggle() {
    let was = OPEN.fetch_xor(true, Ordering::SeqCst);
    crate::log_line(&format!("F9: co-op menu {}", if was { "closed" } else { "opened" }));
}

#[derive(Default)]
pub struct CoopMenu {
    name: String,
    pw: String,
    join_tab: bool,
    settings_tab: bool,
    fonts_ok: bool,
    last_status: i32,
}

fn status_text() -> &'static str {
    match crate::status_code() {
        1 => "Connecting through Steam...",
        2 => "Connected! Load a save or return to the game.",
        3 => "The server refused.",
        5 => "No such session. Check the name.",
        6 => "Wrong password.",
        7 => "Steam is unavailable.",
        8 => "Session is full (4 players).",
        _ => "",
    }
}

/// The game's own menu fonts, taken from this PC's copy of the game (never shipped with the mod):
/// %LOCALAPPDATA%\sa-de-coop\fonts\Beckett.ttf for headings; rows use Windows' Arial Bold like the game's options.
fn font_file(name: &str) -> Option<Vec<u8>> {
    let dir = crate::game::data_dir()?.join("fonts");
    std::fs::read(dir.join(name)).ok()
}

const ROW_A: [f32; 4] = [0.105, 0.110, 0.122, 0.94];
const ROW_B: [f32; 4] = [0.140, 0.145, 0.160, 0.94];
const ROW_HOVER: [f32; 4] = [0.230, 0.235, 0.260, 0.97];
const TAB_ON: [f32; 4] = [0.86, 0.84, 0.95, 1.0]; // the pale lavender of the active tab
const TAB_OFF: [f32; 4] = [0.17, 0.18, 0.19, 0.96];
const ROW_H: f32 = 64.0;

impl ImguiRenderLoop for CoopMenu {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        let ranges = FontGlyphRanges::from_slice(&[0x20, 0x17F, 0]); // Lithuanian letters are in Latin Extended-A
        let leak = |v: Vec<u8>| -> &'static [u8] { Box::leak(v.into_boxed_slice()) };
        let arial = std::fs::read(r"C:\Windows\Fonts\arialbd.ttf").ok().map(leak);
        let beckett = font_file("Beckett.ttf").map(leak);
        let src = |data: &'static [u8], size: f32| FontSource::TtfData {
            data,
            size_pixels: size,
            config: Some(FontConfig { glyph_ranges: ranges.clone(), oversample_h: 2, ..FontConfig::default() }),
        };
        if let (Some(a), Some(b)) = (arial, beckett) {
            ctx.fonts().add_font(&[src(a, 30.0)]); // 0: rows
            ctx.fonts().add_font(&[src(b, 46.0)]); // 1: tabs
            ctx.fonts().add_font(&[src(a, 24.0)]); // 2: status
            self.fonts_ok = true;
        }
        let style = ctx.style_mut();
        style.window_rounding = 0.0;
        style.frame_rounding = 0.0;
        style.window_border_size = 0.0;
        style.frame_border_size = 0.0;
        style.window_padding = [0.0, 0.0];
        style.item_spacing = [0.0, 0.0];
        style.frame_padding = [14.0, 12.0];
        style.colors[StyleColor::WindowBg as usize] = [0.0, 0.0, 0.0, 0.0];
        style.colors[StyleColor::FrameBg as usize] = [0.06, 0.06, 0.07, 1.0];
        style.colors[StyleColor::FrameBgHovered as usize] = [0.09, 0.09, 0.10, 1.0];
        style.colors[StyleColor::FrameBgActive as usize] = [0.09, 0.09, 0.10, 1.0];
        style.colors[StyleColor::Text as usize] = [1.0, 1.0, 1.0, 1.0];
        style.colors[StyleColor::TextSelectedBg as usize] = [0.55, 0.53, 0.75, 0.6];
    }

    fn before_render<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        // in the game the cursor is hidden and held in the middle: draw our own and let the mouse move freely
        let open = OPEN.load(Ordering::SeqCst);
        ctx.io_mut().mouse_draw_cursor = open;
        if open {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ClipCursor(std::ptr::null()) };
        }
    }

    fn render(&mut self, ui: &mut Ui) {
        if !RENDERED.swap(true, Ordering::SeqCst) {
            crate::log_line("overlay: first frame drawn");
        }
        let fonts = ui.fonts().fonts().to_vec();
        draw_tags(ui, if self.fonts_ok && fonts.len() >= 3 { Some(fonts[2]) } else { None });
        draw_hud(ui, if self.fonts_ok && fonts.len() >= 2 { Some(fonts[0]) } else { None });
        if !OPEN.load(Ordering::SeqCst) {
            return;
        }
        let [dw, dh] = ui.io().display_size;
        // dim the game behind, like its own pause screen
        ui.get_background_draw_list().add_rect([0.0, 0.0], [dw, dh], [0.0, 0.0, 0.0, 0.62]).filled(true).build();
        // where the game's options panel sits: right of the menu list
        let (x0, y0, w) = (dw * 0.30, dh * 0.33, dw * 0.62);
        let (row_font, tab_font, small_font) =
            if self.fonts_ok && fonts.len() >= 3 { (Some(fonts[0]), Some(fonts[1]), Some(fonts[2])) } else { (None, None, None) };
        let mut close = false;
        let mut go = false;
        let mut tab_click: Option<usize> = None;
        let join_tab = self.join_tab;
        let settings_tab = self.settings_tab;
        let busy = matches!(crate::status_code(), 1 | 2);
        let can = !busy && !self.name.trim().is_empty();
        let st = status_text();
        let name = &mut self.name;
        let pw = &mut self.pw;
        ui.window("##sacoop")
            .position([x0, y0], Condition::Always)
            .size([w, ROW_H * 10.4], Condition::Always)
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .scroll_bar(false)
            .build(|| {
                let dl = ui.get_window_draw_list();
                let origin = ui.cursor_screen_pos();
                // ---- tabs (Beckett, like "controls / gameplay / graphics")
                let tab_w = w / 3.0;
                let tab_h = ROW_H * 1.15;
                for (i, label) in ["host", "join", "settings"].iter().enumerate() {
                    let on = if settings_tab { i == 2 } else { i < 2 && (i == 1) == join_tab };
                    let p0 = [origin[0] + tab_w * i as f32, origin[1]];
                    let p1 = [p0[0] + tab_w - 4.0, p0[1] + tab_h];
                    dl.add_rect(p0, p1, if on { TAB_ON } else { TAB_OFF }).filled(true).build();
                    ui.set_cursor_screen_pos(p0);
                    if ui.invisible_button(format!("tab{i}"), [tab_w - 4.0, tab_h]) {
                        tab_click = Some(i);
                    }
                    let _f = tab_font.map(|f| ui.push_font(f));
                    let ts = ui.calc_text_size(label);
                    let col = if on { [0.05, 0.05, 0.08, 1.0] } else { [0.92, 0.92, 0.95, 1.0] };
                    dl.add_text([p0[0] + (tab_w - ts[0]) / 2.0, p0[1] + (tab_h - ts[1]) / 2.0], col, label);
                }
                let _rf = row_font.map(|f| ui.push_font(f));
                // ---- rows, like the options list: label left, control right, alternating shades, hover lighter
                let mut y = origin[1] + tab_h + 6.0;
                let field_x = origin[0] + w * 0.48;
                let field_w = w * 0.48;
                let row_bg = |y: f32, idx: usize, label: &str| -> bool {
                    let p0 = [origin[0], y];
                    let p1 = [origin[0] + w, y + ROW_H];
                    let hover = ui.is_mouse_hovering_rect(p0, p1);
                    dl.add_rect(p0, p1, if hover { ROW_HOVER } else if idx % 2 == 0 { ROW_A } else { ROW_B }).filled(true).build();
                    let ts = ui.calc_text_size(label);
                    dl.add_text([p0[0] + 26.0, y + (ROW_H - ts[1]) / 2.0], [1.0, 1.0, 1.0, 1.0], label);
                    hover && ui.is_mouse_clicked(imgui::MouseButton::Left)
                };

                // ---- settings: the launcher's choices; they change game files, so they apply after the game closes
                if settings_tab {
                    let before = crate::modsettings::get();
                    let mut o = before;
                    let value = |y: f32, text: &str| {
                        let ts = ui.calc_text_size(text);
                        dl.add_text([origin[0] + w - ts[0] - 30.0, y + (ROW_H - ts[1]) / 2.0], [1.0, 1.0, 1.0, 1.0], text);
                    };
                    let on_off = |on: bool| if on { "<  On  >" } else { "<  Off  >" };
                    let mut idx = 0;
                    if row_bg(y, idx, "Graphics") {
                        o.gfx = (o.gfx + 1) % crate::modsettings::GFX.len() as u8;
                    }
                    value(y, &format!("<  {}  >", crate::modsettings::GFX[o.gfx as usize]));
                    y += ROW_H;
                    idx += 1;
                    if crate::game::current().kind == crate::game::Kind::Vc {
                        if row_bg(y, idx, "Better car handling") {
                            o.grip = !o.grip;
                        }
                        value(y, on_off(o.grip));
                        y += ROW_H;
                        idx += 1;
                        if row_bg(y, idx, "Faster cars (260 km/h)") {
                            o.fast = !o.fast;
                        }
                        value(y, on_off(o.fast));
                        y += ROW_H;
                        idx += 1;
                    }
                    if crate::game::current().kind == crate::game::Kind::Vc {
                        if row_bg(y, idx, "Tougher enemies") {
                            o.tough = !o.tough;
                        }
                        value(y, on_off(o.tough));
                        y += ROW_H;
                        idx += 1;
                    }
                    if row_bg(y, idx, "ReShade colors") {
                        o.reshade = !o.reshade;
                    }
                    value(y, on_off(o.reshade));
                    y += ROW_H;
                    idx += 1;
                    row_bg(y, idx, "");
                    {
                        let _s = small_font.map(|f| ui.push_font(f));
                        let line = if crate::modsettings::CHANGED.load(Ordering::SeqCst) {
                            "Saved. Applied when you close the game; you'll see it next time you play."
                        } else {
                            "Tougher enemies changes at once. The rest applies after a game restart."
                        };
                        dl.add_text([origin[0] + 26.0, y + 20.0], [0.98, 0.90, 0.40, 1.0], line);
                    }
                    y += ROW_H;
                    idx += 1;
                    if row_bg(y, idx, "Back") {
                        close = true;
                    }
                    if o != before {
                        crate::modsettings::set(o);
                    }
                    return;
                }

                row_bg(y, 0, "Session name");
                ui.set_cursor_screen_pos([field_x, y + (ROW_H - ui.frame_height()) / 2.0]);
                ui.set_next_item_width(field_w);
                ui.input_text("##name", name).build();
                y += ROW_H;

                row_bg(y, 1, "Password");
                ui.set_cursor_screen_pos([field_x, y + (ROW_H - ui.frame_height()) / 2.0]);
                ui.set_next_item_width(field_w);
                ui.input_text("##pw", pw).password(true).build();
                y += ROW_H;

                let names = skin_names();
                let skin = SKIN.load(Ordering::SeqCst) as usize % names.len();
                if row_bg(y, 2, "Look") {
                    SKIN.store(((skin + 1) % names.len()) as u8, Ordering::SeqCst);
                }
                let label = format!("<  {}  >", names[skin]);
                let ts = ui.calc_text_size(&label);
                dl.add_text([origin[0] + w - ts[0] - 30.0, y + (ROW_H - ts[1]) / 2.0], [1.0, 1.0, 1.0, 1.0], &label);
                y += ROW_H;

                // own saves or a separate co-op set (switch before loading a game)
                let coop_saves = crate::saves::mode() == "coop";
                if row_bg(y, 3, "Saves") {
                    let note = match crate::saves::switch_to(if coop_saves { "solo" } else { "coop" }) {
                        Ok(n) => n,
                        Err(e) => e,
                    };
                    *SAVE_NOTE.lock().unwrap() = note;
                }
                let label = if coop_saves { "<  Co-op  >" } else { "<  Mine  >" };
                let ts = ui.calc_text_size(label);
                dl.add_text([origin[0] + w - ts[0] - 30.0, y + (ROW_H - ts[1]) / 2.0], [1.0, 1.0, 1.0, 1.0], label);
                y += ROW_H;

                let go_label = if join_tab { "Join" } else { "Host session" };
                if row_bg(y, 4, go_label) {
                    go = true;
                }
                if can {
                    dl.add_text([origin[0] + w - 50.0, y + 16.0], [1.0, 1.0, 1.0, 1.0], ">");
                }
                y += ROW_H;

                row_bg(y, 5, "");
                {
                    let _s = small_font.map(|f| ui.push_font(f));
                    let note = SAVE_NOTE.lock().unwrap().clone();
                    let line = if st.is_empty() { note.as_str() } else { st };
                    dl.add_text([origin[0] + 26.0, y + 20.0], [0.98, 0.90, 0.40, 1.0], line);
                }
                y += ROW_H;

                // the AI partner is a San Andreas script (ai-partneris.js) only
                if !join_tab && crate::game::current().kind == crate::game::Kind::Sa {
                    let on = crate::ai::ENABLED.load(Ordering::SeqCst);
                    if row_bg(y, 6, "AI partner (Jev)") {
                        crate::ai::ENABLED.store(!on, Ordering::SeqCst);
                    }
                    let label = if on { "<  On  >" } else { "<  Off  >" };
                    let ts = ui.calc_text_size(label);
                    dl.add_text([origin[0] + w - ts[0] - 30.0, y + (ROW_H - ts[1]) / 2.0], [1.0, 1.0, 1.0, 1.0], label);
                    y += ROW_H;
                }
                if !join_tab && row_bg(y, 7, "Test with a bot (no friend)") {
                    // leave any session first, then host over UDP so the test bot can join
                    crate::leave_session();
                    if crate::coop_host() == 1 {
                        crate::log_line("test host started for the bot");
                    }
                }
                if !join_tab {
                    y += ROW_H;
                }
                if row_bg(y, 8, "Back") {
                    close = true;
                }
            });
        if let Some(t) = tab_click {
            self.settings_tab = t == 2;
            if t < 2 {
                self.join_tab = t == 1;
            }
        }
        if go && can && !self.settings_tab {
            crate::start_steam_session(!self.join_tab, self.name.trim().to_string(), self.pw.clone());
        }
        // a session that just connected: back to the game, like Saints Row 2
        let status = crate::status_code();
        if self.last_status == 1 && status == 2 {
            close = true;
        }
        self.last_status = status;
        if close || ui.is_key_pressed(imgui::Key::Escape) {
            OPEN.store(false, Ordering::SeqCst);
        }
    }

    fn message_filter(&self, _io: &imgui::Io) -> MessageFilter {
        if OPEN.load(Ordering::SeqCst) {
            MessageFilter::InputKeyboard | MessageFilter::InputMouse | MessageFilter::InputRaw
        } else {
            MessageFilter::empty()
        }
    }
}

// ---- the game's own menu: catch the "co-op" entry (FunctionToCallOnPress = "COOP") ---------------------------
// UObject::ProcessEvent and the name pool per game (crate::game, static RE, see re/).

type ProcessEvent = unsafe extern "system" fn(*mut u8, *mut u8, *mut u8);
static mut ORIGINAL: Option<ProcessEvent> = None;
static mut BASE: usize = 0;
static mut CFS_FN: usize = 0; // the CustomFunctionFromString UFunction once seen
static LOGGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static mut LAST_IDX: i32 = -1;
static mut MAX_IDX: i32 = -1;
static RENDERED: AtomicBool = AtomicBool::new(false);

fn log_once(text: &str) {
    let mut l = LOGGED.lock().unwrap();
    if l.len() < 200 && !l.iter().any(|x| x == text) {
        crate::log_line(text);
        l.push(text.to_string());
    }
}

unsafe fn class_of(obj: *const u8) -> String {
    let class = *(obj.add(0x10) as *const *const u8);
    if class.is_null() { String::new() } else { fname(class) }
}

unsafe fn outer_of(obj: *const u8) -> String {
    let outer = *(obj.add(0x20) as *const *const u8);
    if outer.is_null() { String::new() } else { format!("{} {}", class_of(outer), fname(outer)) }
}

unsafe fn fname(obj: *const u8) -> String {
    let idx = *(obj.add(0x18) as *const u32);
    let blk = *((BASE + crate::game::current().name_pool + 0x10 + (idx >> 16) as usize * 8) as *const usize);
    if blk == 0 {
        return String::new();
    }
    let e = (blk + (idx & 0xFFFF) as usize * 2) as *const u16;
    let (len, wide) = ((*e >> 6) as usize, *e & 1 != 0);
    if len == 0 || len > 256 {
        return String::new();
    }
    if wide {
        String::from_utf16_lossy(std::slice::from_raw_parts(e.add(1), len))
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(e.add(1) as *const u8, len)).into_owned()
    }
}

// ---- UE 4.26 reflection: walk a class's properties (FProperty list) to find which one says "COOP" ----------
// UStruct: SuperStruct +0x40, ChildProperties +0x50. FField: Class +0x08, Next +0x20, Name +0x28.
// FProperty: Offset_Internal +0x4C. FFieldClass: Name +0x00.

#[allow(dead_code)]
unsafe fn fname_at(p: *const u8) -> String {
    // same pool decoding as fname(), for an FName stored at `p`
    let idx = *(p as *const u32);
    let blk = *((BASE + crate::game::current().name_pool + 0x10 + (idx >> 16) as usize * 8) as *const usize);
    if blk == 0 {
        return String::new();
    }
    let e = (blk + (idx & 0xFFFF) as usize * 2) as *const u16;
    let (len, wide) = ((*e >> 6) as usize, *e & 1 != 0);
    if len == 0 || len > 256 {
        return String::new();
    }
    if wide {
        String::from_utf16_lossy(std::slice::from_raw_parts(e.add(1), len))
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(e.add(1) as *const u8, len)).into_owned()
    }
}

#[allow(dead_code)]
unsafe fn read_fstring(p: *const u8) -> Option<String> {
    let data = *(p as *const *const u16);
    let num = *(p.add(8) as *const i32);
    (!data.is_null() && (1..256).contains(&num)).then(|| String::from_utf16_lossy(std::slice::from_raw_parts(data, num as usize - 1)))
}

/// Every string property of `obj` (own class and parents): (name, value). Logs the full property list once per class.
#[allow(dead_code)]
unsafe fn string_props(obj: *const u8) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut class = *(obj.add(0x10) as *const *const u8);
    let first_class = class_of(obj);
    let mut dump = vec![];
    for _ in 0..6 {
        if class.is_null() {
            break;
        }
        let mut f = *(class.add(0x50) as *const *const u8);
        let mut n = 0;
        while !f.is_null() && n < 400 {
            let fclass = *(f.add(0x08) as *const *const u8);
            let ty = if fclass.is_null() { String::new() } else { fname_at(fclass) };
            let name = fname_at(f.add(0x28));
            let off = *(f.add(0x4C) as *const i32);
            if ty == "StrProperty" && (0..0x4000).contains(&off) {
                if let Some(v) = read_fstring(obj.add(off as usize)) {
                    dump.push(format!("{name}:{ty}@{off:#x}={v}"));
                    out.push((name, v));
                } else {
                    dump.push(format!("{name}:{ty}@{off:#x}"));
                }
            } else {
                dump.push(format!("{name}:{ty}@{off:#x}"));
            }
            f = *(f.add(0x20) as *const *const u8);
            n += 1;
        }
        class = *(class.add(0x40) as *const *const u8);
    }
    log_once(&format!("props of {first_class}: {}", dump.join(", ")));
    out
}

unsafe extern "system" fn detour(this: *mut u8, func: *mut u8, parms: *mut u8) {
    if !func.is_null() {
        let f = func as usize;
        if CFS_FN == 0 || f == CFS_FN {
            let name = fname(func);
            if name == "CustomFunctionFromString" {
                CFS_FN = f;
                if !parms.is_null() {
                    let data = *(parms as *const *const u16);
                    let num = *(parms.add(8) as *const i32);
                    if !data.is_null() && (2..64).contains(&num) {
                        let s = String::from_utf16_lossy(std::slice::from_raw_parts(data, num as usize - 1));
                        if s == "COOP" {
                            OPEN.store(true, Ordering::SeqCst);
                            return; // ours: the menu does not need to hear about it
                        }
                    }
                }
            } else if name == "InteractedButton" && !this.is_null() && !parms.is_null() {
                // the hovered/used entry's position in the visible list; co-op is always next to last (before exit)
                let idx = *(parms as *const i32);
                LAST_IDX = idx;
                crate::log_line(&format!("InteractedButton idx={idx}"));
                if idx > MAX_IDX { MAX_IDX = idx; }
                log_once(&format!("InteractedButton on {} (outer {})", class_of(this), outer_of(this)));
            } else if name.contains("OnButtonClickedEvent") && !this.is_null() {
                // the menu button knows its entry: ValueToBind (+0x318) = index in LeftButtons. Which menu it
                // belongs to comes from its outer chain. co-op sits before exit: main menu 4, pause menu 7.
                let value = *(this.add(0x318) as *const i32);
                let mut menu = String::new();
                let mut o = *(this.add(0x20) as *const *const u8);
                for _ in 0..8 {
                    if o.is_null() {
                        break;
                    }
                    let c = class_of(o);
                    if c.contains("MainMenu") || c.contains("PauseMenu") {
                        menu = c;
                        break;
                    }
                    o = *(o.add(0x20) as *const *const u8);
                }
                let cls = class_of(this);
                // SA, measured 11-08: title menu buttons are UI_Button_MainMenu_SA_C (co-op = 4), pause menu
                // buttons UI_Button_Menu_SA_C (co-op = 7); the owning menu is not reachable through the outer chain
                let g = crate::game::current();
                let main_btn = !g.main_button.is_empty() && cls == g.main_button;
                let pause_btn = !g.pause_button.is_empty() && cls == g.pause_button;
                let hit = (menu.contains("MainMenu") || (menu.is_empty() && main_btn)) && value == g.main_coop
                    || (menu.contains("PauseMenu") || (menu.is_empty() && pause_btn)) && value == g.pause_coop;
                crate::log_line(&format!("click on {cls} value={value} menu={menu} coop={hit}"));
                if hit {
                    OPEN.store(true, Ordering::SeqCst);
                }
            } else if name.contains("Click") || name.contains("Press") || name.contains("Button") || name.contains("Selected") {
                // first run: learn which menu events go through here (coopnet.log), so we can catch the click
                let mut l = LOGGED.lock().unwrap();
                if l.len() < 200 && !l.contains(&name) {
                    crate::log_line(&format!("menu event: {name}"));
                    l.push(name);
                }
            }
        }
    }
    if let Some(orig) = ORIGINAL {
        orig(this, func, parms);
    }
}

/// Install the ProcessEvent hook if the exe is the build we know (checks the function's first bytes).
pub unsafe fn hook_menu() {
    use hudhook::mh::{MH_CreateHook, MH_EnableHook, MH_Initialize, MH_STATUS};
    BASE = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as usize;
    let g = crate::game::current();
    if g.process_event == 0 || g.name_pool == 0 {
        crate::log_line("menu hook: addresses unknown for this game, co-op menu entry disabled (F9 still works)");
        return;
    }
    let target = (BASE + g.process_event) as *mut std::ffi::c_void;
    let expect = g.process_event_prologue;
    if std::slice::from_raw_parts(target as *const u8, expect.len()) != expect {
        crate::log_line("menu hook: unknown game build, co-op menu entry disabled (F9 still works)");
        return;
    }
    let _ = MH_Initialize();
    let mut orig: *mut std::ffi::c_void = std::ptr::null_mut();
    let created = MH_CreateHook(target, detour as *mut std::ffi::c_void, &mut orig);
    if !matches!(created, MH_STATUS::MH_OK) {
        crate::log_line(&format!("menu hook: create failed {created:?}"));
        return;
    }
    ORIGINAL = Some(std::mem::transmute::<*mut std::ffi::c_void, ProcessEvent>(orig));
    let enabled = MH_EnableHook(target);
    crate::log_line(&format!("menu hook: {enabled:?}"));
}
