//! Vault-local activity history. Never record callback arguments, bank replies,
//! challenges, native state, credentials, or diagnostic output.
use crate::{store::Vault, ui::ModelRc, AppState, AppWindow};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Running,
    Success,
    Error,
    Cancelled,
    Interrupted,
    NeedsReview,
}
impl Status {
    pub fn label(self) -> &'static str {
        i18n::tr(match self {
            Self::Running => "Läuft",
            Self::Success => "Erfolgreich",
            Self::Error => "Fehler",
            Self::Cancelled => "Abgebrochen",
            Self::Interrupted => "Unterbrochen",
            Self::NeedsReview => "Bitte prüfen",
        })
    }
    pub fn result(ok: bool, cancelled: bool) -> Self {
        if cancelled {
            Self::Cancelled
        } else if ok {
            Self::Success
        } else {
            Self::Error
        }
    }
}

/// Only the fields needed to review and prepare a NEW transfer are retained.
/// Protocol identifiers, debtor identity and bank messages are deliberately absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transfer {
    pub order_id: u32,
    pub account_id: u32,
    pub name: String,
    pub iban: String,
    pub bic: String,
    pub amount: rust_decimal::Decimal,
    pub purpose: String,
    pub instant: bool,
    pub demo: bool,
}
impl Transfer {
    pub fn from_order(v: &Vault, o: &crate::payments::PaymentOrder) -> Self {
        Self {
            order_id: o.id,
            account_id: o.account_id,
            name: o.transfer.creditor_name.clone(),
            iban: o.transfer.creditor_iban.clone(),
            bic: o.transfer.creditor_bic.clone(),
            amount: o.transfer.amount,
            purpose: o.transfer.purpose.clone(),
            instant: o.transfer.instant,
            demo: v
                .account(o.account_id)
                .and_then(|a| v.access(a.access_id))
                .is_some_and(|a| a.kind == crate::store::AccessKind::Demo),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub time: DateTime<Utc>,
    pub finished: Option<DateTime<Utc>>,
    pub action: String,
    pub status: Status,
    pub transfer: Option<Transfer>,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub save_failed: bool,
}
// Compatibility for action keys in vaults written by earlier app builds.
fn canonical_action(action: &str) -> &str {
    match action {
        "Kontenrundruf" | "Kontenrundruf …" | "Kontenrundruf ..." => "Konten synchronisieren",
        _ => action,
    }
}
impl Entry {
    pub fn action_label(&self) -> &str {
        // Progress punctuation is not part of an activity's description. Keep
        // the complete wording while avoiding a misleading truncation marker.
        i18n::tr(canonical_action(&self.action))
            .trim_end()
            .trim_end_matches('…')
            .trim_end_matches("...")
            .trim_end()
    }

    pub fn timestamp(&self) -> String {
        self.time
            .with_timezone(&Local)
            .format("%d.%m.%Y %H:%M:%S")
            .to_string()
    }
    pub fn detail(&self) -> String {
        if let Some(t) = &self.transfer {
            format!(
                "{} · {} · #{}{}",
                t.name,
                crate::view::fmt_eur(t.amount),
                t.order_id,
                if t.demo { " · Demo" } else { "" }
            )
        } else {
            String::new()
        }
    }
    pub fn note(&self) -> &'static str {
        i18n::tr(if self.save_failed {
            "Lokales Speichern fehlgeschlagen. Dieser Eintrag ist möglicherweise nur für diese Sitzung verfügbar."
        } else if self.legacy {
            "Übernommener Auftragsstatus. Der Zeitpunkt der Bankbestätigung ist nicht bekannt."
        } else {
            match self.status {
        Status::NeedsReview | Status::Interrupted => "Ergebnis unklar. Vor einer Wiederholung den Bankstatus prüfen.",
        Status::Cancelled => "Vorgang abgebrochen. Bereits übermittelte Aufträge sind dadurch nicht storniert.",
        Status::Error => "Vorgang nicht vollständig abgeschlossen. Die Fehlermeldung wurde im Dialog angezeigt.",
        Status::Success if self.action=="Überweisung gespeichert" => "Auftrag gespeichert. Noch keine Bankbestätigung.",
        Status::Success if self.transfer.as_ref().is_some_and(|t|t.demo) => "Demo-Auftrag erfolgreich simuliert.",
        Status::Success if self.transfer.is_some() => "Die Bank hat den Auftrag angenommen.",
        _ => "",
    }
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub initialized: bool,
    pub entries: Vec<Entry>,
}
impl History {
    pub fn add(&mut self, action: &'static str, status: Status, transfer: Option<Transfer>) -> u64 {
        let id = self.entries.last().map_or(1, |e| e.id + 1);
        let time = Utc::now();
        self.entries.push(Entry {
            id,
            time,
            finished: (status != Status::Running).then_some(time),
            action: canonical_action(action).into(),
            status,
            transfer,
            legacy: false,
            save_failed: false,
        });
        id
    }
    pub fn finish(&mut self, id: u64, status: Status) {
        if let Ok(i) = self.entries.binary_search_by_key(&id, |e| e.id) {
            self.entries[i].status = status;
            self.entries[i].finished = Some(Utc::now());
        }
    }
    pub fn save_failed(&mut self, id: u64) {
        if let Ok(i) = self.entries.binary_search_by_key(&id, |e| e.id) {
            self.entries[i].save_failed = true;
        }
    }
}
/// Called only after decryption, never from a refresh. Recover unfinished jobs
/// without retrying them; carry known accepted orders into the new history once.
pub fn opened(v: &mut Vault) {
    for e in &mut v.history.entries {
        e.action = canonical_action(&e.action).to_owned();
        if e.status == Status::Running {
            e.status = Status::Interrupted;
            e.finished = Some(Utc::now());
        }
    }
    if !v.history.initialized {
        v.history.initialized = true;
        let old: Vec<_> = v
            .payment_orders
            .iter()
            .filter(|o| o.status == crate::payments::OrderStatus::Sent)
            .map(|o| Transfer::from_order(v, o))
            .collect();
        for t in old {
            v.history
                .add("Übernommene Überweisung", Status::Success, Some(t));
            v.history.entries.last_mut().unwrap().legacy = true;
        }
    }
    v.history.add("Tresor geöffnet", Status::Success, None);
}
pub fn refresh(app: &AppWindow, v: &Vault) {
    app.global::<AppState>()
        .set_history_entries(ModelRc::new(v.history.entries.clone()));
}

/// Record metadata for non-banking UI actions. A recording failure is visible;
/// it must never silently change the result of an already executed bank job.
pub fn record(app: &AppWindow, action: &'static str, status: Status) {
    if !app.global::<AppState>().get_unlocked() {
        return;
    }
    let Some(c) = crate::CTX.with(|c| c.borrow().clone()) else {
        return;
    };
    if c.bridge.locked.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    c.record_activity(action, status);
    refresh(app, &c.vault.lock().unwrap());
}

pub fn install(app: &AppWindow) {
    let weak = app.as_weak();
    app.global::<AppState>()
        .on_repeat_history(move |id| repeat(&weak.unwrap(), id));
}
pub fn repeat(app: &AppWindow, id: u64) {
    let st = app.global::<AppState>();
    let c = crate::ctx();
    if !st.get_unlocked()
        || c.running.load(std::sync::atomic::Ordering::Acquire)
        || c.bridge.locked.load(std::sync::atomic::Ordering::Acquire)
    {
        return;
    }
    let transfer = c
        .vault
        .lock()
        .unwrap()
        .history
        .entries
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.transfer.clone());
    let Some(t) = transfer else {
        return;
    };
    let index = crate::VIEW.with(|v| {
        v.borrow()
            .sepa_accounts
            .iter()
            .position(|a| *a == t.account_id)
    });
    let Some(index) = index else {
        crate::toast(
            app,
            i18n::tr("Das ursprüngliche Auftraggeberkonto ist nicht mehr verfügbar."),
            true,
        );
        return;
    };
    crate::clear_transfer_form(app);
    st.set_tf_account(index as i32);
    st.set_tf_name(t.name);
    st.set_tf_iban(t.iban);
    st.set_tf_bic(t.bic);
    st.set_tf_amount(t.amount.to_string().replace('.', ","));
    st.set_tf_purpose(t.purpose);
    st.set_tf_mode(if t.instant { 1 } else { 0 });
    // A previous execution date and end-to-end ID are never reused. The ordinary
    // save/send flow creates a new ID and requires the normal confirmation.
    st.set_tf_date(String::new());
    st.set_tf_save_recipient(false);
    st.set_page(3);
    st.set_history_selected(None);
    crate::refresh_view(app);
    st.invoke_check_iban(st.get_tf_iban());
    record(
        app,
        "Wiederholung einer Überweisung vorbereitet",
        Status::Success,
    );
}

/// 0/1=time descending/ascending, 2/3=message type ascending/descending.
/// Secondary ordering is newest first, with stable IDs breaking equal timestamps.
pub fn select<'a>(
    entries: impl Iterator<Item = &'a Entry>,
    query: &str,
    filter: i32,
    sort: i32,
) -> Vec<&'a Entry> {
    let query = query.trim().to_lowercase();
    let mut rows: Vec<_> = entries
        .filter(|e| {
            (filter == 0
                || match filter {
                    1 => e.status == Status::Success,
                    2 => matches!(
                        e.status,
                        Status::Error | Status::NeedsReview | Status::Interrupted
                    ),
                    3 => e.status == Status::Cancelled,
                    4 => e.status == Status::Running,
                    _ => true,
                })
                && (query.is_empty()
                    || format!(
                        "{} {} {} {}",
                        e.action_label(),
                        e.detail(),
                        e.timestamp(),
                        e.status.label()
                    )
                    .to_lowercase()
                    .contains(&query))
        })
        .collect();
    rows.sort_by(|a, b| match sort {
        1 => a.time.cmp(&b.time).then(a.id.cmp(&b.id)),
        2 => a
            .status
            .label()
            .cmp(b.status.label())
            .then(b.time.cmp(&a.time))
            .then(b.id.cmp(&a.id)),
        3 => b
            .status
            .label()
            .cmp(a.status.label())
            .then(b.time.cmp(&a.time))
            .then(b.id.cmp(&a.id)),
        _ => b.time.cmp(&a.time).then(b.id.cmp(&a.id)),
    });
    rows
}
#[cfg(test)]
pub(crate) mod tests;
