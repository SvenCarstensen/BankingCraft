pub(super) use super::icons;
use super::{theme::Theme, Model, ModelRc};
use egui::{
    self, pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Layout, Pos2, Rect, Response,
    RichText, Sense, Stroke, Ui,
};
pub use i18n::tr;
pub fn font(size: f32, bold: bool) -> FontId {
    FontId::new(
        size,
        if bold {
            FontFamily::Name("BankingCraft Semibold".into())
        } else {
            FontFamily::Proportional
        },
    )
}
/// CSS-style gradient angles, preserving the original palette and stop positions.
pub fn gradient(ui: &Ui, rect: Rect, degrees: f32, stops: &[(f32, Color32)]) {
    ui.painter().add(gradient_shape(rect, degrees, stops));
}
pub fn gradient_shape(rect: Rect, degrees: f32, stops: &[(f32, Color32)]) -> egui::Shape {
    let angle = degrees.to_radians();
    let direction = vec2(angle.sin(), -angle.cos());
    let span = direction.x.abs() * rect.width() + direction.y.abs() * rect.height();
    let position = |p: Pos2| ((p - rect.center()).dot(direction) / span + 0.5).clamp(0., 1.);
    let mut mesh = egui::Mesh::default();
    for pair in stops.windows(2) {
        let [(lo, first), (hi, last)] = [pair[0], pair[1]];
        let mut polygon = vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
        for (boundary, lower) in [(lo, true), (hi, false)] {
            let original = std::mem::take(&mut polygon);
            if original.is_empty() {
                break;
            }
            for i in 0..original.len() {
                let a = original[i];
                let b = original[(i + 1) % original.len()];
                let (ta, tb) = (position(a), position(b));
                let inside = |t| if lower { t >= boundary } else { t <= boundary };
                if inside(ta) {
                    polygon.push(a);
                }
                if inside(ta) != inside(tb) {
                    polygon.push(a + (b - a) * ((boundary - ta) / (tb - ta)));
                }
            }
        }
        let base = mesh.vertices.len() as u32;
        for p in &polygon {
            mesh.colored_vertex(
                *p,
                first.lerp_to_gamma(last, ((position(*p) - lo) / (hi - lo)).clamp(0., 1.)),
            );
        }
        for i in 1..polygon.len().saturating_sub(1) {
            mesh.add_triangle(base, base + i as u32, base + i as u32 + 1);
        }
    }
    egui::Shape::mesh(mesh)
}
pub fn text(ui: &mut Ui, text: &str, size: f32, bold: bool, color: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(text).font(font(size, bold)).color(color)).wrap())
}
pub fn elide(
    ui: &Ui,
    rect: Rect,
    text: &str,
    size: f32,
    bold: bool,
    color: Color32,
    align: Align2,
) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.to_owned(), font(size, bold), color);
    job.wrap.max_width = rect.width().max(0.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let p = align.align_size_within_rect(galley.size(), rect).min;
    ui.painter().galley(p, galley, color);
}
pub fn icon(ui: &Ui, path: &str, rect: Rect, color: Color32) {
    use svgtypes::SimplePathSegment::*;
    let point = |x: f64, y: f64| {
        rect.min
            + vec2(
                x as f32 * rect.width() / 24.,
                y as f32 * rect.height() / 24.,
            )
    };
    let mut points = Vec::new();
    let mut current = rect.min;
    let mut start = rect.min;
    let flush = |points: &mut Vec<Pos2>| {
        if points.len() > 1 {
            ui.painter().add(egui::Shape::line(
                std::mem::take(points),
                Stroke::new(1.8 * rect.width() / 24., color),
            ));
        } else {
            points.clear();
        }
    };
    for segment in svgtypes::SimplifyingPathParser::from(path).flatten() {
        match segment {
            MoveTo { x, y } => {
                flush(&mut points);
                current = point(x, y);
                start = current;
                points.push(current);
            }
            LineTo { x, y } => {
                current = point(x, y);
                points.push(current);
            }
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let p0 = current;
                let p1 = point(x1, y1);
                let p2 = point(x2, y2);
                let p3 = point(x, y);
                for i in 1..=12 {
                    let t = i as f32 / 12.;
                    let s = 1. - t;
                    points.push(pos2(
                        s * s * s * p0.x
                            + 3. * s * s * t * p1.x
                            + 3. * s * t * t * p2.x
                            + t * t * t * p3.x,
                        s * s * s * p0.y
                            + 3. * s * s * t * p1.y
                            + 3. * s * t * t * p2.y
                            + t * t * t * p3.y,
                    ));
                }
                current = p3;
            }
            Quadratic { x1, y1, x, y } => {
                let p0 = current;
                let p1 = point(x1, y1);
                let p2 = point(x, y);
                for i in 1..=12 {
                    let t = i as f32 / 12.;
                    let s = 1. - t;
                    points.push(pos2(
                        s * s * p0.x + 2. * s * t * p1.x + t * t * p2.x,
                        s * s * p0.y + 2. * s * t * p1.y + t * t * p2.y,
                    ));
                }
                current = p2;
            }
            ClosePath => {
                points.push(start);
                current = start;
                flush(&mut points);
            }
        }
    }
    flush(&mut points);
}
pub fn button_width(ui: &Ui, label: &str, path: &str, primary: bool) -> f32 {
    let text_size = ui
        .painter()
        .layout_no_wrap(label.into(), font(14., primary), Color32::WHITE)
        .size();
    (text_size.x
        + if primary { 28. } else { 20. }
        + if path.is_empty() {
            0.
        } else {
            if label.is_empty() {
                -2.
            } else {
                22.
            }
        })
    .max(if primary { 110. } else { 34. })
}
pub fn button(
    ui: &mut Ui,
    t: &Theme,
    label: &str,
    path: &str,
    primary: bool,
    enabled: bool,
) -> Response {
    let w = button_width(ui, label, path, primary);
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(w, if primary { 36. } else { 34. }),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if enabled {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let bg = if primary {
        if !enabled {
            t.button_disabled
        } else if response.hovered() {
            t.accent_hover
        } else {
            t.button_bg
        }
    } else if response.hovered() {
        t.row_hover
    } else {
        t.card_bg
    };
    let fg = if primary { Color32::WHITE } else { t.text };
    ui.painter().rect(
        rect,
        6.,
        bg,
        if primary {
            Stroke::NONE
        } else {
            Stroke::new(1., t.card_border)
        },
        egui::StrokeKind::Inside,
    );
    let mut content = rect.shrink2(vec2(if primary { 14. } else { 10. }, 0.));
    if !path.is_empty() {
        icon(
            ui,
            path,
            Rect::from_center_size(
                pos2(content.left() + 8., content.center().y),
                vec2(16., 16.),
            ),
            if primary { Color32::WHITE } else { t.accent },
        );
        content.min.x += 22.;
    }
    if !label.is_empty() {
        elide(
            ui,
            content,
            label,
            14.,
            primary,
            if enabled { fg } else { fg.gamma_multiply(0.5) },
            Align2::CENTER_CENTER,
        );
    }
    response
}
pub fn primary(ui: &mut Ui, t: &Theme, label: &str, path: &str, enabled: bool) -> bool {
    button(ui, t, tr(label), path, true, enabled).clicked()
}
pub fn ghost(ui: &mut Ui, t: &Theme, label: &str, path: &str) -> bool {
    button(ui, t, tr(label), path, false, true).clicked()
}
pub fn input(
    ui: &mut Ui,
    t: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    hint: &str,
    password: bool,
) -> Response {
    let width = ui.available_width();
    let edit = egui::TextEdit::singleline(value)
        .id_salt(id)
        .font(font(14., false))
        .text_color(t.text)
        .hint_text(RichText::new(tr(hint)).color(t.text_muted))
        .password(password)
        .desired_width((width - 22.).max(0.))
        .margin(vec2(11., 7.))
        .background_color(if t.dark {
            super::theme::hex(0x293648)
        } else {
            Color32::WHITE
        })
        .min_size(vec2(width, 32.));
    ui.add_sized(vec2(width, 32.), edit)
}
pub fn accepted(ui: &Ui, r: &Response) -> bool {
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}
pub fn field(
    ui: &mut Ui,
    t: &Theme,
    label: &str,
    value: &mut String,
    hint: &str,
    (note, error): (&str, bool),
    password: bool,
) -> Response {
    let label_response = text(ui, tr(label), 12., true, t.text_muted);
    ui.add_space(4.);
    let r = input(ui, t, label, value, hint, password).labelled_by(label_response.id);
    ui.add_space(4.);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.), Sense::hover());
    elide(
        ui,
        rect,
        note,
        11.,
        false,
        if error { t.negative } else { t.text_faint },
        Align2::LEFT_TOP,
    );
    r
}
pub fn combo(
    ui: &mut Ui,
    t: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    values: &ModelRc<String>,
    selected: &mut i32,
) -> bool {
    let before = *selected;
    let w = ui.available_width();
    egui::ComboBox::from_id_salt(id)
        .width(w)
        .truncate()
        .height(320.)
        .icon(|ui, r, v, _| {
            let c = r.center();
            ui.painter().add(egui::Shape::line(
                vec![c + vec2(-5., -2.5), c + vec2(0., 2.5), c + vec2(5., -2.5)],
                Stroke::new(1., v.fg_stroke.color),
            ));
        })
        .selected_text(
            values
                .row_data((*selected).max(0) as usize)
                .unwrap_or_default(),
        )
        .show_ui(ui, |ui| {
            ui.set_min_width((w - 12.).max(0.));
            for (index, value) in values.iter().enumerate() {
                ui.selectable_value(
                    selected,
                    index as i32,
                    RichText::new(value).font(font(14., false)).color(t.text),
                );
            }
        });
    before != *selected
}
pub fn options(values: &[&str]) -> ModelRc<String> {
    ModelRc::new(values.iter().map(|s| tr(s).to_owned()).collect())
}
pub fn segmented(ui: &mut Ui, t: &Theme, values: &[&str], selected: &mut i32) -> bool {
    let before = *selected;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.), Sense::hover());
    ui.painter().rect_filled(rect, 6., t.segmented_bg);
    let width = (rect.width() - 6.) / values.len() as f32;
    for (i, label) in values.iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + vec2(3. + i as f32 * width, 3.),
            vec2(width - 3., 28.),
        );
        let response = ui.interact(r, ui.id().with(("segment", label)), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                *selected == i as i32,
                tr(label),
            )
        });
        if response.clicked() {
            *selected = i as i32;
        }
        if *selected == i as i32 {
            ui.painter().rect_filled(r, 5., t.card_bg);
        }
        elide(
            ui,
            r,
            tr(label),
            13.,
            *selected == i as i32,
            if *selected == i as i32 {
                t.text
            } else {
                t.text_muted
            },
            Align2::CENTER_CENTER,
        );
    }
    before != *selected
}
pub fn chip(ui: &mut Ui, t: &Theme, label: &str, tint: Color32, selected: bool) -> Response {
    let w = ui
        .painter()
        .layout_no_wrap(label.into(), font(12., false), tint)
        .size()
        .x
        + 20.;
    let (rect, r) = ui.allocate_exact_size(vec2(w, 24.), Sense::click());
    r.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let fg = if t.dark {
        tint.lerp_to_gamma(Color32::WHITE, 0.4)
    } else {
        tint
    };
    ui.painter().rect_filled(
        rect,
        12.,
        if selected {
            fg
        } else {
            tint.gamma_multiply(0.12)
        },
    );
    elide(
        ui,
        rect,
        label,
        12.,
        false,
        if selected {
            if t.dark {
                t.nav_bg
            } else {
                Color32::WHITE
            }
        } else {
            fg
        },
        Align2::CENTER_CENTER,
    );
    r
}
pub fn columns(ui: &mut Ui, widths: &[f32], gap: f32, mut render: impl FnMut(&mut Ui, usize)) {
    let start = ui.cursor().min;
    let mut x = start.x;
    let mut bottom = start.y;
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    for (i, width) in widths.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, start.y), vec2((*width).max(0.), 100000.));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt((id, i))
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        child.set_width((*width).max(0.));
        render(&mut child, i);
        bottom = bottom.max(child.min_rect().bottom());
        x += width + gap;
    }
    ui.advance_cursor_after_rect(Rect::from_min_max(
        start,
        pos2((x - gap).max(start.x), bottom),
    ));
}
pub fn card<R>(ui: &mut Ui, t: &Theme, padding: i8, f: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.card_bg)
        .stroke(Stroke::new(1., t.card_border))
        .corner_radius(10.)
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: t.shadow,
        })
        .inner_margin(padding)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(10., 0.);
            f(ui)
        })
        .inner
}
pub fn header(ui: &mut Ui, t: &Theme, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        text(ui, tr(title), 15., true, t.text);
        if !subtitle.is_empty() {
            ui.add_space(4.);
            text(ui, subtitle, 12., false, t.text_muted);
        }
        ui.add_space(8.);
    });
}
pub fn card_header(
    ui: &mut Ui,
    t: &Theme,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            text(ui, tr(title), 15., true, t.text);
            if !subtitle.is_empty() {
                ui.add_space(4.);
                text(ui, subtitle, 12., false, t.text_muted);
            }
        });
        right(ui, actions);
    });
    ui.add_space(8.);
}
pub fn title(ui: &mut Ui, t: &Theme, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            text(ui, tr(title), 24., true, t.text);
            if !subtitle.is_empty() {
                ui.add_space(4.);
                text(ui, subtitle, 13., false, t.text_muted);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(18.);
}
pub fn right(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 36.),
        Layout::right_to_left(Align::Center),
        f,
    );
}
pub fn separator(ui: &mut Ui, t: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
    ui.painter().rect_filled(r, 0., t.card_border);
}

pub fn pill(ui: &mut Ui, label: &str) -> Response {
    let w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font(14., true), Color32::WHITE)
        .size()
        .x
        + 28.;
    let (r, response) = ui.allocate_exact_size(vec2(w, 32.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    ui.painter().rect_filled(
        r,
        16.,
        Color32::from_white_alpha(if response.hovered() { 51 } else { 34 }),
    );
    elide(
        ui,
        r,
        label,
        14.,
        true,
        Color32::WHITE,
        Align2::CENTER_CENTER,
    );
    response
}
