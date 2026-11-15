// THESIS:    install the co-op mod into the game it finds, say exactly what changes, then start the game.
// AUDIENCE:  players on Windows who are rightly wary of unknown exes.
// WORLD:     a game launcher (Rockstar Games Launcher feel): near black, white uppercase headings, one yellow accent.
// SIGNATURE: the game's own hero art and logo at the top, read from this PC's Steam library cache (never shipped).
// SYSTEM:    Segoe UI, greys on #0D0D0D, yellow only on the primary button and the active tab, 8 px grid, 4 px radius.

use super::*;
use eframe::egui::{Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Vec2};
use std::sync::Mutex;

const BG: Color32 = Color32::from_rgb(13, 13, 13);
const CARD: Color32 = Color32::from_rgb(23, 23, 23);
const LINE: Color32 = Color32::from_rgb(42, 42, 42);
const TEXT: Color32 = Color32::from_rgb(245, 245, 245);
const MUTED: Color32 = Color32::from_rgb(154, 154, 154);
const ACCENT: Color32 = Color32::from_rgb(252, 175, 23);
const OK: Color32 = Color32::from_rgb(108, 203, 95);
const BAD: Color32 = Color32::from_rgb(255, 120, 120);
const REPO: &str = "https://github.com/P1tak4s/vc-sa-coop";

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

#[derive(Clone, PartialEq)]
enum Work {
    Idle,
    Busy(&'static str),
    Done(bool),
}

pub struct App {
    which: usize,
    games: [Option<PathBuf>; 2],
    graphics: usize,
    handling: bool,
    fast: bool,
    tough: bool,
    reshade: bool,
    upd: Arc<Mutex<Upd>>,
    work: Arc<Mutex<Work>>,
    log: Arc<Mutex<Vec<(bool, String)>>>,
    art: [Option<(Option<egui::TextureHandle>, Option<egui::TextureHandle>)>; 2],
    nick: String,
    tab: Tab,
    code: String,
    hosting: Option<Hosting>,
    msg: Vec<String>,
}

pub fn icon() -> egui::IconData {
    egui::IconData { rgba: crate::icon::draw(64), width: 64, height: 64 }
}

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, file) in [("segoe", r"C:\Windows\Fonts\segoeui.ttf"), ("segoe-sb", r"C:\Windows\Fonts\seguisb.ttf")] {
        if let Ok(bytes) = std::fs::read(file) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
        }
    }
    let prop = fonts.families[&egui::FontFamily::Proportional].clone();
    if fonts.font_data.contains_key("segoe") {
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    let strong = if fonts.font_data.contains_key("segoe-sb") { vec!["segoe-sb".to_string()] } else { prop };
    fonts.families.insert(egui::FontFamily::Name("strong".into()), strong);
    ctx.set_fonts(fonts);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    // text colours per widget state (no global override: the selected (yellow) button needs black text)
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.fg_stroke = Stroke::new(1.0_f32, TEXT);
    }
    v.hyperlink_color = Color32::from_rgb(120, 180, 255);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0_f32, Color32::BLACK);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(38, 38, 38);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(66, 66, 66);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(75, 75, 75);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 6.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
    });
}

fn strong(t: &str, size: f32) -> RichText {
    RichText::new(t).font(egui::FontId::new(size, egui::FontFamily::Name("strong".into())))
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(strong(&title.to_uppercase(), 12.5).color(MUTED));
        add(ui)
    });
}

/// Steam's own library art for the game on this PC: (hero, logo). Read at runtime, never shipped with the mod.
fn steam_art(ctx: &egui::Context, g: &GameInfo) -> (Option<egui::TextureHandle>, Option<egui::TextureHandle>) {
    let id = g.steam.rsplit('/').next().unwrap_or("");
    let dir = PathBuf::from(r"C:\Program Files (x86)\Steam\appcache\librarycache").join(id);
    let load = |file: &str| -> Option<egui::TextureHandle> {
        let img = image::open(dir.join(file)).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
        Some(ctx.load_texture(format!("{id}-{file}"), ci, egui::TextureOptions::LINEAR))
    };
    (load("library_hero.jpg"), load("logo.png"))
}

