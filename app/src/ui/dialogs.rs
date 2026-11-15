use super::{
    render::{action, Actions},
    state::Data,
    theme::{hex, Theme},
    widgets::*,
    Model,
};

use egui::{self, pos2, vec2, Align, Align2, Color32, Layout, Rect, Sense, Ui};

use zeroize::Zeroize;

fn modal(ui: &Ui, t: &Theme, id: &str, width: f32, f: impl FnOnce(&mut Ui)) {
    egui::Modal::new(egui::Id::new(id))
        .backdrop_color(Color32::from_rgba_unmultiplied(11, 20, 36, 160))
        .frame(
            egui::Frame::new()
                .fill(t.card_bg)
                .corner_radius(14.)
                .inner_margin(24),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(width - 48.);

            ui.spacing_mut().item_spacing = vec2(10., 14.);

            f(ui);
        });
}

fn focus(ui: &Ui, id: &str, response: &egui::Response) {
    let frame = ui.ctx().cumulative_frame_nr();

    let key = egui::Id::new(("dialog-focus", id));

    let previous = ui.ctx().data_mut(|d| {
        let p = d.get_temp::<u64>(key);

        d.insert_temp(key, frame);

        p
    });

    if previous.is_none_or(|p| p + 1 < frame) {
        response.request_focus();
    }
}

fn submit_unlock(a: &mut Actions, d: &Data, passwords: &mut [String; 2]) {
    if d.first_run {
        action!(
            a,
            invoke_create_vault,
            std::mem::take(&mut passwords[0]),
            std::mem::take(&mut passwords[1])
        );
    } else {
        action!(a, invoke_unlock, std::mem::take(&mut passwords[0]));
    }

    passwords[0].zeroize();

    passwords[1].zeroize();
}

pub fn unlock(
    ui: &mut Ui,
    t: &Theme,
    d: &mut Data,
    a: &mut Actions,
    passwords: &mut [String; 2],
    revision: &mut i32,
) {
    let changed = *revision != d.vault_selection_revision;

    if changed {
        passwords[0].zeroize();

        passwords[1].zeroize();

        *revision = d.vault_selection_revision;
    }

    let r = ui.max_rect();

    gradient(
        ui,
        r,
        140.,
        &[
            (0., hex(0x13233a)),
            (0.6, hex(0x1e3f6b)),
            (1., hex(0x1e6fd9)),
        ],
    );

    egui::Modal::new(egui::Id::new("vault-unlock")).backdrop_color(Color32::TRANSPARENT).frame(egui::Frame::new().fill(t.card_bg).corner_radius(16.).inner_margin(28)).show(ui.ctx(),|ui|{

        ui.set_width(504.);
ui.set_max_height((r.height()-104.).max(300.));
ui.spacing_mut().item_spacing=vec2(10.,12.);

        egui::ScrollArea::vertical().max_height((r.height()-104.).max(300.)).auto_shrink([false,true]).show(ui,|ui|{

            text(ui,tr(if d.first_run{
"Neuen Tresor anlegen"}
else{
"BankingCraft entsperren"}
),22.,true,t.text);

            if d.recent_vaults.row_count()>1{
text(ui,tr("Zuletzt geöffnete Tresore"),12.,false,t.text_muted);
if combo(ui,t,"vault-selection",&d.recent_vaults,&mut d.selected_vault){
action!(a,invoke_choose_vault,d.selected_vault);
}
}

            ui.horizontal(|ui|{
if ghost(ui,t,"Tresor öffnen …",""){
action!(a,invoke_open_vault_file);
}
if ghost(ui,t,"Neuen Tresor anlegen …",""){
action!(a,invoke_new_vault_file);
}
}
);

            text(ui,&d.selected_vault_name,17.,true,t.text);
text(ui,&d.selected_vault_path,11.,false,t.text_muted);

            text(ui,tr(if d.first_run{
"Legen Sie ein Tresor-Passwort fest. Damit werden Konten, Umsätze und Zugangsdaten lokal verschlüsselt."}
else{
"Geben Sie das Passwort für den ausgewählten Tresor ein."}
),13.,false,t.text_muted);

            let response=input(ui,t,"vault-password",&mut passwords[0],"Tresor-Passwort",true);
focus(ui,"vault-password",&response);
if changed{
response.request_focus();
}
if accepted(ui,&response)&&!d.first_run{
submit_unlock(a,d,passwords);
}

            if d.first_run{
let response=input(ui,t,"vault-password-repeat",&mut passwords[1],"Passwort wiederholen",true);
if accepted(ui,&response){
submit_unlock(a,d,passwords);
}
if d.offer_demo_on_create {
    ui.checkbox(&mut d.create_with_demo, tr("Demokonto mit Beispielumsätzen hinzufügen"));
    text(ui, tr("Fiktive Daten zum Ausprobieren, ohne Verbindung zu einer Bank."), 12., false, t.text_muted);
}
}

            if !d.unlock_error.is_empty(){
text(ui,&d.unlock_error,13.,false,t.negative);
}

            if d.unlocked{
text(ui,tr("Beim Öffnen eines anderen Tresors werden ungespeicherte Formulare verworfen."),11.,false,t.text_muted);
}

            right(ui,|ui|{
if primary(ui,t,if d.first_run{
"Tresor anlegen"}
else{
"Entsperren"}
,"",true){
submit_unlock(a,d,passwords);
}
if d.unlocked&&ghost(ui,t,"Abbrechen",""){
passwords[0].zeroize();
passwords[1].zeroize();
action!(a,invoke_cancel_vault_selection);
}
}
);

        }
);

    }
);
}

