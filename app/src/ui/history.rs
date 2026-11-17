use super::{
    render::{action, Actions},
    state::Data,
    theme::Theme,
    widgets::*,
    Model,
};
use crate::history::{self, Status};
use egui::{vec2, Ui};
use std::sync::Arc;

pub fn show(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    title(
        ui,
        t,
        "Historie",
        tr("Aktionen dieses Tresors · lokal und verschlüsselt gespeichert"),
        |_| {},
    );
    input(
        ui,
        t,
        "history-search",
        &mut d.history_search,
        "Historie durchsuchen …",
        false,
    );
    ui.add_space(12.);
    ui.horizontal_wrapped(|ui| {
        text(ui, tr("Meldungstyp"), 12., true, t.text_muted);
        egui::ComboBox::from_id_salt("history-filter")
            .selected_text(tr([
                "Alle Meldungen",
                "Erfolgreich",
                "Fehler / Bitte prüfen",
                "Abgebrochen",
                "Läuft",
            ][d.history_filter.clamp(0, 4) as usize]))
            .show_ui(ui, |ui| {
                for (i, label) in [
                    "Alle Meldungen",
                    "Erfolgreich",
                    "Fehler / Bitte prüfen",
                    "Abgebrochen",
                    "Läuft",
                ]
                .iter()
                .enumerate()
                {
                    ui.selectable_value(&mut d.history_filter, i as i32, tr(label));
                }
            });
        text(ui, tr("Sortieren"), 12., true, t.text_muted);
        egui::ComboBox::from_id_salt("history-sort")
            .selected_text(tr([
                "Zeitpunkt: neueste zuerst",
                "Zeitpunkt: älteste zuerst",
                "Meldungstyp: A–Z",
                "Meldungstyp: Z–A",
            ][d.history_sort.clamp(0, 3) as usize]))
            .show_ui(ui, |ui| {
                for (i, label) in [
                    "Zeitpunkt: neueste zuerst",
                    "Zeitpunkt: älteste zuerst",
                    "Meldungstyp: A–Z",
                    "Meldungstyp: Z–A",
                ]
                .iter()
                .enumerate()
                {
                    ui.selectable_value(&mut d.history_sort, i as i32, tr(label));
                }
            });
    });
    ui.add_space(12.);
    let rows = history::select(
        d.history_entries.as_slice().iter(),
        &d.history_search,
        d.history_filter,
        d.history_sort,
    );
    text(
        ui,
        &i18n::tr_format!(
            "{} von {} Einträgen",
            rows.len(),
            d.history_entries.row_count()
        ),
        12.,
        false,
        t.text_muted,
    );
    ui.add_space(12.);
    if rows.is_empty() {
        text(
            ui,
            tr("Keine passenden Einträge."),
            14.,
            false,
            t.text_muted,
        );
    }
    // Measure full, wrapped text, then render only the visible variable-height
    // rows. Long entries must neither truncate nor overlap the next entry.
    egui::ScrollArea::vertical()
        .id_salt("history-rows")
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let origin = ui.cursor().min;
            let mut offset = 0.;
            for e in rows {
                let layout = EntryLayout::new(ui, t, e, width);
                let top = offset;
                offset += layout.height;
                if offset < viewport.top() || top > viewport.bottom() {
                    continue;
                }
                let rect =
                    egui::Rect::from_min_size(origin + vec2(0., top), vec2(width, layout.height));
                let response = ui.interact(
                    rect,
                    ui.id().with(("history-entry", e.id)),
                    egui::Sense::click(),
                );
                let label = format!(
                    "{} · {}\n{}\n{}",
                    e.timestamp(),
                    e.status.label(),
                    e.action_label(),
                    e.detail()
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
                });
                let card = rect.shrink2(vec2(0., 3.));
                ui.painter().rect(
                    card,
                    10.,
                    if response.hovered() {
                        t.row_hover
                    } else {
                        t.card_bg
                    },
                    egui::Stroke::new(1., t.card_border),
                    egui::StrokeKind::Inside,
                );
                layout.paint(ui, t, card.min + vec2(16., 12.), width - 32.);
                if response.clicked() {
                    d.history_selected = Some(e.id);
                }
            }
            ui.allocate_space(vec2(width, offset));
        });
    let selected = d
        .history_selected
        .and_then(|id| d.history_entries.iter().find(|e| e.id == id));
    if let Some(e) = selected {
        super::dialogs::modal(ui, t, "history-detail", 580., |ui| {
            egui::ScrollArea::vertical()
                .id_salt("history-detail-body")
                .max_height((ui.ctx().content_rect().height() - 170.).max(120.))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    text(ui, e.action_label(), 22., true, t.text);
                    text(
                        ui,
                        &format!("{} · {}", e.timestamp(), e.status.label()),
                        14.,
                        false,
                        t.text_muted,
                    );
                    if let Some(finished) = e.finished.filter(|f| *f != e.time) {
                        text(
                            ui,
                            &i18n::tr_format!(
                                "Abgeschlossen: {}",
                                finished
                                    .with_timezone(&chrono::Local)
                                    .format("%d.%m.%Y %H:%M:%S")
                            ),
                            12.,
                            false,
                            t.text_muted,
                        );
                    }
                    if let Some(transfer) = &e.transfer {
                        text(ui, &e.detail(), 16., true, t.text);
                        text(ui, &transfer.iban, 14., false, t.text);
                        text(ui, &transfer.purpose, 14., false, t.text);
                    }
                    if !e.note().is_empty() {
                        text(ui, e.note(), 14., false, t.text_muted);
                    }
                    if e.transfer.is_some() {
                        text(
                    ui,
                    tr("Die Wiederholung öffnet eine neue Überweisung zur Prüfung und Freigabe."),
                    13.,
                    false,
                    t.text_muted,
                );
                    }
                });
            right(ui, |ui| {
                if e.transfer.is_some()
                    && primary(
                        ui,
                        t,
                        "Überweisung wiederholen",
                        super::icons::REPEAT,
                        !d.busy && e.status != Status::Running,
                    )
                {
                    action!(a, invoke_repeat_history, e.id);
                }
                if ghost(ui, t, "Schließen", "") {
                    d.history_selected = None;
                }
            });
        });
    }
}

