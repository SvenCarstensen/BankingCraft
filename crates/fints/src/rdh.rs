//! RDH security (RSA-DES-Hybrid) with a bank chip card (ZKA/SECCOS "RDH card", e.g. the
//! VR-NetWorld-Card), profiles RDH-7/RDH-9/RDH-10.
//!
//! The message layout follows AqBanking's `msgcrypt_rxh_*` and libchipcard's `zkacard` token
//! (behaviour only, no code taken over):
//!
//! * HNSHK `RDH:<n>`, function 2 (signature key), control reference `YYYYMMDDhhmmss`,
//!   security details `1:<CID>:<system id>` (only `1::0` while no system id exists),
//!   signature counter from the card, hash algorithm `1:6:1` (SHA-256/SHA-256),
//!   signature algorithm `6:10:19` (RSA, PSS), key `280:<blz>:<user>:S:<profile>:<version>`.
//! * The host computes SHA-256 over HNSHK + segments once; the card applies the second
//!   SHA-256 round and the PSS padding (MSE `80 01 46`).
//! * HNVSK function 4, `2:<mode>:13:<encrypted key>:6:1` – 2-key-3DES-CBC message key,
//!   wrapped with the bank's public encryption key (PKCS#1 v1.5 for RDH-7/9, left zero padded
//!   raw RSA for RDH-10); message padded with ANSI X9.23.
//! * Bank keys are fetched in an anonymous dialog (HKISA v3) and checked against the hash on
//!   the card (SHA-256 over exponent and modulus, both left padded to the key size).
//! * Response signatures of the bank are verified with its signature key (PSS, SHA-256).

use crate::error::{Error, Result};
use crate::syntax::{segment_spans, Segment, SegmentBuilder, Val};
use rsa::traits::PublicKeyParts;
use rsa::{BoxedUint, PublicKey};
#[cfg(test)]
use rsa::Pkcs1v15Encrypt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Number and version of a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRef {
    pub num: u32,
    pub version: u32,
}

/// Hash of a bank key as stored on the card (from the bank's INI letter).
#[derive(Debug, Clone, Default)]
pub struct BankKeyHash {
    pub key: KeyRef,
    /// `true` = SHA-256, `false` = RIPEMD-160
    pub sha256: bool,
    pub hash: Vec<u8>,
}

/// One bank access stored in the card's notepad (EF_NOTEPAD).
#[derive(Debug, Clone, Default)]
pub struct RdhContext {
    pub record: u8,
    pub bank_name: String,
    pub country: String,
    pub blz: String,
    pub host: String,
    pub user_id: String,
    pub customer_id: String,
    /// RDH profile version (7, 9, 10 …). On ZKA cards this is the sign key number in the notepad.
    pub profile: u32,
    pub sign_key: KeyRef,
    pub crypt_key: KeyRef,
    pub bank_key_hash: Option<BankKeyHash>,
}

/// User key data on the card.
#[derive(Debug, Clone, Copy, Default)]
pub struct CardKey {
    pub num: u32,
    pub version: u32,
    /// modulus size in bytes
    pub size: usize,
}

/// Operations an RDH chip card must provide. Implemented by the `cardreader` crate via PC/SC.
pub trait RdhCard: Send {
    /// Card id (raw EF_ID record) for the security details of HNSHK/HNVSK.
    fn cid(&mut self) -> Result<Vec<u8>>;
    /// Verify the card holder PIN (on the reader's pin pad when available).
    fn verify_pin(&mut self) -> Result<()>;
    /// Bank accesses in the notepad (requires the PIN).
    fn contexts(&mut self) -> Result<Vec<RdhContext>>;
    /// The user's signature key (ZKA: key #2 in EF_KEYD).
    fn sign_key(&mut self) -> Result<CardKey>;
    /// The user's decipher key (ZKA: key #3 in EF_KEYD).
    fn decipher_key(&mut self) -> Result<CardKey>;
    /// Current signature counter (read fresh before every signature).
    fn sign_counter(&mut self) -> Result<u32>;
    /// Sign a SHA-256 hash; the card computes the second hash round and the PSS padding.
    fn sign(&mut self, hash: &[u8; 32]) -> Result<Vec<u8>>;
    /// Decipher a block with the user's decipher key (returns the unpadded plaintext or the
    /// raw block – callers take the trailing key bytes, as AqBanking does).
    fn decipher(&mut self, data: &[u8]) -> Result<Vec<u8>>;
}

