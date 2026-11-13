
#[cfg(test)]
mod tests {
    use crate::rdh::*;
    use crate::security::{open_message, Opener};
    use crate::syntax::{SegmentBuilder, Val};
    use rsa::{PrivateKey, Pkcs1v15Encrypt, traits::PublicKeyParts};
    use sha2::{Digest, Sha256};
    use std::sync::OnceLock;

    // Offline probes use a disposable software key; no bank or card is contacted.
    fn private_key() -> PrivateKey {
        static KEY: OnceLock<PrivateKey> = OnceLock::new();
        KEY.get_or_init(|| PrivateKey::new(&mut rand_rsa::rng(), 2048).unwrap()).clone()
    }
    struct Card(PrivateKey);
    impl RdhCard for Card {
        fn cid(&mut self) -> crate::Result<Vec<u8>> { Ok(vec![1]) }
        fn verify_pin(&mut self) -> crate::Result<()> { Ok(()) }
        fn contexts(&mut self) -> crate::Result<Vec<RdhContext>> { Ok(vec![]) }
        fn sign_key(&mut self) -> crate::Result<CardKey> { Ok(CardKey {num:2,version:1,size:256}) }
        fn decipher_key(&mut self) -> crate::Result<CardKey> { Ok(CardKey {num:3,version:1,size:256}) }
        fn sign_counter(&mut self) -> crate::Result<u32> { Ok(1) }
        fn sign(&mut self, hash: &[u8;32]) -> crate::Result<Vec<u8>> {
            Ok(self.0.sign_with_rng(&mut rand_rsa::rng(), rsa::Pss::<rsa::sha2::Sha256>::new(), &Sha256::digest(hash)).unwrap())
        }
        fn decipher(&mut self, data: &[u8]) -> crate::Result<Vec<u8>> {
            Ok(self.0.decrypt(Pkcs1v15Encrypt, data).unwrap())
        }
    }
    fn setup() -> (Card, BankKey, RdhSession) {
        let card=Card(private_key());
        let public=card.0.to_public_key();
        let key=BankKey { kind:"S".into(),user_id:"bank".into(),num:1,version:1,
            modulus:public.n_bytes().into_vec(),exponent:public.e_bytes().into_vec() };
        let session=RdhSession { profile:7,sign_key:CardKey {num:2,version:1,size:256}, ..Default::default() };
        (card,key,session)
    }
    fn body() -> SegmentBuilder {
        let mut s=SegmentBuilder::new("HIRMG",2).deg(vec!["0010".into(),Val::Empty,"OK".into()]);
        s.number=3;
        s
    }
    #[test]
    fn rejects_plain_unsigned_response_with_known_bank_key() {
        let (mut card,key,_)=setup();
        assert!(open_message(&body().encode(),Opener::Rdh(&mut card,Some(&key))).is_err());
    }
    #[test]
    fn unencrypted_rdh_rejection_is_diagnostic_only_and_never_accepts_data() {
        let (mut card, key, _) = setup();
        let rejection = SegmentBuilder::new("HIRMG", 2)
            .deg(vec!["9800".into(), Val::Empty, "Dialog abgebrochen".into()])
            .deg(vec!["0010".into(), Val::Empty, "untrusted success".into()]);
        let raw = crate::security::build_message(&crate::security::Envelope {
            dialog_id: "0", message_number: 1, blz: "12345678", user_id: "test", system_id: "0",
        }, crate::security::Auth::None, vec![rejection]).unwrap();
        let error = open_message(&raw, Opener::Rdh(&mut card, Some(&key))).unwrap_err();
        assert!(matches!(&error, crate::Error::Security(_)), "must not become a trusted bank result");
        let text = error.to_string();
        assert!(text.contains("Nicht authentifizierte Fehlermeldung"));
        assert!(text.contains("9800 Dialog abgebrochen"));
        assert!(!text.contains("untrusted success"));

        let mut injected = raw;
        injected.extend(SegmentBuilder::new("HISYN", 4).de("UNTRUSTED-SYSTEM-ID").encode());
        let error = open_message(&injected, Opener::Rdh(&mut card, Some(&key))).unwrap_err();
        assert!(matches!(&error, crate::Error::Security(_)));
        assert!(!error.to_string().contains("9800"), "mixed data must not be interpreted");
    }
    #[test]
    fn unencrypted_rdh_diagnostics_are_bounded_and_strip_control_characters() {
        let (mut card, key, _) = setup();
        let status = SegmentBuilder::new("HIRMG", 2)
            .deg(vec!["9800".into(), Val::Empty, format!("\u{1b}\r\n{}", "X".repeat(600)).into()]);
        let raw: Vec<_> = (0..20).flat_map(|_| status.encode()).collect();
        let error = open_message(&raw, Opener::Rdh(&mut card, Some(&key))).unwrap_err().to_string();
        assert_eq!(error.matches("9800").count(), 8);
        assert!(error.len() < 4400);
        assert!(!error.contains('\u{1b}') && !error.contains('\r'));
        assert!(!error.contains(&"X".repeat(513)));
    }
    #[test]
    fn rejects_encrypted_unsigned_response_with_known_bank_key() {
        let (mut card,key,session)=setup();
        let encrypted=encrypt(&session,&key,"12345678","bank","SYS",false,body().encode()).unwrap();
        assert!(open_message(&encrypted,Opener::Rdh(&mut card,Some(&key))).is_err());
    }
    #[test]
    fn rejects_invalid_signature_in_cleartext() {
        let (mut card,key,session)=setup();
        let b=body();
        let signed=sign(&mut card,&session,"12345678","bank","SYS",false,std::slice::from_ref(&b),4).unwrap();
        let mut raw=signed.hnshk.encode();
        raw.extend(b.encode());
        let mut signature=signed.hnsha.encode();
        let last=signature.len()-2;
        signature[last]^=1;
        raw.extend(signature);
        assert!(verify(&raw,Some(&key)).is_err(),"control: signature is invalid");
        assert!(open_message(&raw,Opener::Rdh(&mut card,Some(&key))).is_err());
    }
    #[test]
    fn rejects_mismatched_signature_control_reference() {
        let (mut card,key,session)=setup();
        let b=body();
        let signed=sign(&mut card,&session,"12345678","bank","SYS",false,std::slice::from_ref(&b),4).unwrap();
        let mut raw=signed.hnshk.encode();
        raw.extend(b.encode());
        let tail=crate::syntax::parse_segments(&signed.hnsha.encode()).unwrap();
        let mut altered=SegmentBuilder::new("HNSHA",2).de("DIFFERENT").bin(tail[0].bin(2,0).unwrap().to_vec());
        altered.number=4;
        raw.extend(altered.encode());
        assert!(verify(&raw,Some(&key)).is_err());
    }
    #[test]
    fn rejects_encrypted_signed_response_without_bank_sign_key() {
        let (mut card,key,session)=setup();
        let b=body();
        let signed=sign(&mut card,&session,"12345678","bank","SYS",false,std::slice::from_ref(&b),4).unwrap();
        let mut raw=signed.hnshk.encode(); raw.extend(b.encode()); raw.extend(signed.hnsha.encode());
        let encrypted=encrypt(&session,&key,"12345678","bank","SYS",false,raw).unwrap();
        assert!(open_message(&encrypted,Opener::Rdh(&mut card,None)).is_err());
    }

