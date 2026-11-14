//! Export of the currently visible transactions (CSV for spreadsheets, camt.052 for
//! accounting software).

use crate::categorize::categorize;
use crate::store::Vault;
use crate::view::account_label;
use chrono::Local;
use std::path::PathBuf;

fn target_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|d| d.to_path_buf()).or_else(|| Some(u.home_dir().to_path_buf())))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn csv_field(s: &str) -> String {
    // Bank-supplied names and purposes are untrusted spreadsheet input. Quote
    // fields and prefix dangerous text with a tab (OWASP Excel mitigation).
    let first = s.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}').chars().next();
    if matches!(first, Some('=' | '+' | '-' | '@' | '＝' | '＋' | '－' | '＠')) {
        return format!("\"\t{}\"", s.replace('"', "\"\""));
    }
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub(crate) fn csv_content(v: &Vault, idx: &[usize]) -> String {
    let headers = ["Konto", "Buchungstag", "Valuta", "Name", "IBAN", "BIC", "Verwendungszweck", "Buchungstext", "Kategorie", "Betrag", "Währung", "Status"];
    let mut out = format!("\u{feff}{}\r\n", headers.map(i18n::tr).join(";"));
    for &i in idx {
        let Some(t) = v.transactions.get(i) else { continue };
        let row = [
            account_label(v, t.account_id),
            if t.tx.date_unknown { String::new() } else { t.tx.booking_date.format("%d.%m.%Y").to_string() },
            if t.tx.date_unknown { String::new() } else { t.tx.value_date.format("%d.%m.%Y").to_string() },
            t.tx.counterparty_name.clone(),
            t.tx.counterparty_iban.clone(),
            t.tx.counterparty_bic.clone(),
            t.tx.purpose.clone(),
            t.tx.posting_text.clone(),
            crate::view::category_label(&categorize(t, &v.rules)).to_string(),
            format!("{:.2}", t.tx.amount).replace('.', ","),
            t.tx.currency.clone(),
            i18n::tr(if t.tx.pending { "vorgemerkt" } else { "gebucht" }).to_string(),
        ];
        // The amount is generated from Decimal, so negative numbers stay numeric.
        out.push_str(&row.iter().enumerate().map(|(column, f)| if column == 9 { f.clone() } else { csv_field(f) }).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    out
}

pub fn csv(v: &Vault, idx: &[usize]) -> Result<PathBuf, String> {
    let out = csv_content(v, idx);
    let path = target_dir().join(format!("bankingcraft-umsaetze-{}-{:08x}.csv", Local::now().format("%Y%m%d-%H%M%S"), rand::random::<u32>()));
    crate::private_file::write_new(&path, out.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

fn x(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// camt.052.001.08 account report, one report per account.
pub(crate) fn camt052_content(v: &Vault, idx: &[usize]) -> String {
    let mut by_account: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for &i in idx {
        if let Some(t) = v.transactions.get(i) {
            by_account.entry(t.account_id).or_default().push(i);
        }
    }
    let now = Local::now();
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.052.001.08\">\n<BkToCstmrAcctRpt>\n");
    xml.push_str(&format!("<GrpHdr><MsgId>BANKINGCRAFT-{}</MsgId><CreDtTm>{}</CreDtTm></GrpHdr>\n", now.format("%Y%m%d%H%M%S"), now.format("%Y-%m-%dT%H:%M:%S")));
    for (acc_id, list) in by_account {
        let Some(acc) = v.account(acc_id) else { continue };
        let account_identifier = if acc.account.iban.is_empty() {
            format!("<Othr><Id>{}</Id></Othr>", x(&acc.account.account_number))
        } else { format!("<IBAN>{}</IBAN>", x(&acc.account.iban)) };
        xml.push_str(&format!(
            "<Rpt><Id>{}-{}</Id><CreDtTm>{}</CreDtTm><Acct><Id>{}</Id><Ccy>{}</Ccy></Acct>\n",
            acc_id,
            now.format("%Y%m%d"),
            now.format("%Y-%m-%dT%H:%M:%S"),
            account_identifier,
            x(&acc.account.currency)
        ));
        if let Some(b) = &acc.balance {
            let (balance_code, amount) = if crate::paypal::is_paypal(v, acc_id) { ("CLAV", b.available) } else { ("CLBD", Some(b.booked)) };
            if let Some(amount) = amount {
                xml.push_str(&format!(
                "<Bal><Tp><CdOrPrtry><Cd>{balance_code}</Cd></CdOrPrtry></Tp><Amt Ccy=\"{}\">{:.2}</Amt><CdtDbtInd>{}</CdtDbtInd><Dt><Dt>{}</Dt></Dt></Bal>\n",
                x(&b.currency),
                amount.abs(),
                if amount.is_sign_negative() { "DBIT" } else { "CRDT" },
                b.date.format("%Y-%m-%d")
                ));
            }
        }
        for i in list {
            let t = &v.transactions[i].tx;
            let credit = !t.amount.is_sign_negative();
            let (party, acct) = if credit { ("Dbtr", "DbtrAcct") } else { ("Cdtr", "CdtrAcct") };
            let party_account = if t.counterparty_iban.is_empty() { String::new() }
                else { format!("<{acct}><Id><IBAN>{}</IBAN></Id></{acct}>", x(&t.counterparty_iban)) };
            xml.push_str(&format!(
                "<Ntry><Amt Ccy=\"{ccy}\">{amt:.2}</Amt><CdtDbtInd>{ind}</CdtDbtInd><Sts><Cd>{sts}</Cd></Sts>{dates}<BkTxCd/>\
<NtryDtls><TxDtls><Refs><EndToEndId>{e2e}</EndToEndId>{mref}</Refs><RltdPties><{party}><Pty><Nm>{name}</Nm></Pty></{party}>{party_account}</RltdPties><RmtInf><Ustrd>{purpose}</Ustrd></RmtInf><AddtlTxInf>{posting}</AddtlTxInf></TxDtls></NtryDtls></Ntry>\n",
                ccy = x(&t.currency),
                amt = t.amount.abs(),
                ind = if credit { "CRDT" } else { "DBIT" },
                sts = if t.pending { "PDNG" } else { "BOOK" },
                dates = if t.date_unknown { String::new() } else { format!("<BookgDt><Dt>{}</Dt></BookgDt><ValDt><Dt>{}</Dt></ValDt>", t.booking_date.format("%Y-%m-%d"), t.value_date.format("%Y-%m-%d")) },
                e2e = if t.end_to_end_id.is_empty() { "NOTPROVIDED".into() } else { x(&t.end_to_end_id) },
                mref = if t.mandate_id.is_empty() { String::new() } else { format!("<MndtId>{}</MndtId>", x(&t.mandate_id)) },
                name = x(&t.counterparty_name),
                purpose = x(&t.purpose),
                posting = x(&t.posting_text),
            ));
        }
        xml.push_str("</Rpt>\n");
    }
    xml.push_str("</BkToCstmrAcctRpt>\n</Document>\n");
    xml
}

pub fn camt052(v: &Vault, idx: &[usize]) -> Result<PathBuf, String> {
    let xml = camt052_content(v, idx);
    let now = Local::now();
    let path = target_dir().join(format!("bankingcraft-camt052-{}-{:08x}.xml", now.format("%Y%m%d-%H%M%S"), rand::random::<u32>()));
    crate::private_file::write_new(&path, xml.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}