pub fn show(
    ui: &mut Ui,
    t: &Theme,
    d: &mut Data,
    a: &mut Actions,
    texture: &mut Option<(super::Image, egui::TextureHandle)>,
) {
    if d.show_vop {
        vop(ui, t, d, a);

        return;
    }

    if d.show_account_name {
        modal(ui, t, "account-name", 540., |ui| {
            text(ui, tr("Anzeigename ändern"), 18., true, t.text);

            text(ui, &d.account_name_context, 14., false, t.text_muted);

            text(
                ui,
                &format!("{}: {}", tr("Kontoname der Bank"), d.account_bank_name),
                14.,
                false,
                t.text_muted,
            );

            text(ui, tr("Anzeigename"), 14., true, t.text);

            let r = input(
                ui,
                t,
                "account-name-draft",
                &mut d.account_name_draft,
                &d.account_bank_name,
                false,
            );

            focus(ui, "account-name", &r);

            if accepted(ui, &r) {
                action!(a, invoke_save_account_name);
            }

            text(
                ui,
                tr("Leer lassen, um den Kontonamen der Bank zu verwenden."),
                14.,
                false,
                t.text_muted,
            );

            ui.horizontal(|ui| {
                if ghost(ui, t, "Originalnamen verwenden", "") {
                    d.account_name_draft.clear();

                    r.request_focus();
                }

                right(ui, |ui| {
                    if primary(ui, t, "Speichern", "", true) {
                        action!(a, invoke_save_account_name);
                    }

                    if ghost(ui, t, "Abbrechen", "") {
                        d.show_account_name = false;
                    }
                });
            });
        });

        return;
    }

    if d.show_confirm {
        modal(ui, t, "confirm", 460., |ui| {
            text(ui, &d.confirm_title, 18., true, t.text);

            text(ui, &d.confirm_text, 14., false, t.text_muted);

            right(ui, |ui| {
                let label = if !d.confirm_ok_text.is_empty() {
                    &d.confirm_ok_text
                } else {
                    tr(if d.confirm_danger {
                        "Löschen"
                    } else {
                        "Ausführen"
                    })
                };

                let mut theme = *t;

                if d.confirm_danger {
                    theme.button_bg = t.danger_button;

                    theme.accent_hover = t.danger_hover;
                }

                if button(ui, &theme, label, "", true, true).clicked() {
                    action!(a, invoke_confirm_ok);
                }

                if ghost(ui, t, "Abbrechen", "") {
                    action!(a, invoke_confirm_cancel);
                }
            });
        });

        return;
    }

    if d.show_choice {
        modal(ui, t, "choice", 460., |ui| {
            text(ui, &d.choice_title, 18., true, t.text);

            egui::ScrollArea::vertical()
                .max_height(ui.ctx().content_rect().height() - 200.)
                .show(ui, |ui| {
                    for c in d.choices.iter() {
                        let (r, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 58.), Sense::click());

                        ui.painter().rect(
                            r,
                            8.,
                            if response.hovered() {
                                t.accent_soft
                            } else {
                                t.card_bg
                            },
                            egui::Stroke::new(
                                1.,
                                if response.hovered() {
                                    t.accent
                                } else {
                                    t.card_border
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );

                        elide(
                            ui,
                            Rect::from_min_max(r.min + vec2(14., 10.), r.max - vec2(14., 28.)),
                            &c.title,
                            14.,
                            true,
                            t.text,
                            Align2::LEFT_TOP,
                        );

                        elide(
                            ui,
                            Rect::from_min_max(r.min + vec2(14., 34.), r.max - vec2(14., 4.)),
                            &c.subtitle,
                            12.,
                            false,
                            t.text_muted,
                            Align2::LEFT_TOP,
                        );

                        if response.clicked() {
                            action!(a, invoke_choice_picked, c.key);
                        }
                    }
                });

            right(ui, |ui| {
                if ghost(ui, t, "Abbrechen", "") {
                    action!(a, invoke_choice_cancel);
                }
            });
        });

        return;
    }

    if d.show_pinpad {
        modal(ui, t, "pinpad", 420., |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(52., 52.), Sense::hover());

                ui.painter().circle_filled(r.center(), 26., t.accent_soft);

                icon(
                    ui,
                    icons::READER,
                    Rect::from_center_size(r.center(), vec2(28., 28.)),
                    t.accent,
                );

                ui.vertical(|ui| {
                    text(ui, tr("Bitte am Kartenleser bestätigen"), 16., true, t.text);

                    text(ui, &d.pinpad_text, 14., false, t.text_muted);
                });
            });
        });

        return;
    }

    if d.show_tan {
        tan(ui, t, d, a, texture);

        return;
    } else {
        *texture = None;
    }

    if d.show_pin {
        modal(ui, t, "pin", 460., |ui| {
            text(
                ui,
                if d.pin_heading.is_empty() {
                    tr("PIN-Eingabe")
                } else {
                    &d.pin_heading
                },
                18.,
                true,
                t.text,
            );

            text(ui, &d.pin_title, 14., false, t.text_muted);

            let w = ui.available_width();

            columns(ui, &[w - 118., 110.], 8., |ui, col| {
                if col == 0 {
                    let r = input(
                        ui,
                        t,
                        "pin-value",
                        &mut d.pin_value,
                        if d.pin_masked { "PIN" } else { "" },
                        d.pin_masked,
                    );

                    focus(ui, "pin", &r);

                    if accepted(ui, &r) {
                        action!(a, invoke_pin_ok);
                    }
                } else if ghost(ui, t, "Einfügen", icons::PASTE) {
                    action!(a, invoke_pin_paste);
                }
            });

            if d.pin_offer_save {
                ui.checkbox(
                    &mut d.pin_save,
                    tr("PIN für diesen Bankzugang speichern (verschlüsselt im Tresor)"),
                );
            }

            text(
                ui,
                if !d.paste_note.is_empty() {
                    &d.paste_note
                } else {
                    tr(if d.pin_save {
                        "Die PIN wird mit AES-256-GCM verschlüsselt im Tresor abgelegt und künftig automatisch verwendet."
                    } else if d.pin_masked {
                        "Die PIN wird nur für diese Sitzung im Arbeitsspeicher gehalten."
                    } else {
                        "Wird zusammen mit dem Bankzugang gespeichert."
                    })
                },
                11.,
                false,
                t.text_faint,
            );

            right(ui, |ui| {
                if primary(ui, t, "OK", "", true) {
                    action!(a, invoke_pin_ok);
                }

                if ghost(ui, t, "Abbrechen", "") {
                    action!(a, invoke_pin_cancel);
                }
            });
        });

        return;
    }

    if d.busy {
        modal(ui, t, "busy", 380., |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(30.));

                text(ui, &d.busy_text, 14., false, t.text);
            });
        });
    }
}

