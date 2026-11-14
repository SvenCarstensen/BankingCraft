use super::{
    render::{action, Actions},
    state::*,
    theme::{hex, Theme},
    widgets::*,
    Model,
};

use egui::{self, pos2, vec2, Align, Align2, Color32, Layout, Rect, Sense, Ui};

use std::collections::HashMap;

pub fn show(
    ui: &mut Ui,
    t: &Theme,
    d: &mut Data,
    a: &mut Actions,
    drafts: &mut HashMap<i32, (String, String)>,
    drag: &mut Option<i32>,
) {
    if d.page == 2 {
        egui::Frame::new()
            .inner_margin(24)
            .show(ui, |ui| transactions(ui, t, d, a));

        return;
    }

    egui::ScrollArea::vertical()
        .id_salt(("page-scroll", d.page))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new().inner_margin(24).show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.spacing_mut().item_spacing = vec2(12., 0.);

                match d.page {
                    0 => overview(ui, t, d, a, drag),
                    1 => accounts(ui, t, d, a, drafts),
                    3 => transfer(ui, t, d, a),
                    4 => standing(ui, t, d, a),
                    5 => analysis(ui, t, d, a),
                    6 => accesses(ui, t, d, a),
                    7 => reader(ui, t, d, a),
                    8 => settings(ui, t, d, a),
                    9 => orders(ui, t, d, a),
                    _ => {}
                }
            });
        });
}

fn group(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions, tile: usize) {
    let w = ui.available_width();

    columns(ui, &[40., w - 50.], 10., |ui, col| {
        if col == 0 {
            ui.add_space(9.);

            text(ui, tr("Gruppe"), 12., false, t.text_muted);
        } else {
            let index = match tile {
                0 => &mut d.recent_group,
                1 => &mut d.category_group,
                2 => &mut d.months_group,
                _ => &mut d.upcoming_group,
            };

            if combo(
                ui,
                t,
                ("overview-group", tile),
                &d.overview_group_names,
                index,
            ) {
                action!(a, invoke_overview_group_changed, tile as i32, *index);
            }
        }
    });

    ui.add_space(8.);
}

fn overview(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions, drag: &mut Option<i32>) {
    title(ui, t, "Finanzübersicht", &d.last_sync, |_| {});

    let w = ui.available_width();

    let left = (249. + (w - 816.) * 90. / 260.).max(240.);

    let extra = ((ui.available_height() - 1190.) / 3.).max(0.);

    let height = (416. + extra)
        .max(80. + d.accounts.row_count() as f32 * 58.)
        .max(300. + d.account_groups.row_count() as f32 * 160.);

    columns(ui, &[left, w - left - 18.], 18., |ui, col| {
        if col == 0 {
            let background = ui.painter().add(egui::Shape::Noop);
            let panel = egui::Frame::new().inner_margin(22).show(ui, |ui| {
                ui.set_min_height(height - 44.);

                text(
                    ui,
                    tr("Gesamtsaldo aller Konten"),
                    13.,
                    false,
                    hex(0xc9d9f2),
                );

                ui.add_space(10.);

                text(ui, tr("Saldo inkl. vorgemerkter Umsätze"), 12., false, hex(0xc9d9f2));
                text(ui, &d.total_balance, 30., true, Color32::WHITE);
                ui.add_space(8.);
                text(ui, tr("Gebuchter Saldo"), 12., false, hex(0xc9d9f2));
                text(ui, &d.total_booked_balance, 18., true, Color32::WHITE);
                text(ui, &d.total_balance_note, 11., false, hex(0xc9d9f2));

                ui.add_space(24.);

                text(ui, &d.month_label, 12., false, hex(0xc9d9f2));

                ui.add_space(10.);

                let width = ui.available_width();

                columns(
                    ui,
                    &[(width - 28.) / 2., (width - 28.) / 2.],
                    28.,
                    |ui, i| {
                        text(
                            ui,
                            tr(if i == 0 { "Einnahmen" } else { "Ausgaben" }),
                            12.,
                            false,
                            hex(0xa8c2e8),
                        );

                        ui.add_space(4.);

                        text(
                            ui,
                            if i == 0 {
                                &d.month_income
                            } else {
                                &d.month_expense
                            },
                            18.,
                            true,
                            hex(if i == 0 { 0x7ff0b0 } else { 0xffb3b8 }),
                        );
                    },
                );

                if d.account_groups.row_count() > 0 {
                    ui.add_space(20.);

                    text(ui, tr("Gruppensalden"), 12., false, hex(0xc9d9f2));

                    ui.add_space(10.);

                    for g in d.account_groups.iter() {
                        egui::Frame::new()
                            .fill(Color32::from_white_alpha(18))
                            .corner_radius(8.)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                let balance_theme = Theme { text: Color32::WHITE, text_muted: hex(0xc9d9f2), negative: hex(0xffb3b8), ..*t };
                                text(ui, &g.name, 14., true, Color32::WHITE);
                                ui.add_space(8.);
                                for b in g.balances.iter() {
                                    balance_pair(ui, &balance_theme, &b.amount, &b.booked_amount, b.negative);
                                    ui.add_space(8.);
                                }
                                if g.balances.row_count() == 0 {
                                    balance_pair(ui, &balance_theme, "–", "–", false);
                                }
                                text(ui, &g.balance_note, 11., false, hex(0xc9d9f2));
                            });

                        ui.add_space(10.);
                    }
                }

                if pill(ui, tr("Umsätze synchronisieren")).clicked() {
                    action!(a, invoke_refresh_all);
                }
            });
            ui.painter().set(background, gradient_shape(panel.response.rect, 135., &[(0., hex(0x1e6fd9)), (1., hex(0x13233a))]));
        } else {
            card(ui, t, 18, |ui| {
                ui.set_min_height(height - 36.);

                card_header(
                    ui,
                    t,
                    "Konten",
                    &format!(
                        "{}{}",
                        d.accounts.row_count(),
                        tr(" Konten · Zum Sortieren ziehen")
                    ),
                    |ui| {
                        if ghost(ui, t, "Alle anzeigen", "") {
                            action!(a, invoke_navigate, 1);
                        }
                    },
                );

                for account in d.accounts.iter() {
                    account_overview(ui, t, &account, d.busy, a, drag);

                    ui.add_space(4.);
                }

                if d.accounts.row_count() == 0 {
                    text(
                        ui,
                        tr("Noch kein Bankzugang eingerichtet."),
                        14.,
                        false,
                        t.text_muted,
                    );

                    ui.add_space(10.);

                    ui.horizontal_wrapped(|ui| {
                        if primary(ui, t, "Bankzugang einrichten", icons::PLUS, true) {
                            action!(a, invoke_navigate, 6);

                            action!(a, invoke_new_access);
                        }

                        if ghost(ui, t, "Demo-Daten laden", "") {
                            action!(a, invoke_load_demo);
                        }
                    });
                }
            });
        }
    });

    ui.add_space(18.);

    let left = (w - 18.) * 0.6 - 36.;

    let height = (411. + extra)
        .max(100. + d.recent.row_count() as f32 * 52.)
        .max(100. + d.category_stats.row_count() as f32 * 46.);

    columns(ui, &[left, w - left - 18.], 18., |ui, col| {
        if col == 0 {
            card(ui, t, 0, |ui| {
                ui.set_min_height(height);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 18,
                        right: 18,
                        top: 18,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        card_header(ui, t, "Letzte Umsätze", "", |ui| {
                            if ghost(ui, t, "Umsatzliste", "") {
                                action!(a, invoke_navigate, 2);
                            }
                        });
                        group(ui, t, d, a, 0);
                    });
                for row in d.recent.iter() {
                    if txline(ui, t, &row, true, false).clicked() {
                        action!(a, invoke_navigate, 2);
                        action!(a, invoke_tx_select, row.index);
                    }
                }
                if d.recent.row_count() == 0 {
                    text(ui, tr("Keine Umsätze vorhanden."), 14., false, t.text_faint);
                }
                ui.add_space(8.);
            });
        } else {
            card(ui, t, 18, |ui| {
                ui.set_min_height(height - 36.);
                header(ui, t, "Ausgaben nach Kategorie", &d.month_label);
                group(ui, t, d, a, 1);
                bars(ui, t, &d.category_stats);
            });
        }
    });

    ui.add_space(18.);

    let left = (w - 18.) * 0.6 - 76.;

    columns(ui, &[left, w - left - 18.], 18., |ui, col| {
        card(ui, t, 18, |ui| {
            ui.set_min_height(295. + extra - 36.);

            if col == 0 {
                header(ui, t, "Einnahmen & Ausgaben", tr("Letzte 6 Monate"));

                group(ui, t, d, a, 2);

                months(ui, t, &d.months);
            } else {
                card_header(ui, t, "Anstehende Daueraufträge", "", |ui| {
                    if ghost(ui, t, "Verwalten", "") {
                        action!(a, invoke_navigate, 4);
                    }
                });

                group(ui, t, d, a, 3);

                for s in d.upcoming.iter() {
                    let (r, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 50.), Sense::hover());

                    icon(
                        ui,
                        icons::REPEAT,
                        Rect::from_min_size(r.min + vec2(0., 15.), vec2(20., 20.)),
                        t.accent,
                    );

                    elide(
                        ui,
                        Rect::from_min_max(
                            r.min + vec2(30., 6.),
                            pos2(r.right() - 110., r.top() + 25.),
                        ),
                        &s.name,
                        14.,
                        true,
                        t.text,
                        Align2::LEFT_TOP,
                    );

                    elide(
                        ui,
                        Rect::from_min_max(
                            r.min + vec2(30., 28.),
                            pos2(r.right() - 110., r.bottom()),
                        ),
                        &s.next,
                        12.,
                        false,
                        t.text_muted,
                        Align2::LEFT_TOP,
                    );

                    elide(
                        ui,
                        Rect::from_min_max(pos2(r.right() - 110., r.top()), r.max),
                        &s.amount,
                        14.,
                        true,
                        t.negative,
                        Align2::RIGHT_CENTER,
                    );
                }

                if d.upcoming.row_count() == 0 {
                    text(
                        ui,
                        tr(if d.upcoming_group == 0 {
                            "Keine Daueraufträge bekannt. Unter Zahlungsverkehr → Daueraufträge abrufen."
                        } else {
                            "Keine anstehenden Daueraufträge für diese Gruppe."
                        }),
                        13.,
                        false,
                        t.text_faint,
                    );
                }
            }
        });
    });
}

