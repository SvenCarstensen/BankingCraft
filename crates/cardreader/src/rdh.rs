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
use zeroize::Zeroizing;

const AID_DF_SIG: [u8; 6] = [0xD2, 0x76, 0x00, 0x00, 0x66, 0x01];
const AID_DF_NOTEPAD: [u8; 9] = [0xD2, 0x76, 0x00, 0x00, 0x25, 0x4E, 0x50, 0x01, 0x00];
const EF_ID: u16 = 0x0003;
const EF_PWDD: u16 = 0x0015;
const EF_FBZ: u16 = 0x0016;
const EF_KEYD: u16 = 0x0013;
const EF_NOTEPAD: u16 = 0xA611;

#[derive(Debug, Clone, Copy)]
struct PinInfo {
    id: u8,
    fpin2: bool,
    min_len: usize,
    retry_record: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PinRetries { maximum: u8, remaining: u8 }

fn parse_pin_retries(raw: &[u8]) -> Result<PinRetries> {
    if raw.len() < 2 || raw[0] == 0 || raw[1] > raw[0] {
        return Err(err(i18n::tr("Ungültiger PIN-Fehlversuchszähler der Karte")));
    }
    Ok(PinRetries { maximum: raw[0], remaining: raw[1] })
}

impl PinInfo {
    fn verify_template(&self) -> Vec<u8> {
        if self.fpin2 {
            apdu_ext(0x00, 0x20, 0x00, self.id, &[0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], None)
        } else {
            // ZKA ASCII is variable-length. The reader appends Lc and exactly the
            // entered characters. An eight-byte space-padded template is only the
            // libchipcard SCM-X32 workaround, not the generic/cyberJack command.
            vec![0x00, 0x20, 0x00, self.id]
        }
    }

    fn pinpad_command(&self) -> Vec<u8> {
        let a = self.verify_template();
        let (fmt, block, len_fmt) = if self.fpin2 { (0x89, 0x47, 0x04) } else { (0x82, 0x00, 0x00) };
        let mut command = vec![0x1e, 0x0f, fmt, block, len_fmt, 8, self.min_len as u8,
            0x02, 0x01, 0x04, 0x09, 0x00, 0x00, 0x00, 0x00];
        command.extend_from_slice(&(a.len() as u32).to_le_bytes());
        command.extend_from_slice(&a);
        command
    }

