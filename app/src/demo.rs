//! Local demo data so the interface can be explored without a bank login.
//! All names and account numbers are fictitious (test BLZ 12345678).

use crate::store::{AccessKind, BankAccess, StoredStanding, Vault};
use chrono::{Datelike, Duration, Local, NaiveDate};
use fints::{Account, Balance, Interval, SepaTransfer, StandingOrder, Transaction};
use rust_decimal::Decimal;

pub const DEMO_BLZ: &str = "12345678";

pub fn iban(blz: &str, account: &str) -> String {
    let bban = format!("{blz}{:0>10}", account);
    let numeric: String = format!("{bban}131400"); // "DE" -> 13 14, "00"
    let mut rem: u64 = 0;
    for c in numeric.chars() {
        rem = (rem * 10 + c.to_digit(10).unwrap() as u64) % 97;
    }
    format!("DE{:02}{}", 98 - rem, bban)
}

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
}

fn tx(date: NaiveDate, amount: Decimal, name: &str, purpose: &str, posting: &str, cp_account: &str) -> Transaction {
    let mut t = Transaction {
        booking_date: date,
        value_date: date,
        amount,
        currency: "EUR".into(),
        counterparty_name: name.into(),
        counterparty_iban: iban("87654321", cp_account),
        counterparty_bic: "DEMODEXXXXX".into(),
        purpose: purpose.into(),
        posting_text: posting.into(),
        ..Default::default()
    };
    t.compute_fingerprint();
    t
}

pub fn add_demo(v: &mut Vault) {
    add_demo_data(v, true);
}

/// First-vault onboarding: one local account with sample transactions.
pub fn add_demo_account(v: &mut Vault) {
    add_demo_data(v, false);
}