/// Keep both balance meanings visible, including when the card is narrow.
fn balance_pair(ui: &mut Ui, t: &Theme, current: &str, booked: &str, negative: bool) {
    let labels = [tr("Saldo inkl. vorgemerkter Umsätze"), tr("Gebuchter Saldo")];
    let amounts = [current, booked];
    let width = ui.available_width();
    let horizontal = width >= 340.;
    let column_width = if horizontal { (width - 18.) / 2. } else { width };
    let label_height = labels.iter().map(|label| ui.painter().layout(
        (*label).into(), font(11., false), t.text_muted, column_width,
    ).size().y).fold(0., f32::max);
    let render = |ui: &mut Ui, i: usize| {
        ui.allocate_ui(vec2(column_width, label_height), |ui| {
            text(ui, labels[i], 11., false, t.text_muted);
        });
        text(ui, amounts[i], if i == 0 { 16. } else { 14. }, i == 0,
            if i == 0 && negative { t.negative } else { t.text });
    };
    if horizontal {
        columns(ui, &[column_width, column_width], 18., render);
    } else {
        render(ui, 0);
        ui.add_space(6.);
        render(ui, 1);
    }
}

fn account_overview(
    ui: &mut Ui,
    t: &Theme,
    a: &AccountRow,
    busy: bool,
    actions: &mut Actions,
    drag: &mut Option<i32>,
) {
    let background = ui.painter().add(egui::Shape::Noop);
    let frame = egui::Frame::new()
        .inner_margin(egui::Margin { left: 48, right: 6, top: 8, bottom: 8 })
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            let labels = [tr("Saldo inkl. vorgemerkter Umsätze"), tr("Gebuchter Saldo")];
            let amounts = [&a.balance, &a.booked_balance];
            let amount_widths = [0, 1].map(|i| ui.painter().layout_no_wrap(
                amounts[i].clone(), font(if i == 0 { 16. } else { 14. }, i == 0), t.text,
            ).size().x);
            let preferred = [0, 1].map(|i| ui.painter().layout_no_wrap(
                labels[i].into(), font(11., false), t.text_muted,
            ).size().x.max(amount_widths[i]));
            let scale = (((width - 36.) * 0.55) / preferred.iter().sum::<f32>()).min(1.);
            let balance_widths = [0, 1].map(|i| (preferred[i] * scale).max(amount_widths[i]));
            let name_width = (width - balance_widths.iter().sum::<f32>() - 36.).max(0.);
            let name = ui.painter().layout(a.name.clone(), font(14., true), t.text, name_width);
            let subtitle = format!("{}  ·  {}", a.bank, a.iban);
            let subtitle_height = ui.painter().layout(subtitle.clone(), font(12., false), t.text_muted, name_width).size().y;
            let balance_heights = [0, 1].map(|i| {
                let label = ui.painter().layout(labels[i].into(), font(11., false), t.text_muted, balance_widths[i]);
                let amount = ui.painter().layout_no_wrap(amounts[i].clone(), font(if i == 0 { 16. } else { 14. }, i == 0), t.text);
                label.size().y + amount.size().y
            });
            let heights = [name.size().y + 8. + subtitle_height, balance_heights[0], balance_heights[1]];
            let height = heights.into_iter().fold(0., f32::max);
            columns(ui, &[name_width, balance_widths[0], balance_widths[1]], 18., |ui, col| {
                ui.add_space((height - heights[col]) / 2.);
                if col == 0 {
                    // The enclosing interactive row exposes the name to accessibility.
                    let (rect, _) = ui.allocate_exact_size(name.size(), Sense::hover());
                    ui.painter().galley(rect.min, name.clone(), t.text);
                    ui.add_space(8.);
                    text(ui, &subtitle, 12., false, t.text_muted);
                } else {
                    let i = col - 1;
                    text(ui, labels[i], 11., false, t.text_muted);
                    text(ui, amounts[i], if i == 0 { 16. } else { 14. }, i == 0,
                        if i == 0 && a.negative { t.negative } else { t.text });
                }
            });
        });
    let r = frame.response.rect;

    let response = ui.interact(
        r,
        ui.id().with(("account", a.id)),
        if busy {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        },
    );

    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy, &a.name));

    if response.hovered() || *drag == Some(a.id) {
        ui.painter().set(background, egui::Shape::rect_filled(r, 6., t.row_hover));
    }

    for x in [0., 3.] {
        for y in [-4., 0., 4.] {
            ui.painter().circle_filled(
                pos2(r.left() + 12. + x, r.center().y + y),
                0.8,
                t.text_faint,
            );
        }
    }

    ui.painter().rect_filled(
        Rect::from_min_size(pos2(r.left() + 32., r.center().y - 14.), vec2(4., 28.)),
        2.,
        a.color,
    );

    if response.drag_started() {
        *drag = Some(a.id);
    }

    if response.clicked() {
        action!(actions, invoke_open_account, a.id);
    }

    if let Some(source) = *drag {
        if let Some(p) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| r.contains(*p))
        {
            let after = p.y > r.center().y;

            ui.painter().hline(
                r.x_range(),
                if after { r.bottom() } else { r.top() },
                egui::Stroke::new(2., t.accent),
            );

            if ui.input(|i| i.pointer.any_released()) {
                action!(actions, invoke_move_account, source, a.id, after);

                *drag = None;
            }
        }
    }
}

