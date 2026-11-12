//! FinTS HIKKU v1 and proprietary DIKKU v2 credit-card statements.
//! Field positions follow AqBanking's jobgettransactions.xml / jobgetbalance.xml.
use crate::{Balance, Error, Result, Transaction, TransactionReport};
use crate::syntax::{parse_amount, parse_fints_date, Segment};
use chrono::NaiveDate;
use rust_decimal::Decimal;

fn date(value: &str) -> Result<Option<NaiveDate>> {
    if value.trim().is_empty() { return Ok(None); }
    parse_fints_date(value).or_else(|| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .map(Some).ok_or_else(|| Error::Syntax("Ungültiges Datum im Kreditkartenumsatz".into()))
}

fn amount(value: &str, sign: &str) -> Result<Decimal> {
    let value = parse_amount(value.trim()).ok_or_else(|| Error::Syntax("Ungültiger Kreditkartenbetrag".into()))?;
    // Some banks supply a signed amount in addition to the debit/credit indicator.
    match sign.trim() {
        "D" | "RC" => Ok(-value.abs()),
        "C" | "RD" => Ok(value.abs()),
        _ => Err(Error::Syntax("Ungültiges Soll-/Haben-Kennzeichen im Kreditkartenumsatz".into())),
    }
}

pub(crate) fn balance(s: &Segment) -> Result<Option<Balance>> {
    if s.de(3).iter().all(|v| v.as_ref().is_none_or(|v| v.is_empty())) { return Ok(None); }
    let currency = s.text_or_empty(3, 2);
    if currency.is_empty() { return Err(Error::Syntax("Währung des Kreditkartensaldos fehlt".into())); }
    Ok(Some(Balance {
        booked: amount(&s.text_or_empty(3, 1), &s.text_or_empty(3, 0))?,
        currency,
        date: date(&s.text_or_empty(3, 3))?.unwrap_or_else(|| chrono::Local::now().date_naive()),
        available: None,
    }))
}

pub(crate) fn statement(s: &Segment) -> Result<TransactionReport> {
    let standard = match (s.code.as_str(), s.version) {
        ("HIKKU", 1) => true,
        ("DIKKU", 2) => false,
        _ => return Err(Error::NotSupported("Version der Kreditkartenumsatzantwort".into())),
    };
    let mut report = TransactionReport { balance: balance(s)?, ..Default::default() };
    for i in 6..=s.de_count() {
        if s.de(i).iter().all(|v| v.as_ref().is_none_or(|v| v.is_empty())) { continue; }
        let t = |j| s.text_or_empty(i, j);
        // Both formats put the transaction/value date before the booking date.
        let value_date = date(&t(1))?;
        let booking_date = date(&t(2))?;
        let fallback = report.balance.as_ref().map(|b| b.date).unwrap_or_else(|| chrono::Local::now().date_naive());
        let amount_index = if standard { 9 } else { 8 };
        let purpose_start = amount_index + 3;
        let purpose_end = purpose_start + 9;
        let currency = t(amount_index + 1);
        if currency.is_empty() { return Err(Error::Syntax("Währung des Kreditkartenumsatzes fehlt".into())); }
        let status = t(if standard { 23 } else { 20 });
        let pending = match status.as_str() {
            "N" => true,
            "J" | "Y" | "" => booking_date.is_none(),
            _ => return Err(Error::Syntax("Unbekannter Buchungsstatus im Kreditkartenumsatz".into())),
        };
        let mut tx = Transaction {
            booking_date: booking_date.or(value_date).unwrap_or(fallback),
            value_date: value_date.or(booking_date).unwrap_or(fallback),
            date_unknown: booking_date.is_none() && value_date.is_none(),
            amount: amount(&t(amount_index), &t(amount_index + 2))?,
            currency,
            counterparty_name: t(purpose_start),
            purpose: (purpose_start..purpose_end).map(t).filter(|p| !p.trim().is_empty()).collect::<Vec<_>>().join(" · "),
            posting_text: "Kreditkartenumsatz".into(),
            end_to_end_id: t(21),
            pending,
            ..Default::default()
        };
        if standard && tx.end_to_end_id.is_empty() { tx.end_to_end_id = t(24); }
        tx.compute_fingerprint();
        report.transactions.push(tx);
    }
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::syntax::{SegmentBuilder, Val, parse_segments};

    pub(crate) fn fixture(standard: bool, sign: &str, value: &str, status: &str) -> SegmentBuilder {
        let mut entry = vec![Val::Empty; if standard { 29 } else { 23 }];
        for (i, text) in [(0, "test-card"), (1, "20261109"), (2, "20261110")] { entry[i] = text.into(); }
        let a = if standard { 9 } else { 8 };
        entry[a] = value.into(); entry[a+1] = "EUR".into(); entry[a+2] = sign.into();
        entry[a+3] = "Test Händler".into(); entry[a+4] = "Hamburg".into();
        entry[if standard { 23 } else { 20 }] = status.into();
        entry[21] = "reference-1".into();
        SegmentBuilder::new(if standard { "HIKKU" } else { "DIKKU" }, if standard { 1 } else { 2 })
            .de("test-card").de(Val::Empty).deg(vec!["D".into(), "-100,50".into(), "EUR".into(), "20261112".into()])
            .de(Val::Empty).de(Val::Empty).deg(entry)
    }
    #[test]
    fn both_card_formats_preserve_dates_signs_status_reference_and_balance() {
        for standard in [false, true] {
            for (sign, value, expected) in [("D", "-12,34", "-12.34"), ("C", "-12,34", "12.34"), ("RD", "12,34", "12.34")] {
                let s = parse_segments(&fixture(standard, sign, value, "J").encode()).unwrap();
                let r = statement(&s[0]).unwrap(); let t = &r.transactions[0];
                assert_eq!(t.amount.to_string(), expected);
                assert_eq!(t.booking_date.to_string(), "2026-11-10");
                assert_eq!(t.value_date.to_string(), "2026-11-09");
                assert_eq!(t.counterparty_name, "Test Händler");
                assert_eq!(t.purpose, "Test Händler · Hamburg");
                assert_eq!(t.end_to_end_id, "reference-1");
                assert!(!t.pending);
                assert_eq!(r.balance.unwrap().booked.to_string(), "-100.50");
                let pending = parse_segments(&fixture(standard, sign, value, "N").encode()).unwrap();
                assert!(statement(&pending[0]).unwrap().transactions[0].pending);
            }
        }
    }
    #[test]
    fn malformed_card_entries_fail_the_whole_report() {
        for (sign, value) in [("X", "12,34"), ("D", "bad"), ("D", "")] {
            let s = parse_segments(&fixture(true, sign, value, "J").encode()).unwrap();
            assert!(statement(&s[0]).is_err());
        }
        let mut s = parse_segments(&fixture(true, "D", "12,34", "J").encode()).unwrap();
        s[0].des[6][2] = Some(b"20260231".to_vec());
        assert!(statement(&s[0]).is_err());
        s[0].version = 99;
        assert!(statement(&s[0]).is_err());
    }
}
