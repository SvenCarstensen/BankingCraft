//! Turns the vault into Slint models.

use crate::categorize::{categorize, color_of, CATEGORIES};
use crate::store::{AccessKind, StoredTx, Vault};
use crate::{AccessRow, AccountRow, AccountGroupRow, GroupBalanceRow, AppState, AppWindow, BarStat, MonthStat, StandingRow, TxDetail, TxRow};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use fints::{Interval, StandingOrder};
use rust_decimal::Decimal;
use slint::{ComponentHandle, Color, ModelRc, SharedString, VecModel};
use std::collections::{BTreeMap, HashSet};

mod overview;
pub use overview::change_group as change_overview_group;

pub fn fmt_eur(d: Decimal) -> String {
    fmt_money(d, "€")
}

pub fn fmt_money(d: Decimal, ccy: &str) -> String {
    let neg = d.is_sign_negative() && !d.is_zero();
    let s = format!("{:.2}", d.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(c);
    }
    let sym = if ccy == "EUR" || ccy.is_empty() { "€" } else { ccy };
    format!("{}{},{} {}", if neg { "−" } else { "" }, grouped, frac, sym)
}

pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%d.%m.%Y")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .ok()
}

pub fn parse_amount(s: &str) -> Option<Decimal> {
    let s = s.trim().replace(['€', ' '], "");
    let s = if s.contains(',') { s.replace('.', "").replace(',', ".") } else { s };
    s.parse::<Decimal>().ok().filter(|d| d.normalize().scale() <= 2)
}

const MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
const WEEKDAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];

fn day_label(d: NaiveDate) -> String {
    let today = Local::now().date_naive();
    let prefix = if d == today {
        i18n::tr("Heute, ").to_string()
    } else if d == today - Duration::days(1) {
        i18n::tr("Gestern, ").to_string()
    } else {
        format!("{}, ", i18n::tr(WEEKDAYS[d.weekday().num_days_from_monday() as usize]))
    };
    i18n::tr_format!("{prefix}{}. {} {}", d.day(), i18n::tr(MONTHS[d.month0() as usize]), d.year())
}

const ACCOUNT_COLORS: [(u8, u8, u8); 6] = [(0x1e, 0x6f, 0xd9), (0x0c, 0x85, 0x99), (0xe8, 0x59, 0x0c), (0x5f, 0x3d, 0xc4), (0x2f, 0x9e, 0x44), (0xc2, 0x25, 0x5c)];

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

pub fn category_label(name: &str) -> &str {
    if CATEGORIES.iter().any(|c| c.name == name) { i18n::tr(name) } else { name }
}

fn ss(s: impl AsRef<str>) -> SharedString {
    SharedString::from(s.as_ref())
}

fn account_kind(kind: u32) -> &'static str {
    match kind {
        1..=9 => i18n::tr("Girokonto"),
        10..=19 => i18n::tr("Sparkonto"),
        20..=29 => i18n::tr("Festgeld"),
        30..=39 => i18n::tr("Depot"),
        40..=49 => i18n::tr("Darlehen"),
        50..=59 => i18n::tr("Kreditkarte"),
        _ => i18n::tr("Konto"),
    }
}

pub fn bank_account_name(account: &fints::Account) -> &str {
    if account.product.is_empty() { account_kind(account.kind) } else { &account.product }
}

pub fn account_label(v: &Vault, id: u32) -> String {
    v.account(id)
        .map(|a| {
            let name = if a.display_name.is_empty() { bank_account_name(&a.account) } else { &a.display_name };
            let bank = v.access(a.access_id).map(|x| x.name.clone()).unwrap_or_default();
            format!("{name} · {bank}")
        })
        .unwrap_or_default()
}