fn bars(ui: &mut Ui, t: &Theme, stats: &super::ModelRc<BarStat>) {
    for s in stats.iter() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.), Sense::hover());

        elide(
            ui,
            Rect::from_min_size(r.min, vec2(r.width() * 0.65, 20.)),
            &s.label,
            13.,
            false,
            t.text,
            Align2::LEFT_TOP,
        );

        elide(
            ui,
            Rect::from_min_size(r.min, vec2(r.width(), 20.)),
            &s.value,
            13.,
            true,
            t.text,
            Align2::RIGHT_TOP,
        );

        let track = Rect::from_min_size(r.min + vec2(0., 30.), vec2(r.width(), 8.));

        ui.painter().rect_filled(track, 4., t.track_bg);

        ui.painter().rect_filled(
            Rect::from_min_size(track.min, vec2(track.width() * s.ratio.clamp(0.02, 1.), 8.)),
            4.,
            s.color,
        );
    }

    if stats.row_count() == 0 {
        text(
            ui,
            tr("Noch keine Daten in diesem Zeitraum."),
            13.,
            false,
            t.text_faint,
        );
    }
}

fn months(ui: &mut Ui, t: &Theme, stats: &super::ModelRc<MonthStat>) {
    ui.horizontal(|ui| {
        for (label, color) in [("Einnahmen", t.positive), ("Ausgaben", t.negative)] {
            let (r, _) = ui.allocate_exact_size(vec2(10., 10.), Sense::hover());

            ui.painter().rect_filled(r, 2., color);

            text(ui, tr(label), 12., false, t.text_muted);

            ui.add_space(4.);
        }
    });

    ui.add_space(8.);

    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.), Sense::hover());

    let count = stats.row_count().max(1);

    let w = r.width() / count as f32;

    for (i, m) in stats.iter().enumerate() {
        let x = r.left() + (i as f32 + 0.5) * w;

        for (offset, ratio, color) in [(-15.5, m.income, t.positive), (1.5, m.expense, t.negative)]
        {
            let height = 150. * ratio.clamp(0.01, 1.);

            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x + offset, r.top() + 150. - height), vec2(14., height)),
                3.,
                color,
            );
        }

        elide(
            ui,
            Rect::from_min_size(pos2(x - w / 2., r.top() + 154.), vec2(w, 16.)),
            &m.label,
            11.,
            false,
            t.text_muted,
            Align2::CENTER_CENTER,
        );

        ui.interact(
            Rect::from_min_size(pos2(x - w / 2., r.top()), vec2(w, 150.)),
            ui.id().with(("month", i)),
            Sense::hover(),
        )
        .on_hover_ui(|ui| {
            egui::Frame::new()
                .fill(t.tooltip_bg)
                .corner_radius(6.)
                .inner_margin(5)
                .show(ui, |ui| {
                    text(
                        ui,
                        &format!("+ {}", m.income_text),
                        11.,
                        false,
                        t.tooltip_text,
                    );
                    text(
                        ui,
                        &format!("− {}", m.expense_text),
                        11.,
                        false,
                        t.tooltip_text,
                    );
                });
        });
    }
}

fn txline(ui: &mut Ui, t: &Theme, item: &TxRow, compact: bool, selected: bool) -> egui::Response {
    let header_stacked = item.is_header && !item.day_balance.is_empty()
        && ui.painter().layout_no_wrap(item.date_label.clone(), font(12., true), t.text_muted).size().x
            + ui.painter().layout_no_wrap(item.day_balance.clone(), font(12., false), t.text_muted).size().x
            + 44. > ui.available_width();
    let (r, response) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            if item.is_header {
                if header_stacked { 52. } else { 34. }
            } else if compact {
                52.
            } else {
                60.
            },
        ),
        if item.is_header {
            Sense::hover()
        } else {
            Sense::click()
        },
    );

    if !ui.is_rect_visible(r) {
        return response;
    }

    let bg = if item.is_header {
        t.header_bg
    } else if selected {
        t.row_selected
    } else if response.hovered() {
        t.row_hover
    } else if item.pending {
        t.pending_bg
    } else {
        Color32::TRANSPARENT
    };

    ui.painter().rect_filled(r, 0., bg);

    if item.is_header {
        let mut date_rect = r.shrink2(vec2(16., 0.));
        let mut balance_rect = date_rect;
        if header_stacked {
            date_rect.max.y = r.center().y;
            balance_rect.min.y = r.center().y;
        }
        elide(
            ui,
            date_rect,
            &item.date_label,
            12.,
            true,
            t.text_muted,
            Align2::LEFT_CENTER,
        );

        elide(
            ui,
            balance_rect,
            &item.day_balance,
            12.,
            false,
            t.text_muted,
            Align2::RIGHT_CENTER,
        );

        return response;
    }

    let avatar = Rect::from_center_size(pos2(r.left() + 33., r.center().y), vec2(34., 34.));

    ui.painter()
        .circle_filled(avatar.center(), 17., item.cat_color.gamma_multiply(0.15));

    elide(
        ui,
        avatar,
        &item.initial,
        14.,
        true,
        item.cat_color
            .lerp_to_gamma(Color32::WHITE, if t.dark { 0.4 } else { 0. }),
        Align2::CENTER_CENTER,
    );

    let mut right = r.right() - 16.;

    elide(
        ui,
        Rect::from_min_max(pos2(right - 110., r.top()), pos2(right, r.bottom())),
        &item.amount,
        14.,
        true,
        if item.negative {
            t.negative
        } else {
            t.positive
        },
        Align2::RIGHT_CENTER,
    );

    right -= 122.;

    if !compact {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    pos2(right - 120., r.center().y - 12.),
                    vec2(120., 24.),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );

        chip(&mut child, t, &item.category, item.cat_color, false);

        right -= 132.;
    }

    if item.pending {
        let rect = Rect::from_min_size(pos2(right - 70., r.center().y - 9.), vec2(70., 18.));

        ui.painter().rect_filled(rect, 9., t.pending_chip);

        elide(
            ui,
            rect,
            tr("vorgemerkt"),
            11.,
            false,
            t.warning,
            Align2::CENTER_CENTER,
        );

        right -= 82.;
    }

    let name = Rect::from_min_max(
        pos2(r.left() + 62., r.center().y - 20.),
        pos2(right, r.center().y),
    );

    elide(ui, name, &item.name, 14., true, t.text, Align2::LEFT_TOP);

    elide(
        ui,
        name.translate(vec2(0., 25.)),
        &if compact || item.account.is_empty() {
            item.purpose.clone()
        } else {
            format!("{}  ·  {}", item.purpose, item.account)
        },
        12.,
        false,
        t.text_muted,
        Align2::LEFT_TOP,
    );

    ui.painter().hline(
        r.x_range(),
        r.bottom() - 0.5,
        egui::Stroke::new(1., t.card_border.gamma_multiply(0.6)),
    );

    response
}

