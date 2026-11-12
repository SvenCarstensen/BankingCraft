//! RDH chip card on ZKA/SECCOS basis (e.g. VR-NetWorld-Card, RDH-7/RDH-9).
//!
//! File layout and command sequence as used by libchipcard's `zkacard` token (AqBanking/GnuCash):
//!
//! | File | Selection | Content |
//! |------|-----------|---------|
//! | MF / EF_ID (`0003`) | record 1 | card id (CID), card number |
//! | MF / EF_PWDD (`0015`) | records | PIN id and format (ASCII or FPIN2) |
//! | DF_SIG (`D27600006601`) / EF_KEYD (`0013`) | records | user keys: number, version, modulus size, usage counter |
//! | DF_NOTEPAD (`D2760000254E500100`) / EF_NOTEPAD (`A611`) | records | bank accesses (BLZ, user id, host, key numbers, bank key hash) |
//!
//! * Signature: `MANAGE SE 00 22 41 A4 [84 01 80|kid, 80 01 46]`, `INTERNAL AUTHENTICATE 00 88 00 00`
//!   with the SHA-256 hash – the card adds the second hash round and the PSS padding.
//! * Decipher: `MANAGE SE 00 22 41 B8 [84 01 80|kid, 80 01 1A]`, `PSO DECIPHER 00 2A 80 86 00||data`.
//! * Signing key = key #2, decipher key = key #3 in EF_KEYD; the signature counter is the
//!   one's complement of the usage counter (tag `91`).

use crate::{apdu_ext, err, hex, Feature, Result, Session, SoftPinProvider};
use fints::rdh::{BankKeyHash, CardKey, KeyRef, RdhCard, RdhContext};
use std::time::Duration;

const AID_DF_SIG: [u8; 6] = [0xD2, 0x76, 0x00, 0x00, 0x66, 0x01];
const AID_DF_NOTEPAD: [u8; 9] = [0xD2, 0x76, 0x00, 0x00, 0x25, 0x4E, 0x50, 0x01, 0x00];
const EF_ID: u16 = 0x0003;
const EF_PWDD: u16 = 0x0015;
const EF_KEYD: u16 = 0x0013;
const EF_NOTEPAD: u16 = 0xA611;

#[derive(Debug, Clone, Copy)]
struct PinInfo {
    id: u8,
    fpin2: bool,
    min_len: usize,
}

#[derive(Debug, Clone, Copy)]
struct KeyEntry {
    record: u8,
    num: u32,
    version: u32,
    size: usize,
    counter: Option<u32>,
}

pub struct RdhChipCard {
    pub session: Session,
    ef_id: Vec<u8>,
    pin: PinInfo,
    pin_ok: bool,
    soft_pin: Option<SoftPinProvider>,
    keys: Vec<KeyEntry>,
    /// Called with `true` before the reader waits for PIN entry on its pin pad and `false` afterwards.
    pub pinpad_notify: Option<Box<dyn Fn(bool) + Send>>,
}

/// Minimal BER-TLV walker (1/2 byte tags, short and 81/82 lengths).
pub(crate) fn tlvs(mut p: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let mut tag = p[0] as u32;
        let mut i = 1;
        if p[0] == 0x00 || p[0] == 0xFF {
            // padding
            p = &p[1..];
            continue;
        }
        if p[0] & 0x1F == 0x1F {
            if p.len() < 2 {
                break;
            }
            tag = (tag << 8) | p[1] as u32;
            i = 2;
        }
        if p.len() <= i {
            break;
        }
        let (len, l) = match p[i] {
            0x81 if p.len() > i + 1 => (p[i + 1] as usize, 2),
            0x82 if p.len() > i + 2 => (((p[i + 1] as usize) << 8) | p[i + 2] as usize, 3),
            b if b < 0x80 => (b as usize, 1),
            _ => break,
        };
        let start = i + l;
        if p.len() < start + len {
            break;
        }
        out.push((tag, &p[start..start + len]));
        p = &p[start + len..];
    }
    out
}

fn ascii(b: &[u8]) -> String {
    fints::syntax::latin1_to_string(b).trim().to_string()
}

fn num3(b: &[u8]) -> u32 {
    ascii(b).parse().unwrap_or(0)
}

