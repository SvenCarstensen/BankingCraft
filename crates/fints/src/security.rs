//! Security envelopes: PIN/TAN (HNVSK 998 / HNSHK with TAN method) and
//! HBCI chip card DDV (2-key 3DES-CBC encryption, RIPEMD-160 + DES retail MAC
//! computed by the card).

use crate::error::{Error, Result};
use crate::syntax::{parse_segments, Segment, SegmentBuilder, Val};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use chrono::Local;
use ripemd::{Digest, Ripemd160};

type TdesEnc = cbc::Encryptor<des::TdesEde3>;
type TdesDec = cbc::Decryptor<des::TdesEde3>;

/// Bank access data stored on a DDV card (EF_BNK record).
#[derive(Debug, Clone, Default)]
pub struct DdvBankData {
    pub record: u8,
    pub short_name: String,
    pub blz: String,
    /// 1 = TCP/IP (port 3000), 2 = BTX (obsolete)
    pub comm_type: u8,
    pub host: String,
    pub country: String,
    pub user_id: String,
}

/// Key number and version as reported by the card.
#[derive(Debug, Clone, Copy, Default)]
pub struct DdvKey {
    pub num: u32,
    pub version: u32,
}

/// Operations a DDV chip card must provide. Implemented by the `cardreader` crate
/// via PC/SC; kept as a trait so the protocol layer has no hardware dependency.
pub trait DdvCard: Send {
    fn cid(&mut self) -> Result<Vec<u8>>;
    fn bank_data(&mut self, record: u8) -> Result<DdvBankData>;
    fn keys(&mut self) -> Result<(DdvKey, DdvKey)>;
    fn sig_id(&mut self) -> Result<u32>;
    fn set_sig_id(&mut self, id: u32) -> Result<()>;
    /// Verify the card PIN. Readers with a pin pad (class 2/3) do this without the
    /// PIN ever reaching the host.
    fn verify_pin(&mut self) -> Result<()>;
    /// Compute the 8 byte MAC over a 20 byte RIPEMD-160 hash.
    fn sign(&mut self, hash: &[u8; 20]) -> Result<[u8; 8]>;
    /// Returns (plain session key, session key encrypted for the bank).
    fn session_keys(&mut self, enc_key_num: u32) -> Result<([u8; 16], [u8; 16])>;
    /// Decrypt a session key received from the bank.
    fn decrypt_key(&mut self, enc_key_num: u32, encrypted: &[u8]) -> Result<[u8; 16]>;
}

pub enum Auth<'a> {
    /// No signature/encryption (anonymous dialogs).
    None,
    PinTan {
        pin: &'a str,
        tan: Option<&'a str>,
        /// Security function, i.e. the TAN method code ("999" = one step).
        security_function: &'a str,
    },
    Ddv {
        card: &'a mut dyn DdvCard,
        cid: &'a [u8],
        sig_key: DdvKey,
        enc_key: DdvKey,
    },
}

pub struct Envelope<'a> {
    pub dialog_id: &'a str,
    pub message_number: u32,
    pub blz: &'a str,
    pub user_id: &'a str,
    pub system_id: &'a str,
}

fn now_deg() -> Vec<Val> {
    let now = Local::now();
    vec![
        "1".into(),
        now.format("%Y%m%d").to_string().into(),
        now.format("%H%M%S").to_string().into(),
    ]
}

fn sec_ref() -> String {
    use rand::Rng;
    rand::thread_rng().gen_range(1_000_000..9_999_999u32).to_string()
}

