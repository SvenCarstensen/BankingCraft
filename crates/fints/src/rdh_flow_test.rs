//! End-to-end RDH-7 dialog against a simulated bank: anonymous key download (HKISA),
//! key check with the card hash, synchronisation without system id, signed and encrypted
//! dialog with system id, bank signature verification.

use crate::client::{Client, Config, Interaction, Medium, State};
use crate::model::*;
use crate::rdh::tests::{bank_key, SoftCard};
use crate::rdh::*;
use crate::security::{build_message, tdes_decrypt, Auth, Envelope};
use crate::syntax::{latin1_to_string, parse_segments, SegmentBuilder, Val};
use crate::transport::Transport;
use rsa::{Pkcs1v15Encrypt, PrivateKey};
use std::sync::{Arc, Mutex};

struct Ui;
impl Interaction for Ui {
    fn pin(&self, _: &str) -> Option<String> {
        None
    }
    fn choose_tan_method(&self, _: &[TanMethod]) -> Option<String> {
        None
    }
    fn choose_tan_medium(&self, _: &[String]) -> Option<String> {
        None
    }
    fn tan(&self, _: &TanChallenge) -> TanAnswer {
        TanAnswer::Cancel
    }
    fn progress(&self, _: &str) {}
    fn confirm(&self, title: &str, text: &str) -> bool {
        // the crypt key is verified by the card hash; the sign key (same number/version) is shown
        assert_eq!(title, "Bankschlüssel prüfen");
        assert!(text.contains("Signaturschlüssel") && !text.contains("Verschlüsselungsschlüssel"), "{text}");
        true
    }
}

/// The user's card with a notepad entry for the bank.
struct UserCard {
    inner: SoftCard,
    hash: BankKeyHash,
}

impl RdhCard for UserCard {
    fn cid(&mut self) -> crate::Result<Vec<u8>> {
        self.inner.cid()
    }
    fn verify_pin(&mut self) -> crate::Result<()> {
        Ok(())
    }
    fn contexts(&mut self) -> crate::Result<Vec<RdhContext>> {
        Ok(vec![RdhContext {
            record: 1,
            bank_name: "VR Bank Nord".into(),
            blz: "21763542".into(),
            host: "fints1.atruvia.de".into(),
            user_id: "9999999999000000001".into(),
            customer_id: "9999999999000000001".into(),
            profile: 7,
            sign_key: KeyRef { num: 7, version: 1 },
            crypt_key: KeyRef { num: 7, version: 1 },
            bank_key_hash: Some(self.hash.clone()),
            ..Default::default()
        }])
    }
    fn sign_key(&mut self) -> crate::Result<CardKey> {
        self.inner.sign_key()
    }
    fn decipher_key(&mut self) -> crate::Result<CardKey> {
        self.inner.decipher_key()
    }
    fn sign_counter(&mut self) -> crate::Result<u32> {
        self.inner.sign_counter()
    }
    fn sign(&mut self, h: &[u8; 32]) -> crate::Result<Vec<u8>> {
        self.inner.sign(h)
    }
    fn decipher(&mut self, d: &[u8]) -> crate::Result<Vec<u8>> {
        self.inner.decipher(d)
    }
}

struct Bank {
    crypt: PrivateKey,
    sign: SoftCard,
    user_sign: BankKey,
    user_crypt: BankKey,
    log: Arc<Mutex<Vec<String>>>,
    msg: u32,
}

impl Bank {
    fn reply(&mut self, segs: Vec<SegmentBuilder>, signed: bool) -> Vec<u8> {
        self.msg += 1;
        let env = Envelope { dialog_id: "DLG42", message_number: self.msg, blz: "21763542", user_id: "21763542", system_id: "SYS" };
        if !signed {
            return build_message(&env, Auth::None, segs).unwrap();
        }
        let session = RdhSession { cid: vec![], profile: 7, sign_key: CardKey { num: 7, version: 1, size: 256 }, ..Default::default() };
        let auth = Auth::Rdh { card: &mut self.sign, session: &session, bank: &self.user_crypt, no_sysid: false };
        build_message(&env, auth, segs).unwrap()
    }
}

fn ok() -> SegmentBuilder {
    SegmentBuilder::new("HIRMG", 2).deg(vec!["0010".into(), Val::Empty, "Nachricht entgegengenommen.".into()])
}

