//! Local payment-form and endpoint validation. AqBanking builds bank messages.
use crate::{SepaTransfer, Error, Result};
use chrono::Local;

/// Restrict to the SEPA (DK) character set and transliterate umlauts.
pub fn sepa_charset(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'Ä' => out.push_str("Ae"),
            'Ö' => out.push_str("Oe"),
            'Ü' => out.push_str("Ue"),
            'ß' => out.push_str("ss"),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | '-' | '?' | ':' | '(' | ')' | '.' | ',' | '\'' | '+' | ' ' | '&' | '*' | '$' | '%' => out.push(c),
            _ => out.push(' '),
        }
    }
    out
}

pub fn msg_id() -> String {
    use rand::Rng;
    let r: u64 = rand::thread_rng().gen();
    // PmtInfId adds "-1"; both identifiers must remain within 35 characters.
    format!("BC-{}-{:08X}", Local::now().format("%Y%m%d%H%M%S"), r as u32)
}

/// Reject invalid payment data before opening a bank dialog or rounding amounts.
pub fn validate_transfer(t: &SepaTransfer) -> crate::Result<()> {
    if !iban_valid(&t.debtor_iban) || !iban_valid(&t.creditor_iban) {
        return Err(crate::Error::Other(i18n::tr("Ungültige Auftraggeber- oder Empfänger-IBAN").into()));
    }
    if t.amount <= rust_decimal::Decimal::ZERO || t.amount.normalize().scale() > 2 {
        return Err(crate::Error::Other(i18n::tr("Betrag muss positiv sein und darf höchstens zwei Nachkommastellen haben").into()));
    }
    for name in [&t.debtor_name, &t.creditor_name] {
        let converted = sepa_charset(name);
        if converted.trim().is_empty() || converted.chars().count() > 70 {
            return Err(crate::Error::Other(i18n::tr("Auftraggeber und Empfänger benötigen einen Namen mit höchstens 70 SEPA-Zeichen").into()));
        }
    }
    if sepa_charset(&t.purpose).chars().count() > 140 || sepa_charset(&t.end_to_end_id).chars().count() > 35 {
        return Err(crate::Error::Other(i18n::tr("Verwendungszweck oder Ende-zu-Ende-Referenz ist zu lang").into()));
    }
    Ok(())
}

pub fn validate_standing_schedule(so: &crate::model::StandingOrder) -> crate::Result<()> {
    let day_valid = match so.interval {
        crate::model::Interval::Weekly => (1..=7).contains(&so.day),
        crate::model::Interval::Monthly => (1..=31).contains(&so.day) || (97..=99).contains(&so.day),
    };
    if !day_valid || so.every == 0 || so.first_date < Local::now().date_naive()
        || so.last_date.map(|last| last < so.first_date).unwrap_or(false)
    {
        return Err(crate::Error::Other(i18n::tr("Ungültiger Ausführungsrhythmus oder Zeitraum des Dauerauftrags").into()));
    }
    Ok(())
}

pub fn iban_valid(iban: &str) -> bool {
    let s: String = iban.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if s.len() < 15 || s.len() > 34 || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !s.as_bytes()[..2].iter().all(u8::is_ascii_alphabetic)
        || !s.as_bytes()[2..4].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let v = if c.is_ascii_digit() { c as u32 - '0' as u32 } else { c as u32 - 'A' as u32 + 10 };
        rem = if v >= 10 { (rem * 100 + v) % 97 } else { (rem * 10 + v) % 97 };
    }
    rem == 1
}
pub fn validate_https_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|_| Error::Security(i18n::tr("Ungültige FinTS-Adresse").into()))?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some() || value.contains('\\') || value.chars().any(char::is_control) {
        return Err(Error::Security(i18n::tr("FinTS benötigt eine eindeutige HTTPS-Adresse ohne Benutzerinfo oder Fragment").into()));
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_local_inputs_before_native_dispatch() {
        assert!(iban_valid("DE89370400440532013000"));
        for bad in ["DE00370400440532013000","12DE370400440532013000","DE89<70400440532013000",""] {assert!(!iban_valid(bad));}
        assert!(validate_https_url("https://bank.invalid/fints").is_ok());
        for bad in ["http://bank.invalid","https://user@bank.invalid","https://bank.invalid/#fragment","https://bank.invalid\\@evil.invalid","https://bank.invalid/\n"] {
            assert!(validate_https_url(bad).is_err());
        }
        let mut transfer=SepaTransfer{debtor_name:"Alice".into(),debtor_iban:"DE89370400440532013000".into(),debtor_bic:"COBADEFFXXX".into(),
            creditor_name:"Bob".into(),creditor_iban:"DE02120300000000202051".into(),creditor_bic:"BYLADEM1001".into(),
            amount:rust_decimal::Decimal::new(123,2),purpose:"synthetic".into(),end_to_end_id:msg_id(),execution_date:None,instant:false};
        assert!(validate_transfer(&transfer).is_ok());assert!(transfer.end_to_end_id.len()<=35);
        for invalid in [rust_decimal::Decimal::ZERO,rust_decimal::Decimal::new(-1,2),rust_decimal::Decimal::new(1234,3)] {
            transfer.amount=invalid;assert!(validate_transfer(&transfer).is_err());
        }
    }
}
