// THESIS:    one window that finds GTA Vice City / San Andreas DE, installs the co-op mod, and says exactly what it changes.
// AUDIENCE:  players on Windows, wary of strange exes; read once, click once.
// WORLD:     Vice City at night: deep navy sky, an 80s sunset band, neon pink and cyan.
// SIGNATURE: the sunset header with the striped retro sun. Everything else stays quiet.
// SYSTEM:    Segoe UI (Windows' own), navy surfaces, pink = the one primary action, cyan = focus/links, 8 px grid.
#![windows_subsystem = "windows"]

mod icon;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "https://github.com/P1tak4s/sa-de-coop";

// palette
const BG: Color32 = Color32::from_rgb(14, 18, 48);
const PANEL: Color32 = Color32::from_rgb(24, 29, 70);
const LINE: Color32 = Color32::from_rgb(52, 58, 110);
const TEXT: Color32 = Color32::from_rgb(243, 240, 255);
const MUTED: Color32 = Color32::from_rgb(169, 166, 201);
const PINK: Color32 = Color32::from_rgb(255, 79, 163);
const CYAN: Color32 = Color32::from_rgb(61, 224, 245);
const GREEN: Color32 = Color32::from_rgb(91, 228, 155);
const RED: Color32 = Color32::from_rgb(255, 107, 107);

struct Game {
    short: &'static str,
    name: &'static str,
    arg: &'static str,
    exe: &'static str,
    subdir: &'static str,
    steam: &'static str,
    data: &'static str,
}

const GAMES: [Game; 2] = [
    Game {
        short: "Vice City",
        name: "GTA Vice City – The Definitive Edition",
        arg: "vc",
        exe: "ViceCity.exe",
        subdir: r"steamapps\common\GTA Vice City - The Definitive Edition\Gameface\Binaries\Win64",
        steam: "steam://rungameid/1546990",
        data: "vc-de-coop",
    },
    Game {
        short: "San Andreas",
        name: "GTA San Andreas – The Definitive Edition",
        arg: "sa",
        exe: "SanAndreas.exe",
        subdir: r"steamapps\common\GTA San Andreas - The Definitive Edition\Gameface\Binaries\Win64",
        steam: "steam://rungameid/1547000",
        data: "sa-de-coop",
    },
];

fn find_game(g: &Game) -> Option<PathBuf> {
    let steam = PathBuf::from(r"C:\Program Files (x86)\Steam");
    let mut libs = vec![steam.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")) {
        for line in vdf.lines() {
            if let Some(rest) = line.trim().strip_prefix("\"path\"") {
                libs.push(PathBuf::from(rest.trim().trim_matches('"').replace("\\\\", "\\")));
            }
        }
    }
    libs.into_iter().map(|l| l.join(g.subdir)).find(|p| p.join(g.exe).is_file())
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(r"Programs\VC-SA-Coop")
}

fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // no console window
}

fn game_running(g: &Game) -> bool {
    hidden(std::process::Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {}", g.exe), "/NH"]))
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(g.exe))
        .unwrap_or(false)
}

/// The mod's files go to %LOCALAPPDATA%\Programs\VC-SA-Coop (sa-coop.exe stays there for settings later).
fn extract() -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD)).map_err(|e| e.to_string())?;
    if zip.is_empty() {
        return Err("diegiklyje nėra modo failų (blogai sukurtas paketas)".into());
    }
    let root = home();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = f.enclosed_name() else { continue };
        let dst = root.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
            continue;
        }
        std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut out = std::fs::File::create(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// sa-coop.exe <args>; its result is the log file it writes.
fn run_tool(g: &Game, args: &[&str], log: &str) -> Result<String, String> {
    let exe = home().join("sa-coop.exe");
    let logf = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(g.data).join(log);
    let _ = std::fs::remove_file(&logf);
    let st = hidden(std::process::Command::new(&exe).args(args)).status().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(format!("sa-coop.exe {} baigėsi klaida", args.join(" ")));
    }
    Ok(std::fs::read_to_string(&logf).unwrap_or_default())
}

#[derive(Clone, PartialEq)]
enum Phase {
    Welcome,
    Working,
    Done,
    Removed,
    Failed(String),
}

struct Step {
    text: String,
    ok: bool,
}

struct Options {
    graphics: usize,
    handling: bool,
    reshade: bool,
}