fn accounts(
    ui: &mut Ui,
    t: &Theme,
    d: &mut Data,
    a: &mut Actions,
    drafts: &mut HashMap<i32, (String, String)>,
) {
    title(
        ui,
        t,
        "Konten",
        tr("Konten nach eigenen Gruppen ordnen"),
        |ui| {
            if primary(ui, t, "Umsätze synchronisieren", icons::REFRESH, !d.busy) {
                action!(a, invoke_refresh_all);
            }
        },
    );

    let w = ui.available_width();

    columns(ui, &[w - 165., 153.], 12., |ui, col| {
        if col == 0 {
            let r = input(
                ui,
                t,
                "new-group",
                &mut d.new_group_name,
                "Neue Gruppe, z. B. Privat",
                false,
            );

            if accepted(ui, &r) {
                action!(a, invoke_create_group, d.new_group_name.clone());
            }
        } else if primary(ui, t, "Gruppe anlegen", "", !d.busy) {
            action!(a, invoke_create_group, d.new_group_name.clone());
        }
    });

    ui.add_space(18.);

    for g in d.account_groups.iter() {
        ui.push_id(g.id, |ui| {
            card(ui, t, 16, |ui| {
                let count = format!("{}{}", g.accounts.row_count(), tr(" Konten"));
                let count_width = ui.painter().layout_no_wrap(count.clone(), font(12., false), t.text_muted).size().x;
                let actions_width = count_width + 10. + button_width(ui, tr("Umsätze synchronisieren"), icons::REFRESH, false);
                let w = ui.available_width();
                columns(ui, &[w - actions_width - 12., actions_width], 12., |ui, col| {
                    if col == 0 {
                        if g.id == 0 {
                            ui.add_space(9.);
                            text(ui, &g.name, 14., true, t.text);
                        } else {
                            let draft = drafts.entry(g.id).or_insert_with(|| (g.name.clone(), g.name.clone()));
                            if draft.0 != g.name {
                                *draft = (g.name.clone(), g.name.clone());
                            }
                            let r = input(ui, t, "group-title", &mut draft.1, "", false);
                            if accepted(ui, &r) {
                                action!(a, invoke_rename_group, g.id, draft.1.clone());
                            }
                        }
                    } else {
                        ui.horizontal(|ui| {
                            text(ui, &count, 12., false, t.text_muted);
                            if button(ui, t, tr("Umsätze synchronisieren"), icons::REFRESH, false, !d.busy && g.accounts.row_count() > 0).clicked() {
                                action!(a, invoke_refresh_group, g.id);
                            }
                        });
                    }
                });
                ui.add_space(12.);
                for account in g.accounts.iter() {
                    ui.push_id(account.id, |ui| {
                        let frame = egui::Frame::new().fill(t.header_bg).corner_radius(8.)
                            .inner_margin(egui::Margin { left: 28, right: 12, top: 12, bottom: 12 })
                            .show(ui, |ui| {
                                text(ui, &account.name, 14., true, t.text);
                                ui.add_space(9.);
                                balance_pair(ui, t, &account.balance, &account.booked_balance, account.negative);
                                ui.add_space(9.);
                                text(ui, &account.iban, 12., false, t.text);
                                ui.add_space(9.);
                                text(ui, &format!("{} · {}", account.bank, account.owner), 11., false, t.text_muted);
                                ui.add_space(9.);
                                ui.horizontal(|ui| {
                                    text(ui, tr("Gruppe"), 11., false, t.text_muted);
                                    ui.allocate_ui(vec2(160., 32.), |ui| {
                                        let mut index = account.group_index;
                                        if combo(ui, t, "account-group", &d.group_names, &mut index) {
                                            action!(a, invoke_assign_group, account.id, index);
                                        }
                                    });
                                    right(ui, |ui| {
                                        if ghost(ui, t, "Umsätze", "") {
                                            action!(a, invoke_open_account, account.id);
                                        }
                                        if ghost(ui, t, "Anzeigename ändern", "") {
                                            action!(a, invoke_edit_account_name, account.id);
                                        }
                                    });
                                });
                            });
                        let r = frame.response.rect.shrink(12.);
                        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(4., r.height())), 2., account.color);
                    });
                    ui.add_space(12.);
                }
                if g.accounts.row_count() == 0 {
                    text(ui, tr("Noch keine Konten in dieser Gruppe. Wählen Sie die Gruppe direkt am gewünschten Konto."), 14., false, t.text_muted);
                }
            });
        });
        ui.add_space(18.);
    }
}

fn transactions(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    if ghost(ui, t, "Zurück zu den Konten", icons::BACK) {
        action!(a, invoke_navigate, 1);
    }

    ui.add_space(14.);

    title(ui, t, "Umsätze", &d.tx_summary, |ui| {
        if primary(
            ui,
            t,
            "Umsätze synchronisieren",
            icons::REFRESH,
            !d.busy && d.account_names.row_count() > 1,
        ) {
            action!(a, invoke_refresh_transactions);
        }

        if ghost(ui, t, "camt.052", icons::DOWNLOAD) {
            action!(a, invoke_export_camt);
        }

        if ghost(ui, t, "CSV", icons::DOWNLOAD) {
            action!(a, invoke_export_csv);
        }
    });

    let w = ui.available_width();

    let account_width = 260f32.min(w * 0.26);

    let periods_width = 360f32.min(w * 0.36);

    columns(
        ui,
        &[
            account_width,
            periods_width,
            w - account_width - periods_width - 24.,
        ],
        12.,
        |ui, col| {
            let changed = match col {
                0 => combo(ui, t, "tx-account", &d.account_names, &mut d.tx_account),
                1 => segmented(
                    ui,
                    t,
                    &["30 Tage", "90 Tage", "Dieses Jahr", "Alle"],
                    &mut d.tx_period,
                ),
                _ => input(
                    ui,
                    t,
                    "tx-search",
                    &mut d.tx_search,
                    "Suche nach Name, Zweck, IBAN oder Betrag …",
                    false,
                )
                .changed(),
            };

            if changed {
                action!(a, invoke_tx_filter_changed);
            }
        },
    );

    if let Some(account) = (d.tx_account as usize).checked_sub(1).and_then(|index| d.accounts.row_data(index)) {
        ui.add_space(12.);
        balance_pair(ui, t, &account.balance, &account.booked_balance, account.negative);
    }

    ui.add_space(14.);

    egui::ScrollArea::horizontal()
        .id_salt("categories")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if chip(
                    ui,
                    t,
                    tr("Alle Kategorien"),
                    t.accent,
                    d.tx_category.is_empty(),
                )
                .clicked()
                {
                    d.tx_category.clear();

                    action!(a, invoke_tx_filter_changed);
                }

                for (i, c) in d.categories.iter().enumerate() {
                    if chip(
                        ui,
                        t,
                        &d.category_labels.row_data(i).unwrap_or_default(),
                        d.category_colors.row_data(i).unwrap_or(t.accent),
                        d.tx_category == c,
                    )
                    .clicked()
                    {
                        d.tx_category = if d.tx_category == c { String::new() } else { c };

                        action!(a, invoke_tx_filter_changed);
                    }
                }
            });
        });

    ui.add_space(14.);

    let h = ui.available_height().max(200.);

    columns(ui, &[w - 358., 340.], 18., |ui, col| {
        card(ui, t, if col == 0 { 0 } else { 20 }, |ui| {
            ui.set_min_height(h - 42.);

            if col == 0 {
                egui::ScrollArea::vertical()
                    .id_salt("tx-list")
                    .max_height(h - 2.)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for r in d.tx_rows.iter() {
                            if txline(
                                ui,
                                t,
                                &r,
                                false,
                                d.tx_detail.valid && d.tx_detail.index == r.index && !r.is_header,
                            )
                            .clicked()
                            {
                                action!(a, invoke_tx_select, r.index);
                            }
                        }

                        if d.tx_rows.row_count() == 0 {
                            text(
                                ui,
                                tr("Keine Umsätze für diese Auswahl."),
                                14.,
                                false,
                                t.text_faint,
                            );
                        }
                    });
            } else if !d.tx_detail.valid {
                ui.add_space((h - 140.) / 2.);

                text(ui,tr("Wählen Sie einen Umsatz aus, um Details zu sehen und die Kategorie zu ändern."),14.,false,t.text_faint);
            } else {
                egui::ScrollArea::vertical().id_salt("tx-detail").max_height(h-42.).auto_shrink([false,false]).show(ui,|ui|{

        let detail=&d.tx_detail;
text(ui,&detail.amount,28.,true,if detail.negative{
t.negative}
else{
t.positive}
);
ui.add_space(12.);
text(ui,&detail.name,16.,true,t.text);
ui.add_space(12.);
if detail.pending{
text(ui,tr("Vorgemerkter Umsatz – noch nicht gebucht"),12.,false,t.warning);
ui.add_space(12.);
}
separator(ui,t);
ui.add_space(12.);

        for(label,value)in[("Konto",&detail.account),("Buchungstag",&detail.booking_date),("Wertstellung",&detail.value_date),("IBAN",&detail.iban),("BIC",&detail.bic),("Verwendungszweck",&detail.purpose),("Buchungstext",&detail.posting_text),("End-to-End-Referenz",&detail.eref),("Mandatsreferenz",&detail.mref),("Gläubiger-ID",&detail.creditor_id)]{
if !value.is_empty(){
text(ui,tr(label),11.,false,t.text_faint);
ui.add_space(6.);
text(ui,value,13.,false,t.text);
ui.add_space(12.);
}
}

        separator(ui,t);
ui.add_space(12.);
text(ui,tr("Kategorie"),12.,true,t.text_muted);
ui.add_space(12.);
let mut selected=d.category_labels.iter().position(|c|c==detail.category).unwrap_or(0) as i32;
if combo(ui,t,"detail-category",&d.category_labels,&mut selected){
if let Some(c)=d.categories.row_data(selected as usize){
action!(a,invoke_set_category,detail.index,c);
}
}
ui.add_space(12.);
text(ui,tr("Gleiche Gegenparteien werden künftig automatisch so kategorisiert."),11.,false,t.text_faint);

    }
);
            }
        });
    });
}