fn tan(
    ui: &mut Ui,
    t: &Theme,
    d: &mut Data,
    a: &mut Actions,
    texture: &mut Option<(super::Image, egui::TextureHandle)>,
) {
    modal(ui, t, "tan", 560., |ui| {
        text(ui, &d.tan_title, 18., true, t.text);

        // Keep the approval/cancel footer reachable even at maximum flicker size.
        egui::ScrollArea::vertical()
            .id_salt("tan-body")
            .max_height((ui.ctx().content_rect().height() - 180.).max(200.))
            .auto_shrink([false, true])
            .show(ui, |ui| {

        egui::ScrollArea::vertical()
            .id_salt("challenge")
            .max_height(170.)
            .show(ui, |ui| {
                text(ui, &d.tan_text, 14., false, t.text);
            });

        if d.tan_kind == 1 {
            let height = d.flicker_width * 0.42;

            let (r, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), height + 26.), Sense::hover());

            ui.painter().rect_filled(r, 8., Color32::BLACK);

            let x = r.center().x - d.flicker_width / 2.;

            let y = r.top() + 22.;

            let spacing = d.flicker_width * 0.04;

            let width = (d.flicker_width - spacing * 4.) / 5.;

            for (i, lit) in [d.flick0, d.flick1, d.flick2, d.flick3, d.flick4]
                .iter()
                .enumerate()
            {
                if *lit {
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            pos2(x + i as f32 * (width + spacing), y),
                            vec2(width, height),
                        ),
                        0.,
                        Color32::WHITE,
                    );
                }
            }

            for ratio in [0.1, 0.9] {
                let center = x + d.flicker_width * ratio;

                ui.painter().add(egui::Shape::convex_polygon(
                    vec![
                        pos2(center - 8., y - 16.),
                        pos2(center + 8., y - 16.),
                        pos2(center, y - 4.),
                    ],
                    Color32::WHITE,
                    egui::Stroke::NONE,
                ));
            }

            ui.horizontal(|ui| {
                if ghost(ui, t, "Langsamer", "") {
                    action!(a, invoke_flicker_speed, -2);
                }

                if ghost(ui, t, "Schneller", "") {
                    action!(a, invoke_flicker_speed, 2);
                }

                if ghost(ui, t, "Kleiner", "") {
                    d.flicker_width = (d.flicker_width - 20.).max(160.);
                }

                if ghost(ui, t, "Größer", "") {
                    d.flicker_width = (d.flicker_width + 20.).min(480.);
                }
            });

            text(ui,&format!("{}{} Hz).",tr("Karte in den TAN-Generator stecken, „F“ drücken und das Gerät an die Markierungen halten ("),d.flicker_hz),12.,false,t.text_muted);
        }

        if d.tan_kind == 2 && d.tan_image.0.width() > 0 && d.tan_image.0.height() > 0 {
            if texture
                .as_ref()
                .is_none_or(|(image, _)| image != &d.tan_image)
            {
                let p = &d.tan_image.0;

                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [p.width() as usize, p.height() as usize],
                    &p.bytes,
                );

                *texture = Some((
                    d.tan_image.clone(),
                    ui.ctx()
                        .load_texture("TAN", image, egui::TextureOptions::NEAREST),
                ));
            }

            if let Some((_, texture)) = texture {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.add(
                        egui::Image::new((texture.id(), texture.size_vec2()))
                            .fit_to_exact_size(vec2(240., 240.))
                            .maintain_aspect_ratio(true),
                    );
                });
            }
        }

        if d.tan_usb_possible {
            egui::Frame::new()
                .fill(t.accent_soft)
                .corner_radius(8.)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal(|ui|{
text(ui,tr("TAN mit chipTAN-USB-Leser erzeugen (Daten am Leser-Display prüfen)"),13.,false,t.text);
if primary(ui,t,"Am Leser","",true){
action!(a,invoke_tan_usb);
}
}
);
                });
        }

        if d.tan_kind == 3 {
            egui::Frame::new().fill(t.accent_soft).corner_radius(8.).inner_margin(12).show(ui,|ui|{
text(ui,tr("Bitte geben Sie den Auftrag in Ihrer Banking-App frei\nund klicken Sie anschließend auf „Freigegeben“."),14.,false,t.text);
}
);
        } else {
            let w = ui.available_width();

            columns(ui, &[w - 118., 110.], 8., |ui, col| {
                if col == 0 {
                    let r = input(ui, t, "tan-value", &mut d.tan_value, "TAN", false);

                    focus(ui, "tan", &r);

                    if accepted(ui, &r) {
                        action!(a, invoke_tan_ok);
                    }
                } else if ghost(ui, t, "Einfügen", icons::PASTE) {
                    action!(a, invoke_tan_paste);
                }
            });
        }

            });

        right(ui, |ui| {
            if d.tan_kind == 3 {
                if primary(ui, t, "Freigegeben", icons::CHECK, true) {
                    action!(a, invoke_tan_confirmed);
                }
            } else if primary(ui, t, "TAN senden", icons::CHECK, true) {
                action!(a, invoke_tan_ok);
            }

            if ghost(ui, t, "Abbrechen", "") {
                action!(a, invoke_tan_cancel);
            }
        });
    });
}