    #[test]
    fn accepts_valid_encrypted_signed_response_but_rejects_unsigned_additions() {
        let (mut card,key,session)=setup();
        let b=body();
        let signed=sign(&mut card,&session,"12345678","bank","SYS",false,std::slice::from_ref(&b),4).unwrap();
        let mut raw=signed.hnshk.encode(); raw.extend(b.encode()); raw.extend(signed.hnsha.encode());
        let encrypted=encrypt(&session,&key,"12345678","bank","SYS",false,raw.clone()).unwrap();
        assert!(open_message(&encrypted,Opener::Rdh(&mut card,Some(&key))).is_ok());
        let mut injected=encrypted;
        injected.extend(b.encode());
        assert!(open_message(&injected,Opener::Rdh(&mut card,Some(&key))).is_err());
        raw.extend(b.encode());
        assert!(verify(&raw,Some(&key)).is_err());
    }

    #[test]
    fn overflowing_binary_length_returns_error_instead_of_panicking() {
        let msg=format!("HNVSD:999:1+@{}@x'",usize::MAX);
        assert!(crate::syntax::parse_segments(msg.as_bytes()).is_err());
    }

    #[test]
    fn malformed_mt940_is_safe() {
        for data in [b":20:X\n:60F:\n".as_slice(), b":20:X\n:61:261112D12,00\n:86:\xe4\xf6\xfcX\n", b":20:X\n:61:261112\xe4\xf6\xfcD12,00\n"] {
            crate::mt940::parse(data,false);
        }
    }

    #[test]
    fn camt_rejects_bad_amounts_and_unicode_dates() {
        let xml=|amt:&str,date:&str|format!("<Document><Rpt><Ntry><Amt Ccy=\"EUR\">{amt}</Amt><CdtDbtInd>DBIT</CdtDbtInd><BookgDt><Dt>{date}</Dt></BookgDt></Ntry></Rpt></Document>");
        assert!(crate::camt::parse(xml("not-money","2026-11-12").as_bytes()).is_err());
        assert!(crate::camt::parse(xml("12.34","123456789ä").as_bytes()).is_err());
        assert!(crate::camt::parse(b"<Document/>").is_err());
        let good=crate::camt::parse(xml("12.34","2026-11-12").as_bytes()).unwrap();
        assert_eq!(good.transactions[0].amount.to_string(),"-12.34");
    }

