//! pain.001 generation (SEPA credit transfer) for 001.001.03, 001.003.03 and 001.001.09
//! and a minimal reader to recover transfers from bank-supplied documents (standing orders).

use crate::model::SepaTransfer;
use chrono::{Local, NaiveDate};

fn esc(s: &str) -> String {
    sepa_charset(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

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
    format!("KONTOR-{}-{:08X}", Local::now().format("%Y%m%d%H%M%S"), r as u32)
}

/// Build a pain.001 document for `descriptor` (the urn from HISPAS).
pub fn build_pain001(descriptor: &str, t: &SepaTransfer, default_exec: Option<NaiveDate>) -> Vec<u8> {
    let v09 = descriptor.contains("001.001.09");
    let ns = if descriptor.contains("001.003.03") {
        "urn:iso:std:iso:20022:tech:xsd:pain.001.003.03"
    } else if v09 {
        "urn:iso:std:iso:20022:tech:xsd:pain.001.001.09"
    } else {
        "urn:iso:std:iso:20022:tech:xsd:pain.001.001.03"
    };
    let bic_tag = if v09 { "BICFI" } else { "BIC" };
    let exec = t.execution_date.or(default_exec).unwrap_or_else(|| NaiveDate::from_ymd_opt(1999, 1, 1).unwrap());
    let exec_xml = if v09 {
        format!("<ReqdExctnDt><Dt>{}</Dt></ReqdExctnDt>", exec.format("%Y-%m-%d"))
    } else {
        format!("<ReqdExctnDt>{}</ReqdExctnDt>", exec.format("%Y-%m-%d"))
    };
    let amount = format!("{:.2}", t.amount);
    let id = msg_id();
    let e2e = if t.end_to_end_id.is_empty() { "NOTPROVIDED".to_string() } else { esc(&t.end_to_end_id) };
    let agent = |bic: &str| {
        if bic.trim().is_empty() {
            "<FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr></FinInstnId>".to_string()
        } else {
            format!("<FinInstnId><{bic_tag}>{}</{bic_tag}></FinInstnId>", esc(bic.trim()))
        }
    };
    let svc = if t.instant {
        "<PmtTpInf><SvcLvl><Cd>SEPA</Cd></SvcLvl><LclInstrm><Cd>INST</Cd></LclInstrm></PmtTpInf>"
    } else {
        "<PmtTpInf><SvcLvl><Cd>SEPA</Cd></SvcLvl></PmtTpInf>"
    };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="{ns}" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<CstmrCdtTrfInitn>
<GrpHdr><MsgId>{id}</MsgId><CreDtTm>{now}</CreDtTm><NbOfTxs>1</NbOfTxs><CtrlSum>{amount}</CtrlSum><InitgPty><Nm>{debtor}</Nm></InitgPty></GrpHdr>
<PmtInf><PmtInfId>{id}-1</PmtInfId><PmtMtd>TRF</PmtMtd><NbOfTxs>1</NbOfTxs><CtrlSum>{amount}</CtrlSum>{svc}{exec_xml}
<Dbtr><Nm>{debtor}</Nm></Dbtr><DbtrAcct><Id><IBAN>{diban}</IBAN></Id></DbtrAcct><DbtrAgt>{dagent}</DbtrAgt><ChrgBr>SLEV</ChrgBr>
<CdtTrfTxInf><PmtId><EndToEndId>{e2e}</EndToEndId></PmtId><Amt><InstdAmt Ccy="EUR">{amount}</InstdAmt></Amt>{cagent}<Cdtr><Nm>{creditor}</Nm></Cdtr><CdtrAcct><Id><IBAN>{ciban}</IBAN></Id></CdtrAcct><RmtInf><Ustrd>{purpose}</Ustrd></RmtInf></CdtTrfTxInf>
</PmtInf>
</CstmrCdtTrfInitn>
</Document>"#,
        now = Local::now().format("%Y-%m-%dT%H:%M:%S"),
        debtor = esc(&truncate(&t.debtor_name, 70)),
        diban = esc(&t.debtor_iban.replace(' ', "")),
        dagent = agent(&t.debtor_bic),
        cagent = if t.creditor_bic.trim().is_empty() { String::new() } else { format!("<CdtrAgt>{}</CdtrAgt>", agent(&t.creditor_bic)) },
        creditor = esc(&truncate(&t.creditor_name, 70)),
        ciban = esc(&t.creditor_iban.replace(' ', "")),
        purpose = esc(&truncate(&t.purpose, 140)),
    );
    xml.into_bytes()
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Read the first transaction of a pain.001 document (used for standing order lists).
pub fn read_pain001(xml: &[u8]) -> Option<SepaTransfer> {
    let s = String::from_utf8_lossy(xml);
    let doc = roxmltree::Document::parse(&s).ok()?;
    let find = |name: &str| doc.descendants().find(|n| n.is_element() && n.tag_name().name() == name);
    let text_in = |parent: Option<roxmltree::Node>, names: &[&str]| -> String {
        let mut cur = match parent {
            Some(p) => p,
            None => return String::new(),
        };
        for n in names {
            match cur.children().find(|c| c.is_element() && c.tag_name().name() == *n) {
                Some(c) => cur = c,
                None => return String::new(),
            }
        }
        cur.text().unwrap_or("").trim().to_string()
    };
    let pmt = find("PmtInf");
    let tx = find("CdtTrfTxInf");
    let bic = |p: Option<roxmltree::Node>, agt: &str| {
        let b = text_in(p, &[agt, "FinInstnId", "BIC"]);
        if b.is_empty() { text_in(p, &[agt, "FinInstnId", "BICFI"]) } else { b }
    };
    Some(SepaTransfer {
        debtor_name: text_in(pmt, &["Dbtr", "Nm"]),
        debtor_iban: text_in(pmt, &["DbtrAcct", "Id", "IBAN"]),
        debtor_bic: bic(pmt, "DbtrAgt"),
        creditor_name: text_in(tx, &["Cdtr", "Nm"]),
        creditor_iban: text_in(tx, &["CdtrAcct", "Id", "IBAN"]),
        creditor_bic: bic(tx, "CdtrAgt"),
        amount: text_in(tx, &["Amt", "InstdAmt"]).parse().ok()?,
        purpose: text_in(tx, &["RmtInf", "Ustrd"]),
        end_to_end_id: text_in(tx, &["PmtId", "EndToEndId"]),
        execution_date: None,
        instant: false,
    })
}

/// IBAN checksum validation (ISO 13616, mod 97).
pub fn iban_valid(iban: &str) -> bool {
    let s: String = iban.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if s.len() < 15 || s.len() > 34 || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
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