/// Build a complete, ready-to-send FinTS message around `segments`.
pub fn build_message(env: &Envelope, auth: Auth, mut segments: Vec<SegmentBuilder>) -> Result<Vec<u8>> {
    let signed = !matches!(auth, Auth::None);
    let first_num = if signed { 3 } else { 2 };
    for (k, s) in segments.iter_mut().enumerate() {
        s.number = first_num + k as u32;
    }
    let last_num = first_num + segments.len() as u32 + if signed { 1 } else { 0 };

    let body: Vec<u8> = match auth {
        Auth::None => segments.iter().flat_map(|s| s.encode()).collect(),
        Auth::PinTan { pin, tan, security_function } => {
            let r = sec_ref();
            // security profile version: 1 = one-step (999), 2 = two-step (as AqBanking)
            let profile_version = if security_function == "999" { "1" } else { "2" };
            let mut hnshk = SegmentBuilder::new("HNSHK", 4)
                .deg(vec!["PIN".into(), profile_version.into()])
                .de(security_function)
                .de(r.as_str())
                .de("1")
                .de("1")
                .deg(vec!["1".into(), Val::Empty, env.system_id.into()])
                .de("1")
                .deg(now_deg())
                .deg(vec!["1".into(), "999".into(), "1".into()])
                .deg(vec!["6".into(), "10".into(), "16".into()])
                .deg(key_name(env, "S", 0, 0));
            hnshk.number = 2;
            let mut sig_vals = vec![Val::from(pin)];
            if let Some(t) = tan {
                sig_vals.push(t.into());
            }
            let mut hnsha = SegmentBuilder::new("HNSHA", 2).de(r.as_str()).de(Val::Empty).deg(sig_vals);
            hnsha.number = last_num - 1;

            let mut inner = hnshk.encode();
            for s in &segments {
                inner.extend(s.encode());
            }
            inner.extend(hnsha.encode());

            let mut hnvsk = SegmentBuilder::new("HNVSK", 3)
                .deg(vec!["PIN".into(), profile_version.into()])
                .de("998")
                .de("1")
                .deg(vec!["1".into(), Val::Empty, env.system_id.into()])
                .deg(now_deg())
                .deg(vec![
                    "2".into(),
                    "2".into(),
                    "13".into(),
                    Val::Bin(vec![0u8; 8]),
                    "5".into(),
                    "1".into(),
                ])
                .deg(key_name(env, "V", 0, 0))
                .de("0");
            hnvsk.number = 998;
            let mut hnvsd = SegmentBuilder::new("HNVSD", 1).bin(inner);
            hnvsd.number = 999;
            let mut out = hnvsk.encode();
            out.extend(hnvsd.encode());
            out
        }
        Auth::Ddv { card, cid, sig_key, enc_key } => {
            card.verify_pin()?;
            let sig_id = card.sig_id()?;
            let r = sec_ref();
            let mut hnshk = SegmentBuilder::new("HNSHK", 4)
                .deg(vec!["DDV".into(), "1".into()])
                .de("2")
                .de(r.as_str())
                .de("1")
                .de("1")
                .deg(vec!["1".into(), Val::Bin(cid.to_vec())])
                .de(sig_id)
                .deg(now_deg())
                .deg(vec!["1".into(), "999".into(), "1".into()])
                .deg(vec!["6".into(), "1".into(), "999".into()])
                .deg(key_name(env, "S", sig_key.num, sig_key.version));
            hnshk.number = 2;

            let mut signed_data = hnshk.encode();
            for s in &segments {
                signed_data.extend(s.encode());
            }
            let hash: [u8; 20] = Ripemd160::digest(&signed_data).into();
            let mac = card.sign(&hash)?;
            card.set_sig_id(sig_id.wrapping_add(1))?;

            let mut hnsha = SegmentBuilder::new("HNSHA", 2).de(r.as_str()).bin(mac.to_vec());
            hnsha.number = last_num - 1;
            let mut plain = signed_data;
            plain.extend(hnsha.encode());

            let (plain_key, enc_session_key) = card.session_keys(enc_key.num)?;
            let crypted = tdes_encrypt(&plain_key, &pad(plain));

            let mut hnvsk = SegmentBuilder::new("HNVSK", 3)
                .deg(vec!["DDV".into(), "1".into()])
                .de("4")
                .de("1")
                .deg(vec!["1".into(), Val::Bin(cid.to_vec())])
                .deg(now_deg())
                .deg(vec![
                    "2".into(),
                    "2".into(),
                    "13".into(),
                    Val::Bin(enc_session_key.to_vec()),
                    "5".into(),
                    "1".into(),
                ])
                .deg(key_name(env, "V", enc_key.num, enc_key.version))
                .de("0");
            hnvsk.number = 998;
            let mut hnvsd = SegmentBuilder::new("HNVSD", 1).bin(crypted);
            hnvsd.number = 999;
            let mut out = hnvsk.encode();
            out.extend(hnvsd.encode());
            out
        }
    };

    let mut hnhbs = SegmentBuilder::new("HNHBS", 1).de(env.message_number);
    hnhbs.number = last_num;
    let tail = hnhbs.encode();

    // HNHBK carries the total length as a fixed 12 digit field, so its own size is constant.
    let header_for_len = |total: usize| {
        let mut h = SegmentBuilder::new("HNHBK", 3)
            .de(format!("{:012}", total))
            .de("300")
            .de(env.dialog_id)
            .de(env.message_number);
        h.number = 1;
        h.encode()
    };
    let header_len = header_for_len(0).len();
    let total = header_len + body.len() + tail.len();
    let mut msg = header_for_len(total);
    msg.extend(body);
    msg.extend(tail);
    Ok(msg)
}

