//! The co-op menu drawn inside the game (Dear ImGui through hudhook, DirectX 11 or 12), from the title screen on.
//! Opened by the "co-op" entry of the game's own menus (see menu_hook) or by F9. While it is open the game gets
//! no mouse or keyboard input.

use hudhook::imgui::{self, Condition, FontConfig, FontGlyphRanges, FontSource, StyleColor, Ui};
use hudhook::{ImguiRenderLoop, MessageFilter, RenderContext};
use std::sync::atomic::{AtomicBool, Ordering};

pub static OPEN: AtomicBool = AtomicBool::new(false);

pub fn toggle() {
    OPEN.fetch_xor(true, Ordering::SeqCst);
}

#[derive(Default)]
pub struct CoopMenu {
    name: String,
    pw: String,
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
        _ => "Sugalvok sesijos pavadinimą ir slaptažodį ir pasakyk juos draugui.",
    }
}

impl ImguiRenderLoop for CoopMenu {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _rc: &'a mut dyn RenderContext) {
        // Lithuanian letters need Latin Extended-A; Segoe UI ships with every Windows
        if let Ok(ttf) = std::fs::read(r"C:\Windows\Fonts\segoeuib.ttf").or_else(|_| std::fs::read(r"C:\Windows\Fonts\segoeui.ttf")) {
            let data: &'static [u8] = Box::leak(ttf.into_boxed_slice());
            let ranges = FontGlyphRanges::from_slice(&[0x20, 0x17F, 0]);
            let cfg = |size: f32| FontSource::TtfData { data, size_pixels: size, config: Some(FontConfig { glyph_ranges: ranges.clone(), ..FontConfig::default() }) };
            ctx.fonts().add_font(&[cfg(22.0)]);
            ctx.fonts().add_font(&[cfg(40.0)]); // the title, second font
        }
        let style = ctx.style_mut();
        style.window_rounding = 0.0;
        style.frame_rounding = 0.0;
        style.window_border_size = 0.0;
        style.frame_padding = [12.0, 8.0];
        style.item_spacing = [10.0, 12.0];
        style.window_padding = [28.0, 24.0];
        style.colors[StyleColor::WindowBg as usize] = [0.0, 0.0, 0.0, 0.88];
        style.colors[StyleColor::FrameBg as usize] = [0.12, 0.12, 0.12, 1.0];
        style.colors[StyleColor::FrameBgHovered as usize] = [0.18, 0.18, 0.18, 1.0];
        style.colors[StyleColor::Button as usize] = [0.10, 0.10, 0.10, 1.0];
        // Saints Row 2 style: the chosen entry is bright yellow
        style.colors[StyleColor::ButtonHovered as usize] = [0.98, 0.90, 0.10, 1.0];
        style.colors[StyleColor::ButtonActive as usize] = [1.0, 0.95, 0.35, 1.0];
        style.colors[StyleColor::Separator as usize] = [0.75, 0.80, 0.95, 1.0];
        style.colors[StyleColor::Text as usize] = [1.0, 1.0, 1.0, 1.0];
    }

    fn render(&mut self, ui: &mut Ui) {
        if !OPEN.load(Ordering::SeqCst) {
            return;
        }
        let [dw, dh] = ui.io().display_size;
        let mut keep_open = true;
        ui.window("##sacoop")
            .position([dw * 0.5, dh * 0.5], Condition::Always)
            .position_pivot([0.5, 0.5])
            .size([560.0, 0.0], Condition::Always)
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .collapsible(false)
            .build(|| {
                let fonts = ui.fonts().fonts().to_vec();
                if let Some(&big) = fonts.get(1) {
                    let _f = ui.push_font(big);
                    ui.text("CO-OP");
                } else {
                    ui.text("CO-OP");
                }
                ui.separator();
                ui.text("Sesijos pavadinimas");
                ui.set_next_item_width(-1.0);
                ui.input_text("##name", &mut self.name).build();
                ui.text("Slaptažodis");
                ui.set_next_item_width(-1.0);
                ui.input_text("##pw", &mut self.pw).password(true).build();
                let w = (ui.content_region_avail()[0] - 10.0) / 2.0;
                let busy = matches!(crate::status_code(), 1 | 2);
                if ui.button_with_size("SUKURTI", [w, 44.0]) && !busy && !self.name.trim().is_empty() {
                    crate::start_steam_session(true, self.name.trim().to_string(), self.pw.clone());
                }
                ui.same_line();
                if ui.button_with_size("PRISIJUNGTI", [w, 44.0]) && !busy && !self.name.trim().is_empty() {
                    crate::start_steam_session(false, self.name.trim().to_string(), self.pw.clone());
                }
                ui.text_wrapped(status_text());
                if ui.button_with_size("UŽDARYTI", [-1.0, 40.0]) {
                    keep_open = false;
                }
            });
        if !keep_open || ui.is_key_pressed(imgui::Key::Escape) {
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
                // the menu widget (UUI_Menu_Base) tells which entry was used: look it up in FunctionToCallOnPress
                let idx = *(parms as *const i32);
                let arr = *(this.add(0x258) as *const *const u8);
                let num = *(this.add(0x260) as *const i32);
                crate::log_line(&format!("InteractedButton idx={idx} entries={num}"));
                if !arr.is_null() && idx >= 0 && idx < num && num < 32 {
                    let e = arr.add(idx as usize * 16);
                    let data = *(e as *const *const u16);
                    let n = *(e.add(8) as *const i32);
                    if !data.is_null() && (2..64).contains(&n) {
                        let s = String::from_utf16_lossy(std::slice::from_raw_parts(data, n as usize - 1));
                        crate::log_line(&format!("  entry = {s}"));
                        if s == "COOP" {
                            OPEN.store(true, Ordering::SeqCst);
                        }
                    }
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