struct App {
    found: [Option<PathBuf>; 2],
    which: usize,
    opts: Options,
    phase: Arc<Mutex<Phase>>,
    steps: Arc<Mutex<Vec<Step>>>,
    details: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        setup_style(&cc.egui_ctx);
        let found = [find_game(&GAMES[0]), find_game(&GAMES[1])];
        let which = if found[0].is_none() && found[1].is_some() { 1 } else { 0 };
        App {
            found,
            which,
            opts: Options { graphics: 2, handling: true, reshade: true },
            phase: Arc::new(Mutex::new(Phase::Welcome)),
            steps: Arc::new(Mutex::new(vec![])),
            details: false,
        }
    }

    fn start(&mut self, remove: bool, ctx: &egui::Context) {
        *self.phase.lock().unwrap() = Phase::Working;
        self.steps.lock().unwrap().clear();
        let (phase, steps, ctx) = (self.phase.clone(), self.steps.clone(), ctx.clone());
        let g = &GAMES[self.which];
        let (graphics, handling, reshade) = (self.opts.graphics, self.opts.handling, self.opts.reshade);
        std::thread::spawn(move || {
            let step = |t: String, ok: bool| {
                steps.lock().unwrap().push(Step { text: t, ok });
                ctx.request_repaint();
            };
            let result = (|| -> Result<(), String> {
                if game_running(g) {
                    return Err(format!("{} dabar veikia. Uždaryk žaidimą ir spausk dar kartą.", g.short));
                }
                extract()?;
                step("Modo failai paruošti".into(), true);
                if remove {
                    for l in run_tool(g, &["--uninstall", g.arg], "uninstall.log")?.lines() {
                        step(l.to_string(), true);
                    }
                    return Ok(());
                }
                let log = run_tool(g, &["--install", g.arg], "install.log")?;
                for l in log.lines().filter(|l| !l.is_empty()) {
                    step(l.to_string(), !l.contains("nepavyko"));
                }
                if !log.contains("Modas įdiegtas") {
                    return Err("modo įdiegti nepavyko (žr. aukščiau)".into());
                }
                let gl = run_tool(g, &["--grafika", g.arg, &graphics.to_string()], "grafika.log")?;
                step(gl, true);
                if g.arg == "vc" {
                    let v = run_tool(g, &["--valdymas", "vc", if handling { "1" } else { "0" }], "valdymas.log")?;
                    let ok = !v.contains("nepavyko");
                    step(v, ok);
                }
                let r = run_tool(g, &["--reshade", g.arg, if reshade { "1" } else { "0" }], "reshade.log")?;
                step(r, true);
                Ok(())
            })();
            *phase.lock().unwrap() = match result {
                Ok(()) if remove => Phase::Removed,
                Ok(()) => Phase::Done,
                Err(e) => Phase::Failed(e),
            };
            ctx.request_repaint();
        });
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Windows' own UI font when it is there (never shipped): the installer looks like a normal Windows app
    for (name, file) in [("segoe", r"C:\Windows\Fonts\segoeui.ttf"), ("segoe-bold", r"C:\Windows\Fonts\segoeuib.ttf")] {
        if let Ok(bytes) = std::fs::read(file) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
        }
    }
    if fonts.font_data.contains_key("segoe") {
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    if fonts.font_data.contains_key("segoe-bold") {
        fonts.families.insert(egui::FontFamily::Name("bold".into()), vec!["segoe-bold".into(), "segoe".into()]);
    } else {
        let p = fonts.families[&egui::FontFamily::Proportional].clone();
        fonts.families.insert(egui::FontFamily::Name("bold".into()), p);
    }
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = CYAN;
    v.selection.bg_fill = PINK;
    v.selection.stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.bg_fill = PANEL;
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(36, 42, 96);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, CYAN);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(44, 50, 112);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
    });
}

fn bold(text: &str, size: f32) -> RichText {
    RichText::new(text).font(egui::FontId::new(size, egui::FontFamily::Name("bold".into())))
}