fn save_enter(ui: &Ui, r: egui::Response, a: &mut Actions) {
    if accepted(ui, &r) {
        action!(a, invoke_save_transfer);
    }
}

fn transfer(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Überweisung",
        tr("SEPA-Überweisung, Echtzeitüberweisung oder Terminüberweisung"),
        |_| {},
    );

    card(ui, t, 22, |ui| {
        text(ui, tr("Auftraggeberkonto"), 12., true, t.text_muted);

        ui.add_space(14.);

        if combo(
            ui,
            t,
            "transfer-account",
            &d.sepa_accounts,
            &mut d.tf_account,
        ) {
            action!(a, invoke_payment_account_changed);
        }

        ui.add_space(14.);

        separator(ui, t);

        ui.add_space(14.);

        text(ui, tr("Adressbuch dieses Kontos"), 12., true, t.text_muted);

        ui.add_space(14.);

        let w = ui.available_width();

        columns(ui, &[w - 185., 173.], 12., |ui, col| {
            if col == 0 {
                if combo(ui, t, "recipient", &d.recipient_names, &mut d.tf_recipient) {
                    action!(a, invoke_select_recipient, d.tf_recipient);
                }
            } else if button(
                ui,
                t,
                tr("Empfänger löschen"),
                icons::TRASH,
                false,
                !d.busy && d.tf_recipient > 0,
            )
            .clicked()
            {
                action!(a, invoke_delete_recipient);
            }
        });

        ui.add_space(14.);

        let r = field(
            ui,
            t,
            "Empfänger",
            &mut d.tf_name,
            "Name des Empfängers",
            ("", false),
            false,
        );

        save_enter(ui, r, a);

        ui.add_space(14.);

        columns(ui, &[w - 212., 200.], 12., |ui, col| {
            if col == 0 {
                let r = field(
                    ui,
                    t,
                    "IBAN",
                    &mut d.tf_iban,
                    "DE00 0000 0000 0000 0000 00",
                    (&d.tf_iban_hint, d.tf_iban_error),
                    false,
                );

                if r.changed() {
                    action!(a, invoke_check_iban, d.tf_iban.clone());
                }

                save_enter(ui, r, a);
            } else {
                let r = field(
                    ui,
                    t,
                    "BIC (optional)",
                    &mut d.tf_bic,
                    "optional",
                    ("", false),
                    false,
                );

                save_enter(ui, r, a);
            }
        });

        ui.add_space(14.);

        columns(ui, &[180., w - 192.], 12., |ui, col| {
            let r = if col == 0 {
                field(
                    ui,
                    t,
                    "Betrag (EUR)",
                    &mut d.tf_amount,
                    "0,00",
                    ("", false),
                    false,
                )
            } else {
                let count = d.tf_purpose.chars().count();

                field(
                    ui,
                    t,
                    "Verwendungszweck",
                    &mut d.tf_purpose,
                    "max. 140 Zeichen",
                    (&format!("{count} / 140"), count > 140),
                    false,
                )
            };

            save_enter(ui, r, a);
        });

        ui.add_space(14.);

        ui.checkbox(&mut d.tf_save_recipient, tr("Im Adressbuch speichern"));

        ui.add_space(14.);

        text(ui, tr("Ausführung"), 12., true, t.text_muted);

        ui.add_space(14.);

        columns(ui, &[420., 150.], 12., |ui, col| {
            if col == 0 {
                segmented(
                    ui,
                    t,
                    &["Standard", "Echtzeit", "Terminüberweisung"],
                    &mut d.tf_mode,
                );
            } else if d.tf_mode == 2 {
                let r = input(ui, t, "transfer-date", &mut d.tf_date, "TT.MM.JJJJ", false);

                save_enter(ui, r, a);
            }
        });

        ui.add_space(14.);

        text(ui,tr("Beim Synchronisieren werden zuerst die gespeicherten Aufträge der gewählten Konten gesendet, danach die Umsätze abgerufen."),12.,false,t.text_muted);

        ui.add_space(20.);

        right(ui, |ui| {
            if primary(ui, t, "Überweisung speichern", icons::LIST, !d.busy) {
                action!(a, invoke_save_transfer);
            }

            if button(
                ui,
                t,
                tr("Überweisung senden"),
                icons::TRANSFER,
                false,
                !d.busy,
            )
            .clicked()
            {
                action!(a, invoke_send_transfer);
            }
        });
    });
}

