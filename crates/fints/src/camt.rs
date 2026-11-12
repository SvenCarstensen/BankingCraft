//! camt.052 (account report) parser, namespace agnostic (001.02 ... 001.08).

use crate::model::Transaction;
use chrono::NaiveDate;
use roxmltree::{Document, Node};
use rust_decimal::Decimal;

#[derive(Debug, Default)]
pub struct Report {
    pub closing_booked: Option<(Decimal, NaiveDate)>,
    pub transactions: Vec<Transaction>,
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn path<'a, 'i>(n: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    let mut cur = n;
    for name in names {
        cur = child(cur, name)?;
    }
    Some(cur)
}

fn text_at(n: Node, names: &[&str]) -> String {
    path(n, names).and_then(|x| x.text()).unwrap_or("").trim().to_string()
}

fn date_at(n: Node, names: &[&str]) -> Option<NaiveDate> {
    let node = path(n, names)?;
    let s = child(node, "Dt")
        .and_then(|d| d.text())
        .or_else(|| child(node, "DtTm").and_then(|d| d.text()))?;
    NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d").ok()
}

pub fn parse(xml: &[u8]) -> Result<Report, String> {
    let s = String::from_utf8_lossy(xml);
    let doc = Document::parse(&s).map_err(|e| e.to_string())?;
    let mut report = Report::default();
    for rpt in doc.descendants().filter(|n| n.is_element() && (n.tag_name().name() == "Rpt" || n.tag_name().name() == "Stmt")) {
        for bal in rpt.children().filter(|c| c.is_element() && c.tag_name().name() == "Bal") {
            let code = text_at(bal, &["Tp", "CdOrPrtry", "Cd"]);
            if code == "CLBD" || code == "ITBD" {
                let amount: Decimal = text_at(bal, &["Amt"]).parse().unwrap_or_default();
                let sign = if text_at(bal, &["CdtDbtInd"]) == "DBIT" { -Decimal::ONE } else { Decimal::ONE };
                if let Some(d) = date_at(bal, &["Dt"]) {
                    report.closing_booked = Some((amount * sign, d));
                }
            }
        }
        for ntry in rpt.children().filter(|c| c.is_element() && c.tag_name().name() == "Ntry") {
            report.transactions.push(parse_entry(ntry));
        }
    }
    Ok(report)
}

fn parse_entry(ntry: Node) -> Transaction {
    let amount_node = child(ntry, "Amt");
    let amount: Decimal = amount_node.and_then(|n| n.text()).unwrap_or("0").trim().parse().unwrap_or_default();
    let currency = amount_node.and_then(|n| n.attribute("Ccy")).unwrap_or("EUR").to_string();
    let credit = text_at(ntry, &["CdtDbtInd"]) != "DBIT";
    let status = {
        let s = text_at(ntry, &["Sts"]);
        if s.is_empty() { text_at(ntry, &["Sts", "Cd"]) } else { s }
    };
    let booking = date_at(ntry, &["BookgDt"]).or_else(|| date_at(ntry, &["ValDt"])).unwrap_or_default();
    let value = date_at(ntry, &["ValDt"]).unwrap_or(booking);

    let mut t = Transaction {
        booking_date: booking,
        value_date: value,
        amount: if credit { amount } else { -amount },
        currency,
        pending: status == "PDNG",
        posting_text: text_at(ntry, &["AddtlNtryInf"]),
        ..Default::default()
    };

    if let Some(tx) = path(ntry, &["NtryDtls", "TxDtls"]) {
        // counterparty is the debtor for credits and the creditor for debits
        let (party, acct, agent) = if credit { ("Dbtr", "DbtrAcct", "DbtrAgt") } else { ("Cdtr", "CdtrAcct", "CdtrAgt") };
        let rp = child(tx, "RltdPties");
        if let Some(rp) = rp {
            let mut name = text_at(rp, &[party, "Nm"]);
            if name.is_empty() {
                name = text_at(rp, &[party, "Pty", "Nm"]);
            }
            t.counterparty_name = name;
            t.counterparty_iban = text_at(rp, &[acct, "Id", "IBAN"]);
            if t.counterparty_iban.is_empty() {
                t.counterparty_iban = text_at(rp, &[acct, "Id", "Othr", "Id"]);
            }
        }
        if let Some(ra) = child(tx, "RltdAgts") {
            t.counterparty_bic = text_at(ra, &[agent, "FinInstnId", "BIC"]);
            if t.counterparty_bic.is_empty() {
                t.counterparty_bic = text_at(ra, &[agent, "FinInstnId", "BICFI"]);
            }
        }
        let purposes: Vec<String> = child(tx, "RmtInf")
            .map(|r| {
                r.children()
                    .filter(|c| c.is_element() && c.tag_name().name() == "Ustrd")
                    .filter_map(|c| c.text().map(|s| s.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        t.purpose = purposes.join(" ");
        t.end_to_end_id = text_at(tx, &["Refs", "EndToEndId"]);
        if t.end_to_end_id == "NOTPROVIDED" {
            t.end_to_end_id.clear();
        }
        t.mandate_id = text_at(tx, &["Refs", "MndtId"]);
        let add = text_at(tx, &["AddtlTxInf"]);
        if !add.is_empty() {
            t.posting_text = add;
        }
    }
    t.compute_fingerprint();
    t
}