fn short_iban(iban: &str) -> String {
    iban.chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Filter state kept on the Rust side.
#[derive(Default)]
pub struct ViewState {
    pub recipient_ids: Vec<u32>,
    pub account_filter: Vec<u32>,
    pub sepa_accounts: Vec<u32>,
    pub standing_index: Vec<usize>,
    overview: overview::Filters,
}

fn account_group(v: &Vault, id: u32, name: &str, rows: Vec<AccountRow>) -> AccountGroupRow {
    let mut totals = BTreeMap::<String, Decimal>::new();
    let mut known = 0;
    for row in &rows {
        if let Some(balance) = v.account(row.id as u32).and_then(|a| a.balance.as_ref()) {
            let currency = if balance.currency.is_empty() { "EUR" } else { &balance.currency };
            *totals.entry(currency.to_string()).or_default() += balance.booked;
            known += 1;
        }
    }
    let note = if rows.is_empty() {
        i18n::tr("Keine Konten").to_string()
    } else if known == 0 {
        i18n::tr("Noch keine Salden abgerufen").to_string()
    } else if known < rows.len() {
        i18n::tr_format!("Teilsaldo · {known} von {} Kontosalden verfügbar", rows.len())
    } else {
        format!("{} {}", rows.len(), if rows.len() == 1 { i18n::tr("Konto") } else { i18n::tr("Konten") })
    };
    let balances = totals.into_iter().map(|(currency, amount)| GroupBalanceRow {
        amount: ss(fmt_money(amount, &currency)), negative: amount.is_sign_negative(),
    }).collect();
    AccountGroupRow { id: id as i32, name: ss(name), accounts: model(rows), balances: model(balances), balance_note: ss(note) }
}

pub fn refresh_all(app: &AppWindow, v: &Vault, vs: &mut ViewState) {
    let st = app.global::<AppState>();
    let today = Local::now().date_naive();
    let selected_tx = (st.get_tx_account() as usize).checked_sub(1).and_then(|i| vs.account_filter.get(i)).copied();
    let selected_payment = vs.sepa_accounts.get(st.get_tf_account().max(0) as usize).copied();

    // accounts
    let mut rows = Vec::new();
    let mut total = Decimal::ZERO;
    let mut names = vec![ss(i18n::tr("Alle Konten"))];
    vs.account_filter.clear();
    vs.sepa_accounts.clear();
    let mut sepa_names = Vec::new();
    for a in &v.accounts {
        let c = ACCOUNT_COLORS[a.id as usize % ACCOUNT_COLORS.len()];
        let bank = v.access(a.access_id).map(|x| x.name.clone()).unwrap_or_default();
        let (balance, negative, available) = match &a.balance {
            Some(b) => {
                total += b.booked;
                (fmt_money(b.booked, &b.currency), b.booked.is_sign_negative(), b.available.map(|x| fmt_money(x, &b.currency)).unwrap_or_default())
            }
            None => ("–".into(), false, String::new()),
        };
        let kind = account_kind(a.account.kind);
        let bank_name = bank_account_name(&a.account);
        let name = if a.display_name.is_empty() { bank_name } else { &a.display_name };
        rows.push(AccountRow {
            id: a.id as i32,
            group_index: v.groups.iter().position(|g| g.account_ids.contains(&a.id)).map(|i| i as i32 + 1).unwrap_or(0),
            group_name: ss(v.groups.iter().find(|g| g.account_ids.contains(&a.id)).map(|g| g.name.as_str()).unwrap_or(i18n::tr("Ohne Gruppe"))),
            name: ss(name),
            bank: ss(&bank),
            iban: ss(if a.account.iban.is_empty() { a.account.account_number.clone() } else { short_iban(&a.account.iban) }),
            owner: ss(&a.account.owner),
            balance: ss(balance),
            available: ss(available),
            negative,
            last_sync: ss(a.last_sync.map(|t| i18n::tr_format!("aktualisiert {}", t.format("%d.%m.%Y %H:%M"))).unwrap_or_else(|| i18n::tr("noch nicht abgerufen").into())),
            kind: ss(kind),
            color: Color::from_rgb_u8(c.0, c.1, c.2),
        });
        names.push(ss(format!("{name} · {bank}")));
        vs.account_filter.push(a.id);
        if a.account.is_sepa() {
            vs.sepa_accounts.push(a.id);
            sepa_names.push(ss(format!("{name} · {}", short_iban(&a.account.iban))));
        }
    }
    let mut grouped = Vec::new();
    for (index, group) in v.groups.iter().enumerate() {
        grouped.push(account_group(v, group.id, &group.name, rows.iter().filter(|r| r.group_index == index as i32 + 1).cloned().collect()));
    }
    let ungrouped: Vec<_> = rows.iter().filter(|r| r.group_index == 0).cloned().collect();
    if !ungrouped.is_empty() || grouped.is_empty() {
        grouped.push(account_group(v, 0, i18n::tr("Ohne Gruppe"), ungrouped));
    }
    st.set_account_groups(model(grouped));
    st.set_group_names(model(std::iter::once(ss(i18n::tr("Ohne Gruppe"))).chain(v.groups.iter().map(|g| ss(&g.name))).collect()));
    st.set_accounts(model(rows));
    st.set_account_names(model(names));
    st.set_sepa_accounts(model(sepa_names));
    if let Some(id) = selected_tx {
        st.set_tx_account(vs.account_filter.iter().position(|a| *a == id).map(|i| i as i32 + 1).unwrap_or(0));
    }
    if let Some(id) = selected_payment {
        st.set_tf_account(vs.sepa_accounts.iter().position(|a| *a == id).unwrap_or(0) as i32);
    }
    if st.get_tf_account() as usize >= vs.sepa_accounts.len() {
        st.set_tf_account(0);
    }
    crate::payments_ui::refresh(app, v, vs);
    st.set_total_balance(ss(fmt_eur(total)));
    st.set_total_negative(total.is_sign_negative());
    if let Some(t) = v.accounts.iter().filter_map(|a| a.last_sync).max() {
        st.set_last_sync(ss(i18n::tr_format!("Letzter Kontenrundruf: {}", t.format(i18n::tr("%d.%m.%Y um %H:%M Uhr")))));
    } else {
        st.set_last_sync(ss(i18n::tr("Willkommen – richten Sie einen Bankzugang ein oder laden Sie die Demo-Daten.")));
    }

    // categories
    st.set_categories(model(CATEGORIES.iter().map(|c| ss(c.name)).collect()));
    st.set_category_labels(model(CATEGORIES.iter().map(|c| ss(i18n::tr(c.name))).collect()));
    st.set_category_colors(model(CATEGORIES.iter().map(|c| color_of(c.name)).collect()));

    // overview month figures
    let month_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let (inc, exp, _) = month_stats(v, month_start);
    st.set_month_label(ss(format!("{} {}", i18n::tr(MONTHS[today.month0() as usize]), today.year())));
    st.set_month_income(ss(fmt_eur(inc)));
    st.set_month_expense(ss(fmt_eur(-exp)));

    // standing orders
    vs.standing_index.clear();
    let mut standing = Vec::new();
    for i in 0..v.standing.len() {
        standing.push(standing_row(v, i, today));
        vs.standing_index.push(i);
    }
    st.set_standing(model(standing));
    overview::refresh(app, v, &mut vs.overview);

    // accesses
    let accesses = v
        .accesses
        .iter()
        .map(|a| {
            let (kind, detail) = match &a.kind {
                AccessKind::PinTan => ("PIN/TAN", i18n::tr_format!("BLZ {} · {}", a.config.blz, a.config.user_id)),
                AccessKind::ChipCard { reader, record } => (i18n::tr("HBCI-Chipkarte"), i18n::tr_format!("BLZ {} · Eintrag {record} · {reader}", a.config.blz)),
                AccessKind::RdhCard { reader, record } => (i18n::tr("RDH-Chipkarte"), i18n::tr_format!("BLZ {} · {} · Eintrag {record} · {reader}", a.config.blz, a.config.user_id)),
                AccessKind::Demo => ("Demo", i18n::tr("lokale Beispieldaten").to_string()),
            };
            let method = match &a.kind {
                AccessKind::ChipCard { .. } => i18n::tr("Signatur mit Chipkarte").to_string(),
                AccessKind::RdhCard { .. } => i18n::tr("RSA-Signatur mit Chipkarte (RDH)").to_string(),
                AccessKind::Demo => i18n::tr("chipTAN (Demo)").to_string(),
                AccessKind::PinTan => a
                    .state
                    .tan_method
                    .as_ref()
                    .map(|sf| a.state.bpd.tan_methods.iter().find(|m| &m.security_function == sf).map(|m| m.name.clone()).unwrap_or(sf.clone()))
                    .unwrap_or_else(|| i18n::tr("noch nicht gewählt").into()),
            };
            AccessRow {
                id: a.id as i32,
                name: ss(&a.name),
                kind: ss(kind),
                detail: ss(detail),
                tan_method: ss(method),
                accounts: v.accounts.iter().filter(|x| x.access_id == a.id).count() as i32,
                demo: a.kind == AccessKind::Demo,
                pin_saved: a.saved_pin.is_some(),
                card_based: matches!(a.kind, AccessKind::ChipCard { .. } | AccessKind::RdhCard { .. }),
            }
        })
        .collect();
    st.set_accesses(model(accesses));

    refresh_transactions(app, v, vs);
    refresh_analysis(app, v);
}

fn month_stats(v: &Vault, month_start: NaiveDate) -> (Decimal, Decimal, BTreeMap<String, Decimal>) {
    month_stats_for(v, month_start, None)
}

fn month_stats_for(v: &Vault, month_start: NaiveDate, accounts: Option<&HashSet<u32>>) -> (Decimal, Decimal, BTreeMap<String, Decimal>) {
    let month_end = month_start + Months::new(1);
    let mut inc = Decimal::ZERO;
    let mut exp = Decimal::ZERO;
    let mut cats: BTreeMap<String, Decimal> = BTreeMap::new();
    for t in &v.transactions {
        if accounts.is_some_and(|ids| !ids.contains(&t.account_id)) || t.tx.pending || t.tx.date_unknown || t.tx.booking_date < month_start || t.tx.booking_date >= month_end {
            continue;
        }
        let cat = categorize(t, &v.rules);
        if cat == "Umbuchung" {
            continue;
        }
        if t.tx.amount.is_sign_negative() {
            exp += t.tx.amount;
            *cats.entry(cat).or_default() += -t.tx.amount;
        } else {
            inc += t.tx.amount;
        }
    }
    (inc, exp, cats)
}

fn bar_stats(cats: &BTreeMap<String, Decimal>, limit: usize) -> Vec<BarStat> {
    let mut list: Vec<(&String, &Decimal)> = cats.iter().collect();
    list.sort_by(|a, b| b.1.cmp(a.1));
    let max = list.first().map(|x| *x.1).unwrap_or(Decimal::ONE);
    list.into_iter()
        .take(limit)
        .map(|(name, amount)| BarStat {
            label: ss(category_label(name)),
            value: ss(fmt_eur(*amount)),
            ratio: if max.is_zero() { 0.0 } else { (*amount / max).to_string().parse::<f32>().unwrap_or(0.0) },
            color: color_of(name),
        })
        .collect()
}

fn month_series(v: &Vault, last_month_start: NaiveDate, count: u32) -> Vec<MonthStat> {
    month_series_for(v, last_month_start, count, None)
}

fn month_series_for(v: &Vault, last_month_start: NaiveDate, count: u32, accounts: Option<&HashSet<u32>>) -> Vec<MonthStat> {
    let mut data = Vec::new();
    for k in (0..count).rev() {
        let start = last_month_start - Months::new(k);
        let (inc, exp, _) = month_stats_for(v, start, accounts);
        data.push((start, inc, -exp));
    }
    let max = data.iter().map(|x| x.1.max(x.2)).max().unwrap_or(Decimal::ONE).max(Decimal::ONE);
    data.into_iter()
        .map(|(s, inc, exp)| MonthStat {
            label: ss(format!("{} {:02}", i18n::tr(MONTHS_SHORT[s.month0() as usize]), s.year() % 100)),
            income: (inc / max).to_string().parse().unwrap_or(0.0),
            expense: (exp / max).to_string().parse().unwrap_or(0.0),
            income_text: ss(fmt_eur(inc)),
            expense_text: ss(fmt_eur(exp)),
        })
        .collect()
}

pub fn refresh_analysis(app: &AppWindow, v: &Vault) {
    let st = app.global::<AppState>();
    let today = Local::now().date_naive();
    let offset = st.get_analysis_offset();
    let base = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let start = if offset < 0 { base - Months::new((-offset) as u32) } else { base + Months::new(offset as u32) };
    st.set_analysis_label(ss(format!("{} {}", i18n::tr(MONTHS[start.month0() as usize]), start.year())));
    let (_, _, cats) = month_stats(v, start);
    st.set_analysis_categories(model(bar_stats(&cats, 15)));
    // income by category
    let end = start + Months::new(1);
    let mut inc: BTreeMap<String, Decimal> = BTreeMap::new();
    for t in &v.transactions {
        if !t.tx.pending && !t.tx.date_unknown && t.tx.booking_date >= start && t.tx.booking_date < end && !t.tx.amount.is_sign_negative() {
            let c = categorize(t, &v.rules);
            if c != "Umbuchung" {
                *inc.entry(c).or_default() += t.tx.amount;
            }
        }
    }
    st.set_analysis_income(model(bar_stats(&inc, 10)));
    st.set_analysis_months(model(month_series(v, start, 12)));
}

pub fn tx_row(v: &Vault, i: usize, with_account: bool) -> TxRow {
    let t = &v.transactions[i];
    let cat = categorize(t, &v.rules);
    let name = if t.tx.counterparty_name.is_empty() { t.tx.posting_text.clone() } else { t.tx.counterparty_name.clone() };
    let purpose = if t.tx.purpose.is_empty() { t.tx.posting_text.clone() } else { t.tx.purpose.clone() };
    TxRow {
        is_header: false,
        date_label: SharedString::default(),
        day_balance: SharedString::default(),
        index: i as i32,
        initial: ss(name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())),
        name: ss(if name.is_empty() { i18n::tr("Umsatz").into() } else { name }),
        purpose: ss(purpose.replace('\n', " ")),
        amount: ss(fmt_money(t.tx.amount, &t.tx.currency)),
        negative: t.tx.amount.is_sign_negative(),
        pending: t.tx.pending,
        cat_color: color_of(&cat),
        category: ss(category_label(&cat)),
        account: ss(if with_account { account_label(v, t.account_id) } else { String::new() }),
    }
}