/// The sunset header: sky gradient, the striped retro sun, the title.
fn header(ui: &mut egui::Ui, subtitle: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 168.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let stops = [
        (0.0, Color32::from_rgb(14, 18, 48)),
        (0.45, Color32::from_rgb(88, 24, 110)),
        (0.8, Color32::from_rgb(232, 60, 140)),
        (1.0, Color32::from_rgb(255, 150, 90)),
    ];
    let mut mesh = egui::epaint::Mesh::default();
    for w in stops.windows(2) {
        let (y0, y1) = (rect.top() + rect.height() * w[0].0, rect.top() + rect.height() * w[1].0);
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(rect.left(), y0), w[0].1);
        mesh.colored_vertex(Pos2::new(rect.right(), y0), w[0].1);
        mesh.colored_vertex(Pos2::new(rect.left(), y1), w[1].1);
        mesh.colored_vertex(Pos2::new(rect.right(), y1), w[1].1);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    p.add(egui::Shape::mesh(mesh));
    // the sun: horizontal slices, yellow to pink, gaps below its middle
    let (c, r) = (Pos2::new(rect.right() - 96.0, rect.bottom() - 40.0), 62.0);
    let mut y = c.y - r;
    while y < rect.bottom() {
        let dy = y - c.y;
        let half = (r * r - dy * dy).max(0.0).sqrt();
        let below = dy > 0.0;
        let stripe = below && (((dy / 6.0) as i32) % 2 == 1);
        if half > 0.0 && !stripe {
            let t = ((y - (c.y - r)) / r).clamp(0.0, 1.0);
            let col = Color32::from_rgb(255, (214.0 - 150.0 * t) as u8, (70.0 + 70.0 * t) as u8);
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - half, y), Pos2::new(c.x + half, y + 2.0)), 0.0, col);
        }
        y += 2.0;
    }
    p.text(Pos2::new(rect.left() + 24.0, rect.top() + 34.0), egui::Align2::LEFT_TOP, "VICE CITY", egui::FontId::new(34.0, egui::FontFamily::Name("bold".into())), TEXT);
    p.text(Pos2::new(rect.left() + 26.0, rect.top() + 74.0), egui::Align2::LEFT_TOP, "CO-OP", egui::FontId::new(22.0, egui::FontFamily::Name("bold".into())), CYAN);
    p.text(Pos2::new(rect.left() + 26.0, rect.top() + 112.0), egui::Align2::LEFT_TOP, subtitle, egui::FontId::proportional(14.0), TEXT);
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let b = egui::Button::new(bold(text, 16.0).color(Color32::WHITE)).fill(if enabled { PINK } else { LINE }).min_size(Vec2::new(0.0, 46.0)).corner_radius(CornerRadius::same(10));
    ui.vertical_centered_justified(|ui| ui.add_enabled(enabled, b).clicked()).inner
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let phase = self.phase.lock().unwrap().clone();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ctx, |ui| {
            let sub = match phase {
                Phase::Done => "Paruošta. Liko tik paleisti žaidimą.",
                Phase::Removed => "Modas pašalintas.",
                _ => "Draugas tavo Vice City ar San Andreas pasaulyje",
            };
            header(ui, sub);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 16)).show(ui, |ui| match &phase {
                Phase::Welcome => self.welcome(ui, ctx),
                Phase::Working => self.progress(ui, None),
                Phase::Failed(e) => {
                    self.progress(ui, Some(e.clone()));
                    ui.add_space(8.0);
                    if primary(ui, "Bandyti dar kartą", true) {
                        *self.phase.lock().unwrap() = Phase::Welcome;
                    }
                }
                Phase::Done => self.done(ui),
                Phase::Removed => {
                    self.progress(ui, None);
                    ui.add_space(8.0);
                    if primary(ui, "Uždaryti", true) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });
        });
    }
}