impl Transport for Bank {
    fn send(&mut self, message: &[u8]) -> crate::Result<Vec<u8>> {
        let outer = parse_segments(message).unwrap();
        self.msg = outer[0].text_or_empty(4, 0).parse::<u32>().unwrap() - 1;
        let text = latin1_to_string(message);
        if outer.iter().any(|s| s.code == "HKISA") {
            self.log.lock().unwrap().push("anon-keys".into());
            assert!(text.contains("HKISA:4:3+2+124+RDH:7+280:21763542:999:V:999:999'"), "{text}");
            let key = |kind: &str, k: &rsa::PublicKey| {
                let b = bank_key(kind, k);
                SegmentBuilder::new("HIISA", 3)
                    .de("1")
                    .de("X")
                    .de("1")
                    .de("224")
                    .deg(vec!["280".into(), "21763542".into(), "21763542".into(), kind.into(), "1".into(), "3".into()])
                    .deg(vec!["5".into(), "16".into(), "10".into(), Val::Bin(b.modulus), "12".into(), Val::Bin(b.exponent), "13".into()])
            };
            let segs = vec![ok(), key("V", &self.crypt.to_public_key()), key("S", &self.sign.sign_key.to_public_key())];
            return Ok(self.reply(segs, false));
        }
        if outer.iter().any(|s| s.code == "HKEND") && !outer.iter().any(|s| s.code == "HNVSK") {
            return Ok(self.reply(vec![ok()], false));
        }
        // signed and encrypted user message
        let hnvsk = outer.iter().find(|s| s.code == "HNVSK").expect("encrypted");
        assert_eq!(hnvsk.text(1, 0).as_deref(), Some("RDH"));
        assert_eq!(hnvsk.text(1, 1).as_deref(), Some("7"));
        assert_eq!(hnvsk.text(6, 1).as_deref(), Some("18"));
        assert_eq!(hnvsk.text(7, 2).as_deref(), Some("21763542"), "peer id from HIISA");
        let key = self.crypt.decrypt(Pkcs1v15Encrypt, hnvsk.bin(6, 3).unwrap()).unwrap();
        let mut k = [0u8; 16];
        k.copy_from_slice(&key);
        let hnvsd = outer.iter().find(|s| s.code == "HNVSD").unwrap();
        let mut plain = tdes_decrypt(&k, hnvsd.bin(1, 0).unwrap()).unwrap();
        let pl = *plain.last().unwrap() as usize;
        plain.truncate(plain.len() - pl);
        assert_eq!(verify(&plain, Some(&self.user_sign)).unwrap(), SignatureCheck::Valid, "user signature");
        let inner = parse_segments(&plain).unwrap();
        let shk = &inner[0];
        assert_eq!(shk.code, "HNSHK");
        let sysid = shk.text(6, 2).unwrap_or_default();
        let has_cid = shk.bin(6, 1).map(|b| !b.is_empty()).unwrap_or(false);
        let idn = inner.iter().find(|s| s.code == "HKIDN");
        if inner.iter().any(|s| s.code == "HKSYN") {
            assert_eq!(sysid, "0");
            assert!(!has_cid, "no CID without system id (AqBanking noSysId)");
            assert_eq!(idn.unwrap().text(4, 0).as_deref(), Some("1"));
            self.log.lock().unwrap().push("sync".into());
            let syn = SegmentBuilder::new("HISYN", 4).de("SYS777");
            return Ok(self.reply(vec![ok(), syn], true));
        }
        if let Some(idn) = idn {
            assert_eq!(idn.text(3, 0).as_deref(), Some("SYS777"));
            assert_eq!(sysid, "SYS777");
            assert!(has_cid);
            assert_eq!(shk.text(11, 3).as_deref(), Some("S"));
            assert_eq!(shk.text(11, 4).as_deref(), Some("7"));
            self.log.lock().unwrap().push(format!("init seq={}", shk.text_or_empty(7, 0)));
            return Ok(self.reply(vec![ok()], true));
        }
        self.log.lock().unwrap().push(format!("{} seq={}", inner[1].code, shk.text_or_empty(7, 0)));
        Ok(self.reply(vec![ok()], true))
    }
}

#[test]
fn rdh7_dialog_flow() {
    let mut rng = rand_rsa::rng();
    let bank_crypt = PrivateKey::new(&mut rng, 2048).unwrap();
    let bank_sign = SoftCard::new();
    let user = SoftCard::new();
    let crypt_pub = bank_key("V", &bank_crypt.to_public_key());
    let hash = BankKeyHash { key: KeyRef { num: 1, version: 3 }, sha256: true, hash: crypt_pub.hash(true) };
    let log = Arc::new(Mutex::new(Vec::new()));
    let bank = Bank {
        crypt: bank_crypt,
        user_sign: bank_key("S", &user.sign_key.to_public_key()),
        user_crypt: bank_key("V", &user.crypt_key.to_public_key()),
        sign: bank_sign,
        log: log.clone(),
        msg: 0,
    };
    let card = UserCard { inner: user, hash };
    let ui = Ui;
    let mut c = Client::new(Config::default(), State::default(), Medium::Rdh(Box::new(card), 0), &ui).unwrap();
    assert_eq!(c.cfg.blz, "21763542");
    assert_eq!(c.cfg.url, "fints1.atruvia.de");
    c.set_transport(Box::new(bank));
    c.open().unwrap();
    c.end_dialog().unwrap();
    let st = c.finish();
    assert_eq!(st.system_id, "SYS777");
    let keys = st.bank_keys.unwrap();
    assert!(keys.verified);
    assert!(keys.sign.is_some());
    let log = log.lock().unwrap().clone();
    assert_eq!(log, vec!["anon-keys", "sync", "HKEND seq=42", "init seq=43", "HKEND seq=44"], "{log:?}");
}
