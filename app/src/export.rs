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
    if s.contains([';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn csv(v: &Vault, idx: &[usize]) -> Result<PathBuf, String> {
    let mut out = String::from("\u{feff}Konto;Buchungstag;Valuta;Name;IBAN;BIC;Verwendungszweck;Buchungstext;Kategorie;Betrag;Währung;Status\r\n");
    for &i in idx {
        let Some(t) = v.transactions.get(i) else { continue };
        let row = [
            account_label(v, t.account_id),
            t.tx.booking_date.format("%d.%m.%Y").to_string(),
            t.tx.value_date.format("%d.%m.%Y").to_string(),
            t.tx.counterparty_name.clone(),
            t.tx.counterparty_iban.clone(),
            t.tx.counterparty_bic.clone(),
            t.tx.purpose.clone(),
            t.tx.posting_text.clone(),
            categorize(t, &v.rules),
            format!("{:.2}", t.tx.amount).replace('.', ","),
            t.tx.currency.clone(),
            if t.tx.pending { "vorgemerkt" } else { "gebucht" }.to_string(),
        ];
        out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(";"));
        out.push_str("\r\n");
    }
    let path = target_dir().join(format!("kontor-umsaetze-{}.csv", Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, out).map_err(|e| e.to_string())?;
    Ok(path)
}

fn x(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// camt.052.001.08 account report, one report per account.
pub fn camt052(v: &Vault, idx: &[usize]) -> Result<PathBuf, String> {
    let mut by_account: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for &i in idx {
        if let Some(t) = v.transactions.get(i) {
            by_account.entry(t.account_id).or_default().push(i);
        }
    }
    let now = Local::now();
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.052.001.08\">\n<BkToCstmrAcctRpt>\n");
    xml.push_str(&format!("<GrpHdr><MsgId>KONTOR-{}</MsgId><CreDtTm>{}</CreDtTm></GrpHdr>\n", now.format("%Y%m%d%H%M%S"), now.format("%Y-%m-%dT%H:%M:%S")));
    for (acc_id, list) in by_account {
        let Some(acc) = v.account(acc_id) else { continue };
        xml.push_str(&format!(
            "<Rpt><Id>{}-{}</Id><CreDtTm>{}</CreDtTm><Acct><Id><IBAN>{}</IBAN></Id><Ccy>{}</Ccy></Acct>\n",
            acc_id,
            now.format("%Y%m%d"),
            now.format("%Y-%m-%dT%H:%M:%S"),
            x(&acc.account.iban),
            x(&acc.account.currency)
        ));
        if let Some(b) = &acc.balance {
            xml.push_str(&format!(
                "<Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy=\"{}\">{:.2}</Amt><CdtDbtInd>{}</CdtDbtInd><Dt><Dt>{}</Dt></Dt></Bal>\n",
                x(&b.currency),
                b.booked.abs(),
                if b.booked.is_sign_negative() { "DBIT" } else { "CRDT" },
                b.date.format("%Y-%m-%d")
            ));
        }
        for i in list {
            let t = &v.transactions[i].tx;
            let credit = !t.amount.is_sign_negative();
            let (party, acct) = if credit { ("Dbtr", "DbtrAcct") } else { ("Cdtr", "CdtrAcct") };
            xml.push_str(&format!(
                "<Ntry><Amt Ccy=\"{ccy}\">{amt:.2}</Amt><CdtDbtInd>{ind}</CdtDbtInd><Sts><Cd>{sts}</Cd></Sts><BookgDt><Dt>{bd}</Dt></BookgDt><ValDt><Dt>{vd}</Dt></ValDt><BkTxCd/>\
<NtryDtls><TxDtls><Refs><EndToEndId>{e2e}</EndToEndId>{mref}</Refs><RltdPties><{party}><Pty><Nm>{name}</Nm></Pty></{party}><{acct}><Id><IBAN>{iban}</IBAN></Id></{acct}></RltdPties><RmtInf><Ustrd>{purpose}</Ustrd></RmtInf><AddtlTxInf>{posting}</AddtlTxInf></TxDtls></NtryDtls></Ntry>\n",
                ccy = x(&t.currency),
                amt = t.amount.abs(),
                ind = if credit { "CRDT" } else { "DBIT" },
                sts = if t.pending { "PDNG" } else { "BOOK" },
                bd = t.booking_date.format("%Y-%m-%d"),
                vd = t.value_date.format("%Y-%m-%d"),
                e2e = if t.end_to_end_id.is_empty() { "NOTPROVIDED".into() } else { x(&t.end_to_end_id) },
                mref = if t.mandate_id.is_empty() { String::new() } else { format!("<MndtId>{}</MndtId>", x(&t.mandate_id)) },
                name = x(&t.counterparty_name),
                iban = x(&t.counterparty_iban),
                purpose = x(&t.purpose),
                posting = x(&t.posting_text),
            ));
        }
        xml.push_str("</Rpt>\n");
    }
    xml.push_str("</BkToCstmrAcctRpt>\n</Document>\n");
    let path = target_dir().join(format!("kontor-camt052-{}.xml", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, xml).map_err(|e| e.to_string())?;
    Ok(path)
}