fn transaction_indices(v: &Vault, account: Option<u32>, since: Option<NaiveDate>, search: &str, cat_filter: &str) -> Vec<usize> {
    let search = search.to_lowercase();
    let mut idx: Vec<usize> = (0..v.transactions.len())
        .filter(|&i| {
            let t: &StoredTx = &v.transactions[i];
            if let Some(a) = account {
                if t.account_id != a {
                    return false;
                }
            }
            if let Some(s) = since {
                if !t.tx.date_unknown && t.tx.booking_date < s {
                    return false;
                }
            }
            if !cat_filter.is_empty() && categorize(t, &v.rules) != cat_filter {
                return false;
            }
            if !search.is_empty() {
                let hay = format!(
                    "{} {} {} {} {}",
                    t.tx.counterparty_name, t.tx.purpose, t.tx.counterparty_iban, t.tx.posting_text, fmt_money(t.tx.amount, "")
                )
                .to_lowercase();
                if !search.split_whitespace().all(|w| hay.contains(w)) {
                    return false;
                }
            }
            true
        })
        .collect();
    // pending first, then newest booking date
    idx.sort_by(|&a, &b| {
        let ta = &v.transactions[a].tx;
        let tb = &v.transactions[b].tx;
        tb.pending.cmp(&ta.pending).then(tb.date_unknown.cmp(&ta.date_unknown)).then(tb.booking_date.cmp(&ta.booking_date)).then(b.cmp(&a))
    });

    idx
}