/// A public key of the bank (from HIISA).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankKey {
    /// "V" = encryption, "S" = signature, "D" = authentication
    pub kind: String,
    /// Benutzerkennung in the key name (the bank's "peer id").
    pub user_id: String,
    pub num: u32,
    pub version: u32,
    pub modulus: Vec<u8>,
    pub exponent: Vec<u8>,
}

impl BankKey {
    pub fn size(&self) -> usize {
        self.modulus.len()
    }

    fn rsa(&self) -> Result<PublicKey> {
        PublicKey::new_with_size_limits(
            BoxedUint::from_be_slice_vartime(&self.modulus),
            BoxedUint::from_be_slice_vartime(&self.exponent),
            1024..=8192,
        )
        .map_err(|e| Error::Security(format!("ungültiger Bankschlüssel: {e}")))
    }

    /// Hash as printed on the INI letter / stored on the card: exponent and modulus, each left
    /// padded with zeros to the key size.
    pub fn hash(&self, sha256: bool) -> Vec<u8> {
        let size = self.modulus.len().max(self.exponent.len());
        let mut buf = vec![0u8; 2 * size];
        buf[size - self.exponent.len()..size].copy_from_slice(&self.exponent);
        buf[2 * size - self.modulus.len()..].copy_from_slice(&self.modulus);
        if sha256 {
            Sha256::digest(&buf).to_vec()
        } else {
            use ripemd::Ripemd160;
            Ripemd160::digest(&buf).to_vec()
        }
    }

    /// Hash in the usual INI letter notation (hex pairs, 16 per line).
    pub fn fingerprint(&self) -> String {
        hex_blocks(&self.hash(true))
    }
}