    #[test]
    fn camt_missing_dates_are_retained_without_fabricating_a_booking_date() {
        let xml = b"<Document><Rpt><CreDtTm>2026-11-12T12:00:00+01:00</CreDtTm><Ntry><Amt Ccy=\"EUR\">12.34</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>PDNG</Cd></Sts></Ntry><Ntry><Amt Ccy=\"EUR\">1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><DtTm>2026-11-11T14:00:00Z</DtTm></BookgDt></Ntry></Rpt></Document>";
        let report = crate::camt::parse(xml).unwrap();
        assert_eq!(report.transactions.len(), 2);
        assert!(report.transactions[0].pending && report.transactions[0].date_unknown);
        assert!(!report.transactions[1].pending && !report.transactions[1].date_unknown);
        assert_eq!(report.transactions[1].booking_date.to_string(), "2026-11-11");
        let pending = b"<Document><Rpt><Ntry><Amt Ccy=\"EUR\">1</Amt><CdtDbtInd>DBIT</CdtDbtInd></Ntry></Rpt></Document>";
        let report = crate::camt::parse_pending(pending).unwrap();
        assert!(report.transactions[0].pending && report.transactions[0].date_unknown);
    }

    fn transfer() -> crate::SepaTransfer {
        crate::SepaTransfer {debtor_name:"Sender".into(),debtor_iban:"DE89370400440532013000".into(),debtor_bic:String::new(),creditor_name:"Receiver".into(),creditor_iban:"DE89370400440532013000".into(),creditor_bic:String::new(),amount:"12.34".parse().unwrap(),purpose:"Test".into(),end_to_end_id:String::new(),execution_date:None,instant:false}
    }

    #[test]
    fn rejects_payment_rounding_and_transliterated_overflow() {
        let mut t=transfer();
        assert!(crate::pain::validate_transfer(&t).is_ok());
        t.amount="12.345".parse().unwrap();
        assert!(crate::pain::validate_transfer(&t).is_err());
        t.amount="12.34".parse().unwrap();
        t.creditor_name="ü".repeat(36);
        assert!(crate::pain::validate_transfer(&t).is_err());
    }

    #[test]
    fn generated_sepa_ids_fit_schema_limits() {
        let xml=crate::pain::build_pain001("pain.001.001.09",&transfer(),None);
        let text=String::from_utf8(xml).unwrap();
        let doc=roxmltree::Document::parse(&text).unwrap();
        for n in doc.descendants().filter(|n|n.is_element() && matches!(n.tag_name().name(),"MsgId"|"PmtInfId")) {
            assert!(n.text().unwrap().len()<=35);
        }
    }

    #[test]
    fn invalid_standing_schedule_is_rejected() {
        let today=chrono::Local::now().date_naive();
        let mut so=crate::StandingOrder { order_id:String::new(),execution_time:None,transfer:transfer(),first_date:today,last_date:None,interval:crate::Interval::Weekly,every:1,day:8,raw_pain:vec![],descriptor:String::new() };
        assert!(crate::pain::validate_standing_schedule(&so).is_err());
        so.day=3;
        assert!(crate::pain::validate_standing_schedule(&so).is_ok());
        so.last_date=Some(today-chrono::Duration::days(1));
        assert!(crate::pain::validate_standing_schedule(&so).is_err());
    }

    #[test]
    fn rejects_incomplete_and_wrong_dialog_responses() {
        use crate::security::{build_message, Auth, Envelope};
        let env=Envelope {dialog_id:"DLG",message_number:2,blz:"12345678",user_id:"user",system_id:"SYS"};
        let raw=build_message(&env,Auth::None,vec![body()]).unwrap();
        assert!(crate::response::validate_envelope(&raw,"DLG",2).is_ok());
        assert!(crate::response::validate_envelope(&raw,"OTHER",2).is_err());
        assert!(crate::response::validate_envelope(&raw,"DLG",3).is_err());
        assert!(crate::response::validate_envelope(&raw[..raw.len()-1],"DLG",2).is_err());
        assert!(crate::response::validate_envelope(b"HIRMG:2:2+0010::OK'","0",1).is_err());
    }

    #[test]
    fn malformed_flicker_markers_do_not_panic() {
        assert!(crate::tan::FlickerCode::parse("CHLGUCCHLGTEXT").is_none());
        assert!(crate::tan::FlickerCode::parse("CHLGUC12äCHLGTEXT").is_none());
    }
}