fn standing(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Daueraufträge",
        tr("Bestand abrufen, neue Daueraufträge anlegen, bestehende löschen"),
        |ui| {
            if primary(ui, t, "Bestand abrufen", icons::REFRESH, !d.busy) {
                action!(a, invoke_fetch_standing);
            }
        },
    );

    card(ui, t, 12, |ui| {
        for order in d.standing.iter() {
            ui.push_id(order.index, |ui| {
                let w = ui.available_width();

                columns(
                    ui,
                    &[20., (w - 230.) * 0.6, (w - 230.) * 0.4, 110., 34.],
                    14.,
                    |ui, col| match col {
                        0 => {
                            let (r, _) = ui.allocate_exact_size(vec2(20., 65.), Sense::hover());

                            icon(
                                ui,
                                icons::REPEAT,
                                Rect::from_center_size(r.center(), vec2(20., 20.)),
                                t.accent,
                            );
                        }

                        1 | 2 => {
                            let (r, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 65.),
                                Sense::hover(),
                            );
                            let (headline, detail) = if col == 1 {
                                (
                                    order.name.clone(),
                                    format!("{}  ·  {}", order.iban, order.purpose),
                                )
                            } else {
                                (
                                    order.schedule.clone(),
                                    format!("{}  ·  {}", order.next, order.account),
                                )
                            };
                            elide(
                                ui,
                                Rect::from_min_size(r.min + vec2(0., 15.), vec2(r.width(), 18.)),
                                &headline,
                                if col == 1 { 14. } else { 13. },
                                col == 1,
                                t.text,
                                Align2::LEFT_TOP,
                            );
                            elide(
                                ui,
                                Rect::from_min_size(r.min + vec2(0., 37.), vec2(r.width(), 16.)),
                                &detail,
                                12.,
                                false,
                                t.text_muted,
                                Align2::LEFT_TOP,
                            );
                        }
                        3 => {
                            let (r, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 65.),
                                Sense::hover(),
                            );
                            elide(
                                ui,
                                r,
                                &order.amount,
                                14.,
                                true,
                                t.negative,
                                Align2::RIGHT_CENTER,
                            );
                        }
                        _ => {
                            ui.add_space(14.);

                            if ghost(ui, t, "", icons::TRASH) {
                                action!(a, invoke_delete_standing, order.index);
                            }
                        }
                    },
                );

                separator(ui, t);
            });
        }

        if d.standing.row_count() == 0 {
            ui.add_space(12.);

            text(
                ui,
                tr("Noch keine Daueraufträge geladen."),
                14.,
                false,
                t.text_faint,
            );

            ui.add_space(12.);
        }
    });

    ui.add_space(18.);

    card(ui, t, 22, |ui| {
        header(
            ui,
            t,
            "Neuer Dauerauftrag",
            tr("Empfänger, Betrag und Zweck werden aus dem Überweisungsformular übernommen."),
        );

        ui.add_space(6.);

        text(ui, tr("Auftraggeberkonto"), 12., true, t.text_muted);

        ui.add_space(14.);

        if combo(
            ui,
            t,
            "standing-account",
            &d.sepa_accounts,
            &mut d.tf_account,
        ) {
            action!(a, invoke_payment_account_changed);
        }

        ui.add_space(14.);

        let w = ui.available_width();

        columns(
            ui,
            &[(w - 164.) / 2., (w - 164.) / 2., 140.],
            12.,
            |ui, col| match col {
                0 => {
                    field(
                        ui,
                        t,
                        "Empfänger",
                        &mut d.tf_name,
                        "Name",
                        ("", false),
                        false,
                    );
                }

                1 => {
                    if field(
                        ui,
                        t,
                        "IBAN",
                        &mut d.tf_iban,
                        "DE…",
                        (&d.tf_iban_hint, d.tf_iban_error),
                        false,
                    )
                    .changed()
                    {
                        action!(a, invoke_check_iban, d.tf_iban.clone());
                    }
                }

                _ => {
                    field(
                        ui,
                        t,
                        "Betrag (EUR)",
                        &mut d.tf_amount,
                        "0,00",
                        ("", false),
                        false,
                    );
                }
            },
        );

        ui.add_space(14.);

        field(
            ui,
            t,
            "Verwendungszweck",
            &mut d.tf_purpose,
            "z. B. Miete",
            ("", false),
            false,
        );

        ui.add_space(14.);

        let date_width = (w - 236.) / 3.;

        columns(
            ui,
            &[200., date_width, date_width, date_width],
            12.,
            |ui, col| {
                text(
                    ui,
                    tr([
                        "Turnus",
                        "Erste Ausführung",
                        "Ausführungstag",
                        "Letzte Ausführung",
                    ][col]),
                    12.,
                    true,
                    t.text_muted,
                );

                ui.add_space(12.);

                ui.add_space(4.);

                match col {
                    0 => {
                        combo(
                            ui,
                            t,
                            "so-interval",
                            &options(&[
                                "monatlich",
                                "vierteljährlich",
                                "halbjährlich",
                                "jährlich",
                                "wöchentlich",
                            ]),
                            &mut d.so_interval,
                        );
                    }

                    1 => {
                        input(ui, t, "so-first", &mut d.so_first, "TT.MM.JJJJ", false);
                    }

                    2 => {
                        input(ui, t, "so-day", &mut d.so_day, "1–31 / 99 = Ultimo", false);
                    }

                    _ => {
                        input(ui, t, "so-last", &mut d.so_last, "optional", false);
                    }
                }

                ui.add_space(4.);
            },
        );

        ui.add_space(14.);

        right(ui, |ui| {
            if primary(ui, t, "Dauerauftrag anlegen", icons::PLUS, !d.busy) {
                action!(a, invoke_send_standing);
            }
        });
    });
}

fn orders(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(ui,t,"Auftragsliste",tr("Beim Synchronisieren werden zuerst die gespeicherten Aufträge der gewählten Konten gesendet, danach die Umsätze abgerufen."),|_|{
}
);

    for order in d.payment_orders.iter() {
        ui.push_id(order.id, |ui| {
            card(ui, t, 18, |ui| {
                ui.horizontal(|ui| {
                    text(ui, &order.name, 15., true, t.text);

                    right(ui, |ui| {
                        if button(ui, t, "", icons::TRASH, false, !d.busy).clicked() {
                            action!(a, invoke_delete_payment_order, order.id);
                        }

                        text(ui, &order.amount, 14., true, t.text);

                        chip(
                            ui,
                            t,
                            &order.status,
                            if order.needs_review {
                                t.negative
                            } else if order.queued {
                                t.accent
                            } else {
                                t.positive
                            },
                            false,
                        );
                    });
                });

                ui.add_space(8.);

                text(ui, &order.account, 14., false, t.text_muted);

                ui.add_space(8.);

                text(
                    ui,
                    &format!("{} · {}", order.iban, order.execution),
                    14.,
                    false,
                    t.text_muted,
                );

                ui.add_space(8.);

                text(ui, &order.purpose, 14., false, t.text);

                if !order.detail.is_empty() {
                    ui.add_space(8.);

                    text(
                        ui,
                        &order.detail,
                        12.,
                        false,
                        if order.needs_review {
                            t.negative
                        } else {
                            t.text_muted
                        },
                    );
                }
            });
        });

        ui.add_space(14.);
    }

    if d.payment_orders.row_count() == 0 {
        text(
            ui,
            tr("Noch keine Überweisungsaufträge gespeichert."),
            14.,
            false,
            t.text_muted,
        );
    }
}

fn analysis(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Auswertungen",
        tr("Einnahmen und Ausgaben nach Kategorien"),
        |ui| {
            if ghost(ui, t, "›", "") {
                action!(a, invoke_analysis_shift, 1);
            }

            text(ui, &d.analysis_label, 14., true, t.text);

            if ghost(ui, t, "‹", "") {
                action!(a, invoke_analysis_shift, -1);
            }
        },
    );

    let w = (ui.available_width() - 18.) / 2.;

    let h = 75.
        + d.analysis_categories
            .row_count()
            .max(d.analysis_income.row_count()) as f32
            * 46.;

    columns(ui, &[w, w], 18., |ui, col| {
        card(ui, t, 20, |ui| {
            ui.set_min_height(h);

            header(
                ui,
                t,
                if col == 0 { "Ausgaben" } else { "Einnahmen" },
                &d.analysis_label,
            );

            bars(
                ui,
                t,
                if col == 0 {
                    &d.analysis_categories
                } else {
                    &d.analysis_income
                },
            );
        });
    });

    ui.add_space(18.);

    card(ui, t, 20, |ui| {
        header(
            ui,
            t,
            "Jahresverlauf",
            &format!("{}{}", tr("12 Monate bis "), d.analysis_label),
        );

        months(ui, t, &d.analysis_months);
    });
}

