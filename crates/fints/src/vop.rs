//! FinTS VoP 1.02 (2026-10-19) and DK Anlage 3, 2.2.6 (pain.002.001.10).
//! A result is scoped to the exact single payment submitted with HKVPP.
use crate::{error::{Error, Result}, syntax::Segment, SepaTransfer};
use roxmltree::{Document, Node};

pub const REPORT_FORMAT: &str = "urn:iso:std:iso:20022:tech:xsd:pain.002.001.10";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayeeStatus { Match, CloseMatch, NoMatch, Unavailable }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayeeVerification {
    pub status: PayeeStatus,
    pub name: String,
    pub iban: String,
    pub suggested_name: String,
    pub reason: String,
    pub bank_notice: String,
}
pub(crate) fn invalid() -> Error {
    Error::Security(i18n::tr("Empfängerprüfung unvollständig oder ungültig; keine Zahlungsfreigabe erteilt").into())
}
fn status(code: &str) -> Result<PayeeStatus> {
    match code {
        "RCVC" => Ok(PayeeStatus::Match), "RVMC" => Ok(PayeeStatus::CloseMatch),
        "RVNM" => Ok(PayeeStatus::NoMatch), "RVNA" => Ok(PayeeStatus::Unavailable), _ => Err(invalid()),
    }
}
fn normalized_iban(s: &str) -> String { s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase() }
fn child<'a, 'i>(n: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> { n.children().find(|n| n.has_tag_name(tag)) }
fn value<'a, 'i>(n: Node<'a, 'i>, path: &[&str]) -> Option<&'a str> {
    let mut n = n;
    for tag in path { n = child(n, tag)?; }
    n.text()
}
fn xml(bytes: &[u8]) -> Result<Document<'_>> {
    if bytes.len() > 1_048_576 { return Err(invalid()); }
    Document::parse_with_options(std::str::from_utf8(bytes).map_err(|_| invalid())?, roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 25_000 }).map_err(|_| invalid())
}