    fn soft_command(&self, pin: &str) -> Result<Zeroizing<Vec<u8>>> {
        if pin.len() < self.min_len || pin.len() > 8 || !pin.is_ascii() {
            return Err(err(i18n::tr_format!("Die Karten-PIN muss {} bis 8 ASCII-Zeichen lang sein", self.min_len)));
        }
        if !self.fpin2 {
            return Ok(Zeroizing::new(apdu_ext(0x00, 0x20, 0x00, self.id, pin.as_bytes(), None)));
        }
        if !pin.bytes().all(|c| c.is_ascii_digit()) {
            return Err(err(i18n::tr("Die Karten-PIN darf nur Ziffern enthalten")));
        }
        let mut body = Zeroizing::new([0xff; 8]);
        body[0] = 0x20 | pin.len() as u8;
        for (i, c) in pin.bytes().enumerate() {
            let d = c - b'0';
            let idx = 1 + i / 2;
            body[idx] = if i % 2 == 0 { (d << 4) | 0x0f } else { (body[idx] & 0xf0) | d };
        }
        Ok(Zeroizing::new(apdu_ext(0x00, 0x20, 0x00, self.id, body.as_ref(), None)))
    }
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
    pin_verification: PinVerification,
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

/// PIN format from EF_PWDD tag 89. Unknown encodings must never trigger an
/// attempted verification with a guessed ASCII encoding.
fn pin_format(format: &[u8]) -> Option<(bool, usize)> {
    let mut values = Vec::new(); let mut value = 0u32; let mut continued = false;
    for nibble in format.iter().flat_map(|b| [b >> 4, b & 0xf]) {
        value = value.checked_mul(8)?.checked_add(u32::from(nibble & 7))?;
        continued = nibble & 8 != 0;
        if !continued { values.push(value); value = 0; }
    }
    if continued { return None; }
    let fpin2 = match values.get(..2)? { [1, 1] => true, [2, 1] => false, _ => return None };
    let min = *values.get(2)? as usize;
    (4..=8).contains(&min).then_some((fpin2, min))
}

fn access_pin(records: &[(u8, Vec<u8>)]) -> Result<PinInfo> {
    let mut selected = None;
    for (_, raw) in records {
        let items = tlvs(raw);
        let Some((tag, reference)) = items.iter().find(|(t, v)| matches!(t, 0x83 | 0x93 | 0x84 | 0x94) && !v.is_empty()) else { continue };
        // ZKA's cardholder/access PIN is global reference 3. Do not select a PUK,
        // management PIN or a DF-specific PIN just because it is the first record.
        if reference[0] != 3 { continue; }
        if selected.is_some() { return Err(err(i18n::tr("Zugriffs-PIN der Karte ist nicht eindeutig beschrieben"))); }
        let (fpin2, min_len) = items.iter().find(|(t, _)| *t == 0x89)
            .and_then(|(_, bytes)| pin_format(bytes))
            .ok_or_else(|| err(i18n::tr("PIN-Format der Karte ist unbekannt. Keine PIN-Prüfung ausgeführt.")))?;
        let retry_record = if matches!(*tag, 0x83 | 0x93) {
            Some(*reference.get(1).filter(|r| **r > 0)
                .ok_or_else(|| err(i18n::tr("PIN-Fehlversuchszähler ist nicht eindeutig zugeordnet")))?)
        } else { None };
        selected = Some(PinInfo { id: 3, fpin2, min_len, retry_record });
    }
    selected.ok_or_else(|| err(i18n::tr("Zugriffs-PIN (Referenz 3) fehlt in den Kartendaten. Keine PIN-Prüfung ausgeführt.")))
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
    // VR-NetWorld cards also use 1984-bit (248-byte) RDH keys. Their
    // 2048-bit authentication key must not hide valid signing/decipher keys.
    if !(248..=512).contains(&size) {
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
            pin: PinInfo { id: 3, fpin2: false, min_len: 4, retry_record: None },
            pin_verification: PinVerification::default(),
            soft_pin,
            keys: vec![],
            pinpad_notify: None,
        };
        me.select_mf()?;
        me.select_ef(EF_ID)?;
        me.ef_id = me.read_record(1)?;
        me.select_mf()?;
        me.select_df(&AID_DF_SIG)
            .map_err(|e| err(i18n::tr_format!("Keine RDH-Chipkarte (ZKA/SECCOS-Signaturanwendung fehlt): {e}")))?;
        me.read_pwdd()?;
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
            i18n::tr_format!("RDH-Chipkarte (ZKA/SECCOS) · Kartennummer {}", self.card_number()),
            i18n::tr_format!("Karten-ID: {}", hex(&self.ef_id)),
            i18n::tr_format!("PIN: ID {} · {} · {}–8 Zeichen", self.pin.id, if self.pin.fpin2 { "Format 2 (BCD)" } else { "ASCII" }, self.pin.min_len),
        ];
        out.push(match self.pin_retries() {
            Ok(Some(status)) => i18n::tr_format!("PIN-Versuche verbleibend: {} von {} (nur gelesen, keine PIN-Prüfung)", status.remaining, status.maximum),
            Ok(None) => i18n::tr("PIN-Fehlversuchszähler: von dieser Karte nicht zugeordnet").into(),
            Err(e) => i18n::tr_format!("PIN-Fehlversuchszähler nicht lesbar: {e}"),
        });
        for k in &self.keys {
            out.push(i18n::tr_format!(
                "Schlüssel #{} Version {} · {} Bit{}",
                k.num,
                k.version,
                k.size * 8,
                k.counter.map(|c| i18n::tr_format!(" · Signatur-Zähler {c}")).unwrap_or_default()
            ));
        }
        out
    }

