use crate::*;
use chrono::{Duration, Local};
use rust_decimal::Decimal;

fn transfer() -> SepaTransfer {
    SepaTransfer { debtor_name: "Müller & Söhne".into(), debtor_iban: "DE89370400440532013000".into(), debtor_bic: "COBADEFFXXX".into(),
        creditor_name: "Test Empfänger".into(), creditor_iban: "DE12500105170648489890".into(), creditor_bic: "INGDDEFFXXX".into(),
        amount: Decimal::new(12345, 2), purpose: "Rechnung & Größe".into(), end_to_end_id: "REF-123".into(), execution_date: Some("2099-01-01".parse().unwrap()), instant: false }
}

#[test]
fn all_supported_pain_formats_roundtrip_transfer_fields_and_correct_xml_structure() {
    for format in ["pain.001.001.03", "pain.001.003.03", "pain.001.001.09"] { for instant in [false, true] {
        let mut t = transfer(); t.instant = instant;
        let xml = pain::build_pain001(format, &t, None); let s = std::str::from_utf8(&xml).unwrap(); let doc = roxmltree::Document::parse(s).unwrap();
        assert!(doc.root_element().tag_name().namespace().unwrap().ends_with(format));
        let value = |tag| doc.descendants().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).unwrap().to_string();
        assert_eq!(value("CtrlSum"), "123.45"); assert_eq!(value("EndToEndId"), "REF-123");
        assert_eq!(value(if format.ends_with(".09") { "BICFI" } else { "BIC" }), "COBADEFFXXX");
        assert!(s.contains("2099-01-01")); let result = pain::read_pain001(&xml).unwrap();
        assert_eq!(result.debtor_name, "Mueller & Soehne"); assert_eq!(result.creditor_name, "Test Empfaenger");
        assert_eq!(result.purpose, "Rechnung & Groesse"); assert_eq!(result.amount, t.amount); assert_eq!(result.instant, instant);
        assert_eq!(result.debtor_iban, t.debtor_iban); assert_eq!(result.creditor_iban, t.creditor_iban); assert_eq!(result.creditor_bic, t.creditor_bic);
    }}
}

#[test]
fn payment_validation_enforces_transliterated_name_purpose_and_reference_limits() {
    let good = transfer();
    for field in 0..4 {
        let mut t = good.clone();
        let (value, limit) = match field { 0 => (&mut t.debtor_name, 70), 1 => (&mut t.creditor_name, 70), 2 => (&mut t.purpose, 140), _ => (&mut t.end_to_end_id, 35) };
        *value = "x".repeat(limit); assert!(pain::validate_transfer(&t).is_ok());
        let value = match field { 0 => &mut t.debtor_name, 1 => &mut t.creditor_name, 2 => &mut t.purpose, _ => &mut t.end_to_end_id };
        value.push('x'); assert!(pain::validate_transfer(&t).is_err());
    }
    for name in ["", "   ", "💸"] { let mut t = good.clone(); t.creditor_name = name.into(); assert!(pain::validate_transfer(&t).is_err()); }
    let mut t = good; t.amount = "1.2300".parse().unwrap(); assert!(pain::validate_transfer(&t).is_ok());
}

#[test]
fn iban_checksum_accepts_international_examples_and_rejects_mutations() {
    for iban in ["DE89370400440532013000", "GB82WEST12345698765432", "FR1420041010050500013M02606", "de89 3704 0044 0532 0130 00"] { assert!(pain::iban_valid(iban), "{iban}"); }
    for iban in ["", "DE00370400440532013000", "1234370400440532013000", "DEAA370400440532013000", "DE8937040044053201300💸", "DE89-370400440532013000"] { assert!(!pain::iban_valid(iban), "{iban}"); }
}

#[test]
fn standing_validation_checks_monthly_weekly_ultimo_and_period_boundaries() {
    let today = Local::now().date_naive();
    let mut so = StandingOrder { order_id: String::new(), transfer: transfer(), first_date: today, last_date: None, execution_time: None,
        interval: Interval::Monthly, every: 1, day: 1, raw_pain: vec![], descriptor: String::new() };
    for day in [1, 28, 31, 97, 98, 99] { so.day = day; assert!(pain::validate_standing_schedule(&so).is_ok()); }
    for day in [0, 32, 96, 100] { so.day = day; assert!(pain::validate_standing_schedule(&so).is_err()); }
    so.interval = Interval::Weekly; for day in 1..=7 { so.day = day; assert!(pain::validate_standing_schedule(&so).is_ok()); }
    so.day = 8; assert!(pain::validate_standing_schedule(&so).is_err()); so.day = 1;
    so.first_date = today - Duration::days(1); assert!(pain::validate_standing_schedule(&so).is_err()); so.first_date = today;
    so.last_date = Some(today); assert!(pain::validate_standing_schedule(&so).is_ok());
    so.last_date = Some(today - Duration::days(1)); assert!(pain::validate_standing_schedule(&so).is_err());
}

