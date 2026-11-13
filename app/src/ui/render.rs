use super::{
    dialogs, pages,
    state::Data,
    theme::{hex, Theme},
    widgets::*,
    AppState, AppWindow,
};
use egui::{self, pos2, vec2, Align, Align2, Color32, Layout, Rect, Sense, Ui};
use std::{collections::HashMap, sync::Arc};
pub type Actions = Vec<Box<dyn FnOnce(&AppState)>>;
macro_rules! action {
    ($a:expr,$m:ident) => {
        $a.push(Box::new(|s| {
            s.$m();
        }));
    };
    ($a:expr,$m:ident,$x:expr) => {{
        let x = $x;
        $a.push(Box::new(move |s| {
            s.$m(x);
        }));
    }};
    ($a:expr,$m:ident,$x:expr,$y:expr) => {{
        let (x, y) = ($x, $y);
        $a.push(Box::new(move |s| {
            s.$m(x, y);
        }));
    }};
    ($a:expr,$m:ident,$x:expr,$y:expr,$z:expr) => {{
        let (x, y, z) = ($x, $y, $z);
        $a.push(Box::new(move |s| {
            s.$m(x, y, z);
        }));
    }};
}
pub(super) use action;
type NativeGeometry = (
    winit::dpi::PhysicalSize<u32>,
    Option<winit::dpi::PhysicalPosition<i32>>,
    bool,
    bool,
);
pub struct Desktop {
    pub app: AppWindow,
    pub passwords: [String; 2],
    pub vault_revision: i32,
    active_vault_path: String,
    theme_mode: i32,
    title: String,
    pub group_drafts: HashMap<i32, (String, String)>,
    pub drag_account: Option<i32>,
    pub texture: Option<(super::Image, egui::TextureHandle)>,
    logo: egui::TextureHandle,
    native: Option<Arc<winit::window::Window>>,
    window_state: Option<crate::window_state::WindowState>,
    last_geometry: Option<NativeGeometry>,
    geometry_changed: std::time::Instant,
}
impl Desktop {
    pub fn new(app: AppWindow, ctx: &egui::Context) -> Self {
        app.attach(ctx);
        install_fonts(ctx);
        let bytes = include_bytes!("../../../macos/bankingcraft-icon.png");
        let img = image::load_from_memory(bytes)
            .expect("application icon")
            .to_rgba8();
        let logo = ctx.load_texture(
            "BankingCraft",
            egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            app,
            passwords: Default::default(),
            vault_revision: -1,
            active_vault_path: String::new(),
            theme_mode: -1,
            title: String::new(),
            group_drafts: Default::default(),
            drag_account: None,
            texture: None,
            logo,
            native: None,
            window_state: None,
            last_geometry: None,
            geometry_changed: std::time::Instant::now(),
        }
    }
    pub fn draw(&mut self, ui: &mut Ui) {
        let st = self.app.global::<AppState>();
        let before = st.snapshot();
        let mut d = before.clone();
        if d.data_path != self.active_vault_path {
            self.group_drafts.clear();
            self.drag_account = None;
            self.active_vault_path = d.data_path.clone();
        }

        let mut actions = Actions::new();
        let preference = match d.theme_mode {
            1 => egui::ThemePreference::Light,
            2 => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        };
        if self.theme_mode != d.theme_mode {
            self.theme_mode = d.theme_mode;
            ui.ctx().set_theme(preference);
            if let Some(native) = &self.native {
                native.set_theme(match d.theme_mode {
                    1 => Some(winit::window::Theme::Light),
                    2 => Some(winit::window::Theme::Dark),
                    _ => None,
                });
            }
        }
        let t = Theme::new(ui.ctx().theme() == egui::Theme::Dark);
        style(ui.ctx(), &t);
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0., t.page_bg);
        let modal = d.show_pin
            || d.show_tan
            || d.show_choice
            || d.show_confirm
            || d.show_account_name
            || d.show_vop
            || d.show_pinpad
            || d.busy
            || d.show_vault_manager;
        if d.unlocked {
            ui.scope_builder(
                if modal {
                    egui::UiBuilder::new().disabled()
                } else {
                    egui::UiBuilder::new()
                },
                |ui| {
                    let rail = Rect::from_min_size(rect.min, vec2(236., rect.height()));
                    self.navigation(ui, rail, &t, &d, &mut actions);
                    let content = Rect::from_min_max(pos2(rail.right(), rect.top()), rect.max);
                    let mut body = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("page", d.page))
                            .max_rect(content)
                            .layout(Layout::top_down(Align::Min)),
                    );
                    pages::show(
                        &mut body,
                        &t,
                        &mut d,
                        &mut actions,
                        &mut self.group_drafts,
                        &mut self.drag_account,
                    );
                },
            );
        }
        if !d.unlocked || d.show_vault_manager {
            dialogs::unlock(
                ui,
                &t,
                &mut d,
                &mut actions,
                &mut self.passwords,
                &mut self.vault_revision,
            );
        }
        dialogs::show(ui, &t, &mut d, &mut actions, &mut self.texture);
        if d.toast.visible {
            egui::Area::new(egui::Id::new("toast"))
                .anchor(
                    Align2::CENTER_BOTTOM,
                    vec2(if rect.width() > 900. { 118. } else { 0. }, -24.),
                )
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(hex(if d.toast.error { 0x3a1418 } else { 0x13233a }))
                        .corner_radius(10.)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_max_width(616.);
                            ui.horizontal(|ui| {
                                ui.set_max_width(616.);
                                text(
                                    ui,
                                    &d.toast.text,
                                    14.,
                                    false,
                                    if d.toast.error {
                                        hex(0xffc9cd)
                                    } else {
                                        Color32::WHITE
                                    },
                                );
                                if ghost(ui, &t, "", icons::CLOSE) {
                                    action!(actions, invoke_toast_close);
                                }
                            });
                        });
                });
        }
        if ui.input(|i| i.pointer.any_released()) {
            self.drag_account = None;
        }
        st.patch(&before, &d);
        for action in actions {
            action(&st);
        }
        let title = if d.unlocked && !d.active_vault_name.is_empty() {
            format!("BankingCraft · {}", d.active_vault_name)
        } else {
            "BankingCraft".into()
        };
        if title != self.title {
            self.title = title.clone();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }
    fn navigation(&self, ui: &mut Ui, rail: Rect, t: &Theme, d: &Data, a: &mut Actions) {
        ui.painter().rect_filled(rail, 0., t.nav_bg);
        ui.painter().image(
            self.logo.id(),
            Rect::from_min_size(rail.min + vec2(20., 20.), vec2(38., 38.)),
            Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
            Color32::WHITE,
        );
        elide(
            ui,
            Rect::from_min_size(rail.min + vec2(68., 20.), vec2(160., 22.)),
            "BankingCraft",
            18.,
            true,
            Color32::WHITE,
            Align2::LEFT_TOP,
        );
        elide(
            ui,
            Rect::from_min_size(rail.min + vec2(68., 46.), vec2(160., 14.)),
            tr("Online-Banking"),
            11.,
            false,
            t.nav_text_muted,
            Align2::LEFT_TOP,
        );
        let mut y = rail.top() + 83.;
        for (page, label, path) in [
            (0, "Übersicht", icons::OVERVIEW),
            (1, "Konten", icons::BANK),
            (2, "Umsätze", icons::LIST),
            (-1, "ZAHLUNGSVERKEHR", ""),
            (3, "Überweisung", icons::TRANSFER),
            (9, "Auftragsliste", icons::LIST),
            (4, "Daueraufträge", icons::REPEAT),
            (-1, "FINANZEN", ""),
            (5, "Auswertungen", icons::CHART),
        ] {
            if page < 0 {
                elide(
                    ui,
                    Rect::from_min_size(pos2(rail.left() + 26., y), vec2(190., 34.)),
                    tr(label),
                    11.,
                    true,
                    t.nav_text_muted,
                    Align2::LEFT_BOTTOM,
                );
                y += 37.;
                continue;
            }
            let label = if page == 9 && d.queued_order_count > 0 {
                format!("{} ({})", tr(label), d.queued_order_count)
            } else {
                tr(label).into()
            };
            nav(
                ui,
                Rect::from_min_size(pos2(rail.left() + 14., y), vec2(208., 40.)),
                t,
                (page, &label, path),
                d.page,
                a,
            );
            y += 43.;
        }
        for (i, (page, label, path)) in [
            (6, "Bankzugänge", icons::KEY),
            (7, "Kartenleser", icons::READER),
            (8, "Einstellungen", icons::SETTINGS),
        ]
        .into_iter()
        .enumerate()
        {
            nav(
                ui,
                Rect::from_min_size(
                    pos2(rail.left() + 14., rail.bottom() - 140. + i as f32 * 43.),
                    vec2(208., 40.),
                ),
                t,
                (page, tr(label), path),
                d.page,
                a,
            );
        }
    }
}
fn nav(
    ui: &Ui,
    r: Rect,
    t: &Theme,
    (page, label, path): (i32, &str, &str),
    current: i32,
    a: &mut Actions,
) {
    let response = ui.interact(r, egui::Id::new(("nav", page)), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if page == current || response.hovered() {
        ui.painter().rect_filled(
            r,
            6.,
            if page == current {
                t.nav_active
            } else {
                t.nav_bg_hover
            },
        );
    }
    icon(
        ui,
        path,
        Rect::from_min_size(r.min + vec2(12., 10.), vec2(20., 20.)),
        t.nav_text,
    );
    elide(
        ui,
        Rect::from_min_max(r.min + vec2(44., 0.), r.max - vec2(12., 0.)),
        label,
        14.,
        page == current,
        if page == current {
            Color32::WHITE
        } else {
            t.nav_text
        },
        Align2::LEFT_CENTER,
    );
    if response.clicked() {
        action!(a, invoke_navigate, page);
    }
}
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    #[cfg(target_os = "macos")]
    let families = [
        fontdb::Family::Name("Helvetica"),
        fontdb::Family::Name("Arial"),
        fontdb::Family::SansSerif,
    ];
    #[cfg(not(target_os = "macos"))]
    let families = [
        fontdb::Family::Name("Segoe UI"),
        fontdb::Family::Name("Noto Sans"),
        fontdb::Family::SansSerif,
    ];
    for (name, weight, bold) in [
        ("BankingCraft Regular", fontdb::Weight::NORMAL, false),
        ("BankingCraft Semibold", fontdb::Weight::SEMIBOLD, true),
    ] {
        let mut family = fonts.families[&egui::FontFamily::Proportional].clone();
        if let Some(id) = db.query(&fontdb::Query {
            families: &families,
            weight,
            ..Default::default()
        }) {
            if let Some(data) = db.with_face_data(id, |bytes, index| {
                let mut d = egui::FontData::from_owned(bytes.to_vec());
                d.index = index;
                d
            }) {
                fonts.font_data.insert(name.into(), Arc::new(data));
                family.insert(0, name.into());
            }
        }
        fonts.families.insert(
            if bold {
                egui::FontFamily::Name(name.into())
            } else {
                egui::FontFamily::Proportional
            },
            family,
        );
    }
    ctx.set_fonts(fonts);
}
fn style(ctx: &egui::Context, t: &Theme) {
    ctx.style_mut_of(ctx.theme(), |s| {
        s.override_font_id = Some(font(14., false));
        s.spacing.item_spacing = vec2(10., 0.);
        s.spacing.button_padding = vec2(10., 7.);
        s.spacing.interact_size = vec2(32., 32.);
        s.spacing.combo_width = 160.;
        s.visuals.override_text_color = Some(t.text);
        s.visuals.panel_fill = t.page_bg;
        s.visuals.window_fill = t.card_bg;
        s.visuals.window_stroke = egui::Stroke::new(1., t.card_border);
        s.visuals.selection.bg_fill = t.accent_soft;
        s.visuals.selection.stroke = egui::Stroke::new(1., t.accent);
        for widget in [
            &mut s.visuals.widgets.inactive,
            &mut s.visuals.widgets.active,
            &mut s.visuals.widgets.hovered,
            &mut s.visuals.widgets.noninteractive,
        ] {
            widget.bg_fill = if t.dark {
                hex(0x293648)
            } else {
                Color32::WHITE
            };
            widget.weak_bg_fill = widget.bg_fill;
            widget.bg_stroke = egui::Stroke::new(1., t.card_border);
            widget.corner_radius = 4.into();
            widget.fg_stroke = egui::Stroke::new(1., t.text);
        }
        s.visuals.widgets.hovered.bg_fill = t.row_hover;
        s.visuals.widgets.hovered.weak_bg_fill = t.row_hover;
        s.visuals.widgets.active.bg_stroke = egui::Stroke::new(1., t.accent);
    });
}
impl eframe::App for Desktop {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.app.pump(ctx);
    }
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        self.draw(ui);
        if let (Some(window), Some(state)) = (&self.native, &self.window_state) {
            let current = (
                window.inner_size(),
                window.outer_position().ok(),
                window.is_maximized(),
                window.fullscreen().is_some(),
            );
            if self.last_geometry != Some(current) {
                self.last_geometry = Some(current);
                self.geometry_changed = std::time::Instant::now();
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(350));
            } else if self.geometry_changed.elapsed() >= std::time::Duration::from_millis(300) {
                state.capture(window);
            }
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if let Some(state) = &self.window_state {
            if let Some(native) = &self.native {
                state.capture(native);
            }
            state.save();
        }
        crate::CTX.with(|c| {
            if let Some(c) = c.borrow().as_ref() {
                c.bridge.answer(crate::bridge::Answer::Cancel);
            }
        });
    }
}
pub fn run(app: AppWindow) -> eframe::Result {
    let img = image::load_from_memory(include_bytes!("../../../macos/bankingcraft-icon.png"))
        .expect("icon")
        .to_rgba8();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BankingCraft")
            .with_app_id("de.appcreation.bankingcraft")
            .with_inner_size([1360., 860.])
            .with_min_inner_size([1100., 680.])
            .with_icon(egui::IconData {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            }),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "BankingCraft",
        options,
        Box::new(move |cc| {
            let mut desktop = Desktop::new(app, &cc.egui_ctx);
            let state = crate::window_state::WindowState::new();
            if let Some(window) = cc.winit_window() {
                state.restore(window);
                desktop.app.attach_native(window.clone());
                desktop.native = Some(window.clone());
            }
            desktop.window_state = Some(state);
            Ok(Box::new(desktop))
        }),
    )
}