struct EntryLayout {
    time: Arc<egui::Galley>,
    status: Arc<egui::Galley>,
    action: Arc<egui::Galley>,
    detail: Option<Arc<egui::Galley>>,
    metadata_height: f32,
    height: f32,
}
impl EntryLayout {
    fn new(ui: &Ui, t: &Theme, e: &history::Entry, width: f32) -> Self {
        let text_width = (width - 32.).max(1.);
        let metadata_width = ((text_width - 16.) / 2.).max(1.);
        let color = match e.status {
            Status::Success => t.positive,
            Status::Running => t.accent,
            _ => t.negative,
        };
        let time = ui.painter().layout(
            e.timestamp(),
            font(12., false),
            t.text_muted,
            metadata_width,
        );
        let status = ui.painter().layout(
            e.status.label().into(),
            font(12., true),
            color,
            metadata_width,
        );
        let action =
            ui.painter()
                .layout(e.action_label().into(), font(15., true), t.text, text_width);
        let detail = e.detail();
        let detail = (!detail.is_empty()).then(|| {
            ui.painter()
                .layout(detail, font(13., false), t.text_muted, text_width)
        });
        let metadata_height = time.size().y.max(status.size().y);
        let height = 6.
            + 24.
            + metadata_height
            + 8.
            + action.size().y
            + detail.as_ref().map_or(0., |g| 6. + g.size().y);
        Self {
            time,
            status,
            action,
            detail,
            metadata_height,
            height,
        }
    }
    fn paint(&self, ui: &Ui, t: &Theme, origin: egui::Pos2, width: f32) {
        ui.painter().galley(origin, self.time.clone(), t.text_muted);
        ui.painter().galley(
            origin + vec2(width - self.status.size().x, 0.),
            self.status.clone(),
            t.text,
        );
        let y = self.metadata_height + 8.;
        ui.painter()
            .galley(origin + vec2(0., y), self.action.clone(), t.text);
        if let Some(detail) = &self.detail {
            ui.painter().galley(
                origin + vec2(0., y + self.action.size().y + 6.),
                detail.clone(),
                t.text_muted,
            );
        }
    }
}