impl App {
    fn welcome(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let any = self.found.iter().any(|f| f.is_some());
        card(ui, |ui| {
            ui.label(bold("Žaidimas", 13.0).color(MUTED));
            if !any {
                ui.colored_label(RED, "Steam bibliotekose neradau nei Vice City, nei San Andreas Definitive Edition.");
                ui.small("Reikia legalios Steam versijos. Įdiek žaidimą ir paleisk šį diegiklį dar kartą.");
                return;
            }
            ui.horizontal(|ui| {
                for (i, g) in GAMES.iter().enumerate() {
                    if self.found[i].is_some() {
                        ui.selectable_value(&mut self.which, i, bold(g.short, 15.0));
                    }
                }
            });
            if let Some(dir) = &self.found[self.which] {
                ui.colored_label(GREEN, format!("✔ {}", GAMES[self.which].name));
                ui.label(RichText::new(dir.display().to_string()).small().color(MUTED));
            }
        });
        if !any {
            return;
        }
        ui.add_space(4.0);
        card(ui, |ui| {
            ui.label(bold("Nustatymai", 13.0).color(MUTED));
            ui.horizontal(|ui| {
                ui.label("Grafika");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (i, n) in ["Maksimum", "Gražu", "Paprasta", "Originali"].iter().enumerate().map(|(i, n)| (3 - i, n)) {
                        ui.selectable_value(&mut self.opts.graphics, i, *n);
                    }
                });
            });
            if self.opts.graphics == 3 {
                ui.small("Maksimum reikia stiprios vaizdo plokštės.");
            }
            if GAMES[self.which].arg == "vc" {
                ui.checkbox(&mut self.opts.handling, "Geresnis mašinų valdymas (daugiau sukibimo, ABS)");
            }
            ui.checkbox(&mut self.opts.reshade, "ReShade spalvos (Home: meniu, Scroll Lock: išjungti)");
        });
        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new("Ką tiksliai pakeis").color(CYAN)).default_open(self.details).show(ui, |ui| {
            ui.small("• Žaidimo aplanke Gameface\\Binaries\\Win64: version.dll (ASI įkėliklis), cleo_redux64.asi, coopnet64.asi, CLEO aplankas.");
            ui.small("• Gameface\\Content\\Paks\\~mods: co-op meniu įrašas (sukuriamas iš tavo žaidimo failų) ir, jei pažymėta, valdymas.");
            ui.small("• Documents\\Rockstar Games: Engine.ini ir CustomSettings.ini grafikos eilutės (tavo paties failas neliečiamas).");
            ui.small("• %LOCALAPPDATA%\\Programs\\VC-SA-Coop: nustatymų programa sa-coop.exe.");
            ui.small("Administratoriaus teisių nereikia. Viską atšauks mygtukas „Pašalinti“.");
        });
        ui.add_space(10.0);
        let g = &GAMES[self.which];
        if primary(ui, &format!("Įdiegti į {}", g.short), true) {
            self.start(false, ctx);
        }
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Pašalinti").color(MUTED)).frame(false)).clicked() {
                self.start(true, ctx);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.hyperlink_to(RichText::new("Atviras kodas").small(), REPO);
                ui.small(format!("v{VERSION} · MIT"));
            });
        });
        ui.small(RichText::new("Neoficialus modas. Ne Rockstar Games ir ne Take-Two produktas.").color(MUTED));
    }

    fn progress(&self, ui: &mut egui::Ui, error: Option<String>) {
        card(ui, |ui| {
            for s in self.steps.lock().unwrap().iter() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(if s.ok { GREEN } else { RED }, if s.ok { "✔" } else { "✖" });
                    ui.label(&s.text);
                });
            }
            match &error {
                Some(e) => {
                    ui.colored_label(RED, e);
                }
                None if *self.phase.lock().unwrap() == Phase::Working => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Diegiama...").color(MUTED));
                    });
                }
                None => {}
            }
        });
    }

    fn done(&self, ui: &mut egui::Ui) {
        self.progress(ui, None);
        let g = &GAMES[self.which];
        ui.add_space(4.0);
        card(ui, |ui| {
            ui.label(bold("Kaip žaisti kartu", 13.0).color(MUTED));
            ui.label("1. Paleisk žaidimą ir įkelk išsaugojimą.");
            ui.label("2. ESC, tada CO-OP. Sugalvok sesijos pavadinimą ir slaptažodį.");
            ui.label("3. Draugas įveda tą patį pavadinimą ir slaptažodį.");
            ui.small("G: sėsti į draugo mašiną · E: atgaivinti · F: išlipti");
        });
        ui.add_space(10.0);
        if primary(ui, &format!("Paleisti {}", g.short), true) {
            let _ = hidden(std::process::Command::new("cmd").args(["/C", "start", "", g.steam])).spawn();
        }
        ui.small(RichText::new("Nustatymus pakeisi paleidęs šį diegiklį dar kartą.").color(MUTED));
    }
}

fn main() -> eframe::Result {
    let px = icon::draw(64);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vice City Co-op diegimas")
            .with_inner_size([520.0, 700.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(Arc::new(egui::IconData { rgba: px, width: 64, height: 64 })),
        ..Default::default()
    };
    eframe::run_native("Vice City Co-op diegimas", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