pub fn refresh_transactions(app: &AppWindow, v: &Vault, vs: &ViewState) {
    let st = app.global::<AppState>();
    let today = Local::now().date_naive();
    let acc_idx = st.get_tx_account();
    let account: Option<u32> = if acc_idx <= 0 { None } else { vs.account_filter.get(acc_idx as usize - 1).copied() };
    let since = match st.get_tx_period() {
        0 => Some(today - Duration::days(30)),
        1 => Some(today - Duration::days(90)),
        2 => NaiveDate::from_ymd_opt(today.year(), 1, 1),
        _ => None,
    };
    let search = st.get_tx_search().to_lowercase();
    let cat_filter = st.get_tx_category().to_string();

    let idx = transaction_indices(v, account, since, &search, &cat_filter);

    // running end-of-day balance (only meaningful for a single account with known balance)
    let mut running = account.and_then(|a| v.account(a)).and_then(|a| a.balance.as_ref()).map(|b| b.booked);
    let mut rows = Vec::new();
    let mut sum_in = Decimal::ZERO;
    let mut sum_out = Decimal::ZERO;
    let mut i = 0;
    while i < idx.len() {
        let first = &v.transactions[idx[i]].tx;
        let pending = first.pending;
        let date = first.booking_date;
        let unknown = first.date_unknown;
        if unknown && !pending { running = None; }
        let mut j = i;
        let mut day_sum = Decimal::ZERO;
        while j < idx.len() && v.transactions[idx[j]].tx.pending == pending && (pending || (v.transactions[idx[j]].tx.date_unknown == unknown && (unknown || v.transactions[idx[j]].tx.booking_date == date))) {
            day_sum += v.transactions[idx[j]].tx.amount;
            j += 1;
        }
        let label = if pending { i18n::tr("Vorgemerkte Umsätze").to_string() } else if unknown { i18n::tr("Buchungsdatum nicht geliefert").to_string() } else { day_label(date) };
        let balance_text = if pending {
            String::new()
        } else if let Some(r) = running {
            let s = i18n::tr_format!("Tagessaldo {}", fmt_eur(r));
            running = Some(r - day_sum);
            s
        } else {
            i18n::tr_format!("Tagessumme {}", fmt_eur(day_sum))
        };
        rows.push(TxRow {
            is_header: true,
            date_label: ss(label),
            day_balance: ss(balance_text),
            ..Default::default()
        });
        for &k in &idx[i..j] {
            let a = v.transactions[k].tx.amount;
            if !v.transactions[k].tx.pending {
                if a.is_sign_negative() {
                    sum_out += a;
                } else {
                    sum_in += a;
                }
            }
            rows.push(tx_row(v, k, account.is_none()));
        }
        i = j;
    }
    st.set_tx_summary(ss(i18n::tr_format!(
        "{} Umsätze · Einnahmen {} · Ausgaben {}",
        idx.len(),
        fmt_eur(sum_in),
        fmt_eur(-sum_out)
    )));
    st.set_tx_rows(model(rows));
}