/// For a single transfer the final report contains at most one transaction. A missing
/// detail is a match only when the group/count explicitly confirms that result.
fn report(bytes: &[u8], original: &[u8], t: &SepaTransfer) -> Result<(PayeeStatus, String, String, String)> {
    let doc = xml(bytes)?;
    let root = doc.root_element();
    if root.tag_name().name() != "Document" || root.tag_name().namespace() != Some(REPORT_FORMAT) { return Err(invalid()); }
    let report = child(root, "CstmrPmtStsRpt").ok_or_else(invalid)?;
    let group = child(report, "OrgnlGrpInfAndSts").ok_or_else(invalid)?;
    let payment = xml(original)?;
    let original = child(payment.root_element(), "CstmrCdtTrfInitn").ok_or_else(invalid)?;
    if value(group, &["OrgnlMsgId"]) != value(original, &["GrpHdr", "MsgId"]) { return Err(invalid()); }
    if value(group, &["OrgnlNbOfTxs"]).is_some_and(|n| n != "1") { return Err(invalid()); }
    let infos: Vec<_> = report.children().filter(|n| n.has_tag_name("OrgnlPmtInfAndSts")).collect();
    if infos.len() > 1 { return Err(invalid()); }
    if let Some(info) = infos.first() {
        if value(*info, &["OrgnlPmtInfId"]) != value(original, &["PmtInf", "PmtInfId"]) { return Err(invalid()); }
    }
    let txs: Vec<_> = infos.iter().flat_map(|n| n.children().filter(|n| n.has_tag_name("TxInfAndSts"))).collect();
    if txs.len() > 1 { return Err(invalid()); }
    let mut notices = Vec::new();
    for reason in group.children().filter(|n| n.has_tag_name("StsRsnInf")) {
        notices.extend(reason.children().filter(|n| n.has_tag_name("AddtlInf")).filter_map(|n| n.text()).map(str::to_owned));
    }
    let counts: Vec<_> = group.children().filter(|n| n.has_tag_name("NbOfTxsPerSts")).collect();
    let mut count_status = None;
    for count in counts {
        let n: u32 = value(count, &["DtldNbOfTxs"]).ok_or_else(invalid)?.parse().map_err(|_| invalid())?;
        if n == 0 { continue; }
        if n != 1 || count_status.is_some() { return Err(invalid()); }
        count_status = Some(status(value(count, &["DtldSts"]).ok_or_else(invalid)?)?);
    }
    if let Some(tx) = txs.first().copied() {
        let result = status(value(tx, &["TxSts"]).ok_or_else(invalid)?)?;
        if count_status.is_some_and(|s| s != result) || (value(group, &["GrpSts"]) == Some("RCVC") && result != PayeeStatus::Match) { return Err(invalid()); }
        if value(tx, &["OrgnlEndToEndId"]) != value(original, &["PmtInf", "CdtTrfTxInf", "PmtId", "EndToEndId"]) { return Err(invalid()); }
        let reference = child(tx, "OrgnlTxRef").ok_or_else(invalid)?;
        if normalized_iban(value(reference, &["CdtrAcct", "Id", "IBAN"]).ok_or_else(invalid)?) != normalized_iban(&t.creditor_iban)
            || value(reference, &["Cdtr", "Pty", "Nm"]) != value(original, &["PmtInf", "CdtTrfTxInf", "Cdtr", "Nm"]) { return Err(invalid()); }
        let mut additional = Vec::new();
        let mut reason_code = String::new();
        for reason in tx.children().filter(|n| n.has_tag_name("StsRsnInf")) {
            if let Some(code) = value(reason, &["Rsn", "Cd"]) { reason_code = code.into(); }
            additional.extend(reason.children().filter(|n| n.has_tag_name("AddtlInf")).filter_map(|n| n.text()));
        }
        let suggested = if result == PayeeStatus::CloseMatch {
            additional.iter().enumerate().map(|(i, s)| if i == 0 { *s } else { s.trim_start_matches(['"', '“']) }).collect::<String>()
        } else { String::new() };
        let reason = if result == PayeeStatus::Unavailable { format!("{} {}", reason_code, additional.join("\n")).trim().to_owned() } else { String::new() };
        Ok((result, suggested, reason, notices.join("\n")))
    } else if count_status == Some(PayeeStatus::Match) || (count_status.is_none() && value(group, &["GrpSts"]) == Some("RCVC")) {
        Ok((PayeeStatus::Match, String::new(), String::new(), notices.join("\n")))
    } else { Err(invalid()) }
}

pub(crate) fn parse(s: &Segment, original: &[u8], t: &SepaTransfer, structured: bool) -> Result<PayeeVerification> {
    if s.version != 1 { return Err(invalid()); }
    let raw = s.bin(5, 0).filter(|b| !b.is_empty());
    let direct = s.text(6, 4);
    if direct.is_none() && s.de(6).iter().flatten().any(|b| !b.is_empty()) { return Err(invalid()); }
    if raw.is_some() && direct.is_some() { return Err(invalid()); }
    let (result, suggested_name, reason, notice) = if let Some(raw) = raw {
        if s.text_or_empty(4, 0) != REPORT_FORMAT { return Err(invalid()); }
        report(raw, original, t)?
    } else if let Some(code) = direct {
        if normalized_iban(&s.text_or_empty(6, 0)) != normalized_iban(&t.creditor_iban) || !s.text_or_empty(6, 3).is_empty() { return Err(invalid()); }
        (status(&code)?, s.text_or_empty(6, 2), s.text_or_empty(6, 5), String::new())
    } else {
        // The current specification permits a complete response without a result.
        // Never present that as a match, including the "notprovided" migration ID.
        (PayeeStatus::Unavailable, String::new(), i18n::tr("Die Bank hat kein Prüfergebnis geliefert.").into(), String::new())
    };
    if result == PayeeStatus::CloseMatch && suggested_name.trim().is_empty() { return Err(invalid()); }
    let bank_text = s.text_or_empty(7, 0);
    let bank_text = if structured { plain_notice(&bank_text) } else { bank_text };
    let bank_notice = [notice, bank_text].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    Ok(PayeeVerification { status: result, name: crate::pain::sepa_charset(&t.creditor_name), iban: normalized_iban(&t.creditor_iban), suggested_name, reason, bank_notice })
}