fn add_demo_data(v: &mut Vault, full_demo: bool) {
    if v.accesses.iter().any(|a| a.kind == AccessKind::Demo) {
        return;
    }
    let access_id = v.new_id();
    v.accesses.push(BankAccess {
        id: access_id,
        name: "Demo-Bank (lokal)".into(),
        kind: AccessKind::Demo,
        config: fints::Config { blz: DEMO_BLZ.into(), user_id: "demo".into(), ..Default::default() },
        state: fints::State { tan_method: Some("Demo-chipTAN".into()), ..Default::default() },
        saved_pin: None,
    });
    let giro = Account {
        iban: iban(DEMO_BLZ, "4711000"),
        bic: "DEMODEXXXXX".into(),
        account_number: "4711000".into(),
        blz: DEMO_BLZ.into(),
        owner: "Max Mustermann".into(),
        product: "Girokonto Demo".into(),
        currency: "EUR".into(),
        kind: 1,
        ..Default::default()
    };
    let savings = Account {
        iban: iban(DEMO_BLZ, "4711099"),
        account_number: "4711099".into(),
        product: "Tagesgeld Demo".into(),
        kind: 10,
        ..giro.clone()
    };
    let ids = v.merge_accounts(access_id, if full_demo { vec![giro.clone(), savings] } else { vec![giro.clone()] });
    let (giro_id, sav_id) = (ids[0], ids.get(1).copied());

    let today = Local::now().date_naive();
    let mut rng = Lcg(42);
    let mut list = Vec::new();
    let mut sav = Vec::new();
    let start = today - Duration::days(200);
    let mut day = start;
    while day <= today {
        if day.day() == 1 {
            list.push(tx(day, d("-890.00"), "Wohnbau Ostsee eG", "Miete Wohnung 3 OG links", "DAUERAUFTRAG", "100200"));
            list.push(tx(day, d("-250.00"), "Max Mustermann", "Sparrate Tagesgeld", "UMBUCHUNG", "4711099"));
            sav.push(tx(day, d("250.00"), "Max Mustermann", "Sparrate Tagesgeld", "UMBUCHUNG", "4711000"));
        }
        if day.day() == 28 {
            list.push(tx(day, d("3420.55"), "Muster Werkstätten GmbH", "Lohn/Gehalt Abrechnung", "LOHN GEHALT", "900100"));
        }
        if day.day() == 3 {
            list.push(tx(day, d("-86.00"), "Stadtwerke Förde", "Abschlag Strom und Gas", "LASTSCHRIFT", "300400"));
            list.push(tx(day, d("-39.99"), "Mobilfunk Nord", "Rechnung Mobilfunkvertrag", "LASTSCHRIFT", "300500"));
        }
        if day.day() == 15 {
            list.push(tx(day, d("-17.99"), "Streamingdienst Nordwind", "Monatsabo Premium", "LASTSCHRIFT", "300600"));
            list.push(tx(day, d("-58.40"), "Kfz-Versicherung Küste", "Beitrag Kfz-Haftpflicht", "LASTSCHRIFT", "300700"));
        }
        let wd = day.weekday().num_days_from_monday();
        if wd == 1 || wd == 4 || wd == 5 {
            let cents = rng.range(1890, 9450);
            list.push(tx(day, -Decimal::new(cents, 2), "Supermarkt Nordlicht", "Kartenzahlung girocard", "KARTENZAHLUNG", "200100"));
        }
        if rng.range(0, 9) == 0 {
            let cents = rng.range(4000, 7800);
            list.push(tx(day, -Decimal::new(cents, 2), "Tankstelle am Ring", "Kartenzahlung girocard", "KARTENZAHLUNG", "200200"));
        }
        if rng.range(0, 14) == 0 {
            let cents = rng.range(1500, 12900);
            list.push(tx(day, -Decimal::new(cents, 2), "Versandhaus Möwe", "Bestellung 2026-", "LASTSCHRIFT", "200300"));
        }
        if rng.range(0, 25) == 0 {
            list.push(tx(day, d("-100.00"), "Geldautomat Hafen", "Bargeldauszahlung", "AUSZAHLUNG", "200400"));
        }
        if rng.range(0, 30) == 0 {
            let cents = rng.range(800, 4500);
            list.push(tx(day, -Decimal::new(cents, 2), "Apotheke am Markt", "Kartenzahlung girocard", "KARTENZAHLUNG", "200500"));
        }
        day += Duration::days(1);
    }
    let mut pending = tx(today, d("-42.80"), "Café Hafenblick", "Kartenzahlung girocard", "KARTENZAHLUNG", "200600");
    pending.pending = true;
    list.push(pending);

    let giro_balance: Decimal = d("2412.37");
    v.merge_transactions(giro_id, list);
    if let Some(sav_id) = sav_id { v.merge_transactions(sav_id, sav); }
    let now = Local::now().naive_local();
    for a in v.accounts.iter_mut() {
        if a.id == giro_id {
            a.balance = Some(Balance { booked: giro_balance, available: Some(giro_balance + d("2000")), currency: "EUR".into(), date: today });
            a.last_sync = Some(now);
        }
        if Some(a.id) == sav_id {
            a.balance = Some(Balance { booked: d("12750.00"), available: None, currency: "EUR".into(), date: today });
            a.last_sync = Some(now);
        }
    }
    if !full_demo { return; }
    let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    for (name, acc, amount, purpose) in [
        ("Wohnbau Ostsee eG", "100200", "890.00", "Miete Wohnung 3 OG links"),
        ("Max Mustermann", "4711099", "250.00", "Sparrate Tagesgeld"),
    ] {
        v.standing.push(StoredStanding {
            account_id: giro_id,
            order: StandingOrder {
                order_id: format!("DEMO-{acc}"),
                execution_time: None,
                transfer: SepaTransfer {
                    debtor_name: giro.owner.clone(),
                    debtor_iban: giro.iban.clone(),
                    debtor_bic: giro.bic.clone(),
                    creditor_name: name.into(),
                    creditor_iban: if acc == "4711099" { iban(DEMO_BLZ, acc) } else { iban("87654321", acc) },
                    creditor_bic: String::new(),
                    amount: d(amount),
                    purpose: purpose.into(),
                    end_to_end_id: String::new(),
                    execution_date: None,
                    instant: false,
                },
                first_date: first,
                last_date: None,
                interval: Interval::Monthly,
                every: 1,
                day: 1,
                raw_pain: Vec::new(),
                descriptor: String::new(),
            },
        });
    }
}

/// HHD 1.4 sample from the hbci4java test-suite, used to demonstrate the flicker dialog.
pub const DEMO_FLICKER: &str = "039870110490631098765432100812345678041,00";