fn key_name(env: &Envelope, kind: &str, num: u32, version: u32) -> Vec<Val> {
    vec!["280".into(), env.blz.into(), env.user_id.into(), kind.into(), num.into(), version.into()]
}

fn pad(mut data: Vec<u8>) -> Vec<u8> {
    let pad_len = 8 - (data.len() % 8);
    data.extend(std::iter::repeat_n(0u8, pad_len - 1));
    data.push(pad_len as u8);
    data
}

fn long_key(k: &[u8; 16]) -> [u8; 24] {
    let mut l = [0u8; 24];
    l[..16].copy_from_slice(k);
    l[16..].copy_from_slice(&k[..8]);
    l
}

fn tdes_encrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let enc = TdesEnc::new(&long_key(key).into(), &[0u8; 8].into());
    enc.encrypt_padded_vec_mut::<cbc::cipher::block_padding::NoPadding>(data)
}

fn tdes_decrypt(key: &[u8; 16], data: &[u8]) -> Result<Vec<u8>> {
    let dec = TdesDec::new(&long_key(key).into(), &[0u8; 8].into());
    dec.decrypt_padded_vec_mut::<cbc::cipher::block_padding::NoPadding>(data)
        .map_err(|_| Error::Security("decryption failed".into()))
}

/// Unwrap a received message: returns all segments with the HNVSD contents inlined.
pub fn open_message(raw: &[u8], card: Option<(&mut dyn DdvCard, u32)>) -> Result<Vec<Segment>> {
    let outer = parse_segments(raw)?;
    let mut result = Vec::with_capacity(outer.len() + 8);
    let mut enc_key: Option<Vec<u8>> = None;
    let mut card = card;
    for seg in outer {
        match seg.code.as_str() {
            "HNVSK" => {
                if seg.text(1, 0).as_deref() == Some("DDV") {
                    enc_key = seg.bin(6, 3).map(|b| b.to_vec());
                }
            }
            "HNVSD" => {
                let payload = seg.bin(1, 0).unwrap_or_default().to_vec();
                let plain = match (&enc_key, card.as_mut()) {
                    (Some(k), Some((c, num))) => {
                        let session = c.decrypt_key(*num, k)?;
                        let mut p = tdes_decrypt(&session, &payload)?;
                        if let Some(&pl) = p.last() {
                            let pl = pl as usize;
                            if pl > 0 && pl <= 8 && pl <= p.len() {
                                p.truncate(p.len() - pl);
                            }
                        }
                        p
                    }
                    _ => payload,
                };
                result.extend(parse_segments(&plain)?);
            }
            _ => result.push(seg),
        }
    }
    Ok(result)
}