/// Render the bank's limited formatting as plain text; preserve link destinations.
fn plain_notice(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else { out.push_str(&rest[start..]); return out };
        let tag = &rest[start + 1..start + end];
        if matches!(tag, "br" | "br/" | "br /" | "p" | "/p" | "/li") { out.push('\n'); }
        else if tag == "li" { out.push_str("• "); }
        else if let Some(link) = tag.strip_prefix("a href=\"").and_then(|s| s.strip_suffix('"')) { out.push_str(&format!("({link}) ")); }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&nbsp;", " ").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pain, syntax::{parse_segments, SegmentBuilder, Val}, response::Response, bpd::Bpd};
    fn payment() -> SepaTransfer {
        SepaTransfer { debtor_name: "Sender".into(), debtor_iban: "DE89370400440532013000".into(), debtor_bic: "COBADEFFXXX".into(), creditor_name: "Test Receiver".into(), creditor_iban: "DE12500105170648489890".into(), creditor_bic: "INGDDEFFXXX".into(), amount: rust_decimal::Decimal::new(1234, 2), purpose: "Test".into(), end_to_end_id: "REFERENCE".into(), execution_date: None, instant: false }
    }
    fn payload() -> Vec<u8> { pain::build_pain001("urn:iso:std:iso:20022:tech:xsd:pain.001.001.09", &payment(), None) }
    fn native(code: &str, name: &str) -> Segment {
        parse_segments(&SegmentBuilder::new("HIVPP", 1).bin(vec![1, 2, 3]).de(Val::Empty).de(Val::Empty).de(Val::Empty).de(Val::Empty)
            .deg(vec![payment().creditor_iban.into(), Val::Empty, name.into(), Val::Empty, code.into(), "Reason from bank".into()]).de("<p>Bank <b>notice</b></p><p>Second paragraph</p>").encode()).unwrap().remove(0)
    }
    fn report_xml(original: &[u8], code: &str) -> String {
        let doc = xml(original).unwrap(); let p = child(doc.root_element(), "CstmrCdtTrfInitn").unwrap();
        let msg = value(p, &["GrpHdr", "MsgId"]).unwrap(); let info = value(p, &["PmtInf", "PmtInfId"]).unwrap();
        let details = if code == "RCVC" { String::new() } else { format!("<OrgnlPmtInfAndSts><OrgnlPmtInfId>{info}</OrgnlPmtInfId><TxInfAndSts><OrgnlEndToEndId>REFERENCE</OrgnlEndToEndId><TxSts>{code}</TxSts><StsRsnInf><Rsn><Cd>AG03</Cd></Rsn><AddtlInf>Test Receiver GmbH</AddtlInf><AddtlInf>\" Berlin</AddtlInf></StsRsnInf><OrgnlTxRef><Cdtr><Pty><Nm>Test Receiver</Nm></Pty></Cdtr><CdtrAcct><Id><IBAN>DE12500105170648489890</IBAN></Id></CdtrAcct></OrgnlTxRef></TxInfAndSts></OrgnlPmtInfAndSts>") };
        format!("<Document xmlns=\"{REPORT_FORMAT}\"><CstmrPmtStsRpt><GrpHdr><MsgId>REPORT</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>{msg}</OrgnlMsgId><OrgnlNbOfTxs>1</OrgnlNbOfTxs><StsRsnInf><AddtlInf>{code} Legal notice line one.</AddtlInf><AddtlInf>{code} Legal notice line two.</AddtlInf></StsRsnInf><NbOfTxsPerSts><DtldNbOfTxs>1</DtldNbOfTxs><DtldSts>{code}</DtldSts></NbOfTxsPerSts></OrgnlGrpInfAndSts>{details}</CstmrPmtStsRpt></Document>")
    }
    #[test]
    fn native_statuses_and_bank_notice_are_preserved_and_bound_to_iban() {
        for (code, expected) in [("RCVC", PayeeStatus::Match), ("RVMC", PayeeStatus::CloseMatch), ("RVNM", PayeeStatus::NoMatch), ("RVNA", PayeeStatus::Unavailable)] {
            let mut s = native(code, "Test Receiver GmbH"); let r = parse(&s, &payload(), &payment(), true).unwrap();
            assert_eq!(r.status, expected); assert!(r.bank_notice.contains("Bank notice\n\nSecond paragraph"));
            s.des[6][0] = Some(b"DE89370400440532013000".to_vec()); assert!(parse(&s, &payload(), &payment(), true).is_err());
        }
        for code in ["PDNG", "PNDG", "ACCP", "XXXX"] { assert!(parse(&native(code, ""), &payload(), &payment(), false).is_err()); }
        assert!(parse(&native("RVMC", ""), &payload(), &payment(), false).is_err());
        let s = native("RCVC", ""); assert!(parse(&s, &payload(), &payment(), false).unwrap().bank_notice.contains("<b>"));
    }
    #[test]
    fn pain_report_maps_all_results_with_legal_notices_and_suggested_name() {
        let p = payload();
        for (code, expected) in [("RCVC", PayeeStatus::Match), ("RVMC", PayeeStatus::CloseMatch), ("RVNM", PayeeStatus::NoMatch), ("RVNA", PayeeStatus::Unavailable)] {
            let result = report(report_xml(&p, code).as_bytes(), &p, &payment()).unwrap();
            assert_eq!(result.0, expected); assert!(result.3.contains("Legal notice line one.")); assert!(result.3.contains("Legal notice line two."));
            if code == "RVMC" { assert_eq!(result.1, "Test Receiver GmbH Berlin"); }
            if code == "RVNA" { assert!(result.2.contains("AG03")); }
        }
    }
    #[test]
    fn pain_report_rejects_wrong_payment_unknown_pending_or_conflicting_results() {
        let p = payload(); let good = report_xml(&p, "RVMC");
        for (from, to) in [("REFERENCE", "OTHER"), ("DE12500105170648489890", "DE89370400440532013000"), ("<Nm>Test Receiver</Nm>", "<Nm>Different</Nm>"), ("<DtldNbOfTxs>1", "<DtldNbOfTxs>2"), ("<TxSts>RVMC", "<TxSts>RVNM"), ("<TxSts>RVMC", "<TxSts>PDNG"), (REPORT_FORMAT, "unknown")] {
            assert!(report(good.replace(from, to).as_bytes(), &p, &payment()).is_err(), "{from}");
        }
        assert!(report(good.as_bytes(), &payload(), &payment()).is_err(), "new pain message has different identity");
        assert!(report(report_xml(&p, "PDNG").as_bytes(), &p, &payment()).is_err());
        assert!(report(b"<!DOCTYPE Document [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><Document>&x;</Document>", &p, &payment()).is_err());
        assert!(report(&vec![b'x'; 1_048_577], &p, &payment()).is_err());
    }
    #[test]
    fn bpd_advertises_supported_jobs_and_report_format() {
        let segment = SegmentBuilder::new("HIVPPS", 1).de("1").de("1").de("1").deg(vec!["1".into(), "J".into(), "V".into(), "N".into(), "N".into(), REPORT_FORMAT.into(), "HKCCS".into(), "HKIPZ".into()]);
        let mut bpd = Bpd::default(); bpd.update_from(&Response::new(parse_segments(&segment.encode()).unwrap()));
        assert!(bpd.vop_structured_notice); assert_eq!(bpd.vop_jobs, ["HKCCS", "HKIPZ"]); assert_eq!(bpd.vop_formats, [REPORT_FORMAT]); assert!(bpd.supports("HKVPP"));
    }
}
