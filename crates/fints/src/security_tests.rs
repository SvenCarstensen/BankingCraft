use crate::{security::*, syntax::*, *};

#[test]
fn https_configuration_rejects_ambiguous_destinations_before_credentials() {
    for url in ["http://bank.example", "https://bank.example@attacker.example", "https://user:password@bank.example", "https://bank.example/#other", "https://bank.example\\@other", "https://bank.example/\npath", "https://"] {
        assert!(transport::validate_https_url(url).is_err(), "{url}");
    }
    for url in ["https://bank.example/fints", "https://bank.example:443/fints?version=300"] { assert!(transport::validate_https_url(url).is_ok()); }
}

#[test]
fn parser_rejects_structure_expansion_in_both_data_and_signature_parsers() {
    let inputs = [format!("HIKAZ:1:7+{}'", ":".repeat(65_537)), "HIRMG:1:2+0010::OK'".repeat(8193),
        (0..5).map(|_| format!("HIKAZ:1:7+{}'", ":".repeat(60_000))).collect::<String>()];
    for input in inputs {
        assert!(parse_segments(input.as_bytes()).is_err()); assert!(segment_spans(input.as_bytes()).is_err());
    }
}

#[test]
fn xml_readers_reject_entities_and_excessive_node_counts() {
    let entity = br#"<!DOCTYPE Document [<!ENTITY secret SYSTEM "file:///nonexistent-test-file">]><Document><Rpt>&secret;</Rpt></Document>"#;
    assert!(camt::parse(entity).is_err()); assert!(pain::read_pain001(entity).is_none());
    let xml = format!("<Document><Rpt>{}</Rpt></Document>", "<Node/>".repeat(250_001));
    assert!(camt::parse(xml.as_bytes()).is_err()); assert!(pain::read_pain001(xml.as_bytes()).is_none());
}

#[test]
fn bank_key_reader_rejects_oversize_zero_padded_and_invalid_keys() {
    let segment = |modulus: Vec<u8>, exponent: Vec<u8>| SegmentBuilder::new("HIISA", 3)
        .de("2").de("124").de("RDH").de(Val::Empty)
        .deg(vec!["280".into(), "12345678".into(), "bank".into(), "V".into(), "1".into(), "1".into()])
        .deg(vec!["6".into(), "18".into(), "1".into(), Val::Bin(modulus), "12".into(), Val::Bin(exponent), "13".into()]);
    for (modulus, exponent) in [(vec![0; 1026], vec![1, 0, 1]), (vec![0xff; 256], vec![1; 17]), (vec![], vec![3]), (vec![2; 256], vec![1, 0, 1]), (vec![0xff; 256], vec![0])] {
        let parsed = parse_segments(&segment(modulus, exponent).encode()).unwrap(); assert!(rdh::parse_hiisa(&parsed[0]).is_none());
    }
    let parsed = parse_segments(&segment(vec![0xff; 256], vec![1, 0, 1]).encode()).unwrap(); assert!(rdh::parse_hiisa(&parsed[0]).is_some());
}

struct Card;
impl DdvCard for Card {
    fn cid(&mut self) -> Result<Vec<u8>> { panic!("not needed") }
    fn bank_data(&mut self, _: u8) -> Result<DdvBankData> { panic!("not needed") }
    fn keys(&mut self) -> Result<(DdvKey, DdvKey)> { panic!("not needed") }
    fn sig_id(&mut self) -> Result<u32> { panic!("not needed") }
    fn set_sig_id(&mut self, _: u32) -> Result<()> { panic!("not needed") }
    fn verify_pin(&mut self) -> Result<()> { panic!("not needed") }
    fn sign(&mut self, _: &[u8; 20]) -> Result<[u8; 8]> { panic!("not needed") }
    fn session_keys(&mut self, _: u32) -> Result<([u8; 16], [u8; 16])> { panic!("not needed") }
    fn decrypt_key(&mut self, _: u32, _: &[u8]) -> Result<[u8; 16]> { Ok([1; 16]) }
}
fn envelope(segments: Vec<SegmentBuilder>) -> Vec<u8> {
    build_message(&Envelope { dialog_id: "TEST", message_number: 1, blz: "12345678", user_id: "test", system_id: "TEST" }, Auth::None, segments).unwrap()
}
#[test]
fn ddv_does_not_accept_plaintext_response_or_plaintext_business_segments() {
    let status = SegmentBuilder::new("HIRMG", 2).deg(vec!["0010".into(), Val::Empty, "fake success".into()]);
    assert!(open_message(&envelope(vec![status.clone()]), Opener::Ddv(&mut Card, 2)).is_err());
    let header = SegmentBuilder::new("HNVSK", 3).deg(vec!["DDV".into(), "1".into()]).de("4").de("1").de(Val::Empty).de(Val::Empty)
        .deg(vec!["2".into(), "2".into(), "13".into(), Val::Bin(vec![2; 16])]);
    let body = SegmentBuilder::new("HNVSD", 1).bin(crate::security::tdes_encrypt(&[1; 16], &crate::security::pad(status.encode())));
    assert!(open_message(&envelope(vec![header.clone(), body.clone()]), Opener::Ddv(&mut Card, 2)).is_ok());
    assert!(open_message(&envelope(vec![header, body, status]), Opener::Ddv(&mut Card, 2)).is_err());
}

#[test]
fn ddv_rejects_invalid_padding_instead_of_interpreting_truncated_data() {
    let header = SegmentBuilder::new("HNVSK", 3).deg(vec!["DDV".into(), "1".into()]).de("4").de("1").de(Val::Empty).de(Val::Empty)
        .deg(vec!["2".into(), "2".into(), "13".into(), Val::Bin(vec![2; 16])]);
    let mut padded = crate::security::pad(b"HIRMG:2:2+0010::OK'".to_vec());
    let last = padded.len() - 1; padded[last] = 3; padded[last - 1] = 99;
    let body = SegmentBuilder::new("HNVSD", 1).bin(crate::security::tdes_encrypt(&[1; 16], &padded));
    assert!(open_message(&envelope(vec![header, body]), Opener::Ddv(&mut Card, 2)).is_err());
}