pub fn hex_blocks(b: &[u8]) -> String {
    b.chunks(16)
        .map(|c| c.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bank keys persisted with the access.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankKeys {
    pub crypt: Option<BankKey>,
    pub sign: Option<BankKey>,
    /// Keys were verified against the card hash or confirmed by the user.
    pub verified: bool,
}

/// Result of checking a bank key against the hash on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCheck {
    /// Hash on the card matches.
    Verified,
    /// Hash on the card refers to this key but does not match – never use the key.
    Mismatch,
    /// No hash for this key on the card – the user has to compare with the INI letter.
    Unknown,
}

pub fn check_bank_key(key: &BankKey, card_hash: Option<&BankKeyHash>) -> KeyCheck {
    match card_hash {
        Some(h) if h.key.num == key.num && h.key.version == key.version => {
            let mine = key.hash(h.sha256);
            // version 1 notepads store RIPEMD-160 hashes right aligned in 32 bytes
            let theirs = if !h.sha256 && h.hash.len() == 32 { &h.hash[12..] } else { &h.hash[..] };
            if mine == theirs { KeyCheck::Verified } else { KeyCheck::Mismatch }
        }
        _ => KeyCheck::Unknown,
    }
}

/// HKISA v3 request for one public bank key (anonymous dialog).
pub fn hkisa(profile: u32, blz: &str, kind: &str) -> SegmentBuilder {
    SegmentBuilder::new("HKISA", 3)
        .de("2") // message context: request
        .de("124") // function: public key request
        .deg(vec!["RDH".into(), profile.into()])
        .deg(vec!["280".into(), blz.into(), "999".into(), kind.into(), "999".into(), "999".into()])
}

/// Parse HIISA (v2/v3): key name in DE 5, key in DE 6 (`purpose:opmode:type:modulus:12:exponent:13`).
pub fn parse_hiisa(seg: &Segment) -> Option<BankKey> {
    let kind = seg.text(5, 3)?;
    let modulus = seg.bin(6, 3)?;
    let exponent = seg.bin(6, 5).unwrap_or(&[1, 0, 1]);
    if !matches!(kind.as_str(), "S" | "V" | "D") || modulus.len() > 1025 || exponent.len() > 16 { return None; }
    let modulus = modulus.iter().copied().skip_while(|b| *b == 0).collect::<Vec<_>>();
    let exponent = exponent.iter().copied().skip_while(|b| *b == 0).collect::<Vec<_>>();
    let key = BankKey {
        kind,
        user_id: seg.text_or_empty(5, 2),
        num: seg.text(5, 4).and_then(|s| s.parse().ok()).unwrap_or(0),
        version: seg.text(5, 5).and_then(|s| s.parse().ok()).unwrap_or(0),
        modulus,
        exponent,
    };
    key.rsa().ok()?;
    Some(key)
}

/// Static data of an RDH session (read once from the card).
#[derive(Debug, Clone, Default)]
pub struct RdhSession {
    pub cid: Vec<u8>,
    pub profile: u32,
    pub sign_key: CardKey,
    pub decipher_key: CardKey,
}

fn now_parts() -> (String, String, String) {
    let now = chrono::Local::now();
    (now.format("%Y%m%d%H%M%S").to_string(), now.format("%Y%m%d").to_string(), now.format("%H%M%S").to_string())
}

/// Security details: `1:<CID>:<system id>`, or `1::0` for messages without system id.
fn sec_details(s: &RdhSession, system_id: &str, no_sysid: bool) -> Vec<Val> {
    if no_sysid || system_id.is_empty() || system_id == "0" {
        vec!["1".into(), Val::Empty, "0".into()]
    } else if s.cid.is_empty() {
        vec!["1".into(), Val::Empty, system_id.into()]
    } else {
        vec!["1".into(), Val::Bin(s.cid.clone()), system_id.into()]
    }
}

/// Mode of the message key wrapping in HNVSK (AqBanking `opmodCrypt`).
fn crypt_mode(profile: u32) -> u32 {
    if profile == 10 { 2 } else { 18 }
}

pub struct Signed {
    pub hnshk: SegmentBuilder,
    pub hnsha: SegmentBuilder,
}

/// Build HNSHK and HNSHA around `segments` (numbers already assigned).
#[allow(clippy::too_many_arguments)]
pub fn sign(
    card: &mut dyn RdhCard,
    s: &RdhSession,
    blz: &str,
    user_id: &str,
    system_id: &str,
    no_sysid: bool,
    segments: &[SegmentBuilder],
    hnsha_number: u32,
) -> Result<Signed> {
    card.verify_pin()?;
    let counter = card.sign_counter()?;
    let (ctrlref, date, time) = now_parts();
    let mut hnshk = SegmentBuilder::new("HNSHK", 4)
        .deg(vec!["RDH".into(), s.profile.into()])
        .de("2") // signature with the signature key
        .de(ctrlref.as_str())
        .de("1") // area: signature head and data
        .de("1") // role: issuer
        .deg(sec_details(s, system_id, no_sysid))
        .de(counter)
        .deg(vec!["1".into(), date.into(), time.into()])
        .deg(vec!["1".into(), "6".into(), "1".into()]) // SHA-256/SHA-256
        .deg(vec!["6".into(), "10".into(), "19".into()]) // RSA, PSS
        .deg(vec!["280".into(), blz.into(), user_id.into(), "S".into(), s.profile.into(), s.sign_key.version.into()]);
    hnshk.number = 2;
    let mut data = hnshk.encode();
    for seg in segments {
        data.extend(seg.encode());
    }
    let hash: [u8; 32] = Sha256::digest(&data).into();
    let signature = card.sign(&hash)?;
    if signature.is_empty() {
        return Err(Error::Security("Die Chipkarte hat keine Signatur geliefert".into()));
    }
    let mut hnsha = SegmentBuilder::new("HNSHA", 2).de(ctrlref.as_str()).bin(signature);
    hnsha.number = hnsha_number;
    Ok(Signed { hnshk, hnsha })
}

/// Encrypt the signed plaintext: returns HNVSK + HNVSD.
pub fn encrypt(s: &RdhSession, bank: &BankKey, blz: &str, user_id: &str, system_id: &str, no_sysid: bool, plain: Vec<u8>) -> Result<Vec<u8>> {
    use rand::RngCore;
    let mut key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut key);
    let crypted = crate::security::tdes_encrypt(&key, &crate::security::pad(plain));
    let pk = bank.rsa()?;
    let k = pk.size();
    let enc_key = if crypt_mode(s.profile) == 18 {
        pk.pkcs1v15_encrypt(&key)
            .map_err(|e| Error::Security(format!("Verschlüsselung des Nachrichtenschlüssels: {e}")))?
    } else {
        raw_rsa(&pk, &key, k)?
    };
    let (_, date, time) = now_parts();
    let peer = if bank.user_id.is_empty() || bank.user_id == "999" { user_id.to_string() } else { bank.user_id.clone() };
    let mut hnvsk = SegmentBuilder::new("HNVSK", 3)
        .deg(vec!["RDH".into(), s.profile.into()])
        .de("4") // encryption
        .de("1") // role: issuer
        .deg(sec_details(s, system_id, no_sysid))
        .deg(vec!["1".into(), date.into(), time.into()])
        .deg(vec![
            "2".into(),
            crypt_mode(s.profile).into(),
            "13".into(),
            Val::Bin(enc_key),
            "6".into(),
            "1".into(),
        ])
        .deg(vec!["280".into(), blz.into(), peer.into(), "V".into(), bank.num.into(), bank.version.into()])
        .de("0");
    hnvsk.number = 998;
    let mut hnvsd = SegmentBuilder::new("HNVSD", 1).bin(crypted);
    hnvsd.number = 999;
    let mut out = hnvsk.encode();
    out.extend(hnvsd.encode());
    Ok(out)
}