#[test]
fn mt940_handles_year_rollover_reversals_multiple_statements_and_line_endings() {
    let body = ":20:A\n:60F:C261231EUR100,00\n:61:2701011231D10,00NTRFNONREF\n:86:Kauf\n:61:2701010101RC5,00NTRFNONREF\n:86:Storno Gutschrift\n:61:2701010101RD2,00NTRFNONREF\n:86:Storno Belastung\n:62F:C270101EUR87,00\n:20:B\n:60F:C270101EUR87,00\n:61:2701020102C1,00NTRFNONREF\n:86:Eingang\n";
    for newline in ["\n", "\r\n", "\r"] {
        let report = mt940::parse(body.replace('\n', newline).as_bytes(), false);
        assert_eq!(report.len(), 2); assert_eq!(report[0].transactions.len(), 3);
        assert_eq!(report[0].transactions[0].booking_date.to_string(), "2026-12-31");
        assert_eq!(report[0].transactions[0].value_date.to_string(), "2027-01-01");
        assert_eq!(report[0].transactions[1].amount, Decimal::from(-5)); assert_eq!(report[0].transactions[2].amount, Decimal::from(2));
        assert_eq!(report[0].closing.unwrap().0, Decimal::from(87)); assert!(!report[1].transactions[0].fingerprint.is_empty());
    }
}

