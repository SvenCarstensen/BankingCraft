//! MT940 (booked) / MT942 (pending) parser with German structured :86: fields
//! and SEPA purpose keys (EREF+, KREF+, MREF+, CRED+, SVWZ+, ABWA+ ...).

use crate::model::Transaction;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;

#[derive(Debug, Default)]
pub struct Statement {
    pub opening: Option<(Decimal, NaiveDate)>,
    pub closing: Option<(Decimal, NaiveDate)>,
    pub currency: String,
    pub transactions: Vec<Transaction>,
}

pub fn parse(data: &[u8], pending: bool) -> Vec<Statement> {
    let text: String = data.iter().map(|&b| b as char).collect();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut statements = Vec::new();
    let mut cur = Statement::default();
    let mut started = false;

    for (tag, value) in split_tags(&text) {
        match tag.as_str() {
            "20" => {
                if started && (!cur.transactions.is_empty() || cur.opening.is_some()) {
                    statements.push(std::mem::take(&mut cur));
                }
                started = true;
            }
            "60F" | "60M" => {
                if let Some((amt, date, ccy)) = parse_balance(&value) {
                    cur.opening = Some((amt, date));
                    cur.currency = ccy;
                }
            }
            "62F" | "62M" => {
                if let Some((amt, date, ccy)) = parse_balance(&value) {
                    cur.closing = Some((amt, date));
                    if cur.currency.is_empty() {
                        cur.currency = ccy;
                    }
                }
            }
            "61" => {
                if let Some(mut t) = parse_61(&value) {
                    t.pending = pending;
                    t.currency = if cur.currency.is_empty() { "EUR".into() } else { cur.currency.clone() };
                    cur.transactions.push(t);
                }
            }
            "86" => {
                if let Some(t) = cur.transactions.last_mut() {
                    apply_86(t, &value);
                    t.compute_fingerprint();
                }
            }
            _ => {}
        }
    }
    if started || !cur.transactions.is_empty() {
        statements.push(cur);
    }
    for s in &mut statements {
        for t in &mut s.transactions {
            if t.fingerprint.is_empty() {
                t.compute_fingerprint();
            }
        }
    }
    statements
}

/// Split into (tag, value). Tags start at line beginnings with ":NN[A]:".
fn split_tags(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        if line == "-" || line.starts_with("-}") {
            continue;
        }
        if let Some(rest) = line.strip_prefix(':') {
            if let Some(end) = rest.find(':') {
                let tag = &rest[..end];
                if !tag.is_empty() && tag.len() <= 3 && tag.chars().next().unwrap().is_ascii_digit() {
                    out.push((tag.to_string(), rest[end + 1..].to_string()));
                    continue;
                }
            }
        }
        if let Some(last) = out.last_mut() {
            last.1.push('\n');
            last.1.push_str(line);
        }
    }
    out
}

fn parse_amount(s: &str) -> Option<Decimal> {
    s.trim().replace(',', ".").parse().ok()
}

fn parse_yymmdd(s: &str) -> Option<NaiveDate> {
    if s.len() < 6 {
        return None;
    }
    let yy: i32 = s[0..2].parse().ok()?;
    let year = if yy > 79 { 1900 + yy } else { 2000 + yy };
    NaiveDate::from_ymd_opt(year, s[2..4].parse().ok()?, s[4..6].parse().ok()?)
}

fn parse_balance(v: &str) -> Option<(Decimal, NaiveDate, String)> {
    // C230115EUR1234,56
    let v = v.trim();
    let sign = if v.starts_with('D') { -1 } else { 1 };
    let date = parse_yymmdd(&v[1..])?;
    let ccy = v.get(7..10)?.to_string();
    let amt = parse_amount(v.get(10..)?)? * Decimal::from(sign);
    Some((amt, date, ccy))
}

fn parse_61(v: &str) -> Option<Transaction> {
    let first = v.lines().next()?;
    let value_date = parse_yymmdd(first)?;
    let mut rest = &first[6..];
    // optional entry date MMDD
    let mut booking_date = value_date;
    if rest.len() >= 4 && rest[..4].chars().all(|c| c.is_ascii_digit()) {
        let mm: u32 = rest[0..2].parse().ok()?;
        let dd: u32 = rest[2..4].parse().ok()?;
        let mut year = value_date.year();
        // booking around new year
        if mm == 12 && value_date.month() == 1 {
            year -= 1;
        } else if mm == 1 && value_date.month() == 12 {
            year += 1;
        }
        booking_date = NaiveDate::from_ymd_opt(year, mm, dd).unwrap_or(value_date);
        rest = &rest[4..];
    }
    let (sign, skip) = if rest.starts_with("RC") {
        (-1, 2)
    } else if rest.starts_with("RD") {
        (1, 2)
    } else if rest.starts_with('C') {
        (1, 1)
    } else if rest.starts_with('D') {
        (-1, 1)
    } else {
        return None;
    };
    rest = &rest[skip..];
    // optional funds code (third currency letter)
    if rest.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
        rest = &rest[1..];
    }
    let amount_end = rest.find(|c: char| !(c.is_ascii_digit() || c == ',')).unwrap_or(rest.len());
    let amount = parse_amount(&rest[..amount_end])? * Decimal::from(sign);
    Some(Transaction { booking_date, value_date, amount, ..Default::default() })
}

fn apply_86(t: &mut Transaction, v: &str) {
    let v = v.replace('\n', "");
    if v.len() < 4 {
        t.purpose = v;
        return;
    }
    let sep = v.chars().nth(3).unwrap_or('?');
    if !v[..3].chars().all(|c| c.is_ascii_digit()) || sep.is_alphanumeric() {
        t.purpose = v;
        return;
    }
    let mut purpose_parts: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for part in v[4..].split(sep) {
        if part.len() < 2 {
            continue;
        }
        let (code, val) = part.split_at(2);
        match code {
            "00" => t.posting_text = val.trim().to_string(),
            "20" | "21" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" | "60" | "61" | "62" | "63" => {
                purpose_parts.push(val.to_string())
            }
            "30" => t.counterparty_bic = val.trim().to_string(),
            "31" => t.counterparty_iban = val.trim().to_string(),
            "32" | "33" => names.push(val.to_string()),
            _ => {}
        }
    }
    t.counterparty_name = names.concat().trim().to_string();
    let raw = purpose_parts.concat();
    apply_sepa_keys(t, &raw);
}

/// Extract SEPA keys from the concatenated purpose text.
fn apply_sepa_keys(t: &mut Transaction, raw: &str) {
    const KEYS: [&str; 10] = ["EREF+", "KREF+", "MREF+", "CRED+", "DEBT+", "SVWZ+", "ABWA+", "ABWE+", "COAM+", "OAMT+"];
    let mut positions: Vec<(usize, &str)> = KEYS
        .iter()
        .filter_map(|k| raw.find(k).map(|p| (p, *k)))
        .collect();
    if positions.is_empty() {
        t.purpose = raw.trim().to_string();
        return;
    }
    positions.sort();
    let prefix = raw[..positions[0].0].trim();
    let mut svwz = String::new();
    for (i, (p, k)) in positions.iter().enumerate() {
        let start = p + k.len();
        let end = positions.get(i + 1).map(|x| x.0).unwrap_or(raw.len());
        let val = raw[start..end].trim().to_string();
        match *k {
            "EREF+" => t.end_to_end_id = val,
            "MREF+" => t.mandate_id = val,
            "CRED+" => t.creditor_id = val,
            "SVWZ+" => svwz = val,
            _ => {}
        }
    }
    t.purpose = if !svwz.is_empty() { svwz } else { prefix.to_string() };
}