/// Raw RSA with left zero padding (RDH-10 key wrapping).
fn raw_rsa(pk: &PublicKey, data: &[u8], k: usize) -> Result<Vec<u8>> {
    // RDH-10 defines mode 2 as the raw public RSA operation. Keep it confined
    // to this protocol-specific message-key transport; never use it for data.
    let m = BoxedUint::from_be_slice(data, pk.n_bits_precision())
        .map_err(|_| Error::Security("Ungültiger RDH-Nachrichtenschlüssel".into()))?;
    if m >= pk.n().as_ref() || k != pk.size() {
        return Err(Error::Security("Ungültiger RDH-Nachrichtenschlüssel".into()));
    }
    let c = rsa::hazmat::rsa_encrypt(pk, &m)
        .map_err(|_| Error::Security("RDH-Schlüsseltransport fehlgeschlagen".into()))?.to_be_bytes();
    let mut out = vec![0u8; k.saturating_sub(c.len())];
    out.extend(c);
    Ok(out)
}

/// Decrypt the HNVSD payload with the message key from HNVSK (deciphered by the card).
pub fn decrypt(card: &mut dyn RdhCard, encrypted_key: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let size = card.decipher_key()?.size;
    if !(128..=1024).contains(&size) || encrypted_key.is_empty() || encrypted_key.len() > size
        || payload.is_empty() || !payload.len().is_multiple_of(8) {
        return Err(Error::Security("Ungültige Größe der verschlüsselten RDH-Antwort".into()));
    }
    card.verify_pin()?;
    let mut block = vec![0u8; size - encrypted_key.len()];
    block.extend_from_slice(encrypted_key);
    let plain = card.decipher(&block)?;
    if plain.len() < 16 {
        return Err(Error::Security("Nachrichtenschlüssel der Bank zu kurz".into()));
    }
    let mut key = [0u8; 16];
    key.copy_from_slice(&plain[plain.len() - 16..]);
    let mut p = crate::security::tdes_decrypt(&key, payload)?;
    let pl = *p.last().unwrap_or(&0) as usize;
    if pl == 0 || pl > 8 || pl > p.len() || p[p.len() - pl..p.len() - 1].iter().any(|b| *b != 0) {
        return Err(Error::Security("Antwort der Bank ließ sich nicht entschlüsseln (Padding)".into()));
    }
    p.truncate(p.len() - pl);
    Ok(p)
}

/// Outcome of the signature check of a bank message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureCheck {
    NotSigned,
    NoKey,
    Valid,
}