pub fn show_detail(app: &AppWindow, v: &Vault, i: usize) {
    let st = app.global::<AppState>();
    let Some(t) = v.transactions.get(i) else {
        st.set_tx_detail(TxDetail::default());
        return;
    };
    st.set_tx_detail(TxDetail {
        valid: true,
        index: i as i32,
        name: ss(if t.tx.counterparty_name.is_empty() { &t.tx.posting_text } else { &t.tx.counterparty_name }),
        iban: ss(short_iban(&t.tx.counterparty_iban)),
        bic: ss(&t.tx.counterparty_bic),
        amount: ss(fmt_money(t.tx.amount, &t.tx.currency)),
        negative: t.tx.amount.is_sign_negative(),
        booking_date: ss(if t.tx.date_unknown { i18n::tr("nicht geliefert").into() } else { fmt_date(t.tx.booking_date) }),
        value_date: ss(if t.tx.date_unknown { i18n::tr("nicht geliefert").into() } else { fmt_date(t.tx.value_date) }),
        purpose: ss(&t.tx.purpose),
        posting_text: ss(&t.tx.posting_text),
        eref: ss(&t.tx.end_to_end_id),
        mref: ss(&t.tx.mandate_id),
        creditor_id: ss(&t.tx.creditor_id),
        category: ss(category_label(&categorize(t, &v.rules))),
        account: ss(account_label(v, t.account_id)),
        pending: t.tx.pending,
    });
}