pub fn templates(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    text(ui, tr("Vorlagen für dieses Konto"), 12., true, t.text_muted);
    ui.add_space(8.);
    ui.horizontal_wrapped(|ui| {
        let label = d
            .templates
            .iter()
            .find(|v| v.id == d.tf_template as u32)
            .map(|v| v.name)
            .unwrap_or_else(|| tr("Vorlage auswählen …").into());
        egui::ComboBox::from_id_salt("transfer-template")
            .width(250.)
            .selected_text(label)
            .show_ui(ui, |ui| {
                for template in d.templates.iter() {
                    if ui
                        .selectable_label(template.id == d.tf_template as u32, &template.name)
                        .clicked()
                        && !d.busy
                    {
                        action!(a, invoke_load_template, template.id as i32);
                    }
                }
            });
        if button(
            ui,
            t,
            tr("Als Vorlage speichern"),
            super::icons::LIST,
            false,
            !d.busy,
        )
        .clicked()
        {
            d.template_name = String::new();
            d.template_error = String::new();
            d.show_template_save = true;
        }
        if button(
            ui,
            t,
            tr("Vorlage löschen"),
            super::icons::TRASH,
            false,
            !d.busy && d.tf_template > 0,
        )
        .clicked()
        {
            action!(a, invoke_delete_template);
        }
    });
    ui.add_space(18.);
}
pub fn template_dialog(ui: &mut Ui, t: &Theme, d: &mut Data, a: &mut Actions) {
    if !d.show_template_save {
        return;
    }
    super::dialogs::modal(ui, t, "save-transfer-template", 540., |ui| {
        text(
            ui,
            tr("Überweisung als Vorlage speichern"),
            22.,
            true,
            t.text,
        );
        text(ui,tr("Empfänger, Betrag und Verwendungszweck werden für das ausgewählte Konto gespeichert."),14.,false,t.text_muted);
        input(
            ui,
            t,
            "template-name",
            &mut d.template_name,
            "Name der Vorlage",
            false,
        );
        if !d.template_error.is_empty() {
            text(ui, &d.template_error, 14., false, t.negative);
        }
        right(ui, |ui| {
            if primary(ui, t, "Vorlage speichern", super::icons::LIST, !d.busy) {
                action!(a, invoke_save_template);
            }
            if ghost(ui, t, "Abbrechen", "") {
                d.show_template_save = false;
                d.template_error.clear();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_galleys_wrap_without_elision_at_each_width() {
        let f = crate::history::tests::Fixture::new();
        let id = f.transfer(Status::Success);
        let mut entry = f
            .ctx
            .vault
            .lock()
            .unwrap()
            .history
            .entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .clone();
        entry.action =
            "Vollständige Aktionsbeschreibung mit mehreren Wörtern und Zeilen ".repeat(6);
        entry.transfer.as_mut().unwrap().name = "Langer vollständiger Empfängername ".repeat(7);
        let ctx = egui::Context::default();
        super::super::render::install_fonts(&ctx);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            for width in [180., 350., 800.] {
                let layout = EntryLayout::new(ui, &Theme::new(true), &entry, width);
                assert_eq!(layout.action.job.text, entry.action_label());
                assert_eq!(layout.detail.as_ref().unwrap().job.text, entry.detail());
                for galley in [
                    &layout.action,
                    layout.detail.as_ref().unwrap(),
                    &layout.time,
                    &layout.status,
                ] {
                    assert!(!galley.elided);
                    assert!(galley.size().x <= width - 32. + 1.);
                }
                assert!(layout.action.rows.len() > 1);
                assert!(
                    layout.height
                        >= layout.metadata_height
                            + layout.action.size().y
                            + layout.detail.as_ref().unwrap().size().y
                            + 30.
                );
            }
        });
        output.textures_delta.clear();
    }
}