/// Verify the bank's signature(s) in a decrypted message body.
pub fn verify(plain: &[u8], key: Option<&BankKey>) -> Result<SignatureCheck> {
    let spans = segment_spans(plain)?;
    let heads: Vec<_> = spans.iter().filter(|s| s.2 == "HNSHK").collect();
    if heads.is_empty() {
        return Ok(SignatureCheck::NotSigned);
    }
    let tails: Vec<_> = spans.iter().filter(|s| s.2 == "HNSHA").collect();
    if heads.len() != tails.len()
        || spans.iter().take(heads.len()).any(|s| s.2 != "HNSHK")
        || spans.iter().rev().take(tails.len()).any(|s| s.2 != "HNSHA")
    {
        return Err(Error::Security("Signatur der Bank: unvollständige oder ungeschützte Segmentfolge".into()));
    }
    let Some(key) = key else { return Ok(SignatureCheck::NoKey) };
    let data_start = heads.last().map(|h| h.1).unwrap_or(0);
    let data_end = tails.first().map(|t| t.0).ok_or_else(|| Error::Security("Signaturabschluss der Bank fehlt".into()))?;
    if data_end < data_start {
        return Err(Error::Security("Signatur der Bank: Segmentfolge ungültig".into()));
    }
    let pk = key.rsa()?;
    // signature heads are nested: the innermost head belongs to the first tail
    for (k, head) in heads.iter().rev().enumerate() {
        let tail = tails.get(k).ok_or_else(|| Error::Security("Signaturabschluss der Bank fehlt".into()))?;
        let tail_seg = crate::syntax::parse_segments(&plain[tail.0..tail.1])?;
        let head_seg = crate::syntax::parse_segments(&plain[head.0..head.1])?;
        let hs = &head_seg[0];
        if hs.text(3, 0).is_none() || hs.text(3, 0) != tail_seg[0].text(1, 0) {
            return Err(Error::Security("Kontrollreferenzen der Banksignatur stimmen nicht überein".into()));
        }
        if hs.text_or_empty(1, 0) != "RDH" || !matches!(hs.text_or_empty(1, 1).as_str(), "7" | "9" | "10")
            || hs.text_or_empty(9, 1) != "6" || hs.text_or_empty(10, 1) != "10" || hs.text_or_empty(10, 2) != "19"
        {
            return Err(Error::Security("Unerwartetes Verfahren der Banksignatur".into()));
        }
        let sig = tail_seg
            .first()
            .and_then(|s| s.bin(2, 0))
            .ok_or_else(|| Error::Security("Signatur der Bank fehlt".into()))?;
        let mut h = Sha256::new();
        h.update(&plain[head.0..head.1]);
        h.update(&plain[data_start..data_end]);
        let first: [u8; 32] = h.finalize().into();
        let digest: [u8; 32] = Sha256::digest(first).into();
        pk.verify(rsa::Pss::<rsa::sha2::Sha256>::new(), &digest, sig)
            .map_err(|_| Error::Security("Die Signatur der Bank ist ungültig – Antwort verworfen".into()))?;
    }
    Ok(SignatureCheck::Valid)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rsa::traits::PublicKeyParts;
    use rsa::PrivateKey;

    #[test]
    fn rdh10_public_rsa_matches_independent_modular_exponentiation_vector() {
        // Expected value computed independently with Python pow(m, 65537, n).
        let key = BankKey { modulus: vec![0xff; 256], exponent: vec![1, 0, 1], ..Default::default() }.rsa().unwrap();
        let result = raw_rsa(&key, &(1..=32).collect::<Vec<u8>>(), 256).unwrap();
        assert_eq!(result.len(), 256);
        assert_eq!(format!("{:x}", Sha256::digest(&result)), "d1081169b8f5596ba6b87fd3adc1148d11067d23c7a441de8b8dd56f7a3b844f");
        assert!(raw_rsa(&key, &[0xff; 256], 256).is_err());
    }

    /// Software stand-in for the card (signs like the ZKA card: second SHA-256 round + PSS).
    pub struct SoftCard {
        pub sign_key: PrivateKey,
        pub crypt_key: PrivateKey,
        pub counter: u32,
    }

    impl SoftCard {
        pub fn new() -> Self {
            let mut rng = rand_rsa::rng();
            Self {
                sign_key: PrivateKey::new(&mut rng, 2048).unwrap(),
                crypt_key: PrivateKey::new(&mut rng, 2048).unwrap(),
                counter: 41,
            }
        }
    }

    impl RdhCard for SoftCard {
        fn cid(&mut self) -> Result<Vec<u8>> {
            Ok(vec![0x67, 0x21, 0x76, 0x35, 0x42, 0x28, 0x00, 0x55, 0x75, 0x34, 0x07])
        }
        fn verify_pin(&mut self) -> Result<()> {
            Ok(())
        }
        fn contexts(&mut self) -> Result<Vec<RdhContext>> {
            Ok(vec![])
        }
        fn sign_key(&mut self) -> Result<CardKey> {
            Ok(CardKey { num: 2, version: 1, size: 256 })
        }
        fn decipher_key(&mut self) -> Result<CardKey> {
            Ok(CardKey { num: 3, version: 1, size: 256 })
        }
        fn sign_counter(&mut self) -> Result<u32> {
            Ok(self.counter)
        }
        fn sign(&mut self, hash: &[u8; 32]) -> Result<Vec<u8>> {
            self.counter += 1;
            let d: [u8; 32] = Sha256::digest(hash).into();
            Ok(self.sign_key.sign_with_rng(&mut rand_rsa::rng(), rsa::Pss::<rsa::sha2::Sha256>::new(), &d).unwrap())
        }
        fn decipher(&mut self, data: &[u8]) -> Result<Vec<u8>> {
            Ok(self.crypt_key.decrypt(Pkcs1v15Encrypt, data).unwrap())
        }
    }

    pub fn bank_key(kind: &str, k: &rsa::PublicKey) -> BankKey {
        BankKey {
            kind: kind.into(),
            user_id: "21763542".into(),
            num: 1,
            version: 3,
            modulus: k.n_bytes().into_vec(),
            exponent: k.e_bytes().into_vec(),
        }
    }

    #[test]
    fn hiisa_roundtrip_and_hash() {
        let mut rng = rand_rsa::rng();
        let k = PrivateKey::new(&mut rng, 2048).unwrap().to_public_key();
        let mut seg = SegmentBuilder::new("HIISA", 3)
            .de("1")
            .de("ABC")
            .de("1")
            .de("224")
            .deg(vec!["280".into(), "21763542".into(), "21763542".into(), "V".into(), "1".into(), "3".into()])
            .deg(vec![
                "5".into(),
                "16".into(),
                "10".into(),
                Val::Bin([vec![0u8], k.n_bytes().into_vec()].concat()),
                "12".into(),
                Val::Bin(k.e_bytes().into_vec()),
                "13".into(),
            ]);
        seg.number = 4;
        let parsed = crate::syntax::parse_segments(&seg.encode()).unwrap();
        let key = parse_hiisa(&parsed[0]).unwrap();
        assert_eq!(key.kind, "V");
        assert_eq!((key.num, key.version), (1, 3));
        assert_eq!(key.modulus.len(), 256);
        assert_eq!(key.exponent, vec![1, 0, 1]);
        let h = BankKeyHash { key: KeyRef { num: 1, version: 3 }, sha256: true, hash: key.hash(true) };
        assert_eq!(check_bank_key(&key, Some(&h)), KeyCheck::Verified);
        let mut bad = h.clone();
        bad.hash[0] ^= 1;
        assert_eq!(check_bank_key(&key, Some(&bad)), KeyCheck::Mismatch);
        let other = BankKeyHash { key: KeyRef { num: 2, version: 1 }, ..h };
        assert_eq!(check_bank_key(&key, Some(&other)), KeyCheck::Unknown);
    }

    #[test]
    fn sign_encrypt_decrypt_verify() {
        let mut user = SoftCard::new();
        let mut rng = rand_rsa::rng();
        let bank_crypt = PrivateKey::new(&mut rng, 2048).unwrap();
        let bank_sign = PrivateKey::new(&mut rng, 2048).unwrap();
        let session = RdhSession {
            cid: user.cid().unwrap(),
            profile: 7,
            sign_key: user.sign_key().unwrap(),
            decipher_key: user.decipher_key().unwrap(),
        };
        let mut seg = SegmentBuilder::new("HKSPA", 1);
        seg.number = 3;
        let signed = sign(&mut user, &session, "21763542", "9999999999000000001", "SYS1", false, &[seg.clone()], 4).unwrap();
        let head = signed.hnshk.encode();
        let text = crate::syntax::latin1_to_string(&head);
        assert!(text.starts_with("HNSHK:2:4+RDH:7+2+"), "{text}");
        assert!(text.contains("+1+1+1:@11@"), "{text}");
        assert!(text.ends_with(":SYS1+41+1:") || text.contains(":SYS1+41+1:"), "{text}");
        assert!(text.ends_with("+1:6:1+6:10:19+280:21763542:9999999999000000001:S:7:1'"), "{text}");

        // the user's signature verifies with the user's public sign key (as the bank does it)
        let mut plain = head.clone();
        plain.extend(seg.encode());
        plain.extend(signed.hnsha.encode());
        let user_pub = bank_key("S", &user.sign_key.to_public_key());
        assert_eq!(verify(&plain, Some(&user_pub)).unwrap(), SignatureCheck::Valid);
        let wrong = bank_key("S", &bank_sign.to_public_key());
        assert!(verify(&plain, Some(&wrong)).is_err());

        // encryption for the bank: the bank can unwrap the key and decrypt
        let bk = bank_key("V", &bank_crypt.to_public_key());
        let enc = encrypt(&session, &bk, "21763542", "9999999999000000001", "SYS1", false, plain.clone()).unwrap();
        let segs = crate::syntax::parse_segments(&enc).unwrap();
        assert_eq!(segs[0].code, "HNVSK");
        assert_eq!(segs[0].text(2, 0).as_deref(), Some("4"));
        assert_eq!(segs[0].text(6, 1).as_deref(), Some("18"));
        assert_eq!(segs[0].text(7, 3).as_deref(), Some("V"));
        let wrapped = segs[0].bin(6, 3).unwrap();
        assert_eq!(wrapped.len(), 256);
        let key = bank_crypt.decrypt(Pkcs1v15Encrypt, wrapped).unwrap();
        let mut k16 = [0u8; 16];
        k16.copy_from_slice(&key);
        let mut back = crate::security::tdes_decrypt(&k16, segs[1].bin(1, 0).unwrap()).unwrap();
        let pl = *back.last().unwrap() as usize;
        back.truncate(back.len() - pl);
        assert_eq!(back, plain);

        // response from the bank, encrypted for the user and signed by the bank
        let user_crypt_pub = bank_key("V", &user.crypt_key.to_public_key());
        let mut bank_card = SoftCard { sign_key: bank_sign.clone(), crypt_key: bank_crypt.clone(), counter: 1 };
        let mut hirmg = SegmentBuilder::new("HIRMG", 2).deg(vec!["0010".into(), Val::Empty, "Nachricht entgegengenommen.".into()]);
        hirmg.number = 3;
        let bsig = sign(&mut bank_card, &session, "21763542", "21763542", "SYS1", false, &[hirmg.clone()], 4).unwrap();
        let mut resp_plain = bsig.hnshk.encode();
        resp_plain.extend(hirmg.encode());
        resp_plain.extend(bsig.hnsha.encode());
        let resp = encrypt(&session, &user_crypt_pub, "21763542", "21763542", "SYS1", false, resp_plain.clone()).unwrap();
        let rs = crate::syntax::parse_segments(&resp).unwrap();
        let dec = decrypt(&mut user, rs[0].bin(6, 3).unwrap(), rs[1].bin(1, 0).unwrap()).unwrap();
        assert_eq!(dec, resp_plain);
        let bank_sign_pub = bank_key("S", &bank_sign.to_public_key());
        assert_eq!(verify(&dec, Some(&bank_sign_pub)).unwrap(), SignatureCheck::Valid);
        assert_eq!(verify(&dec, None).unwrap(), SignatureCheck::NoKey);
    }

    #[test]
    fn no_sysid_details() {
        let s = RdhSession { cid: vec![1, 2, 3], profile: 7, ..Default::default() };
        let mut b = SegmentBuilder::new("X", 1).deg(sec_details(&s, "0", true));
        b.number = 1;
        assert_eq!(crate::syntax::latin1_to_string(&b.encode()), "X:1:1+1::0'");
    }
}