fn standing_row(v: &Vault, index: usize, today: NaiveDate) -> StandingRow {
    let s = &v.standing[index];
    let next = next_execution(&s.order, today);
    StandingRow {
        index: index as i32,
        name: ss(&s.order.transfer.creditor_name),
        iban: ss(short_iban(&s.order.transfer.creditor_iban)),
        amount: ss(fmt_eur(-s.order.transfer.amount)),
        purpose: ss(&s.order.transfer.purpose),
        schedule: ss(format!("{}{}", schedule_text(&s.order), s.order.execution_time.map(|t| i18n::tr_format!(" · {} Uhr", t.format("%H:%M"))).unwrap_or_default())),
        next: ss(next.map(|d| i18n::tr_format!("nächste Ausführung {}", fmt_date(d))).unwrap_or_else(|| i18n::tr("beendet").into())),
        account: ss(account_label(v, s.account_id)),
    }
}

pub fn schedule_text(o: &StandingOrder) -> String {
    let base = match (o.interval, o.every) {
        (Interval::Weekly, 1) => i18n::tr("wöchentlich").to_string(),
        (Interval::Weekly, n) => i18n::tr_format!("alle {n} Wochen"),
        (Interval::Monthly, 1) => i18n::tr("monatlich").to_string(),
        (Interval::Monthly, 3) => i18n::tr("vierteljährlich").to_string(),
        (Interval::Monthly, 6) => i18n::tr("halbjährlich").to_string(),
        (Interval::Monthly, 12) => i18n::tr("jährlich").to_string(),
        (Interval::Monthly, n) => i18n::tr_format!("alle {n} Monate"),
    };
    let day = match (o.interval, o.day) {
        (Interval::Monthly, 97) => i18n::tr(" zwei Tage vor Ultimo").to_string(),
        (Interval::Monthly, 98) => i18n::tr(" einen Tag vor Ultimo").to_string(),
        (Interval::Monthly, 99) => i18n::tr(" zum Ultimo").to_string(),
        (Interval::Monthly, d) => i18n::tr_format!(" zum {d}."),
        (Interval::Weekly, d) if (1..=7).contains(&d) => i18n::tr_format!(" am {}", i18n::tr(WEEKDAYS[d as usize - 1])),
        _ => String::new(),
    };
    format!("{base}{day}")
}