    fn send(&self, a: &[u8]) -> Result<Vec<u8>> {
        let (d, sw) = self.session.transmit(a)?;
        if sw == 0x9000 || sw >> 8 == 0x61 {
            Ok(d)
        } else {
            Err(err(i18n::tr_format!("Fehler {:04X}: {}", sw, crate::status_text(sw))))
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

    fn read_pwdd(&mut self) -> Result<()> {
        self.select_mf()?;
        self.select_ef(EF_PWDD)?;
        let mut records = Vec::new();
        for record in 1..=8 {
            let (data, sw) = self.session.transmit(&apdu_ext(0, 0xb2, record, 4, &[], Some(256)))?;
            match sw {
                0x9000 => records.push((record, data)),
                0x6a83 => break,
                _ => return Err(err(i18n::tr_format!("PIN-Beschreibung nicht lesbar ({sw:04X}). Keine PIN-Prüfung ausgeführt."))),
            }
        }
        self.pin = access_pin(&records)?;
        Ok(())
    }

    /// EF_FBZ is a read-only counter query; never send VERIFY to discover retries.
    fn pin_retries(&self) -> Result<Option<PinRetries>> {
        let Some(record) = self.pin.retry_record else { return Ok(None) };
        self.select_mf()?;
        self.select_ef(EF_FBZ)?;
        parse_pin_retries(&self.read_record(record)?).map(Some)
    }

    fn read_keys(&mut self) -> Result<()> {
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.select_ef(EF_KEYD)?;
        self.keys = self.read_records(31).iter().filter_map(|(rec, raw)| parse_keyd(*rec, raw)).collect();
        if self.keys.is_empty() {
            return Err(err(i18n::tr("Auf der Chipkarte wurden keine RDH-Schlüssel gefunden")));
        }
        Ok(())
    }

    fn key(&self, num: u32) -> Result<KeyEntry> {
        self.keys
            .iter()
            .filter(|k| k.num == num)
            .max_by_key(|k| k.version)
            .copied()
            .ok_or_else(|| err(i18n::tr_format!("Schlüssel #{num} nicht auf der Chipkarte")))
    }

}

#[derive(Default)]
struct PinVerification { attempted: bool, verified: bool }

impl PinVerification {
    fn run(&mut self, operation: impl FnOnce() -> Result<u16>) -> Result<()> {
        if self.verified { return Ok(()); }
        if self.attempted {
            return Err(err(i18n::tr("PIN-Prüfung dieser Kartenverbindung ist bereits fehlgeschlagen oder wurde abgebrochen. Keine automatische Wiederholung.")));
        }
        self.attempted = true;
        pin_result(operation()?)?;
        self.verified = true;
        Ok(())
    }
}

fn pinpad_status(response: &[u8]) -> Result<u16> {
    let bytes: [u8; 2] = response.try_into()
        .map_err(|_| err(i18n::tr("Ungültige Antwort des Lesers auf die PIN-Prüfung. Kein weiterer Versuch ausgeführt.")))?;
    Ok(u16::from_be_bytes(bytes))
}

fn pin_result(sw: u16) -> Result<()> {
    match sw {
        0x9000 => Ok(()),
        0x6401 => Err(fints::Error::Cancelled),
        0x6983 | 0x63c0 => Err(err(i18n::tr_format!("Karten-PIN gesperrt (0 Versuche verbleibend, {sw:04X}). Keine automatische Wiederholung."))),
        code if code & 0xfff0 == 0x63c0 => Err(err(i18n::tr_format!("Die Chipkarte hat die PIN abgelehnt. Verbleibende PIN-Versuche: {} ({sw:04X}). Keine automatische Wiederholung.", code & 0xf))),
        _ => Err(err(i18n::tr_format!("PIN-Prüfung: {} ({:04X})", crate::status_text(sw), sw))),
    }
}

impl RdhCard for RdhChipCard {
    fn cid(&mut self) -> fints::Result<Vec<u8>> {
        Ok(self.ef_id.clone())
    }

    fn verify_pin(&mut self) -> fints::Result<()> {
        if self.pin_verification.verified { return Ok(()); }
        if self.pin_retries()?.is_some_and(|status| status.remaining == 0) {
            return Err(err(i18n::tr("Die Karten-PIN ist gesperrt (0 Versuche verbleibend). Keine weitere PIN-Prüfung ausgeführt.")));
        }
        self.select_mf()?;
        let session = &self.session;
        let pin = self.pin;
        let provider = self.soft_pin.as_ref();
        let notify = self.pinpad_notify.as_ref();
        self.pin_verification.run(|| {
            if session.has(Feature::VerifyPinDirect) {
                if let Some(n) = notify { n(true); }
                let result = session.control(Feature::VerifyPinDirect, &pin.pinpad_command());
                if let Some(n) = notify { n(false); }
                pinpad_status(&result?)
            } else {
                let provider = provider.ok_or_else(|| err(i18n::tr("Leser ohne PIN-Pad und keine PIN-Abfrage verfügbar")))?;
                let secret = Zeroizing::new(provider().ok_or(fints::Error::Cancelled)?);
                let command = pin.soft_command(&secret)?;
                // VERIFY is state-changing: no generic 6Cxx APDU resubmission.
                session.transmit_raw(&command).map(|(_, sw)| sw)
            }
        })
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
        fresh.ok_or_else(|| err(i18n::tr("Signatur-Zähler der Chipkarte nicht lesbar")))
    }

    fn sign(&mut self, hash: &[u8; 32]) -> fints::Result<Vec<u8>> {
        self.verify_pin()?;
        let k = self.key(2)?;
        self.select_mf()?;
        self.select_df(&AID_DF_SIG)?;
        self.send(&apdu_ext(0x00, 0x22, 0x41, 0xA4, &[0x84, 0x01, 0x80 | k.record, 0x80, 0x01, 0x46], None))?;
        let sig = self.send(&apdu_ext(0x00, 0x88, 0x00, 0x00, hash, Some(k.size as u16)))?;
        if sig.len() != k.size {
            return Err(err(i18n::tr("Unerwartete Länge der Kartensignatur")));
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
    fn keyd_recognizes_1984_bit_vr_card_signing_and_decipher_keys() {
        // EF_KEYD layout observed on a VR-NetWorld card (counter is synthetic).
        // Its authentication key is 2048 bits, but keys #2 and #3 use 248 bytes.
        // Do not discard those two keys while accepting only the unrelated #4.
        for (record, number, size, exponent) in [(1, 4, 256u16, 0x84), (2, 2, 248, 0x84), (3, 3, 248, 0x82)] {
            let [hi, lo] = size.to_be_bytes();
            let raw = [
                tlv(0x84, &[number, 0, 0x0f, number, 0x0e, number]),
                tlv(0xd0, &[0x81, 0x82, hi, lo, exponent, if exponent == 0x82 { 3 } else { 0 }, 0x92, 0]),
                tlv(0x90, &[0xff]),
                tlv(0x91, &[0xff, 0xd5]),
            ].concat();
            let key = parse_keyd(record, &raw).expect("existing VR card key must not be discarded");
            assert_eq!((key.record, key.num, key.version, key.size, key.counter),
                (record, u32::from(number), 0, usize::from(size), Some(42)));
        }
    }

    #[test]
    fn keyd_still_rejects_public_missing_and_out_of_range_keys() {
        for tag in [0x83, 0x93] {
            let raw = [tlv(tag, &[2, 1]), tlv(0xd0, &[0x81, 0x81, 0xf8])].concat();
            assert!(parse_keyd(1, &raw).is_none());
        }
        assert!(parse_keyd(1, &tlv(0x84, &[2, 1])).is_none());
        for size in [0u16, 128, 247, 513] {
            let [hi, lo] = size.to_be_bytes();
            let raw = [tlv(0x84, &[2, 1]), tlv(0xd0, &[0x81, 0x82, hi, lo])].concat();
            assert!(parse_keyd(1, &raw).is_none(), "invalid modulus length {size}");
        }
        for size in [248u16, 256, 384, 512] {
            let [hi, lo] = size.to_be_bytes();
            let raw = [tlv(0x94, &[3, 1]), tlv(0xd0, &[0x81, 0x82, hi, lo])].concat();
            assert_eq!(parse_keyd(1, &raw).unwrap().size, usize::from(size));
        }
    }

    #[test]
    fn pin_formats() {
        // 2.1.6 → ASCII, min 6 ; 1.1.4 → FPIN2
        assert_eq!(pin_format(&[0x21, 0x60]), Some((false, 6)));
        assert_eq!(pin_format(&[0x11, 0x40]), Some((true, 4)));
        assert_eq!(pin_format(&[0x21, 0x90]), Some((false, 8)));
        for bytes in [&[][..], &[0x21], &[0x31, 0x60], &[0x21, 0x30], &[0x21, 0x91], &[0x21, 0x68]] {
            assert_eq!(pin_format(bytes), None);
        }
    }

    fn ascii_pin(min_len: usize) -> PinInfo {
        PinInfo { id: 3, fpin2: false, min_len, retry_record: Some(1) }
    }

    #[test]
    fn ascii_pinpad_matches_generic_libchipcard_header_only_command() {
        // Golden vector from generic_pcsc/card.xml (our timeout/display settings).
        let command = ascii_pin(6).pinpad_command();
        assert_eq!(command, [0x1e, 0x0f, 0x82, 0, 0, 8, 6, 2, 1, 4, 9, 0, 0, 0, 0,
            4, 0, 0, 0, 0, 0x20, 0, 3]);
        assert_eq!(&command[19..], &[0, 0x20, 0, 3]); // no Lc, spaces, or guessed PIN length
    }

    #[test]
    fn ascii_reader_supplied_pin_and_software_pin_have_identical_exact_length() {
        let info = ascii_pin(4);
        // Only synthetic test PINs. Model a reader appending the typed PIN to the header.
        for secret in ["1234", "123456", "12345678"] {
            let mut from_reader = info.verify_template();
            from_reader.push(secret.len() as u8); from_reader.extend_from_slice(secret.as_bytes());
            let software = info.soft_command(secret).unwrap();
            assert_eq!(*software, from_reader);
            assert_eq!(&software[5..], secret.as_bytes());
            assert_eq!(software.len(), 5 + secret.len());
        }
        assert_eq!(*ascii_pin(6).soft_command("123456").unwrap(), [0, 0x20, 0, 3, 6, b'1', b'2', b'3', b'4', b'5', b'6']);
    }

    #[test]
    fn fpin2_pinpad_and_software_encoding_keep_the_length_nibble_and_fillers() {
        let info = PinInfo { fpin2: true, ..ascii_pin(4) };
        let command = info.pinpad_command();
        assert_eq!(&command[2..7], &[0x89, 0x47, 4, 8, 4]);
        assert_eq!(&command[15..19], &[13, 0, 0, 0]);
        assert_eq!(&command[19..], &[0, 0x20, 0, 3, 8, 0x20, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(&info.soft_command("12345").unwrap()[5..], &[0x25, 0x12, 0x34, 0x5f, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(&info.soft_command("12345678").unwrap()[5..], &[0x28, 0x12, 0x34, 0x56, 0x78, 0xff, 0xff, 0xff]);
        assert!(info.soft_command("abcd").is_err());
    }

    #[test]
    fn invalid_software_pin_is_rejected_before_card_access() {
        for secret in ["", "12345", "123456789", "12ä456"] { assert!(ascii_pin(6).soft_command(secret).is_err()); }
        assert!(ascii_pin(6).soft_command("123456").is_ok());
    }

    #[test]
    fn metadata_selects_only_global_access_pin_and_its_counter_record() {
        let puk = [tlv(0x83, &[1, 2]), tlv(0x89, &[0x21, 0x60])].concat();
        let access = [tlv(0x93, &[3, 7]), tlv(0x89, &[0x21, 0x60])].concat();
        let pin = access_pin(&[(1, puk.clone()), (2, access.clone())]).unwrap();
        assert_eq!((pin.id, pin.min_len, pin.retry_record, pin.fpin2), (3, 6, Some(7), false));
        assert!(access_pin(&[(1, puk)]).is_err());
        assert!(access_pin(&[(1, access.clone()), (2, access)]).is_err());
        let local_pin = [tlv(0x83, &[0x83, 1]), tlv(0x89, &[0x21, 0x60])].concat();
        assert!(access_pin(&[(1, local_pin)]).is_err());
    }

    #[test]
    fn missing_or_unknown_pin_metadata_never_guesses_ascii_or_another_reference() {
        for record in [tlv(0x83, &[3, 1]), [tlv(0x83, &[3]), tlv(0x89, &[0x21, 0x60])].concat(),
            [tlv(0x83, &[3, 0]), tlv(0x89, &[0x21, 0x60])].concat(),
            [tlv(0x83, &[3, 1]), tlv(0x89, &[0x31, 0x60])].concat()] {
            assert!(access_pin(&[(1, record)]).is_err());
        }
        assert!(access_pin(&[]).is_err());
    }

    #[test]
    fn pin_verification_success_is_cached_and_all_failures_stop_after_one_attempt() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let mut state = PinVerification::default();
        state.run(|| { calls.set(calls.get()+1); Ok(0x9000) }).unwrap();
        state.run(|| panic!("success must not ask for PIN twice")).unwrap();
        assert_eq!(calls.get(), 1);
        for sw in [0x63c2, 0x63c1, 0x63c0, 0x6983, 0x6a88, 0x6a86, 0x6401, 0x6400] {
            let mut state = PinVerification::default();
            assert!(state.run(|| Ok(sw)).is_err());
            assert!(state.run(|| panic!("failed/cancelled PIN must not retry or toggle P2")).is_err());
            assert!(!state.verified);
        }
        let mut state = PinVerification::default();
        assert!(state.run(|| Err(err("simulated transport loss"))).is_err());
        assert!(state.run(|| panic!("uncertain outcome must not retry")).is_err());
    }

    #[test]
    fn counter_and_reader_status_parsing_distinguish_blocked_cancelled_and_remaining() {
        assert_eq!(parse_pin_retries(&[3, 2]).unwrap(), PinRetries { maximum: 3, remaining: 2 });
        assert_eq!(parse_pin_retries(&[3, 0]).unwrap().remaining, 0);
        for bytes in [&[][..], &[3], &[0, 0], &[3, 4]] { assert!(parse_pin_retries(bytes).is_err()); }
        assert_eq!(pinpad_status(&[0x90, 0]).unwrap(), 0x9000);
        for bytes in [&[][..], &[0x90], &[0, 0x90, 0]] { assert!(pinpad_status(bytes).is_err()); }
        assert!(matches!(pin_result(0x6401), Err(fints::Error::Cancelled)));
        assert!(pin_result(0x63c1).unwrap_err().to_string().contains("PIN-Versuche: 1"));
        assert!(pin_result(0x6983).unwrap_err().to_string().contains("gesperrt"));
    }

    #[test]
    fn apdu_uses_exact_1984_bit_card_key_size() {
        let hash = [0xaa; 32];
        let sign = apdu_ext(0, 0x88, 0, 0, &hash, Some(248));
        assert_eq!(&sign[..5], &[0, 0x88, 0, 0, 32]);
        assert_eq!(&sign[5..37], &hash);
        assert_eq!(&sign[37..], &[0xf8]);

        let mut block = vec![0];
        block.extend([0xab; 248]);
        let decipher = apdu_ext(0, 0x2a, 0x80, 0x86, &block, Some(248));
        assert_eq!(&decipher[..5], &[0, 0x2a, 0x80, 0x86, 249]);
        assert_eq!(&decipher[5..254], &block);
        assert_eq!(&decipher[254..], &[0xf8]);
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