fn access_actions(ui: &mut Ui, t: &Theme, access: &AccessRow, a: &mut Actions) {
    right(ui, |ui| {
        if ghost(ui, t, "", icons::TRASH) {
            action!(a, invoke_delete_access, access.id);
        }
        if access.pin_saved && ghost(ui, t, "PIN vergessen", icons::LOCK) {
            action!(a, invoke_forget_pin, access.id);
        }
        if access.kind == "PIN/TAN" && ghost(ui, t, "TAN-Verfahren wählen", "") {
            action!(a, invoke_reset_tan, access.id);
        }
        if ghost(ui, t, "Umsätze synchronisieren", icons::REFRESH) {
            action!(a, invoke_sync_access, access.id);
        }
    });
}

fn accesses(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Bankzugänge",
        tr("FinTS PIN/TAN oder HBCI-Chipkarte"),
        |ui| {
            if primary(ui, t, "Neuer Bankzugang", icons::PLUS, !d.busy) {
                action!(a, invoke_new_access);
            }
        },
    );

    for access in d.accesses.iter() {
        ui.push_id(access.id, |ui| {
            card(ui, t, 18, |ui| {
                let mut actions_width =
                    button_width(ui, tr("Umsätze synchronisieren"), icons::REFRESH, false) + 44.;
                if access.kind == "PIN/TAN" {
                    actions_width += button_width(ui, tr("TAN-Verfahren wählen"), "", false) + 10.;
                }
                if access.pin_saved {
                    actions_width +=
                        button_width(ui, tr("PIN vergessen"), icons::LOCK, false) + 10.;
                }
                let width = ui.available_width();
                let inline = width - actions_width - 70. >= 220.;
                let text_width = if inline {
                    width - actions_width - 70.
                } else {
                    width - 56.
                };
                let widths = if inline {
                    vec![42., text_width, actions_width]
                } else {
                    vec![42., text_width]
                };
                columns(ui, &widths, 14., |ui, col| match col {
                    0 => {
                        ui.add_space(8.);
                        let (r, _) = ui.allocate_exact_size(vec2(42., 42.), Sense::hover());
                        ui.painter().circle_filled(r.center(), 21., t.accent_soft);
                        icon(
                            ui,
                            if access.card_based {
                                icons::READER
                            } else {
                                icons::KEY
                            },
                            Rect::from_center_size(r.center(), vec2(20., 20.)),
                            t.accent,
                        );
                    }
                    1 => {
                        text(ui, &access.name, 15., true, t.text);
                        ui.add_space(6.);
                        text(
                            ui,
                            &format!("{}  ·  {}", access.kind, access.detail),
                            12.,
                            false,
                            t.text_muted,
                        );
                        ui.add_space(6.);
                        text(
                            ui,
                            &format!(
                                "{}{}  ·  {}{}{}",
                                tr("TAN-Verfahren: "),
                                access.tan_method,
                                access.accounts,
                                tr(" Konten"),
                                if access.pin_saved {
                                    tr("  ·  PIN gespeichert (verschlüsselt)")
                                } else {
                                    ""
                                }
                            ),
                            12.,
                            false,
                            t.text_muted,
                        );
                    }
                    _ => {
                        ui.add_space(12.);
                        access_actions(ui, t, &access, a);
                    }
                });
                if !inline {
                    ui.add_space(12.);
                    access_actions(ui, t, &access, a);
                }
            });
        });
        ui.add_space(18.);
    }

    if !d.af_open {
        return;
    }

    card(ui, t, 22, |ui| {
        header(ui, t, "Bankzugang einrichten", "");

        ui.add_space(6.);

        ui.allocate_ui(vec2(560., 34.), |ui| {
            if segmented(
                ui,
                t,
                &[
                    "PIN/TAN (FinTS)",
                    "HBCI-Chipkarte (DDV)",
                    "RDH-Chipkarte (VR-NetWorld-Card)",
                ],
                &mut d.af_kind,
            ) {
                action!(a, invoke_bank_search, d.bank_query.clone());
            }
        });

        ui.add_space(14.);

        text(ui, tr("Bank suchen"), 12., true, t.text_muted);

        ui.add_space(4.);

        if input(
            ui,
            t,
            "bank-search",
            &mut d.bank_query,
            "Name, Ort, BLZ oder BIC – z. B. „Förde Sparkasse“, „ING“, „Volksbank Kiel“",
            false,
        )
        .changed()
        {
            action!(a, invoke_bank_search, d.bank_query.clone());
        }

        if !d.bank_picked.is_empty() {
            ui.add_space(4.);

            text(ui, &d.bank_picked, 11., false, t.positive);
        }

        ui.add_space(14.);

        for bank in d.bank_results.iter() {
            let (r, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 56.), Sense::click());

            if response.hovered() {
                ui.painter().rect_filled(r, 4., t.accent_soft);
            }

            icon(
                ui,
                icons::BANK,
                Rect::from_min_size(r.min + vec2(12., 18.), vec2(20., 20.)),
                t.accent,
            );

            elide(
                ui,
                Rect::from_min_max(r.min + vec2(44., 8.), pos2(r.right() - 200., r.top() + 28.)),
                &bank.name,
                14.,
                true,
                t.text,
                Align2::LEFT_TOP,
            );

            elide(
                ui,
                Rect::from_min_max(r.min + vec2(44., 32.), pos2(r.right() - 200., r.bottom())),
                &bank.detail,
                12.,
                false,
                t.text_muted,
                Align2::LEFT_TOP,
            );

            elide(
                ui,
                Rect::from_min_max(pos2(r.right() - 190., r.top()), r.max - vec2(12., 0.)),
                &bank.endpoint,
                11.,
                false,
                t.text_faint,
                Align2::RIGHT_CENTER,
            );

            separator(ui, t);

            if response.clicked() {
                action!(a, invoke_bank_pick, bank.index);
            }
        }

        field(
            ui,
            t,
            "Bezeichnung",
            &mut d.af_name,
            "z. B. Förde Sparkasse – privat",
            ("", false),
            false,
        );

        ui.add_space(14.);

        let w = ui.available_width();

        if d.af_kind == 0 {
            columns(
                ui,
                &[160., (w - 184.) / 2., (w - 184.) / 2.],
                12.,
                |ui, col| match col {
                    0 => {
                        field(
                            ui,
                            t,
                            "Bankleitzahl",
                            &mut d.af_blz,
                            "8 Ziffern",
                            ("", false),
                            false,
                        );
                    }

                    1 => {
                        field(
                            ui,
                            t,
                            "Anmeldename / Benutzerkennung",
                            &mut d.af_user,
                            "",
                            ("", false),
                            false,
                        );
                    }

                    _ => {
                        field(
                            ui,
                            t,
                            "Kunden-ID (optional)",
                            &mut d.af_customer,
                            "",
                            ("", false),
                            false,
                        );
                    }
                },
            );

            ui.add_space(14.);

            field(
                ui,
                t,
                "FinTS-Server-URL",
                &mut d.af_url,
                "https://…",
                ("", false),
                false,
            );

            text(ui,tr("Wird über die Banksuche ausgefüllt. Abweichende Adressen finden Sie im Onlinebanking Ihrer Bank (Stichwort „FinTS“). Die PIN wird nicht gespeichert."),11.,false,t.text_faint);

            ui.add_space(14.);
            text(ui, tr("Umsatzformat"), 12., true, t.text_muted);
            ui.add_space(8.);
            combo(ui, t, "statement-format", &options(&[
                "Automatisch (CAMT bevorzugen)", "CAMT bevorzugen", "MT940 bevorzugen (ältere Zugänge)",
            ]), &mut d.af_statement_format);
            text(ui, tr("Die Auswahl gilt nur für Formate, die Ihre Bank für das Konto anbietet."), 11., false, t.text_faint);
            ui.add_space(14.);
        } else {
            columns(ui, &[w - 354., 120., 210.], 12., |ui, col| {
                text(
                    ui,
                    if col == 0 {
                        tr("Kartenleser")
                    } else if col == 1 {
                        tr("Karteneintrag")
                    } else {
                        ""
                    },
                    12.,
                    true,
                    t.text_muted,
                );

                ui.add_space(10.);

                match col {
                    0 => {
                        combo(ui, t, "card-reader", &d.reader_names, &mut d.af_reader);
                    }

                    1 => {
                        input(ui, t, "card-record", &mut d.af_record, "1–5", false);
                    }

                    _ => {
                        if ghost(ui, t, "Daten von Karte lesen", icons::READER) {
                            action!(a, invoke_read_card);
                        }
                    }
                }
            });

            ui.add_space(14.);

            columns(
                ui,
                &[160., (w - 184.) / 3., (w - 184.) * 2. / 3.],
                12.,
                |ui, col| {
                    let (label, value) = match col {
                        0 => ("Bankleitzahl", &mut d.af_blz),
                        1 => ("Benutzerkennung", &mut d.af_user),
                        _ => ("Server (Host oder URL)", &mut d.af_url),
                    };

                    text(ui, tr(label), 12., true, t.text_muted);

                    ui.add_space(6.);

                    input(ui, t, label, value, "", false);
                },
            );

            ui.add_space(6.);

            text(
                ui,
                tr(if d.af_kind == 2 {
                    "Von der Karte gelesen (RDH, PIN am Leser); HBCI über TCP-Port 3000."
                } else {
                    "Von der Karte gelesen; HBCI über TCP-Port 3000."
                }),
                11.,
                false,
                t.text_faint,
            );

            ui.add_space(14.);
        }

        right(ui, |ui| {
            if primary(ui, t, "Speichern & verbinden", icons::CHECK, !d.busy) {
                action!(a, invoke_save_access);
            }

            if ghost(ui, t, "Abbrechen", "") {
                d.af_open = false;
            }
        });
    });
}