pub fn next_execution(o: &StandingOrder, today: NaiveDate) -> Option<NaiveDate> {
    let mut d = o.first_date;
    for _ in 0..1000 {
        let actual = match o.interval {
            Interval::Monthly => {
                let last = (NaiveDate::from_ymd_opt(d.year(), d.month(), 1)? + Months::new(1) - Duration::days(1)).day();
                let day = if (97..=99).contains(&o.day) { last - (99 - o.day) } else { o.day.clamp(1, last) };
                NaiveDate::from_ymd_opt(d.year(), d.month(), day)?
            }
            Interval::Weekly => d,
        };
        if let Some(last) = o.last_date {
            if actual > last {
                return None;
            }
        }
        if actual >= today && actual >= o.first_date {
            return Some(actual);
        }
        d = match o.interval {
            Interval::Monthly => d + Months::new(o.every.max(1)),
            Interval::Weekly => d + Duration::weeks(o.every.max(1) as i64),
        };
    }
    None
}


#[cfg(test)]
mod amount_tests {
    use super::*;
    use slint::Model;

    #[test]
    fn monthly_analysis_excludes_pending_unknown_transfers_and_outside_dates() {
        use crate::business_tests::{tx, date};
        let mut v = Vault::default();
        v.merge_transactions(1, vec![tx(10000, "2026-11-01"), tx(-2500, "2026-11-30"), tx(-90000, "2026-10-31"), tx(80000, "2026-12-01")]);
        let mut pending = tx(-70000, "2026-11-01"); pending.pending = true;
        let mut unknown = tx(-60000, "2026-11-01"); unknown.date_unknown = true;
        let mut transfer = tx(-50000, "2026-11-01"); transfer.purpose = "Umbuchung".into();
        v.merge_transactions(1, vec![pending, unknown, transfer]);
        let (income, expense, cats) = month_stats(&v, date("2026-11-01"));
        assert_eq!(income, Decimal::from(100)); assert_eq!(expense, Decimal::from(-25)); assert_eq!(cats.values().copied().sum::<Decimal>(), Decimal::from(25));
        let series = month_series(&v, date("2026-11-01"), 3); assert_eq!(series.len(), 3); assert_eq!(series[0].label, "Sep 26"); assert_eq!(series[2].income_text, "100,00 €");
    }