/// The top of the window: the game's hero art (cropped to fill), a fade into the page, the game's logo.
fn hero(ui: &mut egui::Ui, art: &(Option<egui::TextureHandle>, Option<egui::TextureHandle>), g: &GameInfo) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 200.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    match &art.0 {
        Some(tex) => {
            let [tw, th] = tex.size();
            let (ta, ra) = (tw as f32 / th as f32, rect.width() / rect.height());
            // cover: crop the longer side, keep the centre
            let uv = if ta > ra {
                let w = ra / ta;
                Rect::from_min_max(Pos2::new(0.5 - w / 2.0, 0.0), Pos2::new(0.5 + w / 2.0, 1.0))
            } else {
                let h = ta / ra;
                Rect::from_min_max(Pos2::new(0.0, 0.5 - h / 2.0), Pos2::new(1.0, 0.5 + h / 2.0))
            };
            p.image(tex.id(), rect, uv, Color32::WHITE);
        }
        None => {
            p.rect_filled(rect, 0.0, Color32::from_rgb(30, 18, 40));
        }
    }
    // fade the bottom into the page
    let fade = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 90.0), rect.max);
    let mut mesh = egui::epaint::Mesh::default();
    mesh.colored_vertex(fade.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(fade.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(fade.left_bottom(), BG);
    mesh.colored_vertex(fade.right_bottom(), BG);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    p.add(egui::Shape::mesh(mesh));
    match &art.1 {
        Some(logo) => {
            let [lw, lh] = logo.size();
            let h = 70.0_f32;
            let w = (lw as f32 / lh as f32 * h).min(rect.width() * 0.6);
            let at = Rect::from_min_size(Pos2::new(rect.left() + 24.0, rect.bottom() - h - 18.0), Vec2::new(w, w / (lw as f32 / lh as f32)));
            p.image(logo.id(), at, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.text(Pos2::new(rect.left() + 24.0, rect.bottom() - 30.0), egui::Align2::LEFT_BOTTOM, g.name.to_uppercase(), egui::FontId::new(22.0, egui::FontFamily::Name("strong".into())), TEXT);
        }
    }
}

/// The bar above the art: our icon and name, the game tabs on the right.
#[allow(dead_code)]
fn title(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let tex = ui.ctx().load_texture("icon", egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::draw(64)), Default::default());
        ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(36.0)));
        ui.vertical(|ui| {
            ui.label(strong("VC / SA Co-op", 19.0));
            ui.label(RichText::new(format!("Vice City and San Andreas Definitive Edition co-op mod · v{}", env!("CARGO_PKG_VERSION"))).small().color(MUTED));
        });
    });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 3.0), egui::Sense::hover());
    let stops = [Color32::from_rgb(255, 196, 80), Color32::from_rgb(240, 80, 140), Color32::from_rgb(120, 60, 200)];
    let mut mesh = egui::epaint::Mesh::default();
    for (k, w) in stops.windows(2).enumerate() {
        let (x0, x1) = (rect.left() + rect.width() * k as f32 / 2.0, rect.left() + rect.width() * (k + 1) as f32 / 2.0);
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(x0, rect.top()), w[0]);
        mesh.colored_vertex(Pos2::new(x1, rect.top()), w[1]);
        mesh.colored_vertex(Pos2::new(x0, rect.bottom()), w[0]);
        mesh.colored_vertex(Pos2::new(x1, rect.bottom()), w[1]);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let _ = Rect::NOTHING;
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> App {
        style(&cc.egui_ctx);
        let games = [find_game(&GAMES[0]), find_game(&GAMES[1])];
        // Vice City first when it is there
        let which = if games[1].is_some() || games[0].is_none() { 1 } else { 0 };
        let mut app = App {
            which,
            games,
            graphics: 2,
            handling: false, // 11-15: off by default, the tuned cars felt slow and loose in the first real test
            fast: false,
            tough: false,
            reshade: true,
            upd: Arc::new(Mutex::new(Upd::Idle)),
            work: Arc::new(Mutex::new(Work::Idle)),
            log: Arc::new(Mutex::new(vec![])),
            art: [None, None],
            nick: "Player".into(),
            tab: Tab::Host,
            code: String::new(),
            hosting: None,
            msg: vec![],
        };
        app.read_current();
        if let Ok(me) = std::env::current_exe() {
            let _ = std::fs::remove_file(me.with_extension("exe.old")); // left by the last update
        }
        start_update(app.upd.clone(), cc.egui_ctx.clone());
        // just updated: put the new mod files into the game too (says so if the game is running)
        if std::env::args().any(|a| a == "--updated") && app.games[app.which].as_ref().is_some_and(|d| installed(&GAMES[app.which], d)) {
            app.run(false, &cc.egui_ctx);
        }
        app
    }

    /// Show what is installed now (re-running the app shows the real state, not defaults).
    fn read_current(&mut self) {
        let g = &GAMES[self.which];
        if let Some(dir) = &self.games[self.which] {
            if installed(g, dir) {
                self.graphics = graphics_level(g).unwrap_or(1);
                let pak = handling_pak(dir).is_some_and(|p| p.is_file());
                let saved = load_settings(g);
                self.fast = pak && saved.is_some_and(|s| s.fast);
                self.tough = saved.is_some_and(|s| s.tough);
                // a pak from before settings.txt existed was the "better" one
                self.handling = pak && saved.map_or(true, |s| s.grip || !s.fast);
                self.reshade = reshade_on(dir) == Some(true);
            }
        }
    }

    fn run(&self, remove: bool, ctx: &egui::Context) {
        let Some(dir) = self.games[self.which].clone() else { return };
        let (work, log, ctx) = (self.work.clone(), self.log.clone(), ctx.clone());
        let s = Settings { gfx: self.graphics, grip: self.handling, fast: self.fast, reshade: self.reshade, tough: self.tough };
        let which = self.which;
        log.lock().unwrap().clear();
        *work.lock().unwrap() = Work::Busy(if remove { "Removing..." } else { "Installing..." });
        std::thread::spawn(move || {
            let g = &GAMES[which];
            let push = |ok: bool, t: String| {
                log.lock().unwrap().push((ok, t));
                ctx.request_repaint();
            };
            let ok = (|| {
                if game_running(g) {
                    push(false, format!("{} is running: close the game and try again.", g.name));
                    return false;
                }
                if remove {
                    for l in uninstall(g, &dir) {
                        push(true, l);
                    }
                    return true;
                }
                match install(g, &dir) {
                    Ok(lines) => lines.into_iter().for_each(|l| push(!l.contains("failed"), l)),
                    Err(e) => {
                        push(false, e);
                        return false;
                    }
                }
                for (ok, l) in apply_settings(g, &dir, s) {
                    push(ok, l);
                }
                true
            })();
            *work.lock().unwrap() = Work::Done(ok);
            ctx.request_repaint();
        });
    }

    fn host(&mut self) {
        let sock = match UdpSocket::bind(("0.0.0.0", DEFAULT_PORT)) {
            Ok(s) => s,
            Err(e) => return self.msg.push(format!("Could not open port {DEFAULT_PORT}: {e}")),
        };
        let key = random_key();
        let players: Players = Arc::default();
        let p2 = players.clone();
        std::thread::spawn(move || run_server(sock, key, p2));
        let internet_code = upnp(DEFAULT_PORT).map(|ip| make_code(ip.octets(), DEFAULT_PORT, key));
        let lan_code = lan_ip().map(|ip| make_code(ip.octets(), DEFAULT_PORT, key));
        if internet_code.is_none() {
            self.msg.push(format!("The router did not open the port (UPnP). For a friend over the internet, open UDP {DEFAULT_PORT} manually."));
        }
        if let Err(e) = write_session(&GAMES[self.which], SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)), key, &self.nick) {
            self.msg.push(format!("Could not save the session: {e}"));
        }
        self.hosting = Some(Hosting { internet_code, lan_code, players });
    }

    fn join(&mut self) -> bool {
        let Some((ip, port, key)) = parse_code(&self.code) else {
            self.msg.push("Invalid code: it must have 16 characters.".into());
            return false;
        };
        match write_session(&GAMES[self.which], SocketAddr::from((ip, port)), key, &self.nick) {
            Ok(()) => true,
            Err(e) => {
                self.msg.push(format!("Could not save the session: {e}"));
                false
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(500));
        let work = self.work.lock().unwrap().clone();
        let upd = self.upd.lock().unwrap().clone();
        // updated and nothing running: start the new version in our place
        if let (Upd::Done(_), false) = (&upd, matches!(work, Work::Busy(_))) {
            if let Ok(me) = std::env::current_exe() {
                if std::process::Command::new(me).arg("--updated").spawn().is_ok() {
                    std::process::exit(0);
                }
            }
            *self.upd.lock().unwrap() = Upd::Failed("Updated. Close this window and open VC-SA-Coop.exe again.".into());
        }
        let busy = matches!(work, Work::Busy(_)) || matches!(upd, Upd::Working(_) | Upd::Done(_));
        if self.art[self.which].is_none() {
            self.art[self.which] = Some(steam_art(ctx, &GAMES[self.which]));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ctx, |ui| {
            // top bar: icon, name, game tabs
            egui::Frame::new().fill(Color32::from_rgb(8, 8, 8)).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let tex = ui.ctx().load_texture("icon", egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::draw(64)), Default::default());
                    ui.add(egui::Image::new(&tex).fit_to_exact_size(Vec2::splat(22.0)));
                    ui.label(strong("VC / SA CO-OP", 14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (i, name) in [(0usize, "SAN ANDREAS"), (1, "VICE CITY")] {
                            if self.games[i].is_none() {
                                continue;
                            }
                            let on = self.which == i;
                            let r = ui.add_enabled(!busy, egui::Button::new(strong(name, 13.0).color(if on { TEXT } else { MUTED })).frame(false));
                            if on {
                                let b = r.rect;
                                ui.painter().line_segment([Pos2::new(b.left(), b.bottom() + 3.0), Pos2::new(b.right(), b.bottom() + 3.0)], Stroke::new(2.0_f32, ACCENT));
                            }
                            if r.clicked() && !on {
                                self.which = i;
                                self.read_current();
                            }
                        }
                    });
                });
            });
            if let Some(art) = &self.art[self.which] {
                hero(ui, art, &GAMES[self.which]);
            }
            egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 8)).show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| self.body(ui, ctx, &work, busy));
            });
        });
    }
}