#[test]
fn camt_supports_namespaces_datetime_fallback_and_all_pending_statuses() {
    for status in ["BOOK", "PDNG", "INFO"] {
        let xml = format!(r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.052.001.08"><BkToCstmrAcctRpt><Rpt><Ntry><Amt Ccy="USD">12.34</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>{status}</Cd></Sts><ValDt><DtTm>2026-11-01T23:59:59+01:00</DtTm></ValDt><NtryDtls><TxDtls><RltdPties><Dbtr><Pty><Nm>A &amp; B</Nm></Pty></Dbtr></RltdPties><RmtInf><Ustrd>Zeile 1</Ustrd><Ustrd>Zeile 2</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry></Rpt></BkToCstmrAcctRpt></Document>"#);
        let t = camt::parse(xml.as_bytes()).unwrap().transactions.remove(0);
        assert_eq!(t.currency, "USD"); assert_eq!(t.amount, Decimal::new(1234, 2)); assert_eq!(t.booking_date.to_string(), "2026-11-01");
        assert_eq!(t.booking_date, t.value_date); assert!(!t.date_unknown); assert_eq!(t.pending, status != "BOOK");
        assert_eq!(t.counterparty_name, "A & B"); assert_eq!(t.purpose, "Zeile 1 Zeile 2");
    }
}

#[test]
fn camt_error_in_later_entry_rejects_entire_report() {
    let entry = "<Ntry><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><BookgDt><Dt>2026-11-01</Dt></BookgDt></Ntry>";
    for bad in [entry.replace("1.00", "bad"), entry.replace("CRDT", "OTHER"), entry.replace("2026-11-01", "2026-02-30")] {
        assert!(camt::parse(format!("<Document><Rpt>{entry}{bad}</Rpt></Document>").as_bytes()).is_err());
    }
}

#[test]
fn fingerprints_ignore_unknown_sort_dates_but_distinguish_bank_references() {
    let mut t = Transaction { date_unknown: true, amount: Decimal::ONE, purpose: "Fixture".into(), ..Default::default() };
    t.compute_fingerprint(); let first = t.fingerprint.clone(); t.booking_date = "2099-01-01".parse().unwrap(); t.compute_fingerprint(); assert_eq!(t.fingerprint, first);
    t.end_to_end_id = "BANK-REF".into(); t.compute_fingerprint(); assert_ne!(t.fingerprint, first);
    let with_ref = t.fingerprint.clone(); t.date_unknown = false; t.compute_fingerprint(); assert_ne!(t.fingerprint, with_ref);
}

#[test]
fn photo_tan_matrix_payload_checks_mime_and_declared_lengths() {
    let mime = b"image/png"; let image = b"synthetic image";
    let mut data = (mime.len() as u16).to_be_bytes().to_vec(); data.extend(mime); data.extend((image.len() as u16).to_be_bytes()); data.extend(image);
    assert_eq!(tan::parse_matrix_code(&data), Some(("image/png".into(), image.to_vec())));
    for len in 0..data.len() { assert!(tan::parse_matrix_code(&data[..len]).is_none()); }
    data[2..11].copy_from_slice(b"text/html"); assert!(tan::parse_matrix_code(&data).is_none());
}

#[test]
fn optical_and_usb_tan_match_independent_hbci4java_reference_vectors() {
    // Published anonymous vectors from hbci4java/secmech/FlickerTest.java.
    // https://github.com/hbci4j/hbci4java/blob/master/src/test/java/org/kapott/hbci4java/secmech/FlickerTest.java
    for (input, expected) in [
        ("039870110490631098765432100812345678041,00", "1784011049063F059876543210041234567844312C303019"),
        ("039870110418751012345678900812030000040,20", "1784011041875F051234567890041203000044302C323015"),
        ("0248A0120452019980812345678", "0D85012045201998041234567855"),
        ("...TAN-Nummer: CHLGUC 002624088715131306389726041,00CHLGTEXT0244 Sie h...", "0F04871513130338972614312C30303B"),
        ("0248A01204520199808123F5678", "118501204520199848313233463536373875"),
    ] {
        let code = tan::FlickerCode::parse(input).unwrap(); assert_eq!(code.render(), expected);
        assert_eq!(code.usb_payload().iter().map(|b| format!("{b:02X}")).collect::<String>(), expected);
        let frames = code.frames(); assert!(!frames.is_empty());
        let (pairs, remainder) = frames.as_chunks::<2>(); assert!(remainder.is_empty());
        for pair in pairs { assert!(pair[0][0]); assert!(!pair[1][0]); assert_eq!(pair[0][1..], pair[1][1..]); }
    }
}

#[test]
fn bpd_version_negotiation_chooses_highest_common_supported_version() {
    let mut b = bpd::Bpd::default(); b.versions.insert("HKSAL".into(), vec![9, 5, 7, 6]);
    assert_eq!(b.version_for("HKSAL", &[5, 6, 7]), Some(7)); assert_eq!(b.version_for("HKSAL", &[1]), None); assert_eq!(b.version_for("UNKNOWN", &[1]), None);
    assert!(!b.needs_tan("HKSAL")); b.tan_required.insert("HKSAL".into(), true); assert!(b.needs_tan("HKSAL"));
    b.pain001_formats = vec!["pain.001.003.03".into(), "pain.001.001.03".into(), "pain.001.001.09".into()]; assert_eq!(b.preferred_pain001(), "pain.001.001.09");
}

#[test]
fn ddv_envelope_uses_card_signature_counter_and_encrypts_business_payload() {
    use security::*;
    struct Card { counter: u32, verified: bool, signed: bool }
    impl DdvCard for Card {
        fn cid(&mut self) -> Result<Vec<u8>> { Ok(vec![1, 2, 3]) }
        fn bank_data(&mut self, _: u8) -> Result<DdvBankData> { Ok(DdvBankData::default()) }
        fn keys(&mut self) -> Result<(DdvKey, DdvKey)> { Ok((DdvKey { num: 1, version: 1 }, DdvKey { num: 2, version: 1 })) }
        fn sig_id(&mut self) -> Result<u32> { Ok(self.counter) }
        fn set_sig_id(&mut self, id: u32) -> Result<()> { self.counter = id; Ok(()) }
        fn verify_pin(&mut self) -> Result<()> { self.verified = true; Ok(()) }
        fn sign(&mut self, hash: &[u8; 20]) -> Result<[u8; 8]> { assert!(self.verified); assert_ne!(*hash, [0; 20]); self.signed = true; Ok([7; 8]) }
        fn session_keys(&mut self, key: u32) -> Result<([u8; 16], [u8; 16])> { assert_eq!(key, 2); Ok(([1; 16], [2; 16])) }
        fn decrypt_key(&mut self, key: u32, encrypted: &[u8]) -> Result<[u8; 16]> { assert_eq!(key, 2); assert_eq!(encrypted, [2; 16]); Ok([1; 16]) }
    }
    let mut card = Card { counter: 7, verified: false, signed: false }; let (sign, enc) = card.keys().unwrap();
    let env = Envelope { dialog_id: "TEST", message_number: 1, blz: "12345678", user_id: "synthetic-user", system_id: "TEST" };
    let wire = build_message(&env, Auth::Ddv { card: &mut card, cid: &[1, 2, 3], sig_key: sign, enc_key: enc },
        vec![syntax::SegmentBuilder::new("HKSAL", 5).de("PRIVATE-FIXTURE-ACCOUNT")]).unwrap();
    assert!(card.verified && card.signed); assert_eq!(card.counter, 8);
    assert!(!String::from_utf8_lossy(&wire).contains("PRIVATE-FIXTURE-ACCOUNT"));
    response::validate_envelope(&wire, "TEST", 1).unwrap();
    let opened = open_message(&wire, Opener::Ddv(&mut card, 2)).unwrap();
    assert_eq!(opened.iter().find(|s| s.code == "HKSAL").unwrap().text_or_empty(1, 0), "PRIVATE-FIXTURE-ACCOUNT");
}
