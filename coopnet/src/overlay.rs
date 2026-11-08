//! The co-op menu drawn inside the game (Dear ImGui through hudhook, DirectX 11 or 12), from the title screen on.
//! Opened by the "co-op" entry of the game's own menus (see menu_hook) or by F9. While it is open the game gets
//! no mouse or keyboard input.

use hudhook::imgui::{self, Condition, FontConfig, FontGlyphRanges, FontSource, StyleColor, Ui};
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};
use std::sync::atomic::{AtomicBool, Ordering};

pub static OPEN: AtomicBool = AtomicBool::new(false);

pub fn toggle() {
    let was = OPEN.fetch_xor(true, Ordering::SeqCst);
    crate::log_line(&format!("F9: co-op menu {}", if was { "closed" } else { "opened" }));
}

#[derive(Default)]
pub struct CoopMenu {
    name: String,
    pw: String,
    join_tab: bool,
    fonts_ok: bool,
}

fn status_text() -> &'static str {
    match crate::status_code() {
        1 => "Jungiamasi per Steam...",
        2 => "Prisijungta! Įkelk išsaugojimą arba grįžk į žaidimą.",
        3 => "Serveris nepriėmė.",
        5 => "Tokios sesijos nėra. Patikrink pavadinimą.",
        6 => "Neteisingas slaptažodis.",
        7 => "Steam nepasiekiamas.",
        8 => "Sesija pilna (4 žaidėjai).",
        _ => "",
    }
}

/// The game's own menu fonts, taken from this PC's copy of the game (never shipped with the mod):
/// %LOCALAPPDATA%\sa-de-coop\fonts\Beckett.ttf for headings; rows use Windows' Arial Bold like the game's options.
fn font_file(name: &str) -> Option<Vec<u8>> {
    let dir = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("sa-de-coop").join("fonts");
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

    fn render(&mut self, ui: &mut Ui) {
        if !RENDERED.swap(true, Ordering::SeqCst) {
            crate::log_line("overlay: first frame drawn");
        }
        if !OPEN.load(Ordering::SeqCst) {
            return;
        }
        let [dw, dh] = ui.io().display_size;
        // where the game's options panel sits: right of the menu list
        let (x0, y0, w) = (dw * 0.30, dh * 0.33, dw * 0.62);
        let fonts = ui.fonts().fonts().to_vec();
        let (row_font, tab_font, small_font) =
            if self.fonts_ok && fonts.len() >= 3 { (Some(fonts[0]), Some(fonts[1]), Some(fonts[2])) } else { (None, None, None) };
        let mut close = false;
        let mut go = false;
        let mut tab_click: Option<bool> = None;
        let join_tab = self.join_tab;
        let busy = matches!(crate::status_code(), 1 | 2);
        let can = !busy && !self.name.trim().is_empty();
        let st = status_text();
        let name = &mut self.name;
        let pw = &mut self.pw;
        ui.window("##sacoop")
            .position([x0, y0], Condition::Always)
            .size([w, ROW_H * 6.4], Condition::Always)
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .scroll_bar(false)
            .build(|| {
                let dl = ui.get_window_draw_list();
                let origin = ui.cursor_screen_pos();
                // ---- tabs (Beckett, like "controls / gameplay / graphics")
                let tab_w = w / 2.0;
                let tab_h = ROW_H * 1.15;
                for (i, label) in ["sukurti", "prisijungti"].iter().enumerate() {
                    let on = (i == 1) == join_tab;
                    let p0 = [origin[0] + tab_w * i as f32, origin[1]];
                    let p1 = [p0[0] + tab_w - 4.0, p0[1] + tab_h];
                    dl.add_rect(p0, p1, if on { TAB_ON } else { TAB_OFF }).filled(true).build();
                    ui.set_cursor_screen_pos(p0);
                    if ui.invisible_button(format!("tab{i}"), [tab_w - 4.0, tab_h]) {
                        tab_click = Some(i == 1);
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

                row_bg(y, 0, "Sesijos pavadinimas");
                ui.set_cursor_screen_pos([field_x, y + 8.0]);
                ui.set_next_item_width(field_w);
                ui.input_text("##name", name).build();
                y += ROW_H;

                row_bg(y, 1, "Slaptažodis");
                ui.set_cursor_screen_pos([field_x, y + 8.0]);
                ui.set_next_item_width(field_w);
                ui.input_text("##pw", pw).password(true).build();
                y += ROW_H;

                let go_label = if join_tab { "Prisijungti" } else { "Sukurti sesiją" };
                if row_bg(y, 2, go_label) {
                    go = true;
                }
                if can {
                    dl.add_text([origin[0] + w - 50.0, y + 16.0], [1.0, 1.0, 1.0, 1.0], ">");
                }
                y += ROW_H;

                row_bg(y, 3, "");
                {
                    let _s = small_font.map(|f| ui.push_font(f));
                    dl.add_text([origin[0] + 26.0, y + 20.0], [0.98, 0.90, 0.40, 1.0], st);
                }
                y += ROW_H;

                if row_bg(y, 4, "Atgal") {
                    close = true;
                }
            });
        if let Some(t) = tab_click {
            self.join_tab = t;
        }
        if go && can {
            crate::start_steam_session(!self.join_tab, self.name.trim().to_string(), self.pw.clone());
        }
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
// UObject::ProcessEvent and the name pool for Steam build 1.0.113.49697469 (static RE, see re/).

const PROCESS_EVENT: usize = 0x1C7C9C0;
const NAME_POOL: usize = 0x570DDC0;

type ProcessEvent = unsafe extern "system" fn(*mut u8, *mut u8, *mut u8);
static mut ORIGINAL: Option<ProcessEvent> = None;
static mut BASE: usize = 0;
static mut CFS_FN: usize = 0; // the CustomFunctionFromString UFunction once seen
static LOGGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static mut LAST_IDX: i32 = -1;
static mut MAX_IDX: i32 = -1;
static mut MENU_IS_MAIN: bool = false;
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
    let blk = *((BASE + NAME_POOL + 0x10 + (idx >> 16) as usize * 8) as *const usize);
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
                MENU_IS_MAIN = class_of(this) == "UI_MainMenu_C";
                crate::log_line(&format!("InteractedButton idx={idx}"));
                if idx > MAX_IDX { MAX_IDX = idx; }
                log_once(&format!("InteractedButton on {} (outer {})", class_of(this), outer_of(this)));
            } else if name.contains("OnButtonClickedEvent") && !this.is_null() {
                let (li, mi) = (LAST_IDX, MAX_IDX);
                crate::log_line(&format!("click on {} idx={li} max={mi}", class_of(this)));
                // measured 11-08: the title menu's co-op entry reports index 2 (the numbering skips hidden entries)
                if MENU_IS_MAIN && li == 2 {
                    OPEN.store(true, Ordering::SeqCst);
                    crate::log_line("co-op entry: opening the co-op menu");
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
    let target = (BASE + PROCESS_EVENT) as *mut std::ffi::c_void;
    let expect = [0x40u8, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC];
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