fn reader(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Kartenleser",
        tr("PC/SC-Leser für HBCI-Chipkarte und chipTAN USB"),
        |ui| {
            if primary(ui, t, "Leser & Karte prüfen", icons::CHECK, !d.busy) {
                action!(a, invoke_reader_diagnose);
            }

            if ghost(ui, t, "Neu suchen", icons::REFRESH) {
                action!(a, invoke_reader_refresh);
            }
        },
    );

    if d.cyberjack_found {
        egui::Frame::new().fill(t.success_bg).stroke(egui::Stroke::new(1.,t.success_border)).corner_radius(10.).inner_margin(16).show(ui,|ui|{
text(ui,tr("REINER SCT cyberJack erkannt. BankingCraft nutzt automatisch die sichere PIN-Eingabe am Pinpad und – sofern der Leser es meldet – chipTAN USB mit Anzeige der Auftragsdaten im Display."),14.,false,t.success_text);
}
);

        ui.add_space(18.);
    }

    card(ui, t, 20, |ui| {
        header(
            ui,
            t,
            "Leser für chipTAN USB",
            tr("Wird im TAN-Dialog verwendet"),
        );

        ui.allocate_ui(vec2(380., 32.), |ui| {
            combo(ui, t, "tan-reader", &d.reader_names, &mut d.tan_reader);
        });
    });

    ui.add_space(18.);

    card(ui, t, 20, |ui| {
        header(
            ui,
            t,
            "Diagnose",
            tr("Gefundene Leser, gemeldete Features und erkannte Karte"),
        );

        for line in d.reader_report.iter() {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(12.)
                    .color(t.text),
            );

            ui.add_space(10.);
        }

        if d.reader_report.row_count() == 0 {
            text(
                ui,
                tr("Noch keine Prüfung durchgeführt."),
                14.,
                false,
                t.text_faint,
            );
        }
    });
}

fn settings(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(ui, t, "Einstellungen", "", |_| {});

    card(ui, t, 22, |ui| {
        header(
            ui,
            t,
            "Tresore",
            tr("Getrennte Tresore für unterschiedliche Konten und Zugangsdaten."),
        );

        ui.add_space(4.);

        text(ui, &d.active_vault_name, 16., true, t.text);

        ui.add_space(12.);

        text(
            ui,
            &format!("{}{}", tr("Speicherort: "), d.data_path),
            12.,
            false,
            t.text_muted,
        );

        ui.add_space(12.);

        if primary(ui, t, "Tresor wechseln / anlegen", icons::LOCK, !d.busy) {
            action!(a, invoke_manage_vaults);
        }
    });

    ui.add_space(18.);

    card(ui, t, 22, |ui| {
        header(
            ui,
            t,
            "Sprache",
            tr("Wählen Sie die Sprache der Anwendung."),
        );

        ui.add_space(4.);

        ui.allocate_ui(vec2(320., 32.), |ui| {
            ui.add_enabled_ui(!d.busy, |ui| {
                if combo(
                    ui,
                    t,
                    "language",
                    &options(&[
                        "Systemeinstellung",
                        "Deutsch",
                        "English",
                        "Français",
                        "Español",
                    ]),
                    &mut d.language_mode,
                ) {
                    action!(a, invoke_change_language, d.language_mode);
                }
            });
        });

        ui.add_space(12.);

        text(
            ui,
            tr(if d.language_mode == 0 {
                "Verwendet die Systemsprache, sofern verfügbar. Andernfalls wird Englisch verwendet."
            } else {
                "Ihre Auswahl wird sofort angewendet und für den nächsten Start gespeichert."
            }),
            13.,
            false,
            t.text_muted,
        );
    });

    ui.add_space(18.);

    card(ui, t, 22, |ui| {
        header(
            ui,
            t,
            "Erscheinungsbild",
            tr("Wählen Sie ein helles oder dunkles Design."),
        );

        ui.add_space(4.);

        ui.allocate_ui(vec2(320., 32.), |ui| {
            if combo(
                ui,
                t,
                "theme",
                &options(&["Systemeinstellung", "Hell", "Dunkel"]),
                &mut d.theme_mode,
            ) {
                action!(a, invoke_change_theme, d.theme_mode);
            }
        });

        ui.add_space(12.);

        text(
            ui,
            tr(if d.theme_mode == 0 {
                "Folgt automatisch dem Design Ihres Betriebssystems. Änderungen werden sofort übernommen."
            } else {
                "Ihre Auswahl wird sofort angewendet und für den nächsten Start gespeichert."
            }),
            13.,
            false,
            t.text_muted,
        );
    });

    ui.add_space(18.);

    card(ui, t, 22, |ui| {
        header(
            ui,
            t,
            "Tresor-Passwort ändern",
            tr("Alle Daten werden lokal mit AES-256-GCM verschlüsselt (Schlüssel per Argon2id)."),
        );

        ui.add_space(6.);

        let w = (ui.available_width() - 12.) / 2.;

        columns(ui, &[w, w], 12., |ui, col| {
            if col == 0 {
                field(
                    ui,
                    t,
                    "Aktuelles Passwort",
                    &mut d.old_password,
                    "",
                    ("", false),
                    true,
                );
            } else {
                field(
                    ui,
                    t,
                    "Neues Passwort",
                    &mut d.new_password,
                    "",
                    ("", false),
                    true,
                );
            }
        });

        ui.add_space(14.);

        right(ui, |ui| {
            if primary(ui, t, "Passwort ändern", icons::LOCK, !d.busy) {
                action!(a, invoke_change_password);
            }
        });
    });

    ui.add_space(18.);

    card(ui, t, 22, |ui| {
        header(ui, t, "Daten", "");

        if ghost(ui, t, "Demo-Bankzugang hinzufügen", "") {
            action!(a, invoke_load_demo);
        }
    });
}
