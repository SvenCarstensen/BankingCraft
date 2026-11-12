//! Turns the vault into Slint models.

use crate::categorize::{categorize, color_of, CATEGORIES};
use crate::store::{AccessKind, StoredTx, Vault};
use crate::{AccessRow, AccountRow, AppState, AppWindow, BarStat, MonthStat, StandingRow, TxDetail, TxRow};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use fints::{Interval, StandingOrder};
use rust_decimal::Decimal;
use slint::{ComponentHandle, Color, ModelRc, SharedString, VecModel};
use std::collections::BTreeMap;

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
    s.parse::<Decimal>().ok().map(|d| d.round_dp(2))
}

const MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
const WEEKDAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];

fn day_label(d: NaiveDate) -> String {
    let today = Local::now().date_naive();
    let prefix = if d == today {
        "Heute, ".to_string()
    } else if d == today - Duration::days(1) {
        "Gestern, ".to_string()
    } else {
        format!("{}, ", WEEKDAYS[d.weekday().num_days_from_monday() as usize])
    };
    format!("{prefix}{}. {} {}", d.day(), MONTHS[d.month0() as usize], d.year())
}

const ACCOUNT_COLORS: [(u8, u8, u8); 6] = [(0x1e, 0x6f, 0xd9), (0x0c, 0x85, 0x99), (0xe8, 0x59, 0x0c), (0x5f, 0x3d, 0xc4), (0x2f, 0x9e, 0x44), (0xc2, 0x25, 0x5c)];

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

fn ss(s: impl AsRef<str>) -> SharedString {
    SharedString::from(s.as_ref())
}