impl App {
    fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, work: &Work, busy: bool) {
        let g = &GAMES[self.which];
        card(ui, "Game", |ui| {
            if self.games.iter().all(|x| x.is_none()) {
                ui.colored_label(BAD, "Found neither Vice City nor San Andreas Definitive Edition in your Steam libraries.");
                ui.small("A legal Steam copy is required.");
                return;
            }
            if let Some(dir) = &self.games[self.which] {
                let state = if installed(g, dir) { ("Mod installed", OK) } else { ("Mod not installed yet", MUTED) };
                ui.colored_label(state.1, state.0);
                ui.label(RichText::new(dir.display().to_string()).small().color(MUTED));
            }
        });
        let Some(dir) = self.games[self.which].clone() else { return };
        ui.add_space(4.0);
        card(ui, "Settings", |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if graphics_level(g).is_none() {
                    ui.label(RichText::new("Graphics: your own Engine.ini, left untouched.").color(MUTED));
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Graphics");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for i in (0..GRAPHICS.len()).rev() {
                                let name = if i == 0 { "Original" } else { GRAPHICS[i].0 };
                                ui.selectable_value(&mut self.graphics, i, name);
                            }
                        });
                    });
                }
                if is_vc(g) {
                    ui.checkbox(&mut self.handling, "Better car handling (faster, more grip, ABS)");
                    ui.checkbox(&mut self.fast, "Faster cars (every car reaches at least 260 km/h)");
                    ui.checkbox(&mut self.tough, "Tougher enemies (armed enemies aim better and take more hits)");
                }
                if reshade_on(&dir).is_none() {
                    ui.label(RichText::new("ReShade: the game already has another mod's dxgi.dll.").color(MUTED));
                } else {
                    ui.checkbox(&mut self.reshade, "ReShade colors (Home: menu, Scroll Lock: off)");
                }
            });
        });
        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new("What exactly changes").color(MUTED)).show(ui, |ui| {
            for t in [
                "In the game folder Gameface\\Binaries\\Win64: version.dll (ASI loader), cleo_redux64.asi, coopnet64.asi, the CLEO folder; if ticked, ReShade (dxgi.dll).",
                "Gameface\\Content\\Paks\\~mods: the co-op menu entry and, if ticked, handling. Both are built from your own game files.",
                "Documents\\Rockstar Games: graphics lines in Engine.ini and CustomSettings.ini. Your own entries stay untouched.",
                "No admin rights needed. \"Uninstall\" undoes everything.",
                "When it starts, this program asks GitHub for a newer version of the mod and updates itself.",
            ] {
                ui.label(RichText::new(format!("• {t}")).small());
            }
        });
        // result of the last run
        let lines = self.log.lock().unwrap().clone();
        if !lines.is_empty() || busy {
            ui.add_space(4.0);
            card(ui, "Progress", |ui| {
                for (ok, t) in &lines {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(if *ok { OK } else { BAD }, if *ok { "✔" } else { "✖" });
                        ui.label(t);
                    });
                }
                if let Work::Busy(t) = work {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(*t).color(MUTED));
                    });
                }
            });
        }
        match self.upd.lock().unwrap().clone() {
            Upd::Working(v) | Upd::Done(v) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Updating to v{v}. This takes a few seconds..."));
                });
            }
            Upd::Failed(why) => {
                ui.label(RichText::new(why).small().color(ACCENT));
            }
            Upd::Idle => {}
        }
        ui.add_space(10.0);
        let is_in = installed(g, &dir);
        ui.horizontal(|ui| {
            let main = if is_in { "SAVE" } else { "INSTALL" };
            let b = egui::Button::new(strong(main, 14.5).color(Color32::BLACK)).fill(ACCENT).min_size(Vec2::new(150.0, 38.0));
            if ui.add_enabled(!busy, b).clicked() {
                self.run(false, ctx);
            }
            if is_in && ui.add_enabled(!busy, egui::Button::new(strong("PLAY", 14.5)).min_size(Vec2::new(110.0, 38.0))).clicked() {
                start_game(g);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if is_in && ui.add_enabled(!busy, egui::Button::new(RichText::new("Uninstall").color(MUTED)).frame(false)).clicked() {
                    self.run(true, ctx);
                }
            });
        });
        if let Work::Done(true) = work {
            if is_in {
                ui.label(RichText::new("Start the game, load a save, then ESC → CO-OP: session name and password. Your friend enters the same.").small().color(MUTED));
            }
        }
        ui.add_space(12.0);
        egui::CollapsingHeader::new(RichText::new("Play with an IP code (no Steam)").color(MUTED)).show(ui, |ui| self.ip_mode(ui, ctx));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.hyperlink_to(RichText::new("Open source").small(), REPO);
            ui.label(RichText::new("· MIT · Unofficial mod, not a Rockstar Games or Take-Two product.").small().color(MUTED));
        });
    }

    fn ip_mode(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let g = &GAMES[self.which];
        ui.horizontal(|ui| {
            ui.label("Nickname:");
            ui.add(egui::TextEdit::singleline(&mut self.nick).char_limit(NICK_LEN));
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Host, "Host game");
            ui.selectable_value(&mut self.tab, Tab::Join, "Join");
        });
        match self.tab {
            Tab::Host => match &self.hosting {
                None => {
                    ui.label(RichText::new("The server runs on this PC; you give your friend a code.").small().color(MUTED));
                    if ui.button("Host game").clicked() {
                        self.host();
                    }
                }
                Some(h) => {
                    for (label, code) in [("Code for a friend over the internet:", &h.internet_code), ("Code on the same network:", &h.lan_code)] {
                        if let Some(code) = code {
                            ui.label(label);
                            ui.horizontal(|ui| {
                                ui.monospace(code);
                                if ui.button("Copy").clicked() {
                                    ctx.copy_text(code.clone());
                                }
                            });
                        }
                    }
                    let players = h.players.lock().unwrap().clone();
                    ui.label(format!("Connected: {}", if players.is_empty() { "nobody yet".into() } else { players.join(", ") }));
                    if ui.button("Start game").clicked() {
                        start_game(g);
                    }
                    ui.label(RichText::new("Keep this window open while you play: it runs the server.").small().color(MUTED));
                }
            },
            Tab::Join => {
                ui.label("Friend's code:");
                ui.text_edit_singleline(&mut self.code);
                if ui.button("Join and start game").clicked() && self.join() {
                    start_game(g);
                    self.msg.push("Starting the game. You will join automatically after loading a save.".into());
                }
            }
        }
        for m in self.msg.iter().rev().take(4) {
            ui.label(RichText::new(m).small().color(MUTED));
        }
    }
}