    #[test]
    fn transaction_filters_combine_account_date_search_and_category_and_sort_pending_first() {
        use crate::business_tests::{tx, date};
        let mut v = Vault::default(); let mut pending = tx(-400, "2026-11-01"); pending.pending = true;
        let mut unknown = tx(-500, "2025-01-01"); unknown.date_unknown = true;
        let mut grocery = tx(-600, "2026-11-03"); grocery.counterparty_name = "REWE".into(); grocery.purpose = "Wocheneinkauf".into();
        v.merge_transactions(1, vec![tx(-100, "2026-10-01"), tx(-200, "2026-11-01"), tx(-300, "2026-11-02"), pending, unknown, grocery]);
        v.merge_transactions(2, vec![tx(-700, "2026-11-04")]);
        assert_eq!(transaction_indices(&v, Some(1), Some(date("2026-11-01")), "", ""), vec![3, 4, 5, 2, 1]);
        assert_eq!(transaction_indices(&v, Some(1), None, "rEwE 6,00", "Lebensmittel"), vec![5]);
        assert!(transaction_indices(&v, Some(2), None, "REWE", "").is_empty());
        assert!(transaction_indices(&v, None, None, "REWE", "Wohnen").is_empty());
        assert_eq!(transaction_indices(&v, None, None, "REWE fehlend", ""), Vec::<usize>::new());
    }

    #[test]
    fn transaction_row_preserves_pending_currency_and_multiline_purpose() {
        let mut v = Vault::default(); let mut t = crate::business_tests::tx(-100, "2026-11-01");
        t.pending = true; t.currency = "USD".into(); t.counterparty_name.clear(); t.posting_text = "Kartenkauf".into(); t.purpose = "erste\nzweite".into();
        v.merge_transactions(1, vec![t]); let row = tx_row(&v, 0, false);
        assert_eq!(row.name, "Kartenkauf"); assert_eq!(row.purpose, "erste zweite"); assert_eq!(row.amount, "−1,00 USD"); assert!(row.pending && row.negative);
    }

    #[test]
    fn group_balances_sum_booked_values_and_keep_currencies_separate() {
        let mut vault = Vault::default();
        let mut rows = Vec::new();
        for (index, balance) in [Some(("100.10", "EUR")), Some(("-150.35", "EUR")), Some(("12.34", "USD")), None].into_iter().enumerate() {
            let id = vault.merge_accounts(1, vec![fints::Account { account_number: index.to_string(), ..Default::default() }])[0];
            vault.accounts.iter_mut().find(|a| a.id == id).unwrap().balance = balance.map(|(amount, currency)| fints::Balance {
                booked: amount.parse().unwrap(), available: Some(Decimal::from(999)), currency: currency.into(),
                date: NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
            });
            rows.push(AccountRow { id: id as i32, ..Default::default() });
        }
        let group = account_group(&vault, 9, "Test", rows);
        assert_eq!(group.balances.row_count(), 2);
        let eur = group.balances.row_data(0).unwrap();
        assert_eq!(eur.amount, "−50,25 €");
        assert!(eur.negative);
        assert_eq!(group.balances.row_data(1).unwrap().amount, "12,34 USD");
        assert_eq!(group.balance_note, "Teilsaldo · 3 von 4 Kontosalden verfügbar");
    }

    #[test]
    fn unavailable_group_balances_are_distinct_from_zero() {
        let mut vault = Vault::default();
        let id = vault.merge_accounts(1, vec![fints::Account::default()])[0];
        let rows = vec![AccountRow { id: id as i32, ..Default::default() }];
        let group = account_group(&vault, 9, "Test", rows.clone());
        assert_eq!(group.balances.row_count(), 0);
        assert_eq!(group.balance_note, "Noch keine Salden abgerufen");
        vault.accounts[0].balance = Some(fints::Balance {
            booked: Decimal::ZERO, available: None, currency: "EUR".into(),
            date: NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        });
        let group = account_group(&vault, 9, "Test", rows);
        assert_eq!(group.balances.row_data(0).unwrap().amount, "0,00 €");
        assert_eq!(group.balance_note, "1 Konto");
    }
    #[test]
    fn amount_entry_does_not_silently_round() {
        assert_eq!(parse_amount("1.234,56 €").unwrap().to_string(),"1234.56");
        assert_eq!(parse_amount("12.34").unwrap().to_string(),"12.34");
        assert!(parse_amount("12,345").is_none());
        assert!(parse_amount("0,001").is_none());
    }
}