pub fn account_label(v: &Vault, id: u32) -> String {
    v.account(id)
        .map(|a| {
            let name = if a.account.product.is_empty() { a.account.account_number.clone() } else { a.account.product.clone() };
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
    pub account_filter: Vec<u32>,
    pub sepa_accounts: Vec<u32>,
    pub standing_index: Vec<usize>,
}

pub fn refresh_all(app: &AppWindow, v: &Vault, vs: &mut ViewState) {
    let st = app.global::<AppState>();
    let today = Local::now().date_naive();

    // accounts
    let mut rows = Vec::new();
    let mut total = Decimal::ZERO;
    let mut names = vec![ss("Alle Konten")];
    vs.account_filter.clear();
    vs.sepa_accounts.clear();
    let mut sepa_names = Vec::new();
    for (i, a) in v.accounts.iter().enumerate() {
        let c = ACCOUNT_COLORS[i % ACCOUNT_COLORS.len()];
        let bank = v.access(a.access_id).map(|x| x.name.clone()).unwrap_or_default();
        let (balance, negative, available) = match &a.balance {
            Some(b) => {
                total += b.booked;
                (fmt_money(b.booked, &b.currency), b.booked.is_sign_negative(), b.available.map(|x| fmt_money(x, &b.currency)).unwrap_or_default())
            }
            None => ("–".into(), false, String::new()),
        };
        let kind = match a.account.kind {
            1..=9 => "Girokonto",
            10..=19 => "Sparkonto",
            20..=29 => "Festgeld",
            30..=39 => "Depot",
            40..=49 => "Darlehen",
            50..=59 => "Kreditkarte",
            _ => "Konto",
        };
        let name = if a.account.product.is_empty() { kind.to_string() } else { a.account.product.clone() };
        rows.push(AccountRow {
            id: a.id as i32,
            name: ss(&name),
            bank: ss(&bank),
            iban: ss(if a.account.iban.is_empty() { a.account.account_number.clone() } else { short_iban(&a.account.iban) }),
            owner: ss(&a.account.owner),
            balance: ss(balance),
            available: ss(available),
            negative,
            last_sync: ss(a.last_sync.map(|t| format!("aktualisiert {}", t.format("%d.%m.%Y %H:%M"))).unwrap_or_else(|| "noch nicht abgerufen".into())),
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
    st.set_accounts(model(rows));
    st.set_account_names(model(names));
    st.set_sepa_accounts(model(sepa_names));
    if st.get_tf_account() as usize >= vs.sepa_accounts.len() {
        st.set_tf_account(0);
    }
    st.set_total_balance(ss(fmt_eur(total)));
    st.set_total_negative(total.is_sign_negative());
    if let Some(t) = v.accounts.iter().filter_map(|a| a.last_sync).max() {
        st.set_last_sync(ss(format!("Letzter Kontenrundruf: {}", t.format("%d.%m.%Y um %H:%M Uhr"))));
    } else {
        st.set_last_sync(ss("Willkommen – richten Sie einen Bankzugang ein oder laden Sie die Demo-Daten."));
    }

    // categories
    st.set_categories(model(CATEGORIES.iter().map(|c| ss(c.name)).collect()));
    st.set_category_colors(model(CATEGORIES.iter().map(|c| color_of(c.name)).collect()));

    // overview month figures
    let month_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let (inc, exp, cats) = month_stats(v, month_start);
    st.set_month_label(ss(format!("{} {}", MONTHS[today.month0() as usize], today.year())));
    st.set_month_income(ss(fmt_eur(inc)));
    st.set_month_expense(ss(fmt_eur(-exp)));
    st.set_category_stats(model(bar_stats(&cats, 6)));
    st.set_months(model(month_series(v, month_start, 6)));

    // recent
    let mut idx: Vec<usize> = (0..v.transactions.len()).collect();
    idx.sort_by(|&a, &b| v.transactions[b].tx.booking_date.cmp(&v.transactions[a].tx.booking_date));
    let recent: Vec<TxRow> = idx.iter().take(6).map(|&i| tx_row(v, i, true)).collect();
    st.set_recent(model(recent));

    // standing orders
    vs.standing_index.clear();
    let mut standing = Vec::new();
    let mut upcoming: Vec<(NaiveDate, StandingRow)> = Vec::new();
    for (i, s) in v.standing.iter().enumerate() {
        let next = next_execution(&s.order, today);
        let row = StandingRow {
            index: i as i32,
            name: ss(&s.order.transfer.creditor_name),
            iban: ss(short_iban(&s.order.transfer.creditor_iban)),
            amount: ss(fmt_eur(-s.order.transfer.amount)),
            purpose: ss(&s.order.transfer.purpose),
            schedule: ss(schedule_text(&s.order)),
            next: ss(next.map(|d| format!("nächste Ausführung {}", fmt_date(d))).unwrap_or_else(|| "beendet".into())),
            account: ss(account_label(v, s.account_id)),
        };
        if let Some(n) = next {
            upcoming.push((n, row.clone()));
        }
        standing.push(row);
        vs.standing_index.push(i);
    }
    upcoming.sort_by_key(|x| x.0);
    st.set_standing(model(standing));
    st.set_upcoming(model(upcoming.into_iter().take(5).map(|x| x.1).collect()));

    // accesses
    let accesses = v
        .accesses
        .iter()
        .map(|a| {
            let (kind, detail) = match &a.kind {
                AccessKind::PinTan => ("PIN/TAN", format!("BLZ {} · {}", a.config.blz, a.config.user_id)),
                AccessKind::ChipCard { reader, record } => ("HBCI-Chipkarte", format!("BLZ {} · Eintrag {record} · {reader}", a.config.blz)),
                AccessKind::Demo => ("Demo", "lokale Beispieldaten".to_string()),
            };
            let method = match &a.kind {
                AccessKind::ChipCard { .. } => "Signatur mit Chipkarte".to_string(),
                AccessKind::Demo => "chipTAN (Demo)".to_string(),
                AccessKind::PinTan => a
                    .state
                    .tan_method
                    .as_ref()
                    .map(|sf| a.state.bpd.tan_methods.iter().find(|m| &m.security_function == sf).map(|m| m.name.clone()).unwrap_or(sf.clone()))
                    .unwrap_or_else(|| "noch nicht gewählt".into()),
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
            }
        })
        .collect();
    st.set_accesses(model(accesses));

    refresh_transactions(app, v, vs);
    refresh_analysis(app, v);
}

fn month_stats(v: &Vault, month_start: NaiveDate) -> (Decimal, Decimal, BTreeMap<String, Decimal>) {
    let month_end = month_start + Months::new(1);
    let mut inc = Decimal::ZERO;
    let mut exp = Decimal::ZERO;
    let mut cats: BTreeMap<String, Decimal> = BTreeMap::new();
    for t in &v.transactions {
        if t.tx.booking_date < month_start || t.tx.booking_date >= month_end {
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
            label: ss(name),
            value: ss(fmt_eur(*amount)),
            ratio: if max.is_zero() { 0.0 } else { (*amount / max).to_string().parse::<f32>().unwrap_or(0.0) },
            color: color_of(name),
        })
        .collect()
}

fn month_series(v: &Vault, last_month_start: NaiveDate, count: u32) -> Vec<MonthStat> {
    let mut data = Vec::new();
    for k in (0..count).rev() {
        let start = last_month_start - Months::new(k);
        let (inc, exp, _) = month_stats(v, start);
        data.push((start, inc, -exp));
    }
    let max = data.iter().map(|x| x.1.max(x.2)).max().unwrap_or(Decimal::ONE).max(Decimal::ONE);
    data.into_iter()
        .map(|(s, inc, exp)| MonthStat {
            label: ss(format!("{} {:02}", MONTHS_SHORT[s.month0() as usize], s.year() % 100)),
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
    st.set_analysis_label(ss(format!("{} {}", MONTHS[start.month0() as usize], start.year())));
    let (_, _, cats) = month_stats(v, start);
    st.set_analysis_categories(model(bar_stats(&cats, 15)));
    // income by category
    let end = start + Months::new(1);
    let mut inc: BTreeMap<String, Decimal> = BTreeMap::new();
    for t in &v.transactions {
        if t.tx.booking_date >= start && t.tx.booking_date < end && !t.tx.amount.is_sign_negative() {
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
        name: ss(if name.is_empty() { "Umsatz".into() } else { name }),
        purpose: ss(purpose.replace('\n', " ")),
        amount: ss(fmt_money(t.tx.amount, &t.tx.currency)),
        negative: t.tx.amount.is_sign_negative(),
        pending: t.tx.pending,
        cat_color: color_of(&cat),
        category: ss(cat),
        account: ss(if with_account { account_label(v, t.account_id) } else { String::new() }),
    }
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

    let mut idx: Vec<usize> = (0..v.transactions.len())
        .filter(|&i| {
            let t: &StoredTx = &v.transactions[i];
            if let Some(a) = account {
                if t.account_id != a {
                    return false;
                }
            }
            if let Some(s) = since {
                if t.tx.booking_date < s {
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
        tb.pending.cmp(&ta.pending).then(tb.booking_date.cmp(&ta.booking_date)).then(b.cmp(&a))
    });

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
        let mut j = i;
        let mut day_sum = Decimal::ZERO;
        while j < idx.len() && v.transactions[idx[j]].tx.pending == pending && (pending || v.transactions[idx[j]].tx.booking_date == date) {
            day_sum += v.transactions[idx[j]].tx.amount;
            j += 1;
        }
        let label = if pending { "Vorgemerkte Umsätze".to_string() } else { day_label(date) };
        let balance_text = if pending {
            String::new()
        } else if let Some(r) = running {
            let s = format!("Tagessaldo {}", fmt_eur(r));
            running = Some(r - day_sum);
            s
        } else {
            format!("Tagessumme {}", fmt_eur(day_sum))
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
    st.set_tx_summary(ss(format!(
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
        booking_date: ss(fmt_date(t.tx.booking_date)),
        value_date: ss(fmt_date(t.tx.value_date)),
        purpose: ss(&t.tx.purpose),
        posting_text: ss(&t.tx.posting_text),
        eref: ss(&t.tx.end_to_end_id),
        mref: ss(&t.tx.mandate_id),
        creditor_id: ss(&t.tx.creditor_id),
        category: ss(categorize(t, &v.rules)),
        account: ss(account_label(v, t.account_id)),
        pending: t.tx.pending,
    });
}

pub fn schedule_text(o: &StandingOrder) -> String {
    let base = match (o.interval, o.every) {
        (Interval::Weekly, 1) => "wöchentlich".to_string(),
        (Interval::Weekly, n) => format!("alle {n} Wochen"),
        (Interval::Monthly, 1) => "monatlich".to_string(),
        (Interval::Monthly, 3) => "vierteljährlich".to_string(),
        (Interval::Monthly, 6) => "halbjährlich".to_string(),
        (Interval::Monthly, 12) => "jährlich".to_string(),
        (Interval::Monthly, n) => format!("alle {n} Monate"),
    };
    let day = match (o.interval, o.day) {
        (Interval::Monthly, 99) => " zum Ultimo".to_string(),
        (Interval::Monthly, d) => format!(" zum {d}."),
        (Interval::Weekly, d) if (1..=7).contains(&d) => format!(" am {}", WEEKDAYS[d as usize - 1]),
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
                let day = if o.day >= 97 { last } else { o.day.clamp(1, last) };
                NaiveDate::from_ymd_opt(d.year(), d.month(), day)?
            }
            Interval::Weekly => d,
        };
        if let Some(last) = o.last_date {
            if actual > last {
                return None;
            }
        }
        if actual >= today {
            return Some(actual);
        }
        d = match o.interval {
            Interval::Monthly => d + Months::new(o.every.max(1)),
            Interval::Weekly => d + Duration::weeks(o.every.max(1) as i64),
        };
    }
    None
}
