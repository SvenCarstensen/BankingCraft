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

fn date_at(n: Node, names: &[&str]) -> Result<Option<NaiveDate>, String> {
    let Some(node) = path(n, names) else { return Ok(None) };
    let s = child(node, "Dt")
        .and_then(|d| d.text())
        .or_else(|| child(node, "DtTm").and_then(|d| d.text()));
    let Some(s) = s.filter(|s| !s.trim().is_empty()) else { return Ok(None) };
    let s = s.trim();
    s.get(..10).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(Some).ok_or_else(|| i18n::tr("Ungültiges camt-Datum").into())
}

pub fn parse(xml: &[u8]) -> Result<Report, String> {
    parse_context(xml, false)
}

/// FinTS can mark the entire HICAZ payload as pending even without an entry status.
pub fn parse_pending(xml: &[u8]) -> Result<Report, String> {
    parse_context(xml, true)
}

fn parse_context(xml: &[u8], pending: bool) -> Result<Report, String> {
    let s = String::from_utf8_lossy(xml);
    let doc = Document::parse_with_options(&s, roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 250_000 })
        .map_err(|_| i18n::tr("Ungültiges oder zu umfangreiches camt-Dokument").to_string())?;
    let mut report = Report::default();
    if !doc.descendants().any(|n| n.is_element() && matches!(n.tag_name().name(), "Rpt" | "Stmt")) {
        return Err(i18n::tr("camt enthält keinen Kontoauszug").into());
    }
    for rpt in doc.descendants().filter(|n| n.is_element() && (n.tag_name().name() == "Rpt" || n.tag_name().name() == "Stmt")) {
        // Both entry dates are optional in camt. Never invent a displayed booking date:
        // retain an internal report/receipt date for sorting and mark the date as unknown.
        let report_date = text_at(rpt, &["CreDtTm"]).get(..10)
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        for bal in rpt.children().filter(|c| c.is_element() && c.tag_name().name() == "Bal") {
            let code = text_at(bal, &["Tp", "CdOrPrtry", "Cd"]);
            if code == "CLBD" || code == "ITBD" {
                let amount: Decimal = text_at(bal, &["Amt"]).parse().map_err(|_| i18n::tr("Ungültiger camt-Saldo"))?;
                let sign = if text_at(bal, &["CdtDbtInd"]) == "DBIT" { -Decimal::ONE } else { Decimal::ONE };
                if let Some(d) = date_at(bal, &["Dt"])? {
                    report.closing_booked = Some((amount * sign, d));
                }
            }
        }
        for ntry in rpt.children().filter(|c| c.is_element() && c.tag_name().name() == "Ntry") {
            report.transactions.push(parse_entry(ntry, report_date, pending)?);
        }
    }
    Ok(report)
}

fn parse_entry(ntry: Node, report_date: NaiveDate, force_pending: bool) -> Result<Transaction, String> {
    let amount_node = child(ntry, "Amt");
    let amount: Decimal = amount_node.and_then(|n| n.text()).ok_or(i18n::tr("camt-Betrag fehlt"))?.trim().parse().map_err(|_| i18n::tr("Ungültiger camt-Betrag"))?;
    let currency = amount_node.and_then(|n| n.attribute("Ccy")).unwrap_or("EUR").to_string();
    let credit = match text_at(ntry, &["CdtDbtInd"]).as_str() {
        "CRDT" => true,
        "DBIT" => false,
        _ => return Err(i18n::tr("Ungültiges camt-Soll/Haben-Kennzeichen").into()),
    };
    let status = {
        let s = text_at(ntry, &["Sts"]);
        if s.is_empty() { text_at(ntry, &["Sts", "Cd"]) } else { s }
    };
    let booking = date_at(ntry, &["BookgDt"])?;
    let value = date_at(ntry, &["ValDt"])?;
    let date_unknown = booking.is_none() && value.is_none();
    let booking = booking.or(value).unwrap_or(report_date);
    let value = value.unwrap_or(booking);

    let mut t = Transaction {
        booking_date: booking,
        value_date: value,
        date_unknown,
        amount: if credit { amount } else { -amount },
        currency,
        pending: force_pending || matches!(status.as_str(), "PDNG" | "INFO"),
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
    Ok(t)
}