/// PIN format from the "pseudo OIDs" in EF_PWDD tag 89 (libchipcard semantics:
/// one value per nibble without continuation bit).
fn pin_format(format: &[u8]) -> (bool, usize) {
    let vals: Vec<u8> = format
        .iter()
        .flat_map(|b| [b >> 4, b & 0x0F])
        .filter(|n| n & 0x08 == 0)
        .map(|n| n & 0x07)
        .collect();
    let fpin2 = vals.len() >= 2 && vals[0] == 1 && vals[1] == 1;
    let min = vals.get(2).map(|v| *v as usize).filter(|v| *v > 0).unwrap_or(4);
    (fpin2, min)
}

/// Parse one EF_NOTEPAD record (tag F0 with the HBCI data).
pub(crate) fn parse_notepad(record: u8, raw: &[u8]) -> Option<RdhContext> {
    let (_, hbci) = tlvs(raw).into_iter().find(|(t, _)| *t == 0xF0)?;
    let mut c = RdhContext { record, ..Default::default() };
    let mut version = 1u32;
    let mut sign = KeyRef::default();
    let mut crypt = KeyRef::default();
    let mut hash: Option<(KeyRef, Option<u8>, Vec<u8>)> = None;
    for (tag, v) in tlvs(hbci) {
        match tag {
            0xE0 | 0xC0 => version = num3(v).max(1),
            0xE1 => {
                for (t, x) in tlvs(v) {
                    match t {
                        0xC1 => c.bank_name = ascii(x),
                        0xC2 => c.country = ascii(x),
                        0xC3 => c.blz = ascii(x),
                        0xC4 if x.len() >= 6 => {
                            let k = KeyRef { num: num3(&x[0..3]), version: num3(&x[3..6]) };
                            let algo = x.get(6).copied();
                            let h = if x.len() > 7 { x[7..].to_vec() } else { vec![] };
                            hash = Some((k, algo, h));
                        }
                        _ => {}
                    }
                }
            }
            0xE2 => {
                for (t, x) in tlvs(v) {
                    if t == 0xC7 {
                        c.host = ascii(x);
                    }
                }
            }
            0xE3 => {
                for (t, x) in tlvs(v) {
                    match t {
                        0xC8 => c.user_id = ascii(x),
                        0xC9 => c.customer_id = ascii(x),
                        0xCA if x.len() >= 12 => {
                            sign = KeyRef { num: num3(&x[0..3]), version: num3(&x[3..6]) };
                            crypt = KeyRef { num: num3(&x[6..9]), version: num3(&x[9..12]) };
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if c.blz.is_empty() && c.user_id.is_empty() {
        return None;
    }
    c.sign_key = sign;
    c.crypt_key = crypt;
    // on ZKA cards the key numbers in the notepad are the RDH profile version
    c.profile = if sign.num != 0 && sign.num == crypt.num { sign.num } else { 0 };
    if c.customer_id.is_empty() {
        c.customer_id = c.user_id.clone();
    }
    if let Some((k, algo, h)) = hash {
        if !h.is_empty() {
            let sha256 = match algo {
                Some(0x03) => true,
                Some(0x02) => false,
                _ => version >= 2 && c.profile >= 6,
            };
            let h = if !sha256 && h.len() == 32 { h[12..].to_vec() } else { h };
            c.bank_key_hash = Some(BankKeyHash { key: k, sha256, hash: h });
        }
    }
    Some(c)
}

/// Parse one EF_KEYD record.
fn parse_keyd(record: u8, raw: &[u8]) -> Option<KeyEntry> {
    let items = tlvs(raw);
    let (first_tag, keyref) = *items.first()?;
    if !matches!(first_tag, 0x84 | 0x94) || keyref.len() < 2 {
        return None;
    }
    let direct = items.iter().find(|(t, _)| *t == 0xD0)?.1;
    // D0 holds tag/length descriptors, not the actual RSA modulus/exponent.
    // See libchipcard's LC_TokenZkaCard__ParseKeyData.
    let size = modulus_size(direct)?;
    if !(256..=512).contains(&size) {
        return None;
    }
    let counter = items.iter().skip(1).find(|(t, _)| *t == 0x91).map(|(_, v)| {
        let n = v.iter().fold(0u32, |a, b| (a << 8) | *b as u32);
        (!n) & 0xFFFF
    });
    Some(KeyEntry { record, num: keyref[0] as u32, version: keyref[1] as u32, size, counter })
}

fn modulus_size(mut descriptors: &[u8]) -> Option<usize> {
    let mut modulus = None;
    while !descriptors.is_empty() {
        let tag = *descriptors.first()?;
        let length = *descriptors.get(1)?;
        let (length, consumed) = match length {
            0x81 => (*descriptors.get(2)? as usize, 3),
            0x82 => (((*descriptors.get(2)? as usize) << 8) | *descriptors.get(3)? as usize, 4),
            n if n < 0x80 => (n as usize, 2),
            _ => return None,
        };
        if tag == 0x81 { modulus = Some(length); }
        descriptors = &descriptors[consumed..];
    }
    modulus
}

impl RdhChipCard {
    pub fn open(reader: Option<&str>, soft_pin: Option<SoftPinProvider>) -> Result<Self> {
        let session = Session::open(reader, Duration::from_secs(60))?;
        let mut me = Self {
            session,
            ef_id: vec![],
            pin: PinInfo { id: 3, fpin2: false, min_len: 4 },
            pin_ok: false,
            soft_pin,
            keys: vec![],
            pinpad_notify: None,
        };
        me.select_mf()?;
        me.select_ef(EF_ID)?;
        me.ef_id = me.read_record(1)?;
        me.select_mf()?;
        me.select_df(&AID_DF_SIG)
            .map_err(|e| err(format!("Keine RDH-Chipkarte (ZKA/SECCOS-Signaturanwendung fehlt): {e}")))?;
        me.read_pwdd();
        me.read_keys()?;
        Ok(me)
    }

    /// 10-digit card number (as printed on the card / shown by other banking programs).
    pub fn card_number(&self) -> String {
        let mut s = String::new();
        for b in self.ef_id.iter().skip(4).take(5) {
            s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('?'));
            s.push(char::from_digit((b & 0x0F) as u32, 16).unwrap_or('?'));
        }
        s
    }

    pub fn describe(&self) -> Vec<String> {
        let mut out = vec![
            format!("RDH-Chipkarte (ZKA/SECCOS) · Kartennummer {}", self.card_number()),
            format!("Karten-ID: {}", hex(&self.ef_id)),
            format!("PIN: ID {} · {}", self.pin.id, if self.pin.fpin2 { "Format 2 (BCD)" } else { "ASCII" }),
        ];
        for k in &self.keys {
            out.push(format!(
                "Schlüssel #{} Version {} · {} Bit{}",
                k.num,
                k.version,
                k.size * 8,
                k.counter.map(|c| format!(" · Signatur-Zähler {c}")).unwrap_or_default()
            ));
        }
        out
    }

    fn send(&self, a: &[u8]) -> Result<Vec<u8>> {
        let (d, sw) = self.session.transmit(a)?;
        if sw == 0x9000 || sw >> 8 == 0x61 {
            Ok(d)
        } else {
            Err(err(format!("Fehler {:04X}: {}", sw, crate::status_text(sw))))
        }
    }

    fn select_mf(&self) -> Result<()> {
        self.send(&apdu_ext(0x00, 0xA4, 0x00, 0x0C, &[0x3F, 0x00], None)).map(|_| ())
    }

    fn select_df(&self, aid: &[u8]) -> Result<()> {
        self.send(&apdu_ext(0x00, 0xA4, 0x04, 0x0C, aid, None)).map(|_| ())
    }

    fn select_ef(&self, fid: u16) -> Result<()> {
        self.send(&apdu_ext(0x00, 0xA4, 0x02, 0x0C, &fid.to_be_bytes(), None)).map(|_| ())
    }

    fn read_record(&self, rec: u8) -> Result<Vec<u8>> {
        self.send(&apdu_ext(0x00, 0xB2, rec, 0x04, &[], Some(256)))
    }

    /// Read records 1.. until "record not found" (6A83).
    fn read_records(&self, max: u8) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        for rec in 1..=max {
            match self.session.transmit(&apdu_ext(0x00, 0xB2, rec, 0x04, &[], Some(256))) {
                Ok((d, 0x9000)) => out.push((rec, d)),
                _ => break,
            }
        }
        out
    }

    fn read_pwdd(&mut self) {
        if self.select_mf().and_then(|_| self.select_ef(EF_PWDD)).is_err() {
            return;
        }
        let mut infos = Vec::new();
        for (_, raw) in self.read_records(8) {
            let items = tlvs(&raw);
            let id = items.iter().find(|(t, v)| matches!(t, 0x83 | 0x93 | 0x84 | 0x94) && !v.is_empty()).map(|(_, v)| v[0]);
            let (fpin2, min_len) = items.iter().find(|(t, _)| *t == 0x89).map(|(_, v)| pin_format(v)).unwrap_or((false, 4));
            if let Some(id) = id {
                infos.push(PinInfo { id, fpin2, min_len });
            }
        }
        if let Some(p) = infos.iter().find(|p| p.id & 0x7F == 3).or(infos.first()) {
            self.pin = *p;
        }
    }

    fn read_keys(&mut self) -> Result<()> {
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.select_ef(EF_KEYD)?;
        self.keys = self.read_records(31).iter().filter_map(|(rec, raw)| parse_keyd(*rec, raw)).collect();
        if self.keys.is_empty() {
            return Err(err("Auf der Chipkarte wurden keine RDH-Schlüssel gefunden"));
        }
        Ok(())
    }

    fn key(&self, num: u32) -> Result<KeyEntry> {
        self.keys
            .iter()
            .filter(|k| k.num == num)
            .max_by_key(|k| k.version)
            .copied()
            .ok_or_else(|| err(format!("Schlüssel #{num} nicht auf der Chipkarte")))
    }

    fn verify_apdu(&self, p2: u8) -> Vec<u8> {
        let body: [u8; 8] = if self.pin.fpin2 { [0x20, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] } else { [0x20; 8] };
        apdu_ext(0x00, 0x20, 0x00, p2, &body, None)
    }

    fn verify_on_pinpad(&self, p2: u8) -> Result<u16> {
        let a = self.verify_apdu(p2);
        let (fmt, block, len_fmt) = if self.pin.fpin2 { (0x89, 0x47, 0x04) } else { (0x82, 0x00, 0x00) };
        let mut s = vec![
            0x1E, // bTimeOut (30 s)
            0x0F, // bTimeOut2
            fmt,  // bmFormatString
            block, // bmPINBlockString
            len_fmt, // bmPINLengthFormat
            0x08,
            self.pin.min_len.clamp(4, 8) as u8, // wPINMaxExtraDigit (max 8, min)
            0x02, // bEntryValidationCondition: OK key
            0x01, // bNumberMessage
            0x04,
            0x09, // wLangId
            0x00, // bMsgIndex
            0x00,
            0x00,
            0x00, // bTeoPrologue
        ];
        s.extend_from_slice(&(a.len() as u32).to_le_bytes());
        s.extend_from_slice(&a);
        let r = self.session.control(Feature::VerifyPinDirect, &s)?;
        if r.len() < 2 {
            return Err(err("Leser lieferte keine Antwort auf die PIN-Prüfung"));
        }
        Ok(u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]]))
    }

    fn verify_soft(&self, p2: u8, pin: &str) -> Result<u16> {
        if pin.len() < self.pin.min_len.max(4) || pin.len() > 8 || !pin.is_ascii() {
            return Err(err(format!("Die Karten-PIN muss {} bis 8 ASCII-Zeichen lang sein", self.pin.min_len.max(4))));
        }
        let mut body = [0x20u8; 8];
        if self.pin.fpin2 {
            if !pin.chars().all(|c| c.is_ascii_digit()) {
                return Err(err("Die Karten-PIN darf nur Ziffern enthalten"));
            }
            body = [0xFF; 8];
            body[0] = 0x20 | pin.len() as u8;
            for (i, c) in pin.bytes().enumerate() {
                let d = c - b'0';
                let idx = 1 + i / 2;
                body[idx] = if i % 2 == 0 { (d << 4) | 0x0F } else { (body[idx] & 0xF0) | d };
            }
        } else {
            body[..pin.len()].copy_from_slice(pin.as_bytes());
        }
        let (_, sw) = self.session.transmit(&apdu_ext(0x00, 0x20, 0x00, p2, &body, None))?;
        Ok(sw)
    }
}

fn pin_result(sw: u16) -> Result<()> {
    match sw {
        0x9000 => Ok(()),
        0x6401 => Err(fints::Error::Cancelled),
        _ => Err(err(format!("PIN-Prüfung: {} ({:04X})", crate::status_text(sw), sw))),
    }
}

impl RdhCard for RdhChipCard {
    fn cid(&mut self) -> fints::Result<Vec<u8>> {
        Ok(self.ef_id.clone())
    }

    fn verify_pin(&mut self) -> fints::Result<()> {
        if self.pin_ok {
            return Ok(());
        }
        self.select_mf()?;
        let pinpad = self.session.has(Feature::VerifyPinDirect);
        let soft = if pinpad {
            None
        } else {
            let provider = self.soft_pin.as_ref().ok_or_else(|| err("Leser ohne PIN-Pad und keine PIN-Abfrage verfügbar"))?;
            Some(provider().ok_or(fints::Error::Cancelled)?)
        };
        let mut sw = 0;
        // the PIN reference from EF_PWDD first; some cards address it DF-specific (bit 8)
        for p2 in [self.pin.id, self.pin.id ^ 0x80] {
            sw = match &soft {
                None => {
                    if let Some(n) = &self.pinpad_notify {
                        n(true);
                    }
                    let r = self.verify_on_pinpad(p2);
                    if let Some(n) = &self.pinpad_notify {
                        n(false);
                    }
                    r?
                }
                Some(pin) => self.verify_soft(p2, pin)?,
            };
            if sw != 0x6A88 && sw != 0x6A86 {
                break;
            }
        }
        pin_result(sw)?;
        self.pin_ok = true;
        Ok(())
    }

    fn contexts(&mut self) -> fints::Result<Vec<RdhContext>> {
        self.verify_pin()?;
        self.select_mf()?;
        self.select_df(&AID_DF_NOTEPAD)?;
        self.select_ef(EF_NOTEPAD)?;
        Ok(self.read_records(30).iter().filter_map(|(rec, raw)| parse_notepad(*rec, raw)).collect())
    }

    fn sign_key(&mut self) -> fints::Result<CardKey> {
        let k = self.key(2)?;
        Ok(CardKey { num: k.num, version: k.version, size: k.size })
    }

    fn decipher_key(&mut self) -> fints::Result<CardKey> {
        let k = self.key(3)?;
        Ok(CardKey { num: k.num, version: k.version, size: k.size })
    }

    fn sign_counter(&mut self) -> fints::Result<u32> {
        let k = self.key(2)?;
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.select_ef(EF_KEYD)?;
        let raw = self.read_record(k.record)?;
        let fresh = parse_keyd(k.record, &raw).and_then(|e| e.counter);
        fresh.ok_or_else(|| err("Signatur-Zähler der Chipkarte nicht lesbar"))
    }

    fn sign(&mut self, hash: &[u8; 32]) -> fints::Result<Vec<u8>> {
        self.verify_pin()?;
        let k = self.key(2)?;
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.send(&apdu_ext(0x00, 0x22, 0x41, 0xA4, &[0x84, 0x01, 0x80 | k.record, 0x80, 0x01, 0x46], None))?;
        let sig = self.send(&apdu_ext(0x00, 0x88, 0x00, 0x00, hash, Some(k.size as u16)))?;
        if sig.len() != k.size {
            return Err(err("Unerwartete Länge der Kartensignatur"));
        }
        if let Some(e) = self.keys.iter_mut().find(|e| e.record == k.record) {
            e.counter = e.counter.map(|c| c.wrapping_add(1));
        }
        Ok(sig)
    }

    fn decipher(&mut self, data: &[u8]) -> fints::Result<Vec<u8>> {
        self.verify_pin()?;
        let k = self.key(3)?;
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.send(&apdu_ext(0x00, 0x22, 0x41, 0xB8, &[0x84, 0x01, 0x80 | k.record, 0x80, 0x01, 0x1A], None))?;
        let mut body = Vec::with_capacity(data.len() + 1);
        body.push(0x00); // padding indicator
        body.extend_from_slice(data);
        self.send(&apdu_ext(0x00, 0x2A, 0x80, 0x86, &body, Some(k.size as u16)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tlv(tag: u8, v: &[u8]) -> Vec<u8> {
        let mut o = vec![tag, v.len() as u8];
        o.extend_from_slice(v);
        o
    }

    #[test]
    fn notepad_record() {
        let mut hash = vec![0x03];
        hash.extend([0xAB; 32]);
        let institute = [
            tlv(0xC1, b"VR Bank Nord"),
            tlv(0xC2, b"280"),
            tlv(0xC3, b"21763542"),
            tlv(0xC4, &[b"001003".as_slice(), &hash].concat()),
        ]
        .concat();
        let user = [tlv(0xC8, b"9999999999000000001"), tlv(0xCA, b"007001007001")].concat();
        let hbci = [tlv(0xE0, b"002"), tlv(0xE1, &institute), tlv(0xE2, &tlv(0xC7, b"fints1.atruvia.de")), tlv(0xE3, &user)].concat();
        let mut rec = vec![0xF0, 0x81, hbci.len() as u8];
        rec.extend(hbci);
        let c = parse_notepad(1, &rec).unwrap();
        assert_eq!(c.blz, "21763542");
        assert_eq!(c.user_id, "9999999999000000001");
        assert_eq!(c.customer_id, "9999999999000000001");
        assert_eq!(c.host, "fints1.atruvia.de");
        assert_eq!(c.profile, 7);
        let h = c.bank_key_hash.unwrap();
        assert!(h.sha256);
        assert_eq!(h.key, KeyRef { num: 1, version: 3 });
        assert_eq!(h.hash, vec![0xAB; 32]);
    }

    #[test]
    fn keyd_record() {
        let d0 = tlv(0xD0, &[0x81, 0x82, 0x01, 0x00, 0x82, 0x03]);
        let rec = [tlv(0x84, &[0x02, 0x01]), d0, tlv(0x91, &[0xFF, 0xD5])].concat();
        let k = parse_keyd(4, &rec).unwrap();
        assert_eq!((k.num, k.version, k.size, k.record), (2, 1, 256, 4));
        assert_eq!(k.counter, Some(0x2A));
        let larger = [tlv(0x84, &[2, 1]), tlv(0xD0, &[0x81, 0x82, 0x01, 0x80, 0x83, 0x00])].concat();
        assert_eq!(parse_keyd(1, &larger).unwrap().size, 384);
        let public = [tlv(0x83, &[2, 1]), tlv(0xD0, &[0x81, 0x82, 0x01, 0x00])].concat();
        assert!(parse_keyd(1, &public).is_none());
        assert!(modulus_size(&[0x81, 0x82, 1]).is_none());
    }

    #[test]
    fn pin_formats() {
        // 2.1.6 → ASCII, min 6 ; 1.1.4 → FPIN2
        assert_eq!(pin_format(&[0x21, 0x60]), (false, 6));
        assert_eq!(pin_format(&[0x11, 0x40]), (true, 4));
    }

    #[test]
    fn extended_apdu() {
        let a = apdu_ext(0x00, 0x2A, 0x80, 0x86, &[0u8; 257], Some(256));
        assert_eq!(&a[..7], &[0x00, 0x2A, 0x80, 0x86, 0x00, 0x01, 0x01]);
        assert_eq!(&a[a.len() - 2..], &[0x01, 0x00]);
        assert_eq!(a.len(), 7 + 257 + 2);
        let sign = apdu_ext(0, 0x88, 0, 0, &[0; 32], Some(384));
        assert_eq!(&sign[..7], &[0, 0x88, 0, 0, 0, 0, 32]);
        assert_eq!(&sign[sign.len() - 2..], &[1, 128]);
        assert_eq!(apdu_ext(0, 0xCA, 0, 0, &[], Some(384)), [0, 0xCA, 0, 0, 0, 1, 128]);
        let corrected = crate::corrected_le(&a, 128).unwrap();
        assert_eq!(&corrected[corrected.len()-2..], &[0, 128]);
        assert_eq!(&corrected[..corrected.len()-2], &a[..a.len()-2]);
        let no_le = vec![0, 0x22, 0x41, 0xA4, 2, 0x80, 0x46];
        assert_eq!(crate::corrected_le(&no_le, 0).unwrap(), [0, 0x22, 0x41, 0xA4, 2, 0x80, 0x46, 0]);
    }
}