fn vop(ui: &mut Ui, t: &Theme, d: &Data, a: &mut Actions) {
    modal(ui, t, "payee", 640., |ui| {
        egui::ScrollArea::vertical().id_salt("payee-body").max_height((ui.ctx().content_rect().height()-180.).max(200.)).auto_shrink([false,true]).show(ui,|ui|{

            text(ui,tr("Empfängerprüfung"),22.,true,t.text);
text(ui,tr(match d.vop_status{
0=>"Name und IBAN stimmen überein",1=>"Der Empfängername weicht leicht ab",2=>"Name und IBAN stimmen nicht überein",_=>"Empfängerprüfung nicht möglich"}
),17.,true,if d.vop_status==0{
t.success_text}
else{
t.warning}
);

            text(ui,&format!("{}: {}",tr("Empfänger"),d.vop_name),14.,false,t.text);
text(ui,&format!("IBAN: {}",d.vop_iban),14.,false,t.text_muted);

            if !d.vop_suggested_name.is_empty(){
text(ui,&format!("{}: {}",tr("Name bei der Empfängerbank"),d.vop_suggested_name),14.,true,t.text);
if ghost(ui,t,"Namen kopieren",""){
action!(a,invoke_vop_copy_name);
}
}

            if !d.vop_reason.is_empty(){
text(ui,&d.vop_reason,14.,false,t.text_muted);
}
if d.vop_status!=0&&!d.vop_executed{
text(ui,tr("Wenn Sie fortfahren, kann das Geld auf ein Konto gelangen, dessen Inhaber nicht dem angegebenen Empfänger entspricht. Prüfen Sie die Angaben vor der Freigabe."),14.,false,t.text);
}

            if !d.vop_notice.is_empty(){
text(ui,tr("Hinweis Ihrer Bank"),14.,true,t.text);
text(ui,&d.vop_notice,14.,false,t.text_muted);
}
text(ui,tr(if d.vop_executed{
"Die Bank hat den Auftrag bereits ausgeführt."}
else{
"Zum Ändern des Empfängernamens brechen Sie ab und senden den korrigierten Auftrag erneut zur Prüfung."}
),14.,false,t.text_faint);

        }
);

        right(ui, |ui| {
            if primary(
                ui,
                t,
                if d.vop_executed {
                    "Schließen"
                } else if d.vop_status == 0 {
                    "Weiter zur Freigabe"
                } else {
                    "Trotzdem fortfahren"
                },
                "",
                true,
            ) {
                action!(a, invoke_vop_accept);
            }

            if !d.vop_executed && ghost(ui, t, "Abbrechen", "") {
                action!(a, invoke_vop_cancel);
            }
        });
    });
}
